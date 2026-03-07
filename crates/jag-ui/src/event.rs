//! Standalone event types and the `EventHandler` trait.
//!
//! These types are self-contained — no external platform dependencies.

/// Mouse button identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Whether a key or button is pressed or released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementState {
    Pressed,
    Released,
}

/// Active modifier keys at the time of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// Keyboard key codes relevant for widget interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Tab,
    Enter,
    Escape,
    Space,
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    KeyA,
    KeyC,
    KeyV,
    KeyX,
    KeyZ,
    /// Any key not explicitly listed above.
    Other(u32),
}

/// Result of an event handler invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// The event was consumed by the handler.
    Handled,
    /// The event was not consumed and may propagate further.
    Ignored,
}

/// A mouse click (press or release) at a position in logical coordinates.
#[derive(Debug, Clone)]
pub struct MouseClickEvent {
    pub button: MouseButton,
    pub state: ElementState,
    pub x: f32,
    pub y: f32,
    pub click_count: u32,
}

/// A keyboard key event with optional text input.
#[derive(Debug, Clone)]
pub struct KeyboardEvent {
    pub key: KeyCode,
    pub state: ElementState,
    pub modifiers: Modifiers,
    pub text: Option<String>,
}

/// A mouse-move event at a position in logical coordinates.
#[derive(Debug, Clone)]
pub struct MouseMoveEvent {
    pub x: f32,
    pub y: f32,
}

/// A scroll event with position and delta.
#[derive(Debug, Clone)]
pub struct ScrollEvent {
    pub x: f32,
    pub y: f32,
    pub delta_x: f32,
    pub delta_y: f32,
}

/// Trait for elements that can receive and handle input events.
pub trait EventHandler {
    /// Handle a mouse click event.  Returns `Handled` if the event was consumed.
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        let _ = event;
        EventResult::Ignored
    }

    /// Handle a keyboard event.  Returns `Handled` if the event was consumed.
    fn handle_keyboard(&mut self, event: &KeyboardEvent) -> EventResult {
        let _ = event;
        EventResult::Ignored
    }

    /// Handle a mouse-move event.
    fn handle_mouse_move(&mut self, event: &MouseMoveEvent) -> EventResult {
        let _ = event;
        EventResult::Ignored
    }

    /// Handle a scroll event.
    fn handle_scroll(&mut self, event: &ScrollEvent) -> EventResult {
        let _ = event;
        EventResult::Ignored
    }

    /// Whether this element currently has focus.
    fn is_focused(&self) -> bool;

    /// Set focus state for this element.
    fn set_focused(&mut self, focused: bool);

    /// Hit-test: does this element contain the given logical point?
    fn contains_point(&self, x: f32, y: f32) -> bool;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_click_event_fields() {
        let evt = MouseClickEvent {
            button: MouseButton::Left,
            state: ElementState::Pressed,
            x: 10.0,
            y: 20.0,
            click_count: 2,
        };
        assert_eq!(evt.button, MouseButton::Left);
        assert_eq!(evt.state, ElementState::Pressed);
        assert!((evt.x - 10.0).abs() < f32::EPSILON);
        assert!((evt.y - 20.0).abs() < f32::EPSILON);
        assert_eq!(evt.click_count, 2);
    }

    #[test]
    fn event_result_matching() {
        let handled = EventResult::Handled;
        let ignored = EventResult::Ignored;
        assert_eq!(handled, EventResult::Handled);
        assert_eq!(ignored, EventResult::Ignored);
        assert_ne!(handled, ignored);
    }

    #[test]
    fn modifiers_default_all_false() {
        let m = Modifiers::default();
        assert!(!m.shift);
        assert!(!m.ctrl);
        assert!(!m.alt);
        assert!(!m.meta);
    }

    #[test]
    fn key_code_variants() {
        // Ensure each named variant is distinct.
        let keys = [
            KeyCode::Tab,
            KeyCode::Enter,
            KeyCode::Escape,
            KeyCode::Space,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::KeyA,
            KeyCode::KeyC,
            KeyCode::KeyV,
            KeyCode::KeyX,
            KeyCode::KeyZ,
            KeyCode::Other(0),
        ];
        // All pairs should be distinct.
        for (i, a) in keys.iter().enumerate() {
            for (j, b) in keys.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "KeyCode variants at {i} and {j} should differ");
                }
            }
        }
    }

    #[test]
    fn keyboard_event_with_text() {
        let evt = KeyboardEvent {
            key: KeyCode::KeyA,
            state: ElementState::Pressed,
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            text: Some("A".to_string()),
        };
        assert_eq!(evt.key, KeyCode::KeyA);
        assert_eq!(evt.text.as_deref(), Some("A"));
        assert!(evt.modifiers.shift);
    }

    #[test]
    fn scroll_event_fields() {
        let evt = ScrollEvent {
            x: 5.0,
            y: 6.0,
            delta_x: -1.0,
            delta_y: 3.5,
        };
        assert!((evt.delta_x - (-1.0)).abs() < f32::EPSILON);
        assert!((evt.delta_y - 3.5).abs() < f32::EPSILON);
    }
}
