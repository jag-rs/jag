//! GPU → CPU readback helpers for `JagSurface`.
//!
//! The runner renders into [`JagSurface`]'s intermediate texture (allocated
//! by `PassManager::ensure_intermediate_texture` with `COPY_SRC` usage) and
//! blits it to the swapchain. After `end_frame` the intermediate retains
//! the rendered pixels until the next frame's clear, so we can copy it to
//! a CPU-mappable buffer between frames.
//!
//! This is the building block for `detir-scene`'s snapshot mode — the
//! interactive runner can call this after any rendered frame, get the same
//! bytes `JagSurface::end_frame_headless` produces, and write a PNG.

use anyhow::Result;

use jag_draw::wgpu;

use crate::JagSurface;

/// Copy the most-recently rendered intermediate texture into a tightly
/// packed RGBA byte buffer (same layout as `end_frame_headless` returns).
///
/// Requirements:
/// - `surface.set_use_intermediate(true)` must be in effect (the default).
///   Without it, `PassManager::intermediate_texture` is `None` and this
///   returns an error rather than silently producing garbage.
/// - Call after `end_frame` for the frame you want to grab and before the
///   next `begin_frame` (which will clear the intermediate).
///
/// Returns `(width, height, rgba_bytes)` where `rgba_bytes.len() == width *
/// height * 4`. The bytes are normalized to RGBA even when the underlying
/// surface/intermediate texture uses BGRA, which is common for swapchains.
pub fn grab_last_frame_rgba(surface: &mut JagSurface) -> Result<(u32, u32, Vec<u8>)> {
    // Take owned Arc clones first so we don't conflict with the
    // `&mut PassManager` borrow used to read the intermediate texture.
    let device = surface.device();
    let queue = surface.queue();

    let pass = surface.pass_manager();
    let intermediate = pass.intermediate_texture.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "no intermediate texture: enable use_intermediate before end_frame, \
             then call grab_last_frame_rgba immediately after end_frame"
        )
    })?;

    let width = intermediate.key.width;
    let height = intermediate.key.height;
    let format = intermediate.key.format;

    // wgpu requires bytes_per_row to be a multiple of
    // COPY_BYTES_PER_ROW_ALIGNMENT (256). Same padding rule as
    // `end_frame_headless`.
    let bytes_per_pixel = 4u32;
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let padded_bytes_per_row = (unpadded_bytes_per_row + 255) & !255;
    let buffer_size = (padded_bytes_per_row * height) as u64;

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("grab-last-frame-readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("grab-last-frame-encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &intermediate.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            result.expect("failed to map grab-last-frame buffer");
            tx.send(()).expect("failed to signal grab-last-frame");
        });
    device.poll(wgpu::Maintain::Wait);
    rx.recv()
        .map_err(|e| anyhow::anyhow!("grab-last-frame recv: {}", e))?;

    let mapped = readback.slice(..).get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * bytes_per_pixel) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + (width * bytes_per_pixel) as usize;
        append_rgba_row(&mut pixels, &mapped[start..end], format);
    }
    drop(mapped);
    readback.unmap();

    Ok((width, height, pixels))
}

fn append_rgba_row(out: &mut Vec<u8>, row: &[u8], format: wgpu::TextureFormat) {
    if matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for px in row.chunks_exact(4) {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    } else {
        out.extend_from_slice(row);
    }
}

#[cfg(test)]
mod tests {
    use jag_draw::wgpu;

    use super::append_rgba_row;

    #[test]
    fn readback_normalizes_bgra_rows_to_rgba() {
        let mut out = Vec::new();
        append_rgba_row(
            &mut out,
            &[0x08, 0xf0, 0xf4, 0xff, 0xf8, 0xfd, 0xff, 0xff],
            wgpu::TextureFormat::Bgra8UnormSrgb,
        );

        assert_eq!(out, vec![0xf4, 0xf0, 0x08, 0xff, 0xff, 0xfd, 0xf8, 0xff]);
    }

    #[test]
    fn readback_keeps_rgba_rows_unchanged() {
        let mut out = Vec::new();
        append_rgba_row(
            &mut out,
            &[0xf4, 0xf0, 0xe8, 0xff],
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );

        assert_eq!(out, vec![0xf4, 0xf0, 0xe8, 0xff]);
    }
}
