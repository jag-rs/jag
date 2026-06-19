use jag_draw::{BackdropBlurDraw, PathClip, Rect, RoundedRect, Transform2D, snap_to_device};

use super::helpers::intersect_rect;
use super::{Canvas, ImageFitMode, RawImageDraw, RoundedRectClip};

impl Canvas {
    /// Queue an SVG to be rasterized and drawn at origin, scaled to fit within max_size.
    /// Captures the current transform from the painter's transform stack.
    /// Optional style parameter allows overriding fill, stroke, and stroke-width.
    pub fn draw_svg<P: Into<std::path::PathBuf>>(
        &mut self,
        path: P,
        origin: [f32; 2],
        max_size: [f32; 2],
        z: i32,
    ) {
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: max_size[0],
                h: max_size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let rounded_clip = self.rounded_clip_stack.last().cloned().flatten();
        let transform = self.painter.current_transform();
        self.svg_draws.push((
            path.into(),
            origin,
            max_size,
            None,
            z,
            self.current_opacity(),
            transform,
            device_clip,
            rounded_clip,
        ));
    }

    /// Queue an SVG with style overrides to be rasterized and drawn.
    pub fn draw_svg_styled<P: Into<std::path::PathBuf>>(
        &mut self,
        path: P,
        origin: [f32; 2],
        max_size: [f32; 2],
        style: jag_draw::SvgStyle,
        z: i32,
    ) {
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: max_size[0],
                h: max_size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let rounded_clip = self.rounded_clip_stack.last().cloned().flatten();
        let path_buf = path.into();
        let transform = self.painter.current_transform();
        self.svg_draws.push((
            path_buf,
            origin,
            max_size,
            Some(style),
            z,
            self.current_opacity(),
            transform,
            device_clip,
            rounded_clip,
        ));
    }

    /// Queue a raster image (PNG/JPEG/GIF/WebP) to be drawn at origin with the given size.
    /// The fit parameter controls how the image is scaled within the size bounds.
    /// Captures the current transform from the painter's transform stack.
    pub fn draw_image<P: Into<std::path::PathBuf>>(
        &mut self,
        path: P,
        origin: [f32; 2],
        size: [f32; 2],
        fit: ImageFitMode,
        z: i32,
    ) {
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: size[0],
                h: size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let rounded_clip = self.rounded_clip_stack.last().cloned().flatten();
        let transform = self.painter.current_transform();
        self.image_draws.push((
            path.into(),
            origin,
            size,
            fit,
            z,
            self.current_opacity(),
            transform,
            device_clip,
            rounded_clip,
        ));
    }

    /// Queue a CSS backdrop blur over the current framebuffer contents.
    ///
    /// This is a post-process draw that must be interleaved by z-index with
    /// transparent web paint, before the element's own translucent background.
    pub fn backdrop_blur_rect(&mut self, rect: Rect, radius: f32, z: i32) {
        if rect.w <= 0.0 || rect.h <= 0.0 || radius <= 0.0 || !radius.is_finite() {
            return;
        }
        if let Some(clip) = self.clip_rect_local()
            && intersect_rect(rect, clip).is_none()
        {
            return;
        }
        self.backdrop_blur_draws.push(BackdropBlurDraw {
            rect,
            radius,
            z,
            transform: self.painter.current_transform(),
            clip: self.clip_stack.last().copied().flatten(),
        });
    }

    /// Queue raw pixel data to be drawn at origin with the given size.
    /// Pixels should be in BGRA format (4 bytes per pixel) to match CEF native output.
    /// Captures the current transform from the painter's transform stack.
    pub fn draw_raw_image(
        &mut self,
        pixels: Vec<u8>,
        src_width: u32,
        src_height: u32,
        origin: [f32; 2],
        dst_size: [f32; 2],
        z: i32,
    ) {
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: dst_size[0],
                h: dst_size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let transform = self.painter.current_transform();
        self.raw_image_draws.push(RawImageDraw {
            pixels,
            src_width,
            src_height,
            origin,
            dst_size,
            z,
            transform,
            dirty_rects: Vec::new(), // Full frame update
            clip: device_clip,
        });
    }

    /// Queue raw pixel data with dirty rects for partial update.
    /// Pixels should be in BGRA format (4 bytes per pixel) to match CEF native output.
    /// Only the dirty rectangles will be uploaded to the GPU texture.
    pub fn draw_raw_image_with_dirty_rects(
        &mut self,
        pixels: Vec<u8>,
        src_width: u32,
        src_height: u32,
        origin: [f32; 2],
        dst_size: [f32; 2],
        z: i32,
        dirty_rects: Vec<(u32, u32, u32, u32)>,
    ) {
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: dst_size[0],
                h: dst_size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let transform = self.painter.current_transform();
        self.raw_image_draws.push(RawImageDraw {
            pixels,
            src_width,
            src_height,
            origin,
            dst_size,
            z,
            transform,
            dirty_rects,
            clip: device_clip,
        });
    }

    // Expose some painter helpers for advanced users
    pub fn push_clip_rect(&mut self, rect: Rect) {
        self.push_clip_rect_inner(rect);
        // No rounded clip for plain rect clips.
        self.rounded_clip_stack.push(None);
    }

    /// Push a rounded-rect clip.  The AABB is used for scissor-based coarse
    /// clipping; the full rounded rect (with per-corner radii) is forwarded
    /// to image draws for SDF-based fragment discard in the shader.
    pub fn push_clip_rounded_rect(&mut self, rrect: RoundedRect) {
        self.push_clip_rect_inner(rrect.rect);
        // Compute device-space rounded clip for the image shader.
        let s = self.dpi_scale;
        let t = self.painter.current_transform();
        let [a, _b, _c, d, e, f] = t.m;
        // Assumes axis-aligned transform (translation + uniform scale).
        let sx = a.abs() * s;
        let sy = d.abs() * s;
        let dev_rect = Rect {
            x: (rrect.rect.x * a + e) * s,
            y: (rrect.rect.y * d + f) * s,
            w: rrect.rect.w * sx,
            h: rrect.rect.h * sy,
        };
        let scale_r = sx.min(sy); // uniform radius scale
        let dev_radii = [
            rrect.radii.tl * scale_r,
            rrect.radii.tr * scale_r,
            rrect.radii.br * scale_r,
            rrect.radii.bl * scale_r,
        ];
        self.rounded_clip_stack.push(Some(RoundedRectClip {
            rect: dev_rect,
            radii: dev_radii,
        }));
    }

    fn push_clip_rect_inner(&mut self, rect: Rect) {
        // Forward to Painter to keep display list behavior.
        self.painter.push_clip_rect(rect);

        // Compute device-space clip rect based on current transform and dpi.
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        let x0 = rect.x;
        let y0 = rect.y;
        let x1 = rect.x + rect.w;
        let y1 = rect.y + rect.h;

        let p0 = [a * x0 + c * y0 + e, b * x0 + d * y0 + f];
        let p1 = [a * x1 + c * y0 + e, b * x1 + d * y0 + f];
        let p2 = [a * x0 + c * y1 + e, b * x0 + d * y1 + f];
        let p3 = [a * x1 + c * y1 + e, b * x1 + d * y1 + f];

        let min_x = p0[0].min(p1[0]).min(p2[0]).min(p3[0]) * self.dpi_scale;
        let max_x = p0[0].max(p1[0]).max(p2[0]).max(p3[0]) * self.dpi_scale;
        let min_y = p0[1].min(p1[1]).min(p2[1]).min(p3[1]) * self.dpi_scale;
        let max_y = p0[1].max(p1[1]).max(p2[1]).max(p3[1]) * self.dpi_scale;

        let new_clip = Rect {
            x: min_x,
            y: min_y,
            w: (max_x - min_x).max(0.0),
            h: (max_y - min_y).max(0.0),
        };

        let merged = match self.clip_stack.last().cloned().unwrap_or(None) {
            None => Some(new_clip),
            Some(prev) => Some(intersect_rect(prev, new_clip).unwrap_or(Rect {
                x: prev.x,
                y: prev.y,
                w: 0.0,
                h: 0.0,
            })),
        };
        self.clip_stack.push(merged);
    }

    pub fn pop_clip(&mut self) {
        self.painter.pop_clip();
        if self.clip_stack.len() > 1 {
            self.clip_stack.pop();
        }
        if self.rounded_clip_stack.len() > 1 {
            self.rounded_clip_stack.pop();
        }
    }
    pub fn push_transform(&mut self, t: Transform2D) {
        self.painter.push_transform(t);
    }
    pub fn pop_transform(&mut self) {
        self.painter.pop_transform();
    }

    pub fn push_opacity(&mut self, opacity: f32) {
        let parent_opacity = self.current_opacity();
        self.opacity_stack
            .push(parent_opacity * opacity.clamp(0.0, 1.0));
        self.painter.push_opacity(opacity);
    }

    pub fn pop_opacity(&mut self) {
        if self.opacity_stack.len() > 1 {
            self.opacity_stack.pop();
        }
        self.painter.pop_opacity();
    }

    /// Add a hit-only region (invisible, used for interaction detection)
    pub fn hit_region_rect(&mut self, id: u32, rect: Rect, z: i32) {
        self.painter.hit_region_rect(id, rect, z);
    }

    /// Return the current number of commands in the display list.
    pub fn command_count(&self) -> usize {
        self.painter.command_count()
    }

    /// Get a reference to the display list for hit testing
    pub fn display_list(&self) -> &jag_draw::DisplayList {
        self.painter.display_list()
    }

    /// Snap a rectangle defined in logical coordinates so that, after applying
    /// the current transform and DPI scale, its edges land on physical pixel
    /// boundaries. This assumes the current transform is an axis-aligned
    /// translate/scale (no rotation/skew); for more complex transforms the
    /// original rect is returned unchanged.
    pub fn snap_rect_logical_to_device(&self, rect: Rect) -> Rect {
        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        // Only handle simple translate/scale transforms. If there is rotation
        // or skew, fall back to the original rect to avoid warping.
        let is_simple = (b.abs() < 1e-4)
            && (c.abs() < 1e-4)
            && ((a - 1.0).abs() < 1e-4)
            && ((d - 1.0).abs() < 1e-4);
        if !is_simple {
            return rect;
        }

        let tx = e;
        let ty = f;

        // Snap both corners in device space, then bring them back to logical
        // by subtracting the translation and dividing by scale factor.
        let x0_device = snap_to_device(rect.x + tx, sf);
        let y0_device = snap_to_device(rect.y + ty, sf);
        let x1_device = snap_to_device(rect.x + rect.w + tx, sf);
        let y1_device = snap_to_device(rect.y + rect.h + ty, sf);

        let x0 = x0_device - tx;
        let y0 = y0_device - ty;
        let x1 = x1_device - tx;
        let y1 = y1_device - ty;

        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        }
    }

    /// Get the current effective clip rect in local (pre-transform) coordinates.
    ///
    /// Returns `None` when no clip is active or when the transform contains
    /// rotation/skew (where axis-aligned clipping would be incorrect).
    /// For axis-aligned transforms (translation + scale), the device-space clip
    /// is inverse-transformed back to the local coordinate space.
    pub(crate) fn clip_rect_local(&self) -> Option<Rect> {
        let clip_device = match self.clip_stack.last() {
            Some(Some(r)) => *r,
            _ => return None,
        };
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        // Only handle axis-aligned transforms (no rotation/skew).
        if b.abs() > 1e-4 || c.abs() > 1e-4 {
            return None;
        }

        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };

        let sx = a * sf;
        let sy = d * sf;
        if sx.abs() < 1e-6 || sy.abs() < 1e-6 {
            return None;
        }

        // Inverse-transform: device = (a * local + e) * sf
        //                  → local = device / (a * sf) - e / a
        let local_x0 = clip_device.x / sx - e / a;
        let local_y0 = clip_device.y / sy - f / d;
        let local_x1 = (clip_device.x + clip_device.w) / sx - e / a;
        let local_y1 = (clip_device.y + clip_device.h) / sy - f / d;

        // Handle negative scales (flips).
        let (lx0, lx1) = if local_x0 < local_x1 {
            (local_x0, local_x1)
        } else {
            (local_x1, local_x0)
        };
        let (ly0, ly1) = if local_y0 < local_y1 {
            (local_y0, local_y1)
        } else {
            (local_y1, local_y0)
        };

        Some(Rect {
            x: lx0,
            y: ly0,
            w: lx1 - lx0,
            h: ly1 - ly0,
        })
    }

    /// The active clip as a [`PathClip`] in the path's local space: the rect from
    /// [`Self::clip_rect_local`] plus the current rounded-clip corner radii (if
    /// any), converted device->local by the transform's scale. `None` when there
    /// is no clip (or a rotated/skewed transform `clip_rect_local` can't invert).
    /// Lets filled/stroked SVG paths honor `border-radius` `overflow:hidden`
    /// corners, not just the bounding rect.
    pub(super) fn rounded_clip_local(&self) -> Option<PathClip> {
        let rect = self.clip_rect_local()?;
        // The nearest active rounded clip. Descendants (e.g. an SVG element's own
        // default `overflow` rect clip) push `None` entries on top of an ancestor
        // rounded `overflow:hidden` box, so checking only the top would miss it.
        let Some(rc) = self
            .rounded_clip_stack
            .iter()
            .rev()
            .find_map(|e| e.as_ref())
        else {
            return Some(PathClip::rect(rect));
        };
        // Convert device-space corner radii to local space by the transform's
        // x-scale (dpi * any CSS scale). These clips occur under uniform scale in
        // practice; a non-uniform scale would make corners slightly elliptical
        // (still rounded, not square). `rect` is the intersected clip bounds; when
        // a tighter inner rect clip cuts inside the rounded box the corners are
        // applied to that tighter rect, which is exact in the common case (inner
        // rect == the rounded box, as for an SVG filling its container).
        let t = self.painter.current_transform();
        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };
        let sx = (t.m[0] * sf).abs();
        if sx < 1e-6 {
            return Some(PathClip::rect(rect));
        }
        let radii = [
            rc.radii[0] / sx,
            rc.radii[1] / sx,
            rc.radii[2] / sx,
            rc.radii[3] / sx,
        ];
        Some(PathClip { rect, radii })
    }
}
