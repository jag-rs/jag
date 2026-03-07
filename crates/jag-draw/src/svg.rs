use crate::scene::ColorLinPremul;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Shared system font database for SVG text rendering.
///
/// Loaded once on first use so that `<text>` elements in SVGs can be
/// resolved to glyph outlines by usvg/resvg. Without fonts the text
/// is silently dropped.
///
/// Uses `usvg::fontdb` (re-exported from usvg) to match the version
/// expected by `usvg::Options::fontdb`.
static SYSTEM_FONTDB: std::sync::LazyLock<Arc<usvg::fontdb::Database>> =
    std::sync::LazyLock::new(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    });

/// Return embedded bytes for built-in SVG icons that ship with the
/// Detir workspace (toolbar, sidebar, dock, chat, etc.).
///
/// This allows the core chrome icons to render even when the `images/`
/// directory is not present next to the binary (for example, when the
/// Detir browser is launched from another project like WAID).
fn builtin_svg_bytes(path: &Path) -> Option<&'static [u8]> {
    let name = path.file_name()?.to_str()?;
    match name {
        "arrow-left.svg" => Some(include_bytes!("../../../images/arrow-left.svg")),
        "arrow-right.svg" => Some(include_bytes!("../../../images/arrow-right.svg")),
        "bookmark.svg" => Some(include_bytes!("../../../images/bookmark.svg")),
        "calendar.svg" => Some(include_bytes!("../../../images/calendar.svg")),
        "check.svg" => Some(include_bytes!("../../../images/check.svg")),
        "check_white.svg" => Some(include_bytes!("../../../images/check_white.svg")),
        "chevron-down.svg" => Some(include_bytes!("../../../images/chevron-down.svg")),
        "chevron-left.svg" => Some(include_bytes!("../../../images/chevron-left.svg")),
        "chevron-right.svg" => Some(include_bytes!("../../../images/chevron-right.svg")),
        "chevron-up.svg" => Some(include_bytes!("../../../images/chevron-up.svg")),
        "circle-x.svg" => Some(include_bytes!("../../../images/circle-x.svg")),
        "clock.svg" => Some(include_bytes!("../../../images/clock.svg")),
        "file.svg" => Some(include_bytes!("../../../images/file.svg")),
        "home.svg" => Some(include_bytes!("../../../images/home.svg")),
        "image.svg" => Some(include_bytes!("../../../images/image.svg")),
        "inspection-panel.svg" => Some(include_bytes!("../../../images/inspection-panel.svg")),
        "landscape.svg" => Some(include_bytes!("../../../images/landscape.svg")),
        "layout-grid.svg" => Some(include_bytes!("../../../images/layout-grid.svg")),
        "loader.svg" => Some(include_bytes!("../../../images/loader.svg")),
        "message-circle-more.svg" => {
            Some(include_bytes!("../../../images/message-circle-more.svg"))
        }
        "message-circle.svg" => Some(include_bytes!("../../../images/message-circle.svg")),
        "panel-left-white.svg" => Some(include_bytes!("../../../images/panel-left-white.svg")),
        "panel-left.svg" => Some(include_bytes!("../../../images/panel-left.svg")),
        "pause-white.svg" => Some(include_bytes!("../../../images/pause-white.svg")),
        "pause.svg" => Some(include_bytes!("../../../images/pause.svg")),
        "play-white.svg" => Some(include_bytes!("../../../images/play-white.svg")),
        "play.svg" => Some(include_bytes!("../../../images/play.svg")),
        "plus.svg" => Some(include_bytes!("../../../images/plus.svg")),
        "refresh.svg" => Some(include_bytes!("../../../images/refresh.svg")),
        "search.svg" => Some(include_bytes!("../../../images/search.svg")),
        "send.svg" => Some(include_bytes!("../../../images/send.svg")),
        "settings.svg" => Some(include_bytes!("../../../images/settings.svg")),
        "share.svg" => Some(include_bytes!("../../../images/share.svg")),
        "square-mouse-pointer.svg" => {
            Some(include_bytes!("../../../images/square-mouse-pointer.svg"))
        }
        "square-terminal.svg" => Some(include_bytes!("../../../images/square-terminal.svg")),
        "square-white.svg" => Some(include_bytes!("../../../images/square-white.svg")),
        "square.svg" => Some(include_bytes!("../../../images/square.svg")),
        "tabs.svg" => Some(include_bytes!("../../../images/tabs.svg")),
        "upload.svg" => Some(include_bytes!("../../../images/upload.svg")),
        "view.svg" => Some(include_bytes!("../../../images/view.svg")),
        "volume-x-white.svg" => Some(include_bytes!("../../../images/volume-x-white.svg")),
        "volume-x.svg" => Some(include_bytes!("../../../images/volume-x.svg")),
        "weather-animated-icons.svg" => {
            Some(include_bytes!("../../../images/weather-animated-icons.svg"))
        }
        "x.svg" => Some(include_bytes!("../../../images/x.svg")),
        "folder-open.svg" => Some(include_bytes!("../../../images/folder-open.svg")),
        "new-file.svg" => Some(include_bytes!("../../../images/new-file.svg")),
        "new-folder.svg" => Some(include_bytes!("../../../images/new-folder.svg")),
        // Programming language / file-type icons (devicon.dev)
        "file-generic.svg" => Some(include_bytes!("../../../images/lang/file-generic.svg")),
        "bash.svg" => Some(include_bytes!("../../../images/lang/bash.svg")),
        "c.svg" => Some(include_bytes!("../../../images/lang/c.svg")),
        "cplusplus.svg" => Some(include_bytes!("../../../images/lang/cplusplus.svg")),
        "csharp.svg" => Some(include_bytes!("../../../images/lang/csharp.svg")),
        "css.svg" => Some(include_bytes!("../../../images/lang/css.svg")),
        "docker.svg" => Some(include_bytes!("../../../images/lang/docker.svg")),
        "elixir.svg" => Some(include_bytes!("../../../images/lang/elixir.svg")),
        "git.svg" => Some(include_bytes!("../../../images/lang/git.svg")),
        "go.svg" => Some(include_bytes!("../../../images/lang/go.svg")),
        "haskell.svg" => Some(include_bytes!("../../../images/lang/haskell.svg")),
        "html.svg" => Some(include_bytes!("../../../images/lang/html.svg")),
        "java.svg" => Some(include_bytes!("../../../images/lang/java.svg")),
        "javascript.svg" => Some(include_bytes!("../../../images/lang/javascript.svg")),
        "json.svg" => Some(include_bytes!("../../../images/lang/json.svg")),
        "kotlin.svg" => Some(include_bytes!("../../../images/lang/kotlin.svg")),
        "lock.svg" => Some(include_bytes!("../../../images/lang/lock.svg")),
        "lua.svg" => Some(include_bytes!("../../../images/lang/lua.svg")),
        "markdown.svg" => Some(include_bytes!("../../../images/lang/markdown.svg")),
        "php.svg" => Some(include_bytes!("../../../images/lang/php.svg")),
        "python.svg" => Some(include_bytes!("../../../images/lang/python.svg")),
        "r.svg" => Some(include_bytes!("../../../images/lang/r.svg")),
        "react.svg" => Some(include_bytes!("../../../images/lang/react.svg")),
        "ruby.svg" => Some(include_bytes!("../../../images/lang/ruby.svg")),
        "rust.svg" => Some(include_bytes!("../../../images/lang/rust.svg")),
        "scala.svg" => Some(include_bytes!("../../../images/lang/scala.svg")),
        "swift.svg" => Some(include_bytes!("../../../images/lang/swift.svg")),
        "toml.svg" => Some(include_bytes!("../../../images/lang/toml.svg")),
        "typescript.svg" => Some(include_bytes!("../../../images/lang/typescript.svg")),
        "yaml.svg" => Some(include_bytes!("../../../images/lang/yaml.svg")),
        "zig.svg" => Some(include_bytes!("../../../images/lang/zig.svg")),
        _ => None,
    }
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

        let mut opt = usvg::Options::default();
        opt.resources_dir = path.parent().map(|p| p.to_path_buf());
        opt.fontdb = SYSTEM_FONTDB.clone();
        let tree = usvg::Tree::from_data(&data, &opt).ok()?;
        let size = tree.size().to_int_size();
        let (w0, h0): (u32, u32) = (size.width().max(1), size.height().max(1));
        let s = scale_b.as_f32();
        let w = ((w0 as f32) * s).round() as u32;
        let h = ((h0 as f32) * s).round() as u32;
        if w == 0 || h == 0 {
            return None;
        }
        if w > self.max_tex_size || h > self.max_tex_size {
            return None;
        }

        let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
        let mut pm = pixmap.as_mut();
        let ts = tiny_skia::Transform::from_scale(s, s);
        resvg::render(&tree, ts, &mut pm);

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

// --- SVG → Geometry import (Phase 7.5.2) ---

/// Import result counters for basic visibility/debugging.
#[derive(Clone, Copy, Debug, Default)]
pub struct SvgImportStats {
    pub rects: u32,
    pub rounded_rects: u32,
    pub ellipses: u32,
    pub paths: u32,
    pub strokes: u32,
    pub skipped: u32,
}

fn color_from_usvg(color: usvg::Color, opacity: f32) -> crate::scene::ColorLinPremul {
    crate::scene::ColorLinPremul::from_srgba(color.red, color.green, color.blue, opacity)
}

fn transform2d_from_usvg(t: usvg::Transform) -> crate::scene::Transform2D {
    // tiny_skia_path::Transform uses fields (sx, kx, ky, sy, tx, ty)
    crate::scene::Transform2D {
        m: [
            t.sx as f32,
            t.ky as f32,
            t.kx as f32,
            t.sy as f32,
            t.tx as f32,
            t.ty as f32,
        ],
    }
}

fn fill_rule_from_usvg(rule: usvg::FillRule) -> crate::scene::FillRule {
    match rule {
        usvg::FillRule::NonZero => crate::scene::FillRule::NonZero,
        usvg::FillRule::EvenOdd => crate::scene::FillRule::EvenOdd,
    }
}

// Note: usvg outputs only Path/Image/Text/Group nodes; basic shapes are already converted to paths.

fn import_path_fill(
    painter: &mut crate::painter::Painter,
    node_transform: usvg::Transform,
    p: &usvg::Path,
    color: crate::scene::ColorLinPremul,
    stats: &mut SvgImportStats,
) {
    use crate::scene::{Path, PathCmd};
    let mut cmds: Vec<PathCmd> = Vec::new();
    // Convert usvg path data → our PathCmd. This covers move/line/quad/cubic/close.
    for seg in p.data().segments() {
        use usvg::tiny_skia_path::PathSegment;
        match seg {
            PathSegment::MoveTo(pt) => cmds.push(PathCmd::MoveTo([pt.x as f32, pt.y as f32])),
            PathSegment::LineTo(pt) => cmds.push(PathCmd::LineTo([pt.x as f32, pt.y as f32])),
            PathSegment::QuadTo(c, p) => cmds.push(PathCmd::QuadTo(
                [c.x as f32, c.y as f32],
                [p.x as f32, p.y as f32],
            )),
            PathSegment::CubicTo(c1, c2, p) => cmds.push(PathCmd::CubicTo(
                [c1.x as f32, c1.y as f32],
                [c2.x as f32, c2.y as f32],
                [p.x as f32, p.y as f32],
            )),
            PathSegment::Close => cmds.push(PathCmd::Close),
        }
    }
    let fill_rule = p
        .fill()
        .map(|f| fill_rule_from_usvg(f.rule()))
        .unwrap_or(crate::scene::FillRule::NonZero);
    let path = Path { cmds, fill_rule };
    let t = transform2d_from_usvg(node_transform);
    painter.push_transform(t);
    painter.fill_path(path, color, 0);
    painter.pop_transform();
    stats.paths += 1;
}

/// If the given usvg path is an axis-aligned rectangle made of straight
/// line segments (MoveTo + 3x LineTo + Close), return it as a Rect in
/// local coordinates. Rounded corners and curves are not considered a match.
fn detect_axis_aligned_rect(p: &usvg::Path) -> Option<crate::scene::Rect> {
    use usvg::tiny_skia_path::PathSegment;
    // Collect the first closed subpath consisting only of MoveTo/LineTo/Close
    let mut points: Vec<[f32; 2]> = Vec::new();
    let mut started = false;
    for seg in p.data().segments() {
        match seg {
            PathSegment::MoveTo(pt) => {
                if started {
                    break;
                } // Only consider first subpath
                started = true;
                points.clear();
                points.push([pt.x as f32, pt.y as f32]);
            }
            PathSegment::LineTo(pt) => {
                if !started {
                    return None;
                }
                let q = [pt.x as f32, pt.y as f32];
                // Skip exact duplicates
                if points
                    .last()
                    .map_or(true, |last| last[0] != q[0] || last[1] != q[1])
                {
                    points.push(q);
                }
            }
            PathSegment::QuadTo(..) | PathSegment::CubicTo(..) => {
                // Curves present → not a simple rect
                return None;
            }
            PathSegment::Close => {
                break;
            }
        }
    }
    if points.len() != 4 {
        return None;
    }
    // Verify axis alignment: each edge must be horizontal or vertical
    for i in 0..4 {
        let a = points[i];
        let b = points[(i + 1) % 4];
        let dx = (a[0] - b[0]).abs();
        let dy = (a[1] - b[1]).abs();
        if dx > 1e-4 && dy > 1e-4 {
            return None;
        }
    }
    // Build rect from min/max
    let mut minx = f32::INFINITY;
    let mut miny = f32::INFINITY;
    let mut maxx = f32::NEG_INFINITY;
    let mut maxy = f32::NEG_INFINITY;
    for p in &points {
        minx = minx.min(p[0]);
        miny = miny.min(p[1]);
        maxx = maxx.max(p[0]);
        maxy = maxy.max(p[1]);
    }
    let w = (maxx - minx).abs();
    let h = (maxy - miny).abs();
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some(crate::scene::Rect {
        x: minx.min(maxx),
        y: miny.min(maxy),
        w,
        h,
    })
}

fn paint_from_fill(fill: &usvg::Fill) -> Option<crate::scene::Brush> {
    match fill.paint() {
        usvg::Paint::Color(c) => Some(crate::scene::Brush::Solid(color_from_usvg(
            *c,
            fill.opacity().get() as f32,
        ))),
        _ => None,
    }
}

/// Import an SVG file into the display list as vector geometry.
///
/// Notes:
/// - Supports Rect/RoundedRect/Circle/Ellipse and basic filled Paths.
/// - Only solid fills are mapped. Unsupported paints/filters/masks/text are skipped.
pub fn import_svg_geometry_to_painter(
    painter: &mut crate::painter::Painter,
    path: &Path,
) -> Option<SvgImportStats> {
    let data = std::fs::read(path).ok()?;
    let mut opt = usvg::Options::default();
    opt.resources_dir = path.parent().map(|p| p.to_path_buf());
    opt.fontdb = SYSTEM_FONTDB.clone();
    let tree = usvg::Tree::from_data(&data, &opt).ok()?;
    let mut stats = SvgImportStats::default();

    // Traverse the tree in document order; apply node-local transforms only for now.
    fn walk(
        group: &usvg::Group,
        painter: &mut crate::painter::Painter,
        stats: &mut SvgImportStats,
    ) {
        for node in group.children() {
            match node {
                usvg::Node::Path(p) => {
                    if let Some(fill) = p.fill() {
                        if let Some(crate::scene::Brush::Solid(col)) = paint_from_fill(fill) {
                            // Try fast-path: detect simple axis-aligned rectangle and emit as a primitive
                            if let Some(rect) = detect_axis_aligned_rect(p) {
                                let t = transform2d_from_usvg(p.abs_transform());
                                painter.push_transform(t);
                                painter.rect(rect, crate::scene::Brush::Solid(col), 0);
                                painter.pop_transform();
                                stats.rects += 1;
                            } else {
                                import_path_fill(painter, p.abs_transform(), p, col, stats);
                            }
                        } else {
                            // Unsupported paint servers (gradients/patterns) are skipped for geometry import.
                            stats.skipped += 1;
                        }
                    }
                    // Stroke (solid-only for now)
                    if let Some(st) = p.stroke() {
                        if let usvg::Paint::Color(c) = st.paint() {
                            let col = color_from_usvg(*c, st.opacity().get() as f32);
                            // If the path is a simple rect, stroke it via the rect stroke primitive
                            if let Some(rect) = detect_axis_aligned_rect(p) {
                                let t = transform2d_from_usvg(p.abs_transform());
                                painter.push_transform(t);
                                painter.stroke_rect(
                                    rect,
                                    crate::scene::Stroke {
                                        width: st.width().get() as f32,
                                    },
                                    crate::scene::Brush::Solid(col),
                                    0,
                                );
                                painter.pop_transform();
                                stats.strokes += 1;
                            } else {
                                // Build a Path copy from usvg data for stroke as well
                                use crate::scene::{Path as EPath, PathCmd};
                                let mut cmds: Vec<PathCmd> = Vec::new();
                                for seg in p.data().segments() {
                                    use usvg::tiny_skia_path::PathSegment;
                                    match seg {
                                        PathSegment::MoveTo(pt) => {
                                            cmds.push(PathCmd::MoveTo([pt.x as f32, pt.y as f32]))
                                        }
                                        PathSegment::LineTo(pt) => {
                                            cmds.push(PathCmd::LineTo([pt.x as f32, pt.y as f32]))
                                        }
                                        PathSegment::QuadTo(c, q) => cmds.push(PathCmd::QuadTo(
                                            [c.x as f32, c.y as f32],
                                            [q.x as f32, q.y as f32],
                                        )),
                                        PathSegment::CubicTo(c1, c2, q) => {
                                            cmds.push(PathCmd::CubicTo(
                                                [c1.x as f32, c1.y as f32],
                                                [c2.x as f32, c2.y as f32],
                                                [q.x as f32, q.y as f32],
                                            ))
                                        }
                                        PathSegment::Close => cmds.push(PathCmd::Close),
                                    }
                                }
                                let epath = EPath {
                                    cmds,
                                    fill_rule: crate::scene::FillRule::NonZero,
                                };
                                let t = transform2d_from_usvg(p.abs_transform());
                                painter.push_transform(t);
                                painter.stroke_path(
                                    epath,
                                    crate::scene::Stroke {
                                        width: st.width().get() as f32,
                                    },
                                    col,
                                    0,
                                );
                                painter.pop_transform();
                                stats.strokes += 1;
                            }
                        } else {
                            stats.skipped += 1;
                        }
                    }
                }
                usvg::Node::Group(g) => {
                    // Render group contents normally.
                    walk(g, painter, stats);
                }
                usvg::Node::Image(_img) => {
                    // Only traverse subroots for embedded SVG images.
                    // This avoids drawing clipPath/mask/pattern definition subtrees.
                    node.subroots(|subroot| walk(subroot, painter, stats));
                }
                usvg::Node::Text(_) => {
                    // Text-as-geometry not supported yet.
                }
            }
        }
    }

    let root = tree.root();
    walk(root, painter, &mut stats);

    Some(stats)
}

/// Get the intrinsic pixel size of an SVG file according to usvg's parsing
/// of width/height/viewBox. Returns (width,height) rounded to integers.
pub fn svg_intrinsic_size(path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(path).ok()?;
    let mut opt = usvg::Options::default();
    opt.resources_dir = path.parent().map(|p| p.to_path_buf());
    opt.fontdb = SYSTEM_FONTDB.clone();
    let tree = usvg::Tree::from_data(&data, &opt).ok()?;
    let size = tree.size().to_int_size();
    Some((size.width().max(1), size.height().max(1)))
}

/// Determine if an SVG requires rasterization or can be rendered as vector geometry.
/// Returns true if the SVG uses features that cannot be expressed analytically
/// (filters, patterns, masks, gradients, images, text, etc.)
pub fn svg_requires_rasterization(path: &Path) -> Option<bool> {
    let data = std::fs::read(path).ok()?;
    let mut opt = usvg::Options::default();
    opt.resources_dir = path.parent().map(|p| p.to_path_buf());
    opt.fontdb = SYSTEM_FONTDB.clone();
    let tree = usvg::Tree::from_data(&data, &opt).ok()?;

    fn check_node(node: &usvg::Node) -> bool {
        match node {
            usvg::Node::Path(p) => {
                // Check if fill uses non-solid paint (gradients, patterns)
                if let Some(fill) = p.fill() {
                    if !matches!(fill.paint(), usvg::Paint::Color(_)) {
                        return true; // Gradient or pattern fill
                    }
                }

                // Check if stroke uses non-solid paint
                if let Some(stroke) = p.stroke() {
                    if !matches!(stroke.paint(), usvg::Paint::Color(_)) {
                        return true; // Gradient or pattern stroke
                    }
                }

                // Check subroots (e.g., clipPath definitions)
                let mut needs_raster = false;
                node.subroots(|subroot| {
                    if check_group(subroot) {
                        needs_raster = true;
                    }
                });
                needs_raster
            }
            usvg::Node::Image(_) => {
                // Embedded images require rasterization
                true
            }
            usvg::Node::Text(_) => {
                // Text-as-graphics requires rasterization
                true
            }
            usvg::Node::Group(g) => check_group(g),
        }
    }

    fn check_group(group: &usvg::Group) -> bool {
        // Check if group has filters, masks, or other complex features
        // Note: usvg pre-flattens many attributes, so we check children
        for child in group.children() {
            if check_node(&child) {
                return true;
            }
        }
        false
    }

    let requires_raster = check_group(tree.root());
    Some(requires_raster)
}
