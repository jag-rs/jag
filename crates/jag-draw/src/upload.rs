//! GPU upload: extract a `DisplayList` into vertex/index buffers and per-type
//! draw lists. Split into focused submodules; the public surface is preserved
//! via the re-exports below (`lib.rs` does `pub use upload::*;`).

mod display_list;
mod gradients;
mod path_clip;
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
mod shape_aa_tests {
    use super::shapes::{push_rounded_rect_aa, push_rounded_rect_stroke_aa};
    use crate::scene::{Rect, RoundedRadii, RoundedRect, Stroke, Transform2D};

    fn sample_rrect() -> RoundedRect {
        RoundedRect {
            rect: Rect {
                x: 10.0,
                y: 20.0,
                w: 80.0,
                h: 40.0,
            },
            radii: RoundedRadii {
                tl: 8.0,
                tr: 8.0,
                br: 8.0,
                bl: 8.0,
            },
        }
    }

    #[test]
    fn rounded_rect_aa_emits_transparent_edge_vertices() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_rounded_rect_aa(
            &mut vertices,
            &mut indices,
            sample_rrect(),
            [0.2, 0.4, 0.6, 1.0],
            3.0,
            Transform2D::identity(),
        );

        assert!(!indices.is_empty());
        assert!(vertices.iter().any(|v| v.color[3] == 1.0));
        assert!(vertices.iter().any(|v| v.color[3] == 0.0));
    }

    #[test]
    fn rounded_stroke_aa_emits_inner_and_outer_fringes() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_rounded_rect_stroke_aa(
            &mut vertices,
            &mut indices,
            sample_rrect(),
            Stroke { width: 1.0 },
            [0.2, 0.4, 0.6, 1.0],
            3.0,
            Transform2D::identity(),
        );

        assert!(!indices.is_empty());
        assert!(vertices.iter().any(|v| v.color[3] == 1.0));
        assert!(vertices.iter().any(|v| v.color[3] == 0.0));
    }
}

#[cfg(test)]
mod path_clip_tests {
    use super::path_clip::{clip_triangle_to_rect, clip_triangle_to_rounded};
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

    // Signed distance to a rounded rect (negative = inside), matching the GPU SDF.
    fn rrect_sdf(p: [f32; 2], r: Rect, rad: f32) -> f32 {
        let cx = r.x + r.w * 0.5;
        let cy = r.y + r.h * 0.5;
        let qx = (p[0] - cx).abs() - (r.w * 0.5 - rad);
        let qy = (p[1] - cy).abs() - (r.h * 0.5 - rad);
        let ox = qx.max(0.0);
        let oy = qy.max(0.0);
        qx.max(qy).min(0.0) + (ox * ox + oy * oy).sqrt() - rad
    }

    #[test]
    fn rounded_clip_cuts_the_corner() {
        // A triangle filling the top-left corner of a rounded rect: the sharp
        // corner (0,0) lies outside the radius-4 arc, so the clipped polygon must
        // stay inside the rounded boundary and must NOT include the rect corner.
        let r = rect();
        let radii = [4.0; 4];
        let poly = clip_triangle_to_rounded([[0.0, 0.0], [5.0, 0.0], [0.0, 5.0]], r, radii);
        assert!(poly.len() >= 3, "expected a non-empty clipped polygon");
        for p in &poly {
            assert!(
                rrect_sdf(*p, r, radii[0]) <= 0.2,
                "vertex {p:?} is outside the rounded rect (sdf {})",
                rrect_sdf(*p, r, radii[0])
            );
            assert!(
                p[0] > 0.05 || p[1] > 0.05,
                "vertex {p:?} sits at the cut-off sharp corner"
            );
        }
    }

    #[test]
    fn rounded_clip_leaves_interior_triangle_whole() {
        // Fully inside the inner (un-rounded) region → unchanged 3-vertex triangle.
        let poly = clip_triangle_to_rounded([[5.0, 5.0], [6.0, 5.0], [5.0, 6.0]], rect(), [4.0; 4]);
        assert_eq!(poly.len(), 3);
    }
}
