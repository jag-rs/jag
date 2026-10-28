# Headless RGBA readback

`JagSurface::end_frame_headless` returns tightly packed RGBA bytes, regardless
of whether its render target is RGBA or BGRA. The target's format determines
the red/blue channel normalization after GPU mapping. This uses the same helper
as `grab_last_frame_rgba`, so screenshots and headless captures agree.

P33 Babylon qualification exposed a discrepancy: the headless path previously
returned raw BGRA bytes while documenting RGBA. A red clear on `Bgra8Unorm`
returned `[0, 0, 255, 255]`. `headless_readback_normalizes_bgra_surface_formats`
failed before the correction and passes afterward. It uses actual GPU readback,
odd row width, two rows, red and cyan, and both unorm/sRGB RGBA/BGRA formats.

```sh
cargo test -p jag-surface --test headless_readback -- --test-threads=1
```

The normalization changes CPU readback only; GPU rendering, texture/shader
layouts and presented surface colors are unchanged.
