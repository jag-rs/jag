# Jag Extraction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract Detir's GPU renderer and UI elements into publishable crates (`jag-draw`, `jag-ui`, `jag`) so the community can build UIs in Rust and contribute fixes upstream.

## Progress

### Phase 1: jag-draw (Extract engine-core)
- [x] Task 1: Set up jag workspace and jag-draw crate
- [x] Task 2: Copy detir-surface as jag-surface

### Phase 2: jag-ui (Extract elements/widgets)
- [x] Task 3: Create jag-ui scaffold (events, focus, theme)
- [x] Task 4: Extract Button, Text, Checkbox
- [x] Task 5: Extract remaining form elements
- [x] Task 6: Extract container/compound elements
- [x] Task 7: Extract widgets
- [x] Task 8: Build Ui coordinator
- [x] Task 9: Add demo example
- [x] Task 10: Wire up jag meta-crate

### Phase 3: detir-scene bridge (internal)
- [x] Task 11: Make detir-scene depend on jag crates
- [x] Task 12: Create IR bridge adapter
- [ ] Task 13: Migrate IrRenderer to use jag-ui elements
- [ ] Task 14: Remove duplicate element code from detir-scene
- [x] ~~Task 15: Migrate detir crates to jag-draw~~ (not viable — bridge with convert functions used)

### Phase 4: Publish
- [x] Task 16: Publish to crates.io (v0.1.1 live)
- [x] Add README, LICENSE, per-crate READMEs
- [x] Add github:jag-rs:jag team as crate owner
- [x] Remove all detir/internal references from published code

### v0.1.2 Roadmap
- [ ] Full element showcase demo (all 19 elements + widgets)
- [ ] Taffy layout integration (`Ui::layout_elements()` auto-wiring)
- [ ] Layout-driven demo (flex/grid instead of hardcoded coords)

**Architecture:** Three published crates — `jag-draw` (GPU 2D renderer, extracted from `engine-core`), `jag-ui` (elements/widgets/layout/events, extracted from `detir-scene`), and `jag` (meta-crate re-exporting both). `detir-scene` becomes a consumer via an IR bridge layer.

**Tech Stack:** Rust 2024, wgpu 0.19, Taffy 0.9, harfrust 0.5, lyon 1.0, fontdue 0.7

**Design Doc:** `docs/plans/2026-03-07-jag-extraction-design.md`

**GitHub:** https://github.com/jag-rs/jag

---

## Phase 1: jag-draw (Extract engine-core)

### Task 1: Set up jag workspace and jag-draw crate

**Files:**
- Create: `jag/Cargo.toml` (workspace root)
- Modify: `jag/crates/jag-draw/Cargo.toml` (replace placeholder)
- Modify: `jag/crates/jag-draw/src/lib.rs` (replace placeholder)
- Copy: `detir/crates/engine-core/src/*` → `jag/crates/jag-draw/src/`
- Copy: `detir/crates/engine-shaders/` → `jag/crates/jag-shaders/`
- Copy: `detir/crates/detir-text/` → `jag/crates/jag-text/`

**Step 1: Create workspace Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/jag",
    "crates/jag-draw",
    "crates/jag-ui",
    "crates/jag-shaders",
    "crates/jag-text",
]

[workspace.package]
edition = "2024"
license = "MIT OR Apache-2.0"
repository = "https://github.com/jag-rs/jag"

[workspace.dependencies]
wgpu = "0.19"
palette = "0.7"
bytemuck = { version = "1.14", features = ["derive"] }
anyhow = "1.0"
thiserror = "1.0"
fontdue = "0.7"
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "webp"] }
swash = "0.1"
fontdb = "0.23"
harfrust = "0.5.2"
unicode-segmentation = "1.11"
unicode-linebreak = "0.1"
unicode-bidi = "0.3"
pdf-writer = { version = "0.14", optional = true }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
taffy = "0.5"
```

**Step 2: Copy engine-core sources into jag-draw**

```bash
# Copy engine-core source files
cp -r detir/crates/engine-core/src/* jag/crates/jag-draw/src/

# Copy engine-shaders as jag-shaders
cp -r detir/crates/engine-shaders jag/crates/jag-shaders

# Copy detir-text as jag-text
cp -r detir/crates/detir-text jag/crates/jag-text
```

**Step 3: Rename all internal references**

In `jag/crates/jag-draw/`:
- Replace `engine_shaders` → `jag_shaders` in all `.rs` files
- Replace `engine-shaders` → `jag-shaders` in Cargo.toml
- Replace `detir_text` → `jag_text` in all `.rs` files
- Replace `detir-text` → `jag-text` in Cargo.toml

In `jag/crates/jag-shaders/`:
- Rename crate to `jag-shaders` in Cargo.toml

In `jag/crates/jag-text/`:
- Rename crate to `jag-text` in Cargo.toml

**Step 4: Update jag-draw Cargo.toml with real dependencies**

Replace the placeholder Cargo.toml with full dependencies copied from
`detir/crates/engine-core/Cargo.toml`, updating internal crate references:
- `engine-shaders = { path = "../engine-shaders" }` → `jag-shaders = { path = "../jag-shaders" }`
- `detir-text = { path = "../detir-text" }` → `jag-text = { path = "../jag-text" }`

**Step 5: Build and fix**

```bash
cd jag && cargo build -p jag-draw
```

Fix any remaining import/reference issues until it compiles.

**Step 6: Run engine-core tests**

```bash
cd jag && cargo test -p jag-draw
```

Ensure all existing tests pass.

**Step 7: Commit**

```bash
git add jag/
git commit -m "feat: set up jag-draw crate extracted from engine-core"
```

---

### Task 2: Copy detir-surface as jag-surface (optional canvas layer)

**Files:**
- Copy: `detir/crates/detir-surface/` → `jag/crates/jag-surface/`
- Modify: `jag/Cargo.toml` (add to workspace members)

**Step 1: Copy and rename**

```bash
cp -r detir/crates/detir-surface jag/crates/jag-surface
```

In `jag/crates/jag-surface/`:
- Rename crate to `jag-surface` in Cargo.toml
- Replace `engine-core` → `jag-draw` in Cargo.toml
- Replace `engine_core` → `jag_draw` in all `.rs` files
- Replace `detir_surface` → `jag_surface` in all `.rs` files

Add `"crates/jag-surface"` to workspace members in `jag/Cargo.toml`.

**Step 2: Build and fix**

```bash
cd jag && cargo build -p jag-surface
```

**Step 3: Run tests**

```bash
cd jag && cargo test -p jag-surface
```

**Step 4: Commit**

```bash
git add jag/crates/jag-surface/
git commit -m "feat: add jag-surface canvas API layer"
```

---

## Phase 2: jag-ui (Extract elements/widgets)

### Task 3: Create jag-ui crate scaffold with event types

**Files:**
- Modify: `jag/crates/jag-ui/Cargo.toml` (replace placeholder)
- Modify: `jag/crates/jag-ui/src/lib.rs` (replace placeholder)
- Create: `jag/crates/jag-ui/src/event.rs`
- Create: `jag/crates/jag-ui/src/focus.rs`
- Create: `jag/crates/jag-ui/src/theme.rs`

The event system from `detir-scene` currently re-exports from `detir_platform`. For jag-ui,
we define our own event types so there's no dependency on detir-platform.

**Step 1: Write event type tests**

Create `jag/crates/jag-ui/src/event.rs` with test:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_click_event_creation() {
        let evt = MouseClickEvent {
            button: MouseButton::Left,
            state: ElementState::Pressed,
            x: 100.0,
            y: 200.0,
            click_count: 1,
        };
        assert_eq!(evt.x, 100.0);
        assert_eq!(evt.click_count, 1);
    }

    #[test]
    fn event_result_default() {
        let r = EventResult::Ignored;
        assert!(matches!(r, EventResult::Ignored));
    }
}
```

**Step 2: Implement event types**

Extract from `detir/crates/detir-scene/src/event_handler.rs` (lines 9-100+).
Define standalone (no detir-platform dependency):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton { Left, Right, Middle }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementState { Pressed, Released }

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

// KeyCode — subset of common keys needed by elements
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Tab, Enter, Escape, Space, Backspace, Delete,
    ArrowLeft, ArrowRight, ArrowUp, ArrowDown,
    Home, End, PageUp, PageDown,
    KeyA, KeyC, KeyV, KeyX, KeyZ,
    // ... add as needed
    Other(u32),
}

#[derive(Debug, Clone, Copy)]
pub enum EventResult { Handled, Ignored }

#[derive(Debug, Clone)]
pub struct MouseClickEvent {
    pub button: MouseButton,
    pub state: ElementState,
    pub x: f32,
    pub y: f32,
    pub click_count: u32,
}

#[derive(Debug, Clone)]
pub struct KeyboardEvent {
    pub key: KeyCode,
    pub state: ElementState,
    pub modifiers: Modifiers,
    pub text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MouseMoveEvent { pub x: f32, pub y: f32 }

#[derive(Debug, Clone)]
pub struct ScrollEvent {
    pub x: f32, pub y: f32,
    pub delta_x: f32, pub delta_y: f32,
}

pub trait EventHandler {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult { EventResult::Ignored }
    fn handle_keyboard(&mut self, event: &KeyboardEvent) -> EventResult { EventResult::Ignored }
    fn handle_mouse_move(&mut self, event: &MouseMoveEvent) -> EventResult { EventResult::Ignored }
    fn handle_scroll(&mut self, event: &ScrollEvent) -> EventResult { EventResult::Ignored }
    fn is_focused(&self) -> bool;
    fn set_focused(&mut self, focused: bool);
    fn contains_point(&self, x: f32, y: f32) -> bool;
}
```

**Step 3: Implement focus manager**

Extract from `detir/crates/detir-scene/src/focus_manager.rs`.
Replace `ElementType` enum with a generic `FocusId`:

```rust
// jag/crates/jag-ui/src/focus.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusId(pub u64);

#[derive(Debug, Clone, Copy)]
pub enum FocusDirection { Forward, Backward }

#[derive(Debug, Clone, Copy)]
pub enum FocusResult {
    Moved(FocusId),
    Wrapped(FocusId),
    NoFocusableElements,
    Unchanged,
}

pub struct FocusManager {
    current: Option<FocusId>,
    focus_visible: bool,
    order: Vec<(FocusId, i32)>, // (id, tabindex)
}

impl FocusManager {
    pub fn new() -> Self { ... }
    pub fn current(&self) -> Option<FocusId> { ... }
    pub fn is_focus_visible(&self) -> bool { ... }
    pub fn set_focus(&mut self, id: FocusId) { ... }
    pub fn clear_focus(&mut self) { ... }
    pub fn navigate(&mut self, dir: FocusDirection) -> FocusResult { ... }
    pub fn register(&mut self, id: FocusId, tabindex: i32) { ... }
    pub fn unregister(&mut self, id: FocusId) { ... }
    pub fn rebuild_order(&mut self) { ... }
}
```

**Step 4: Implement theme**

Extract from `detir/crates/detir-scene/src/theme.rs`.
Uses only `jag_draw::Color` / `ColorLinPremul`:

```rust
// jag/crates/jag-ui/src/theme.rs
use jag_draw::ColorLinPremul;

pub enum ThemeMode { Dark, Light }

pub struct ElementColors {
    pub text: ColorLinPremul,
    pub input_bg: ColorLinPremul,
    pub input_border: ColorLinPremul,
    pub button_bg: ColorLinPremul,
    pub button_fg: ColorLinPremul,
    pub focus_ring: ColorLinPremul,
    pub error: ColorLinPremul,
}

impl ElementColors {
    pub fn for_theme(mode: ThemeMode) -> Self { ... }
}

pub struct Theme {
    pub mode: ThemeMode,
    pub colors: ElementColors,
    pub font_size: f32,
    pub border_radius: f32,
    pub spacing: f32,
}

impl Default for Theme {
    fn default() -> Self { Self::dark() }
}

impl Theme {
    pub fn dark() -> Self { ... }
    pub fn light() -> Self { ... }
}
```

**Step 5: Update jag-ui Cargo.toml**

```toml
[package]
name = "jag-ui"
version = "0.1.0"
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "UI elements, widgets, and layout for jag-draw"
keywords = ["ui", "widgets", "layout", "gpu", "gui"]
categories = ["gui", "rendering"]

[dependencies]
jag-draw = { path = "../jag-draw" }
jag-surface = { path = "../jag-surface" }
jag-text = { path = "../jag-text" }
taffy = { workspace = true }
```

**Step 6: Wire up lib.rs**

```rust
pub mod event;
pub mod focus;
pub mod theme;

pub use event::*;
pub use focus::{FocusId, FocusDirection, FocusManager, FocusResult};
pub use theme::{Theme, ThemeMode, ElementColors};
```

**Step 7: Build and test**

```bash
cd jag && cargo test -p jag-ui
```

**Step 8: Commit**

```bash
git add jag/crates/jag-ui/
git commit -m "feat: jag-ui scaffold with event types, focus manager, theme"
```

---

### Task 4: Extract core elements — Button, Text, Checkbox

**Files:**
- Create: `jag/crates/jag-ui/src/elements/mod.rs`
- Create: `jag/crates/jag-ui/src/elements/button.rs`
- Create: `jag/crates/jag-ui/src/elements/text.rs`
- Create: `jag/crates/jag-ui/src/elements/checkbox.rs`

Start with the three simplest elements to establish the pattern. These have
minimal IR coupling (Button and Checkbox have zero direct IR imports).

**Step 1: Define the Element trait**

```rust
// jag/crates/jag-ui/src/elements/mod.rs

use jag_draw::Rect;
use jag_surface::Canvas;
use crate::event::{EventHandler, EventResult, MouseClickEvent, KeyboardEvent};
use crate::focus::FocusId;

pub mod button;
pub mod text;
pub mod checkbox;

pub use button::Button;
pub use text::Text;
pub use checkbox::Checkbox;

pub trait Element: EventHandler {
    fn rect(&self) -> Rect;
    fn set_rect(&mut self, rect: Rect);
    fn render(&self, canvas: &mut Canvas, z: i32);
    fn focus_id(&self) -> Option<FocusId>;
}
```

**Step 2: Extract Button**

Source: `detir/crates/detir-scene/src/elements/button.rs` (359 lines)

Key changes from original:
- Remove `data_onclick: Option<String>` (IR-specific intent dispatch) →
  replace with `on_click: Option<Box<dyn FnMut()>>`
- Remove IR-specific icon_source path resolution → use direct path/bytes
- Replace `engine_core::` imports → `jag_draw::`
- Replace `detir_surface::` → `jag_surface::`
- All state fields are directly settable (builder pattern)

```rust
// jag/crates/jag-ui/src/elements/button.rs
use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect, SvgStyle};
use jag_surface::{Canvas, ImageFitMode};
use crate::event::*;
use crate::focus::FocusId;
use crate::theme::Theme;

pub struct Button {
    pub rect: Rect,
    pub label: String,
    pub label_size: f32,
    pub bg: ColorLinPremul,
    pub fg: ColorLinPremul,
    pub radius: f32,
    pub focused: bool,
    pub focus_visible: bool,
    pub padding: [f32; 4], // [top, right, bottom, left]
    pub icon_path: Option<String>,
    pub icon_size: f32,
    pub icon_spacing: f32,
    pub icon_only: bool,
    focus_id: FocusId,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self { ... }
    pub fn with_theme(label: impl Into<String>, theme: &Theme) -> Self { ... }
    pub fn render(&self, canvas: &mut Canvas, z: i32) { ... }
    pub fn hit_test(&self, x: f32, y: f32) -> bool { ... }
}

impl EventHandler for Button {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult { ... }
    fn handle_keyboard(&mut self, event: &KeyboardEvent) -> EventResult { ... }
    fn is_focused(&self) -> bool { self.focused }
    fn set_focused(&mut self, focused: bool) { self.focused = focused; }
    fn contains_point(&self, x: f32, y: f32) -> bool { self.hit_test(x, y) }
}
```

Copy the rendering logic from the original `button.rs` `render()` method
(lines 78-231), replacing import paths.

**Step 3: Extract Text**

Source: `detir/crates/detir-scene/src/elements/text.rs` (23 lines)

This is trivially small — copy directly, replace `engine_core::Color` → `jag_draw::Color`.

**Step 4: Extract Checkbox**

Source: `detir/crates/detir-scene/src/elements/checkbox.rs` (386 lines)

Key changes:
- Remove validation fields tied to IR (`required`, `error_message`) →
  make them generic optional fields
- Replace `engine_core::` → `jag_draw::`
- Replace `detir_surface::` → `jag_surface::`

**Step 5: Write tests for each element**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_hit_test() {
        let btn = Button::new("Click me");
        // Assuming default rect is set via set_rect
        let mut btn = btn;
        btn.rect = Rect { x: 10.0, y: 10.0, w: 100.0, h: 40.0 };
        assert!(btn.hit_test(50.0, 30.0));
        assert!(!btn.hit_test(0.0, 0.0));
    }

    #[test]
    fn checkbox_toggle() {
        let mut cb = Checkbox::new();
        assert!(!cb.checked);
        cb.toggle();
        assert!(cb.checked);
    }
}
```

**Step 6: Build and test**

```bash
cd jag && cargo test -p jag-ui
```

**Step 7: Commit**

```bash
git add jag/crates/jag-ui/src/elements/
git commit -m "feat: extract Button, Text, Checkbox elements into jag-ui"
```

---

### Task 5: Extract remaining form elements

**Files:**
- Create: `jag/crates/jag-ui/src/elements/input_box.rs`
- Create: `jag/crates/jag-ui/src/elements/text_area.rs`
- Create: `jag/crates/jag-ui/src/elements/radio.rs`
- Create: `jag/crates/jag-ui/src/elements/select.rs`
- Create: `jag/crates/jag-ui/src/elements/toggle_switch.rs`
- Create: `jag/crates/jag-ui/src/elements/slider.rs`
- Create: `jag/crates/jag-ui/src/elements/date_picker.rs`
- Create: `jag/crates/jag-ui/src/elements/link.rs`

**Sources in detir-scene:**
- `input_box.rs` (2000+ lines) — Has `detir_ir::view::TextAlign` import. Replace with local `TextAlign` enum.
- `text_area.rs` — Similar to InputBox, same treatment.
- `radio.rs` — No IR imports (via adapter). Direct copy + rename.
- `select.rs` — No direct IR imports. Direct copy + rename.
- `toggle_switch.rs` — No IR imports. Direct copy + rename.
- `slider.rs`, `date_picker.rs`, `link.rs` — Extract similarly.

**Step 1: Define local TextAlign enum**

```rust
// jag/crates/jag-ui/src/elements/text_align.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}
```

**Step 2: Extract each element**

For each element file:
1. Copy from `detir/crates/detir-scene/src/elements/<name>.rs`
2. Replace `engine_core::` → `jag_draw::`
3. Replace `detir_surface::` → `jag_surface::`
4. Replace `detir_text::` → `jag_text::`
5. Replace any `detir_ir::view::TextAlign` → `crate::elements::TextAlign`
6. Remove `ViewNodeId` references (Modal) → use `FocusId` or element-local ID
7. Remove `HitRegionRegistry` dependency → use element's own `contains_point()`
8. Implement `EventHandler` trait

**Step 3: Write tests for InputBox**

```rust
#[test]
fn input_box_text_insertion() {
    let mut input = InputBox::new();
    input.insert_text("hello");
    assert_eq!(input.text(), "hello");
}

#[test]
fn input_box_placeholder() {
    let mut input = InputBox::new();
    input.set_placeholder("Type here...");
    assert_eq!(input.placeholder(), "Type here...");
    assert!(input.text().is_empty());
}
```

**Step 4: Build and test**

```bash
cd jag && cargo test -p jag-ui
```

**Step 5: Commit**

```bash
git add jag/crates/jag-ui/src/elements/
git commit -m "feat: extract remaining form elements into jag-ui"
```

---

### Task 6: Extract container elements and compound elements

**Files:**
- Create: `jag/crates/jag-ui/src/elements/container.rs`
- Create: `jag/crates/jag-ui/src/elements/card.rs`
- Create: `jag/crates/jag-ui/src/elements/modal.rs`
- Create: `jag/crates/jag-ui/src/elements/alert.rs`
- Create: `jag/crates/jag-ui/src/elements/image.rs`
- Create: `jag/crates/jag-ui/src/elements/table.rs`
- Create: `jag/crates/jag-ui/src/elements/badge.rs`

**Key changes for Container:**
- Owns scroll state directly (extracted from `ir_renderer/state.rs` `ContainerScrollState`)
- Flex/Grid layout configured via Taffy styles
- Children are `Vec<Box<dyn Element>>`

**Step 1-4:** Extract, rename imports, write tests, build.

**Step 5: Commit**

```bash
git commit -m "feat: extract container and compound elements into jag-ui"
```

---

### Task 7: Extract widgets

**Files:**
- Create: `jag/crates/jag-ui/src/widgets/mod.rs`
- Create: `jag/crates/jag-ui/src/widgets/text_input.rs`
- Create: `jag/crates/jag-ui/src/widgets/tab_bar.rs`
- Create: `jag/crates/jag-ui/src/widgets/popup_menu.rs`
- Create: `jag/crates/jag-ui/src/widgets/splitter.rs`
- Create: `jag/crates/jag-ui/src/widgets/pane_tree.rs`
- Create: `jag/crates/jag-ui/src/widgets/tree_view.rs`

These are the easiest — widgets in `detir-scene/src/widgets/` have **zero IR coupling**.
They only depend on engine-core types.

**Step 1:** Copy each file, rename imports (`engine_core` → `jag_draw`, `detir_surface` → `jag_surface`).

**Step 2:** Build and test.

**Step 3: Commit**

```bash
git commit -m "feat: extract widgets into jag-ui"
```

---

### Task 8: Build the Ui coordinator

**Files:**
- Create: `jag/crates/jag-ui/src/ui.rs`
- Create: `jag/crates/jag-ui/src/hit_region.rs`
- Create: `jag/crates/jag-ui/src/layout.rs`

This is the main integration point — a `Ui` struct that coordinates layout, painting,
hit-testing, and event dispatch for a tree of elements.

**Step 1: Define HitRegionRegistry (standalone)**

Extract from `detir/crates/detir-scene/src/ir_renderer/hit_region.rs`.
Replace `ViewNodeId` → `FocusId`:

```rust
// jag/crates/jag-ui/src/hit_region.rs
use crate::focus::FocusId;

pub struct HitRegionRegistry {
    id_to_region: HashMap<FocusId, u32>,
    region_to_id: HashMap<u32, FocusId>,
    next_region_id: u32,
}

impl HitRegionRegistry {
    pub fn new() -> Self { ... }
    pub fn register(&mut self, id: FocusId) -> u32 { ... }
    pub fn lookup(&self, region_id: u32) -> Option<FocusId> { ... }
    pub fn clear(&mut self) { ... }
}
```

**Step 2: Define Layout wrapper**

```rust
// jag/crates/jag-ui/src/layout.rs
use taffy::prelude::*;

pub struct Layout {
    tree: TaffyTree<FocusId>,
}

impl Layout {
    pub fn new() -> Self { ... }
    pub fn add_node(&mut self, style: Style, id: FocusId) -> NodeId { ... }
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) { ... }
    pub fn compute(&mut self, root: NodeId, available: Size<AvailableSpace>) { ... }
    pub fn get_layout(&self, node: NodeId) -> &taffy::Layout { ... }
}
```

**Step 3: Define Ui coordinator**

```rust
// jag/crates/jag-ui/src/ui.rs
use jag_draw::{HitIndex, Painter};
use jag_surface::Canvas;
use crate::focus::FocusManager;
use crate::hit_region::HitRegionRegistry;
use crate::layout::Layout;
use crate::theme::Theme;
use crate::event::*;

pub struct Ui {
    pub focus: FocusManager,
    pub hit_registry: HitRegionRegistry,
    pub layout: Layout,
    pub theme: Theme,
}

impl Ui {
    pub fn new() -> Self { ... }
    pub fn with_theme(theme: Theme) -> Self { ... }
}
```

**Step 4: Write tests**

```rust
#[test]
fn ui_creation() {
    let ui = Ui::new();
    assert!(ui.focus.current().is_none());
}
```

**Step 5: Build and test**

```bash
cd jag && cargo test -p jag-ui
```

**Step 6: Commit**

```bash
git commit -m "feat: add Ui coordinator with layout, hit regions, focus"
```

---

### Task 9: Add demo example

**Files:**
- Create: `jag/examples/basic/Cargo.toml`
- Create: `jag/examples/basic/src/main.rs`
- Modify: `jag/Cargo.toml` (add to workspace members)

**Step 1: Create a minimal working example**

```rust
use jag_draw::*;
use jag_ui::*;
use winit::...;

fn main() {
    // Create window
    // Initialize GPU (jag_draw::GraphicsEngine)
    // Create text provider
    // Create UI with a button and text input
    // Run event loop:
    //   - Handle events via Ui
    //   - Layout via Ui
    //   - Paint via Painter
    //   - Render via PassManager
}
```

This proves the full pipeline works end-to-end without any detir dependencies.

**Step 2: Build and run**

```bash
cd jag && cargo run -p basic-example
```

**Step 3: Commit**

```bash
git commit -m "feat: add basic jag-ui example app"
```

---

### Task 10: Wire up jag meta-crate

**Files:**
- Modify: `jag/crates/jag/Cargo.toml`
- Modify: `jag/crates/jag/src/lib.rs`

**Step 1: Update Cargo.toml**

```toml
[package]
name = "jag"
version = "0.1.0"
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "GPU-accelerated 2D rendering and UI toolkit"
keywords = ["gpu", "rendering", "ui", "2d", "wgpu"]
categories = ["graphics", "gui", "rendering"]

[dependencies]
jag-draw = { path = "../jag-draw" }
jag-ui = { path = "../jag-ui" }
jag-surface = { path = "../jag-surface" }
```

**Step 2: Update lib.rs**

```rust
//! # Jag
//!
//! GPU-accelerated 2D rendering and UI toolkit.

pub use jag_draw as draw;
pub use jag_ui as ui;
pub use jag_surface as surface;
```

**Step 3: Build**

```bash
cd jag && cargo build -p jag
```

**Step 4: Commit**

```bash
git commit -m "feat: wire up jag meta-crate re-exporting draw + ui + surface"
```

---

## Phase 3: detir-scene bridge (internal)

### Task 11: Make detir-scene depend on jag crates

**Files:**
- Modify: `detir/crates/detir-scene/Cargo.toml`
- Modify: `detir/Cargo.toml` (workspace deps)

**Step 1: Add jag dependencies to detir workspace**

```toml
# detir/Cargo.toml [workspace.dependencies]
jag-draw = { path = "../../jag/crates/jag-draw" }
jag-ui = { path = "../../jag/crates/jag-ui" }
jag-surface = { path = "../../jag/crates/jag-surface" }
```

**Step 2: Add to detir-scene Cargo.toml**

```toml
jag-ui = { workspace = true }
```

At this stage, detir-scene still uses engine-core directly. We'll migrate
incrementally — element by element — in the next tasks.

**Step 3: Build**

```bash
cd detir && cargo build -p detir-scene
```

**Step 4: Commit**

```bash
git commit -m "feat: add jag crate dependencies to detir-scene"
```

---

### Task 12: Create IR bridge adapter

**Files:**
- Create: `detir/crates/detir-scene/src/jag_bridge.rs`

This module maps IR ViewNode specs → jag-ui element state. It replaces the
current `ir_adapter.rs` over time.

**Step 1: Implement bridge functions**

```rust
// detir/crates/detir-scene/src/jag_bridge.rs
use detir_ir::view::*;
use jag_ui::elements::*;
use jag_draw::ColorLinPremul;

pub fn button_from_view_node(node: &ViewNode, rect: jag_draw::Rect, label: &str) -> Button {
    // Map ButtonSpec fields → Button fields
    // Map ViewNode styles → Button colors
    // Map ViewNode click handler → callback
}

pub fn checkbox_from_view_node(node: &ViewNode, rect: jag_draw::Rect, label: &str) -> Checkbox {
    // Map CheckboxSpec fields → Checkbox fields
}

// ... one function per element type
```

**Step 2: Write test**

```rust
#[test]
fn bridge_creates_button_from_spec() {
    let spec = ButtonSpec { /* ... */ };
    let node = ViewNode::with_spec(spec);
    let btn = button_from_view_node(&node, Rect::new(0.0, 0.0, 100.0, 40.0), "Test");
    assert_eq!(btn.label, "Test");
}
```

**Step 3: Build and test**

```bash
cd detir && cargo test -p detir-scene -- jag_bridge
```

**Step 4: Commit**

```bash
git commit -m "feat: add jag-ui IR bridge adapter in detir-scene"
```

---

### Task 13: Migrate IrRenderer to use jag-ui elements

**Files:**
- Modify: `detir/crates/detir-scene/src/ir_renderer/state.rs`
- Modify: `detir/crates/detir-scene/src/ir_renderer/core.rs`
- Modify: `detir/crates/detir-scene/src/ir_renderer/runner.rs`

This is the highest-risk task. Migrate incrementally — one element type at a time.

#### Challenges Discovered During Migration

**1. Event system mismatch**
- detir-scene passes events **by value**: `handle_click(event: MouseClickEvent)`
- jag-ui passes events **by reference**: `handle_click(event: &MouseClickEvent)`
- detir-scene uses `detir_platform` event types; jag-ui has its own standalone types
- **Solution:** The `jag_bridge.rs` adapter must convert between the two event type systems.
  Each element migration needs a thin shim that converts `detir_platform::MouseClickEvent` →
  `jag_ui::MouseClickEvent` and passes by reference. Consider adding `From` impls in the bridge.

**2. IR integration depth varies by element**
- **Near-identical** (easiest): Radio, ToggleSwitch, Select — rendering logic is almost
  the same between detir-scene and jag-ui versions. These should be migrated first.
- **Minor visual differences**: Button, Checkbox — detir-scene versions have slight
  rendering differences (focus ring style, icon sizing) that need reconciliation.
- **NOT viable for direct replacement**: InputBox — detir-scene's version is ~2000 lines
  with deep IR integration (IME handling, selection rendering, caret management, scroll state,
  form validation, data binding). jag-ui's InputBox is a clean-room ~200 line implementation.
  These must coexist; detir-scene keeps its own InputBox.

**3. State ownership model**
- detir-scene stores element state in `IrElementState` keyed by `ViewNodeId`, with
  the IR renderer owning the lifecycle (create on first render, persist across frames).
- jag-ui elements are standalone structs with no external ID system.
- **Solution:** `IrElementState` can hold `jag_ui::Button` etc. directly, but the bridge
  must reconstruct/update them from ViewNode specs each frame or cache them with a
  dirty-check pattern.

**4. Hit-testing and focus routing**
- detir-scene uses `HitRegionRegistry` with `ViewNodeId` → `u32` mapping, tightly
  coupled to the IR scene graph.
- jag-ui uses `FocusId(u64)` with its own `HitRegionRegistry`.
- **Solution:** During migration, keep detir-scene's hit-test system and only use jag-ui
  elements for rendering + event handling. Don't try to unify hit-testing until all
  elements are migrated.

**5. Theme/color divergence**
- detir-scene has its own `Theme` in `theme.rs` with colors derived from the IR
  document's theme spec.
- jag-ui has a standalone `Theme` with hardcoded dark/light palettes.
- **Solution:** Bridge must map detir theme colors → jag-ui element color fields
  at construction time, not rely on jag-ui's Theme.

#### Recommended Migration Order

```
Phase A (low risk, high confidence):
  1. Radio         — near-identical, no IR coupling
  2. ToggleSwitch  — near-identical, no IR coupling
  3. Select        — near-identical, dropdown rendering matches

Phase B (moderate risk):
  4. Button        — reconcile focus ring and icon differences
  5. Checkbox      — reconcile check/indeterminate rendering

Phase C (high risk, defer):
  6. Label/Text    — trivial but touches many render paths
  7. Table         — complex but contained
  8. InputBox      — DO NOT MIGRATE (keep detir-scene's version)
  9. TextArea      — DO NOT MIGRATE (keep detir-scene's version)
```

**Step 1: Start with Radio**

Replace `crate::elements::Radio` usage in `ir_renderer/elements.rs` with
`jag_ui::elements::Radio`, using `jag_bridge` to convert ViewNode → Radio.

**Step 2: Update rendering code**

In `elements.rs`, update Radio rendering calls to use jag-ui's
`Radio::render()` method.

**Step 3: Add event shim**

```rust
// In jag_bridge.rs
pub fn convert_click_event(e: &detir_platform::MouseClickEvent) -> jag_ui::MouseClickEvent {
    jag_ui::MouseClickEvent {
        button: convert_mouse_button(e.button),
        state: convert_element_state(e.state),
        x: e.x,
        y: e.y,
        click_count: e.click_count,
    }
}
```

**Step 4: Run full test suite**

```bash
cd detir && cargo test
```

**Step 5: Visual verification**

```bash
cd detir && cargo run -p detir-scene
```

Verify radio buttons render and toggle correctly.

**Step 6: Repeat for each element in migration order**

After each element, run tests + visual verification before moving to the next.

**Step 7: Commit after each element migration**

```bash
git commit -m "refactor: migrate Radio to jag-ui in IrRenderer"
git commit -m "refactor: migrate ToggleSwitch to jag-ui in IrRenderer"
# ... etc
```

---

### Task 14: Remove duplicate element code from detir-scene

**Files:**
- Delete: migrated element files from `detir/crates/detir-scene/src/elements/`
- Modify: `detir/crates/detir-scene/src/elements/mod.rs` (re-export from jag-ui)

#### What stays in detir-scene (NOT removed)

These elements have deep IR/platform integration and must NOT be replaced:
- `input_box.rs` — IME, selection, caret, form validation, data binding (~2000 lines)
- `text_area.rs` — multiline editing with scroll state
- `video.rs`, `camera.rs`, `microphone.rs` — platform media backends
- `webview.rs` — CEF/WKWebView integration
- `node_graph.rs` — IDE-specific
- `rte.rs` — rich text editor
- `file_input.rs` — OS file dialog integration
- `multiline_text.rs` — IR-specific text layout with ViewNode styles

#### What gets replaced (re-exported from jag-ui)

Only elements that were successfully migrated in Task 13:
```rust
// detir/crates/detir-scene/src/elements/mod.rs
// Re-export migrated elements from jag-ui
pub use jag_ui::elements::{Radio, ToggleSwitch, Select};
// Phase B (if completed):
// pub use jag_ui::elements::{Button, Checkbox};

// Keep detir-specific elements
pub mod input_box;
pub mod text_area;
pub mod video;
pub mod camera;
pub mod webview;
// ... etc
```

**Step 1: Delete migrated element files one at a time**

Only delete after confirming the jag-ui version works identically in Task 13.

**Step 2: Run full test suite**

```bash
cd detir && cargo test
```

**Step 3: Visual verification**

```bash
cd detir && cargo run -p detir-scene
```

**Step 4: Commit**

```bash
git commit -m "refactor: remove duplicate elements, re-export from jag-ui"
```

---

### Task 15: Migrate remaining detir crates to jag-draw

**Files:**
- Modify: `detir/crates/detir-surface/Cargo.toml` (depend on jag-draw instead of engine-core)
- Modify: All crates that depend on engine-core

This task is **optional for initial release** — you can keep engine-core as an
internal alias to jag-draw, or do a full migration later. The important thing is
that jag-draw and jag-ui are publishable and self-contained.

**If doing full migration:**
1. In detir workspace Cargo.toml, add `engine-core = { path = "../../jag/crates/jag-draw", package = "jag-draw" }`
2. This makes `engine_core` an alias for `jag_draw` — zero code changes needed
3. Gradually rename imports in each crate

**Step 1: Alias approach**

```toml
# detir/Cargo.toml [workspace.dependencies]
engine-core = { path = "../../jag/crates/jag-draw", package = "jag-draw" }
```

**Step 2: Build**

```bash
cd detir && cargo build --workspace
```

**Step 3: Commit**

```bash
git commit -m "refactor: alias engine-core to jag-draw"
```

---

## Phase 4: Publish

### Task 16: Prepare crates for publishing

**Files:**
- Modify: All jag crate Cargo.toml files (version, metadata)
- Create: `jag/README.md`
- Create: `jag/LICENSE-MIT`, `jag/LICENSE-APACHE`

**Step 1: Set version to 0.1.0 across all crates**

**Step 2: Add README with usage examples**

**Step 3: Add license files**

**Step 4: Dry run publish**

```bash
cd jag/crates/jag-text && cargo publish --dry-run
cd jag/crates/jag-shaders && cargo publish --dry-run
cd jag/crates/jag-draw && cargo publish --dry-run
cd jag/crates/jag-surface && cargo publish --dry-run
cd jag/crates/jag-ui && cargo publish --dry-run
cd jag/crates/jag && cargo publish --dry-run
```

**Step 5: Publish (order matters — dependencies first)**

```bash
cargo publish -p jag-shaders
cargo publish -p jag-text
cargo publish -p jag-draw    # depends on jag-shaders, jag-text
cargo publish -p jag-surface # depends on jag-draw
cargo publish -p jag-ui      # depends on jag-draw, jag-surface, jag-text
cargo publish -p jag          # depends on jag-draw, jag-ui, jag-surface
```

**Step 6: Commit and tag**

```bash
git tag v0.1.0
git push origin main --tags
```

---

## Risk Checkpoints

After each phase, verify:

| Checkpoint | Command | Expected |
|-----------|---------|----------|
| Phase 1 complete | `cd jag && cargo test --workspace` | All jag tests pass |
| Phase 2 complete | `cd jag && cargo test --workspace` | All jag tests pass, example runs |
| Phase 3 task 13 (each element) | `cd detir && cargo test` | All detir tests pass |
| Phase 3 task 13 (visual) | `cargo run -p demo-app` | Renders correctly |
| Phase 3 complete | `cd detir && cargo test && cargo run -p demo-app` | Full regression pass |
| Phase 4 complete | Published on crates.io | `cargo add jag` works |

## v0.1.1 Roadmap

- **Full element showcase demo** — Interactive demo exercising all 19 elements and widgets
- **Taffy layout integration** — Automatic `Ui::layout_elements()` that wires Taffy computed layouts to element rects (no manual positioning)
- **Layout-driven demo** — Showcase using flex/grid layout instead of hardcoded coordinates

## Elements NOT extracted (stay in detir-scene)

These are Detir-specific and depend on platform integrations:
- `video.rs` — requires detir-media traits
- `audio.rs` — requires detir-media traits
- `camera.rs` — requires detir-media traits
- `microphone.rs` — requires detir-media traits
- `webview.rs` — requires detir-cef/detir-wkwebview
- `canvas3d` — requires engine-3d
- `node_graph.rs` — IDE-specific
- `rte.rs` — Rich text editor (consider extracting later if demand exists)
