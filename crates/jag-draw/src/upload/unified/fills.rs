use crate::display_list::Command;
use crate::scene::Brush;

use super::super::gradients::{
    push_ellipse, push_ellipse_radial_gradient, push_rect_conic_gradient, push_rect_linear_gradient,
};
use super::super::verts::rect_to_verts;
use super::UnifiedBuilder;

impl UnifiedBuilder {
    pub(super) fn handle_rect(&mut self, cmd: &Command) {
        let Command::DrawRect {
            rect,
            brush,
            transform,
            z,
            ..
        } = cmd
        else {
            return;
        };
        // Rect draws already carry the full world transform.
        let final_transform = *transform;
        let opa = self.current_opacity();
        match brush {
            Brush::Solid(col) => {
                let color = Self::premul_opa([col.r, col.g, col.b, col.a], opa);
                let (v, i) = rect_to_verts(*rect, color, final_transform, *z as f32);
                if Self::is_transparent(color[3]) {
                    let index_start = self.transparent_indices.len();
                    let base = self.transparent_vertices.len() as u16;
                    self.transparent_vertices.extend_from_slice(&v);
                    self.transparent_indices
                        .extend(i.iter().map(|idx| base + idx));
                    let index_end = self.transparent_indices.len();
                    let clip = self.current_clip();
                    self.record_transparent_batch(*z, index_start, index_end, clip);
                } else {
                    let base = self.vertices.len() as u16;
                    self.vertices.extend_from_slice(&v);
                    self.indices.extend(i.iter().map(|idx| base + idx));
                }
            }
            Brush::LinearGradient { stops, .. } => {
                // Only handle horizontal gradients for now: map t along x within rect
                let mut packed: Vec<(f32, [f32; 4])> = stops
                    .iter()
                    .map(|(tpos, c)| (*tpos, Self::premul_opa([c.r, c.g, c.b, c.a], opa)))
                    .collect();
                if packed.is_empty() {
                    return;
                }
                // Clamp and ensure 0 and 1 exist
                if packed.first().unwrap().0 > 0.0 {
                    let c = packed.first().unwrap().1;
                    packed.insert(0, (0.0, c));
                }
                if packed.last().unwrap().0 < 1.0 {
                    let c = packed.last().unwrap().1;
                    packed.push((1.0, c));
                }
                let gradient_transparent = packed.iter().any(|(_, c)| Self::is_transparent(c[3]));
                if gradient_transparent {
                    let index_start = self.transparent_indices.len();
                    push_rect_linear_gradient(
                        &mut self.transparent_vertices,
                        &mut self.transparent_indices,
                        *rect,
                        &packed,
                        final_transform,
                        *z as f32,
                    );
                    let index_end = self.transparent_indices.len();
                    let clip = self.current_clip();
                    self.record_transparent_batch(*z, index_start, index_end, clip);
                } else {
                    push_rect_linear_gradient(
                        &mut self.vertices,
                        &mut self.indices,
                        *rect,
                        &packed,
                        final_transform,
                        *z as f32,
                    );
                }
            }
            Brush::ConicGradient {
                center,
                start_angle,
                stops,
            } => {
                let packed: Vec<(f32, [f32; 4])> = stops
                    .iter()
                    .map(|(tpos, c)| (*tpos, Self::premul_opa([c.r, c.g, c.b, c.a], opa)))
                    .collect();
                if packed.is_empty() {
                    return;
                }
                let gradient_transparent = packed.iter().any(|(_, c)| Self::is_transparent(c[3]));
                if gradient_transparent {
                    let index_start = self.transparent_indices.len();
                    push_rect_conic_gradient(
                        &mut self.transparent_vertices,
                        &mut self.transparent_indices,
                        *rect,
                        *center,
                        *start_angle,
                        &packed,
                        *z as f32,
                        final_transform,
                    );
                    let index_end = self.transparent_indices.len();
                    let clip = self.current_clip();
                    self.record_transparent_batch(*z, index_start, index_end, clip);
                } else {
                    push_rect_conic_gradient(
                        &mut self.vertices,
                        &mut self.indices,
                        *rect,
                        *center,
                        *start_angle,
                        &packed,
                        *z as f32,
                        final_transform,
                    );
                }
            }
            _ => {}
        }
    }

    pub(super) fn handle_ellipse(&mut self, cmd: &Command) {
        let Command::DrawEllipse {
            center,
            radii,
            brush,
            transform,
            z,
            ..
        } = cmd
        else {
            return;
        };
        let final_transform = *transform;
        let opa = self.current_opacity();
        match brush {
            Brush::Solid(col) => {
                let color = Self::premul_opa([col.r, col.g, col.b, col.a], opa);
                if Self::is_transparent(color[3]) {
                    let index_start = self.transparent_indices.len();
                    push_ellipse(
                        &mut self.transparent_vertices,
                        &mut self.transparent_indices,
                        *center,
                        *radii,
                        color,
                        *z as f32,
                        final_transform,
                    );
                    let index_end = self.transparent_indices.len();
                    let clip = self.current_clip();
                    self.record_transparent_batch(*z, index_start, index_end, clip);
                } else {
                    push_ellipse(
                        &mut self.vertices,
                        &mut self.indices,
                        *center,
                        *radii,
                        color,
                        *z as f32,
                        final_transform,
                    );
                }
            }
            Brush::RadialGradient {
                center: _gcenter,
                radius: _r,
                stops,
            } => {
                let mut packed: Vec<(f32, [f32; 4])> = stops
                    .iter()
                    .map(|(t, c)| (*t, Self::premul_opa([c.r, c.g, c.b, c.a], opa)))
                    .collect();
                if packed.is_empty() {
                    return;
                }
                if packed.first().unwrap().0 > 0.0 {
                    let c = packed.first().unwrap().1;
                    packed.insert(0, (0.0, c));
                }
                if packed.last().unwrap().0 < 1.0 {
                    let c = packed.last().unwrap().1;
                    packed.push((1.0, c));
                }
                let gradient_transparent = packed.iter().any(|(_, c)| Self::is_transparent(c[3]));
                if gradient_transparent {
                    let index_start = self.transparent_indices.len();
                    push_ellipse_radial_gradient(
                        &mut self.transparent_vertices,
                        &mut self.transparent_indices,
                        *center,
                        *radii,
                        &packed,
                        *z as f32,
                        final_transform,
                    );
                    let index_end = self.transparent_indices.len();
                    let clip = self.current_clip();
                    self.record_transparent_batch(*z, index_start, index_end, clip);
                } else {
                    push_ellipse_radial_gradient(
                        &mut self.vertices,
                        &mut self.indices,
                        *center,
                        *radii,
                        &packed,
                        *z as f32,
                        final_transform,
                    );
                }
            }
            _ => {}
        }
    }
}
