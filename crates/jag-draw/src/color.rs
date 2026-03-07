use palette::{FromColor, LinSrgba, Srgba};

use crate::scene::ColorLinPremul;

// sRGB → Linear premultiplied conversions, kept out of scene.rs for separation of concerns.
impl ColorLinPremul {
    /// Convenience alias matching Color::rgba(...) widely used in UI code.
    #[inline]
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::from_srgba_u8([r, g, b, a])
    }

    /// Create from sRGB u8 RGBA array (premultiplied in linear space).
    #[inline]
    pub fn from_srgba_u8(c: [u8; 4]) -> Self {
        let s = Srgba::new(
            c[0] as f32 / 255.0,
            c[1] as f32 / 255.0,
            c[2] as f32 / 255.0,
            c[3] as f32 / 255.0,
        );
        let lin: LinSrgba = LinSrgba::from_color(s);
        Self {
            r: lin.red * lin.alpha,
            g: lin.green * lin.alpha,
            b: lin.blue * lin.alpha,
            a: lin.alpha,
        }
    }

    /// Create from sRGB u8 RGB with float alpha (CSS-like rgba).
    #[inline]
    pub fn from_srgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        let s = Srgba::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a);
        let lin: LinSrgba = LinSrgba::from_color(s);
        Self {
            r: lin.red * lin.alpha,
            g: lin.green * lin.alpha,
            b: lin.blue * lin.alpha,
            a: lin.alpha,
        }
    }

    /// Create directly from linear RGBA floats and premultiply.
    #[inline]
    pub fn from_lin_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: r * a,
            g: g * a,
            b: b * a,
            a,
        }
    }

    /// Convert back to sRGB u8 RGBA array (unpremultiplied).
    #[inline]
    pub fn to_srgba_u8(&self) -> [u8; 4] {
        // Unpremultiply
        let (r, g, b) = if self.a > 0.0001 {
            (self.r / self.a, self.g / self.a, self.b / self.a)
        } else {
            (0.0, 0.0, 0.0)
        };

        // Convert linear to sRGB
        let lin = LinSrgba::new(r, g, b, self.a);
        let srgb: Srgba = Srgba::from_color(lin);

        [
            (srgb.red * 255.0).round().clamp(0.0, 255.0) as u8,
            (srgb.green * 255.0).round().clamp(0.0, 255.0) as u8,
            (srgb.blue * 255.0).round().clamp(0.0, 255.0) as u8,
            (srgb.alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    }
}
