# jag-surface

Canvas-style drawing API on top of [jag-draw](https://crates.io/crates/jag-draw).

Part of the [jag](https://crates.io/crates/jag) toolkit.

## Features

- `Canvas` with `fill_rect`, `rounded_rect`, `draw_text`, `draw_image`, etc.
- `JagSurface` manages GPU device, frame encoding, and presentation
- Intermediate texture support for smooth window resizing

## Usage

```toml
[dependencies]
jag-surface = "0.1"
```

Most users should use the [`jag`](https://crates.io/crates/jag) meta-crate instead.

## License

MIT
