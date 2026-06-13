//! GPU upload: extract a `DisplayList` into vertex/index buffers and per-type
//! draw lists. Split into focused submodules; the public surface is preserved
//! via the re-exports below (`lib.rs` does `pub use upload::*;`).

mod display_list;
mod gradients;
mod shapes;
mod tessellate;
mod types;
mod unified;
mod verts;

pub use display_list::upload_display_list;
pub use gradients::{lerp_color, sample_gradient_stops};
pub use types::{
    ExtractedExternalTextureDraw, ExtractedImageDraw, ExtractedSvgDraw, ExtractedTextDraw,
    GpuScene, SolidBatch, TransparentBatch, UnifiedSceneData, Vertex,
};
pub use unified::upload_display_list_unified;

#[cfg(test)]
mod path_clip_tests {
    use super::tessellate::clip_triangle_to_rect;
    use crate::scene::Rect;

    fn rect() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        }
    }

    #[test]
    fn fully_inside_triangle_is_unchanged() {
        let poly = clip_triangle_to_rect([[1.0, 1.0], [8.0, 1.0], [4.0, 8.0]], rect());
        assert_eq!(poly.len(), 3);
    }

    #[test]
    fn fully_outside_triangle_is_dropped() {
        let poly = clip_triangle_to_rect([[20.0, 20.0], [30.0, 20.0], [25.0, 30.0]], rect());
        assert!(poly.is_empty());
    }

    #[test]
    fn straddling_triangle_is_clipped_within_bounds() {
        // Triangle pokes out the top and sides; result must stay inside the rect.
        let poly = clip_triangle_to_rect([[-5.0, -5.0], [15.0, -5.0], [5.0, 8.0]], rect());
        assert!(poly.len() >= 3);
        for p in &poly {
            assert!(p[0] >= -0.01 && p[0] <= 10.01, "x out of clip: {}", p[0]);
            assert!(p[1] >= -0.01 && p[1] <= 10.01, "y out of clip: {}", p[1]);
        }
    }
}
