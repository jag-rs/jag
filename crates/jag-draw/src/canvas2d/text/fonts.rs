use super::*;
use fontdb::{Database, Family, ID, Query, Style, Weight};
use jag_text::font::FontFace;
use std::{collections::HashMap, sync::OnceLock};
pub struct Fonts {
    db: Database,
    cache: HashMap<ID, FontFace>,
    bytes: usize,
    registered: HashMap<ID, usize>,
}
impl Default for Fonts {
    fn default() -> Self {
        static SYSTEM: OnceLock<Database> = OnceLock::new();
        let mut db = SYSTEM
            .get_or_init(|| {
                let mut d = Database::new();
                d.load_system_fonts();
                d
            })
            .clone();
        let empty = db.is_empty();
        let mut bytes = 0;
        for data in crate::svg_fontdb::registered_fallback_fonts() {
            if bytes + data.len() <= 64 * 1024 * 1024 {
                bytes += data.len();
                db.load_font_data(data.as_ref().clone());
            }
        }
        let fallback_name = db
            .faces()
            .next()
            .and_then(|f| f.families.first())
            .map(|(n, _)| n.clone());
        if empty && let Some(name) = fallback_name {
            db.set_sans_serif_family(name.clone());
            db.set_serif_family(name.clone());
            db.set_monospace_family(name);
        }
        Self {
            db,
            cache: HashMap::new(),
            bytes,
            registered: HashMap::new(),
        }
    }
}
impl Fonts {
    pub fn register(
        &mut self,
        family: &str,
        bytes: Vec<u8>,
        weight: u16,
        italic: bool,
    ) -> Result<(), String> {
        if bytes.len() > 16 * 1024 * 1024 || self.bytes + bytes.len() > 64 * 1024 * 1024 {
            return Err("QuotaExceededError".into());
        }
        FontFace::from_bytes(bytes.clone().into(), 0).map_err(|_| "SyntaxError")?;
        self.remove(family, weight, italic);
        let cost = bytes.len();
        let previous: Vec<_> = self.db.faces().map(|f| f.id).collect();
        self.bytes += bytes.len();
        self.db.load_font_data(bytes);
        let ids: Vec<_> = self
            .db
            .faces()
            .filter(|f| !previous.contains(&f.id))
            .map(|f| f.id)
            .collect();
        let mut first = true;
        for id in ids {
            if let Some(mut face) = self.db.face(id).cloned() {
                face.families = vec![(family.into(), fontdb::Language::English_UnitedStates)];
                face.weight = Weight(weight);
                face.style = if italic { Style::Italic } else { Style::Normal };
                self.db.remove_face(id);
                let id = self.db.push_face_info(face);
                self.registered.insert(id, if first { cost } else { 0 });
                first = false;
            }
        }
        Ok(())
    }
    pub fn remove(&mut self, family: &str, weight: u16, italic: bool) {
        let ids: Vec<_> = self
            .db
            .faces()
            .filter(|f| {
                self.registered.contains_key(&f.id)
                    && f.families.iter().any(|(n, _)| n == family)
                    && f.weight == Weight(weight)
                    && (f.style != Style::Normal) == italic
            })
            .map(|f| f.id)
            .collect();
        for id in ids {
            self.db.remove_face(id);
            self.bytes = self
                .bytes
                .saturating_sub(self.registered.remove(&id).unwrap_or(0));
            if let Some(face) = self.cache.remove(&id) {
                self.bytes = self.bytes.saturating_sub(face.as_bytes().len());
            }
        }
    }
    pub fn validate(bytes: &[u8]) -> bool {
        FontFace::from_bytes(bytes.into(), 0).is_ok()
    }
    fn face(&mut self, id: ID) -> Option<FontFace> {
        if !self.cache.contains_key(&id) {
            if self.cache.len() >= 128 {
                return None;
            }
            let data = self
                .db
                .with_face_data(id, |data, index| (data.to_vec(), index))?;
            if self.bytes + data.0.len() > 64 * 1024 * 1024 {
                return None;
            }
            self.bytes += data.0.len();
            let face = FontFace::from_vec(data.0, data.1 as usize).ok()?;
            self.cache.insert(id, face);
        }
        self.cache.get(&id).cloned()
    }
    pub(super) fn primary(&mut self, s: &TextStyle) -> Option<(ID, FontFace)> {
        let names: Vec<_> = s
            .families
            .iter()
            .map(|n| match n.to_ascii_lowercase().as_str() {
                "serif" => Family::Serif,
                "sans-serif" | "system-ui" => Family::SansSerif,
                "monospace" => Family::Monospace,
                "cursive" => Family::Cursive,
                "fantasy" => Family::Fantasy,
                _ => Family::Name(n),
            })
            .collect();
        let id = self
            .db
            .query(&Query {
                families: &names,
                weight: Weight(s.weight),
                style: if s.italic {
                    Style::Italic
                } else {
                    Style::Normal
                },
                ..Default::default()
            })
            .or_else(|| {
                self.db.query(&Query {
                    families: &[Family::SansSerif],
                    ..Default::default()
                })
            })?;
        Some((id, self.face(id)?))
    }
    pub(super) fn for_cluster(
        &mut self,
        cluster: &str,
        s: &TextStyle,
        primary: (ID, FontFace),
    ) -> Option<(ID, FontFace)> {
        let covers = |f: &FontFace| {
            let bytes = f.as_bytes();
            let font = swash::FontRef::from_index(&bytes, f.index()).unwrap();
            cluster.chars().all(|c| {
                c.is_whitespace()
                    || matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')
                    || font.charmap().map(c) != 0
            })
        };
        if covers(&primary.1) {
            return Some(primary);
        }
        for name in &s.families {
            if let Some(id) = self.db.query(&Query {
                families: &[Family::Name(name)],
                weight: Weight(s.weight),
                ..Default::default()
            }) && let Some(face) = self.face(id)
                && covers(&face)
            {
                return Some((id, face));
            }
        }
        let ids: Vec<_> = self.db.faces().map(|f| f.id).collect();
        // Inspect cmaps before caching whole fallback files.
        for id in ids {
            let yes = self
                .db
                .with_face_data(id, |data, index| {
                    swash::FontRef::from_index(data, index as usize).is_some_and(|font| {
                        cluster.chars().all(|c| {
                            c.is_whitespace()
                                || matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')
                                || font.charmap().map(c) != 0
                        })
                    })
                })
                .unwrap_or(false);
            if yes && let Some(face) = self.face(id) {
                return Some((id, face));
            }
        }
        Some(primary)
    }
}
