use crate::display_list::Command;
use crate::scene::Brush;

use super::super::gradients::{
    push_rounded_rect_conic_gradient, push_rounded_rect_linear_gradient,
    push_rounded_rect_radial_gradient,
};
use super::super::shapes::push_rounded_rect_aa;
use super::UnifiedBuilder;

impl UnifiedBuilder {
    pub(super) fn handle_rounded_rect(&mut self, cmd: &Command) {
        let Command::DrawRoundedRect {
            rrect,
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
                let index_start = self.transparent_indices.len();
                push_rounded_rect_aa(
                    &mut self.transparent_vertices,
                    &mut self.transparent_indices,
                    *rrect,
                    color,
                    *z as f32,
                    final_transform,
                );
                let index_end = self.transparent_indices.len();
                let clip = self.current_clip();
                self.record_transparent_batch(*z, index_start, index_end, clip);
            }
            Brush::LinearGradient { start, end, stops } => {
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
                    push_rounded_rect_linear_gradient(
                        &mut self.transparent_vertices,
                        &mut self.transparent_indices,
                        *rrect,
                        *start,
                        *end,
                        &packed,
                        *z as f32,
                        final_transform,
                    );
                    let index_end = self.transparent_indices.len();
                    let clip = self.current_clip();
                    self.record_transparent_batch(*z, index_start, index_end, clip);
                } else {
                    push_rounded_rect_linear_gradient(
                        &mut self.vertices,
                        &mut self.indices,
                        *rrect,
                        *start,
                        *end,
                        &packed,
                        *z as f32,
                        final_transform,
                    );
                }
            }
            Brush::RadialGradient {
                center,
                radius,
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
                    push_rounded_rect_radial_gradient(
                        &mut self.transparent_vertices,
                        &mut self.transparent_indices,
                        *rrect,
                        *center,
                        *radius,
                        &packed,
                        *z as f32,
                        final_transform,
                    );
                    let index_end = self.transparent_indices.len();
                    let clip = self.current_clip();
                    self.record_transparent_batch(*z, index_start, index_end, clip);
                } else {
                    push_rounded_rect_radial_gradient(
                        &mut self.vertices,
                        &mut self.indices,
                        *rrect,
                        *center,
                        *radius,
                        &packed,
                        *z as f32,
                        final_transform,
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
                    push_rounded_rect_conic_gradient(
                        &mut self.transparent_vertices,
                        &mut self.transparent_indices,
                        *rrect,
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
                    push_rounded_rect_conic_gradient(
                        &mut self.vertices,
                        &mut self.indices,
                        *rrect,
                        *center,
                        *start_angle,
                        &packed,
                        *z as f32,
                        final_transform,
                    );
                }
            }
        }
    }
}
