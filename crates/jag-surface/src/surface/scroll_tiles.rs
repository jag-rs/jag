//! Content-space scroll rasterization. Scroll/clip/effect boundaries partition
//! the display list; child scrollers are never painted into a parent's tiles.
use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, ensure};
use jag_draw::{Command, ExternalTextureId, Rect, TextProvider, Transform2D, Viewport};

use super::{
    JagSurface,
    opacity::{LayerGeometry, transformed_rect_bounds},
};

const TILE: i32 = 512;
const GUTTER: i32 = 1;
mod compiled;
use compiled::RasterRun;

#[derive(Default)]
pub(super) struct ScrollTiles {
    compiled: HashMap<(u64, u64, u32), Arc<compiled::CompiledScene>>,
    ids: HashMap<(String, usize, i32, i32), (ExternalTextureId, u64)>,
    frame: u64,
    next: u64,
}

impl ScrollTiles {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        // Texture residency is independently bounded by RetainedLayers. Drop
        // identities for departed documents and distant, long-unseen tiles.
        self.ids
            .retain(|_, (_, seen)| self.frame.saturating_sub(*seen) < 120);
    }

    fn id(&mut self, owner: &str, segment: usize, x: i32, y: i32) -> ExternalTextureId {
        let entry = self
            .ids
            .entry((owner.to_owned(), segment, x, y))
            .or_insert_with(|| {
                self.next += 1;
                (
                    ExternalTextureId(0x7100_0000_0000_0000 + self.next),
                    self.frame,
                )
            });
        entry.1 = self.frame;
        entry.0
    }
}

struct Owner {
    key: String,
    origin: [f32; 2],
    segment: usize,
}

#[derive(Clone, Copy)]
struct Clip {
    rect: Rect,
    authored: Rect,
    rounded: Option<jag_draw::RoundedRectClipGpu>,
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rect {
        x,
        y,
        w: ((a.x + a.w).min(b.x + b.w) - x).max(0.0),
        h: ((a.y + a.h).min(b.y + b.h) - y).max(0.0),
    }
}

fn translated(rect: Rect, offset: [f32; 2]) -> Rect {
    Rect {
        x: rect.x + offset[0],
        y: rect.y + offset[1],
        ..rect
    }
}

/// Normalize only placement. Local path coordinates, CSS transforms and font
/// sizes remain untouched, preserving the document's authoritative geometry.
fn normalize(command: &mut Command, origin: [f32; 2]) {
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
        | Command::DrawExternalTexture { transform, .. } => transform,
        _ => return,
    };
    transform.m[4] -= origin[0];
    transform.m[5] -= origin[1];
}

impl JagSurface {
    pub(super) fn flatten_scroll_layers(
        &mut self,
        commands: &[Command],
        viewport: Viewport,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
    ) -> Result<Vec<Command>> {
        if !commands.iter().any(|c| {
            matches!(
                c,
                Command::PushScrollLayer { .. } | Command::DrawScrollScene { .. }
            )
        }) {
            return Ok(commands.to_vec());
        }
        let scale =
            jag_draw::logical_multiplier(self.logical_pixels, self.dpi_scale, self.ui_scale);
        let viewport_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: viewport.width as f32 / scale,
            h: viewport.height as f32 / scale,
        };
        let mut clips = vec![Clip {
            rect: viewport_rect,
            authored: viewport_rect,
            rounded: None,
        }];
        let mut transforms = vec![Transform2D::identity()];
        let mut owners: Vec<Owner> = Vec::new();
        let mut run = Vec::new();
        let mut out = Vec::new();
        for command in commands {
            // Preserve hit metadata without allowing its z to fragment paint.
            if matches!(
                command,
                Command::HitRegionRect { .. }
                    | Command::HitRegionRoundedRect { .. }
                    | Command::HitRegionEllipse { .. }
            ) {
                out.push(command.clone());
                continue;
            }
            let boundary = matches!(
                command,
                Command::PushScrollLayer { .. }
                    | Command::PopScrollLayer
                    | Command::DrawScrollScene { .. }
                    | Command::PushClip(_)
                    | Command::PopClip
                    | Command::ScrollClipRadii(_)
                    | Command::PushOpacity(_)
                    | Command::PopOpacity
                    | Command::PushFilter(_)
                    | Command::PopFilter
                    | Command::BackdropFilter(_)
                    | Command::DrawExternalTexture { .. }
            );
            let reversed_z = command
                .z_index()
                .zip(run.last().and_then(Command::z_index))
                .is_some_and(|(z, last)| z < last);
            if boundary || reversed_z {
                if let Some(owner) = owners.last_mut() {
                    self.raster_scroll_run(
                        &mut run,
                        owner,
                        *clips.last().unwrap(),
                        scale,
                        provider,
                        &mut out,
                    )?;
                }
            }
            match command {
                Command::DrawScrollScene {
                    scene,
                    base,
                    inputs,
                } => self.compose_committed_scene(
                    scene,
                    *base,
                    inputs,
                    *clips.last().unwrap(),
                    scale,
                    &mut out,
                )?,
                Command::PushScrollLayer { key, origin, .. } => {
                    let key = owners.last().map_or_else(
                        || key.clone(),
                        |parent| format!("{}\u{1f}{key}", parent.key),
                    );
                    owners.push(Owner {
                        key,
                        origin: *origin,
                        segment: 0,
                    });
                }
                Command::PopScrollLayer => {
                    ensure!(owners.pop().is_some(), "unbalanced scroll owner");
                }
                Command::PushTransform(t) => {
                    transforms.push(*t);
                    out.push(command.clone());
                }
                Command::PopTransform => {
                    ensure!(transforms.len() > 1, "unbalanced transform");
                    transforms.pop();
                    out.push(command.clone());
                }
                Command::PushClip(clip) => {
                    let parent = *clips.last().unwrap();
                    let rect = transformed_rect_bounds(clip.0, *transforms.last().unwrap());
                    clips.push(Clip {
                        rect: intersect(parent.rect, rect),
                        authored: rect,
                        rounded: parent.rounded,
                    });
                    out.push(command.clone());
                }
                Command::ScrollClipRadii(radii) => {
                    let clip = clips.last_mut().unwrap();
                    clip.rounded = Some(jag_draw::RoundedRectClipGpu {
                        rect: [
                            clip.authored.x * scale,
                            clip.authored.y * scale,
                            clip.authored.w * scale,
                            clip.authored.h * scale,
                        ],
                        radii: *radii,
                    });
                }
                Command::PopClip => {
                    ensure!(clips.len() > 1, "unbalanced clip");
                    clips.pop();
                    out.push(command.clone());
                }
                Command::PushOpacity(_)
                | Command::PopOpacity
                | Command::PushFilter(_)
                | Command::PopFilter
                | Command::BackdropFilter(_)
                | Command::DrawExternalTexture { .. }
                | Command::HitRegionRect { .. }
                | Command::HitRegionRoundedRect { .. }
                | Command::HitRegionEllipse { .. } => out.push(command.clone()),
                _ if owners.is_empty() => out.push(command.clone()),
                _ => run.push(command.clone()),
            }
        }
        ensure!(owners.is_empty(), "unclosed scroll owner");
        Ok(out)
    }

    fn raster_scroll_run(
        &mut self,
        run: &mut Vec<Command>,
        owner: &mut Owner,
        clip: Clip,
        scale: f32,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
        out: &mut Vec<Command>,
    ) -> Result<()> {
        if run.is_empty() {
            return Ok(());
        }
        let segment = owner.segment;
        owner.segment += 1;
        let mut commands = std::mem::take(run);
        for command in &mut commands {
            normalize(command, owner.origin);
        }
        let data = RasterRun::new(commands, scale, self.dpi_scale, provider);
        self.composite_scroll_run(&data, owner, segment, clip, scale, provider, out)
    }

    fn composite_scroll_run(
        &mut self,
        data: &RasterRun,
        owner: &Owner,
        segment: usize,
        clip: Clip,
        scale: f32,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
        out: &mut Vec<Command>,
    ) -> Result<()> {
        let Some(z) = data.commands.iter().filter_map(Command::z_index).min() else {
            return Ok(());
        };
        if clip.rect.w <= 0.0 || clip.rect.h <= 0.0 {
            return Ok(());
        }
        let visible = translated(clip.rect, [-owner.origin[0], -owner.origin[1]]);
        let tight_bounds = data.tight;
        let x0 = (visible.x * scale / TILE as f32).floor() as i32;
        let y0 = (visible.y * scale / TILE as f32).floor() as i32;
        let x1 = ((visible.x + visible.w) * scale / TILE as f32).ceil() as i32;
        let y1 = ((visible.y + visible.h) * scale / TILE as f32).ceil() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let grid_tile = Rect {
                    x: (x as f32 * TILE as f32 - GUTTER as f32) / scale,
                    y: (y as f32 * TILE as f32 - GUTTER as f32) / scale,
                    w: (TILE + 2 * GUTTER) as f32 / scale,
                    h: (TILE + 2 * GUTTER) as f32 / scale,
                };
                let tile = tight_bounds.map_or(grid_tile, |ink| {
                    let ink = Rect {
                        x: ((ink.x * scale).floor() - 2.0) / scale,
                        y: ((ink.y * scale).floor() - 2.0) / scale,
                        w: ((ink.w * scale).ceil() + 5.0) / scale,
                        h: ((ink.h * scale).ceil() + 5.0) / scale,
                    };
                    intersect(grid_tile, ink)
                });
                if tile.w <= 0.0 || tile.h <= 0.0 {
                    continue;
                }
                let tile_commands = data.tile_commands(x, y, tile);
                if tile_commands.is_empty() {
                    continue;
                }
                let id = self.scroll_tiles.id(&owner.key, segment, x, y);
                self.render_effect_group_layer(
                    LayerGeometry {
                        origin: [tile.x, tile.y],
                        logical_size: [tile.w, tile.h],
                        pixel_size: [
                            (tile.w * scale).round() as u32,
                            (tile.h * scale).round() as u32,
                        ],
                    },
                    tile_commands,
                    jag_draw::SurfaceEffect::Opacity(1.0),
                    provider,
                    scale,
                    Some(id),
                )?;
                let world = translated(tile, owner.origin);
                let inner = translated(
                    Rect {
                        x: grid_tile.x + GUTTER as f32 / scale,
                        y: grid_tile.y + GUTTER as f32 / scale,
                        w: TILE as f32 / scale,
                        h: TILE as f32 / scale,
                    },
                    owner.origin,
                );
                let rect = intersect(intersect(inner, world), clip.rect);
                if rect.w <= 0.0 || rect.h <= 0.0 {
                    continue;
                }
                out.push(Command::DrawCompositeTile {
                    rect,
                    texture_id: id,
                    uv: [
                        (rect.x - world.x) / world.w,
                        (rect.y - world.y) / world.h,
                        (rect.x + rect.w - world.x) / world.w,
                        (rect.y + rect.h - world.y) / world.h,
                    ],
                    rounded_clip: clip.rounded,
                    z,
                });
            }
        }
        Ok(())
    }
}
