//! Immutable paint commits and the input properties that can move them.
use crate::{Command, Transform2D};
use std::{collections::HashMap, sync::Arc};

pub type ScrollInputs = HashMap<String, [f32; 2]>;

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollBinding {
    pub key: String,
    pub value: [f32; 2],
    pub factor: [f32; 2],
    pub basis: [f32; 4],
}

#[derive(Clone)]
pub struct ScrollScene {
    pub id: u64,
    pub commands: Arc<[Command]>,
    pub base: Transform2D,
    pub dpi_scale: f32,
    pub provider: Option<Arc<dyn crate::TextProvider + Send + Sync>>,
}

impl std::fmt::Debug for ScrollScene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollScene")
            .field("id", &self.id)
            .field("commands", &self.commands.len())
            .finish()
    }
}
impl PartialEq for ScrollScene {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl ScrollScene {
    pub fn new(
        commands: Arc<[Command]>,
        base: Transform2D,
        dpi_scale: f32,
        provider: Option<Arc<dyn crate::TextProvider + Send + Sync>>,
    ) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            commands,
            base,
            dpi_scale,
            provider,
        }
    }
    /// Rebase an immutable paint commit onto live scroll properties. All
    /// author transforms remain intact; only compositor translations change.
    pub fn resolve(&self, base: Transform2D, inputs: &ScrollInputs) -> Vec<Command> {
        let root_delta = [base.m[4] - self.base.m[4], base.m[5] - self.base.m[5]];
        let mut deltas = vec![root_delta];
        let mut transforms = vec![self.base];
        let mut out = Vec::with_capacity(self.commands.len());
        for original in self.commands.iter() {
            let mut delta = *deltas.last().unwrap();
            match original {
                Command::PushScrollLayer {
                    key,
                    origin,
                    binding,
                } => {
                    let value = inputs.get(&binding.key).copied().unwrap_or(binding.value);
                    let dx = (value[0] - binding.value[0]) * binding.factor[0];
                    let dy = (value[1] - binding.value[1]) * binding.factor[1];
                    let [a, b, c, d] = binding.basis;
                    delta[0] += a * dx + c * dy;
                    delta[1] += b * dx + d * dy;
                    deltas.push(delta);
                    let mut transform = *transforms.last().unwrap();
                    transform.m[4] += delta[0];
                    transform.m[5] += delta[1];
                    out.push(Command::PushTransform(transform));
                    out.push(Command::PushScrollLayer {
                        key: key.clone(),
                        origin: [origin[0] + delta[0], origin[1] + delta[1]],
                        binding: ScrollBinding {
                            value,
                            ..binding.clone()
                        },
                    });
                }
                Command::PopScrollLayer => {
                    out.push(Command::PopScrollLayer);
                    out.push(Command::PopTransform);
                    deltas.pop();
                }
                Command::PushTransform(t) => {
                    transforms.push(*t);
                    let mut t = *t;
                    t.m[4] += delta[0];
                    t.m[5] += delta[1];
                    out.push(Command::PushTransform(t));
                }
                Command::PopTransform => {
                    transforms.pop();
                    out.push(original.clone());
                }
                _ => {
                    let mut command = original.clone();
                    translate_command(&mut command, delta);
                    out.push(command);
                }
            }
        }
        out
    }
}

pub fn translate_command(command: &mut Command, delta: [f32; 2]) {
    let transform = match command {
        Command::DrawRect { transform, .. }
        | Command::DrawRoundedRect { transform, .. }
        | Command::StrokeRect { transform, .. }
        | Command::StrokeRoundedRect { transform, .. }
        | Command::DrawText { transform, .. }
        | Command::DrawHyperlink { transform, .. }
        | Command::DrawEllipse { transform, .. }
        | Command::FillPath { transform, .. }
        | Command::StrokePath { transform, .. }
        | Command::BoxShadow { transform, .. }
        | Command::DrawSvg { transform, .. }
        | Command::DrawImage { transform, .. }
        | Command::DrawExternalTexture { transform, .. }
        | Command::HitRegionRect { transform, .. }
        | Command::HitRegionRoundedRect { transform, .. }
        | Command::HitRegionEllipse { transform, .. }
        | Command::PushTransform(transform) => transform,
        Command::BackdropFilter(draw) => &mut draw.transform,
        _ => return,
    };
    transform.m[4] += delta[0];
    transform.m[5] += delta[1];
}
