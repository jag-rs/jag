//! Font database for SVG `<text>` rasterization.
//!
//! SVG text is resolved to glyph outlines by usvg/resvg using a `fontdb`
//! database. `fontdb::Database::load_system_fonts()` finds nothing on platforms
//! whose fonts are not exposed on the filesystem paths it scans — notably iOS —
//! so `<text>` in SVGs (e.g. chart axis labels) renders invisibly there, even
//! though normal UI text drawn via the separate [`crate::TextProvider`] works.
//! Hosts call [`register_svg_fallback_font`] at startup with bundled font bytes
//! to give the rasterizer glyph outlines to draw.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

#[cfg(test)]
#[path = "svg_fontdb_tests.rs"]
mod svg_fontdb_tests;

/// Font database for SVG text rendering, paired with the family name to use as
/// the `usvg::Options::font_family` default. The default family lets text whose
/// requested family is unavailable fall back to a registered font instead of
/// vanishing.
#[derive(Clone)]
pub(crate) struct SvgFontDb {
    pub(crate) db: Arc<usvg::fontdb::Database>,
    pub(crate) default_family: Option<String>,
}

/// Host-registered fallback fonts for SVG text rasterization.
static REGISTERED_SVG_FONTS: Mutex<Vec<Arc<Vec<u8>>>> = Mutex::new(Vec::new());

/// Cached SVG font database (system fonts plus registered fallbacks), rebuilt
/// whenever a new fallback font is registered.
static SVG_FONTDB: RwLock<Option<SvgFontDb>> = RwLock::new(None);

/// Register fallback font data for SVG `<text>` rasterization.
///
/// Call this at startup, before the first SVG containing text is rendered.
/// Required on platforms where `fontdb` cannot see the system fonts (iOS);
/// harmless elsewhere, where it simply adds an extra fallback face.
pub fn register_svg_fallback_font(data: Vec<u8>) {
    REGISTERED_SVG_FONTS.lock().unwrap().push(Arc::new(data));
    // Invalidate the cache so the next render rebuilds with the new font.
    *SVG_FONTDB.write().unwrap() = None;
}

/// Resolve the (cached) SVG font database, building it on first use and after
/// any [`register_svg_fallback_font`] call.
pub(crate) fn svg_font_db() -> SvgFontDb {
    if let Some(cached) = SVG_FONTDB.read().unwrap().clone() {
        return cached;
    }
    let built = {
        let registered = REGISTERED_SVG_FONTS.lock().unwrap();
        build_svg_fontdb(true, registered.iter().map(|font| font.as_ref().as_slice()))
    };
    *SVG_FONTDB.write().unwrap() = Some(built.clone());
    built
}

/// Build an SVG font database from optionally-loaded system fonts plus the
/// given fallback font blobs.
///
/// When system fonts are unavailable (the iOS case), the first fallback face's
/// family is wired in as every generic family and returned as the default, so
/// `<text>` whose requested family is unknown still resolves to a real font
/// instead of being dropped. `load_system` is a parameter so that condition is
/// testable on hosts that *do* have system fonts.
pub(crate) fn build_svg_fontdb<'a>(
    load_system: bool,
    fallback_fonts: impl IntoIterator<Item = &'a [u8]>,
) -> SvgFontDb {
    let mut db = usvg::fontdb::Database::new();
    if load_system {
        db.load_system_fonts();
    }
    let system_empty = db.is_empty();
    for data in fallback_fonts {
        db.load_font_data(data.to_vec());
    }

    let default_family = if system_empty {
        let family = db
            .faces()
            .next()
            .and_then(|face| face.families.first().map(|(name, _)| name.clone()));
        if let Some(family) = family.clone() {
            db.set_serif_family(family.clone());
            db.set_sans_serif_family(family.clone());
            db.set_monospace_family(family.clone());
            db.set_cursive_family(family.clone());
            db.set_fantasy_family(family);
        }
        family
    } else {
        None
    };

    SvgFontDb {
        db: Arc::new(db),
        default_family,
    }
}

/// Rasterize SVG bytes to an RGBA pixmap at `scale`, using `fonts` for `<text>`.
///
/// Shared by the GPU upload path and tests so the font-resolution behaviour is
/// verifiable without a GPU device. Returns `None` if the SVG fails to parse or
/// its scaled size is degenerate or exceeds `max_tex_size`.
pub(crate) fn render_svg_to_pixmap(
    data: &[u8],
    scale: f32,
    fonts: &SvgFontDb,
    resources_dir: Option<PathBuf>,
    max_tex_size: u32,
) -> Option<tiny_skia::Pixmap> {
    let mut opt = usvg::Options {
        resources_dir,
        fontdb: fonts.db.clone(),
        ..usvg::Options::default()
    };
    if let Some(family) = fonts.default_family.clone() {
        opt.font_family = family;
    }
    let tree = usvg::Tree::from_data(data, &opt).ok()?;
    let size = tree.size().to_int_size();
    let (w0, h0): (u32, u32) = (size.width().max(1), size.height().max(1));
    let w = ((w0 as f32) * scale).round() as u32;
    let h = ((h0 as f32) * scale).round() as u32;
    if w == 0 || h == 0 || w > max_tex_size || h > max_tex_size {
        return None;
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let ts = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, ts, &mut pixmap.as_mut());
    Some(pixmap)
}
