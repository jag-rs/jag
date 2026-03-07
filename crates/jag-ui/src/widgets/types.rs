//! Shared widget types: colors, events, and cursor hints.

use jag_draw::ColorLinPremul;

use crate::ThemeMode;

/// Cursor hint returned by widgets so the host can set the OS cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorHint {
    Default,
    ResizeHorizontal,
    ResizeVertical,
    Pointer,
}

/// Semantic events returned by widgets after processing input.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetEvent {
    /// Input was not relevant to this widget.
    Ignored,
    /// Input was consumed but produced no semantic action.
    Consumed,
    /// Tab was selected by index.
    TabSelected { index: usize },
    /// Tab close requested by index.
    TabClose { index: usize },
    /// Popup menu item selected by index.
    PopupItemSelected { index: usize },
    /// Popup was dismissed (Escape or outside click).
    PopupDismissed,
    /// Widget requests a cursor change.
    SetCursor(CursorHint),
}

/// Color palette for widget rendering, derived from [`ThemeMode`].
#[derive(Debug, Clone, Copy)]
pub struct WidgetColors {
    /// Main background of panels / containers.
    pub bg: ColorLinPremul,
    /// Slightly lighter/darker surface (e.g. active tab, hovered row).
    pub surface: ColorLinPremul,
    /// Hover highlight color.
    pub hover: ColorLinPremul,
    /// Active/selected item background.
    pub active: ColorLinPremul,
    /// Accent underline / indicator color.
    pub accent: ColorLinPremul,
    /// Primary text color.
    pub text: ColorLinPremul,
    /// Muted / secondary text color.
    pub text_muted: ColorLinPremul,
    /// Border / separator line color.
    pub border: ColorLinPremul,
    /// Close / destructive icon color.
    pub close_icon: ColorLinPremul,
    /// Dirty-file indicator dot color.
    pub dirty_dot: ColorLinPremul,
    /// Shadow color for popups.
    pub shadow: ColorLinPremul,
}

impl WidgetColors {
    /// Derive widget colors from the given theme mode.
    pub fn from_theme_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                bg: ColorLinPremul::from_srgba_u8([22, 27, 47, 255]),
                surface: ColorLinPremul::from_srgba_u8([30, 35, 55, 255]),
                hover: ColorLinPremul::from_srgba_u8([40, 46, 70, 255]),
                active: ColorLinPremul::from_srgba_u8([50, 56, 82, 255]),
                accent: ColorLinPremul::from_srgba_u8([100, 140, 255, 255]),
                text: ColorLinPremul::from_srgba_u8([220, 225, 240, 255]),
                text_muted: ColorLinPremul::from_srgba_u8([140, 150, 170, 255]),
                border: ColorLinPremul::from_srgba_u8([60, 65, 85, 255]),
                close_icon: ColorLinPremul::from_srgba_u8([100, 110, 130, 255]),
                dirty_dot: ColorLinPremul::from_srgba_u8([255, 200, 50, 255]),
                shadow: ColorLinPremul::from_srgba_u8([0, 0, 0, 120]),
            },
            ThemeMode::Light => Self {
                bg: ColorLinPremul::from_srgba_u8([248, 250, 252, 255]),
                surface: ColorLinPremul::from_srgba_u8([241, 245, 249, 255]),
                hover: ColorLinPremul::from_srgba_u8([226, 232, 240, 255]),
                active: ColorLinPremul::from_srgba_u8([203, 213, 225, 255]),
                accent: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
                text: ColorLinPremul::from_srgba_u8([15, 23, 42, 255]),
                text_muted: ColorLinPremul::from_srgba_u8([100, 116, 139, 255]),
                border: ColorLinPremul::from_srgba_u8([226, 232, 240, 255]),
                close_icon: ColorLinPremul::from_srgba_u8([148, 163, 184, 255]),
                dirty_dot: ColorLinPremul::from_srgba_u8([245, 158, 11, 255]),
                shadow: ColorLinPremul::from_srgba_u8([0, 0, 0, 40]),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_event_equality() {
        assert_eq!(WidgetEvent::Ignored, WidgetEvent::Ignored);
        assert_ne!(WidgetEvent::Ignored, WidgetEvent::Consumed);
        assert_eq!(
            WidgetEvent::TabSelected { index: 1 },
            WidgetEvent::TabSelected { index: 1 }
        );
        assert_ne!(
            WidgetEvent::TabSelected { index: 0 },
            WidgetEvent::TabClose { index: 0 }
        );
    }

    #[test]
    fn widget_colors_dark_and_light_differ() {
        let dark = WidgetColors::from_theme_mode(ThemeMode::Dark);
        let light = WidgetColors::from_theme_mode(ThemeMode::Light);
        // Background colors should differ between themes.
        assert_ne!(dark.bg, light.bg);
    }

    #[test]
    fn cursor_hint_variants() {
        let hints = [
            CursorHint::Default,
            CursorHint::ResizeHorizontal,
            CursorHint::ResizeVertical,
            CursorHint::Pointer,
        ];
        for (i, a) in hints.iter().enumerate() {
            for (j, b) in hints.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }
}
