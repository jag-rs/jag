# Ordered persistent Canvas 2D rendering

`jag_draw::canvas2d` is the native bitmap backend for Detir's P51-T1 browser
Canvas 2D foundation. It is separate from the frame-oriented `JagSurface::Canvas`
API: author drawing calls modify retained pixels in order, without scene
z sorting or replaying historical calls at each page repaint.

The browser host resolves drawing state and sends immutable `RectOp` values:
rectangle coordinates, affine transform, straight sRGB color, rectangle stroke
width or clear operation. JAG keeps an RGBA8 premultiplied sRGB drawing bitmap
and an RGBA16F linear premultiplied composition texture. Only new operations
are applied; `reset` clears retained pixels before subsequent operations.

`Renderer::new` takes the compositor's existing device/queue. `create_bitmap`
requires positive dimensions within native limits; the host represents a zero
bitmap without a GPU target. The caller admits the target's 12 bytes/pixel and
renderer/temporary submission resources before allocation. Detir uses its
shared page/device governor and retains reservations with native owner handles.
No new graphics device or WebGL backend is introduced.

`Renderer::apply` returns whether native work was submitted. The host retains
transient reservations through queue completion. `Bitmap::composition_view`
provides the linear texture view for the existing external-texture compositor;
the host must keep the bitmap owner alive and revoke stale presentation leases
on resize, reset and realm retirement.

Rectangle rasterization handles transformed fills, default miter rectangle
strokes, degenerate lines and clear operations. Axis-aligned fill antialiasing
and fractional clears match the frozen Chrome profile. General rotated edges
use geometric coverage and can differ from Chrome's raster antialiasing;
Detir's reference suite records a separate edge-only tolerance and preserves
its original strict comparison failure. Full browser pixel equivalence is not
claimed.

Browser wrappers, styles/state, sizing, scheduling and lifecycle policy live in
Detir. Paths and text/images are described in the later milestones below;
effects, pixel APIs, wider color formats and complete platform qualification
remain open. This backend does not by itself establish Chart.js,
D3, full Canvas 2D, OffscreenCanvas or PDF export support.


## P51-T2 native paths and paint frontend

`canvas2d::geometry` builds SVG/current paths, stroke outlines and geometric hit
tests. `canvas2d::raster` resolves cumulative arbitrary masks and sRGB paints
through the existing tiny-skia raster dependency. `Renderer::upload` publishes
premultiplied sRGB pixels into the same persistent GPU target and normalizes
for native composition. The rectangle GPU path remains available independently.

Paints support linear, two-circle radial and conic gradients with ordered color
stops, plus immutable image snapshots and transformed repetition. Browser
conversions/state and source snapshots remain Detir's responsibility. Raster
commands validate their native descriptors before using geometry or paint data.
This CPU raster frontend does not certify GPU path performance, exact Chrome
antialiasing, wider color formats or full browser conformance. The local Detir
reference profile explicitly retains up to 96/255 differences only near
reference geometry edges; interiors retain 2/255. Physical/platform and upstream
qualification remains open. See Detir's `docs/canvas2d-paths.md` for the precise
profile, limits and evidence policy.


## P51-T3 text and image sources

`canvas2d::text::Fonts` resolves real system/host/loaded font data and prepares
HarfBuzz-shaped glyph paths plus ink/advance/baseline metrics. Bidi and grapheme
fallback reuse Jag text infrastructure. Native image drawing supports clipped
source rectangles, normalized negative geometry, immutable snapshots, nearest
and premultiplied bilinear sampling. SVG image data reuses usvg/resvg and can be
rasterized at destination density without external file/URL references.

Browser state, source readiness and host asset/media scheduling stay in Detir.
The local outline-font profile does not certify all CSS Fonts, variable/color
fonts, exact Chrome text AA, full upstream conformance or platform performance.
Text edge tolerances and original failures are explicit in Detir's
`docs/canvas2d-content.md`; glyph metrics derive from real font geometry.
