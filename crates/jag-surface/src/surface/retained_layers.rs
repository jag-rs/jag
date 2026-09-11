//! Bounded retention of isolated surfaces. Keys describe pixels in layer-local
//! coordinates; placement and group opacity remain composition properties.
use std::{collections::HashMap, sync::Arc};

use jag_draw::{Command, ExternalTextureId, SurfaceEffect, TextProvider, wgpu};

#[derive(Clone, Copy, Debug, Default)]
pub struct RetainedLayerStats {
    pub hits: u64,
    pub misses: u64,
    pub bypassed: u64,
    pub rasterized_pixels: u64,
    pub reused_pixels: u64,
    pub resident_bytes: u64,
    pub evictions: u64,
}

pub(super) struct LayerKey {
    commands: Vec<Command>,
    effect: SurfaceEffect,
    pixels: [u32; 2],
    scale: f32,
    provider: Option<Arc<dyn TextProvider + Send + Sync>>,
    font_tag: u64,
    children: Vec<(ExternalTextureId, u64)>,
}

impl PartialEq for LayerKey {
    fn eq(&self, other: &Self) -> bool {
        self.pixels == other.pixels
            && self.scale == other.scale
            && self.font_tag == other.font_tag
            && self.effect == other.effect
            && self.children == other.children
            && match (&self.provider, &other.provider) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && self.commands == other.commands
    }
}

struct Entry {
    key: LayerKey,
    view: Arc<wgpu::TextureView>,
    bytes: u64,
    revision: u64,
    used: u64,
}

pub(super) struct RetainedLayers {
    entries: HashMap<ExternalTextureId, Entry>,
    current: HashMap<ExternalTextureId, u64>,
    budget: u64,
    resident: u64,
    clock: u64,
    revision: u64,
    stats: RetainedLayerStats,
}

impl Default for RetainedLayers {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            current: HashMap::new(),
            budget: 32 * 1024 * 1024,
            resident: 0,
            clock: 0,
            revision: 0,
            stats: Default::default(),
        }
    }
}

impl RetainedLayers {
    pub fn begin_frame(&mut self) {
        self.current.clear();
        self.stats = Default::default();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current.clear();
        self.resident = 0;
    }

    pub fn set_budget(&mut self, bytes: u64) {
        self.budget = bytes;
        self.clear();
    }

    pub fn stats(&self) -> RetainedLayerStats {
        RetainedLayerStats {
            resident_bytes: self.resident,
            ..self.stats
        }
    }

    /// Clips have already been localized to the render target. Keep fractional
    /// device-pixel phase in the key, so reuse never blurs text or SVG edges.
    pub fn key(
        &self,
        commands: &[Command],
        effect: &SurfaceEffect,
        origin: [f32; 2],
        pixels: [u32; 2],
        scale: f32,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
    ) -> Option<LayerKey> {
        if self.budget == 0 || commands.len() > 4096 {
            return None;
        }
        let effect = match effect {
            SurfaceEffect::Opacity(_) => SurfaceEffect::Opacity(1.0),
            SurfaceEffect::Blur(_)
            | SurfaceEffect::ColorMatrix(_)
            | SurfaceEffect::DropShadow(_) => effect.clone(),
            // Texture handles alone do not describe changing mask pixels.
            SurfaceEffect::Mask(_) | SurfaceEffect::MaskGroup(_) => return None,
        };
        let mut commands = commands.to_vec();
        let mut children = Vec::new();
        let mut has_text = false;
        for command in &mut commands {
            let transform = match command {
                Command::DrawText {
                    transform,
                    dynamic: false,
                    ..
                }
                | Command::DrawHyperlink { transform, .. } => {
                    has_text = true;
                    transform
                }
                Command::DrawRect { transform, .. }
                | Command::DrawRoundedRect { transform, .. }
                | Command::StrokeRect { transform, .. }
                | Command::StrokeRoundedRect { transform, .. }
                | Command::DrawEllipse { transform, .. }
                | Command::FillPath { transform, .. }
                | Command::StrokePath { transform, .. }
                | Command::BoxShadow { transform, .. }
                | Command::HitRegionRect { transform, .. }
                | Command::HitRegionRoundedRect { transform, .. }
                | Command::HitRegionEllipse { transform, .. }
                | Command::PushTransform(transform) => transform,
                Command::DrawExternalTexture {
                    texture_id,
                    transform,
                    rect,
                    ..
                } => {
                    // Only a child produced by this compositor in this frame
                    // has a trustworthy revision. Video/canvas textures bypass.
                    children.push((*texture_id, *self.current.get(texture_id)?));
                    if transform.m[..4] == [1.0, 0.0, 0.0, 1.0] {
                        rect.x += transform.m[4] - origin[0];
                        rect.y += transform.m[5] - origin[1];
                        transform.m[4] = 0.0;
                        transform.m[5] = 0.0;
                        continue;
                    }
                    transform
                }
                Command::PushClip(_) | Command::PopClip | Command::PopTransform => continue,
                // File-backed images/SVGs need decode/asset revisions before
                // they can be retained. Never freeze async loads or HMR assets.
                _ => return None,
            };
            transform.m[4] -= origin[0];
            transform.m[5] -= origin[1];
        }
        let provider = if has_text { provider.cloned() } else { None };
        let font_tag = provider.as_ref().map_or(0, |p| p.cache_tag());
        Some(LayerKey {
            commands,
            effect,
            pixels,
            scale,
            provider,
            font_tag,
            children,
        })
    }

    pub fn lookup(
        &mut self,
        id: ExternalTextureId,
        key: &LayerKey,
    ) -> Option<Arc<wgpu::TextureView>> {
        let entry = self.entries.get_mut(&id)?;
        if entry.key != *key {
            return None;
        }
        self.clock += 1;
        entry.used = self.clock;
        self.current.insert(id, entry.revision);
        self.stats.hits += 1;
        self.stats.reused_pixels += u64::from(key.pixels[0]) * u64::from(key.pixels[1]);
        Some(entry.view.clone())
    }

    pub fn rasterized(&mut self, eligible: bool, pixels: [u32; 2]) {
        if eligible {
            self.stats.misses += 1;
        } else {
            self.stats.bypassed += 1;
        }
        self.stats.rasterized_pixels += u64::from(pixels[0]) * u64::from(pixels[1]);
    }

    pub fn insert(
        &mut self,
        id: ExternalTextureId,
        key: LayerKey,
        view: Arc<wgpu::TextureView>,
        bytes: u64,
    ) {
        if let Some(old) = self.entries.remove(&id) {
            self.resident -= old.bytes;
        }
        if bytes > self.budget {
            return;
        }
        while self.resident + bytes > self.budget || self.entries.len() >= 128 {
            let Some(old_id) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(id, _)| *id)
            else {
                break;
            };
            let old = self.entries.remove(&old_id).unwrap();
            self.resident -= old.bytes;
            self.stats.evictions += 1;
        }
        self.clock += 1;
        self.revision += 1;
        self.current.insert(id, self.revision);
        self.resident += bytes;
        self.entries.insert(
            id,
            Entry {
                key,
                view,
                bytes,
                revision: self.revision,
                used: self.clock,
            },
        );
    }
}
