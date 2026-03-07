//! Central UI coordinator that owns focus, hit-testing, layout, and theming.
//!
//! [`Ui`] is the main entry point for building and managing a jag UI.

use crate::focus::FocusManager;
use crate::hit_region::HitRegionRegistry;
use crate::layout::Layout;
use crate::theme::Theme;

/// Central coordinator for a jag UI.
///
/// Holds the focus manager, hit-region registry, layout tree, and theme.
/// Pass this around to widget and element rendering code so they can
/// register themselves for focus, hit-testing, and layout.
pub struct Ui {
    /// Focus tracking and keyboard navigation.
    pub focus: FocusManager,
    /// Hit-region registry for GPU-based hit-testing.
    pub hit_registry: HitRegionRegistry,
    /// Taffy-backed layout tree.
    pub layout: Layout,
    /// Active theme (colors, spacing, font size).
    pub theme: Theme,
}

impl Ui {
    /// Create a new `Ui` with the default (dark) theme.
    pub fn new() -> Self {
        Self {
            focus: FocusManager::new(),
            hit_registry: HitRegionRegistry::new(),
            layout: Layout::new(),
            theme: Theme::default(),
        }
    }

    /// Create a new `Ui` with the given theme.
    pub fn with_theme(theme: Theme) -> Self {
        Self {
            focus: FocusManager::new(),
            hit_registry: HitRegionRegistry::new(),
            layout: Layout::new(),
            theme,
        }
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;

    #[test]
    fn new_creates_default_ui() {
        let ui = Ui::new();
        assert!(ui.focus.current().is_none());
        assert_eq!(ui.theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn with_theme_uses_provided_theme() {
        let ui = Ui::with_theme(Theme::light());
        assert_eq!(ui.theme.mode, ThemeMode::Light);
    }

    #[test]
    fn default_has_no_focus() {
        let ui = Ui::default();
        assert!(ui.focus.current().is_none());
    }

    #[test]
    fn hit_registry_starts_empty() {
        let ui = Ui::new();
        assert_eq!(ui.hit_registry.lookup(1), None);
    }
}
