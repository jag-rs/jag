//! Canvas text shaping, real outline metrics and paint geometry share one result.
mod fonts;
mod shape;
use super::geometry::Commands;
pub use fonts::Fonts;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize)]
pub struct TextStyle {
    pub families: Vec<String>,
    pub size: f64,
    pub weight: u16,
    pub italic: bool,
    pub direction: String,
    pub align: String,
    pub baseline: String,
    pub kerning: String,
    #[serde(default)]
    pub stretch: String,
    #[serde(default)]
    pub caps: String,
    #[serde(default)]
    pub rendering: String,
    pub letter: f64,
    pub word: f64,
    pub lang: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub width: f64,
    pub actual_bounding_box_left: f64,
    pub actual_bounding_box_right: f64,
    pub actual_bounding_box_ascent: f64,
    pub actual_bounding_box_descent: f64,
    pub font_bounding_box_ascent: f64,
    pub font_bounding_box_descent: f64,
    pub em_height_ascent: f64,
    pub em_height_descent: f64,
    pub hanging_baseline: f64,
    pub alphabetic_baseline: f64,
    pub ideographic_baseline: f64,
}
pub struct Prepared {
    pub path: Commands,
    pub metrics: Metrics,
}
impl TextStyle {
    pub fn valid(&self) -> bool {
        self.size.is_finite()
            && self.size >= 0.
            && self.size <= 4096.
            && self.letter.is_finite()
            && self.word.is_finite()
            && self.families.len() <= 64
    }
}
impl Fonts {
    pub fn prepare(&mut self, text: &str, s: &TextStyle) -> Result<Prepared, String> {
        if !s.valid() || text.len() > 65536 {
            return Err("QuotaExceededError".into());
        }
        let text: String = text
            .chars()
            .map(|c| {
                if matches!(c, '\t' | '\n' | '\r' | '\u{000c}') {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        let (primary, face) = self.primary(s).ok_or("InvalidStateError")?;
        let m = face.scaled_metrics(s.size as f32);
        let raw_ascent = f64::from(m.ascent).abs();
        let raw_descent = f64::from(m.descent).abs();
        let ascent = raw_ascent.round();
        let descent = raw_descent.round();
        let em_ascent = if raw_ascent + raw_descent > 0. {
            (raw_ascent / (raw_ascent + raw_descent) * s.size * 64.).round() / 64.
        } else {
            0.
        };
        let em_descent = s.size - em_ascent;
        let (mut path, width) = if text.is_empty() || s.size == 0. {
            (Vec::new(), 0.)
        } else {
            shape::shape(self, &text, s, (primary, face))?
        };
        let shift = match s.baseline.as_str() {
            "top" => em_ascent,
            "hanging" => ascent * 0.8,
            "middle" => (em_ascent - em_descent) / 2.,
            "ideographic" => -descent,
            "bottom" => -em_descent,
            _ => 0.,
        };
        let align = match s.align.as_str() {
            "center" => width / 2.,
            "right" => width,
            "start" if s.direction == "rtl" => width,
            "end" if s.direction != "rtl" => width,
            _ => 0.,
        };
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for command in &mut path {
            for pair in command[1..].chunks_exact_mut(2) {
                pair[0] -= align;
                pair[1] += shift;
            }
        }
        if let Some(p) = super::geometry::path(&path) {
            let b = p.compute_tight_bounds().unwrap_or(p.bounds());
            bounds = [
                b.left() as f64,
                b.top() as f64,
                b.right() as f64,
                b.bottom() as f64,
            ];
        }
        let ink = bounds.iter().all(|n| n.is_finite());
        Ok(Prepared {
            path,
            metrics: Metrics {
                width,
                actual_bounding_box_left: if ink { -bounds[0] } else { 0. },
                actual_bounding_box_right: if ink { bounds[2] } else { 0. },
                actual_bounding_box_ascent: if ink { -bounds[1] } else { 0. },
                actual_bounding_box_descent: if ink { bounds[3] } else { 0. },
                font_bounding_box_ascent: ascent - shift,
                font_bounding_box_descent: descent + shift,
                em_height_ascent: em_ascent - shift,
                em_height_descent: em_descent + shift,
                hanging_baseline: ascent * 0.8 - shift,
                alphabetic_baseline: -shift,
                ideographic_baseline: -descent - shift,
            },
        })
    }
}
