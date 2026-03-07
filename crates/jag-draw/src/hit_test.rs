use crate::display_list::{Command, DisplayList};
use crate::scene::*;

/// Result of a hit test for a single topmost element.
#[derive(Clone, Debug)]
pub struct HitResult {
    /// Increasing draw-order identifier within the display list.
    pub id: usize,
    /// Z layering value; larger is visually on top.
    pub z: i32,
    /// Kind of primitive that was hit.
    pub kind: HitKind,
    /// Geometry of the hit shape.
    pub shape: HitShape,
    /// The local-to-world transform at draw time.
    pub transform: Transform2D,
    /// If this hit corresponds to a hit region, returns the user-specified region id.
    pub region_id: Option<u32>,
    /// Local point within the hit item's local space (after inverse transform), relative to the shape's origin.
    pub local_pos: Option<[f32; 2]>,
    /// Normalized coordinates within the shape's bounding box when applicable ([0,1] range).
    pub local_uv: Option<[f32; 2]>,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum HitKind {
    Rect,
    RoundedRect,
    Ellipse,
    Text,
    StrokeRect,
    StrokeRoundedRect,
    Path,
    BoxShadow,
    HitRegion,
    Hyperlink,
}

/// Public geometry snapshot of a hit element.
#[derive(Clone, Debug, PartialEq)]
pub enum HitShape {
    Rect(Rect),
    RoundedRect(RoundedRect),
    Ellipse { center: [f32; 2], radii: [f32; 2] },
    StrokeRect { rect: Rect, width: f32 },
    StrokeRoundedRect { rrect: RoundedRect, width: f32 },
    PathBBox { rect: Rect },
    Text,
    BoxShadow { rrect: RoundedRect },
    Hyperlink { rect: Rect, url: String },
}

/// Preprocessed hit-test item built from a display list command.
#[derive(Clone, Debug)]
struct HitItem {
    id: usize,
    z: i32,
    kind: HitKind,
    transform: Transform2D,
    data: HitData,
    clips: Vec<ClipEntry>,
    region_id: Option<u32>,
}

#[derive(Clone, Debug)]
enum HitData {
    Rect(Rect),
    RoundedRect(RoundedRect),
    Ellipse { center: [f32; 2], radii: [f32; 2] },
    StrokeRect { rect: Rect, width: f32 },
    StrokeRoundedRect { rrect: RoundedRect, width: f32 },
    PathBBox(Rect),
    Text(TextRun),
    BoxShadow { rrect: RoundedRect },
    Hyperlink { rect: Rect, url: String },
}

#[derive(Clone, Debug)]
struct ClipEntry {
    rect: Rect,
    transform: Transform2D,
}

/// Spatial index for hit testing. Currently implemented as a flat list while
/// preserving z-order and clip stacks. Can be upgraded to a tree later.
#[derive(Default, Clone)]
pub struct HitIndex {
    items: Vec<HitItem>,
}

impl HitIndex {
    /// Build a hit-test index from the display list. Records each drawable
    /// element with its transform and the active clip stack at that point.
    pub fn build(list: &DisplayList) -> Self {
        let mut items = Vec::new();
        let mut clips: Vec<ClipEntry> = Vec::new();
        let mut tstack: Vec<Transform2D> = vec![Transform2D::identity()];
        let mut next_id: usize = 0;

        for cmd in &list.commands {
            match cmd {
                Command::PushClip(ClipRect(rect)) => {
                    clips.push(ClipEntry {
                        rect: *rect,
                        transform: *tstack.last().unwrap(),
                    });
                }
                Command::PopClip => {
                    let _ = clips.pop();
                }
                Command::PushTransform(t) => {
                    tstack.push(*t);
                }
                Command::PopTransform => {
                    let _ = tstack.pop();
                }
                Command::DrawRect {
                    rect, z, transform, ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::Rect,
                        transform: *transform,
                        data: HitData::Rect(*rect),
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::DrawRoundedRect {
                    rrect,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::RoundedRect,
                        transform: *transform,
                        data: HitData::RoundedRect(*rrect),
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::DrawEllipse {
                    center,
                    radii,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::Ellipse,
                        transform: *transform,
                        data: HitData::Ellipse {
                            center: *center,
                            radii: *radii,
                        },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::StrokeRect {
                    rect,
                    stroke,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::StrokeRect,
                        transform: *transform,
                        data: HitData::StrokeRect {
                            rect: *rect,
                            width: stroke.width,
                        },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::StrokeRoundedRect {
                    rrect,
                    stroke,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::StrokeRoundedRect,
                        transform: *transform,
                        data: HitData::StrokeRoundedRect {
                            rrect: *rrect,
                            width: stroke.width,
                        },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::DrawText {
                    run, z, transform, ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::Text,
                        transform: *transform,
                        data: HitData::Text(run.clone()),
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::DrawHyperlink {
                    hyperlink,
                    z,
                    transform,
                    ..
                } => {
                    // Keep hyperlinks above generic per-node hit regions (commonly z + 10)
                    // so inline links remain hoverable/clickable inside text containers.
                    let hit_z = z.saturating_add(11);
                    let text_width = hyperlink.measured_width.unwrap_or_else(|| {
                        // Fallback heuristic (slightly generous). Include a small
                        // boost for heavier font weights.
                        let weight_boost = ((hyperlink.weight - 400.0).max(0.0) / 500.0) * 0.08;
                        let char_width = hyperlink.size * (0.55 + weight_boost);
                        hyperlink.text.chars().count() as f32 * char_width
                    });
                    let text_height = hyperlink.size * 1.2; // Include descenders

                    let rect = Rect {
                        x: hyperlink.pos[0],
                        y: hyperlink.pos[1] - hyperlink.size, // Baseline to top
                        w: text_width,
                        h: text_height,
                    };

                    items.push(HitItem {
                        id: next_id,
                        z: hit_z,
                        kind: HitKind::Hyperlink,
                        transform: *transform,
                        data: HitData::Hyperlink {
                            rect,
                            url: hyperlink.url.clone(),
                        },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::FillPath { .. } => {
                    // Coarse bbox hit: compute path bounding box (approximate using control/end points).
                    if let Command::FillPath {
                        path, z, transform, ..
                    } = cmd
                    {
                        if let Some(rect) = bbox_for_path(path) {
                            items.push(HitItem {
                                id: next_id,
                                z: *z,
                                kind: HitKind::Path,
                                transform: *transform,
                                data: HitData::PathBBox(rect),
                                clips: clips.clone(),
                                region_id: None,
                            });
                            next_id += 1;
                        }
                    }
                }
                Command::StrokePath {
                    path, z, transform, ..
                } => {
                    if let Some(mut rect) = bbox_for_path(path) {
                        // Expand bbox by half of stroke width conservatively
                        if let Command::StrokePath { stroke, .. } = cmd {
                            let w = stroke.width.max(0.0) * 0.5;
                            rect.x -= w;
                            rect.y -= w;
                            rect.w += w * 2.0;
                            rect.h += w * 2.0;
                        }
                        items.push(HitItem {
                            id: next_id,
                            z: *z,
                            kind: HitKind::Path,
                            transform: *transform,
                            data: HitData::PathBBox(rect),
                            clips: clips.clone(),
                            region_id: None,
                        });
                        next_id += 1;
                    }
                }
                Command::BoxShadow {
                    rrect,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::BoxShadow,
                        transform: *transform,
                        data: HitData::BoxShadow { rrect: *rrect },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::HitRegionRect {
                    id,
                    rect,
                    z,
                    transform,
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::HitRegion,
                        transform: *transform,
                        data: HitData::Rect(*rect),
                        clips: clips.clone(),
                        region_id: Some(*id),
                    });
                    next_id += 1;
                }
                Command::HitRegionRoundedRect {
                    id,
                    rrect,
                    z,
                    transform,
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::HitRegion,
                        transform: *transform,
                        data: HitData::RoundedRect(*rrect),
                        clips: clips.clone(),
                        region_id: Some(*id),
                    });
                    next_id += 1;
                }
                Command::HitRegionEllipse {
                    id,
                    center,
                    radii,
                    z,
                    transform,
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::HitRegion,
                        transform: *transform,
                        data: HitData::Ellipse {
                            center: *center,
                            radii: *radii,
                        },
                        clips: clips.clone(),
                        region_id: Some(*id),
                    });
                    next_id += 1;
                }
                // Image, SVG, and external texture draws currently do not participate in hit testing.
                Command::DrawImage { .. } => {}
                Command::DrawSvg { .. } => {}
                Command::DrawExternalTexture { .. } => {}
                Command::PushOpacity(_) => {}
                Command::PopOpacity => {}
            }
        }

        // Always include a root viewport hit region to capture scene-surface hits
        // Use minimal z so it doesn't occlude content.
        let root_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: list.viewport.width as f32,
            h: list.viewport.height as f32,
        };
        items.push(HitItem {
            id: next_id,
            z: i32::MIN,
            kind: HitKind::HitRegion,
            transform: Transform2D::identity(),
            data: HitData::Rect(root_rect),
            clips: Vec::new(),
            region_id: Some(u32::MAX),
        });

        Self { items }
    }

    /// Return the topmost element at the given device-space position.
    pub fn topmost_at(&self, pos: [f32; 2]) -> Option<HitResult> {
        let mut best: Option<HitItem> = None;
        for it in &self.items {
            if !passes_clip(it, pos) {
                continue;
            }
            if hit_item_contains(it, pos) {
                best = match best {
                    None => Some(it.clone()),
                    Some(ref cur) => {
                        if it.z > cur.z || (it.z == cur.z && it.id > cur.id) {
                            Some(it.clone())
                        } else {
                            Some(cur.clone())
                        }
                    }
                };
            }
        }
        best.map(|it| {
            let (local_pos, local_uv) = compute_locals(&it, pos);
            HitResult {
                id: it.id,
                z: it.z,
                kind: it.kind,
                shape: match &it.data {
                    HitData::Rect(r) => HitShape::Rect(*r),
                    HitData::RoundedRect(rr) => HitShape::RoundedRect(*rr),
                    HitData::Ellipse { center, radii } => HitShape::Ellipse {
                        center: *center,
                        radii: *radii,
                    },
                    HitData::StrokeRect { rect, width } => HitShape::StrokeRect {
                        rect: *rect,
                        width: *width,
                    },
                    HitData::StrokeRoundedRect { rrect, width } => HitShape::StrokeRoundedRect {
                        rrect: *rrect,
                        width: *width,
                    },
                    HitData::PathBBox(r) => HitShape::PathBBox { rect: *r },
                    HitData::Text(_) => HitShape::Text,
                    HitData::BoxShadow { rrect } => HitShape::BoxShadow { rrect: *rrect },
                    HitData::Hyperlink { rect, url } => HitShape::Hyperlink {
                        rect: *rect,
                        url: url.clone(),
                    },
                },
                transform: it.transform,
                region_id: it.region_id,
                local_pos,
                local_uv,
            }
        })
    }
}

fn bbox_for_path(path: &Path) -> Option<Rect> {
    let mut minx = f32::INFINITY;
    let mut miny = f32::INFINITY;
    let mut maxx = f32::NEG_INFINITY;
    let mut maxy = f32::NEG_INFINITY;
    let mut any = false;
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => {
                minx = minx.min(p[0]);
                miny = miny.min(p[1]);
                maxx = maxx.max(p[0]);
                maxy = maxy.max(p[1]);
                any = true;
            }
            PathCmd::QuadTo(c, p) => {
                for q in [c, p] {
                    minx = minx.min(q[0]);
                    miny = miny.min(q[1]);
                    maxx = maxx.max(q[0]);
                    maxy = maxy.max(q[1]);
                }
                any = true;
            }
            PathCmd::CubicTo(c1, c2, p) => {
                for q in [c1, c2, p] {
                    minx = minx.min(q[0]);
                    miny = miny.min(q[1]);
                    maxx = maxx.max(q[0]);
                    maxy = maxy.max(q[1]);
                }
                any = true;
            }
            PathCmd::Close => {}
        }
    }
    if any {
        Some(Rect {
            x: minx,
            y: miny,
            w: (maxx - minx).max(0.0),
            h: (maxy - miny).max(0.0),
        })
    } else {
        None
    }
}

fn passes_clip(item: &HitItem, world: [f32; 2]) -> bool {
    for c in &item.clips {
        if !point_in_rect_local(world, &c.transform, c.rect) {
            return false;
        }
    }
    true
}

fn hit_item_contains(item: &HitItem, world: [f32; 2]) -> bool {
    match &item.data {
        HitData::Rect(r) => point_in_rect_local(world, &item.transform, *r),
        HitData::RoundedRect(r) => point_in_rounded_rect_local(world, &item.transform, *r),
        HitData::Ellipse { center, radii } => {
            point_in_ellipse_local(world, &item.transform, *center, *radii)
        }
        HitData::StrokeRect { rect, width } => {
            point_in_stroke_rect_local(world, &item.transform, *rect, *width)
        }
        HitData::StrokeRoundedRect { rrect, width } => {
            point_in_stroke_rounded_rect_local(world, &item.transform, *rrect, *width)
        }
        HitData::PathBBox(rect) => point_in_rect_local(world, &item.transform, *rect),
        HitData::Text(_run) => false, // Text hit testing not yet implemented
        HitData::BoxShadow { rrect } => point_in_rounded_rect_local(world, &item.transform, *rrect),
        HitData::Hyperlink { rect, .. } => point_in_rect_local(world, &item.transform, *rect),
    }
}

fn point_in_rect_local(world: [f32; 2], transform: &Transform2D, rect: Rect) -> bool {
    if let Some(p) = transform.inverse_apply(world) {
        p[0] >= rect.x && p[1] >= rect.y && p[0] <= rect.x + rect.w && p[1] <= rect.y + rect.h
    } else {
        false
    }
}

fn point_in_rounded_rect_local(
    world: [f32; 2],
    transform: &Transform2D,
    rrect: RoundedRect,
) -> bool {
    if let Some(p) = transform.inverse_apply(world) {
        let Rect { x, y, w, h } = rrect.rect;
        let tl = rrect.radii.tl.min(w * 0.5).min(h * 0.5);
        let tr = rrect.radii.tr.min(w * 0.5).min(h * 0.5);
        let br = rrect.radii.br.min(w * 0.5).min(h * 0.5);
        let bl = rrect.radii.bl.min(w * 0.5).min(h * 0.5);
        let px = p[0] - x;
        let py = p[1] - y;
        if px < 0.0 || py < 0.0 || px > w || py > h {
            return false;
        }
        // Top-left
        if px < tl && py < tl {
            let dx = tl - px;
            let dy = tl - py;
            return dx * dx + dy * dy <= tl * tl + 1e-5;
        }
        // Top-right
        if px > w - tr && py < tr {
            let dx = px - (w - tr);
            let dy = tr - py;
            return dx * dx + dy * dy <= tr * tr + 1e-5;
        }
        // Bottom-right
        if px > w - br && py > h - br {
            let dx = px - (w - br);
            let dy = py - (h - br);
            return dx * dx + dy * dy <= br * br + 1e-5;
        }
        // Bottom-left
        if px < bl && py > h - bl {
            let dx = bl - px;
            let dy = py - (h - bl);
            return dx * dx + dy * dy <= bl * bl + 1e-5;
        }
        true
    } else {
        false
    }
}

fn point_in_ellipse_local(
    world: [f32; 2],
    transform: &Transform2D,
    center: [f32; 2],
    radii: [f32; 2],
) -> bool {
    if let Some(p) = transform.inverse_apply(world) {
        let dx = (p[0] - center[0]) / radii[0].max(1e-6);
        let dy = (p[1] - center[1]) / radii[1].max(1e-6);
        dx * dx + dy * dy <= 1.0 + 1e-5
    } else {
        false
    }
}

fn point_in_stroke_rect_local(
    world: [f32; 2],
    transform: &Transform2D,
    rect: Rect,
    width: f32,
) -> bool {
    if let Some(p) = transform.inverse_apply(world) {
        let outer = Rect {
            x: rect.x - width * 0.5,
            y: rect.y - width * 0.5,
            w: rect.w + width,
            h: rect.h + width,
        };
        let inner = Rect {
            x: rect.x + width * 0.5,
            y: rect.y + width * 0.5,
            w: (rect.w - width).max(0.0),
            h: (rect.h - width).max(0.0),
        };
        let in_outer = p[0] >= outer.x
            && p[1] >= outer.y
            && p[0] <= outer.x + outer.w
            && p[1] <= outer.y + outer.h;
        let in_inner = p[0] >= inner.x
            && p[1] >= inner.y
            && p[0] <= inner.x + inner.w
            && p[1] <= inner.y + inner.h;
        in_outer && !in_inner
    } else {
        false
    }
}

fn point_in_stroke_rounded_rect_local(
    world: [f32; 2],
    transform: &Transform2D,
    rrect: RoundedRect,
    width: f32,
) -> bool {
    // Approximate by testing ring between rrect and inset rrect.
    if let Some(p) = transform.inverse_apply(world) {
        // Outer check
        let outer_hit = point_in_rounded_rect_untransformed(p, rrect);
        if !outer_hit {
            return false;
        }
        // Inner check (inset by stroke width)
        let inset = width.max(0.0) * 0.5;
        let inner = RoundedRect {
            rect: Rect {
                x: rrect.rect.x + inset,
                y: rrect.rect.y + inset,
                w: (rrect.rect.w - width).max(0.0),
                h: (rrect.rect.h - width).max(0.0),
            },
            radii: RoundedRadii {
                tl: (rrect.radii.tl - inset).max(0.0),
                tr: (rrect.radii.tr - inset).max(0.0),
                br: (rrect.radii.br - inset).max(0.0),
                bl: (rrect.radii.bl - inset).max(0.0),
            },
        };
        let inner_hit = point_in_rounded_rect_untransformed(p, inner);
        return outer_hit && !inner_hit;
    }
    false
}

fn point_in_rounded_rect_untransformed(p: [f32; 2], rrect: RoundedRect) -> bool {
    let Rect { x, y, w, h } = rrect.rect;
    let tl = rrect.radii.tl.min(w * 0.5).min(h * 0.5);
    let tr = rrect.radii.tr.min(w * 0.5).min(h * 0.5);
    let br = rrect.radii.br.min(w * 0.5).min(h * 0.5);
    let bl = rrect.radii.bl.min(w * 0.5).min(h * 0.5);
    let px = p[0] - x;
    let py = p[1] - y;
    if px < 0.0 || py < 0.0 || px > w || py > h {
        return false;
    }
    if px < tl && py < tl {
        let dx = tl - px;
        let dy = tl - py;
        return dx * dx + dy * dy <= tl * tl + 1e-5;
    }
    if px > w - tr && py < tr {
        let dx = px - (w - tr);
        let dy = tr - py;
        return dx * dx + dy * dy <= tr * tr + 1e-5;
    }
    if px > w - br && py > h - br {
        let dx = px - (w - br);
        let dy = py - (h - br);
        return dx * dx + dy * dy <= br * br + 1e-5;
    }
    if px < bl && py > h - bl {
        let dx = bl - px;
        let dy = py - (h - bl);
        return dx * dx + dy * dy <= bl * bl + 1e-5;
    }
    true
}

// --- Transform helpers ---
impl Transform2D {
    /// Apply the transform to a point (x, y).
    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        let a = self.m[0];
        let b = self.m[1];
        let c = self.m[2];
        let d = self.m[3];
        let e = self.m[4];
        let f = self.m[5];
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    /// Apply the inverse transform to a world-space point. Returns None if non-invertible.
    pub fn inverse_apply(&self, p: [f32; 2]) -> Option<[f32; 2]> {
        let a = self.m[0];
        let b = self.m[1];
        let c = self.m[2];
        let d = self.m[3];
        let e = self.m[4];
        let f = self.m[5];
        let det = a * d - b * c;
        if det.abs() < 1e-12 {
            return None;
        }
        let inv_det = 1.0 / det;
        let ia = d * inv_det;
        let ib = -b * inv_det;
        let ic = -c * inv_det;
        let id = a * inv_det;
        // Inverse translation = -inv_linear * [e, f]
        let ie = -(ia * e + ic * f);
        let iff = -(ib * e + id * f);
        Some([ia * p[0] + ic * p[1] + ie, ib * p[0] + id * p[1] + iff])
    }
}

/// Convenience: perform a one-shot hit test on the display list without keeping an index.
pub fn hit_test(list: &DisplayList, pos: [f32; 2]) -> Option<HitResult> {
    HitIndex::build(list).topmost_at(pos)
}

fn compute_locals(item: &HitItem, world: [f32; 2]) -> (Option<[f32; 2]>, Option<[f32; 2]>) {
    let p = match item.transform.inverse_apply(world) {
        Some(p) => p,
        None => return (None, None),
    };
    match &item.data {
        HitData::Rect(r) => {
            let local = [p[0] - r.x, p[1] - r.y];
            let uv = [
                if r.w.abs() > 1e-6 {
                    (local[0] / r.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if r.h.abs() > 1e-6 {
                    (local[1] / r.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::RoundedRect(rr) => {
            let r = rr.rect;
            let local = [p[0] - r.x, p[1] - r.y];
            let uv = [
                if r.w.abs() > 1e-6 {
                    (local[0] / r.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if r.h.abs() > 1e-6 {
                    (local[1] / r.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::Ellipse { center, radii } => {
            let local = [p[0] - center[0], p[1] - center[1]];
            let uv = [
                0.5 + if radii[0].abs() > 1e-6 {
                    local[0] / (2.0 * radii[0])
                } else {
                    0.0
                },
                0.5 + if radii[1].abs() > 1e-6 {
                    local[1] / (2.0 * radii[1])
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::StrokeRect { rect, .. } => {
            let local = [p[0] - rect.x, p[1] - rect.y];
            (Some(local), None)
        }
        HitData::StrokeRoundedRect { rrect, .. } => {
            let r = rrect.rect;
            let local = [p[0] - r.x, p[1] - r.y];
            (Some(local), None)
        }
        HitData::Text(_) => (None, None),
        HitData::PathBBox(r) => {
            let local = [p[0] - r.x, p[1] - r.y];
            let uv = [
                if r.w.abs() > 1e-6 {
                    (local[0] / r.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if r.h.abs() > 1e-6 {
                    (local[1] / r.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::BoxShadow { rrect } => {
            let r = rrect.rect;
            let local = [p[0] - r.x, p[1] - r.y];
            let uv = [
                if r.w.abs() > 1e-6 {
                    (local[0] / r.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if r.h.abs() > 1e-6 {
                    (local[1] / r.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::Hyperlink { rect, .. } => {
            let local = [p[0] - rect.x, p[1] - rect.y];
            let uv = [
                if rect.w.abs() > 1e-6 {
                    (local[0] / rect.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if rect.h.abs() > 1e-6 {
                    (local[1] / rect.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, DisplayList, Viewport};

    #[test]
    fn inline_hyperlink_hit_wins_over_overlapping_text_region() {
        let mut list = DisplayList {
            viewport: Viewport {
                width: 400,
                height: 120,
            },
            commands: Vec::new(),
        };

        let hyperlink = Hyperlink {
            text: "default blue underlined link".to_string(),
            pos: [20.0, 36.0], // baseline
            size: 16.0,
            color: ColorLinPremul::from_srgba_u8([0x00, 0x66, 0xcc, 0xff]),
            url: "https://example.com".to_string(),
            weight: 400.0,
            measured_width: Some(180.0),
            underline: true,
            underline_color: None,
            family: None,
            style: FontStyle::Normal,
        };

        // Inline hyperlink draw.
        list.commands.push(Command::DrawHyperlink {
            hyperlink,
            z: 0,
            transform: Transform2D::identity(),
            id: 1,
        });

        // Simulate overlapping paragraph text hit region at z + 10.
        list.commands.push(Command::HitRegionRect {
            id: 4242,
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 300.0,
                h: 40.0,
            },
            z: 10,
            transform: Transform2D::identity(),
        });

        let hit = HitIndex::build(&list)
            .topmost_at([80.0, 28.0])
            .expect("expected a hit over inline hyperlink");
        assert_eq!(hit.kind, HitKind::Hyperlink);
        match hit.shape {
            HitShape::Hyperlink { url, .. } => assert_eq!(url, "https://example.com"),
            other => panic!("expected hyperlink hit shape, got {other:?}"),
        }
    }
}
