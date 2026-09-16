//! Content-space ink bounds and tile bins, computed once per paint commit.
use super::*;

pub(super) struct RasterRun {
    pub commands: Vec<Command>,
    pub tight: Option<Rect>,
    pub z: Option<i32>,
    bins: HashMap<(i32, i32), Vec<usize>>,
    large: Vec<usize>,
    bounds: Vec<Option<Rect>>,
}

impl RasterRun {
    pub fn new(
        commands: Vec<Command>,
        scale: f32,
        dpi: f32,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
    ) -> Self {
        let z = commands.iter().filter_map(Command::z_index).min();
        let bounds: Vec<_> = commands
            .iter()
            .map(|c| ink_bounds(c, dpi, provider))
            .collect();
        let tight = bounds
            .iter()
            .copied()
            .collect::<Option<Vec<_>>>()
            .and_then(|bounds| {
                bounds
                    .into_iter()
                    .filter(|b| b.w > 0.0 && b.h > 0.0)
                    .reduce(union)
            });
        let mut bins: HashMap<_, Vec<usize>> = HashMap::new();
        let mut large = Vec::new();
        for (index, bound) in bounds.iter().enumerate() {
            let Some(b) = bound else {
                large.push(index);
                continue;
            };
            if b.w <= 0.0 || b.h <= 0.0 {
                continue;
            }
            let x0 = ((b.x * scale - GUTTER as f32) / TILE as f32).floor() as i32;
            let y0 = ((b.y * scale - GUTTER as f32) / TILE as f32).floor() as i32;
            let x1 = (((b.x + b.w) * scale + GUTTER as f32) / TILE as f32).ceil() as i32;
            let y1 = (((b.y + b.h) * scale + GUTTER as f32) / TILE as f32).ceil() as i32;
            if i64::from(x1)
                .saturating_sub(i64::from(x0))
                .saturating_mul(i64::from(y1).saturating_sub(i64::from(y0)))
                > 256
            {
                large.push(index);
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    bins.entry((x, y)).or_default().push(index);
                }
            }
        }
        Self {
            commands,
            tight,
            z,
            bins,
            large,
            bounds,
        }
    }

    pub fn tile_commands(&self, x: i32, y: i32, tile: Rect) -> Vec<Command> {
        let mut indices = self.bins.get(&(x, y)).cloned().unwrap_or_default();
        indices.extend(self.large.iter().copied().filter(|i| {
            self.bounds[*i].is_none_or(|b| {
                let overlap = intersect(b, tile);
                overlap.w > 0.0 && overlap.h > 0.0
            })
        }));
        indices.sort_unstable();
        indices
            .into_iter()
            .map(|i| self.commands[i].clone())
            .collect()
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect {
        x: a.x.min(b.x),
        y: a.y.min(b.y),
        w: (a.x + a.w).max(b.x + b.w) - a.x.min(b.x),
        h: (a.y + a.h).max(b.y + b.h) - a.y.min(b.y),
    }
}

fn ink_bounds(
    command: &Command,
    dpi: f32,
    provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
) -> Option<Rect> {
    match command {
        Command::DrawText { run, transform, .. } => {
            let provider = provider?;
            let [a, b, c, d, e, f] = transform.m;
            let sx = (a * a + b * b).sqrt();
            let sy = (c * c + d * d).sqrt();
            let scale = if sx > 0.0 && sy > 0.0 {
                (sx + sy) * 0.5
            } else {
                sx.max(sy).max(1.0)
            };
            let logical_size = (run.size * scale).max(1.0);
            let physical = jag_draw::TextRun {
                text: run.text.clone(),
                pos: [0.0, 0.0],
                size: (logical_size * dpi).max(1.0),
                logical_size,
                color: run.color,
                weight: run.weight,
                style: run.style,
                family: run.family.clone(),
            };
            let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &physical);
            let origin = [
                a * run.pos[0] + c * run.pos[1] + e,
                b * run.pos[0] + d * run.pos[1] + f,
            ];
            Some(
                glyphs
                    .iter()
                    .map(|glyph| {
                        let (w, h) = match &glyph.mask {
                            jag_draw::GlyphMask::Color(mask) => (mask.width, mask.height),
                            jag_draw::GlyphMask::Subpixel(mask) => (mask.width, mask.height),
                        };
                        let mut x = origin[0] + glyph.offset[0] / dpi;
                        let mut y = origin[1] + glyph.offset[1] / dpi;
                        if logical_size <= 15.0 {
                            x = (x * dpi).round() / dpi;
                            y = (y * dpi).round() / dpi;
                        }
                        Rect {
                            x,
                            y,
                            w: w as f32 / dpi,
                            h: h as f32 / dpi,
                        }
                    })
                    .reduce(union)
                    .unwrap_or(Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 0.0,
                        h: 0.0,
                    }),
            )
        }
        Command::BoxShadow {
            rrect,
            spec,
            transform,
            ..
        } => {
            // Match the actual shader quad, including its antialias padding.
            // Generic command_bounds omits that padding and uses different
            // nonuniform-transform geometry. Unknown bounds here used to put
            // every offscreen shadow into every visible tile and prevent tight
            // allocation of an entire raster run.
            let shadow =
                jag_draw::ShadowInstance::from_box_shadow(*rrect, *spec, 0, *transform, None);
            let reach = 3.0 * shadow.params[0].max(0.0) + 1.5;
            Some(Rect {
                x: shadow.lower[0] - reach,
                y: shadow.lower[1] - reach,
                w: shadow.upper[0] - shadow.lower[0] + 2.0 * reach,
                h: shadow.upper[1] - shadow.lower[1] + 2.0 * reach,
            })
        }
        // Hyperlink glyphs need authoritative ink bounds before binning.
        Command::DrawHyperlink { .. } => None,
        _ => jag_draw::command_bounds(command),
    }
}
