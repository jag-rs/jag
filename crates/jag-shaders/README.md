# jag-shaders

WGSL shader modules for [jag-draw](https://crates.io/crates/jag-draw).

Part of the [jag](https://crates.io/crates/jag) toolkit. This is an internal crate — most users should depend on `jag` or `jag-draw` instead.

## Text coverage

The text pipeline uses single-source premultiplied alpha blending. RGB glyph
coverage is resolved to its arithmetic mean before multiplying the text color
and alpha. Using the strongest subpixel as whole-pixel alpha makes dark glyphs
heavier; tinting each color channel separately with a shared alpha adds fringes.
Equal-channel grayscale masks retain their coverage. Color emoji keep their
separate premultiplied RGBA path.

This is a coverage-compositing correction, not full browser text parity. True
LCD rendering still requires per-channel destination blending and an eligible
opaque destination. The current fallback also works on transparent layers.
Font selection, shaping, requested weight, and glyph placement are unchanged.

The `jag-surface` GPU regression `text_coverage` checks RGB edges on light and
dark backgrounds, colored and translucent text, opacity groups, frame caching,
and DPR 1, 1.25, 1.5, 2, and 3.

## License

MIT
