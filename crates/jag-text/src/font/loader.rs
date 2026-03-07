use std::path::{Path, PathBuf};
use std::sync::Arc;

use hashbrown::HashMap;

use crate::font::{FontError, FontFace, Result};

/// Key for identifying a font within the cache.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct FontKey {
    /// Path to the font file on disk.
    pub path: PathBuf,
    /// Font index within the file (for collections).
    pub index: u32,
}

impl FontKey {
    pub fn new(path: impl AsRef<Path>, index: usize) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            index: index as u32,
        }
    }
}

/// Simple in-memory font cache keyed by file path and index.
#[derive(Debug, Default)]
pub struct FontCache {
    fonts: HashMap<FontKey, Arc<FontFace>>,
}

impl FontCache {
    pub fn new() -> Self {
        Self {
            fonts: HashMap::new(),
        }
    }

    /// Get a font face from the cache or load it from disk.
    pub fn get_or_load(&mut self, path: impl AsRef<Path>, index: usize) -> Result<Arc<FontFace>> {
        let key = FontKey::new(&path, index);
        if let Some(face) = self.fonts.get(&key) {
            return Ok(face.clone());
        }

        let face = Arc::new(FontFace::from_path(&key.path, index)?);
        self.fonts.insert(key, face.clone());
        Ok(face)
    }

    /// Insert an already constructed font face with an explicit key.
    pub fn insert(&mut self, key: FontKey, face: Arc<FontFace>) {
        self.fonts.insert(key, face);
    }

    /// Retrieve a font by key if it exists.
    pub fn get(&self, key: &FontKey) -> Option<Arc<FontFace>> {
        self.fonts.get(key).cloned()
    }
}

/// Load a font face directly from disk without caching.
pub fn load_font(path: impl AsRef<Path>, index: usize) -> Result<FontFace> {
    FontFace::from_path(path, index)
}

/// Load a reasonable default system sans-serif font using `fontdb`.
///
/// This mirrors the selection used by `engine-core::DetirTextProvider::from_system_fonts`
/// so that layout metrics match the primary rendering path.
pub fn load_system_default_font() -> Result<FontFace> {
    use fontdb::{Database, Family, Query, Source, Stretch, Style, Weight};

    let mut db = Database::new();
    db.load_system_fonts();

    let id = db
        .query(&Query {
            families: &[
                Family::Name("SF Pro Text".into()),
                Family::Name("Segoe UI".into()),
                Family::SansSerif,
                Family::Name("Arial".into()),
            ],
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
            ..Query::default()
        })
        .ok_or(FontError::InvalidFont)?;

    let face = db.face(id).ok_or(FontError::InvalidFont)?;

    let bytes: Vec<u8> = match &face.source {
        Source::File(path) => std::fs::read(path)?,
        Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
    };

    FontFace::from_vec(bytes, face.index as usize)
}
