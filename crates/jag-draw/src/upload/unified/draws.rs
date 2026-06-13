use crate::display_list::Command;
use crate::scene::{Rect, TextRun};

use super::super::types::{
    ExtractedExternalTextureDraw, ExtractedImageDraw, ExtractedSvgDraw, ExtractedTextDraw,
};
use super::super::verts::{apply_transform, rect_to_verts};
use super::UnifiedBuilder;

impl UnifiedBuilder {
    pub(super) fn handle_text(&mut self, cmd: &Command) {
        let Command::DrawText {
            run, z, transform, ..
        } = cmd
        else {
            return;
        };
        // Text draws already carry the full world transform.
        let final_transform = *transform;
        let opa = self.current_opacity();
        let mut text_run = run.clone();
        if opa < 0.999 {
            text_run.color.r *= opa;
            text_run.color.g *= opa;
            text_run.color.b *= opa;
            text_run.color.a *= opa;
        }
        let clip = self.current_clip();
        self.text_draws.push(ExtractedTextDraw {
            run: text_run,
            z: *z,
            transform: final_transform,
            clip,
        });
    }

    pub(super) fn handle_hyperlink(&mut self, cmd: &Command) {
        let Command::DrawHyperlink {
            hyperlink,
            z,
            transform,
            ..
        } = cmd
        else {
            return;
        };
        // Hyperlink commands also carry their full world transform.
        let final_transform = *transform;
        let opa = self.current_opacity();

        // Extract hyperlink text as a text draw (with group opacity)
        let mut link_color = hyperlink.color;
        if opa < 0.999 {
            link_color.r *= opa;
            link_color.g *= opa;
            link_color.b *= opa;
            link_color.a *= opa;
        }
        let text_run = TextRun {
            text: hyperlink.text.clone(),
            pos: hyperlink.pos,
            size: hyperlink.size,
            logical_size: 0.0,
            color: link_color,
            weight: hyperlink.weight,
            style: hyperlink.style,
            family: hyperlink.family.clone(),
        };
        let clip = self.current_clip();
        self.text_draws.push(ExtractedTextDraw {
            run: text_run,
            z: *z,
            transform: final_transform,
            clip,
        });

        // Draw underline if enabled
        if hyperlink.underline {
            let underline_color = hyperlink.underline_color.unwrap_or(hyperlink.color);
            let color = Self::premul_opa(
                [
                    underline_color.r,
                    underline_color.g,
                    underline_color.b,
                    underline_color.a,
                ],
                opa,
            );

            // Prefer explicit measured width from layout. Fall back to heuristic.
            let (underline_x, text_width) =
                if let Some(w) = hyperlink.measured_width.map(|v| v.max(0.0)) {
                    (hyperlink.pos[0], w)
                } else {
                    let trimmed = hyperlink.text.trim_end();
                    let char_count = trimmed.chars().count() as f32;
                    let weight_boost = ((hyperlink.weight - 400.0).max(0.0) / 500.0) * 0.08;
                    let char_width = hyperlink.size * (0.50 + weight_boost);
                    let mut width = char_count * char_width;
                    let inset = hyperlink.size * 0.10;
                    if width > inset * 2.0 {
                        width -= inset * 2.0;
                    }
                    (hyperlink.pos[0] + inset, width)
                };

            // Underline is a thin rect slightly below the baseline.
            // `hyperlink.pos[1]` is the baseline Y coordinate; place the
            // underline about ~10% of the font size below it.
            let underline_thickness = (hyperlink.size * 0.08).max(1.0);
            let underline_offset = hyperlink.size * 0.10; // Slightly closer to glyphs

            let underline_rect = Rect {
                x: underline_x,
                y: hyperlink.pos[1] + underline_offset,
                w: text_width,
                h: underline_thickness,
            };

            let (v, i) = rect_to_verts(underline_rect, color, final_transform, *z as f32);
            if Self::is_transparent(color[3]) {
                let index_start = self.transparent_indices.len();
                let base = self.transparent_vertices.len() as u16;
                self.transparent_vertices.extend_from_slice(&v);
                self.transparent_indices
                    .extend(i.iter().map(|idx| base + idx));
                let index_end = self.transparent_indices.len();
                let cur_clip = self.current_clip();
                self.record_transparent_batch(*z, index_start, index_end, cur_clip);
            } else {
                let base = self.vertices.len() as u16;
                self.vertices.extend_from_slice(&v);
                self.indices.extend(i.iter().map(|idx| base + idx));
            }
        }
    }

    pub(super) fn handle_image(&mut self, cmd: &Command) {
        let Command::DrawImage {
            path,
            origin,
            size,
            z,
            transform,
        } = cmd
        else {
            return;
        };
        // Apply the command's world transform to the image origin.
        let final_transform = *transform;
        let world_origin = apply_transform(*origin, final_transform);
        let opa = self.current_opacity();
        self.image_draws.push(ExtractedImageDraw {
            path: path.clone(),
            origin: world_origin,
            size: *size,
            z: *z,
            transform: final_transform,
            opacity: opa,
        });
    }

    pub(super) fn handle_svg(&mut self, cmd: &Command) {
        let Command::DrawSvg {
            path,
            origin,
            max_size,
            z,
            transform,
        } = cmd
        else {
            return;
        };
        // Apply the command's world transform to the SVG origin.
        let final_transform = *transform;
        let world_origin = apply_transform(*origin, final_transform);
        let opa = self.current_opacity();
        self.svg_draws.push(ExtractedSvgDraw {
            path: path.clone(),
            origin: world_origin,
            size: *max_size,
            z: *z,
            transform: final_transform,
            opacity: opa,
        });
    }

    pub(super) fn handle_external_texture(&mut self, cmd: &Command) {
        let Command::DrawExternalTexture {
            rect,
            texture_id,
            z,
            transform,
            opacity,
            premultiplied,
        } = cmd
        else {
            return;
        };
        let final_transform = *transform;
        let world_origin = apply_transform([rect.x, rect.y], final_transform);
        let opa = self.current_opacity();
        self.external_texture_draws
            .push(ExtractedExternalTextureDraw {
                texture_id: *texture_id,
                origin: world_origin,
                size: [rect.w, rect.h],
                z: *z,
                opacity: *opacity * opa,
                premultiplied: *premultiplied,
            });
    }
}
