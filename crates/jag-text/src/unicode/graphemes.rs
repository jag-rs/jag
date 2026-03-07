use core::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// A Unicode extended grapheme cluster within a UTF-8 string.
///
/// The range is expressed in byte offsets into the original string.
#[derive(Debug, Clone)]
pub struct GraphemeCluster {
    pub range: Range<usize>,
}

/// Compute all grapheme clusters for the given text in scan order.
pub fn grapheme_clusters(text: &str) -> Vec<GraphemeCluster> {
    text.grapheme_indices(true)
        .map(|(byte_idx, g)| GraphemeCluster {
            range: byte_idx..byte_idx + g.len(),
        })
        .collect()
}

/// Returns `true` if `offset` is at a grapheme cluster boundary.
///
/// `offset` is clamped to `0..=text.len()`.
pub fn is_grapheme_boundary(text: &str, offset: usize) -> bool {
    let len = text.len();
    let offset = offset.min(len);
    if offset == 0 || offset == len {
        return true;
    }

    for (byte_idx, _) in text.grapheme_indices(true) {
        if byte_idx == offset {
            return true;
        }
        if byte_idx > offset {
            break;
        }
    }
    false
}

/// Find the previous grapheme cluster boundary before `offset`.
///
/// If `offset` lies inside a grapheme cluster, this returns the start
/// of that cluster. If `offset` is exactly at a cluster boundary,
/// this returns the start of the previous cluster. Returns `None`
/// when there is no previous cluster.
pub fn prev_grapheme_boundary(text: &str, offset: usize) -> Option<usize> {
    let len = text.len();
    let offset = offset.min(len);
    if offset == 0 || text.is_empty() {
        return None;
    }

    let mut prev_start: Option<usize> = None;
    for (start, g) in text.grapheme_indices(true) {
        let end = start + g.len();
        if offset <= start {
            // We've reached the cluster that starts at or after offset.
            break;
        }
        // offset > start here. If offset is within this cluster, or
        // beyond its end, this cluster is the previous one.
        if offset <= end {
            prev_start = Some(start);
            break;
        }
        prev_start = Some(start);
    }
    prev_start
}

/// Find the next grapheme cluster boundary after `offset`.
///
/// If `offset` lies inside a grapheme cluster, this returns the end
/// of that cluster. If `offset` is exactly at a cluster boundary,
/// this returns the end of the next cluster. Returns `None` when
/// there is no next cluster.
pub fn next_grapheme_boundary(text: &str, offset: usize) -> Option<usize> {
    let len = text.len();
    let offset = offset.min(len);
    if offset >= len || text.is_empty() {
        return None;
    }

    for (start, g) in text.grapheme_indices(true) {
        let end = start + g.len();
        if offset < end {
            return Some(end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ascii_graphemes() {
        let text = "abc";
        let clusters = grapheme_clusters(text);
        assert_eq!(clusters.len(), 3);
        assert_eq!(clusters[0].range, 0..1);
        assert_eq!(clusters[1].range, 1..2);
        assert_eq!(clusters[2].range, 2..3);
    }

    #[test]
    fn combining_mark_stays_with_base() {
        let text = "a\u{0301}"; // a + COMBINING ACUTE
        let clusters = grapheme_clusters(text);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].range, 0..text.len());
        // Cursor navigation should treat the combined glyph as one unit.
        assert_eq!(prev_grapheme_boundary(text, text.len()), Some(0));
        assert_eq!(next_grapheme_boundary(text, 0), Some(text.len()));
    }

    #[test]
    fn emoji_zwj_sequence_is_single_cluster() {
        // Family: man, woman, girl, boy (uses ZWJ sequences)
        let text = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
        let clusters = grapheme_clusters(text);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].range, 0..text.len());
    }
}
