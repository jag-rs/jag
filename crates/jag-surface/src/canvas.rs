use std::sync::Arc;

use jag_draw::{
    Brush, ColorLinPremul, FontStyle, Painter, Path, RasterizedGlyph, Rect, RoundedRadii,
    RoundedRect, Stroke, TextProvider, TextRun, Transform2D, Viewport, snap_to_device,
};

/// Rounded-rect clip region in device pixels, passed to the image shader
/// for SDF-based fragment discard.
#[derive(Debug, Clone, Copy)]
pub struct RoundedRectClip {
    /// Bounding rect in device pixels.
    pub rect: Rect,
    /// Per-corner radii in device pixels (tl, tr, br, bl).
    pub radii: [f32; 4],
}

/// How an image should fit within its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFitMode {
    /// Stretch to fill (may distort aspect ratio)
    Fill,
    /// Fit inside maintaining aspect ratio (letterbox/pillarbox)
    Contain,
    /// Fill maintaining aspect ratio (may crop edges)
    Cover,
}

impl Default for ImageFitMode {
    fn default() -> Self {
        Self::Contain
    }
}

/// Builder for a single frame’s draw commands. Wraps `Painter` and adds canvas helpers.
pub struct Canvas {
    pub(crate) viewport: Viewport,
    pub(crate) painter: Painter,
    pub(crate) clear_color: Option<ColorLinPremul>,
    pub(crate) text_provider: Option<Arc<dyn TextProvider + Send + Sync>>, // optional high-level text shaper
    pub(crate) glyph_draws: Vec<([f32; 2], RasterizedGlyph, ColorLinPremul, i32, Option<Rect>)>, // low-level glyph masks with z-index + clip
    pub(crate) svg_draws: Vec<(
        std::path::PathBuf,
        [f32; 2],
        [f32; 2],
        Option<jag_draw::SvgStyle>,
        i32,
        Transform2D,
        Option<Rect>,
    )>, // (path, origin, max_size, style, z, transform, device_clip)
    pub(crate) image_draws: Vec<(
        std::path::PathBuf,
        [f32; 2],
        [f32; 2],
        ImageFitMode,
        i32,
        Transform2D,
        Option<Rect>,
        Option<RoundedRectClip>,
    )>, // (path, origin, size, fit, z, transform, device_clip, rounded_clip)
    /// Raw pixel data draws: (pixels_rgba, src_width, src_height, origin, dst_size, z, transform)
    pub(crate) raw_image_draws: Vec<RawImageDraw>,
    pub(crate) dpi_scale: f32, // DPI scale factor for text rendering
    // Effective clip stack in device coordinates for direct text rendering.
    // Each entry is the intersection of all active clips at that depth.
    pub(crate) clip_stack: Vec<Option<Rect>>,
    // Parallel stack: optional rounded-rect clip for the current depth.
    // When present, image draws receive this for SDF-based corner clipping.
    pub(crate) rounded_clip_stack: Vec<Option<RoundedRectClip>>,
    // Overlay rectangles that render without depth testing (for modal scrims).
    // These are rendered in a separate pass after the main scene.
    pub(crate) overlay_draws: Vec<(Rect, ColorLinPremul)>,
    // Scrim draws that blend over content but allow z-ordered content to render on top.
    // Supports either a full-rect scrim or a scrim with a rounded-rect cutout via stencil.
    pub(crate) scrim_draws: Vec<ScrimDraw>,
}

/// Scrim drawing modes.
#[derive(Clone, Copy)]
pub enum ScrimDraw {
    Rect(Rect, ColorLinPremul),
    Cutout {
        hole: RoundedRect,
        color: ColorLinPremul,
    },
}

/// Raw image draw request for rendering pixel data directly.
#[derive(Clone)]
pub struct RawImageDraw {
    /// BGRA pixel data (4 bytes per pixel) - matches CEF native format
    pub pixels: Vec<u8>,
    /// Source image width
    pub src_width: u32,
    /// Source image height
    pub src_height: u32,
    /// Destination origin in scene coordinates
    pub origin: [f32; 2],
    /// Destination size in scene coordinates
    pub dst_size: [f32; 2],
    /// Z-index for depth ordering
    pub z: i32,
    /// Transform at draw time
    pub transform: Transform2D,
    /// Dirty rectangles for partial update (x, y, w, h) - empty = full frame
    pub dirty_rects: Vec<(u32, u32, u32, u32)>,
    /// Device-space clip rect for GPU scissor clipping (None = no clip).
    pub clip: Option<Rect>,
}

impl Canvas {
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Get the current transform from the painter's transform stack.
    pub fn current_transform(&self) -> Transform2D {
        self.painter.current_transform()
    }

    /// Set the frame clear/background color (premultiplied linear RGBA).
    pub fn clear(&mut self, color: ColorLinPremul) {
        self.clear_color = Some(color);
    }

    /// Fill a rectangle with a brush.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, brush: Brush, z: i32) {
        let rect = Rect { x, y, w, h };
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

        self.scrim_draws.push(ScrimDraw::Rect(
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

        self.scrim_draws.push(ScrimDraw::Cutout {
            hole: transformed,
            color,
        });
    }

    /// Stroke a path with uniform width and solid color.
    ///
    /// Paths that are not fully contained within the active clip are rejected
    /// because arbitrary path geometry cannot be CPU-clipped to a rectangle.
    pub fn stroke_path(&mut self, path: Path, width: f32, color: ColorLinPremul, z: i32) {
        if let Some(clip) = self.clip_rect_local() {
            if let Some(bounds) = path_bounds(&path) {
                let expanded = Rect {
                    x: bounds.x - width,
                    y: bounds.y - width,
                    w: bounds.w + width * 2.0,
                    h: bounds.h + width * 2.0,
                };
                // Skip only when the path is fully outside the clip.
                // Paths can't be CPU-clipped to a rect, so partially-visible
                // paths are drawn in full; the push_clip_rect zero-area fix
                // handles the viewport-overflow case.
                if intersect_rect(expanded, clip).is_none() {
                    return;
                }
            }
        }
        self.painter.stroke_path(path, Stroke { width }, color, z);
    }

    /// Fill a path with a solid color.
    ///
    /// Paths that are fully outside the active clip are rejected.
    pub fn fill_path(&mut self, path: Path, color: ColorLinPremul, z: i32) {
        if let Some(clip) = self.clip_rect_local() {
            if let Some(bounds) = path_bounds(&path) {
                if intersect_rect(bounds, clip).is_none() {
                    return;
                }
            }
        }
        self.painter.fill_path(path, color, z);
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

    /// Draw text using direct rasterization (recommended).
    ///
    /// This method rasterizes glyphs immediately using the text provider,
    /// bypassing complex display list paths. This is simpler and more
    /// reliable than deferred rendering.
    ///
    /// # Performance
    /// - Glyphs are shaped and rasterized on each call
    /// - Use [`TextLayoutCache`](jag_draw::TextLayoutCache) to cache wrapping computations
    /// - Debounce resize events to avoid excessive rasterization
    ///
    /// # Transform Stack
    /// The current transform is applied to position text correctly
    /// within zones (viewport, toolbar, etc.).
    ///
    /// # DPI Scaling
    /// Both position and size are automatically scaled by `self.dpi_scale`.
    ///
    /// # Example
    /// ```no_run
    /// # use jag_surface::Canvas;
    /// # use jag_draw::ColorLinPremul;
    /// # let mut canvas: Canvas = todo!();
    /// canvas.draw_text_run(
    ///     [10.0, 20.0],
    ///     "Hello, world!".to_string(),
    ///     16.0,
    ///     ColorLinPremul::rgba(255, 255, 255, 255),
    ///     10,  // z-index
    /// );
    /// ```
    pub fn draw_text_run(
        &mut self,
        origin: [f32; 2],
        text: String,
        size_px: f32,
        color: ColorLinPremul,
        z: i32,
    ) {
        // Backwards-compatible wrapper: treat as normal weight text.
        self.draw_text_run_weighted(origin, text, size_px, 400.0, color, z);
    }

    /// Draw a text run with an explicit font weight.
    ///
    /// `weight` should follow CSS semantics (100–900; 400 = normal, 700 = bold).
    pub fn draw_text_run_weighted(
        &mut self,
        origin: [f32; 2],
        text: String,
        size_px: f32,
        weight: f32,
        color: ColorLinPremul,
        z: i32,
    ) {
        self.draw_text_run_styled(
            origin,
            text,
            size_px,
            weight,
            FontStyle::Normal,
            None,
            color,
            z,
        );
    }

    /// Draw a text run with full styling options.
    ///
    /// `weight` should follow CSS semantics (100–900; 400 = normal, 700 = bold).
    /// `style` specifies normal, italic, or oblique rendering.
    /// `family` optionally overrides the font family.
    pub fn draw_text_run_styled(
        &mut self,
        origin: [f32; 2],
        text: String,
        size_px: f32,
        weight: f32,
        style: FontStyle,
        family: Option<String>,
        color: ColorLinPremul,
        z: i32,
    ) {
        // If we have a provider and we're not inside an opacity group,
        // rasterize immediately (simple, reliable).
        // Inside opacity groups we route through display-list text so the
        // whole subtree can be composited once with group alpha.
        if let Some(ref provider) = self.text_provider
            && !self.painter.has_active_opacity()
        {
            // Apply current transform to origin (handles zone positioning)
            let transform = self.painter.current_transform();
            let [a, b, c, d, e, f] = transform.m;
            let transformed_origin = [
                a * origin[0] + c * origin[1] + e,
                b * origin[0] + d * origin[1] + f,
            ];

            // DPI scale for converting logical to device coordinates
            let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
                self.dpi_scale
            } else {
                1.0
            };

            // Rasterize at *physical* pixel size so glyph bitmaps map 1:1 to the
            // backbuffer. We still keep layout in logical pixels and convert
            // offsets back into logical units below.
            // Carry font weight through so providers (e.g., JagTextProvider) can approximate
            // bold/semibold rendering when available.
            let run = TextRun {
                text,
                pos: [0.0, 0.0],
                size: (size_px * sf).max(1.0),
                color,
                weight,
                style,
                family,
            };

            // Rasterize glyphs, using a shared cache to avoid
            // re-rasterizing identical text every frame.
            let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &run);
            // Current effective clip rect in device coordinates, if any.
            let current_clip = self.clip_stack.last().cloned().unwrap_or(None);

            for g in glyphs.iter() {
                // Provider offsets are in *physical* pixels (due to size scaling above).
                // Convert back into logical coordinates so PassManager's logical DPI
                // scale keeps geometry/text aligned.
                let mut glyph_origin_logical = [
                    transformed_origin[0] + g.offset[0] / sf,
                    transformed_origin[1] + g.offset[1] / sf,
                ];

                // Snap small text so the resulting *physical* origin lands on whole pixels.
                if size_px <= 15.0 {
                    glyph_origin_logical[0] = (glyph_origin_logical[0] * sf).round() / sf;
                    glyph_origin_logical[1] = (glyph_origin_logical[1] * sf).round() / sf;
                }

                // For clipping, convert to device pixels using the scaled logical origin.
                let glyph_origin_device =
                    [glyph_origin_logical[0] * sf, glyph_origin_logical[1] * sf];

                if let Some(clip) = current_clip {
                    // Clip glyph to the current rect in device coordinates.
                    if let Some((clipped_mask, clipped_origin_device)) =
                        clip_glyph_to_rect(&g.mask, glyph_origin_device, clip)
                    {
                        let clipped = RasterizedGlyph {
                            offset: [0.0, 0.0],
                            mask: clipped_mask,
                        };
                        // Convert clipped origin back to logical coordinates
                        let mut clipped_origin_logical =
                            [clipped_origin_device[0] / sf, clipped_origin_device[1] / sf];
                        if size_px <= 15.0 {
                            clipped_origin_logical[0] =
                                (clipped_origin_logical[0] * sf).round() / sf;
                            clipped_origin_logical[1] =
                                (clipped_origin_logical[1] * sf).round() / sf;
                        }
                        self.glyph_draws
                            .push((clipped_origin_logical, clipped, color, z, None));
                    }
                } else {
                    self.glyph_draws
                        .push((glyph_origin_logical, g.clone(), color, z, None));
                }
            }
        } else {
            // Fallback: use display list path (complex, but kept for compatibility)
            self.painter.text(
                TextRun {
                    text,
                    pos: origin,
                    size: size_px,
                    color,
                    weight,
                    style,
                    family,
                },
                z,
            );
        }
    }

    /// Draw text with per-glyph gradient color sampling.
    ///
    /// Works like `draw_text_run_styled` but instead of a single flat color,
    /// each glyph is tinted by sampling the provided `Brush` at the glyph's
    /// normalised horizontal position (`t = glyph_x / text_width`).
    /// This implements CSS `background-clip: text` with gradient backgrounds.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_run_gradient(
        &mut self,
        origin: [f32; 2],
        text: String,
        size_px: f32,
        weight: f32,
        style: FontStyle,
        family: Option<String>,
        brush: &Brush,
        text_width: f32,
        z: i32,
    ) {
        if let Some(ref provider) = self.text_provider
            && !self.painter.has_active_opacity()
        {
            let transform = self.painter.current_transform();
            let [a, b, c, d, e, f] = transform.m;
            let transformed_origin = [
                a * origin[0] + c * origin[1] + e,
                b * origin[0] + d * origin[1] + f,
            ];

            let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
                self.dpi_scale
            } else {
                1.0
            };

            // Solid fallback colour (first gradient stop or white).
            let solid_fallback = match brush {
                Brush::Solid(c) => *c,
                Brush::LinearGradient { stops, .. } => stops.first().map_or(
                    ColorLinPremul {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    },
                    |s| s.1,
                ),
                Brush::RadialGradient { stops, .. } => stops.first().map_or(
                    ColorLinPremul {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    },
                    |s| s.1,
                ),
            };

            let run = TextRun {
                text,
                pos: [0.0, 0.0],
                size: (size_px * sf).max(1.0),
                color: solid_fallback,
                weight,
                style,
                family,
            };

            let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &run);
            let current_clip = self.clip_stack.last().cloned().unwrap_or(None);
            let tw = text_width.max(1.0);

            // Pre-convert gradient stops for the sampling function.
            let grad_stops: Vec<(f32, [f32; 4])> = match brush {
                Brush::LinearGradient { stops, .. } | Brush::RadialGradient { stops, .. } => stops
                    .iter()
                    .map(|(t, c)| (*t, [c.r, c.g, c.b, c.a]))
                    .collect(),
                _ => Vec::new(),
            };

            for g in glyphs.iter() {
                let mut glyph_origin_logical = [
                    transformed_origin[0] + g.offset[0] / sf,
                    transformed_origin[1] + g.offset[1] / sf,
                ];
                if size_px <= 15.0 {
                    glyph_origin_logical[0] = (glyph_origin_logical[0] * sf).round() / sf;
                    glyph_origin_logical[1] = (glyph_origin_logical[1] * sf).round() / sf;
                }

                // Sample gradient at the glyph's horizontal position.
                let glyph_color = if grad_stops.is_empty() {
                    solid_fallback
                } else {
                    let glyph_x = g.offset[0] / sf;
                    let t = (glyph_x / tw).clamp(0.0, 1.0);
                    let [r, g, b, a] = jag_draw::sample_gradient_stops(&grad_stops, t);
                    ColorLinPremul { r, g, b, a }
                };

                let glyph_origin_device =
                    [glyph_origin_logical[0] * sf, glyph_origin_logical[1] * sf];

                if let Some(clip) = current_clip {
                    if let Some((clipped_mask, clipped_origin_device)) =
                        clip_glyph_to_rect(&g.mask, glyph_origin_device, clip)
                    {
                        let clipped = RasterizedGlyph {
                            offset: [0.0, 0.0],
                            mask: clipped_mask,
                        };
                        let mut clipped_origin_logical =
                            [clipped_origin_device[0] / sf, clipped_origin_device[1] / sf];
                        if size_px <= 15.0 {
                            clipped_origin_logical[0] =
                                (clipped_origin_logical[0] * sf).round() / sf;
                            clipped_origin_logical[1] =
                                (clipped_origin_logical[1] * sf).round() / sf;
                        }
                        self.glyph_draws.push((
                            clipped_origin_logical,
                            clipped,
                            glyph_color,
                            z,
                            None,
                        ));
                    }
                } else {
                    self.glyph_draws
                        .push((glyph_origin_logical, g.clone(), glyph_color, z, None));
                }
            }
        } else {
            // Fallback: extract solid color and use display list path.
            let fallback_color = match brush {
                Brush::Solid(c) => *c,
                Brush::LinearGradient { stops, .. } | Brush::RadialGradient { stops, .. } => {
                    stops.first().map_or(
                        ColorLinPremul {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        },
                        |s| s.1,
                    )
                }
            };
            self.painter.text(
                TextRun {
                    text,
                    pos: origin,
                    size: size_px,
                    color: fallback_color,
                    weight,
                    style,
                    family,
                },
                z,
            );
        }
    }

    /// Draw text directly by rasterizing immediately (simpler, bypasses display list).
    /// This is the recommended approach - it's simpler and more reliable than draw_text_run.
    pub fn draw_text_direct(
        &mut self,
        origin: [f32; 2],
        text: &str,
        size_px: f32,
        color: ColorLinPremul,
        provider: &dyn TextProvider,
        z: i32,
    ) {
        self.draw_text_direct_styled(
            origin,
            text,
            size_px,
            400.0,
            FontStyle::Normal,
            None,
            color,
            provider,
            z,
        );
    }

    /// Draw text directly with explicit font styling, bypassing the display list.
    pub fn draw_text_direct_styled(
        &mut self,
        origin: [f32; 2],
        text: &str,
        size_px: f32,
        weight: f32,
        style: FontStyle,
        family: Option<&str>,
        color: ColorLinPremul,
        provider: &dyn TextProvider,
        z: i32,
    ) {
        // Apply current transform to origin (handles zone positioning)
        let transform = self.painter.current_transform();
        let [a, b, c, d, e, f] = transform.m;
        let transformed_origin = [
            a * origin[0] + c * origin[1] + e,
            b * origin[0] + d * origin[1] + f,
        ];

        // Current effective clip rect in device coordinates, if any.
        let current_clip = self.clip_stack.last().cloned().unwrap_or(None);

        // DPI scale for converting logical to device coordinates
        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };

        // Rasterize at *physical* pixel size so small styled text matches the
        // same provider path used by display-list text.
        let run = TextRun {
            text: text.to_string(),
            pos: [0.0, 0.0],
            size: (size_px * sf).max(1.0),
            color,
            weight,
            style,
            family: family.map(str::to_string),
        };

        // Rasterize glyphs, using the shared cache to avoid
        // re-rasterizing identical text every frame.
        let glyphs = jag_draw::rasterize_run_cached(provider, &run);

        for g in glyphs.iter() {
            // Provider offsets are in physical pixels; convert back into logical
            // coordinates so PassManager's DPI scaling remains the single source
            // of truth for mapping to device pixels.
            let mut glyph_origin_logical = [
                transformed_origin[0] + g.offset[0] / sf,
                transformed_origin[1] + g.offset[1] / sf,
            ];

            if size_px <= 15.0 {
                glyph_origin_logical[0] = (glyph_origin_logical[0] * sf).round() / sf;
                glyph_origin_logical[1] = (glyph_origin_logical[1] * sf).round() / sf;
            }

            // For clipping, convert to device pixels
            let glyph_origin_device = [glyph_origin_logical[0] * sf, glyph_origin_logical[1] * sf];

            if let Some(clip) = current_clip {
                if let Some((clipped_mask, clipped_origin_device)) =
                    clip_glyph_to_rect(&g.mask, glyph_origin_device, clip)
                {
                    let clipped = RasterizedGlyph {
                        offset: [0.0, 0.0],
                        mask: clipped_mask,
                    };
                    // Convert clipped origin back to logical coordinates
                    let mut clipped_origin_logical =
                        [clipped_origin_device[0] / sf, clipped_origin_device[1] / sf];
                    if size_px <= 15.0 {
                        clipped_origin_logical[0] = (clipped_origin_logical[0] * sf).round() / sf;
                        clipped_origin_logical[1] = (clipped_origin_logical[1] * sf).round() / sf;
                    }
                    self.glyph_draws
                        .push((clipped_origin_logical, clipped, color, z, None));
                }
            } else {
                self.glyph_draws
                    .push((glyph_origin_logical, g.clone(), color, z, None));
            }
        }
    }

    /// Provide a text provider used for high-level text runs in this frame.
    pub fn set_text_provider(&mut self, provider: Arc<dyn TextProvider + Send + Sync>) {
        self.text_provider = Some(provider);
    }

    pub fn text_provider(&self) -> Option<&Arc<dyn TextProvider + Send + Sync>> {
        self.text_provider.as_ref()
    }

    /// Measure the width of a text run in logical pixels using the active text provider.
    ///
    /// This is intended for layout/centering code that needs a more accurate width than
    /// simple character-count heuristics. When no provider is set, falls back to
    /// `font_size * 0.55 * text.len()` to match legacy behavior.
    pub fn measure_text_width(&self, text: &str, size_px: f32) -> f32 {
        if let Some(provider) = self.text_provider() {
            if let Some(shaped) = provider.shape_paragraph(text, size_px) {
                let total: f32 = shaped.glyphs.iter().map(|g| g.x_advance).sum();
                // Clamp to non-negative to avoid surprising negatives in rare cases.
                return total.max(0.0);
            }
        }
        // Fallback heuristic consistent with text_measure.rs and legacy elements.
        text.chars().count() as f32 * size_px * 0.55
    }

    /// Measure the width of a styled text run in logical pixels.
    ///
    /// Unlike `measure_text_width`, this accounts for font weight, style, and
    /// family so that measurements match `draw_text_run_styled` rendering.
    pub fn measure_text_width_styled(
        &self,
        text: &str,
        size_px: f32,
        weight: f32,
        style: FontStyle,
        family: Option<&str>,
    ) -> f32 {
        if let Some(provider) = self.text_provider() {
            // Measure at the same *physical* pixel size that draw_text_run_styled
            // uses for rasterization so that variable-font axes (especially
            // optical size / opsz) produce identical advance widths.  Scale the
            // result back to logical pixels afterward.
            let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
                self.dpi_scale
            } else {
                1.0
            };
            let run = TextRun {
                text: text.to_string(),
                pos: [0.0, 0.0],
                size: (size_px * sf).max(1.0),
                color: ColorLinPremul::from_srgba_u8([0, 0, 0, 0]),
                weight,
                style,
                family: family.map(String::from),
            };
            provider.measure_run(&run) / sf
        } else {
            text.chars().count() as f32 * size_px * 0.55
        }
    }

    /// Draw pre-rasterized glyph masks at the given origin tinted with the color.
    pub fn draw_text_glyphs(
        &mut self,
        origin: [f32; 2],
        glyphs: &[RasterizedGlyph],
        color: ColorLinPremul,
        z: i32,
    ) {
        for g in glyphs.iter().cloned() {
            self.glyph_draws.push((origin, g, color, z, None));
        }
    }

    /// Draw a hyperlink with text, optional underline, and URL target.
    ///
    /// # Example
    /// ```no_run
    /// # use jag_surface::Canvas;
    /// # use jag_draw::{ColorLinPremul, Hyperlink};
    /// # let mut canvas: Canvas = todo!();
    /// let link = Hyperlink {
    ///     text: "Click me".to_string(),
    ///     pos: [10.0, 20.0],
    ///     size: 16.0,
    ///     color: ColorLinPremul::from_srgba_u8([0, 122, 255, 255]),
    ///     url: "https://example.com".to_string(),
    ///     weight: 400.0,
    ///     measured_width: None,
    ///     underline: true,
    ///     underline_color: None,
    ///     family: None,
    ///     style: jag_draw::FontStyle::Normal,
    /// };
    /// canvas.draw_hyperlink(link, 10);
    /// ```
    pub fn draw_hyperlink(&mut self, hyperlink: jag_draw::Hyperlink, z: i32) {
        // When a text provider is available and we're not inside an opacity
        // group, render the text and underline through canvas methods so they
        // respect the clip_stack (per-glyph clipping for text, rect clipping
        // for the underline).  A stripped-down DrawHyperlink is still emitted
        // into the display list so hit testing continues to work.
        if self.text_provider.is_some() && !self.painter.has_active_opacity() {
            // --- visual: text via per-glyph clipped path ---
            self.draw_text_run_styled(
                hyperlink.pos,
                hyperlink.text.clone(),
                hyperlink.size,
                hyperlink.weight,
                hyperlink.style,
                hyperlink.family.clone(),
                hyperlink.color,
                z,
            );

            // --- visual: underline via clip-aware fill_rect ---
            if hyperlink.underline {
                let underline_color = hyperlink.underline_color.unwrap_or(hyperlink.color);
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

                let underline_thickness = (hyperlink.size * 0.08).max(1.0);
                let underline_offset = hyperlink.size * 0.10;
                self.fill_rect(
                    underline_x,
                    hyperlink.pos[1] + underline_offset,
                    text_width,
                    underline_thickness,
                    Brush::Solid(underline_color),
                    z,
                );
            }

            // --- hit testing only: emit DrawHyperlink with no visual payload ---
            let mut hit_only = hyperlink;
            // Ensure measured_width is set so hit testing doesn't rely on
            // text length (which we're about to clear).
            if hit_only.measured_width.is_none() {
                hit_only.measured_width = Some(self.measure_text_width_styled(
                    &hit_only.text,
                    hit_only.size,
                    hit_only.weight,
                    hit_only.style,
                    hit_only.family.as_deref(),
                ));
            }
            hit_only.text = String::new();
            hit_only.underline = false;
            self.painter.hyperlink(hit_only, z);
        } else {
            // Fallback: no text provider or inside opacity group — use
            // display-list path (no clip, but correct compositing).
            self.painter.hyperlink(hyperlink, z);
        }
    }

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
        let transform = self.painter.current_transform();
        self.svg_draws.push((
            path.into(),
            origin,
            max_size,
            None,
            z,
            transform,
            device_clip,
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
        let path_buf = path.into();
        let transform = self.painter.current_transform();
        self.svg_draws.push((
            path_buf,
            origin,
            max_size,
            Some(style),
            z,
            transform,
            device_clip,
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
        self.image_draws
            .push((path.into(), origin, size, fit, z, transform, device_clip, rounded_clip));
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
            Some(prev) => {
                Some(intersect_rect(prev, new_clip).unwrap_or(Rect {
                    x: prev.x,
                    y: prev.y,
                    w: 0.0,
                    h: 0.0,
                }))
            }
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
        self.painter.push_opacity(opacity);
    }

    pub fn pop_opacity(&mut self) {
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
    fn clip_rect_local(&self) -> Option<Rect> {
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
}

/// Intersect two rectangles (device-space); returns None if they do not overlap.
/// Compute the axis-aligned bounding box of a path's control points.
fn path_bounds(path: &jag_draw::Path) -> Option<Rect> {
    use jag_draw::PathCmd;
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut has_points = false;

    let mut extend = |p: &[f32; 2]| {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
        has_points = true;
    };

    for cmd in &path.cmds {
        match cmd {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => extend(p),
            PathCmd::QuadTo(a, b) => {
                extend(a);
                extend(b);
            }
            PathCmd::CubicTo(a, b, c) => {
                extend(a);
                extend(b);
                extend(c);
            }
            PathCmd::Close => {}
        }
    }

    if has_points {
        Some(Rect {
            x: min_x,
            y: min_y,
            w: max_x - min_x,
            h: max_y - min_y,
        })
    } else {
        None
    }
}

fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    let ax1 = a.x + a.w;
    let ay1 = a.y + a.h;
    let bx1 = b.x + b.w;
    let by1 = b.y + b.h;

    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = ax1.min(bx1);
    let y1 = ay1.min(by1);

    if x1 <= x0 || y1 <= y0 {
        None
    } else {
        Some(Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }
}

/// Clip a glyph mask to a device-space rectangle, returning a new mask and origin.
fn clip_glyph_to_rect(
    mask: &jag_draw::GlyphMask,
    origin: [f32; 2],
    clip: Rect,
) -> Option<(jag_draw::GlyphMask, [f32; 2])> {
    use jag_draw::{ColorMask, GlyphMask, SubpixelMask};

    let glyph_x0 = origin[0];
    let glyph_y0 = origin[1];
    let (width, height, data, bpp) = match mask {
        GlyphMask::Subpixel(m) => (m.width, m.height, &m.data, m.bytes_per_pixel()),
        GlyphMask::Color(m) => (m.width, m.height, &m.data, m.bytes_per_pixel()),
    };

    let glyph_x1 = glyph_x0 + width as f32;
    let glyph_y1 = glyph_y0 + height as f32;

    let clip_x0 = clip.x;
    let clip_y0 = clip.y;
    let clip_x1 = clip.x + clip.w;
    let clip_y1 = clip.y + clip.h;

    let ix0 = glyph_x0.max(clip_x0);
    let iy0 = glyph_y0.max(clip_y0);
    let ix1 = glyph_x1.min(clip_x1);
    let iy1 = glyph_y1.min(clip_y1);

    if ix0 >= ix1 || iy0 >= iy1 {
        return None;
    }

    // Convert intersection to pixel indices within the glyph mask.
    let start_x = ((ix0 - glyph_x0).floor().max(0.0)) as u32;
    let start_y = ((iy0 - glyph_y0).floor().max(0.0)) as u32;
    let end_x = ((ix1 - glyph_x0).ceil().min(width as f32)) as u32;
    let end_y = ((iy1 - glyph_y0).ceil().min(height as f32)) as u32;

    if end_x <= start_x || end_y <= start_y {
        return None;
    }

    let new_w = end_x - start_x;
    let new_h = end_y - start_y;

    let src_stride = width * bpp as u32;
    let dst_stride = new_w * bpp as u32;
    let mut clipped_data = vec![0u8; (new_w * new_h * bpp as u32) as usize];

    for row in 0..new_h {
        let src_y = start_y + row;
        let src_offset = (src_y * src_stride + start_x * bpp as u32) as usize;
        let dst_offset = (row * dst_stride) as usize;
        clipped_data[dst_offset..dst_offset + dst_stride as usize]
            .copy_from_slice(&data[src_offset..src_offset + dst_stride as usize]);
    }

    let clipped = match mask {
        GlyphMask::Subpixel(m) => GlyphMask::Subpixel(SubpixelMask {
            width: new_w,
            height: new_h,
            format: m.format,
            data: clipped_data,
        }),
        GlyphMask::Color(_) => GlyphMask::Color(ColorMask {
            width: new_w,
            height: new_h,
            data: clipped_data,
        }),
    };

    let new_origin = [glyph_x0 + start_x as f32, glyph_y0 + start_y as f32];
    Some((clipped, new_origin))
}
