//! Undo/Redo system for text editing operations.
//!
//! Phase 6.8: Implements a comprehensive undo/redo stack with operation grouping,
//! size limits, and support for text insertion, deletion, and selection changes.

use crate::layout::Selection;
use std::time::{Duration, Instant};

/// Maximum number of undo operations to keep in the stack.
const DEFAULT_UNDO_LIMIT: usize = 1000;

/// Time threshold for grouping consecutive typing operations (in milliseconds).
const TYPING_GROUP_THRESHOLD_MS: u64 = 500;

/// A single undoable/redoable operation.
#[derive(Debug, Clone, PartialEq)]
pub enum TextOperation {
    /// Insert text at a position.
    Insert {
        /// Byte offset where text was inserted.
        offset: usize,
        /// The text that was inserted.
        text: String,
        /// Selection before the operation.
        selection_before: Selection,
        /// Selection after the operation.
        selection_after: Selection,
    },
    /// Delete text from a range.
    Delete {
        /// Byte offset where deletion started.
        offset: usize,
        /// The text that was deleted.
        text: String,
        /// Selection before the operation.
        selection_before: Selection,
        /// Selection after the operation.
        selection_after: Selection,
    },
    /// Replace text in a range (used for paste, replace operations).
    Replace {
        /// Byte offset where replacement started.
        offset: usize,
        /// The text that was removed.
        old_text: String,
        /// The text that was inserted.
        new_text: String,
        /// Selection before the operation.
        selection_before: Selection,
        /// Selection after the operation.
        selection_after: Selection,
    },
}

impl TextOperation {
    /// Get the selection state after this operation.
    pub fn selection_after(&self) -> &Selection {
        match self {
            TextOperation::Insert {
                selection_after, ..
            } => selection_after,
            TextOperation::Delete {
                selection_after, ..
            } => selection_after,
            TextOperation::Replace {
                selection_after, ..
            } => selection_after,
        }
    }

    /// Get the selection state before this operation.
    pub fn selection_before(&self) -> &Selection {
        match self {
            TextOperation::Insert {
                selection_before, ..
            } => selection_before,
            TextOperation::Delete {
                selection_before, ..
            } => selection_before,
            TextOperation::Replace {
                selection_before, ..
            } => selection_before,
        }
    }

    /// Check if this operation can be grouped with another operation.
    ///
    /// Operations can be grouped if they are consecutive insertions or deletions
    /// at adjacent positions.
    pub fn can_group_with(&self, other: &TextOperation) -> bool {
        match (self, other) {
            // Group consecutive insertions at the same position
            (
                TextOperation::Insert {
                    offset: offset1,
                    text: text1,
                    ..
                },
                TextOperation::Insert {
                    offset: offset2, ..
                },
            ) => {
                // Check if the second insertion is right after the first
                *offset2 == *offset1 + text1.len()
            }
            // Group consecutive deletions (backspace)
            (
                TextOperation::Delete {
                    offset: offset1, ..
                },
                TextOperation::Delete {
                    offset: offset2,
                    text: text2,
                    ..
                },
            ) => {
                // Check if deleting backwards consecutively
                *offset2 + text2.len() == *offset1
            }
            _ => false,
        }
    }

    /// Merge another operation into this one (for grouping).
    ///
    /// Returns a new merged operation if successful.
    pub fn merge_with(&self, other: &TextOperation) -> Option<TextOperation> {
        match (self, other) {
            // Merge consecutive insertions
            (
                TextOperation::Insert {
                    offset,
                    text: text1,
                    selection_before,
                    ..
                },
                TextOperation::Insert {
                    text: text2,
                    selection_after,
                    ..
                },
            ) => {
                let mut merged_text = text1.clone();
                merged_text.push_str(text2);
                Some(TextOperation::Insert {
                    offset: *offset,
                    text: merged_text,
                    selection_before: selection_before.clone(),
                    selection_after: selection_after.clone(),
                })
            }
            // Merge consecutive deletions (backspace)
            (
                TextOperation::Delete {
                    offset: offset1,
                    text: text1,
                    selection_after,
                    ..
                },
                TextOperation::Delete {
                    text: text2,
                    selection_before,
                    ..
                },
            ) => {
                let mut merged_text = text2.clone();
                merged_text.push_str(text1);
                Some(TextOperation::Delete {
                    offset: offset1 - text2.len(),
                    text: merged_text,
                    selection_before: selection_before.clone(),
                    selection_after: selection_after.clone(),
                })
            }
            _ => None,
        }
    }
}

/// A group of operations that should be undone/redone together.
#[derive(Debug, Clone)]
struct OperationGroup {
    /// The operations in this group.
    operations: Vec<TextOperation>,
    /// Timestamp when this group was created.
    timestamp: Instant,
}

impl OperationGroup {
    /// Create a new operation group with a single operation.
    fn new(operation: TextOperation) -> Self {
        Self {
            operations: vec![operation],
            timestamp: Instant::now(),
        }
    }

    /// Try to add an operation to this group.
    ///
    /// Returns true if the operation was added, false if it should start a new group.
    fn try_add(&mut self, operation: TextOperation) -> bool {
        // Check time threshold
        let elapsed = self.timestamp.elapsed();
        if elapsed > Duration::from_millis(TYPING_GROUP_THRESHOLD_MS) {
            return false;
        }

        // Check if we can group with the last operation
        if let Some(last_op) = self.operations.last() {
            if last_op.can_group_with(&operation) {
                // Try to merge with the last operation
                if let Some(merged) = last_op.merge_with(&operation) {
                    // Replace the last operation with the merged one
                    let len = self.operations.len();
                    self.operations[len - 1] = merged;
                    return true;
                }
            }
        }

        false
    }

    /// Get all operations in this group.
    fn operations(&self) -> &[TextOperation] {
        &self.operations
    }
}

/// Undo/Redo stack for text editing operations.
#[derive(Debug)]
pub struct UndoStack {
    /// Stack of operation groups that can be undone.
    undo_stack: Vec<OperationGroup>,
    /// Stack of operation groups that can be redone.
    redo_stack: Vec<OperationGroup>,
    /// Maximum number of undo operations to keep.
    limit: usize,
    /// Whether to group consecutive operations.
    group_operations: bool,
}

impl UndoStack {
    /// Create a new undo stack with default settings.
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_UNDO_LIMIT)
    }

    /// Create a new undo stack with a custom limit.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            limit,
            group_operations: true,
        }
    }

    /// Enable or disable operation grouping.
    pub fn set_grouping(&mut self, enabled: bool) {
        self.group_operations = enabled;
    }

    /// Push a new operation onto the undo stack.
    ///
    /// This clears the redo stack and may group the operation with the previous one.
    pub fn push(&mut self, operation: TextOperation) {
        // Clear redo stack when a new operation is performed
        self.redo_stack.clear();

        // Try to group with the last operation if grouping is enabled
        if self.group_operations {
            if let Some(last_group) = self.undo_stack.last_mut() {
                if last_group.try_add(operation.clone()) {
                    return;
                }
            }
        }

        // Create a new group for this operation
        let group = OperationGroup::new(operation);
        self.undo_stack.push(group);

        // Enforce size limit
        if self.undo_stack.len() > self.limit {
            self.undo_stack.remove(0);
        }
    }

    /// Undo the last operation.
    ///
    /// Returns the operations to apply (in reverse order) to undo the change.
    pub fn undo(&mut self) -> Option<Vec<TextOperation>> {
        if let Some(group) = self.undo_stack.pop() {
            let operations = group.operations().to_vec();
            self.redo_stack.push(group);
            Some(operations)
        } else {
            None
        }
    }

    /// Redo the last undone operation.
    ///
    /// Returns the operations to apply to redo the change.
    pub fn redo(&mut self) -> Option<Vec<TextOperation>> {
        if let Some(group) = self.redo_stack.pop() {
            let operations = group.operations().to_vec();
            self.undo_stack.push(group);
            Some(operations)
        } else {
            None
        }
    }

    /// Check if there are operations that can be undone.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if there are operations that can be redone.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Clear all undo and redo history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Get the number of operations in the undo stack.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Get the number of operations in the redo stack.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Set the maximum number of undo operations to keep.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        // Trim the undo stack if necessary
        while self.undo_stack.len() > limit {
            self.undo_stack.remove(0);
        }
    }

    /// Get the current undo limit.
    pub fn limit(&self) -> usize {
        self.limit
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_undo_stack_creation() {
        let stack = UndoStack::new();
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.undo_count(), 0);
        assert_eq!(stack.redo_count(), 0);
    }

    #[test]
    fn test_push_and_undo() {
        let mut stack = UndoStack::new();

        let op = TextOperation::Insert {
            offset: 0,
            text: "Hello".to_string(),
            selection_before: Selection::collapsed(0),
            selection_after: Selection::collapsed(5),
        };

        stack.push(op.clone());
        assert!(stack.can_undo());
        assert!(!stack.can_redo());

        let undone = stack.undo().unwrap();
        assert_eq!(undone.len(), 1);
        assert_eq!(undone[0], op);
        assert!(!stack.can_undo());
        assert!(stack.can_redo());
    }

    #[test]
    fn test_redo() {
        let mut stack = UndoStack::new();

        let op = TextOperation::Insert {
            offset: 0,
            text: "Hello".to_string(),
            selection_before: Selection::collapsed(0),
            selection_after: Selection::collapsed(5),
        };

        stack.push(op.clone());
        stack.undo();

        let redone = stack.redo().unwrap();
        assert_eq!(redone.len(), 1);
        assert_eq!(redone[0], op);
        assert!(stack.can_undo());
        assert!(!stack.can_redo());
    }

    #[test]
    fn test_push_clears_redo() {
        let mut stack = UndoStack::new();

        let op1 = TextOperation::Insert {
            offset: 0,
            text: "Hello".to_string(),
            selection_before: Selection::collapsed(0),
            selection_after: Selection::collapsed(5),
        };

        let op2 = TextOperation::Insert {
            offset: 5,
            text: " World".to_string(),
            selection_before: Selection::collapsed(5),
            selection_after: Selection::collapsed(11),
        };

        stack.push(op1);
        stack.undo();
        assert!(stack.can_redo());

        stack.push(op2);
        assert!(!stack.can_redo());
    }

    #[test]
    fn test_operation_grouping() {
        let mut stack = UndoStack::new();

        // Insert consecutive characters
        stack.push(TextOperation::Insert {
            offset: 0,
            text: "H".to_string(),
            selection_before: Selection::collapsed(0),
            selection_after: Selection::collapsed(1),
        });

        stack.push(TextOperation::Insert {
            offset: 1,
            text: "e".to_string(),
            selection_before: Selection::collapsed(1),
            selection_after: Selection::collapsed(2),
        });

        stack.push(TextOperation::Insert {
            offset: 2,
            text: "l".to_string(),
            selection_before: Selection::collapsed(2),
            selection_after: Selection::collapsed(3),
        });

        // Should be grouped into one operation
        assert_eq!(stack.undo_count(), 1);

        let undone = stack.undo().unwrap();
        assert_eq!(undone.len(), 1);

        // The grouped operation should contain the merged text
        if let TextOperation::Insert { text, .. } = &undone[0] {
            assert_eq!(text, "Hel");
        } else {
            panic!("Expected Insert operation");
        }
    }

    #[test]
    fn test_operation_grouping_disabled() {
        let mut stack = UndoStack::new();
        stack.set_grouping(false);

        stack.push(TextOperation::Insert {
            offset: 0,
            text: "H".to_string(),
            selection_before: Selection::collapsed(0),
            selection_after: Selection::collapsed(1),
        });

        stack.push(TextOperation::Insert {
            offset: 1,
            text: "e".to_string(),
            selection_before: Selection::collapsed(1),
            selection_after: Selection::collapsed(2),
        });

        // Should NOT be grouped
        assert_eq!(stack.undo_count(), 2);
    }

    #[test]
    fn test_delete_grouping() {
        let mut stack = UndoStack::new();

        // Simulate backspace operations
        stack.push(TextOperation::Delete {
            offset: 2,
            text: "l".to_string(),
            selection_before: Selection::collapsed(3),
            selection_after: Selection::collapsed(2),
        });

        stack.push(TextOperation::Delete {
            offset: 1,
            text: "e".to_string(),
            selection_before: Selection::collapsed(2),
            selection_after: Selection::collapsed(1),
        });

        // Should be grouped
        assert_eq!(stack.undo_count(), 1);

        let undone = stack.undo().unwrap();
        if let TextOperation::Delete { text, .. } = &undone[0] {
            assert_eq!(text, "el");
        } else {
            panic!("Expected Delete operation");
        }
    }

    #[test]
    fn test_size_limit() {
        let mut stack = UndoStack::with_limit(3);
        stack.set_grouping(false); // Disable grouping for this test

        for i in 0..5 {
            stack.push(TextOperation::Insert {
                offset: i,
                text: "x".to_string(),
                selection_before: Selection::collapsed(i),
                selection_after: Selection::collapsed(i + 1),
            });
        }

        // Should only keep the last 3 operations
        assert_eq!(stack.undo_count(), 3);
    }

    #[test]
    fn test_clear() {
        let mut stack = UndoStack::new();

        stack.push(TextOperation::Insert {
            offset: 0,
            text: "Hello".to_string(),
            selection_before: Selection::collapsed(0),
            selection_after: Selection::collapsed(5),
        });

        stack.undo();

        assert!(stack.can_redo());

        stack.clear();

        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
    }

    #[test]
    fn test_can_group_with() {
        let op1 = TextOperation::Insert {
            offset: 0,
            text: "H".to_string(),
            selection_before: Selection::collapsed(0),
            selection_after: Selection::collapsed(1),
        };

        let op2 = TextOperation::Insert {
            offset: 1,
            text: "e".to_string(),
            selection_before: Selection::collapsed(1),
            selection_after: Selection::collapsed(2),
        };

        let op3 = TextOperation::Insert {
            offset: 10,
            text: "x".to_string(),
            selection_before: Selection::collapsed(10),
            selection_after: Selection::collapsed(11),
        };

        assert!(op1.can_group_with(&op2));
        assert!(!op1.can_group_with(&op3));
    }
}
