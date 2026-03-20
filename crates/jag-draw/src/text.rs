//! Text rendering providers for jag-draw.
//!
//! The primary provider is [`JagTextProvider`] which uses:
//! - `harfrust` for text shaping (HarfBuzz implementation)
//! - `swash` for glyph rasterization
//! - `fontdb` for font discovery and fallback
//!
//! This provides high-quality text rendering with:
//! - Proper kerning and ligatures
//! - Subpixel RGB rendering
//! - BiDi support
//! - Complex script support
//!
//! # Example
//! ```no_run
//! use jag_draw::{
//!     JagTextProvider, SubpixelOrientation, TextRun, ColorLinPremul, TextProvider, FontStyle,
//! };
//!
//! let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB)
//!     .expect("Failed to load fonts");
//!
//! let run = TextRun {
//!     text: "Hello, world!".to_string(),
//!     pos: [0.0, 0.0],
//!     size: 16.0,
//!     color: ColorLinPremul::rgba(255, 255, 255, 255),
//!     weight: 400.0,
//!     style: FontStyle::Normal,
//!     family: None,
//! };
//!
//! let glyphs = provider.rasterize_run(&run);
//! ```

use std::hash::Hash;

/// LCD subpixel orientation along X axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubpixelOrientation {
    RGB,
    BGR,
}

/// Storage format for a subpixel coverage mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskFormat {
    Rgba8,
    Rgba16,
}

/// Subpixel mask in RGB coverage format stored in RGBA (A is unused).
/// Supports 8-bit or 16-bit per-channel storage.
#[derive(Clone, Debug)]
pub struct SubpixelMask {
    pub width: u32,
    pub height: u32,
    pub format: MaskFormat,
    /// Pixel data, row-major. For Rgba8, 4 bytes/pixel. For Rgba16, 8 bytes/pixel (little-endian u16s).
    pub data: Vec<u8>,
}

impl SubpixelMask {
    pub fn bytes_per_pixel(&self) -> usize {
        match self.format {
            MaskFormat::Rgba8 => 4,
            MaskFormat::Rgba16 => 8,
        }
    }
}

/// Color emoji mask in full RGBA format (premultiplied alpha).
/// Used for color emoji glyphs that have embedded color information.
#[derive(Clone, Debug)]
pub struct ColorMask {
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixel data, row-major, premultiplied alpha.
    pub data: Vec<u8>,
}

impl ColorMask {
    pub fn bytes_per_pixel(&self) -> usize {
        4
    }
}

/// Glyph mask that can be either subpixel (for text) or color (for emoji).
#[derive(Clone, Debug)]
pub enum GlyphMask {
    /// RGB subpixel coverage mask for regular text rendering
    Subpixel(SubpixelMask),
    /// Full RGBA color mask for color emoji
    Color(ColorMask),
}

impl GlyphMask {
    pub fn width(&self) -> u32 {
        match self {
            GlyphMask::Subpixel(m) => m.width,
            GlyphMask::Color(m) => m.width,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            GlyphMask::Subpixel(m) => m.height,
            GlyphMask::Color(m) => m.height,
        }
    }

    pub fn is_color(&self) -> bool {
        matches!(self, GlyphMask::Color(_))
    }
}

/// GPU-ready batch of glyph masks with positions and color.
/// This is the canonical representation used when sending text to the GPU.
#[derive(Clone, Debug)]
pub struct GlyphBatch {
    pub glyphs: Vec<(SubpixelMask, [f32; 2], crate::scene::ColorLinPremul)>,
}

impl GlyphBatch {
    pub fn new() -> Self {
        Self { glyphs: Vec::new() }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            glyphs: Vec::with_capacity(cap),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.glyphs.len()
    }
}

/// Simple global cache for glyph runs keyed by (text, size, weight, style,
/// family, provider pointer). Used by direct text rendering paths (e.g.,
/// jag-surface Canvas) to avoid re-shaping and re-rasterizing identical
/// text on every frame.
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct GlyphRunKey {
    text_hash: u64,
    size_bits: u32,
    weight_bits: u32,
    style_bits: u8,
    family_hash: u64,
    provider_id: usize,
}

struct GlyphRunCache {
    map: std::sync::Mutex<
        std::collections::HashMap<GlyphRunKey, std::sync::Arc<Vec<RasterizedGlyph>>>,
    >,
    max_entries: usize,
}

impl GlyphRunCache {
    fn new(max_entries: usize) -> Self {
        Self {
            map: std::sync::Mutex::new(std::collections::HashMap::new()),
            max_entries: max_entries.max(1),
        }
    }

    fn get(&self, key: &GlyphRunKey) -> Option<std::sync::Arc<Vec<RasterizedGlyph>>> {
        let map = self.map.lock().unwrap();
        map.get(key).cloned()
    }

    fn insert(
        &self,
        key: GlyphRunKey,
        glyphs: Vec<RasterizedGlyph>,
    ) -> std::sync::Arc<Vec<RasterizedGlyph>> {
        let mut map = self.map.lock().unwrap();

        // Simple eviction strategy to keep memory bounded:
        // when we grow past 2x capacity with new keys, clear everything.
        if map.len() >= self.max_entries * 2 && !map.contains_key(&key) {
            map.clear();
        }

        if let Some(existing) = map.get(&key) {
            return existing.clone();
        }

        let arc = std::sync::Arc::new(glyphs);
        map.insert(key, arc.clone());
        arc
    }

    fn clear(&self) {
        self.map.lock().unwrap().clear();
    }
}

static GLYPH_RUN_CACHE: std::sync::OnceLock<GlyphRunCache> = std::sync::OnceLock::new();

fn global_glyph_run_cache() -> &'static GlyphRunCache {
    GLYPH_RUN_CACHE.get_or_init(|| GlyphRunCache::new(2048))
}

/// Invalidate all cached glyph rasterizations.
///
/// Must be called after web fonts are registered so that text re-renders
/// using the newly available font faces instead of stale cached bitmaps.
pub fn invalidate_glyph_run_cache() {
    global_glyph_run_cache().clear();
}

/// Convert an 8-bit grayscale coverage mask to an RGB subpixel mask.
/// Uses a gentle subpixel shift for improved clarity on small text.
pub fn grayscale_to_subpixel_rgb(
    width: u32,
    height: u32,
    gray: &[u8],
    orientation: SubpixelOrientation,
) -> SubpixelMask {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(gray.len(), w * h);
    let mut out = vec![0u8; w * h * 4];

    // Gentle subpixel rendering: slight horizontal shift per channel
    // Much lighter than the original 3-tap kernel to avoid blurring
    for y in 0..h {
        for x in 0..w {
            let c0 = gray[y * w + x] as f32 / 255.0;
            let cl = if x > 0 {
                gray[y * w + (x - 1)] as f32 / 255.0
            } else {
                c0
            };
            let cr = if x + 1 < w {
                gray[y * w + (x + 1)] as f32 / 255.0
            } else {
                c0
            };

            // Very light blending (10% neighbor influence instead of 33%)
            let sample_left = 0.9 * c0 + 0.1 * cl;
            let sample_center = c0;
            let sample_right = 0.9 * c0 + 0.1 * cr;

            let (r_cov, g_cov, b_cov) = match orientation {
                SubpixelOrientation::RGB => (sample_left, sample_center, sample_right),
                SubpixelOrientation::BGR => (sample_right, sample_center, sample_left),
            };

            let i = (y * w + x) * 4;
            out[i + 0] = (r_cov * 255.0 + 0.5) as u8;
            out[i + 1] = (g_cov * 255.0 + 0.5) as u8;
            out[i + 2] = (b_cov * 255.0 + 0.5) as u8;
            out[i + 3] = 0u8; // alpha unused; output premul alpha computed in shader
        }
    }
    SubpixelMask {
        width,
        height,
        format: MaskFormat::Rgba8,
        data: out,
    }
}

/// Convert an 8-bit grayscale coverage mask to an RGB mask with equal channels (grayscale AA).
pub fn grayscale_to_rgb_equal(width: u32, height: u32, gray: &[u8]) -> SubpixelMask {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(gray.len(), w * h);
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let g = gray[y * w + x];
            let i = (y * w + x) * 4;
            out[i + 0] = g;
            out[i + 1] = g;
            out[i + 2] = g;
            out[i + 3] = 0u8;
        }
    }
    SubpixelMask {
        width,
        height,
        format: MaskFormat::Rgba8,
        data: out,
    }
}

/// 16-bit variants for higher precision masks. Channels are u16 in [0..65535],
/// packed little-endian into the data buffer. Alpha is unused.
pub fn grayscale_to_subpixel_rgb16(
    width: u32,
    height: u32,
    gray: &[u8],
    orientation: SubpixelOrientation,
) -> SubpixelMask {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(gray.len(), w * h);
    let mut out = vec![0u8; w * h * 8];
    for y in 0..h {
        for x in 0..w {
            let c0 = gray[y * w + x] as f32 / 255.0;
            let cl = if x > 0 {
                gray[y * w + (x - 1)] as f32 / 255.0
            } else {
                c0
            };
            let cr = if x + 1 < w {
                gray[y * w + (x + 1)] as f32 / 255.0
            } else {
                c0
            };
            let sample_left = (2.0 / 3.0) * c0 + (1.0 / 3.0) * cl;
            let sample_center = c0;
            let sample_right = (2.0 / 3.0) * c0 + (1.0 / 3.0) * cr;
            let (r_cov, g_cov, b_cov) = match orientation {
                SubpixelOrientation::RGB => (sample_left, sample_center, sample_right),
                SubpixelOrientation::BGR => (sample_right, sample_center, sample_left),
            };
            let (r, g, b) = match orientation {
                SubpixelOrientation::RGB => (r_cov, g_cov, b_cov),
                SubpixelOrientation::BGR => (b_cov, g_cov, r_cov),
            };
            let i = (y * w + x) * 8;
            let write_u16 = |buf: &mut [u8], idx: usize, v: u16| {
                let b = v.to_le_bytes();
                buf[idx] = b[0];
                buf[idx + 1] = b[1];
            };
            write_u16(&mut out, i + 0, (r * 65535.0 + 0.5) as u16);
            write_u16(&mut out, i + 2, (g * 65535.0 + 0.5) as u16);
            write_u16(&mut out, i + 4, (b * 65535.0 + 0.5) as u16);
            write_u16(&mut out, i + 6, 0u16);
        }
    }
    SubpixelMask {
        width,
        height,
        format: MaskFormat::Rgba16,
        data: out,
    }
}

pub fn grayscale_to_rgb_equal16(width: u32, height: u32, gray: &[u8]) -> SubpixelMask {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(gray.len(), w * h);
    let mut out = vec![0u8; w * h * 8];
    for y in 0..h {
        for x in 0..w {
            let g = (gray[y * w + x] as u16) * 257; // 255->65535 scale
            let i = (y * w + x) * 8;
            let b = g.to_le_bytes();
            out[i + 0] = b[0];
            out[i + 1] = b[1];
            out[i + 2] = b[0];
            out[i + 3] = b[1];
            out[i + 4] = b[0];
            out[i + 5] = b[1];
            out[i + 6] = 0;
            out[i + 7] = 0;
        }
    }
    SubpixelMask {
        width,
        height,
        format: MaskFormat::Rgba16,
        data: out,
    }
}

// Optional provider that consumes a patched fontdue fork emitting RGB masks directly.
// Behind a feature flag so it doesn't affect default builds.
#[cfg(feature = "fontdue-rgb-patch")]
pub struct PatchedFontdueProvider {
    font: fontdue_rgb::Font,
}

#[cfg(feature = "fontdue-rgb-patch")]
impl PatchedFontdueProvider {
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let font = fontdue_rgb::Font::from_bytes(bytes, fontdue_rgb::FontSettings::default())?;
        Ok(Self { font })
    }
}

#[cfg(feature = "fontdue-rgb-patch")]
impl TextProvider for PatchedFontdueProvider {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph> {
        use fontdue_rgb::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: 0.0,
            ..LayoutSettings::default()
        });
        layout.append(
            &[&self.font],
            &TextStyle::new(&run.text, run.size.max(1.0), 0),
        );
        let mut out = Vec::new();
        for g in layout.glyphs() {
            // Patched fontdue returns RGB masks directly (u8 or u16). Prefer 16-bit when available.
            let mask = if let Some((w, h, data16)) = self
                .font
                .rasterize_rgb16_indexed(g.key.glyph_index, g.key.px)
            {
                GlyphMask::Subpixel(SubpixelMask {
                    width: w as u32,
                    height: h as u32,
                    format: MaskFormat::Rgba16,
                    data: data16,
                })
            } else {
                let (w, h, data8) = self
                    .font
                    .rasterize_rgb8_indexed(g.key.glyph_index, g.key.px);
                GlyphMask::Subpixel(SubpixelMask {
                    width: w as u32,
                    height: h as u32,
                    format: MaskFormat::Rgba8,
                    data: data8,
                })
            };
            out.push(RasterizedGlyph {
                offset: [g.x, g.y],
                mask,
            });
        }
        out
    }
}

/// A glyph with its top-left offset relative to the run origin and a mask (subpixel or color).
#[derive(Clone, Debug)]
pub struct RasterizedGlyph {
    pub offset: [f32; 2],
    pub mask: GlyphMask,
}

/// Minimal shaped glyph information for paragraph-level wrapping.
#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    /// Glyph's starting UTF-8 byte index in the source text (Harfbuzz cluster).
    pub cluster: u32,
    /// Advance width in pixels.
    pub x_advance: f32,
}

/// Shaped paragraph representation for efficient wrapping.
#[derive(Clone, Debug)]
pub struct ShapedParagraph {
    pub glyphs: Vec<ShapedGlyph>,
}

/// Text provider interface. Implementations convert a `TextRun` into positioned glyph masks.
pub trait TextProvider: Send + Sync {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph>;

    /// Optional paragraph shaping hook for advanced wrappers.
    ///
    /// Implementors that can expose shaping results should
    /// return glyphs with cluster indices and advances. The default implementation
    /// returns `None`, in which case callers must fall back to approximate methods.
    fn shape_paragraph(&self, _text: &str, _px: f32) -> Option<ShapedParagraph> {
        None
    }

    /// Optional cache tag to distinguish providers in text caches.
    /// The default implementation returns 0, which is sufficient when
    /// a single provider is used with a given PassManager.
    fn cache_tag(&self) -> u64 {
        0
    }

    fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        let _ = px;
        None
    }

    /// Measure the total advance width of a styled text run (in the same pixel
    /// units as `run.size`).  The default delegates to `shape_paragraph`,
    /// ignoring weight/style/family.  Providers that support multiple font faces
    /// should override this to select the correct face.
    fn measure_run(&self, run: &crate::scene::TextRun) -> f32 {
        if let Some(shaped) = self.shape_paragraph(&run.text, run.size) {
            shaped
                .glyphs
                .iter()
                .map(|g| g.x_advance)
                .sum::<f32>()
                .max(0.0)
        } else {
            run.text.chars().count() as f32 * run.size * 0.55
        }
    }

    /// Register a web font from raw TTF/OTF bytes.
    /// Returns `Ok(true)` if newly registered, `Ok(false)` if already present.
    /// Default implementation returns `Ok(false)` (no-op).
    fn register_web_font(
        &self,
        _family: &str,
        _data: Vec<u8>,
        _weight: u16,
        _style: crate::scene::FontStyle,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// Rasterize a text run using a global glyph-run cache.
///
/// This is intended for direct text rendering paths that repeatedly render the
/// same text (e.g., during scrolling) and want to avoid re-shaping and
/// re-rasterizing glyphs every frame. The cache key is based on:
/// - text contents
/// - run size in pixels
/// - the concrete text provider instance
pub fn rasterize_run_cached(
    provider: &dyn TextProvider,
    run: &crate::scene::TextRun,
) -> std::sync::Arc<Vec<RasterizedGlyph>> {
    use crate::scene::FontStyle as SceneFontStyle;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut hasher = DefaultHasher::new();
    run.text.hash(&mut hasher);
    let text_hash = hasher.finish();

    // Encode style and family so that bold/italic/monospace runs don't
    // collide in the cache with regular text of the same contents.
    let style_bits: u8 = match run.style {
        SceneFontStyle::Normal => 0,
        SceneFontStyle::Italic => 1,
        SceneFontStyle::Oblique => 2,
    };

    let family_hash: u64 = if let Some(ref family) = run.family {
        let mut fh = DefaultHasher::new();
        family.hash(&mut fh);
        fh.finish()
    } else {
        0
    };
    let size_bits = run.size.to_bits();
    let weight_bits = run.weight.to_bits();
    // Use the concrete provider data pointer as a stable identifier for this run.
    let provider_id = (provider as *const dyn TextProvider as *const ()) as usize;
    let key = GlyphRunKey {
        text_hash,
        size_bits,
        weight_bits,
        style_bits,
        family_hash,
        provider_id,
    };

    let cache = global_glyph_run_cache();
    if let Some(hit) = cache.get(&key) {
        return hit;
    }

    let glyphs = provider.rasterize_run(run);
    cache.insert(key, glyphs)
}

/// LEGACY: Simple fontdue-based provider.
///
/// **NOT RECOMMENDED**: Use [`JagTextProvider`] (harfrust + swash) instead.
/// This provider is kept for compatibility and testing purposes only.
///
/// Limitations:
/// - Basic ASCII-first layout
/// - No advanced shaping features
/// - Lower quality than swash rasterization
pub struct SimpleFontdueProvider {
    font: fontdue::Font,
    orientation: SubpixelOrientation,
}

impl SimpleFontdueProvider {
    pub fn from_bytes(bytes: &[u8], orientation: SubpixelOrientation) -> anyhow::Result<Self> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Self { font, orientation })
    }
}

impl TextProvider for SimpleFontdueProvider {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph> {
        use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: 0.0,
            ..LayoutSettings::default()
        });
        layout.append(
            &[&self.font],
            &TextStyle::new(&run.text, run.size.max(1.0), 0),
        );

        let mut out = Vec::new();
        for g in layout.glyphs() {
            // Rasterize individual glyph to grayscale
            let (metrics, bitmap) = self.font.rasterize_indexed(g.key.glyph_index, g.key.px);
            if metrics.width == 0 || metrics.height == 0 {
                continue;
            }
            // Convert to subpixel mask
            let mask = GlyphMask::Subpixel(grayscale_to_subpixel_rgb(
                metrics.width as u32,
                metrics.height as u32,
                &bitmap,
                self.orientation,
            ));
            // Layout already provides the glyph's top-left (x, y) in pixel space for the
            // chosen CoordinateSystem. Using those directly avoids double-applying the
            // font bearing which would incorrectly shift glyphs vertically (clipping
            // descenders). We keep offsets relative to the run's origin; PassManager
            // snaps the run once using line metrics.
            let ox = g.x;
            let oy = g.y;
            out.push(RasterizedGlyph {
                offset: [ox, oy],
                mask,
            });
        }
        out
    }
    fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        self.font.horizontal_line_metrics(px).map(|lm| {
            let ascent = lm.ascent;
            // Fontdue typically reports descent as a negative number; normalize to positive magnitude.
            let descent = lm.descent.abs();
            let line_gap = lm.line_gap.max(0.0);
            LineMetrics {
                ascent,
                descent,
                line_gap,
            }
        })
    }
}

/// LEGACY: Grayscale fontdue provider.
///
/// **NOT RECOMMENDED**: Use [`JagTextProvider`] (harfrust + swash) instead.
/// This provider is kept for compatibility and testing purposes only.
///
/// Replicates grayscale coverage to RGB channels equally (no subpixel rendering).
pub struct GrayscaleFontdueProvider {
    font: fontdue::Font,
}

impl GrayscaleFontdueProvider {
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Self { font })
    }
}

impl TextProvider for GrayscaleFontdueProvider {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph> {
        use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: 0.0,
            ..LayoutSettings::default()
        });
        layout.append(
            &[&self.font],
            &TextStyle::new(&run.text, run.size.max(1.0), 0),
        );
        let mut out = Vec::new();
        for g in layout.glyphs() {
            let (metrics, bitmap) = self.font.rasterize_indexed(g.key.glyph_index, g.key.px);
            if metrics.width == 0 || metrics.height == 0 {
                continue;
            }
            let mask = GlyphMask::Subpixel(grayscale_to_rgb_equal(
                metrics.width as u32,
                metrics.height as u32,
                &bitmap,
            ));
            // See note above: use layout-provided top-left directly.
            let ox = g.x;
            let oy = g.y;
            out.push(RasterizedGlyph {
                offset: [ox, oy],
                mask,
            });
        }
        out
    }
    fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        self.font.horizontal_line_metrics(px).map(|lm| {
            let ascent = lm.ascent;
            let descent = lm.descent.abs();
            let line_gap = lm.line_gap.max(0.0);
            LineMetrics {
                ascent,
                descent,
                line_gap,
            }
        })
    }
}

/// Simplified line metrics
#[derive(Clone, Copy, Debug, Default)]
pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
}

// ---------------------------------------------------------------------------
// CSS font-family parser
// ---------------------------------------------------------------------------

/// A single candidate in a parsed CSS font-family stack.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FontFamilyCandidate {
    /// A specific font name, e.g. `"Georgia"`.
    Name(String),
    /// A CSS generic family keyword.
    Generic(GenericFamily),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    SystemUi,
    Cursive,
    Fantasy,
}

/// Parse a CSS font-family value into an ordered list of candidates.
///
/// Handles quoted names (`"Times New Roman"`, `'Georgia'`), unquoted names,
/// generic keywords (`serif`, `sans-serif`, `monospace`, `system-ui`,
/// `cursive`, `fantasy`), and browser aliases (`-apple-system`,
/// `BlinkMacSystemFont`).
fn parse_font_family_stack(css_value: &str) -> Vec<FontFamilyCandidate> {
    let mut result = Vec::new();
    for part in css_value.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Strip surrounding quotes
        let name = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        };
        let lower = name.to_ascii_lowercase();
        let candidate = match lower.as_str() {
            "serif" | "ui-serif" => FontFamilyCandidate::Generic(GenericFamily::Serif),
            "sans-serif" | "ui-sans-serif" => {
                FontFamilyCandidate::Generic(GenericFamily::SansSerif)
            }
            "monospace" | "ui-monospace" => FontFamilyCandidate::Generic(GenericFamily::Monospace),
            "system-ui" | "-apple-system" | "blinkmacswissfont" | "blinkmacsystemfont" => {
                FontFamilyCandidate::Generic(GenericFamily::SystemUi)
            }
            "cursive" => FontFamilyCandidate::Generic(GenericFamily::Cursive),
            "fantasy" => FontFamilyCandidate::Generic(GenericFamily::Fantasy),
            _ => FontFamilyCandidate::Name(name.to_string()),
        };
        result.push(candidate);
    }
    result
}

// ---------------------------------------------------------------------------
// Cached font set for a resolved family
// ---------------------------------------------------------------------------

/// Font faces for a single resolved font family (regular + weight/style variants).
#[derive(Clone)]
struct CachedFontSet {
    /// Upright (non-italic) faces keyed by CSS font-weight.
    upright_faces: Vec<(u16, jag_text::FontFace)>,
    /// Italic/oblique faces keyed by CSS font-weight.
    italic_faces: Vec<(u16, jag_text::FontFace)>,
}

// ---------------------------------------------------------------------------
// JagTextProvider
// ---------------------------------------------------------------------------

/// Text provider backed by jag-text (HarfBuzz) for shaping and swash for rasterization.
///
/// This uses a primary `jag-text` `FontFace` for text and an optional emoji font
/// for color emoji fallback. Delegates shaping to `TextShaper::shape_ltr`, then
/// rasterizes glyphs via swash bitmap images.
///
/// Supports CSS font-family stack resolution: when a `TextRun` specifies a
/// `family` string (e.g. `"Georgia, 'Times New Roman', serif"`), the provider
/// parses the stack and resolves candidates against the system font database,
/// caching loaded fonts for subsequent requests.
pub struct JagTextProvider {
    /// Primary (regular) text font.
    font: jag_text::FontFace,
    /// Optional bold/semibold face for heavier weights.
    bold_font: Option<jag_text::FontFace>,
    /// Optional italic face for slanted text.
    italic_font: Option<jag_text::FontFace>,
    /// Optional monospace font for code spans.
    mono_font: Option<jag_text::FontFace>,
    /// Optional emoji font for fallback when primary font lacks emoji glyphs.
    emoji_font: Option<jag_text::FontFace>,
    orientation: SubpixelOrientation,
    /// System font database kept alive for on-demand font resolution.
    /// `None` when the provider was constructed from raw bytes.
    font_db: Option<fontdb::Database>,
    /// Cache of resolved font families, keyed by lowercase family name or
    /// generic family keyword. Protected by a `Mutex` because the
    /// `TextProvider` trait takes `&self`.
    font_cache: std::sync::Mutex<std::collections::HashMap<String, CachedFontSet>>,
}

impl JagTextProvider {
    pub fn from_bytes(bytes: &[u8], orientation: SubpixelOrientation) -> anyhow::Result<Self> {
        let font = jag_text::FontFace::from_vec(bytes.to_vec(), 0)?;
        Ok(Self {
            font,
            bold_font: None,
            italic_font: None,
            mono_font: None,
            emoji_font: None,
            orientation,
            font_db: None,
            font_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Create a provider with a primary font and an emoji font for fallback.
    pub fn from_bytes_with_emoji(
        bytes: &[u8],
        emoji_bytes: &[u8],
        orientation: SubpixelOrientation,
    ) -> anyhow::Result<Self> {
        let font = jag_text::FontFace::from_vec(bytes.to_vec(), 0)?;
        let emoji_font = jag_text::FontFace::from_vec(emoji_bytes.to_vec(), 0)?;
        Ok(Self {
            font,
            bold_font: None,
            italic_font: None,
            mono_font: None,
            emoji_font: Some(emoji_font),
            orientation,
            font_db: None,
            font_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Construct from a reasonable system sans-serif font using `fontdb`.
    /// Also attempts to load a system emoji font for color emoji fallback.
    pub fn from_system_fonts(orientation: SubpixelOrientation) -> anyhow::Result<Self> {
        use fontdb::{Database, Family, Query, Source, Stretch, Style, Weight};

        let mut db = Database::new();
        db.load_system_fonts();

        // macOS: Apple's SF system font (SFNS.ttf) is hidden from standard
        // font enumeration but is readable on disk. Load it explicitly so
        // `from_system_fonts` matches Chrome's default sans-serif.
        #[cfg(target_os = "macos")]
        {
            let sfns_path = std::path::Path::new("/System/Library/Fonts/SFNS.ttf");
            if sfns_path.exists() {
                db.load_font_file(sfns_path).ok();
            }
            let sfns_italic = std::path::Path::new("/System/Library/Fonts/SFNSItalic.ttf");
            if sfns_italic.exists() {
                db.load_font_file(sfns_italic).ok();
            }
            let sfns_mono = std::path::Path::new("/System/Library/Fonts/SFNSMono.ttf");
            if sfns_mono.exists() {
                db.load_font_file(sfns_mono).ok();
            }
            let sfns_mono_italic = std::path::Path::new("/System/Library/Fonts/SFNSMonoItalic.ttf");
            if sfns_mono_italic.exists() {
                db.load_font_file(sfns_mono_italic).ok();
            }
        }

        // Load primary text font (regular weight)
        let id = db
            .query(&Query {
                families: &[
                    // macOS system UI font (SFNS.ttf registers as "System Font"
                    // or ".SFNS-Regular" in fontdb after explicit loading).
                    Family::Name("System Font".into()),
                    Family::Name(".AppleSystemUIFont".into()),
                    Family::Name("SF Pro Text".into()),
                    Family::Name(".SF NS Text".into()),
                    // Windows system UI font
                    Family::Name("Segoe UI".into()),
                    // Generic fallbacks
                    Family::SansSerif,
                    Family::Name("Arial".into()),
                    Family::Name("Helvetica Neue".into()),
                ],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
                ..Query::default()
            })
            .ok_or_else(|| anyhow::anyhow!("no suitable system font found for jag-text"))?;

        let face = db
            .face(id)
            .ok_or_else(|| anyhow::anyhow!("fontdb face missing for system font id"))?;

        let bytes: Vec<u8> = match &face.source {
            Source::File(path) => std::fs::read(path)?,
            Source::Binary(data) => data.as_ref().as_ref().to_vec(),
            Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
        };

        let font = jag_text::FontFace::from_vec(bytes, face.index as usize)?;

        // Try to load a matching bold face from the same family (if available).
        let primary_family = face.families.first().map(|(name, _lang)| name.clone());
        let bold_font = primary_family
            .as_deref()
            .and_then(|family_name| {
                db.query(&Query {
                    families: &[Family::Name(family_name)],
                    weight: Weight::BOLD,
                    stretch: Stretch::Normal,
                    style: Style::Normal,
                    ..Query::default()
                })
            })
            .and_then(|bold_id| db.face(bold_id))
            .and_then(|bold_face| {
                let bytes: Vec<u8> = match &bold_face.source {
                    Source::File(path) => std::fs::read(path).ok()?,
                    Source::Binary(data) => Some(data.as_ref().as_ref().to_vec())?,
                    Source::SharedFile(_, data) => Some(data.as_ref().as_ref().to_vec())?,
                };
                jag_text::FontFace::from_vec(bytes, bold_face.index as usize).ok()
            });

        // Try to load a matching italic face from the same family (if available).
        let italic_font = primary_family
            .as_deref()
            .and_then(|family_name| {
                db.query(&Query {
                    families: &[Family::Name(family_name)],
                    weight: Weight::NORMAL,
                    stretch: Stretch::Normal,
                    style: Style::Italic,
                    ..Query::default()
                })
            })
            .and_then(|italic_id| db.face(italic_id))
            .and_then(|italic_face| {
                let bytes: Vec<u8> = match &italic_face.source {
                    Source::File(path) => std::fs::read(path).ok()?,
                    Source::Binary(data) => Some(data.as_ref().as_ref().to_vec())?,
                    Source::SharedFile(_, data) => Some(data.as_ref().as_ref().to_vec())?,
                };
                jag_text::FontFace::from_vec(bytes, italic_face.index as usize).ok()
            });

        // Try to load a monospace font for code spans
        let mono_font = db
            .query(&Query {
                families: &[
                    // macOS
                    Family::Name("SF Mono".into()),
                    Family::Name(".SF NS Mono".into()),
                    Family::Name("Menlo".into()),
                    Family::Name("Monaco".into()),
                    // Windows
                    Family::Name("Cascadia Code".into()),
                    Family::Name("Consolas".into()),
                    // Linux
                    Family::Name("DejaVu Sans Mono".into()),
                    Family::Name("Liberation Mono".into()),
                    Family::Monospace,
                ],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
                ..Query::default()
            })
            .and_then(|mono_id| db.face(mono_id))
            .and_then(|mono_face| {
                let bytes: Vec<u8> = match &mono_face.source {
                    Source::File(path) => std::fs::read(path).ok()?,
                    Source::Binary(data) => Some(data.as_ref().as_ref().to_vec())?,
                    Source::SharedFile(_, data) => Some(data.as_ref().as_ref().to_vec())?,
                };
                jag_text::FontFace::from_vec(bytes, mono_face.index as usize).ok()
            });

        // Try to load emoji font for fallback
        let emoji_font = db
            .query(&Query {
                families: &[
                    // macOS
                    Family::Name("Apple Color Emoji".into()),
                    // Windows
                    Family::Name("Segoe UI Emoji".into()),
                    // Linux
                    Family::Name("Noto Color Emoji".into()),
                ],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
                ..Query::default()
            })
            .and_then(|emoji_id| {
                let emoji_face = db.face(emoji_id)?;
                let emoji_bytes: Vec<u8> = match &emoji_face.source {
                    Source::File(path) => std::fs::read(path).ok()?,
                    Source::Binary(data) => Some(data.as_ref().as_ref().to_vec())?,
                    Source::SharedFile(_, data) => Some(data.as_ref().as_ref().to_vec())?,
                };
                jag_text::FontFace::from_vec(emoji_bytes, emoji_face.index as usize).ok()
            });

        Ok(Self {
            font,
            bold_font,
            italic_font,
            mono_font,
            emoji_font,
            orientation,
            font_db: Some(db),
            font_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Load a `FontFace` from a `fontdb` face entry.
    fn load_face_from_db(face: &fontdb::FaceInfo) -> Option<jag_text::FontFace> {
        use fontdb::Source;
        let bytes: Vec<u8> = match &face.source {
            Source::File(path) => std::fs::read(path).ok()?,
            Source::Binary(data) => data.as_ref().as_ref().to_vec(),
            Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
        };
        jag_text::FontFace::from_vec(bytes, face.index as usize).ok()
    }

    /// Register a web font from raw TTF/OTF bytes.
    ///
    /// **Idempotent**: returns `Ok(false)` if a font with the same
    /// family + weight + style is already registered.
    ///
    /// **Thread-safe**: font_cache is Mutex-protected.
    ///
    /// The `data` must be raw TTF or OTF — WOFF/WOFF2 must be decompressed
    /// before calling this method.
    pub fn register_web_font(
        &self,
        family: &str,
        data: Vec<u8>,
        weight: u16,
        style: crate::scene::FontStyle,
    ) -> anyhow::Result<bool> {
        let cache_key = family.to_lowercase();
        let is_italic = matches!(
            style,
            crate::scene::FontStyle::Italic | crate::scene::FontStyle::Oblique
        );

        // Idempotent fast-path checks before parsing font bytes.
        {
            let cache = self.font_cache.lock().unwrap();
            if let Some(set) = cache.get(&cache_key) {
                let faces = if is_italic {
                    &set.italic_faces
                } else {
                    &set.upright_faces
                };
                if faces.iter().any(|(w, _)| *w == weight) {
                    return Ok(false);
                }
            }
        }

        // Validate and create FontFace from raw TTF/OTF bytes
        let face = jag_text::FontFace::from_vec(data, 0)
            .map_err(|e| anyhow::anyhow!("invalid font data for '{}': {}", family, e))?;

        // Insert into our font cache
        let mut cache = self.font_cache.lock().unwrap();
        if let Some(set) = cache.get_mut(&cache_key) {
            let faces = if is_italic {
                &mut set.italic_faces
            } else {
                &mut set.upright_faces
            };
            Self::insert_weighted_face(faces, weight, face);
        } else {
            let mut set = CachedFontSet {
                upright_faces: Vec::new(),
                italic_faces: Vec::new(),
            };
            if is_italic {
                set.italic_faces.push((weight, face));
            } else {
                set.upright_faces.push((weight, face));
            }
            cache.insert(cache_key, set);
        }

        // Flush the global glyph rasterization cache so the next frame
        // re-rasterizes text using the newly registered web font face
        // instead of returning stale bitmaps from the old fallback font.
        invalidate_glyph_run_cache();

        Ok(true)
    }

    /// Resolve a single font family candidate against the `fontdb` database,
    /// loading regular + bold + italic variants. Returns `None` if the family
    /// is not installed.
    fn resolve_family(
        db: &fontdb::Database,
        candidate: &FontFamilyCandidate,
    ) -> Option<CachedFontSet> {
        use fontdb::{Family, Query, Stretch, Style, Weight};

        let families: Vec<Family<'_>> = match candidate {
            FontFamilyCandidate::Name(name) => vec![Family::Name(name.as_str())],
            FontFamilyCandidate::Generic(g) => match g {
                GenericFamily::Serif => vec![
                    Family::Serif,
                    Family::Name("Georgia"),
                    Family::Name("Times New Roman"),
                    Family::Name("Times"),
                ],
                GenericFamily::SansSerif => vec![
                    Family::Name("System Font"),
                    Family::Name("SF Pro Text"),
                    Family::Name("Segoe UI"),
                    Family::SansSerif,
                    Family::Name("Arial"),
                    Family::Name("Helvetica Neue"),
                ],
                GenericFamily::Monospace => vec![
                    Family::Name("SF Mono"),
                    Family::Name(".SF NS Mono"),
                    Family::Name("Menlo"),
                    Family::Name("Monaco"),
                    Family::Name("Cascadia Code"),
                    Family::Name("Consolas"),
                    Family::Name("DejaVu Sans Mono"),
                    Family::Monospace,
                ],
                GenericFamily::SystemUi => vec![
                    // macOS system font (SFNS.ttf loaded explicitly).
                    Family::Name("System Font"),
                    // Prefer platform UI faces first so `system-ui` /
                    // `-apple-system` metrics match browser text wrapping.
                    Family::Name("Segoe UI"),
                    Family::Name("system-ui"),
                    Family::Name("-apple-system"),
                    Family::Name("BlinkMacSystemFont"),
                    Family::SansSerif,
                    Family::Name("SF Pro Text"),
                    Family::Name("SF Pro Display"),
                    Family::Name(".SF NS Text"),
                    Family::Name(".SF NS Display"),
                    Family::Name(".AppleSystemUIFont"),
                    Family::Name("Arial"),
                    Family::Name("Helvetica Neue"),
                ],
                GenericFamily::Cursive => vec![
                    Family::Cursive,
                    Family::Name("Snell Roundhand"),
                    Family::Name("Comic Sans MS"),
                ],
                GenericFamily::Fantasy => vec![
                    Family::Fantasy,
                    Family::Name("Papyrus"),
                    Family::Name("Impact"),
                ],
            },
        };

        let regular_id = db.query(&Query {
            families: &families,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        })?;

        let regular_face = db.face(regular_id)?;
        let regular = Self::load_face_from_db(regular_face)?;

        if std::env::var("JAG_TEXT_DEBUG_FAMILY").is_ok() {
            let resolved = regular_face
                .families
                .first()
                .map(|(name, _)| name.as_str())
                .unwrap_or("<unknown>");
            eprintln!("[TEXT] resolve_family {:?} -> {}", candidate, resolved);
        }

        // Resolve bold + italic from the same resolved family name.
        let resolved_family = regular_face.families.first().map(|(name, _)| name.as_str());

        let bold = resolved_family.and_then(|fam| {
            let id = db.query(&Query {
                families: &[Family::Name(fam)],
                weight: Weight::BOLD,
                stretch: Stretch::Normal,
                style: Style::Normal,
            })?;
            Self::load_face_from_db(db.face(id)?)
        });
        let italic = resolved_family.and_then(|fam| {
            let id = db.query(&Query {
                families: &[Family::Name(fam)],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Italic,
            })?;
            Self::load_face_from_db(db.face(id)?)
        });

        let mut set = CachedFontSet {
            upright_faces: vec![(400, regular)],
            italic_faces: Vec::new(),
        };
        if let Some(bold_face) = bold {
            Self::insert_weighted_face(&mut set.upright_faces, 700, bold_face);
        }
        if let Some(italic_face) = italic {
            Self::insert_weighted_face(&mut set.italic_faces, 400, italic_face);
        }

        Some(set)
    }

    /// Cache key for a font family candidate (lowercase for case-insensitive matching).
    fn cache_key_for(candidate: &FontFamilyCandidate) -> String {
        match candidate {
            FontFamilyCandidate::Name(n) => n.to_ascii_lowercase(),
            FontFamilyCandidate::Generic(g) => match g {
                GenericFamily::Serif => "__generic_serif__".to_string(),
                GenericFamily::SansSerif => "__generic_sans-serif__".to_string(),
                GenericFamily::Monospace => "__generic_monospace__".to_string(),
                GenericFamily::SystemUi => "__generic_system-ui__".to_string(),
                GenericFamily::Cursive => "__generic_cursive__".to_string(),
                GenericFamily::Fantasy => "__generic_fantasy__".to_string(),
            },
        }
    }

    fn insert_weighted_face(
        faces: &mut Vec<(u16, jag_text::FontFace)>,
        weight: u16,
        face: jag_text::FontFace,
    ) {
        if let Some(pos) = faces.iter().position(|(w, _)| *w == weight) {
            faces[pos] = (weight, face);
        } else {
            faces.push((weight, face));
        }
        faces.sort_by_key(|(w, _)| *w);
    }

    fn pick_closest_weighted_face(
        faces: &[(u16, jag_text::FontFace)],
        requested_weight: u16,
    ) -> Option<jag_text::FontFace> {
        let mut best: Option<(u16, &jag_text::FontFace)> = None;
        for (weight, face) in faces {
            match best {
                None => best = Some((*weight, face)),
                Some((best_weight, _)) => {
                    let best_dist = (i32::from(best_weight) - i32::from(requested_weight)).abs();
                    let new_dist = (i32::from(*weight) - i32::from(requested_weight)).abs();
                    if new_dist < best_dist || (new_dist == best_dist && *weight > best_weight) {
                        best = Some((*weight, face));
                    }
                }
            }
        }
        best.map(|(_, face)| face.clone())
    }

    /// Select the appropriate font face based on a `TextRun`'s family, weight,
    /// and style.
    ///
    /// When the run specifies a `family` string, parses it as a CSS font-family
    /// stack and walks the candidates in order, resolving each against the system
    /// font database. Resolved fonts are cached for subsequent requests.
    ///
    /// Returns a *cloned* `FontFace` (cheap — inner data is `Arc`).
    fn select_face(&self, run: &crate::scene::TextRun) -> jag_text::FontFace {
        use crate::scene::FontStyle as SceneFontStyle;

        let requested_weight = run.weight.clamp(100.0, 900.0).round() as u16;
        let is_bold = requested_weight >= 600;
        let is_italic = matches!(run.style, SceneFontStyle::Italic | SceneFontStyle::Oblique);

        // --- Try to resolve from the font-family string, if present. ---
        if let Some(ref family_str) = run.family {
            let candidates = parse_font_family_stack(family_str);

            if let Some(db) = &self.font_db {
                for candidate in &candidates {
                    let key = Self::cache_key_for(candidate);

                    // Check the cache first (short lock).
                    {
                        let cache = self.font_cache.lock().unwrap();
                        if let Some(set) = cache.get(&key) {
                            if let Some(face) = Self::pick_variant(set, requested_weight, is_italic)
                            {
                                return face;
                            }
                        }
                    }

                    // Cache miss — resolve from fontdb.
                    if let Some(set) = Self::resolve_family(db, candidate) {
                        let face = Self::pick_variant(&set, requested_weight, is_italic)
                            .unwrap_or_else(|| self.font.clone());
                        self.font_cache.lock().unwrap().insert(key, set);
                        return face;
                    }
                }
            }

            // If no fontdb or no candidates matched, check for the legacy
            // "monospace" shorthand before falling through to defaults.
            if family_str.eq_ignore_ascii_case("monospace") {
                if let Some(ref mono) = self.mono_font {
                    return mono.clone();
                }
            }
        }

        // --- Fallback to the pre-loaded defaults. ---
        if is_italic {
            if let Some(ref italic) = self.italic_font {
                return italic.clone();
            }
        }
        if is_bold {
            if let Some(ref bold) = self.bold_font {
                return bold.clone();
            }
        }
        self.font.clone()
    }

    /// Build font variation axis settings for variable fonts.
    ///
    /// Always sets `wght` (weight) and `opsz` (optical size). For static
    /// fonts these are silently ignored by both HarfBuzz and Swash.
    /// The `opsz` axis is critical for SF Pro (macOS system font) — it
    /// thickens strokes at small sizes for readability, matching Chrome/Safari.
    ///
    /// Returns `(4-byte ASCII tag, value)` pairs usable by both harfrust
    /// (via `Tag::new`) and swash (via string conversion).
    fn build_variations(weight: f32, size_px: f32) -> Vec<([u8; 4], f32)> {
        vec![
            (*b"wght", weight),
            // Optical size: clamp to the SFNS axis range (17–96).
            // Using font size in px directly is the CSS convention.
            (*b"opsz", size_px.clamp(17.0, 96.0)),
        ]
    }

    /// Pick the best weight/style variant from a cached font set.
    fn pick_variant(
        set: &CachedFontSet,
        requested_weight: u16,
        italic: bool,
    ) -> Option<jag_text::FontFace> {
        if std::env::var("JAG_TEXT_DEBUG_FAMILY").is_ok() {
            eprintln!(
                "[TEXT] pick_variant weight={} italic={} upright={} italic_faces={}",
                requested_weight,
                italic,
                set.upright_faces.len(),
                set.italic_faces.len()
            );
        }

        if italic {
            if let Some(face) =
                Self::pick_closest_weighted_face(&set.italic_faces, requested_weight)
            {
                return Some(face);
            }
        }

        if let Some(face) = Self::pick_closest_weighted_face(&set.upright_faces, requested_weight) {
            return Some(face);
        }

        if let Some(face) = Self::pick_closest_weighted_face(&set.italic_faces, requested_weight) {
            return Some(face);
        }

        None
    }

    /// Layout a paragraph using jag-text's `TextLayout` with optional width-based wrapping.
    ///
    /// This exposes jag-text's multi-line layout (including per-line baselines) so that
    /// callers can build GPU-ready glyph batches without relying on `PassManager`
    /// baseline heuristics.
    pub fn layout_paragraph(
        &self,
        text: &str,
        size_px: f32,
        max_width: Option<f32>,
    ) -> jag_text::layout::TextLayout {
        use jag_text::layout::{TextLayout, WrapMode};

        let wrap = if max_width.is_some() {
            WrapMode::BreakWord
        } else {
            WrapMode::NoWrap
        };

        TextLayout::with_wrap(
            text.to_string(),
            &self.font,
            size_px.max(1.0),
            max_width,
            wrap,
        )
    }
}

impl TextProvider for JagTextProvider {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph> {
        use jag_text::shaping::TextShaper;
        use swash::FontRef;
        use swash::scale::image::Content;
        use swash::scale::{Render, ScaleContext, Source, StrikeWith};

        let size = run.size.max(1.0);
        let face = self.select_face(run);

        // Build variation settings for variable fonts (wght + opsz).
        let requested_weight = run.weight.clamp(100.0, 900.0);
        let raw_variations = Self::build_variations(requested_weight, size);

        // Shape the entire run with HarfBuzz so that advances include kerning,
        // ligatures, and contextual alternates. Pass variations so variable
        // fonts produce correctly-weighted, optically-sized glyph outlines.
        let shaped = {
            use jag_text::shaping::hb_tag_from_bytes;
            let hb_vars: Vec<_> = raw_variations
                .iter()
                .map(|(tag, val)| (hb_tag_from_bytes(tag), *val))
                .collect();
            TextShaper::shape_ltr_with_variations(
                &run.text,
                0..run.text.len(),
                &face,
                0,
                size,
                &hb_vars,
            )
        };

        // Build swash resources for rasterisation of shaped glyph IDs.
        let font_bytes = face.as_bytes();
        let font_ref = FontRef::from_index(&font_bytes, face.index())
            .expect("jag-text FontFace bytes should be a valid swash FontRef");

        // Compute normalized coordinates for all variation axes so the scaler
        // renders outlines at the correct weight and optical size.
        // swash Setting<f32> implements From<&([u8; 4], f32)>, so we
        // can pass a reference to our raw_variations slice directly.
        let norm_coords: Vec<swash::NormalizedCoord> = if raw_variations.is_empty() {
            Vec::new()
        } else {
            font_ref
                .variations()
                .normalized_coords(&raw_variations)
                .collect()
        };

        let mut ctx = ScaleContext::new();
        let mut builder = ctx.builder(font_ref).size(size).hint(true);
        if !norm_coords.is_empty() {
            builder = builder.normalized_coords(&norm_coords);
        }
        let mut scaler = builder.build();
        let renderer = Render::new(&[
            Source::Outline,
            Source::Bitmap(StrikeWith::BestFit),
            Source::ColorBitmap(StrikeWith::BestFit),
        ]);

        // Get emoji font bytes if available (we'll create FontRef per-use due to lifetime constraints)
        let emoji_bytes = self.emoji_font.as_ref().map(|f| f.as_bytes());

        let mut out = Vec::new();
        let mut pen_x: f32 = 0.0;

        for idx in 0..shaped.glyphs.len() {
            let glyph_id = shaped.glyphs[idx];
            let advance = shaped.advances[idx];

            if glyph_id == 0 {
                // .notdef — try emoji fallback using the source character at this cluster
                let cluster_byte = shaped.clusters[idx] as usize;
                let emoji_rendered = emoji_bytes.as_ref().and_then(|eb| {
                    let ch = run.text[cluster_byte..].chars().next()?;
                    let emoji_font_ref = FontRef::from_index(eb, 0)?;
                    let emoji_gid = emoji_font_ref.charmap().map(ch);
                    if emoji_gid == 0 {
                        return None;
                    }
                    let mut emoji_ctx = ScaleContext::new();
                    let mut emoji_scaler = emoji_ctx
                        .builder(emoji_font_ref)
                        .size(size)
                        .hint(false)
                        .build();
                    let emoji_renderer = Render::new(&[
                        Source::ColorOutline(0),
                        Source::ColorBitmap(StrikeWith::BestFit),
                        Source::Bitmap(StrikeWith::BestFit),
                        Source::Outline,
                    ]);
                    let img = emoji_renderer.render(&mut emoji_scaler, emoji_gid)?;
                    let w = img.placement.width;
                    let h = img.placement.height;
                    if w == 0 || h == 0 {
                        return None;
                    }
                    let mask = match img.content {
                        Content::Mask => GlyphMask::Subpixel(grayscale_to_subpixel_rgb(
                            w,
                            h,
                            &img.data,
                            self.orientation,
                        )),
                        Content::SubpixelMask => GlyphMask::Subpixel(SubpixelMask {
                            width: w,
                            height: h,
                            format: MaskFormat::Rgba8,
                            data: img.data.clone(),
                        }),
                        Content::Color => GlyphMask::Color(ColorMask {
                            width: w,
                            height: h,
                            data: img.data.clone(),
                        }),
                    };
                    let ox = pen_x + img.placement.left as f32;
                    let oy = -img.placement.top as f32;
                    out.push(RasterizedGlyph {
                        offset: [ox, oy],
                        mask,
                    });
                    Some(w as f32)
                });

                if let Some(emoji_width) = emoji_rendered {
                    pen_x += emoji_width;
                } else {
                    // No emoji fallback — skip with approximate advance
                    pen_x += size * 0.5;
                }
                continue;
            }

            // Rasterize from primary font using the HarfBuzz-produced glyph ID.
            if let Some(img) = renderer.render(&mut scaler, glyph_id) {
                let w = img.placement.width;
                let h = img.placement.height;
                if w > 0 && h > 0 {
                    let mask = match img.content {
                        Content::Mask => GlyphMask::Subpixel(grayscale_to_subpixel_rgb(
                            w,
                            h,
                            &img.data,
                            self.orientation,
                        )),
                        Content::SubpixelMask => GlyphMask::Subpixel(SubpixelMask {
                            width: w,
                            height: h,
                            format: MaskFormat::Rgba8,
                            data: img.data.clone(),
                        }),
                        Content::Color => GlyphMask::Color(ColorMask {
                            width: w,
                            height: h,
                            data: img.data.clone(),
                        }),
                    };

                    let ox = pen_x + img.placement.left as f32;
                    let oy = -img.placement.top as f32;
                    out.push(RasterizedGlyph {
                        offset: [ox, oy],
                        mask,
                    });
                }
            }

            // Advance by the HarfBuzz-computed advance (includes kerning)
            pen_x += advance;
        }

        out
    }

    fn shape_paragraph(&self, text: &str, size_px: f32) -> Option<ShapedParagraph> {
        // Use jag-text layout to compute glyph advances; this matches the
        // shaping used for cursor movement and selection and avoids width
        // drift when centering text.
        let layout = self.layout_paragraph(text, size_px, None);
        let mut glyphs = Vec::new();
        for line in layout.lines() {
            for run in &line.runs {
                for (idx, adv) in run.advances.iter().enumerate() {
                    glyphs.push(ShapedGlyph {
                        cluster: run.clusters.get(idx).copied().unwrap_or(0),
                        x_advance: *adv,
                    });
                }
            }
        }
        Some(ShapedParagraph { glyphs })
    }

    fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        let m = self.font.scaled_metrics(px.max(1.0));
        Some(LineMetrics {
            ascent: m.ascent,
            descent: m.descent,
            line_gap: m.line_gap,
        })
    }

    fn measure_run(&self, run: &crate::scene::TextRun) -> f32 {
        use jag_text::shaping::TextShaper;

        let size = run.size.max(1.0);
        let face = self.select_face(run);

        // Shape with HarfBuzz — returns the same advances used by rasterize_run,
        // so measurement and rendering always agree.
        let requested_weight = run.weight.clamp(100.0, 900.0);
        let raw_variations = Self::build_variations(requested_weight, size);
        let shaped = {
            use jag_text::shaping::hb_tag_from_bytes;
            let hb_vars: Vec<_> = raw_variations
                .iter()
                .map(|(tag, val)| (hb_tag_from_bytes(tag), *val))
                .collect();
            TextShaper::shape_ltr_with_variations(
                &run.text,
                0..run.text.len(),
                &face,
                0,
                size,
                &hb_vars,
            )
        };
        if std::env::var("JAG_TEXT_DEBUG_FAMILY").is_ok()
            && (run.text.contains("Z-Ordering")
                || run.text.contains("Hit Testing")
                || run.text.contains("Depth Buffer")
                || run.text.contains(" System Fonts")
                || run.text.contains(" Opacity")
                || run.text.contains(" Text Runs")
                || run.text.contains(" Inline Block"))
        {
            eprintln!(
                "[TEXT] measure_run text={:?} size={} weight={} width={}",
                run.text, run.size, run.weight, shaped.width
            );
        }
        shaped.width
    }

    fn register_web_font(
        &self,
        family: &str,
        data: Vec<u8>,
        weight: u16,
        style: crate::scene::FontStyle,
    ) -> anyhow::Result<bool> {
        // Delegate to the concrete JagTextProvider implementation.
        JagTextProvider::register_web_font(self, family, data, weight, style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_font_family() {
        let result = parse_font_family_stack("Georgia");
        assert_eq!(result, vec![FontFamilyCandidate::Name("Georgia".into())]);
    }

    #[test]
    fn parse_font_stack_with_generic() {
        let result = parse_font_family_stack("Georgia, \"Times New Roman\", Times, serif");
        assert_eq!(
            result,
            vec![
                FontFamilyCandidate::Name("Georgia".into()),
                FontFamilyCandidate::Name("Times New Roman".into()),
                FontFamilyCandidate::Name("Times".into()),
                FontFamilyCandidate::Generic(GenericFamily::Serif),
            ]
        );
    }

    #[test]
    fn parse_sans_serif_stack() {
        let result = parse_font_family_stack(
            "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
        );
        assert_eq!(
            result,
            vec![
                FontFamilyCandidate::Generic(GenericFamily::SystemUi),
                FontFamilyCandidate::Generic(GenericFamily::SystemUi),
                FontFamilyCandidate::Name("Segoe UI".into()),
                FontFamilyCandidate::Name("Roboto".into()),
                FontFamilyCandidate::Generic(GenericFamily::SansSerif),
            ]
        );
    }

    #[test]
    fn parse_monospace_stack() {
        let result = parse_font_family_stack("'SF Mono', ui-monospace, monospace");
        assert_eq!(
            result,
            vec![
                FontFamilyCandidate::Name("SF Mono".into()),
                FontFamilyCandidate::Generic(GenericFamily::Monospace),
                FontFamilyCandidate::Generic(GenericFamily::Monospace),
            ]
        );
    }

    #[test]
    fn parse_empty_and_whitespace() {
        assert!(parse_font_family_stack("").is_empty());
        assert!(parse_font_family_stack("  ,  , ").is_empty());
    }

    #[test]
    fn generic_families_case_insensitive() {
        let result = parse_font_family_stack("SERIF, Sans-Serif, MONOSPACE");
        assert_eq!(
            result,
            vec![
                FontFamilyCandidate::Generic(GenericFamily::Serif),
                FontFamilyCandidate::Generic(GenericFamily::SansSerif),
                FontFamilyCandidate::Generic(GenericFamily::Monospace),
            ]
        );
    }

    #[test]
    fn cache_key_case_insensitive() {
        let k1 = JagTextProvider::cache_key_for(&FontFamilyCandidate::Name("Georgia".into()));
        let k2 = JagTextProvider::cache_key_for(&FontFamilyCandidate::Name("georgia".into()));
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_generic_distinct() {
        let serif =
            JagTextProvider::cache_key_for(&FontFamilyCandidate::Generic(GenericFamily::Serif));
        let sans =
            JagTextProvider::cache_key_for(&FontFamilyCandidate::Generic(GenericFamily::SansSerif));
        assert_ne!(serif, sans);
    }

    #[test]
    fn system_provider_selects_distinct_face_for_monospace_family() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB);
        if provider.is_err() {
            // Skip on systems without fonts (CI containers)
            return;
        }
        let provider = provider.unwrap();

        let base_run = crate::scene::TextRun {
            text: "font-family".to_string(),
            pos: [0.0, 0.0],
            size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(255, 255, 255, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: Some(
                "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, sans-serif".to_string(),
            ),
        };
        let mono_run = crate::scene::TextRun {
            family: Some("\"SF Mono\", \"Fira Code\", \"Cascadia Code\", monospace".to_string()),
            ..base_run.clone()
        };

        let default_face = provider.select_face(&base_run);
        let mono_face = provider.select_face(&mono_run);

        assert!(
            default_face.index() != mono_face.index()
                || default_face.as_bytes() != mono_face.as_bytes(),
            "monospace family should resolve to a different face than the default sans stack"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_provider_resolves_hidden_sf_mono_for_explicit_family() {
        let sf_mono_path = std::path::Path::new("/System/Library/Fonts/SFNSMono.ttf");
        if !sf_mono_path.exists() {
            return;
        }

        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB);
        if provider.is_err() {
            return;
        }
        let provider = provider.unwrap();

        let run = crate::scene::TextRun {
            text: "mono".to_string(),
            pos: [0.0, 0.0],
            size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(255, 255, 255, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: Some("\"SF Mono\", monospace".to_string()),
        };

        let selected = provider.select_face(&run);
        let expected = std::fs::read(sf_mono_path).expect("read SFNSMono.ttf");

        assert_eq!(
            selected.as_bytes().as_ref(),
            expected.as_slice(),
            "explicit SF Mono family should resolve to the hidden SFNSMono font on macOS"
        );
    }

    #[test]
    fn register_web_font_invalid_data_fails() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB);
        if provider.is_err() {
            // Skip on systems without fonts (CI containers)
            return;
        }
        let provider = provider.unwrap();

        // Invalid font data should return error
        let result = provider.register_web_font(
            "TestFont",
            vec![0, 0, 0, 0],
            400,
            crate::scene::FontStyle::Normal,
        );
        assert!(result.is_err(), "Invalid font data should return error");
    }
}
