# jag-draw

GPU-accelerated 2D rendering engine built on [wgpu](https://crates.io/crates/wgpu).

Part of the [jag](https://crates.io/crates/jag) toolkit.

## Features

- Display list batching with GPU upload
- Text rendering with system fonts (harfrust + swash), subpixel rendering
- SVG rendering (usvg/resvg)
- Image loading and caching
- Rounded rectangles, gradients, shadows
- Hit-testing via color-encoded regions

## Usage

```toml
[dependencies]
jag-draw = "0.1"
```

Most users should use the [`jag`](https://crates.io/crates/jag) meta-crate instead, which re-exports this crate along with `jag-ui` and `jag-surface`.

## License

MIT
