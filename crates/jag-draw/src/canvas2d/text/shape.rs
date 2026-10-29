use super::*;
use harfrust::{Direction, Feature, FontRef, ShaperData, Tag, UnicodeBuffer};
use jag_text::{
    bidi::{BaseDirection, visual_runs},
    font::FontFace,
};
use swash::zeno::Verb;
use unicode_segmentation::UnicodeSegmentation;
pub(super) fn shape(
    fonts: &mut Fonts,
    text: &str,
    s: &TextStyle,
    primary: (fontdb::ID, FontFace),
) -> Result<(Commands, f64), String> {
    let base = if s.direction == "rtl" {
        BaseDirection::Rtl
    } else {
        BaseDirection::Ltr
    };
    let (_, runs) = visual_runs(text, base, 0..text.len());
    let mut path = Vec::new();
    let mut pen = 0.;
    for run in runs {
        let mut pieces: Vec<(fontdb::ID, FontFace, String, harfrust::Script)> = Vec::new();
        let mut previous = harfrust::script::LATIN;
        for cluster in text[run.range].graphemes(true) {
            let (id, face) = fonts
                .for_cluster(cluster, s, primary.clone())
                .ok_or("InvalidStateError")?;
            let mut buf = UnicodeBuffer::new();
            buf.push_str(cluster);
            buf.guess_segment_properties();
            let mut script = buf.script();
            if script == harfrust::script::COMMON || script == harfrust::script::INHERITED {
                script = previous;
            }
            previous = script;
            if let Some(last) = pieces.last_mut()
                && last.0 == id
                && last.3 == script
            {
                last.2.push_str(cluster);
            } else {
                pieces.push((id, face, cluster.into(), script));
            }
        }
        if run.level % 2 == 1 {
            pieces.reverse();
        }
        for (_, face, part, script) in pieces {
            let bytes = face.as_bytes();
            let f = FontRef::from_index(&bytes, face.index() as u32).map_err(|_| "SyntaxError")?;
            let data = ShaperData::new(&f);
            let shaper = data.shaper(&f).build();
            let mut buffer = UnicodeBuffer::new();
            buffer.push_str(&part);
            buffer.set_direction(if run.level % 2 == 1 {
                Direction::RightToLeft
            } else {
                Direction::LeftToRight
            });
            buffer.set_script(script);
            if let Ok(lang) = s.lang.parse() {
                buffer.set_language(lang);
            }
            let mut features = Vec::new();
            if s.kerning == "none" {
                features.push(Feature::new(Tag::new(b"kern"), 0, ..));
            }
            if s.letter != 0. {
                features.push(Feature::new(Tag::new(b"liga"), 0, ..));
                features.push(Feature::new(Tag::new(b"clig"), 0, ..));
            }
            let glyphs = shaper.shape(buffer, &features);
            let scale = s.size / f64::from(face.metrics().units_per_em);
            for (i, (info, pos)) in glyphs
                .glyph_infos()
                .iter()
                .zip(glyphs.glyph_positions())
                .enumerate()
            {
                if let Some(outline) = face.glyph_outline(info.glyph_id as u16, s.size as f32) {
                    let mut point = 0;
                    let points = outline.points();
                    for verb in outline.verbs() {
                        let (tag, count) = match verb {
                            Verb::MoveTo => (0., 1),
                            Verb::LineTo => (1., 1),
                            Verb::QuadTo => (2., 2),
                            Verb::CurveTo => (3., 3),
                            Verb::Close => (4., 0),
                        };
                        let mut c = vec![tag];
                        for p in &points[point..point + count] {
                            c.push(pen + f64::from(pos.x_offset) * scale + f64::from(p.x));
                            c.push(-f64::from(pos.y_offset) * scale - f64::from(p.y));
                        }
                        point += count;
                        path.push(c);
                    }
                }
                pen += f64::from(pos.x_advance) * scale;
                let cluster = info.cluster as usize;
                if glyphs
                    .glyph_infos()
                    .get(i + 1)
                    .is_none_or(|n| n.cluster != info.cluster)
                {
                    pen += s.letter;
                    if part[cluster..].starts_with(' ') {
                        pen += s.word;
                    }
                }
                if path.len() > 262144 {
                    return Err("QuotaExceededError".into());
                }
            }
        }
    }
    Ok((path, pen))
}
