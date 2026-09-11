use std::{path::Path, time::SystemTime};

/// A disk asset revision. Virtual textures deliberately have no disk stamp;
/// their owner must supply an explicit revision instead of freezing pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AssetStamp {
    len: u64,
    modified: SystemTime,
}

impl AssetStamp {
    pub fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }
}
