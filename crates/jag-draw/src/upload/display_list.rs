use anyhow::Result;

use crate::allocator::{BufKey, RenderAllocator};
use crate::display_list::{Command, DisplayList};
use crate::scene::Brush;

use super::gradients::{
    push_ellipse, push_ellipse_radial_gradient, push_rect_conic_gradient,
    push_rect_linear_gradient, push_rounded_rect_conic_gradient, push_rounded_rect_linear_gradient,
    push_rounded_rect_radial_gradient,
};
use super::shapes::{push_rect_stroke, push_rounded_rect, push_rounded_rect_stroke};
use super::tessellate::{tessellate_path_fill, tessellate_path_stroke};
use super::types::{GpuScene, Vertex};
use super::verts::rect_to_verts;

pub fn upload_display_list(
    allocator: &mut RenderAllocator,
    queue: &wgpu::Queue,
    list: &DisplayList,
) -> Result<GpuScene> {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();

    // NOTE: Z-index sorting disabled because it breaks clip/transform stacks.
    // For proper z-ordering, we need to either:
    // 1. Use a depth buffer, or
    // 2. Ensure commands are emitted in the correct z-order from the start
    // let mut sorted_list = list.clone();
    // sorted_list.sort_by_z();

    for cmd in &list.commands {
        match cmd {
            Command::DrawRect {
                rect,
                brush,
                transform,
                z,
                ..
            } => {
                match brush {
                    Brush::Solid(col) => {
                        let color = [col.r, col.g, col.b, col.a];
                        let (v, i) = rect_to_verts(*rect, color, *transform, *z as f32);
                        let base = vertices.len() as u16;
                        vertices.extend_from_slice(&v);
                        indices.extend(i.iter().map(|idx| base + idx));
                    }
                    Brush::LinearGradient { stops, .. } => {
                        // Only handle horizontal gradients for now: map t along x within rect
                        let mut packed: Vec<(f32, [f32; 4])> = stops
                            .iter()
                            .map(|(tpos, c)| (*tpos, [c.r, c.g, c.b, c.a]))
                            .collect();
                        if packed.is_empty() {
                            continue;
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
                        push_rect_linear_gradient(
                            &mut vertices,
                            &mut indices,
                            *rect,
                            &packed,
                            *transform,
                            *z as f32,
                        );
                    }
                    Brush::ConicGradient {
                        center,
                        start_angle,
                        stops,
                    } => {
                        let packed: Vec<(f32, [f32; 4])> = stops
                            .iter()
                            .map(|(tpos, c)| (*tpos, [c.r, c.g, c.b, c.a]))
                            .collect();
                        if packed.is_empty() {
                            continue;
                        }
                        push_rect_conic_gradient(
                            &mut vertices,
                            &mut indices,
                            *rect,
                            *center,
                            *start_angle,
                            &packed,
                            *z as f32,
                            *transform,
                        );
                    }
                    _ => {}
                }
            }
            Command::DrawRoundedRect {
                rrect,
                brush,
                transform,
                z,
                ..
            } => match brush {
                Brush::Solid(col) => {
                    let color = [col.r, col.g, col.b, col.a];
                    push_rounded_rect(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        color,
                        *z as f32,
                        *transform,
                    );
                }
                Brush::LinearGradient { start, end, stops } => {
                    let packed: Vec<(f32, [f32; 4])> = stops
                        .iter()
                        .map(|(tpos, c)| (*tpos, [c.r, c.g, c.b, c.a]))
                        .collect();
                    if packed.is_empty() {
                        continue;
                    }
                    push_rounded_rect_linear_gradient(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        *start,
                        *end,
                        &packed,
                        *z as f32,
                        *transform,
                    );
                }
                Brush::RadialGradient {
                    center,
                    radius,
                    stops,
                } => {
                    let packed: Vec<(f32, [f32; 4])> = stops
                        .iter()
                        .map(|(tpos, c)| (*tpos, [c.r, c.g, c.b, c.a]))
                        .collect();
                    if packed.is_empty() {
                        continue;
                    }
                    push_rounded_rect_radial_gradient(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        *center,
                        *radius,
                        &packed,
                        *z as f32,
                        *transform,
                    );
                }
                Brush::ConicGradient {
                    center,
                    start_angle,
                    stops,
                } => {
                    let packed: Vec<(f32, [f32; 4])> = stops
                        .iter()
                        .map(|(tpos, c)| (*tpos, [c.r, c.g, c.b, c.a]))
                        .collect();
                    if packed.is_empty() {
                        continue;
                    }
                    push_rounded_rect_conic_gradient(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        *center,
                        *start_angle,
                        &packed,
                        *z as f32,
                        *transform,
                    );
                }
            },
            Command::StrokeRect {
                rect,
                stroke,
                brush,
                transform,
                z,
                ..
            } => {
                if let Brush::Solid(col) = brush {
                    let color = [col.r, col.g, col.b, col.a];
                    push_rect_stroke(
                        &mut vertices,
                        &mut indices,
                        *rect,
                        *stroke,
                        color,
                        *z as f32,
                        *transform,
                    );
                }
            }
            Command::StrokeRoundedRect {
                rrect,
                stroke,
                brush,
                transform,
                z,
                ..
            } => {
                if let Brush::Solid(col) = brush {
                    let color = [col.r, col.g, col.b, col.a];
                    push_rounded_rect_stroke(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        *stroke,
                        color,
                        *z as f32,
                        *transform,
                    );
                }
            }
            Command::DrawEllipse {
                center,
                radii,
                brush,
                transform,
                z,
                ..
            } => match brush {
                Brush::Solid(col) => {
                    let color = [col.r, col.g, col.b, col.a];
                    push_ellipse(
                        &mut vertices,
                        &mut indices,
                        *center,
                        *radii,
                        color,
                        *z as f32,
                        *transform,
                    );
                }
                Brush::RadialGradient {
                    center: _gcenter,
                    radius: _r,
                    stops,
                } => {
                    let mut packed: Vec<(f32, [f32; 4])> = stops
                        .iter()
                        .map(|(t, c)| (*t, [c.r, c.g, c.b, c.a]))
                        .collect();
                    if packed.is_empty() {
                        continue;
                    }
                    if packed.first().unwrap().0 > 0.0 {
                        let c = packed.first().unwrap().1;
                        packed.insert(0, (0.0, c));
                    }
                    if packed.last().unwrap().0 < 1.0 {
                        let c = packed.last().unwrap().1;
                        packed.push((1.0, c));
                    }
                    push_ellipse_radial_gradient(
                        &mut vertices,
                        &mut indices,
                        *center,
                        *radii,
                        &packed,
                        *z as f32,
                        *transform,
                    );
                }
                _ => {}
            },
            Command::FillPath {
                path,
                color,
                transform,
                z,
                clip,
            } => {
                let col = [color.r, color.g, color.b, color.a];
                tessellate_path_fill(
                    &mut vertices,
                    &mut indices,
                    path,
                    col,
                    *z as f32,
                    *transform,
                    *clip,
                );
            }
            Command::StrokePath {
                path,
                stroke,
                color,
                transform,
                z,
                clip,
            } => {
                let col = [color.r, color.g, color.b, color.a];
                tessellate_path_stroke(
                    &mut vertices,
                    &mut indices,
                    path,
                    *stroke,
                    col,
                    *z as f32,
                    *transform,
                    *clip,
                );
            }
            // BoxShadow commands are handled by PassManager as a separate pipeline.
            Command::BoxShadow { .. } => {}
            // Hit-only regions: intentionally not rendered.
            Command::HitRegionRect { .. } => {}
            Command::HitRegionRoundedRect { .. } => {}
            Command::HitRegionEllipse { .. } => {}
            _ => {}
        }
    }

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

    // Allocate GPU buffers and upload
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
        queue.write_buffer(&vbuf.buffer, 0, bytemuck::cast_slice(&vertices));
    }
    if isize > 0 {
        queue.write_buffer(&ibuf.buffer, 0, bytemuck::cast_slice(&indices));
    }

    Ok(GpuScene {
        vertex: vbuf,
        index: ibuf,
        vertices: vertices.len() as u32,
        indices: indices.len() as u32,
    })
}
