use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::allocator::{BufKey, OwnedBuffer, RenderAllocator};

const MIN_BUFFER_SIZE: u64 = wgpu::COPY_BUFFER_ALIGNMENT;
const EMPTY_BUFFER_CONTENTS: [u8; MIN_BUFFER_SIZE as usize] = [0; MIN_BUFFER_SIZE as usize];

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum BufferTransferError {
    #[error("buffer upload length {byte_len} is not aligned to {alignment} bytes")]
    UnalignedUpload { byte_len: usize, alignment: u64 },
    #[error("buffer upload length does not fit in a 64-bit GPU buffer size")]
    SizeOverflow,
}

fn checked_upload_size(byte_len: usize) -> Result<u64, BufferTransferError> {
    let alignment = wgpu::COPY_BUFFER_ALIGNMENT;
    if byte_len != 0 && byte_len % alignment as usize != 0 {
        return Err(BufferTransferError::UnalignedUpload {
            byte_len,
            alignment,
        });
    }
    u64::try_from(byte_len)
        .map(|size| size.max(MIN_BUFFER_SIZE))
        .map_err(|_| BufferTransferError::SizeOverflow)
}

/// Allocate a pooled draw buffer and upload its complete initial contents.
///
/// Empty uploads still receive a valid minimum-sized buffer. Non-empty writes
/// are checked before reaching wgpu so malformed geometry returns an
/// attributable error instead of triggering a backend validation failure.
pub(crate) fn allocate_pooled_upload(
    allocator: &mut RenderAllocator,
    queue: &wgpu::Queue,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> Result<OwnedBuffer, BufferTransferError> {
    let buffer = allocator.allocate_buffer(BufKey {
        size: checked_upload_size(contents.len())?,
        usage: usage | wgpu::BufferUsages::COPY_DST,
    });
    if !contents.is_empty() {
        queue.write_buffer(&buffer.buffer, 0, contents);
    }
    Ok(buffer)
}

fn initial_contents(contents: &[u8]) -> &[u8] {
    if contents.is_empty() {
        &EMPTY_BUFFER_CONTENTS
    } else {
        contents
    }
}

/// Create a labeled, non-pooled draw buffer with complete initial contents.
///
/// `DeviceExt` owns backend copy padding on both the current and selected wgpu
/// families. This wrapper additionally keeps empty geometry nonzero and
/// preserves `COPY_DST` for callers that replace the initial contents later.
pub(crate) fn create_transient_upload(
    device: &wgpu::Device,
    label: &str,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: initial_contents(contents),
        usage: usage | wgpu::BufferUsages::COPY_DST,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_sizes_are_nonzero_and_copy_aligned() {
        assert_eq!(checked_upload_size(0), Ok(MIN_BUFFER_SIZE));
        assert_eq!(checked_upload_size(4), Ok(4));
        assert_eq!(checked_upload_size(16), Ok(16));
        assert_eq!(
            checked_upload_size(6),
            Err(BufferTransferError::UnalignedUpload {
                byte_len: 6,
                alignment: wgpu::COPY_BUFFER_ALIGNMENT,
            })
        );
    }

    #[test]
    fn transient_initial_contents_preserve_data_and_fill_empty_buffers() {
        let data = [1, 2, 3];
        assert_eq!(initial_contents(&data), data);
        assert_eq!(initial_contents(&[]), EMPTY_BUFFER_CONTENTS);
    }
}
