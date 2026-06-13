use crate::display_list::Command;
use crate::scene::Brush;

use super::super::shapes::{push_rect_stroke, push_rounded_rect_stroke};
use super::super::tessellate::{tessellate_path_fill, tessellate_path_stroke};
use super::UnifiedBuilder;

impl UnifiedBuilder {
    pub(super) fn handle_stroke_rect(&mut self, cmd: &Command) {
        let Command::StrokeRect {
            rect,
            stroke,
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
        if let Brush::Solid(col) = brush {
            let color = Self::premul_opa([col.r, col.g, col.b, col.a], opa);
            if Self::is_transparent(color[3]) {
                let index_start = self.transparent_indices.len();
                push_rect_stroke(
                    &mut self.transparent_vertices,
                    &mut self.transparent_indices,
                    *rect,
                    *stroke,
                    color,
                    *z as f32,
                    final_transform,
                );
                let index_end = self.transparent_indices.len();
                let clip = self.current_clip();
                self.record_transparent_batch(*z, index_start, index_end, clip);
            } else {
                push_rect_stroke(
                    &mut self.vertices,
                    &mut self.indices,
                    *rect,
                    *stroke,
                    color,
                    *z as f32,
                    final_transform,
                );
            }
        }
    }

    pub(super) fn handle_stroke_rounded_rect(&mut self, cmd: &Command) {
        let Command::StrokeRoundedRect {
            rrect,
            stroke,
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
        if let Brush::Solid(col) = brush {
            let color = Self::premul_opa([col.r, col.g, col.b, col.a], opa);
            if Self::is_transparent(color[3]) {
                let index_start = self.transparent_indices.len();
                push_rounded_rect_stroke(
                    &mut self.transparent_vertices,
                    &mut self.transparent_indices,
                    *rrect,
                    *stroke,
                    color,
                    *z as f32,
                    final_transform,
                );
                let index_end = self.transparent_indices.len();
                let clip = self.current_clip();
                self.record_transparent_batch(*z, index_start, index_end, clip);
            } else {
                push_rounded_rect_stroke(
                    &mut self.vertices,
                    &mut self.indices,
                    *rrect,
                    *stroke,
                    color,
                    *z as f32,
                    final_transform,
                );
            }
        }
    }

    pub(super) fn handle_fill_path(&mut self, cmd: &Command) {
        let Command::FillPath {
            path,
            color,
            transform,
            z,
            clip,
        } = cmd
        else {
            return;
        };
        let final_transform = *transform;
        let opa = self.current_opacity();
        let col = Self::premul_opa([color.r, color.g, color.b, color.a], opa);
        if Self::is_transparent(col[3]) {
            let index_start = self.transparent_indices.len();
            tessellate_path_fill(
                &mut self.transparent_vertices,
                &mut self.transparent_indices,
                path,
                col,
                *z as f32,
                final_transform,
                *clip,
            );
            let index_end = self.transparent_indices.len();
            let cur_clip = self.current_clip();
            self.record_transparent_batch(*z, index_start, index_end, cur_clip);
        } else {
            tessellate_path_fill(
                &mut self.vertices,
                &mut self.indices,
                path,
                col,
                *z as f32,
                final_transform,
                *clip,
            );
        }
    }

    pub(super) fn handle_stroke_path(&mut self, cmd: &Command) {
        let Command::StrokePath {
            path,
            stroke,
            color,
            transform,
            z,
            clip,
        } = cmd
        else {
            return;
        };
        let final_transform = *transform;
        let opa = self.current_opacity();
        let col = Self::premul_opa([color.r, color.g, color.b, color.a], opa);
        if Self::is_transparent(col[3]) {
            let index_start = self.transparent_indices.len();
            tessellate_path_stroke(
                &mut self.transparent_vertices,
                &mut self.transparent_indices,
                path,
                *stroke,
                col,
                *z as f32,
                final_transform,
                *clip,
            );
            let index_end = self.transparent_indices.len();
            let cur_clip = self.current_clip();
            self.record_transparent_batch(*z, index_start, index_end, cur_clip);
        } else {
            tessellate_path_stroke(
                &mut self.vertices,
                &mut self.indices,
                path,
                *stroke,
                col,
                *z as f32,
                final_transform,
                *clip,
            );
        }
    }
}
