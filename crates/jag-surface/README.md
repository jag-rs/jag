# jag-surface

Canvas-style drawing API on top of [jag-draw](https://crates.io/crates/jag-draw).

Part of the [jag](https://crates.io/crates/jag) toolkit.

## Features

- `Canvas` with `fill_rect`, `rounded_rect`, `draw_text`, `draw_image`, etc.
- `JagSurface` manages GPU device, frame encoding, and presentation
- Intermediate texture support for smooth window resizing
- Retained isolated surfaces with bounded GPU texture ownership

## Retained surfaces

Eligible opacity/filter surfaces keep their pixels between frames. This also
applies to iframe isolation groups created by a host. The default texture
budget is 32 MiB, with up to 128 entries. `set_retained_layer_budget(0)` disables
retention through the same compositor; `retained_layer_stats()` reports hits,
misses, bypasses, rasterized/reused pixels, resident bytes, and evictions.

The cache compares layer-local commands and clips, pixel size, device/UI scale,
font provider identity and `cache_tag()`, filters, and child surface revisions.
Moving a layer by whole physical pixels or changing its composite opacity can
reuse its texture. A child paint/scroll change invalidates dependent parents.
Fractional pixel phase stays in the key, preserving raster sharpness.

File-backed image/SVG draws, dynamic text, external canvas/video textures,
backdrop filters and texture masks bypass retention. Their resource revisions
are not tracked here. Vector SVG paths are eligible. Clearing transient texture
registrations after presenting does not discard retained surfaces.

This is the isolated-surface retention phase. Content-space scroll tiles,
damage-region rasterization, and a compositor scheduled independently of the
application's JavaScript thread remain host/rendering work. The budget measures
cache texture ownership, excluding temporary frame resources and other GPU
allocations.

## Usage

```toml
[dependencies]
jag-surface = "0.1"
```

Most users should use the [`jag`](https://crates.io/crates/jag) meta-crate instead.

## License

MIT
