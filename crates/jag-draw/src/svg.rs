use crate::scene::ColorLinPremul;
use crate::svg_fontdb::{render_svg_to_pixmap, svg_font_db};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Pluggable provider for embedded SVG icon bytes.
///
/// Applications call [`set_builtin_svg_provider`] once at startup to
/// register a lookup function.  When the SVG renderer cannot find a
/// file on disk it falls back to this provider.
type SvgBytesProvider = fn(&Path) -> Option<&'static [u8]>;

static SVG_BYTES_PROVIDER: std::sync::OnceLock<SvgBytesProvider> = std::sync::OnceLock::new();

/// Register a function that returns embedded SVG bytes for a given path.
///
/// Call this once at application startup so that chrome icons render even
/// when the `images/` directory is not next to the binary.
pub fn set_builtin_svg_provider(provider: fn(&Path) -> Option<&'static [u8]>) {
    let _ = SVG_BYTES_PROVIDER.set(provider);
}

fn builtin_svg_bytes(path: &Path) -> Option<&'static [u8]> {
    SVG_BYTES_PROVIDER.get().and_then(|f| f(path))
}

/// Optional style overrides for SVG rendering
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvgStyle {
    /// Override fill color (replaces all fill colors in the SVG)
    pub fill: Option<ColorLinPremul>,
    /// Override stroke color (replaces all stroke colors in the SVG)
    pub stroke: Option<ColorLinPremul>,
    /// Override stroke width (replaces all stroke widths in the SVG)
    pub stroke_width: Option<f32>,
}

impl SvgStyle {
    pub fn new() -> Self {
        Self {
            fill: None,
            stroke: None,
            stroke_width: None,
        }
    }

    pub fn with_stroke(mut self, color: ColorLinPremul) -> Self {
        self.stroke = Some(color);
        self
    }

    pub fn with_fill(mut self, color: ColorLinPremul) -> Self {
        self.fill = Some(color);
        self
    }

    pub fn with_stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width);
        self
    }
}

impl Default for SvgStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash-friendly version of SvgStyle for cache keys
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SvgStyleKey {
    fill: Option<[u8; 4]>,
    stroke: Option<[u8; 4]>,
    stroke_width_bits: Option<u32>,
}

impl From<SvgStyle> for SvgStyleKey {
    fn from(style: SvgStyle) -> Self {
        Self {
            fill: style.fill.map(|c| {
                let rgba = c.to_srgba_u8();
                [rgba[0], rgba[1], rgba[2], rgba[3]]
            }),
            stroke: style.stroke.map(|c| {
                let rgba = c.to_srgba_u8();
                [rgba[0], rgba[1], rgba[2], rgba[3]]
            }),
            stroke_width_bits: style.stroke_width.map(|w| w.to_bits()),
        }
    }
}

/// Bucketed scale factor used for raster cache keys.
/// Provides more granular buckets to support icons at various sizes while maintaining cache efficiency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScaleBucket {
    X025, // 0.25x
    X05,  // 0.5x
    X075, // 0.75x
    X1,   // 1.0x
    X125, // 1.25x
    X15,  // 1.5x
    X2,   // 2.0x
    X25,  // 2.5x
    X3,   // 3.0x
    X4,   // 4.0x
    X5,   // 5.0x
    X6,   // 6.0x
    X8,   // 8.0x
}

impl ScaleBucket {
    pub fn from_scale(s: f32) -> Self {
        // Bucket to nearest scale factor
        if s < 0.375 {
            ScaleBucket::X025
        } else if s < 0.625 {
            ScaleBucket::X05
        } else if s < 0.875 {
            ScaleBucket::X075
        } else if s < 1.125 {
            ScaleBucket::X1
        } else if s < 1.375 {
            ScaleBucket::X125
        } else if s < 1.75 {
            ScaleBucket::X15
        } else if s < 2.25 {
            ScaleBucket::X2
        } else if s < 2.75 {
            ScaleBucket::X25
        } else if s < 3.5 {
            ScaleBucket::X3
        } else if s < 4.5 {
            ScaleBucket::X4
        } else if s < 5.5 {
            ScaleBucket::X5
        } else if s < 7.0 {
            ScaleBucket::X6
        } else {
            ScaleBucket::X8
        }
    }

    pub fn as_f32(self) -> f32 {
        match self {
            ScaleBucket::X025 => 0.25,
            ScaleBucket::X05 => 0.5,
            ScaleBucket::X075 => 0.75,
            ScaleBucket::X1 => 1.0,
            ScaleBucket::X125 => 1.25,
            ScaleBucket::X15 => 1.5,
            ScaleBucket::X2 => 2.0,
            ScaleBucket::X25 => 2.5,
            ScaleBucket::X3 => 3.0,
            ScaleBucket::X4 => 4.0,
            ScaleBucket::X5 => 5.0,
            ScaleBucket::X6 => 6.0,
            ScaleBucket::X8 => 8.0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct CacheKey {
    path: PathBuf,
    scale: ScaleBucket,
    style: SvgStyleKey,
}

struct CacheEntry {
    tex: std::sync::Arc<wgpu::Texture>,
    width: u32,
    height: u32,
    last_tick: u64,
    bytes: usize,
}

/// Simple SVG rasterization cache backed by usvg+resvg, with LRU eviction.
///
/// Notes:
/// - Animated SVG (SMIL/CSS/JS) is not supported; files are rasterized as-is.
/// - External resources referenced by relative hrefs are resolved from the SVG's directory.
pub struct SvgRasterCache {
    device: Arc<wgpu::Device>,
    // LRU state
    map: HashMap<CacheKey, CacheEntry>,
    lru: VecDeque<CacheKey>,
    current_tick: u64,
    // guardrails
    max_bytes: usize,
    total_bytes: usize,
    max_tex_size: u32,
}

impl SvgRasterCache {
    pub fn new(device: Arc<wgpu::Device>) -> Self {
        // Conservative default budget: 128 MiB for cached rasters
        let max_bytes = 128 * 1024 * 1024;
        let limits = device.limits();
        let max_tex_size = limits.max_texture_dimension_2d;
        Self {
            device,
            map: HashMap::new(),
            lru: VecDeque::new(),
            current_tick: 0,
            max_bytes,
            total_bytes: 0,
            max_tex_size,
        }
    }

    pub fn set_max_bytes(&mut self, bytes: usize) {
        self.max_bytes = bytes;
        self.evict_if_needed();
    }

    fn touch(&mut self, key: &CacheKey) {
        self.current_tick = self.current_tick.wrapping_add(1);
        if let Some(entry) = self.map.get_mut(key) {
            entry.last_tick = self.current_tick;
        }
        // update LRU order: move key to back
        if let Some(pos) = self.lru.iter().position(|k| k == key) {
            let k = self.lru.remove(pos).unwrap();
            self.lru.push_back(k);
        }
    }

    fn insert(&mut self, key: CacheKey, entry: CacheEntry) {
        self.current_tick = self.current_tick.wrapping_add(1);
        self.total_bytes += entry.bytes;
        self.map.insert(key.clone(), entry);
        self.lru.push_back(key);
        self.evict_if_needed();
    }

    fn evict_if_needed(&mut self) {
        while self.total_bytes > self.max_bytes {
            if let Some(old_key) = self.lru.pop_front() {
                if let Some(entry) = self.map.remove(&old_key) {
                    self.total_bytes = self.total_bytes.saturating_sub(entry.bytes);
                    // dropping `entry.tex` releases GPU memory eventually
                }
            } else {
                break;
            }
        }
    }

    /// Rasterize (or fetch from cache) an SVG file to an RGBA8 sRGB texture for a given scale.
    /// Returns a cloneable `wgpu::Texture` and its dimensions.
    /// Optional style parameter allows overriding fill, stroke, and stroke-width.
    pub fn get_or_rasterize(
        &mut self,
        path: &Path,
        scale: f32,
        style: SvgStyle,
        queue: &wgpu::Queue,
    ) -> Option<(std::sync::Arc<wgpu::Texture>, u32, u32)> {
        let scale_b = ScaleBucket::from_scale(scale);
        let style_key = SvgStyleKey::from(style);
        let key = CacheKey {
            path: path.to_path_buf(),
            scale: scale_b,
            style: style_key,
        };
        if self.map.contains_key(&key) {
            self.touch(&key);
            let e = self.map.get(&key).unwrap();
            return Some((e.tex.clone(), e.width, e.height));
        }

        // Read SVG data. Prefer the actual file on disk when it exists,
        // falling back to built-in embedded bytes for chrome icons that
        // may not have a corresponding file (e.g. bare "icon.svg" names).
        let mut data: Vec<u8> = if path.exists() {
            std::fs::read(path).ok()?
        } else if let Some(bytes) = builtin_svg_bytes(path) {
            bytes.to_vec()
        } else {
            std::fs::read(path).ok()?
        };

        // Apply style overrides by modifying the SVG XML if needed
        if style.fill.is_some() || style.stroke.is_some() || style.stroke_width.is_some() {
            data = apply_style_overrides_to_xml(&data, style)?;
        }

        // Rasterize to an RGBA pixmap. `<text>` glyphs resolve through the
        // shared SVG font database (system fonts plus any host-registered
        // fallback) so chart labels render even where system fonts are absent.
        let fonts = svg_font_db();
        let pixmap = render_svg_to_pixmap(
            &data,
            scale_b.as_f32(),
            &fonts,
            path.parent().map(|p| p.to_path_buf()),
            self.max_tex_size,
        )?;
        let w = pixmap.width();
        let h = pixmap.height();
        let rgba = pixmap.take();

        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("svg-raster"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        let bytes = (w as usize) * (h as usize) * 4;
        let tex_arc = Arc::new(tex);
        let entry = CacheEntry {
            tex: tex_arc.clone(),
            width: w,
            height: h,
            last_tick: self.current_tick,
            bytes,
        };
        self.insert(key, entry);
        Some((tex_arc, w, h))
    }
}

/// Apply style overrides by modifying the SVG XML
/// This replaces stroke="currentColor", fill colors, and stroke-width attributes
fn apply_style_overrides_to_xml(data: &[u8], style: SvgStyle) -> Option<Vec<u8>> {
    let mut svg_str = String::from_utf8(data.to_vec()).ok()?;

    // Replace stroke color
    if let Some(stroke_color) = style.stroke {
        let rgba = stroke_color.to_srgba_u8();
        let hex_color = format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]);

        // Replace stroke="currentColor" with the actual color
        svg_str = svg_str.replace(
            "stroke=\"currentColor\"",
            &format!("stroke=\"{}\"", hex_color),
        );
        svg_str = svg_str.replace("stroke='currentColor'", &format!("stroke='{}'", hex_color));
    }

    // Replace ALL fill colors with the override color.
    // Two steps: (1) set fill on the root <svg> element so paths without
    // an explicit fill attribute inherit it (SVG defaults to black),
    // and (2) replace all existing fill="..." values (except "none").
    if let Some(fill_color) = style.fill {
        let rgba = fill_color.to_srgba_u8();
        let hex_color = format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]);

        // Step 1: Inject fill on the root <svg> element for inheritance.
        // This handles paths that have no explicit fill attribute (SVG
        // defaults to black). Only inject if the <svg> tag doesn't already
        // have a fill attribute — otherwise we'd create a duplicate.
        if let Some(svg_start) = svg_str.find("<svg ") {
            let tag_end = svg_str[svg_start..].find('>').unwrap_or(svg_str.len());
            let svg_tag = &svg_str[svg_start..svg_start + tag_end];
            if !svg_tag.contains("fill=") {
                svg_str.insert_str(svg_start + 5, &format!("fill=\"{}\" ", hex_color));
            }
        }

        // Step 2: Replace all explicit fill="..." values (except "none").
        let mut result = String::new();
        let mut remaining = svg_str.as_str();

        while let Some(start) = remaining.find("fill=\"") {
            result.push_str(&remaining[..start]);
            let after_attr = &remaining[start + 6..]; // skip `fill="`
            if let Some(end_pos) = after_attr.find('"') {
                let old_val = &after_attr[..end_pos];
                if old_val == "none" {
                    result.push_str("fill=\"none\"");
                } else {
                    result.push_str(&format!("fill=\"{}\"", hex_color));
                }
                remaining = &after_attr[end_pos + 1..]; // skip past closing quote
            } else {
                result.push_str("fill=\"");
                result.push_str(after_attr);
                break;
            }
        }
        result.push_str(remaining);
        svg_str = result;
    }

    // Replace stroke-width - handle all occurrences
    if let Some(width) = style.stroke_width {
        // Replace all stroke-width attributes
        let mut result = String::new();
        let mut remaining = svg_str.as_str();

        while let Some(start) = remaining.find("stroke-width=\"") {
            // Add everything before stroke-width
            result.push_str(&remaining[..start]);
            result.push_str("stroke-width=\"");

            // Find the end quote
            let after_attr = &remaining[start + 14..];
            if let Some(end_pos) = after_attr.find('"') {
                // Add the new width value
                result.push_str(&width.to_string());
                // Continue from after the closing quote
                remaining = &after_attr[end_pos..];
            } else {
                // Malformed SVG, just copy the rest
                result.push_str(after_attr);
                break;
            }
        }
        // Add any remaining content
        result.push_str(remaining);
        svg_str = result;
    }

    Some(svg_str.into_bytes())
}
