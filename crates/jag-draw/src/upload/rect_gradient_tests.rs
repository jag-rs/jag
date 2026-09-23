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
