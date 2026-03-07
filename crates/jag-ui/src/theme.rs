//! Theme system providing dark and light color palettes for UI elements.
//!
//! Uses [`jag_draw::ColorLinPremul`] for color values, constructed via
//! `ColorLinPremul::from_srgba_u8`.

use jag_draw::ColorLinPremul;

/// Active color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Semantic colors for common UI elements.
#[derive(Debug, Clone, Copy)]
pub struct ElementColors {
    /// Primary text color.
    pub text: ColorLinPremul,
    /// Background color for text inputs and text areas.
    pub input_bg: ColorLinPremul,
    /// Border color for inputs.
    pub input_border: ColorLinPremul,
    /// Background color for buttons.
    pub button_bg: ColorLinPremul,
    /// Foreground (text) color for buttons.
    pub button_fg: ColorLinPremul,
    /// Color of the keyboard-focus ring.
    pub focus_ring: ColorLinPremul,
    /// Color used for error indicators.
    pub error: ColorLinPremul,
}

impl ElementColors {
    /// Return appropriate colors for the given theme mode.
    pub fn for_theme(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                text: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
                input_bg: ColorLinPremul::from_srgba_u8([40, 40, 40, 255]),
                input_border: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
                button_bg: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
                button_fg: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
                focus_ring: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
                error: ColorLinPremul::from_srgba_u8([239, 68, 68, 255]),
            },
            ThemeMode::Light => Self {
                text: ColorLinPremul::from_srgba_u8([15, 23, 42, 255]),
                input_bg: ColorLinPremul::from_srgba_u8([248, 250, 252, 255]),
                input_border: ColorLinPremul::from_srgba_u8([203, 213, 225, 255]),
                button_bg: ColorLinPremul::from_srgba_u8([37, 99, 235, 255]),
                button_fg: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
                focus_ring: ColorLinPremul::from_srgba_u8([37, 99, 235, 255]),
                error: ColorLinPremul::from_srgba_u8([220, 38, 38, 255]),
            },
        }
    }
}

/// Complete theme applied to UI rendering.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Active color mode.
    pub mode: ThemeMode,
    /// Semantic element colors.
    pub colors: ElementColors,
    /// Default font size in logical pixels.
    pub font_size: f32,
    /// Default border radius in logical pixels.
    pub border_radius: f32,
    /// Default spacing between elements in logical pixels.
    pub spacing: f32,
}

impl Theme {
    /// Create the dark theme.
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            colors: ElementColors::for_theme(ThemeMode::Dark),
            font_size: 14.0,
            border_radius: 6.0,
            spacing: 8.0,
        }
    }

    /// Create the light theme.
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            colors: ElementColors::for_theme(ThemeMode::Light),
            font_size: 14.0,
            border_radius: 6.0,
            spacing: 8.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_theme_has_dark_mode() {
        let t = Theme::dark();
        assert_eq!(t.mode, ThemeMode::Dark);
    }

    #[test]
    fn light_theme_has_light_mode() {
        let t = Theme::light();
        assert_eq!(t.mode, ThemeMode::Light);
    }

    #[test]
    fn default_is_dark() {
        let t = Theme::default();
        assert_eq!(t.mode, ThemeMode::Dark);
    }

    #[test]
    fn element_colors_for_dark() {
        let c = ElementColors::for_theme(ThemeMode::Dark);
        // White text in dark theme.
        let white = ColorLinPremul::from_srgba_u8([255, 255, 255, 255]);
        assert_eq!(c.text, white);
    }

    #[test]
    fn element_colors_for_light() {
        let c = ElementColors::for_theme(ThemeMode::Light);
        // Dark text in light theme.
        let dark_text = ColorLinPremul::from_srgba_u8([15, 23, 42, 255]);
        assert_eq!(c.text, dark_text);
    }

    #[test]
    fn theme_defaults_reasonable() {
        let t = Theme::dark();
        assert!(t.font_size > 0.0);
        assert!(t.border_radius >= 0.0);
        assert!(t.spacing > 0.0);
    }
}
