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
Detir. Paths, arbitrary path clipping, extended stroke state, text/images,
effects, pixel APIs, wider color formats and complete platform qualification
remain later milestones. This backend does not by itself establish Chart.js,
D3, full Canvas 2D, OffscreenCanvas or PDF export support.
