use palette::{FromColor, LinSrgba, Srgba};

use crate::scene::{ColorLinPremul, SrgbColor};

impl SrgbColor {
    #[inline]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline]
    pub const fn from_rgba_u8(c: [u8; 4]) -> Self {
        Self {
            r: c[0],
            g: c[1],
            b: c[2],
            a: c[3],
        }
    }

    #[inline]
    pub const fn to_rgba_u8(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    #[inline]
    pub fn to_linear_premul(self) -> ColorLinPremul {
        ColorLinPremul::from(self)
    }
}

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
        SrgbColor::from_rgba_u8(c).to_linear_premul()
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
        SrgbColor::from(*self).to_rgba_u8()
    }
}

impl From<SrgbColor> for ColorLinPremul {
    fn from(color: SrgbColor) -> Self {
        let s = Srgba::new(
            color.r as f32 / 255.0,
            color.g as f32 / 255.0,
            color.b as f32 / 255.0,
            color.a as f32 / 255.0,
        );
        let lin: LinSrgba = LinSrgba::from_color(s);
        Self {
            r: lin.red * lin.alpha,
            g: lin.green * lin.alpha,
            b: lin.blue * lin.alpha,
            a: lin.alpha,
        }
    }
}

impl From<ColorLinPremul> for SrgbColor {
    fn from(color: ColorLinPremul) -> Self {
        let (r, g, b) = if color.a > 0.0001 {
            (color.r / color.a, color.g / color.a, color.b / color.a)
        } else {
            (0.0, 0.0, 0.0)
        };

        let lin = LinSrgba::new(r, g, b, color.a);
        let srgb: Srgba = Srgba::from_color(lin);

        Self {
            r: (srgb.red * 255.0).round().clamp(0.0, 255.0) as u8,
            g: (srgb.green * 255.0).round().clamp(0.0, 255.0) as u8,
            b: (srgb.blue * 255.0).round().clamp(0.0, 255.0) as u8,
            a: (srgb.alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorLinPremul, SrgbColor};

    #[test]
    fn srgb_color_round_trips_through_linear_premul() {
        let srgb = SrgbColor::rgba(0x95, 0xa5, 0xa6, 0x80);

        assert_eq!(SrgbColor::from(ColorLinPremul::from(srgb)), srgb);
    }

    #[test]
    fn linear_premul_helpers_match_srgb_type_conversions() {
        let srgb = SrgbColor::rgba(52, 152, 219, 26);

        assert_eq!(
            ColorLinPremul::from_srgba_u8(srgb.to_rgba_u8()),
            srgb.to_linear_premul()
        );
        assert_eq!(ColorLinPremul::from(srgb).to_srgba_u8(), srgb.to_rgba_u8());
    }
}
