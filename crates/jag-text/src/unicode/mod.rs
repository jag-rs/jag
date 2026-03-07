//! Unicode utilities for detir-text.
//!
//! Phase 1.4 focuses on grapheme cluster segmentation and
//! cursor-safe navigation over extended grapheme clusters
//! (including combining marks and emoji/ZWJ sequences).

pub mod graphemes;
pub mod properties;

pub use graphemes::{
    GraphemeCluster, grapheme_clusters, is_grapheme_boundary, next_grapheme_boundary,
    prev_grapheme_boundary,
};
