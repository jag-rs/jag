use jag_draw::{Brush, ColorLinPremul, Path, Rect, RoundedRadii, RoundedRect, Stroke};

use super::Canvas;
use super::helpers::{intersect_rect, path_bounds, rect_path};

impl Canvas {
    /// Set the frame clear/background color (premultiplied linear RGBA).
    pub fn clear(&mut self, color: ColorLinPremul) {
        self.clear_color = Some(color);
    }

    /// Fill a rectangle with a brush.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, brush: Brush, z: i32) {
        let rect = Rect { x, y, w, h };
        // A solid rect inside a rounded `overflow:hidden` ancestor must follow the
        // ancestor's corner radii. `painter.rect` clips only to an axis-aligned
        // rectangle, so when an active rounded clip has a non-zero radius we route
        // the rect through the path-fill clipper (the same machinery `fill_path`
        // uses), which cuts each triangle to the rounded region. Non-solid brushes
        // (gradients) keep the rectangular path — a gradient-filled child of a
        // rounded box would still square its corners, a known gap tracked
        // separately rather than silently treated as handled.
        if let Brush::Solid(color) = brush {
            if let Some(clip) = self.rounded_clip_local() {
                if clip.radii.iter().any(|r| *r > 0.0) {
                    if intersect_rect(rect, clip.rect).is_some() {
                        self.painter
                            .fill_path_clipped(rect_path(rect), color, z, Some(clip));
                    }
                    return;
                }
            }
        }
        if let Some(clip) = self.clip_rect_local() {
            if let Some(clipped) = intersect_rect(rect, clip) {
                self.painter.rect(clipped, brush, z);
            }
        } else {
            self.painter.rect(rect, brush, z);
        }
    }

    /// Composite an externally-rendered texture at the given rectangle.
    ///
    /// The `texture_id` must be registered with the `PassManager` before the
    /// frame is submitted via `register_external_texture`.
    pub fn external_texture(
        &mut self,
        rect: Rect,
        texture_id: jag_draw::ExternalTextureId,
        z: i32,
    ) {
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            if intersect_rect(rect, clip).is_none() {
                return;
            }
        }
        self.painter.external_texture(rect, texture_id, z);
    }

    /// Fill a rectangle as an overlay (no depth testing).
    /// Use this for modal scrims and other overlays that should blend over
    /// existing content without blocking text rendered at lower z-indices.
    ///
    /// The rectangle coordinates are transformed by the current canvas transform,
    /// so they should be in local (viewport) coordinates.
    pub fn fill_overlay_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: ColorLinPremul) {
        // Apply current transform to get screen coordinates.
        // Transform all four corners and compute axis-aligned bounding box.
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        // Transform corner points
        let p0 = [a * x + c * y + e, b * x + d * y + f];
        let p1 = [a * (x + w) + c * y + e, b * (x + w) + d * y + f];
        let p2 = [a * (x + w) + c * (y + h) + e, b * (x + w) + d * (y + h) + f];
        let p3 = [a * x + c * (y + h) + e, b * x + d * (y + h) + f];

        // For axis-aligned transforms (translation/scale only), the AABB works.
        // For rotation, this is an approximation but should be fine for scrims.
        let min_x = p0[0].min(p1[0]).min(p2[0]).min(p3[0]);
        let max_x = p0[0].max(p1[0]).max(p2[0]).max(p3[0]);
        let min_y = p0[1].min(p1[1]).min(p2[1]).min(p3[1]);
        let max_y = p0[1].max(p1[1]).max(p2[1]).max(p3[1]);

        self.overlay_draws.push((
            Rect {
                x: min_x,
                y: min_y,
                w: max_x - min_x,
                h: max_y - min_y,
            },
            color,
        ));
    }

    /// Fill a rectangle as a scrim (blends over all existing content but allows
    /// subsequent z-ordered draws to render on top).
    ///
    /// Unlike `fill_overlay_rect`, this uses a depth buffer attachment with:
    /// - depth_compare = Always (always passes depth test)
    /// - depth_write_enabled = false (doesn't affect depth buffer)
    ///
    /// This allows the scrim to dim background content while the modal panel
    /// (rendered at a higher z-index afterward) renders cleanly on top.
    pub fn fill_scrim_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: ColorLinPremul) {
        // Apply current transform to get screen coordinates.
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        // Transform corner points
        let p0 = [a * x + c * y + e, b * x + d * y + f];
        let p1 = [a * (x + w) + c * y + e, b * (x + w) + d * y + f];
        let p2 = [a * (x + w) + c * (y + h) + e, b * (x + w) + d * (y + h) + f];
        let p3 = [a * x + c * (y + h) + e, b * x + d * (y + h) + f];

        // Compute axis-aligned bounding box
        let min_x = p0[0].min(p1[0]).min(p2[0]).min(p3[0]);
        let max_x = p0[0].max(p1[0]).max(p2[0]).max(p3[0]);
        let min_y = p0[1].min(p1[1]).min(p2[1]).min(p3[1]);
        let max_y = p0[1].max(p1[1]).max(p2[1]).max(p3[1]);

        self.scrim_draws.push(super::ScrimDraw::Rect(
            Rect {
                x: min_x,
                y: min_y,
                w: max_x - min_x,
                h: max_y - min_y,
            },
            color,
        ));
    }

    /// Fill a fullscreen scrim that leaves a rounded-rect hole using stencil.
    pub fn fill_scrim_with_cutout(&mut self, hole: RoundedRect, color: ColorLinPremul) {
        // Transform the hole into screen space using the current canvas transform.
        // Assumes transform is affine (translation/scale/skew); uses AABB to keep it simple.
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        let rect = hole.rect;
        let corners = [
            [rect.x, rect.y],
            [rect.x + rect.w, rect.y],
            [rect.x + rect.w, rect.y + rect.h],
            [rect.x, rect.y + rect.h],
        ];

        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for p in corners {
            let tx = a * p[0] + c * p[1] + e;
            let ty = b * p[0] + d * p[1] + f;
            min_x = min_x.min(tx);
            max_x = max_x.max(tx);
            min_y = min_y.min(ty);
            max_y = max_y.max(ty);
        }

        // Approximate radius scaling by average scale of the transform axes.
        let sx = (a * a + b * b).sqrt();
        let sy = (c * c + d * d).sqrt();
        let scale = if sx.is_finite() && sy.is_finite() && sx > 0.0 && sy > 0.0 {
            (sx + sy) * 0.5
        } else {
            1.0
        };

        let transformed = RoundedRect {
            rect: Rect {
                x: min_x,
                y: min_y,
                w: (max_x - min_x).max(0.0),
                h: (max_y - min_y).max(0.0),
            },
            radii: RoundedRadii {
                tl: hole.radii.tl * scale,
                tr: hole.radii.tr * scale,
                br: hole.radii.br * scale,
                bl: hole.radii.bl * scale,
            },
        };

        self.scrim_draws.push(super::ScrimDraw::Cutout {
            hole: transformed,
            color,
        });
    }

    /// Stroke a path with uniform width and solid color.
    ///
    /// The tessellated stroke triangles are clipped to the active clip rect (in
    /// local space); a fully-outside path is skipped.
    pub fn stroke_path(&mut self, path: Path, width: f32, color: ColorLinPremul, z: i32) {
        let clip = self.clip_rect_local();
        if let Some(clip) = clip {
            if let Some(bounds) = path_bounds(&path) {
                let expanded = Rect {
                    x: bounds.x - width,
                    y: bounds.y - width,
                    w: bounds.w + width * 2.0,
                    h: bounds.h + width * 2.0,
                };
                if intersect_rect(expanded, clip).is_none() {
                    return;
                }
            }
        }
        self.painter.stroke_path_clipped(
            path,
            Stroke { width },
            color,
            z,
            self.rounded_clip_local(),
        );
    }

    /// Fill a path with a solid color.
    ///
    /// The tessellated triangles are clipped to the active clip rect (in local
    /// space) so a path straddling the clip is cut at the edge; a fully-outside
    /// path is skipped.
    pub fn fill_path(&mut self, path: Path, color: ColorLinPremul, z: i32) {
        let clip = self.clip_rect_local();
        if let Some(clip) = clip {
            if let Some(bounds) = path_bounds(&path) {
                if intersect_rect(bounds, clip).is_none() {
                    return;
                }
            }
        }
        self.painter
            .fill_path_clipped(path, color, z, self.rounded_clip_local());
    }

    /// Draw an ellipse (y-down coordinates).
    pub fn ellipse(&mut self, center: [f32; 2], radii: [f32; 2], brush: Brush, z: i32) {
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: center[0] - radii[0],
                y: center[1] - radii[1],
                w: radii[0] * 2.0,
                h: radii[1] * 2.0,
            };
            // Skip if any part overflows the clip (ellipse geometry can't be
            // CPU-clipped, so we reject unless fully contained).
            if let Some(clipped) = intersect_rect(bounds, clip) {
                let fully_inside = (clipped.x - bounds.x).abs() < 0.5
                    && (clipped.y - bounds.y).abs() < 0.5
                    && (clipped.w - bounds.w).abs() < 0.5
                    && (clipped.h - bounds.h).abs() < 0.5;
                if !fully_inside {
                    return;
                }
            } else {
                return;
            }
        }
        self.painter.ellipse(center, radii, brush, z);
    }

    /// Draw a circle (y-down coordinates).
    pub fn circle(&mut self, center: [f32; 2], radius: f32, brush: Brush, z: i32) {
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: center[0] - radius,
                y: center[1] - radius,
                w: radius * 2.0,
                h: radius * 2.0,
            };
            // Skip if any part overflows the clip (circle geometry can't be
            // CPU-clipped, so we reject unless fully contained).
            if let Some(clipped) = intersect_rect(bounds, clip) {
                let fully_inside = (clipped.x - bounds.x).abs() < 0.5
                    && (clipped.y - bounds.y).abs() < 0.5
                    && (clipped.w - bounds.w).abs() < 0.5
                    && (clipped.h - bounds.h).abs() < 0.5;
                if !fully_inside {
                    return;
                }
            } else {
                return;
            }
        }
        self.painter.circle(center, radius, brush, z);
    }

    /// Draw a rounded rectangle fill.
    pub fn rounded_rect(&mut self, rrect: RoundedRect, brush: Brush, z: i32) {
        if let Some(clip) = self.clip_rect_local() {
            if let Some(clipped) = intersect_rect(rrect.rect, clip) {
                let fully_inside = (clipped.x - rrect.rect.x).abs() < 0.5
                    && (clipped.y - rrect.rect.y).abs() < 0.5
                    && (clipped.w - rrect.rect.w).abs() < 0.5
                    && (clipped.h - rrect.rect.h).abs() < 0.5;
                if fully_inside {
                    self.painter.rounded_rect(rrect, brush, z);
                } else {
                    // Zero radii on clipped edges for clean clip boundaries.
                    let mut radii = rrect.radii;
                    if clipped.x > rrect.rect.x + 0.5 {
                        radii.tl = 0.0;
                        radii.bl = 0.0;
                    }
                    if clipped.x + clipped.w < rrect.rect.x + rrect.rect.w - 0.5 {
                        radii.tr = 0.0;
                        radii.br = 0.0;
                    }
                    if clipped.y > rrect.rect.y + 0.5 {
                        radii.tl = 0.0;
                        radii.tr = 0.0;
                    }
                    if clipped.y + clipped.h < rrect.rect.y + rrect.rect.h - 0.5 {
                        radii.bl = 0.0;
                        radii.br = 0.0;
                    }
                    self.painter.rounded_rect(
                        RoundedRect {
                            rect: clipped,
                            radii,
                        },
                        brush,
                        z,
                    );
                }
            }
        } else {
            self.painter.rounded_rect(rrect, brush, z);
        }
    }

    /// Stroke a rounded rectangle.
    pub fn stroke_rounded_rect(&mut self, rrect: RoundedRect, width: f32, brush: Brush, z: i32) {
        if let Some(clip) = self.clip_rect_local() {
            // Expand bounds by stroke width for rejection test.
            let expanded = Rect {
                x: rrect.rect.x - width,
                y: rrect.rect.y - width,
                w: rrect.rect.w + width * 2.0,
                h: rrect.rect.h + width * 2.0,
            };
            if let Some(clipped_expanded) = intersect_rect(expanded, clip) {
                let fully_inside = (clipped_expanded.x - expanded.x).abs() < 0.5
                    && (clipped_expanded.y - expanded.y).abs() < 0.5
                    && (clipped_expanded.w - expanded.w).abs() < 0.5
                    && (clipped_expanded.h - expanded.h).abs() < 0.5;
                if fully_inside {
                    self.painter
                        .stroke_rounded_rect(rrect, Stroke { width }, brush, z);
                } else {
                    // Clip the inner rect and zero radii on clipped edges.
                    if let Some(clipped_inner) = intersect_rect(rrect.rect, clip) {
                        let mut radii = rrect.radii;
                        if clipped_inner.x > rrect.rect.x + 0.5 {
                            radii.tl = 0.0;
                            radii.bl = 0.0;
                        }
                        if clipped_inner.x + clipped_inner.w < rrect.rect.x + rrect.rect.w - 0.5 {
                            radii.tr = 0.0;
                            radii.br = 0.0;
                        }
                        if clipped_inner.y > rrect.rect.y + 0.5 {
                            radii.tl = 0.0;
                            radii.tr = 0.0;
                        }
                        if clipped_inner.y + clipped_inner.h < rrect.rect.y + rrect.rect.h - 0.5 {
                            radii.bl = 0.0;
                            radii.br = 0.0;
                        }
                        self.painter.stroke_rounded_rect(
                            RoundedRect {
                                rect: clipped_inner,
                                radii,
                            },
                            Stroke { width },
                            brush,
                            z,
                        );
                    }
                }
            }
        } else {
            self.painter
                .stroke_rounded_rect(rrect, Stroke { width }, brush, z);
        }
    }
}
