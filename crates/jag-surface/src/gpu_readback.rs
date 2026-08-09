use std::sync::mpsc;

use anyhow::{Context, Result, anyhow, bail};

use jag_draw::wgpu;

const ROW_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

pub(crate) struct ReadbackBuffer {
    buffer: wgpu::Buffer,
    layout: ReadbackLayout,
}

impl ReadbackBuffer {
    pub(crate) fn new(
        device: &wgpu::Device,
        label: &str,
        bytes_per_row: u32,
        height: u32,
    ) -> Result<Self> {
        let layout = ReadbackLayout::new(bytes_per_row, height)?;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: layout.buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok(Self { buffer, layout })
    }

    pub(crate) fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub(crate) fn padded_bytes_per_row(&self) -> u32 {
        self.layout.padded_bytes_per_row
    }

    pub(crate) fn map_tightly_packed(&self, device: &wgpu::Device) -> Result<Vec<u8>> {
        let slice = self.buffer.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        wait_for_gpu(device);

        receiver
            .recv()
            .context("readback mapping callback did not complete")?
            .context("GPU readback buffer mapping failed")?;

        let mapped = slice
            .get_mapped_range()
            .context("GPU readback mapped range is unavailable")?;
        let pixels = self.layout.strip_padding(&mapped);
        drop(mapped);
        self.buffer.unmap();
        pixels
    }
}

pub(crate) fn wait_for_gpu(device: &wgpu::Device) {
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadbackLayout {
    bytes_per_row: u32,
    padded_bytes_per_row: u32,
    height: u32,
    buffer_size: u64,
}

impl ReadbackLayout {
    fn new(bytes_per_row: u32, height: u32) -> Result<Self> {
        if bytes_per_row == 0 {
            bail!("readback bytes per row must be greater than zero");
        }
        if height == 0 {
            bail!("readback height must be greater than zero");
        }

        let padded_bytes_per_row = bytes_per_row
            .checked_add(ROW_ALIGNMENT - 1)
            .ok_or_else(|| anyhow!("readback row alignment overflow for {bytes_per_row} bytes"))?
            & !(ROW_ALIGNMENT - 1);
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or_else(|| anyhow!("readback buffer size overflow"))?;

        Ok(Self {
            bytes_per_row,
            padded_bytes_per_row,
            height,
            buffer_size,
        })
    }

    fn strip_padding(&self, mapped: &[u8]) -> Result<Vec<u8>> {
        let expected_len = usize::try_from(self.buffer_size)
            .context("readback buffer is too large for this platform")?;
        if mapped.len() != expected_len {
            bail!(
                "readback mapping length mismatch: expected {expected_len} bytes, got {}",
                mapped.len()
            );
        }

        let output_len = usize::try_from(
            u64::from(self.bytes_per_row)
                .checked_mul(u64::from(self.height))
                .ok_or_else(|| anyhow!("readback output size overflow"))?,
        )
        .context("readback output is too large for this platform")?;
        let row_len = self.bytes_per_row as usize;
        let padded_row_len = self.padded_bytes_per_row as usize;
        let mut output = Vec::with_capacity(output_len);
        for row in mapped.chunks_exact(padded_row_len) {
            output.extend_from_slice(&row[..row_len]);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::{ROW_ALIGNMENT, ReadbackLayout};

    #[test]
    fn layout_aligns_rows_and_sizes_the_buffer() {
        let layout = ReadbackLayout::new(257, 3).unwrap();

        assert_eq!(layout.bytes_per_row, 257);
        assert_eq!(layout.padded_bytes_per_row, ROW_ALIGNMENT * 2);
        assert_eq!(layout.buffer_size, u64::from(ROW_ALIGNMENT * 2 * 3));
    }

    #[test]
    fn layout_keeps_aligned_rows_unchanged() {
        let layout = ReadbackLayout::new(ROW_ALIGNMENT, 2).unwrap();

        assert_eq!(layout.padded_bytes_per_row, ROW_ALIGNMENT);
        assert_eq!(layout.buffer_size, u64::from(ROW_ALIGNMENT * 2));
    }

    #[test]
    fn layout_rejects_empty_and_overflowing_dimensions() {
        assert!(ReadbackLayout::new(0, 1).is_err());
        assert!(ReadbackLayout::new(4, 0).is_err());
        assert!(ReadbackLayout::new(u32::MAX, 1).is_err());
    }

    #[test]
    fn strip_padding_returns_tightly_packed_rows() {
        let layout = ReadbackLayout::new(3, 2).unwrap();
        let mut mapped = vec![0; layout.buffer_size as usize];
        mapped[..3].copy_from_slice(&[1, 2, 3]);
        let second_row = layout.padded_bytes_per_row as usize;
        mapped[second_row..second_row + 3].copy_from_slice(&[4, 5, 6]);

        assert_eq!(layout.strip_padding(&mapped).unwrap(), [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn strip_padding_rejects_an_incomplete_mapping() {
        let layout = ReadbackLayout::new(4, 1).unwrap();

        assert!(layout.strip_padding(&[0; 4]).is_err());
    }
}
