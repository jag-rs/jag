//! Surface-owned command encoding and submission operations.

use jag_draw::wgpu;

pub(crate) struct CommandEncoderDescriptor<'a> {
    pub(crate) label: Option<&'a str>,
}

pub(crate) fn create_command_encoder(
    device: &wgpu::Device,
    descriptor: CommandEncoderDescriptor<'_>,
) -> wgpu::CommandEncoder {
    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: descriptor.label,
    })
}

pub(crate) fn submit_command_encoder(queue: &wgpu::Queue, encoder: wgpu::CommandEncoder) {
    queue.submit(std::iter::once(encoder.finish()));
}

#[cfg(test)]
mod tests {
    #[test]
    fn jag_surface_cannot_bypass_command_submission_seam() {
        fn rust_sources(root: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(root).expect("read jag-surface source directory") {
                let path = entry.expect("read jag-surface source entry").path();
                if path.is_dir() {
                    rust_sources(&path, files);
                } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                    files.push(path);
                }
            }
        }

        let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let seam_path = source_root.join("gpu_commands.rs");
        let mut sources = Vec::new();
        rust_sources(&source_root, &mut sources);
        let mut checked_files = 0;
        for path in sources {
            if path == seam_path {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read jag-surface source");
            for prohibited in [".create_command_encoder(", ".submit("] {
                assert!(
                    !source.contains(prohibited),
                    "{} bypasses the command-submission seam with {prohibited}",
                    path.display()
                );
            }
            for line in source.lines().filter(|line| line.contains(".finish()")) {
                assert!(
                    line.contains("canvas.painter.finish()"),
                    "{} bypasses command-buffer finish isolation: {line}",
                    path.display()
                );
            }
            checked_files += 1;
        }
        assert!(
            checked_files > 20,
            "recursive jag-surface scan was incomplete"
        );
    }
}
