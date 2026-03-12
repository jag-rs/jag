//! Theme system providing dark and light color palettes for UI elements.
//!
//! The [`Theme`] trait defines the full set of visual properties that UI
//! elements query at render time.  [`DefaultTheme`] is a concrete
//! implementation with sensible light-mode defaults (matching the values
//! that detir elements historically hard-code).
//!
//! External crates can implement [`Theme`] to inject CSS-cascade-driven
//! or runtime-customised styling.

use jag_draw::ColorLinPremul;

// ---------------------------------------------------------------------------
// Sides<T>
// ---------------------------------------------------------------------------

/// Four-sided value in CSS order: top, right, bottom, left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Sides<T> {
    /// All four sides set to the same value.
    pub fn all(v: T) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    /// Symmetric padding: `v` for top/bottom, `h` for left/right.
    pub fn symmetric(v: T, h: T) -> Self {
        Self {
            top: v,
            right: h,
            bottom: v,
            left: h,
        }
    }
}

// ---------------------------------------------------------------------------
// ThemeMode
// ---------------------------------------------------------------------------

/// Active color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    Dark,
    Light,
}

// ---------------------------------------------------------------------------
// ElementColors (kept for backward compat)
// ---------------------------------------------------------------------------

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
                button_bg: ColorLinPremul::from_srgba_u8([239, 239, 239, 255]),
                button_fg: ColorLinPremul::from_srgba_u8([0, 0, 0, 255]),
                focus_ring: ColorLinPremul::from_srgba_u8([63, 130, 246, 255]),
                error: ColorLinPremul::from_srgba_u8([220, 38, 38, 255]),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Theme trait
// ---------------------------------------------------------------------------

/// Trait defining the full set of visual properties for UI elements.
///
/// Implement this trait to provide custom theming (e.g. CSS-cascade-driven
/// themes in `detir-scene`).
pub trait Theme {
    // -- global --

    /// Active color mode.
    fn mode(&self) -> ThemeMode;

    /// Primary text color.
    fn text_color(&self) -> ColorLinPremul;

    /// Default font size in logical pixels.
    fn font_size(&self) -> f32;

    /// Default border radius in logical pixels.
    fn border_radius(&self) -> f32;

    /// Default spacing between elements in logical pixels.
    fn spacing(&self) -> f32;

    // -- focus --

    /// Focus ring color.
    fn focus_color(&self) -> ColorLinPremul;

    /// Focus ring stroke width.
    fn focus_ring_width(&self) -> f32;

    /// Focus ring offset from element edge.
    fn focus_ring_offset(&self) -> f32;

    // -- error --

    /// Error indicator color.
    fn error_color(&self) -> ColorLinPremul;

    // -- selection / caret / placeholder --

    /// Text selection highlight color.
    fn selection_color(&self) -> ColorLinPremul;

    /// Text caret (cursor) color.
    fn caret_color(&self) -> ColorLinPremul;

    /// Placeholder text color.
    fn placeholder_color(&self) -> ColorLinPremul;

    /// Background for disabled elements.
    fn disabled_bg(&self) -> ColorLinPremul;

    // -- input --

    /// Input background color.
    fn input_bg(&self) -> ColorLinPremul;

    /// Input border color.
    fn input_border(&self) -> ColorLinPremul;

    /// Input border stroke width.
    fn input_border_width(&self) -> f32;

    /// Input border radius.
    fn input_border_radius(&self) -> f32;

    /// Input internal padding.
    fn input_padding(&self) -> Sides<f32>;

    /// Input font size.
    fn input_font_size(&self) -> f32;

    // -- button --

    /// Button background color.
    fn button_bg(&self) -> ColorLinPremul;

    /// Button foreground (text) color.
    fn button_fg(&self) -> ColorLinPremul;

    /// Button border color.
    fn button_border(&self) -> ColorLinPremul;

    /// Button border stroke width.
    fn button_border_width(&self) -> f32;

    /// Button border radius.
    fn button_border_radius(&self) -> f32;

    /// Button internal padding.
    fn button_padding(&self) -> Sides<f32>;

    /// Button label font size.
    fn button_font_size(&self) -> f32;

    /// Button label font weight.
    fn button_font_weight(&self) -> f32;

    // -- checkbox --

    /// Checkbox corner radius.
    fn check_border_radius(&self) -> f32;

    /// Gap between checkbox and its label.
    fn check_label_gap(&self) -> f32;

    // -- toggle --

    /// Toggle switch track width.
    fn toggle_width(&self) -> f32;

    /// Toggle switch track height.
    fn toggle_height(&self) -> f32;

    // -- select / dropdown --

    /// Select dropdown background color.
    fn select_dropdown_bg(&self) -> ColorLinPremul;

    /// Select dropdown corner radius.
    fn select_dropdown_radius(&self) -> f32;

    /// Height of each option row in the dropdown.
    fn select_option_height(&self) -> f32;

    /// Background color for the currently-selected option.
    fn select_selected_bg(&self) -> ColorLinPremul;

    // -- backward-compat helper --

    /// Return an [`ElementColors`] snapshot for legacy consumers.
    fn element_colors(&self) -> ElementColors {
        ElementColors {
            text: self.text_color(),
            input_bg: self.input_bg(),
            input_border: self.input_border(),
            button_bg: self.button_bg(),
            button_fg: self.button_fg(),
            focus_ring: self.focus_color(),
            error: self.error_color(),
        }
    }
}

// ---------------------------------------------------------------------------
// DefaultTheme
// ---------------------------------------------------------------------------

/// Concrete theme with sensible defaults.
///
/// Light-mode values match the colours that detir elements historically
/// hard-code; dark-mode values come from the original `ElementColors` palette.
#[derive(Debug, Clone, Copy)]
pub struct DefaultTheme {
    /// Active color mode.
    pub mode: ThemeMode,
    /// Semantic element colors (legacy field, kept for convenience).
    pub colors: ElementColors,
    /// Default font size in logical pixels.
    pub font_size_val: f32,
    /// Default border radius in logical pixels.
    pub border_radius_val: f32,
    /// Default spacing between elements in logical pixels.
    pub spacing_val: f32,
}

impl DefaultTheme {
    /// Create the dark theme.
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            colors: ElementColors::for_theme(ThemeMode::Dark),
            font_size_val: 14.0,
            border_radius_val: 6.0,
            spacing_val: 8.0,
        }
    }

    /// Create the light theme.
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            colors: ElementColors::for_theme(ThemeMode::Light),
            font_size_val: 14.0,
            border_radius_val: 6.0,
            spacing_val: 8.0,
        }
    }
}

impl Default for DefaultTheme {
    fn default() -> Self {
        Self::dark()
    }
}

// ---------------------------------------------------------------------------
// Theme impl for DefaultTheme
// ---------------------------------------------------------------------------

impl Theme for DefaultTheme {
    fn mode(&self) -> ThemeMode {
        self.mode
    }

    fn text_color(&self) -> ColorLinPremul {
        self.colors.text
    }

    fn font_size(&self) -> f32 {
        self.font_size_val
    }

    fn border_radius(&self) -> f32 {
        self.border_radius_val
    }

    fn spacing(&self) -> f32 {
        self.spacing_val
    }

    // -- focus --

    fn focus_color(&self) -> ColorLinPremul {
        self.colors.focus_ring
    }

    fn focus_ring_width(&self) -> f32 {
        2.0
    }

    fn focus_ring_offset(&self) -> f32 {
        2.0
    }

    // -- error --

    fn error_color(&self) -> ColorLinPremul {
        self.colors.error
    }

    // -- selection / caret / placeholder --

    fn selection_color(&self) -> ColorLinPremul {
        ColorLinPremul::from_srgba_u8([63, 130, 246, 80])
    }

    fn caret_color(&self) -> ColorLinPremul {
        ColorLinPremul::from_srgba_u8([63, 130, 246, 255])
    }

    fn placeholder_color(&self) -> ColorLinPremul {
        ColorLinPremul::from_srgba_u8([160, 170, 180, 255])
    }

    fn disabled_bg(&self) -> ColorLinPremul {
        ColorLinPremul::from_srgba_u8([240, 240, 240, 255])
    }

    // -- input --

    fn input_bg(&self) -> ColorLinPremul {
        self.colors.input_bg
    }

    fn input_border(&self) -> ColorLinPremul {
        self.colors.input_border
    }

    fn input_border_width(&self) -> f32 {
        1.0
    }

    fn input_border_radius(&self) -> f32 {
        self.border_radius_val
    }

    fn input_padding(&self) -> Sides<f32> {
        Sides {
            top: 10.0,
            right: 12.0,
            bottom: 10.0,
            left: 12.0,
        }
    }

    fn input_font_size(&self) -> f32 {
        15.0
    }

    // -- button --

    fn button_bg(&self) -> ColorLinPremul {
        self.colors.button_bg
    }

    fn button_fg(&self) -> ColorLinPremul {
        self.colors.button_fg
    }

    fn button_border(&self) -> ColorLinPremul {
        ColorLinPremul::from_srgba_u8([118, 118, 118, 255])
    }

    fn button_border_width(&self) -> f32 {
        1.0
    }

    fn button_border_radius(&self) -> f32 {
        self.border_radius_val
    }

    fn button_padding(&self) -> Sides<f32> {
        Sides {
            top: 1.0,
            right: 6.0,
            bottom: 1.0,
            left: 6.0,
        }
    }

    fn button_font_size(&self) -> f32 {
        13.0
    }

    fn button_font_weight(&self) -> f32 {
        400.0
    }

    // -- checkbox --

    fn check_border_radius(&self) -> f32 {
        2.0
    }

    fn check_label_gap(&self) -> f32 {
        8.0
    }

    // -- toggle --

    fn toggle_width(&self) -> f32 {
        44.0
    }

    fn toggle_height(&self) -> f32 {
        24.0
    }

    // -- select / dropdown --

    fn select_dropdown_bg(&self) -> ColorLinPremul {
        ColorLinPremul::from_srgba_u8([255, 255, 255, 255])
    }

    fn select_dropdown_radius(&self) -> f32 {
        6.0
    }

    fn select_option_height(&self) -> f32 {
        36.0
    }

    fn select_selected_bg(&self) -> ColorLinPremul {
        ColorLinPremul::from_srgba_u8([220, 220, 224, 255])
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
        let t = DefaultTheme::dark();
        assert_eq!(t.mode(), ThemeMode::Dark);
    }

    #[test]
    fn light_theme_has_light_mode() {
        let t = DefaultTheme::light();
        assert_eq!(t.mode(), ThemeMode::Light);
    }

    #[test]
    fn default_is_dark() {
        let t = DefaultTheme::default();
        assert_eq!(t.mode(), ThemeMode::Dark);
    }

    #[test]
    fn element_colors_for_dark() {
        let c = ElementColors::for_theme(ThemeMode::Dark);
        let white = ColorLinPremul::from_srgba_u8([255, 255, 255, 255]);
        assert_eq!(c.text, white);
    }

    #[test]
    fn element_colors_for_light() {
        let c = ElementColors::for_theme(ThemeMode::Light);
        let dark_text = ColorLinPremul::from_srgba_u8([15, 23, 42, 255]);
        assert_eq!(c.text, dark_text);
    }

    #[test]
    fn theme_defaults_reasonable() {
        let t = DefaultTheme::dark();
        assert!(t.font_size() > 0.0);
        assert!(t.border_radius() >= 0.0);
        assert!(t.spacing() > 0.0);
    }

    #[test]
    fn trait_object_works() {
        let theme: &dyn Theme = &DefaultTheme::light();
        assert_eq!(theme.mode(), ThemeMode::Light);
        assert_eq!(theme.button_font_weight(), 400.0);
        assert_eq!(theme.focus_ring_width(), 2.0);
    }

    #[test]
    fn element_colors_from_trait() {
        let theme = DefaultTheme::light();
        let ec = theme.element_colors();
        assert_eq!(ec.input_bg, theme.input_bg());
        assert_eq!(ec.button_bg, theme.button_bg());
        assert_eq!(ec.focus_ring, theme.focus_color());
    }

    #[test]
    fn sides_all() {
        let s = Sides::all(5.0_f32);
        assert_eq!(s.top, 5.0);
        assert_eq!(s.right, 5.0);
        assert_eq!(s.bottom, 5.0);
        assert_eq!(s.left, 5.0);
    }

    #[test]
    fn sides_symmetric() {
        let s = Sides::symmetric(10.0_f32, 20.0_f32);
        assert_eq!(s.top, 10.0);
        assert_eq!(s.right, 20.0);
        assert_eq!(s.bottom, 10.0);
        assert_eq!(s.left, 20.0);
    }

    #[test]
    fn input_padding_defaults() {
        let t = DefaultTheme::light();
        let p = t.input_padding();
        assert_eq!(p.top, 10.0);
        assert_eq!(p.right, 12.0);
        assert_eq!(p.bottom, 10.0);
        assert_eq!(p.left, 12.0);
    }

    #[test]
    fn button_padding_defaults() {
        let t = DefaultTheme::light();
        let p = t.button_padding();
        assert_eq!(p.top, 1.0);
        assert_eq!(p.right, 6.0);
        assert_eq!(p.bottom, 1.0);
        assert_eq!(p.left, 6.0);
    }

    #[test]
    fn custom_theme_implementation() {
        struct RedTheme;
        impl Theme for RedTheme {
            fn mode(&self) -> ThemeMode {
                ThemeMode::Light
            }
            fn text_color(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 0, 0, 255])
            }
            fn font_size(&self) -> f32 {
                16.0
            }
            fn border_radius(&self) -> f32 {
                0.0
            }
            fn spacing(&self) -> f32 {
                4.0
            }
            fn focus_color(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 0, 0, 255])
            }
            fn focus_ring_width(&self) -> f32 {
                3.0
            }
            fn focus_ring_offset(&self) -> f32 {
                1.0
            }
            fn error_color(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 0, 0, 255])
            }
            fn selection_color(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 0, 0, 80])
            }
            fn caret_color(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 0, 0, 255])
            }
            fn placeholder_color(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([200, 200, 200, 255])
            }
            fn disabled_bg(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([240, 240, 240, 255])
            }
            fn input_bg(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 240, 240, 255])
            }
            fn input_border(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 0, 0, 255])
            }
            fn input_border_width(&self) -> f32 {
                2.0
            }
            fn input_border_radius(&self) -> f32 {
                4.0
            }
            fn input_padding(&self) -> Sides<f32> {
                Sides::all(8.0)
            }
            fn input_font_size(&self) -> f32 {
                14.0
            }
            fn button_bg(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 0, 0, 255])
            }
            fn button_fg(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 255, 255, 255])
            }
            fn button_border(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([200, 0, 0, 255])
            }
            fn button_border_width(&self) -> f32 {
                1.0
            }
            fn button_border_radius(&self) -> f32 {
                0.0
            }
            fn button_padding(&self) -> Sides<f32> {
                Sides::symmetric(4.0, 12.0)
            }
            fn button_font_size(&self) -> f32 {
                14.0
            }
            fn button_font_weight(&self) -> f32 {
                700.0
            }
            fn check_border_radius(&self) -> f32 {
                0.0
            }
            fn check_label_gap(&self) -> f32 {
                6.0
            }
            fn toggle_width(&self) -> f32 {
                40.0
            }
            fn toggle_height(&self) -> f32 {
                20.0
            }
            fn select_dropdown_bg(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 240, 240, 255])
            }
            fn select_dropdown_radius(&self) -> f32 {
                4.0
            }
            fn select_option_height(&self) -> f32 {
                32.0
            }
            fn select_selected_bg(&self) -> ColorLinPremul {
                ColorLinPremul::from_srgba_u8([255, 200, 200, 255])
            }
        }

        let theme: &dyn Theme = &RedTheme;
        assert_eq!(theme.font_size(), 16.0);
        assert_eq!(theme.button_font_weight(), 700.0);
        assert_eq!(theme.border_radius(), 0.0);
    }
}
