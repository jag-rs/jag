use anyhow::Result;

use crate::allocator::{BufKey, RenderAllocator};
use crate::display_list::{Command, DisplayList};
use crate::scene::{Rect, Transform2D};

use super::types::{
    ExtractedExternalTextureDraw, ExtractedImageDraw, ExtractedSvgDraw, ExtractedTextDraw,
    GpuScene, SolidBatch, TransparentBatch, UnifiedSceneData, Vertex,
};

mod draws;
mod fills;
mod rounded_rect;
mod strokes_paths;

/// Accumulates all unified-scene state while iterating a `DisplayList`.
///
/// Each field mirrors a local that the original monolithic
/// `upload_display_list_unified` mutated; the nested closures it used are now
/// methods on this struct. Behaviour is preserved exactly: same ordering, same
/// batch flushing, same clip/opacity semantics.
struct UnifiedBuilder {
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
    transparent_vertices: Vec<Vertex>,
    transparent_indices: Vec<u16>,
    transparent_batches: Vec<TransparentBatch>,
    solid_batches: Vec<SolidBatch>,
    text_draws: Vec<ExtractedTextDraw>,
    image_draws: Vec<ExtractedImageDraw>,
    svg_draws: Vec<ExtractedSvgDraw>,
    external_texture_draws: Vec<ExtractedExternalTextureDraw>,
    /// Clip stack: tracks nested PushClip/PopClip regions.
    /// Each entry is the intersection of all ancestor clips.
    clip_stack: Vec<Option<Rect>>,
    /// Track the start of the current opaque solid batch and its clip rect.
    solid_batch_start: usize,
    solid_batch_clip: Option<Rect>,
    /// Track transform stack for completeness, but note that draw commands
    /// already carry fully-composed world transforms. For unified upload we
    /// treat the per-command transform as authoritative and use the stack
    /// only to mirror the current state (kept for potential future use).
    transform_stack: Vec<Transform2D>,
    _current_transform: Transform2D,
    /// Track CSS-style group opacity. Each PushOpacity pushes the effective
    /// (accumulated) opacity onto the stack; PopOpacity restores the previous
    /// level.  All vertex colours are pre-multiplied by the current effective
    /// opacity so that nested opacities compose correctly.
    opacity_stack: Vec<f32>,
}

impl UnifiedBuilder {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            transparent_vertices: Vec::new(),
            transparent_indices: Vec::new(),
            transparent_batches: Vec::new(),
            solid_batches: Vec::new(),
            text_draws: Vec::new(),
            image_draws: Vec::new(),
            svg_draws: Vec::new(),
            external_texture_draws: Vec::new(),
            clip_stack: vec![None],
            solid_batch_start: 0,
            solid_batch_clip: None,
            transform_stack: vec![Transform2D::identity()],
            _current_transform: Transform2D::identity(),
            opacity_stack: vec![1.0],
        }
    }

    fn is_transparent(alpha: f32) -> bool {
        alpha < 0.999
    }

    fn current_clip(&self) -> Option<Rect> {
        *self.clip_stack.last().unwrap_or(&None)
    }

    fn current_opacity(&self) -> f32 {
        *self.opacity_stack.last().unwrap_or(&1.0)
    }

    // Helper: intersect two optional clip rects.
    fn intersect_clips(a: Option<Rect>, b: Rect) -> Option<Rect> {
        match a {
            None => Some(b),
            Some(a) => {
                let x0 = a.x.max(b.x);
                let y0 = a.y.max(b.y);
                let x1 = (a.x + a.w).min(b.x + b.w);
                let y1 = (a.y + a.h).min(b.y + b.h);
                if x1 > x0 && y1 > y0 {
                    Some(Rect {
                        x: x0,
                        y: y0,
                        w: x1 - x0,
                        h: y1 - y0,
                    })
                } else {
                    // Empty intersection — clip everything
                    Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 0.0,
                        h: 0.0,
                    })
                }
            }
        }
    }

    // Helper: multiply a premultiplied-alpha colour by group opacity.
    // All four channels are scaled so the premultiplied invariant holds.
    fn premul_opa(c: [f32; 4], o: f32) -> [f32; 4] {
        if o >= 0.999 {
            return c;
        }
        [c[0] * o, c[1] * o, c[2] * o, c[3] * o]
    }

    // Finalize the current solid batch up to `index_end`.
    fn flush_solid_batch(&mut self, index_end: usize, new_clip: Option<Rect>) {
        if index_end > self.solid_batch_start {
            self.solid_batches.push(SolidBatch {
                index_start: self.solid_batch_start as u32,
                index_count: (index_end - self.solid_batch_start) as u32,
                clip: self.solid_batch_clip,
            });
        }
        self.solid_batch_start = index_end;
        self.solid_batch_clip = new_clip;
    }

    fn record_transparent_batch(
        &mut self,
        z: i32,
        index_start: usize,
        index_end: usize,
        clip: Option<Rect>,
    ) {
        if index_end <= index_start {
            return;
        }
        let start = index_start as u32;
        let count = (index_end - index_start) as u32;
        // Only merge if same z AND same clip rect.
        if let Some(last) = self.transparent_batches.last_mut()
            && last.z == z
            && last.clip == clip
            && last.index_start + last.index_count == start
        {
            last.index_count += count;
        } else {
            self.transparent_batches.push(TransparentBatch {
                z,
                index_start: start,
                index_count: count,
                clip,
            });
        }
    }

    /// Dispatch a single command to the appropriate handler.
    fn handle(&mut self, cmd: &Command) {
        match cmd {
            // Handle transform stack
            Command::PushTransform(t) => {
                // `t` is already the composed world transform at this stack depth.
                self._current_transform = *t;
                self.transform_stack.push(self._current_transform);
            }
            Command::PopTransform => {
                self.transform_stack.pop();
                self._current_transform = self
                    .transform_stack
                    .last()
                    .copied()
                    .unwrap_or(Transform2D::identity());
            }

            Command::DrawText { .. } => self.handle_text(cmd),
            Command::DrawHyperlink { .. } => self.handle_hyperlink(cmd),

            Command::DrawRect { .. } => self.handle_rect(cmd),
            Command::DrawRoundedRect { .. } => self.handle_rounded_rect(cmd),
            Command::StrokeRect { .. } => self.handle_stroke_rect(cmd),
            Command::StrokeRoundedRect { .. } => self.handle_stroke_rounded_rect(cmd),
            Command::DrawEllipse { .. } => self.handle_ellipse(cmd),
            Command::FillPath { .. } => self.handle_fill_path(cmd),
            Command::StrokePath { .. } => self.handle_stroke_path(cmd),

            Command::DrawImage { .. } => self.handle_image(cmd),
            Command::DrawSvg { .. } => self.handle_svg(cmd),
            Command::DrawExternalTexture { .. } => self.handle_external_texture(cmd),

            // BoxShadow commands are handled by PassManager as a separate pipeline.
            Command::BoxShadow { .. } => {}
            // Hit-only regions: intentionally not rendered.
            Command::HitRegionRect { .. } => {}
            Command::HitRegionRoundedRect { .. } => {}
            Command::HitRegionEllipse { .. } => {}
            Command::PushClip(clip_rect) => {
                // Flush the current opaque solid batch before changing clip state.
                let new_clip = Self::intersect_clips(self.current_clip(), clip_rect.0);
                let index_end = self.indices.len();
                self.flush_solid_batch(index_end, new_clip);
                self.clip_stack.push(new_clip);
            }
            Command::PopClip => {
                self.clip_stack.pop();
                let restored_clip = self.current_clip();
                // Flush the current opaque solid batch before restoring clip state.
                let index_end = self.indices.len();
                self.flush_solid_batch(index_end, restored_clip);
            }
            Command::PushOpacity(alpha) => {
                let parent = self.current_opacity();
                self.opacity_stack.push(parent * alpha.clamp(0.0, 1.0));
            }
            Command::PopOpacity => {
                if self.opacity_stack.len() > 1 {
                    self.opacity_stack.pop();
                }
            }
        }
    }

    fn align_indices(indices: &mut Vec<u16>) {
        // Ensure index buffer size meets COPY_BUFFER_ALIGNMENT (4 bytes)
        if (indices.len() % 2) != 0 {
            if indices.len() >= 3 {
                let a = indices[indices.len() - 3];
                let b = indices[indices.len() - 2];
                let c = indices[indices.len() - 1];
                indices.extend_from_slice(&[a, b, c]);
            } else {
                indices.push(0);
            }
        }
    }

    fn upload_scene(
        allocator: &mut RenderAllocator,
        queue: &wgpu::Queue,
        vertices: &[Vertex],
        indices: &[u16],
    ) -> GpuScene {
        let vsize = (vertices.len() * std::mem::size_of::<Vertex>()) as u64;
        let isize = (indices.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = allocator.allocate_buffer(BufKey {
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let ibuf = allocator.allocate_buffer(BufKey {
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf.buffer, 0, bytemuck::cast_slice(vertices));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf.buffer, 0, bytemuck::cast_slice(indices));
        }
        GpuScene {
            vertex: vbuf,
            index: ibuf,
            vertices: vertices.len() as u32,
            indices: indices.len() as u32,
        }
    }

    /// Consume the builder, aligning + uploading both scenes into a result.
    fn finalize(
        mut self,
        allocator: &mut RenderAllocator,
        queue: &wgpu::Queue,
    ) -> UnifiedSceneData {
        // Flush the final opaque solid batch.
        let index_end = self.indices.len();
        self.flush_solid_batch(index_end, None);

        Self::align_indices(&mut self.indices);
        Self::align_indices(&mut self.transparent_indices);

        let gpu_scene = Self::upload_scene(allocator, queue, &self.vertices, &self.indices);
        let transparent_gpu_scene = Self::upload_scene(
            allocator,
            queue,
            &self.transparent_vertices,
            &self.transparent_indices,
        );

        UnifiedSceneData {
            gpu_scene,
            solid_batches: self.solid_batches,
            transparent_gpu_scene,
            transparent_batches: self.transparent_batches,
            text_draws: self.text_draws,
            image_draws: self.image_draws,
            svg_draws: self.svg_draws,
            external_texture_draws: self.external_texture_draws,
        }
    }
}

/// Upload a DisplayList extracting all element types for unified rendering.
/// This is the main entry point for the unified rendering system.
///
/// Returns:
/// - GpuScene: Uploaded solid geometry (rectangles, paths, etc.)
/// - text_draws: Text runs with their transforms and z-indices
/// - image_draws: Image draws (currently placeholder, will be implemented)
/// - svg_draws: SVG draws (currently placeholder, will be implemented)
pub fn upload_display_list_unified(
    allocator: &mut RenderAllocator,
    queue: &wgpu::Queue,
    list: &DisplayList,
) -> Result<UnifiedSceneData> {
    let mut builder = UnifiedBuilder::new();
    for cmd in &list.commands {
        builder.handle(cmd);
    }
    Ok(builder.finalize(allocator, queue))
}
