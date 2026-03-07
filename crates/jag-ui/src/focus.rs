//! Generic focus manager using opaque `FocusId` identifiers.
//!
//! Provides centralized focus tracking and Tab / Shift-Tab navigation
//! with tabindex ordering.  Elements with tabindex -1 are registered
//! but excluded from keyboard navigation.

use std::collections::HashSet;

/// Opaque identifier for a focusable element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusId(pub u64);

/// Direction for keyboard-based focus navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    /// Tab — move to next focusable element.
    Forward,
    /// Shift-Tab — move to previous focusable element.
    Backward,
}

/// Outcome of a [`FocusManager::navigate`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusResult {
    /// Focus moved to a new element.
    Moved(FocusId),
    /// Focus wrapped around to the beginning / end.
    Wrapped(FocusId),
    /// No focusable elements available.
    NoFocusableElements,
    /// Only one focusable element and it is already focused.
    Unchanged,
}

/// Centralized focus manager.
///
/// Call [`register`](Self::register) for each focusable element, then
/// [`rebuild_order`](Self::rebuild_order) to sort by tabindex before
/// navigating.
#[derive(Debug)]
pub struct FocusManager {
    /// Currently focused element, if any.
    current: Option<FocusId>,
    /// Whether the focus ring should be drawn (keyboard navigation mode).
    focus_visible: bool,
    /// Registered elements with their tabindex, in insertion order.
    registered: Vec<(FocusId, i32)>,
    /// Sorted navigable order (excludes tabindex -1).
    order: Vec<FocusId>,
    /// Set of registered ids for quick lookup.
    id_set: HashSet<FocusId>,
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusManager {
    /// Create an empty focus manager with no focused element.
    pub fn new() -> Self {
        Self {
            current: None,
            focus_visible: false,
            registered: Vec::new(),
            order: Vec::new(),
            id_set: HashSet::new(),
        }
    }

    /// The currently focused element, if any.
    pub fn current(&self) -> Option<FocusId> {
        self.current
    }

    /// Whether the focus ring should be visible (set by keyboard navigation).
    pub fn is_focus_visible(&self) -> bool {
        self.focus_visible
    }

    /// Explicitly set focus-ring visibility.
    pub fn set_focus_visible(&mut self, visible: bool) {
        self.focus_visible = visible;
    }

    /// Programmatically focus a specific element.
    pub fn set_focus(&mut self, id: FocusId) {
        self.current = Some(id);
    }

    /// Clear focus from all elements.
    pub fn clear_focus(&mut self) {
        self.current = None;
    }

    /// Register a focusable element with a tabindex.
    ///
    /// * tabindex >= 0: participates in keyboard navigation.
    /// * tabindex  < 0: focusable via `set_focus` only, skipped by `navigate`.
    ///
    /// If the element is already registered, its tabindex is updated.
    pub fn register(&mut self, id: FocusId, tabindex: i32) {
        if self.id_set.contains(&id) {
            // Update tabindex for existing entry.
            if let Some(entry) = self.registered.iter_mut().find(|(eid, _)| *eid == id) {
                entry.1 = tabindex;
            }
        } else {
            self.registered.push((id, tabindex));
            self.id_set.insert(id);
        }
    }

    /// Remove a focusable element.  If it was focused, focus is cleared.
    pub fn unregister(&mut self, id: FocusId) {
        self.registered.retain(|(eid, _)| *eid != id);
        self.id_set.remove(&id);
        self.order.retain(|eid| *eid != id);
        if self.current == Some(id) {
            self.current = None;
        }
    }

    /// Rebuild the internal navigation order from registered elements.
    ///
    /// Must be called after registering or unregistering elements, and
    /// before calling [`navigate`](Self::navigate).
    ///
    /// Ordering rules:
    /// 1. Elements with tabindex < 0 are excluded.
    /// 2. Elements with tabindex > 0 come first, sorted ascending.
    /// 3. Elements with tabindex == 0 follow, in registration (document) order.
    pub fn rebuild_order(&mut self) {
        // Collect navigable entries preserving registration order index.
        let mut navigable: Vec<(usize, FocusId, i32)> = self
            .registered
            .iter()
            .enumerate()
            .filter(|(_, (_, ti))| *ti >= 0)
            .map(|(idx, (id, ti))| (idx, *id, *ti))
            .collect();

        // Stable sort: positive tabindex first (ascending), then tabindex-0
        // in document (registration) order.
        navigable.sort_by(|a, b| {
            match (a.2, b.2) {
                (ta, tb) if ta > 0 && tb > 0 => ta.cmp(&tb),
                (ta, _) if ta > 0 => std::cmp::Ordering::Less,
                (_, tb) if tb > 0 => std::cmp::Ordering::Greater,
                // Both tabindex 0 — preserve registration order.
                _ => a.0.cmp(&b.0),
            }
        });

        self.order = navigable.into_iter().map(|(_, id, _)| id).collect();
    }

    /// Navigate focus forward or backward.
    ///
    /// Automatically enables `focus_visible`.
    pub fn navigate(&mut self, direction: FocusDirection) -> FocusResult {
        if self.order.is_empty() {
            return FocusResult::NoFocusableElements;
        }

        self.focus_visible = true;

        let len = self.order.len();
        let current_pos = self
            .current
            .and_then(|id| self.order.iter().position(|eid| *eid == id));

        let (new_pos, wrapped) = match (current_pos, direction) {
            (None, FocusDirection::Forward) => (0, false),
            (None, FocusDirection::Backward) => (len - 1, false),
            (Some(pos), FocusDirection::Forward) => {
                if pos + 1 >= len {
                    (0, true)
                } else {
                    (pos + 1, false)
                }
            }
            (Some(pos), FocusDirection::Backward) => {
                if pos == 0 {
                    (len - 1, true)
                } else {
                    (pos - 1, false)
                }
            }
        };

        let new_id = self.order[new_pos];

        // Single element that is already focused.
        if Some(new_id) == self.current && len == 1 {
            return FocusResult::Unchanged;
        }

        self.current = Some(new_id);

        if wrapped {
            FocusResult::Wrapped(new_id)
        } else {
            FocusResult::Moved(new_id)
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
    fn new_manager_has_no_focus() {
        let fm = FocusManager::new();
        assert!(fm.current().is_none());
        assert!(!fm.is_focus_visible());
    }

    #[test]
    fn register_and_navigate_forward() {
        let mut fm = FocusManager::new();
        fm.register(FocusId(1), 0);
        fm.register(FocusId(2), 0);
        fm.register(FocusId(3), 0);
        fm.rebuild_order();

        let r1 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r1, FocusResult::Moved(FocusId(1)));
        assert_eq!(fm.current(), Some(FocusId(1)));

        let r2 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r2, FocusResult::Moved(FocusId(2)));

        let r3 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r3, FocusResult::Moved(FocusId(3)));
    }

    #[test]
    fn navigate_backward_wraps() {
        let mut fm = FocusManager::new();
        fm.register(FocusId(1), 0);
        fm.register(FocusId(2), 0);
        fm.rebuild_order();

        // Focus the first element.
        fm.set_focus(FocusId(1));

        let r = fm.navigate(FocusDirection::Backward);
        assert_eq!(r, FocusResult::Wrapped(FocusId(2)));
        assert_eq!(fm.current(), Some(FocusId(2)));
    }

    #[test]
    fn tabindex_negative_skipped() {
        let mut fm = FocusManager::new();
        fm.register(FocusId(1), 0);
        fm.register(FocusId(2), -1); // should be skipped
        fm.register(FocusId(3), 0);
        fm.rebuild_order();

        let r1 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r1, FocusResult::Moved(FocusId(1)));

        let r2 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r2, FocusResult::Moved(FocusId(3)));

        // Wrap back to first.
        let r3 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r3, FocusResult::Wrapped(FocusId(1)));
    }

    #[test]
    fn set_and_clear_focus() {
        let mut fm = FocusManager::new();
        fm.register(FocusId(7), 0);
        fm.rebuild_order();

        fm.set_focus(FocusId(7));
        assert_eq!(fm.current(), Some(FocusId(7)));

        fm.clear_focus();
        assert!(fm.current().is_none());
    }

    #[test]
    fn unregister_removes_from_order() {
        let mut fm = FocusManager::new();
        fm.register(FocusId(1), 0);
        fm.register(FocusId(2), 0);
        fm.register(FocusId(3), 0);
        fm.rebuild_order();

        fm.set_focus(FocusId(2));
        fm.unregister(FocusId(2));

        // Focus should be cleared.
        assert!(fm.current().is_none());

        // Navigation should skip the removed element.
        let r1 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r1, FocusResult::Moved(FocusId(1)));

        let r2 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r2, FocusResult::Moved(FocusId(3)));
    }

    #[test]
    fn navigate_empty_returns_no_focusable() {
        let mut fm = FocusManager::new();
        assert_eq!(
            fm.navigate(FocusDirection::Forward),
            FocusResult::NoFocusableElements,
        );
    }

    #[test]
    fn single_element_unchanged() {
        let mut fm = FocusManager::new();
        fm.register(FocusId(1), 0);
        fm.rebuild_order();

        fm.set_focus(FocusId(1));
        let r = fm.navigate(FocusDirection::Forward);
        assert_eq!(r, FocusResult::Unchanged);
    }

    #[test]
    fn positive_tabindex_ordered_first() {
        let mut fm = FocusManager::new();
        fm.register(FocusId(10), 0);
        fm.register(FocusId(20), 2);
        fm.register(FocusId(30), 1);
        fm.rebuild_order();

        // Positive tabindex elements first (sorted ascending), then tabindex-0.
        let r1 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r1, FocusResult::Moved(FocusId(30))); // tabindex 1

        let r2 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r2, FocusResult::Moved(FocusId(20))); // tabindex 2

        let r3 = fm.navigate(FocusDirection::Forward);
        assert_eq!(r3, FocusResult::Moved(FocusId(10))); // tabindex 0
    }
}
