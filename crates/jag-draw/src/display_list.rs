use crate::scene::*;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

/// Opaque handle for an externally-managed texture (e.g., 3D viewport).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExternalTextureId(pub u64);

#[derive(Clone, Debug)]
pub enum Command {
    DrawRect {
        rect: Rect,
        brush: Brush,
        z: i32,
        transform: Transform2D,
    },
    DrawRoundedRect {
        rrect: RoundedRect,
        brush: Brush,
        z: i32,
        transform: Transform2D,
    },
    StrokeRect {
        rect: Rect,
        stroke: Stroke,
        brush: Brush,
        z: i32,
        transform: Transform2D,
    },
    StrokeRoundedRect {
        rrect: RoundedRect,
        stroke: Stroke,
        brush: Brush,
        z: i32,
        transform: Transform2D,
    },
    DrawText {
        run: TextRun,
        z: i32,
        transform: Transform2D,
        id: u64,
        dynamic: bool,
    },
    DrawEllipse {
        center: [f32; 2],
        radii: [f32; 2],
        brush: Brush,
        z: i32,
        transform: Transform2D,
    },
    /// Filled path (solid color only for now)
    FillPath {
        path: Path,
        color: ColorLinPremul,
        z: i32,
        transform: Transform2D,
    },
    /// Stroked path (width only; round join/cap for now)
    StrokePath {
        path: Path,
        stroke: Stroke,
        color: ColorLinPremul,
        z: i32,
        transform: Transform2D,
    },
    /// Box shadow for a rounded rectangle. This is handled by a dedicated pass in PassManager,
    /// not by the generic solid fill pipeline.
    BoxShadow {
        rrect: RoundedRect,
        spec: BoxShadowSpec,
        z: i32,
        transform: Transform2D,
    },
    /// Hit-only regions that do not render. Useful for scene-surface hits or custom zones.
    HitRegionRect {
        id: u32,
        rect: Rect,
        z: i32,
        transform: Transform2D,
    },
    HitRegionRoundedRect {
        id: u32,
        rrect: RoundedRect,
        z: i32,
        transform: Transform2D,
    },
    HitRegionEllipse {
        id: u32,
        center: [f32; 2],
        radii: [f32; 2],
        z: i32,
        transform: Transform2D,
    },
    /// SVG draw at a pixel origin with a max pixel size.
    /// The path is interpreted relative to the process working directory.
    DrawSvg {
        path: PathBuf,
        origin: [f32; 2],
        max_size: [f32; 2],
        z: i32,
        transform: Transform2D,
    },
    /// Raster image draw (PNG/JPEG/GIF/WebP) at a pixel origin with a given size.
    /// The path is interpreted relative to the process working directory.
    DrawImage {
        path: PathBuf,
        origin: [f32; 2],
        size: [f32; 2],
        z: i32,
        transform: Transform2D,
    },
    /// Hyperlink with text, optional underline, and click target.
    DrawHyperlink {
        hyperlink: Hyperlink,
        z: i32,
        transform: Transform2D,
        /// Unique ID for this hyperlink instance (for hit testing)
        id: u64,
    },
    PushClip(ClipRect),
    PopClip,
    PushTransform(Transform2D),
    PopTransform,
    /// Push a CSS-style group opacity for descendant draw commands.
    /// Commands between `PushOpacity` and `PopOpacity` are composited to an
    /// intermediate layer, then blended back once with this opacity.
    PushOpacity(f32),
    PopOpacity,
    /// Blit an externally-rendered texture (e.g., 3D viewport) into the scene.
    DrawExternalTexture {
        rect: Rect,
        texture_id: ExternalTextureId,
        z: i32,
        transform: Transform2D,
        /// Additional alpha multiplier applied at composite time.
        opacity: f32,
        /// Whether the source texture is already premultiplied alpha.
        premultiplied: bool,
    },
}

impl Command {
    /// Get the z-index of this command, or None for non-drawable commands
    pub fn z_index(&self) -> Option<i32> {
        match self {
            Command::DrawRect { z, .. } => Some(*z),
            Command::DrawRoundedRect { z, .. } => Some(*z),
            Command::StrokeRect { z, .. } => Some(*z),
            Command::StrokeRoundedRect { z, .. } => Some(*z),
            Command::DrawText { z, .. } => Some(*z),
            Command::DrawEllipse { z, .. } => Some(*z),
            Command::FillPath { z, .. } => Some(*z),
            Command::StrokePath { z, .. } => Some(*z),
            Command::BoxShadow { z, .. } => Some(*z),
            Command::HitRegionRect { z, .. } => Some(*z),
            Command::HitRegionRoundedRect { z, .. } => Some(*z),
            Command::HitRegionEllipse { z, .. } => Some(*z),
            Command::DrawImage { z, .. } => Some(*z),
            Command::DrawSvg { z, .. } => Some(*z),
            Command::DrawHyperlink { z, .. } => Some(*z),
            Command::DrawExternalTexture { z, .. } => Some(*z),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub viewport: Viewport,
    pub commands: Vec<Command>,
}

impl DisplayList {
    /// Sort commands by z-index while preserving clip/transform stack structure.
    /// This is a simplified implementation that sorts drawable commands but keeps
    /// transform/clip commands in their original order. For proper z-ordering with
    /// transforms and clips, each drawable should store its full transform/clip state.
    pub fn sort_by_z(&mut self) {
        // Sort by z-index. Rust's sort_by is stable, preserving relative order of equal elements.
        // This means transform/clip commands (which have no z-index) will stay in order,
        // but drawable commands will be sorted by z-index.
        self.commands.sort_by(|a, b| {
            match (a.z_index(), b.z_index()) {
                (Some(z_a), Some(z_b)) => z_a.cmp(&z_b),
                (Some(_), None) => std::cmp::Ordering::Greater, // Drawables after non-drawables
                (None, Some(_)) => std::cmp::Ordering::Less,    // Non-drawables before drawables
                (None, None) => std::cmp::Ordering::Equal,      // Preserve order for non-drawables
            }
        });
    }
}

/// Convert z-index to depth value for GPU depth testing.
/// Maps z-index range to [0.0, 1.0] where lower z-index = closer to camera = lower depth.
///
/// Strategy:
/// - z = 0 maps to depth 0.5 (middle)
/// - Negative z (closer) maps to (0.0, 0.5)
/// - Positive z (farther) maps to (0.5, 1.0)
/// - Clamps to reasonable range to avoid precision issues
pub fn z_index_to_depth(z: i32) -> f32 {
    // Clamp z-index to reasonable range [-1000000, 1000000].
    // Compositor windows use z_base up to 20000, plus per-window z offsets
    // up to ~12000 (AppWindow chrome), so ~32000+ per window.
    // Shell overlays (launcher) use z ~500000 to stay above all windows.
    let z_clamped = z.clamp(-1_000_000, 1_000_000) as f32;

    // Map to [0.0, 1.0] with 0.5 as center
    // HIGHER z-index = closer = LOWER depth value (rendered on top)
    // Negate z to invert the mapping
    let normalized = (-z_clamped / 1_000_000.0) * 0.5 + 0.5;

    // Clamp to valid depth range
    normalized.clamp(0.0, 1.0)
}
