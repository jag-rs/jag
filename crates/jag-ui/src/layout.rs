//! Thin wrapper around [Taffy](https://docs.rs/taffy) for flexbox / grid
//! layout.
//!
//! Each layout node can optionally carry a [`FocusId`] so the layout tree
//! can be correlated with focusable UI elements.

use taffy::prelude::*;

use crate::focus::FocusId;

/// Layout tree backed by Taffy.
pub struct Layout {
    tree: TaffyTree<Option<FocusId>>,
}

impl std::fmt::Debug for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layout").finish_non_exhaustive()
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

impl Layout {
    /// Create an empty layout tree.
    pub fn new() -> Self {
        Self {
            tree: TaffyTree::new(),
        }
    }

    /// Add a leaf node with the given style and optional [`FocusId`].
    pub fn add_node(&mut self, style: Style, id: Option<FocusId>) -> NodeId {
        self.tree
            .new_leaf_with_context(style, id)
            .expect("taffy: failed to create leaf node")
    }

    /// Attach `child` as a child of `parent`.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) {
        self.tree
            .add_child(parent, child)
            .expect("taffy: failed to add child");
    }

    /// Compute layout for the subtree rooted at `root`.
    pub fn compute(&mut self, root: NodeId, available: Size<AvailableSpace>) {
        self.tree
            .compute_layout(root, available)
            .expect("taffy: layout computation failed");
    }

    /// Retrieve the computed layout for a node.
    pub fn get_layout(&self, node: NodeId) -> &taffy::tree::Layout {
        self.tree
            .layout(node)
            .expect("taffy: layout not computed for node")
    }

    /// Remove all nodes from the layout tree.
    pub fn clear(&mut self) {
        self.tree.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_node_and_get_layout() {
        let mut layout = Layout::new();
        let root = layout.add_node(
            Style {
                size: Size {
                    width: length(100.0),
                    height: length(50.0),
                },
                ..Default::default()
            },
            None,
        );
        layout.compute(root, Size::MAX_CONTENT);

        let result = layout.get_layout(root);
        assert!((result.size.width - 100.0).abs() < f32::EPSILON);
        assert!((result.size.height - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parent_child_layout() {
        let mut layout = Layout::new();

        let child = layout.add_node(
            Style {
                size: Size {
                    width: length(40.0),
                    height: length(20.0),
                },
                ..Default::default()
            },
            Some(FocusId(1)),
        );

        let parent = layout.add_node(
            Style {
                size: Size {
                    width: length(200.0),
                    height: length(100.0),
                },
                ..Default::default()
            },
            None,
        );

        layout.add_child(parent, child);
        layout.compute(parent, Size::MAX_CONTENT);

        let child_layout = layout.get_layout(child);
        assert!((child_layout.size.width - 40.0).abs() < f32::EPSILON);
        assert!((child_layout.size.height - 20.0).abs() < f32::EPSILON);

        // Child should be positioned at (0, 0) within parent by default.
        assert!((child_layout.location.x).abs() < f32::EPSILON);
        assert!((child_layout.location.y).abs() < f32::EPSILON);
    }

    #[test]
    fn clear_allows_reuse() {
        let mut layout = Layout::new();
        let _node = layout.add_node(Style::default(), None);
        layout.clear();

        // Should be able to add nodes again after clear.
        let root = layout.add_node(
            Style {
                size: Size {
                    width: length(10.0),
                    height: length(10.0),
                },
                ..Default::default()
            },
            None,
        );
        layout.compute(root, Size::MAX_CONTENT);
        let result = layout.get_layout(root);
        assert!((result.size.width - 10.0).abs() < f32::EPSILON);
    }
}
