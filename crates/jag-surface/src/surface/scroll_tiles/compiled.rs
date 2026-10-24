//! Compile a paint commit once. Presentation visits properties and visible
//! tile bins; it does not walk the committed document's drawing commands.
use super::*;

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
        result.split_interleaved_runs(scale);
        Ok(result)
    }

    /// A run's tiles composite at its lowest z, so content above another
    /// layer's z must not share them: a fixed overlay emitted after the whole
    /// document would otherwise cover later-painted content baked into the
    /// document's first tiles. Split each run where it crosses the composite
    /// z of any other op, until no run straddles one.
    fn split_interleaved_runs(&mut self, scale: f32) {
        loop {
            let thresholds: std::collections::BTreeSet<i32> = self
                .ops
                .iter()
                .filter_map(|op| match op {
                    Op::Run { data, .. } => data.z,
                    Op::Draw { command, .. } => command.z_index(),
                    Op::ClipPush(_) | Op::ClipPop => None,
                })
                .collect();
            let mut changed = false;
            let mut ops = Vec::with_capacity(self.ops.len());
            for op in std::mem::take(&mut self.ops) {
                let Op::Run {
                    owner,
                    segment,
                    clip,
                    data,
                } = op
                else {
                    ops.push(op);
                    continue;
                };
                let bands = split_at_thresholds(&data.commands, &thresholds);
                if bands.len() < 2 {
                    ops.push(Op::Run {
                        owner,
                        segment,
                        clip,
                        data,
                    });
                    continue;
                }
                changed = true;
                for band in bands {
                    ops.push(Op::Run {
                        owner,
                        segment,
                        clip,
                        data: RasterRun::new(
                            band,
                            scale,
                            self.source.dpi_scale,
                            self.source.provider.as_ref(),
                        ),
                    });
                }
            }
            self.ops = ops;
            if !changed {
                break;
            }
        }
        // Tile identities are keyed by (owner, segment): keep them unique.
        let mut next = vec![0usize; self.properties.len()];
        for op in &mut self.ops {
            if let Op::Run { owner, segment, .. } = op {
                *segment = next[*owner];
                next[*owner] += 1;
            }
        }
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

/// Split a run (z non-decreasing, since runs break where z falls) before the
/// first command whose z reaches a threshold above the current band's start.
/// Transforms open at a split close in the earlier band and reopen in the
/// next; each `PushTransform` carries its composed world transform.
pub(super) fn split_at_thresholds(
    commands: &[Command],
    thresholds: &std::collections::BTreeSet<i32>,
) -> Vec<Vec<Command>> {
    let mut bands = vec![Vec::new()];
    let mut open: Vec<Command> = Vec::new();
    let mut band_start: Option<i32> = None;
    for command in commands {
        if let Some(z) = command.z_index() {
            match band_start {
                None => band_start = Some(z),
                Some(start) if z > start && thresholds.range(start + 1..=z).next().is_some() => {
                    let band = bands.last_mut().unwrap();
                    band.extend(open.iter().map(|_| Command::PopTransform));
                    bands.push(open.clone());
                    band_start = Some(z);
                }
                Some(_) => {}
            }
        }
        match command {
            Command::PushTransform(_) => open.push(command.clone()),
            Command::PopTransform => {
                open.pop();
            }
            _ => {}
        }
        bands.last_mut().unwrap().push(command.clone());
    }
    bands
}

#[cfg(test)]
#[path = "compiled_tests.rs"]
mod tests;
