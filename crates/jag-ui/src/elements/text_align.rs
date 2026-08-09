//! Text alignment enum used by form elements.

/// Horizontal text alignment within a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    /// Left-aligned text (default).
    #[default]
    Left,
    /// Center-aligned text.
    Center,
    /// Right-aligned text.
    Right,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_left() {
        assert_eq!(TextAlign::default(), TextAlign::Left);
    }

    #[test]
    fn variants_are_distinct() {
        assert_ne!(TextAlign::Left, TextAlign::Center);
        assert_ne!(TextAlign::Center, TextAlign::Right);
        assert_ne!(TextAlign::Left, TextAlign::Right);
    }

    #[test]
    fn clone_and_copy() {
        fn assert_clone_copy<T: Clone + Copy>() {}
        assert_clone_copy::<TextAlign>();
    }
}
