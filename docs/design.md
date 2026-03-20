# Jag Extraction Design

Extract Detir's renderer and UI elements into publishable crates (`jag-draw`, `jag-ui`, `jag`) so the community can build UIs in Rust and contribute fixes that flow back into the Detir stack.

## Crate Structure

```
jag        (crates.io)  — Meta-crate re-exporting jag-draw + jag-ui
jag-draw   (crates.io)  — GPU 2D renderer (display lists, painters, shaders, text, hit-testing)
jag-ui     (crates.io)  — UI elements, widgets, layout (Taffy), events
detir-scene (internal)  — IR bridge, Arth VM, web-compat (consumes jag-draw + jag-ui)
```

GitHub: https://github.com/jag-rs/jag

## Dependency Flow

```
jag-draw  ← no upward deps (self-contained GPU renderer)
   ↑
jag-ui    ← depends on jag-draw for rendering
   ↑
detir-scene ← depends on jag-draw + jag-ui + detir-ir + detir-arth + detir-js
```

Community contributes to `jag-draw` and `jag-ui`. Fixes flow into `detir-scene` via normal dependency updates.

## jag-draw

### Source

Current `engine-core` + `engine-shaders` + `detir-text` renamed/re-exported.

### Public API (carried over from engine-core)

- `GraphicsEngine` — top-level GPU handle (device, queue, allocator)
- `RenderAllocator`, `OwnedBuffer`, `OwnedTexture` — GPU resources
- `Painter` — display list builder (`rect()`, `text()`, `stroke_rect()`, `ellipse()`, `path()`, etc.)
- `DisplayList`, `Command` — display list representation
- `PassManager::render_unified()` — GPU rendering pipeline
- `HitIndex`, `HitTest` — spatial hit-testing
- `Color` / `ColorLinPremul` — premultiplied linear color
- `Brush` — Solid, LinearGradient, RadialGradient
- `Rect`, `RoundedRect`, `RoundedRadii` — geometry
- `Transform2D` — affine transforms
- `Stroke`, `BoxShadowSpec` — stroke/shadow params
- `TextRun`, `FontStyle` — text drawing
- `Path` — SVG/custom paths
- `TextProvider` trait + `CosmicTextProvider`, `FreeTypeProvider`
- `DpiScale`, `LogicalPixels`, `PhysicalPixels` — DPI handling
- `ExternalTextureId` — external texture composition
- `Background` — root background
- `wgpu` — re-exported

### External Dependencies

- wgpu 0.19, palette, bytemuck
- fontdue, harfrust, fontdb, swash
- lyon (path tessellation), usvg/resvg (SVG)
- image (PNG/JPEG/GIF/WebP)

### Features

- `pdf_export` — PDF backend

### Effort

~2-3 days. engine-core has zero upward dependencies — this is a rename + publish.

## jag-ui

### Source

Elements and widgets extracted from `detir-scene`, with IR dependencies removed. Elements own state directly in Rust.

### Target API

```rust
use jag_draw::{Painter, Color, Brush};
use jag_ui::{Ui, Button, TextInput, Container, Flex};

// Elements own their state
let mut btn = Button::new("Submit");
btn.on_click(|ctx| { /* handler */ });

let mut input = TextInput::new();
input.placeholder("Enter name...");

// Layout via Taffy (flex/grid)
let root = Container::flex_column()
    .child(btn)
    .child(input)
    .build();

// Render loop
let mut ui = Ui::new(text_provider);
ui.layout(&root, available_size);    // Taffy computes positions
ui.paint(&root, &mut painter);       // Emits to jag-draw DisplayList
ui.handle_event(&mut root, event);   // Dispatches input events
let hit = ui.hit_test(&root, pos);   // Hit testing
```

### Key Design Decisions

1. **Standalone state** — Elements hold their own state structs directly. No ViewNode, no IR types. Props set in Rust code.

2. **Layout included** — Taffy (flex/grid) wrapped as part of the API. Users can also bypass it and provide manual positions.

3. **Self-contained events** — Event dispatch system owned by `Ui`, not by an external IR renderer. Focus management built in.

4. **Theme/styling** — Default theme included. Users can customize via a `Theme` struct.

### Elements to Extract

From `detir-scene/src/elements/`:
- Button, TextInput, Textarea, Checkbox, RadioButton
- Select, Slider, ToggleSwitch, DatePicker, ColorPicker
- Badge, Card, Modal, Alert, Confirm
- Container (flex/grid), Link, Image, Table
- RichTextEditor

From `detir-scene/src/widgets/`:
- All widget implementations

### What Changes

- Remove all `ViewNode`, `ViewNodeId`, `ViewNodeKind` references
- Remove `IrRenderer` dependency from elements
- Replace `HitRegionRegistry` with standalone version in `Ui`
- Create standalone `ElementState` model (replacing IR-derived state)
- Build `EventDispatcher` for input routing
- Wrap Taffy layout into `Ui::layout()`

### Effort

~3-4 weeks. Bulk of the work:
- Extract elements, remove IR deps (~2-3 weeks)
- Standalone event system (~3-4 days)
- Layout API wrapping Taffy (~2-3 days)

## detir-scene (Internal Bridge)

### Changes

`detir-scene` becomes a consumer of `jag-draw` + `jag-ui` instead of containing the renderer and elements.

### Bridge Pattern

```rust
// ir_adapter.rs — maps IR → jag-ui element state
fn sync_view_node(node: &ViewNode, element: &mut dyn Element) {
    // ViewNode styles → element props
    // ViewNode directives → element state
    // ViewNode events → element callbacks
}
```

- `ir_renderer` delegates to `jag-ui` for layout/paint/events
- `ir_adapter.rs` becomes the IR↔element bridge
- Arth VM, JS runtime, web-compat remain in detir-scene untouched

### Effort

~1-2 weeks. Highest regression risk — rewiring how the entire IR rendering path works.

## Total Effort

| Work Item | Time | Risk |
|-----------|------|------|
| jag-draw: Rename engine-core + publish | 2-3 days | Low |
| jag-ui: Extract elements, standalone state | 2-3 weeks | Medium |
| jag-ui: Event system + layout API | ~1 week | Medium |
| detir-scene: Bridge rewrite | 1-2 weeks | **High** |
| Testing & stabilization | 1 week | Medium |
| Docs, examples, crates.io publishing | 3-4 days | Low |
| **Total** | **~5-7 weeks** | |

## Risk Mitigation

1. **detir-scene bridge regression** (highest risk) — Keep existing tests passing throughout. Extract incrementally: first jag-draw (safe rename), then jag-ui element by element, bridging each one before moving to the next.

2. **API stability** — Start with 0.1.0. Mark APIs as unstable where needed. Avoid committing to stable public APIs prematurely.

3. **Incremental extraction** — Don't extract all elements at once. Start with 3-4 core elements (Button, TextInput, Container, Text), get the full pipeline working, then extract the rest.
