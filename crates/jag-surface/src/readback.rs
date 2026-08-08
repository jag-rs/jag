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

use crate::{JagSurface, gpu_readback::ReadbackBuffer, gpu_texture};

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

    let bytes_per_pixel = 4u32;
    let bytes_per_row = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| anyhow::anyhow!("grab-last-frame row size overflow"))?;
    let readback = ReadbackBuffer::new(&device, "grab-last-frame-readback", bytes_per_row, height)?;

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("grab-last-frame-encoder"),
    });
    gpu_texture::copy_texture_to_buffer_2d(
        &mut encoder,
        &intermediate.texture,
        readback.buffer(),
        readback.padded_bytes_per_row(),
        [width, height],
    );
    queue.submit(std::iter::once(encoder.finish()));

    let mapped = readback.map_tightly_packed(&device)?;
    let mut pixels = Vec::with_capacity(mapped.len());
    for row in mapped.chunks_exact(bytes_per_row as usize) {
        append_rgba_row(&mut pixels, row, format);
    }

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
