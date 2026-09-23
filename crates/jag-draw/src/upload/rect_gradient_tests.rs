use super::*;

const BLACK_40: [f32; 4] = [0.0, 0.0, 0.0, 0.4];
const CLEAR: [f32; 4] = [0.0; 4];

fn fill(start: [f32; 2], end: [f32; 2], stops: &[(f32, [f32; 4])]) -> Vec<Vertex> {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 60.0,
    };
    push_rect_linear_gradient(
        &mut vertices,
        &mut indices,
        rect,
        start,
        end,
        stops,
        Transform2D::identity(),
        0.0,
    );
    assert_eq!(indices.len() % 3, 0);
    vertices
}

/// Alpha of the vertex at `pos`.
fn alpha_at(vertices: &[Vertex], pos: [f32; 2]) -> f32 {
    vertices
        .iter()
        .find(|v| (v.pos[0] - pos[0]).abs() < 1e-3 && (v.pos[1] - pos[1]).abs() < 1e-3)
        .unwrap_or_else(|| panic!("no vertex at {pos:?}"))
        .color[3]
}

#[test]
fn vertical_gradient_varies_along_y_not_x() {
    // `to top`: line from the bottom edge (start) to the top edge (end).
    let v = fill([50.0, 60.0], [50.0, 0.0], &[(0.0, BLACK_40), (1.0, CLEAR)]);
    assert!((alpha_at(&v, [0.0, 60.0]) - 0.4).abs() < 1e-4);
    assert!((alpha_at(&v, [100.0, 60.0]) - 0.4).abs() < 1e-4);
    assert!(alpha_at(&v, [0.0, 0.0]).abs() < 1e-4);
    assert!(alpha_at(&v, [100.0, 0.0]).abs() < 1e-4);
}

#[test]
fn middle_stop_gets_its_own_vertices() {
    let mid = [1.0, 0.0, 0.0, 1.0];
    let v = fill(
        [0.0, 30.0],
        [100.0, 30.0],
        &[(0.0, CLEAR), (0.5, mid), (1.0, CLEAR)],
    );
    assert_eq!(alpha_at(&v, [50.0, 0.0]), 1.0);
    assert_eq!(alpha_at(&v, [50.0, 60.0]), 1.0);
}

#[test]
fn rect_beyond_the_line_takes_end_colors() {
    // The line covers only x 25..75; the rest of the rect is flat end color.
    let v = fill([25.0, 30.0], [75.0, 30.0], &[(0.0, BLACK_40), (1.0, CLEAR)]);
    assert!((alpha_at(&v, [0.0, 0.0]) - 0.4).abs() < 1e-4);
    assert!(alpha_at(&v, [100.0, 0.0]).abs() < 1e-4);
    // The ramp runs only between the line's ends, not across the whole rect.
    assert!((alpha_at(&v, [25.0, 0.0]) - 0.4).abs() < 1e-4);
    assert!(alpha_at(&v, [75.0, 60.0]).abs() < 1e-4);
}

const RED: [f32; 4] = [0.863, 0.058, 0.058, 1.0];
const BLUE: [f32; 4] = [0.044, 0.223, 0.922, 1.0];

/// Largest sRGB-encoded error, halfway between neighbouring vertices along the
/// top edge, of linear-light blending (what the GPU does) against the CSS blend.
fn worst_midpoint_error(vertices: &[Vertex], line_len: f32) -> f32 {
    let mut xs: Vec<(f32, [f32; 4])> = vertices
        .iter()
        .filter(|v| v.pos[1].abs() < 1e-3)
        .map(|v| (v.pos[0], v.color))
        .collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    xs.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-4);
    assert!(xs.len() > 2, "the span should be split");
    xs.windows(2)
        .map(|pair| {
            let ((x0, c0), (x1, c1)) = (pair[0], pair[1]);
            let exact = lerp_color(RED, BLUE, 0.5 * (x0 + x1) / line_len);
            (0..3)
                .map(|i| (linear_to_srgb(0.5 * (c0[i] + c1[i])) - linear_to_srgb(exact[i])).abs())
                .fold(0.0, f32::max)
        })
        .fold(0.0, f32::max)
}

#[test]
fn rect_gradient_blends_in_srgb_within_half_an_output_step() {
    let v = fill([0.0, 30.0], [100.0, 30.0], &[(0.0, RED), (1.0, BLUE)]);
    assert!(worst_midpoint_error(&v, 100.0) <= HALF_OUTPUT_STEP);
}

#[test]
fn rounded_rect_gradient_blends_in_srgb_within_half_an_output_step() {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    let rrect = crate::scene::RoundedRect {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 60.0,
        },
        radii: crate::scene::RoundedRadii {
            tl: 0.0,
            tr: 0.0,
            br: 12.0,
            bl: 12.0,
        },
    };
    super::super::gradients::push_rounded_rect_linear_gradient(
        &mut vertices,
        &mut indices,
        rrect,
        [0.0, 30.0],
        [100.0, 30.0],
        &[(0.0, RED), (1.0, BLUE)],
        0.0,
        Transform2D::identity(),
    );
    assert_eq!(indices.len() % 3, 0);
    assert!(worst_midpoint_error(&vertices, 100.0) <= HALF_OUTPUT_STEP);
    for v in &vertices {
        assert!(
            v.pos[0] >= -1e-3 && v.pos[0] <= 100.001 && v.pos[1] >= -1e-3 && v.pos[1] <= 60.001
        );
    }
}

#[test]
fn hard_stop_switches_color_at_its_line() {
    // `red 50%, blue 50%`: left half flat red, right half flat blue.
    let v = fill(
        [0.0, 30.0],
        [100.0, 30.0],
        &[(0.0, RED), (0.5, RED), (0.5, BLUE), (1.0, BLUE)],
    );
    for vertex in &v {
        let expected = if vertex.pos[0] < 50.0 - 1e-3 {
            RED
        } else if vertex.pos[0] > 50.0 + 1e-3 {
            BLUE
        } else {
            continue;
        };
        let close = (0..4).all(|i| (vertex.color[i] - expected[i]).abs() < 1e-4);
        assert!(close, "at x={}: {:?}", vertex.pos[0], vertex.color);
    }
    // Both colors meet at x = 50, one band on each side.
    let at_line: Vec<_> = v
        .iter()
        .filter(|vertex| (vertex.pos[0] - 50.0).abs() < 1e-3)
        .map(|vertex| vertex.color)
        .collect();
    let near = |c: &[f32; 4], e: [f32; 4]| (0..4).all(|i| (c[i] - e[i]).abs() < 1e-4);
    assert!(at_line.iter().any(|c| near(c, RED)) && at_line.iter().any(|c| near(c, BLUE)));
}
