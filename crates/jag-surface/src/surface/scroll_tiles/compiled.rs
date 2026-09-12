//! Compile a paint commit once. Presentation visits properties and visible
//! tile bins; it does not walk the committed document's drawing commands.
use super::*;

pub(super) struct RasterRun {
    pub commands: Vec<Command>,
    pub tight: Option<Rect>,
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
        // Their full ink can exceed the display-list's estimated bounds.
        Command::BoxShadow { .. } | Command::DrawHyperlink { .. } => None,
        _ => jag_draw::command_bounds(command),
    }
}

struct Property {
    parent: usize,
    binding: Option<jag_draw::ScrollBinding>,
    key: String,
    origin: [f32; 2],
}
struct ClipProperty {
    parent: usize,
    owner: usize,
    rect: Rect,
    radii: Option<[f32; 4]>,
}
enum Op {
    Run {
        owner: usize,
        segment: usize,
        clip: usize,
        data: RasterRun,
    },
    ClipPush(usize),
    ClipPop,
    Draw {
        command: Command,
        owner: usize,
    },
}

pub(super) struct CompiledScene {
    source: Arc<jag_draw::ScrollScene>,
    properties: Vec<Property>,
    clips: Vec<ClipProperty>,
    ops: Vec<Op>,
}

impl CompiledScene {
    fn new(source: Arc<jag_draw::ScrollScene>, scale: f32) -> Result<Self> {
        let mut result = Self {
            source: source.clone(),
            properties: vec![Property {
                parent: 0,
                binding: None,
                key: String::new(),
                origin: [source.base.m[4], source.base.m[5]],
            }],
            clips: Vec::new(),
            ops: Vec::new(),
        };
        let mut owners = vec![0];
        let mut clips = vec![0]; // 0 is the live viewport/embedding clip
        let mut transforms = vec![source.base];
        let mut segments = vec![0];
        let mut run = Vec::new();
        for command in source.commands.iter() {
            // HitIndex reads the immutable source before composition. These
            // markers have no pixels and must not split a raster run per DOM
            // element (or alter the paint-order comparison below).
            if matches!(
                command,
                Command::HitRegionRect { .. }
                    | Command::HitRegionRoundedRect { .. }
                    | Command::HitRegionEllipse { .. }
            ) {
                continue;
            }
            let owner = *owners.last().unwrap();
            let clip = *clips.last().unwrap();
            let boundary = command.z_index().is_none()
                && !matches!(command, Command::PushTransform(_) | Command::PopTransform)
                || matches!(
                    command,
                    Command::BackdropFilter(_) | Command::DrawExternalTexture { .. }
                )
                || command
                    .z_index()
                    .zip(run.last().and_then(Command::z_index))
                    .is_some_and(|(z, last)| z < last);
            if boundary {
                result.flush(&mut run, owner, clip, &mut segments, scale);
            }
            match command {
                Command::PushScrollLayer {
                    key,
                    origin,
                    binding,
                } => {
                    let parent_key = &result.properties[owner].key;
                    let key = if parent_key.is_empty() {
                        key.clone()
                    } else {
                        format!("{parent_key}\u{1f}{key}")
                    };
                    owners.push(result.properties.len());
                    segments.push(0);
                    result.properties.push(Property {
                        parent: owner,
                        binding: Some(binding.clone()),
                        key,
                        origin: *origin,
                    });
                }
                Command::PopScrollLayer => {
                    ensure!(owners.len() > 1, "unbalanced committed owner");
                    owners.pop();
                }
                Command::PushTransform(t) => transforms.push(*t),
                Command::PopTransform => {
                    ensure!(transforms.len() > 1, "unbalanced committed transform");
                    transforms.pop();
                }
                Command::PushClip(rect) => {
                    let index = result.clips.len() + 1;
                    result.clips.push(ClipProperty {
                        parent: clip,
                        owner,
                        rect: transformed_rect_bounds(rect.0, *transforms.last().unwrap()),
                        radii: None,
                    });
                    clips.push(index);
                    result.ops.push(Op::ClipPush(index));
                }
                Command::ScrollClipRadii(radii) => {
                    if clip > 0 {
                        result.clips[clip - 1].radii = Some(*radii);
                    }
                }
                Command::PopClip => {
                    ensure!(clips.len() > 1, "unbalanced committed clip");
                    clips.pop();
                    result.ops.push(Op::ClipPop);
                }
                Command::PushOpacity(_)
                | Command::PopOpacity
                | Command::PushFilter(_)
                | Command::PopFilter
                | Command::BackdropFilter(_)
                | Command::DrawExternalTexture { .. } => result.ops.push(Op::Draw {
                    command: command.clone(),
                    owner,
                }),
                Command::DrawScrollScene { .. } => {
                    anyhow::bail!("nested scroll commits must be submitted separately")
                }
                _ => run.push(command.clone()),
            }
        }
        result.flush(
            &mut run,
            *owners.last().unwrap(),
            *clips.last().unwrap(),
            &mut segments,
            scale,
        );
        ensure!(
            owners.len() == 1 && clips.len() == 1 && transforms.len() == 1,
            "unclosed committed paint scope"
        );
        Ok(result)
    }

    fn flush(
        &mut self,
        run: &mut Vec<Command>,
        owner: usize,
        clip: usize,
        segments: &mut [usize],
        scale: f32,
    ) {
        if run.is_empty() {
            return;
        }
        let mut commands = std::mem::take(run);
        for command in &mut commands {
            normalize(command, self.properties[owner].origin);
        }
        self.ops.push(Op::Run {
            owner,
            segment: segments[owner],
            clip,
            data: RasterRun::new(
                commands,
                scale,
                self.source.dpi_scale,
                self.source.provider.as_ref(),
            ),
        });
        segments[owner] += 1;
    }
}

impl JagSurface {
    pub(super) fn compose_committed_scene(
        &mut self,
        source: &Arc<jag_draw::ScrollScene>,
        base: Transform2D,
        inputs: &jag_draw::ScrollInputs,
        clip: Clip,
        scale: f32,
        out: &mut Vec<Command>,
    ) -> Result<()> {
        let stamp = (
            source.id,
            source.provider.as_ref().map_or(0, |p| p.cache_tag()),
            scale.to_bits(),
        );
        let compiled = if let Some(value) = self.scroll_tiles.compiled.get(&stamp) {
            value.clone()
        } else {
            let value = Arc::new(CompiledScene::new(source.clone(), scale)?);
            if self.scroll_tiles.compiled.len() >= 4 {
                self.scroll_tiles.compiled.clear();
            }
            self.scroll_tiles.compiled.insert(stamp, value.clone());
            value
        };
        let mut deltas = vec![[base.m[4] - source.base.m[4], base.m[5] - source.base.m[5]]];
        for property in compiled.properties.iter().skip(1) {
            let mut delta = deltas[property.parent];
            if let Some(binding) = &property.binding {
                let value = inputs.get(&binding.key).copied().unwrap_or(binding.value);
                let x = (value[0] - binding.value[0]) * binding.factor[0];
                let y = (value[1] - binding.value[1]) * binding.factor[1];
                let [a, b, c, d] = binding.basis;
                delta[0] += a * x + c * y;
                delta[1] += b * x + d * y;
            }
            deltas.push(delta);
        }
        let mut clips = vec![clip];
        for property in &compiled.clips {
            let parent = clips[property.parent];
            let rect = translated(property.rect, deltas[property.owner]);
            let rounded = property
                .radii
                .map(|radii| jag_draw::RoundedRectClipGpu {
                    rect: [
                        rect.x * scale,
                        rect.y * scale,
                        rect.w * scale,
                        rect.h * scale,
                    ],
                    radii,
                })
                .or(parent.rounded);
            clips.push(Clip {
                rect: intersect(parent.rect, rect),
                authored: rect,
                rounded,
            });
        }
        for op in &compiled.ops {
            match op {
                Op::Run {
                    owner,
                    segment,
                    clip,
                    data,
                } => {
                    let property = &compiled.properties[*owner];
                    let delta = deltas[*owner];
                    let owner = Owner {
                        key: property.key.clone(),
                        segment: *segment,
                        origin: [property.origin[0] + delta[0], property.origin[1] + delta[1]],
                    };
                    self.composite_scroll_run(
                        data,
                        &owner,
                        *segment,
                        clips[*clip],
                        scale,
                        source.provider.as_ref(),
                        out,
                    )?;
                }
                Op::ClipPush(index) => {
                    out.push(Command::PushTransform(Transform2D::identity()));
                    out.push(Command::PushClip(jag_draw::ClipRect(clips[*index].rect)));
                }
                Op::ClipPop => {
                    out.push(Command::PopClip);
                    out.push(Command::PopTransform);
                }
                Op::Draw { command, owner } => {
                    let mut command = command.clone();
                    jag_draw::translate_command(&mut command, deltas[*owner]);
                    out.push(command);
                }
            }
        }
        Ok(())
    }
}
