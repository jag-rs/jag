# jag-surface

Canvas-style drawing API on top of [jag-draw](https://crates.io/crates/jag-draw).

Part of the [jag](https://crates.io/crates/jag) toolkit.

## Features

- `Canvas` with `fill_rect`, `rounded_rect`, `draw_text`, `draw_image`, etc.
- `JagSurface` manages GPU device, frame encoding, and presentation
- Intermediate texture support for smooth window resizing
- Retained isolated surfaces with bounded GPU texture ownership
- Immutable paint scenes and content-space tiles for nested scrolling

## Retained surfaces

Eligible opacity/filter surfaces keep their pixels between frames. This also
applies to iframe isolation groups created by a host. The default texture
budget is 32 MiB. Each texture is charged at least 4 KiB to bound bookkeeping
for tiny tiles; there is no separate 128-entry limit. Tiles already used in
the current frame are protected from eviction. When the visible set exceeds
the budget, uncached tiles still render and the resident subset can be reused
on the next frame. `set_retained_layer_budget(0)` disables
retention through the same compositor; `retained_layer_stats()` reports hits,
misses, bypasses, rasterized/reused pixels, actual resident texture bytes,
budget usage (including the minimum entry charge), and evictions.

The cache compares layer-local commands and clips, pixel size, device/UI scale,
font provider identity and `cache_tag()`, filters, and child surface revisions.
Moving a layer by whole physical pixels or changing its composite opacity can
reuse its texture. A child paint/scroll change invalidates dependent parents.
Fractional pixel phase stays in the key, preserving raster sharpness.

File-backed images/SVGs use file length and modification time as revisions;
pending image decoding bypasses retention until pixels are available. SVG
external dependency revisions are not tracked. Dynamic text, external
canvas/video textures, backdrop filters and texture masks bypass raster
retention. Clearing transient compositor texture registrations each frame
releases evicted resources without discarding retained surfaces.

## Retained scroll scenes

`push_bound_scroll_layer` binds an independent content owner to a named input
translation. Nested owners, fixed/sticky counter-translations, and scrollbar
thumbs retain their own raster tiles. Tiles are 512 physical pixels with a
sampling gutter; visible bins are painted or reused and composed under live
clips. Whole physical-pixel movement reuses pixels; fractional phase remains
part of raster validation.

`begin_scroll_scene_capture` / `finish_scroll_scene_capture` commit balanced
paint commands once. `replay_scroll_scene` supplies live properties without
rebuilding those commands. Scene compilation records translation/clip
properties and spatial command bins. Hosts must invalidate capture for actual
paint/resource changes and check `can_replay_scroll_scene` before reuse.
Masks, backdrop effects, and live side channels reject capture and preserve
ordinary painting. Rounded clipping supports the nearest rounded clip plus
rectangular ancestor intersections, not arbitrary rounded-clip intersections.

Content changes compare commands in visible tiles; fine-grained damage-region
updates remain open. This renderer does not create a presentation thread or
move application JavaScript. The budget measures cache texture ownership,
excluding temporary frame resources, compiled CPU metadata, and other GPU
allocations. Physical device scroll acceptance is the host's responsibility.

## Usage

```toml
[dependencies]
jag-surface = "0.1"
```

Most users should use the [`jag`](https://crates.io/crates/jag) meta-crate instead.

## License

MIT
