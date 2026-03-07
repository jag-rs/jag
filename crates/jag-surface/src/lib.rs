//! jag-surface: Canvas-style API on top of jag-draw.

mod canvas;
pub mod shapes;
mod surface;

pub use canvas::{Canvas, ImageFitMode, RawImageDraw, ScrimDraw};
pub use surface::{CachedFrameData, JagSurface, get_last_raw_image_rect};

/// Resolve an asset path by checking multiple locations:
/// 1. Absolute path (as-is)
/// 2. `JAG_ASSETS_ROOT` override (if set) – supports a single
///    directory or a path-list (e.g. "dir1:dir2") using platform
///    `PATH` semantics.
/// 3. Relative to current directory
/// 4. In macOS app bundle Resources directory
/// 5. In app bundle Resources with just filename
pub fn resolve_asset_path(source: &std::path::Path) -> std::path::PathBuf {
    // If absolute path exists, use it
    if source.is_absolute() && source.exists() {
        return source.to_path_buf();
    }

    // Try JAG_ASSETS_ROOT override. This allows launchers (e.g. WAID dev)
    // to point Jag at one or more workspaces where shared assets like
    // `images/` and `fonts/` live, even when the current directory is
    // different from the Jag binary's source tree.
    //
    // The value may be a single directory or a platform-specific
    // path list (e.g. "dir1:dir2" on Unix).
    if let Ok(root_list) = std::env::var("JAG_ASSETS_ROOT") {
        for base in std::env::split_paths(&root_list) {
            let candidate = base.join(source);
            if candidate.exists() {
                return candidate;
            }

            if let Some(filename) = source.file_name() {
                let candidate = base.join(filename);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    // Try relative to current directory
    if source.exists() {
        return source.to_path_buf();
    }

    // Try in macOS app bundle Resources directory
    if let Ok(exe_path) = std::env::current_exe() {
        // exe is at App.app/Contents/MacOS/binary
        // resources are at App.app/Contents/Resources/
        if let Some(contents_dir) = exe_path.parent().and_then(|p| p.parent()) {
            let resources_path = contents_dir.join("Resources").join(source);
            if resources_path.exists() {
                return resources_path;
            }
            // Also try without the leading directory (e.g., "images/foo.png" -> "foo.png")
            if let Some(filename) = source.file_name() {
                let resources_path = contents_dir.join("Resources").join(filename);
                if resources_path.exists() {
                    return resources_path;
                }
            }
        }
    }

    // Return original path (caller will handle non-existent case)
    source.to_path_buf()
}
