#[derive(Clone, Copy, Debug, Default)]
pub struct Transform2D {
    // Affine 2D: [a, b, c, d, e, f] for matrix [[a c e],[b d f],[0 0 1]]
    pub m: [f32; 6],
}

impl Transform2D {
    pub fn identity() -> Self {
        Self {
            m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    /// Compose two transforms: self ∘ other (apply `other`, then `self`).
    pub fn concat(self, other: Self) -> Self {
        let [a1, b1, c1, d1, e1, f1] = self.m;
        let [a2, b2, c2, d2, e2, f2] = other.m;
        let a = a1 * a2 + c1 * b2;
        let b = b1 * a2 + d1 * b2;
        let c = a1 * c2 + c1 * d2;
        let d = b1 * c2 + d1 * d2;
        let e = a1 * e2 + c1 * f2 + e1;
        let f = b1 * e2 + d1 * f2 + f1;
        Self {
            m: [a, b, c, d, e, f],
        }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Self {
            m: [sx, 0.0, 0.0, sy, 0.0, 0.0],
        }
    }

    pub fn translate(tx: f32, ty: f32) -> Self {
        Self {
            m: [1.0, 0.0, 0.0, 1.0, tx, ty],
        }
    }

    /// Create a rotation transform (angle in radians, counter-clockwise).
    pub fn rotate(angle: f32) -> Self {
        let cos = angle.cos();
        let sin = angle.sin();
        Self {
            m: [cos, sin, -sin, cos, 0.0, 0.0],
        }
    }

    /// Create a rotation transform around a specific center point.
    pub fn rotate_around(angle: f32, cx: f32, cy: f32) -> Self {
        // Translate to origin, rotate, translate back
        Self::translate(cx, cy)
            .concat(Self::rotate(angle))
            .concat(Self::translate(-cx, -cy))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorLinPremul {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Alias for the premultiplied linear color type, for a friendlier name in APIs.
pub type Color = ColorLinPremul;

// Constructors for ColorLinPremul are defined in color.rs to keep scene.rs focused

#[derive(Clone, Debug)]
pub enum Brush {
    Solid(ColorLinPremul),
    LinearGradient {
        start: [f32; 2],
        end: [f32; 2],
        stops: Vec<(f32, ColorLinPremul)>,
    },
    RadialGradient {
        center: [f32; 2],
        radius: f32,
        stops: Vec<(f32, ColorLinPremul)>,
    },
    // Pattern, RadialGradient etc. can be added later.
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundedRadii {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedRect {
    pub rect: Rect,
    pub radii: RoundedRadii,
}

/// Rounded-rect clip data ready for the GPU image shader.
/// Rect and radii are in device pixels.
#[derive(Clone, Copy, Debug)]
pub struct RoundedRectClipGpu {
    /// [x, y, w, h] in device pixels.
    pub rect: [f32; 4],
    /// [tl, tr, br, bl] corner radii in device pixels.
    pub radii: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct ClipRect(pub Rect);

#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub width: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct BoxShadowSpec {
    pub offset: [f32; 2],
    pub spread: f32,
    pub blur_radius: f32,
    pub color: ColorLinPremul,
}

#[derive(Clone, Debug)]
pub enum Shape {
    Rect(Rect),
    RoundedRect(RoundedRect),
}

/// Font style for text rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Clone, Debug)]
pub struct TextRun {
    pub text: String,
    pub pos: [f32; 2],
    pub size: f32,
    pub color: ColorLinPremul,
    /// CSS-like font weight in the 100–900 range (normal = 400, bold ≈ 700).
    /// Renderers that do not support varying weights may ignore this.
    pub weight: f32,
    /// Font style (normal, italic, oblique).
    /// Renderers that do not support font styles may ignore this.
    pub style: FontStyle,
    /// Font family name override. If None, uses the default font.
    /// Renderers that do not support font families may ignore this.
    pub family: Option<String>,
}

// --- Path geometry (for SVG import / lyon) ---

#[derive(Clone, Copy, Debug)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Clone, Debug)]
pub enum PathCmd {
    MoveTo([f32; 2]),
    LineTo([f32; 2]),
    QuadTo([f32; 2], [f32; 2]),
    CubicTo([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

#[derive(Clone, Debug)]
pub struct Path {
    pub cmds: Vec<PathCmd>,
    pub fill_rule: FillRule,
}

// --- Hyperlink ---

/// Hyperlink element combining text, optional underline, and a URL target.
#[derive(Clone, Debug)]
pub struct Hyperlink {
    /// The text content to display
    pub text: String,
    /// Position of the hyperlink (baseline-left of text)
    pub pos: [f32; 2],
    /// Font size in pixels
    pub size: f32,
    /// Text color (premultiplied linear)
    pub color: ColorLinPremul,
    /// Target URL
    pub url: String,
    /// Font weight (e.g. 400.0 for normal, 700.0 for bold)
    pub weight: f32,
    /// Optional pre-measured text width (logical pixels).
    ///
    /// When set, renderers/hit-testing should prefer this over heuristic
    /// character-count estimates so inline links stay aligned with layout.
    pub measured_width: Option<f32>,
    /// Whether to show an underline decoration (default: true)
    pub underline: bool,
    /// Underline color (if None, uses text color)
    pub underline_color: Option<ColorLinPremul>,
    /// Font family name override. If None, uses the default font.
    pub family: Option<String>,
    /// Font style (normal, italic, oblique)
    pub style: FontStyle,
}
