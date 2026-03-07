//! Popup/context menu and command-palette style dropdown widget.

use jag_draw::{Brush, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use super::text_input::TextInput;
use super::types::{WidgetColors, WidgetEvent};
use crate::KeyCode;

// Layout constants
const ITEM_HEIGHT: f32 = 28.0;
const SEPARATOR_HEIGHT: f32 = 9.0;
const PADDING_H: f32 = 12.0;
const RADIUS: f32 = 6.0;
const MAX_VISIBLE: usize = 12;
const MIN_WIDTH: f32 = 160.0;
const MAX_WIDTH: f32 = 500.0;
const TEXT_SIZE: f32 = 13.0;
const SHORTCUT_TEXT_SIZE: f32 = 11.0;
const SHADOW_OFFSET: f32 = 4.0;
const INPUT_HEIGHT: f32 = 42.0;
const INPUT_PADDING: f32 = 6.0;

// Hit region ID range.
const POPUP_ITEM_HIT_BASE: u32 = 7600;
const POPUP_ITEM_HIT_MAX: u32 = 7699;

/// A single item in a popup menu.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    pub shortcut: Option<String>,
    pub separator: bool,
    pub disabled: bool,
}

impl MenuItem {
    pub fn action(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
            separator: false,
            disabled: false,
        }
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    pub fn separator() -> Self {
        Self {
            label: String::new(),
            shortcut: None,
            separator: true,
            disabled: true,
        }
    }
}

/// A popup/context menu widget.
#[derive(Debug, Clone)]
pub struct PopupMenu {
    pub items: Vec<MenuItem>,
    /// Anchor point in scene coordinates (where the menu was triggered).
    pub anchor: [f32; 2],
    /// Viewport bounds for flip logic.
    pub viewport: Rect,
    /// Computed bounding rect of the menu.
    pub rect: Rect,
    /// Currently highlighted item index (in filtered list).
    pub highlighted_index: Option<usize>,
    /// Filter input for type-ahead search.
    filter_input: TextInput,
    /// Indices into `items` that match the current filter.
    pub filtered_indices: Vec<usize>,
    /// Vertical scroll offset for long menus.
    pub scroll_offset: f32,
    /// Whether the popup is open.
    pub visible: bool,
    /// Whether to show a filter text input at the top.
    pub show_filter_input: bool,
    /// Override width (bypasses auto-sizing from labels).
    pub fixed_width: Option<f32>,
}

impl PopupMenu {
    pub fn new(items: Vec<MenuItem>) -> Self {
        let count = items.len();
        let filtered: Vec<usize> = (0..count).collect();
        let mut menu = Self {
            items,
            anchor: [0.0, 0.0],
            viewport: Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            highlighted_index: None,
            filter_input: TextInput::new("Type to filter...", TEXT_SIZE),
            filtered_indices: filtered,
            scroll_offset: 0.0,
            visible: false,
            show_filter_input: false,
            fixed_width: None,
        };
        menu.recompute_rect();
        menu
    }

    /// Enable the filter text input field at the top of the popup.
    pub fn with_filter_input(mut self) -> Self {
        self.show_filter_input = true;
        self.recompute_rect();
        self
    }

    /// Set a fixed width (bypasses auto-sizing from label text).
    pub fn with_fixed_width(mut self, w: f32) -> Self {
        self.fixed_width = Some(w);
        self.recompute_rect();
        self
    }

    /// Show the menu at the given anchor point within the viewport.
    pub fn show(&mut self, anchor: [f32; 2], viewport: Rect) {
        self.anchor = anchor;
        self.viewport = viewport;
        self.visible = true;
        self.highlighted_index = None;
        self.filter_input.clear();
        self.filtered_indices = (0..self.items.len()).collect();
        self.scroll_offset = 0.0;
        self.recompute_rect();
    }

    /// Reposition the menu without resetting filter/selection state.
    pub fn show_at(&mut self, anchor: [f32; 2], viewport: Rect) {
        self.anchor = anchor;
        self.viewport = viewport;
        self.recompute_rect();
    }

    /// Dismiss the menu.
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.filter_input.clear();
    }

    // -- Layout --

    fn menu_width(&self, canvas: Option<&Canvas>) -> f32 {
        if let Some(w) = self.fixed_width {
            return w;
        }
        let mut max_w: f32 = MIN_WIDTH;
        for item in &self.items {
            if item.separator {
                continue;
            }
            let label_w = if let Some(c) = canvas {
                c.measure_text_width(&item.label, TEXT_SIZE)
            } else {
                item.label.len() as f32 * TEXT_SIZE * 0.55
            };
            let shortcut_w = item
                .shortcut
                .as_ref()
                .map(|s| {
                    if let Some(c) = canvas {
                        c.measure_text_width(s, SHORTCUT_TEXT_SIZE) + 24.0
                    } else {
                        s.len() as f32 * SHORTCUT_TEXT_SIZE * 0.55 + 24.0
                    }
                })
                .unwrap_or(0.0);
            max_w = max_w.max(label_w + shortcut_w + PADDING_H * 2.0);
        }
        max_w.min(MAX_WIDTH)
    }

    fn input_height(&self) -> f32 {
        if self.show_filter_input {
            INPUT_HEIGHT
        } else {
            0.0
        }
    }

    fn recompute_rect(&mut self) {
        let w = self.menu_width(None);
        let visible_items = self.filtered_indices.len().min(MAX_VISIBLE);
        let h = self.visible_content_height(visible_items) + self.input_height();

        // Placement: below-right of anchor, flip if needed.
        let mut x = self.anchor[0];
        let mut y = self.anchor[1];

        // Flip up if near bottom.
        if y + h > self.viewport.y + self.viewport.h {
            y = (self.anchor[1] - h).max(self.viewport.y);
        }
        // Flip left if near right edge.
        if x + w > self.viewport.x + self.viewport.w {
            x = (self.anchor[0] - w).max(self.viewport.x);
        }

        self.rect = Rect { x, y, w, h };
    }

    fn visible_content_height(&self, visible_count: usize) -> f32 {
        self.filtered_indices
            .iter()
            .take(visible_count)
            .map(|&i| {
                if self.items[i].separator {
                    SEPARATOR_HEIGHT
                } else {
                    ITEM_HEIGHT
                }
            })
            .sum()
    }

    // -- Filter --

    fn apply_filter(&mut self) {
        if self.filter_input.is_empty() {
            self.filtered_indices = (0..self.items.len()).collect();
        } else {
            let needle = self.filter_input.text().to_lowercase();
            self.filtered_indices = (0..self.items.len())
                .filter(|&i| {
                    !self.items[i].separator && self.items[i].label.to_lowercase().contains(&needle)
                })
                .collect();
        }
        self.highlighted_index = if self.filtered_indices.is_empty() {
            None
        } else {
            Some(0)
        };
        self.scroll_offset = 0.0;
        self.recompute_rect();
    }

    // -- Blink --

    /// Advance the caret blink timer.
    pub fn update_blink(&mut self, dt: f32) {
        if self.visible && self.show_filter_input {
            self.filter_input.update_blink(dt);
        }
    }

    // -- Rendering --

    pub fn render(&self, canvas: &mut Canvas, colors: &WidgetColors, z: i32) {
        if !self.visible {
            return;
        }

        // Shadow
        canvas.fill_rect(
            self.rect.x + SHADOW_OFFSET,
            self.rect.y + SHADOW_OFFSET,
            self.rect.w,
            self.rect.h,
            Brush::Solid(colors.shadow),
            z,
        );

        // Background
        let rrect = RoundedRect {
            rect: self.rect,
            radii: RoundedRadii {
                tl: RADIUS,
                tr: RADIUS,
                br: RADIUS,
                bl: RADIUS,
            },
        };
        canvas.rounded_rect(rrect, Brush::Solid(colors.surface), z + 1);
        canvas.stroke_rounded_rect(rrect, 1.0, Brush::Solid(colors.border), z + 2);

        canvas.push_clip_rect(self.rect);

        // Filter input field
        if self.show_filter_input {
            let input_x = self.rect.x + INPUT_PADDING;
            let input_y = self.rect.y + INPUT_PADDING;
            let input_w = self.rect.w - INPUT_PADDING * 2.0;
            let input_h = INPUT_HEIGHT - INPUT_PADDING * 2.0;

            self.filter_input.render(
                canvas,
                input_x,
                input_y,
                input_w,
                input_h,
                colors.bg,
                Some(colors.accent),
                true,
                4.0,
                None,
                colors,
                z + 2,
            );

            // Separator below input
            canvas.fill_rect(
                self.rect.x,
                self.rect.y + INPUT_HEIGHT - 1.0,
                self.rect.w,
                1.0,
                Brush::Solid(colors.border),
                z + 2,
            );
        }

        let mut y = self.rect.y + self.input_height() - self.scroll_offset;
        for (fi, &item_idx) in self.filtered_indices.iter().enumerate() {
            let item = &self.items[item_idx];

            if item.separator {
                let sep_y = y + SEPARATOR_HEIGHT / 2.0;
                canvas.fill_rect(
                    self.rect.x + PADDING_H,
                    sep_y,
                    self.rect.w - PADDING_H * 2.0,
                    1.0,
                    Brush::Solid(colors.border),
                    z + 2,
                );
                y += SEPARATOR_HEIGHT;
                continue;
            }

            let row_rect = Rect {
                x: self.rect.x,
                y,
                w: self.rect.w,
                h: ITEM_HEIGHT,
            };

            // Highlight
            if self.highlighted_index == Some(fi) && !item.disabled {
                canvas.fill_rect(
                    row_rect.x + 4.0,
                    row_rect.y + 2.0,
                    row_rect.w - 8.0,
                    row_rect.h - 4.0,
                    Brush::Solid(colors.accent),
                    z + 2,
                );
            }

            // Label
            let text_color = if item.disabled {
                colors.text_muted
            } else {
                colors.text
            };
            let label_y = y + ITEM_HEIGHT / 2.0 + TEXT_SIZE * 0.35;
            canvas.draw_text_run(
                [self.rect.x + PADDING_H, label_y],
                item.label.clone(),
                TEXT_SIZE,
                text_color,
                z + 3,
            );

            // Shortcut
            if let Some(ref shortcut) = item.shortcut {
                let sw = canvas.measure_text_width(shortcut, SHORTCUT_TEXT_SIZE);
                let sx = self.rect.x + self.rect.w - PADDING_H - sw;
                canvas.draw_text_run(
                    [sx, label_y],
                    shortcut.clone(),
                    SHORTCUT_TEXT_SIZE,
                    colors.text_muted,
                    z + 3,
                );
            }

            // Hit region
            let hit_id = POPUP_ITEM_HIT_BASE + fi as u32;
            if hit_id <= POPUP_ITEM_HIT_MAX {
                canvas.hit_region_rect(hit_id, row_rect, z + 2);
            }

            y += ITEM_HEIGHT;
        }

        canvas.pop_clip();
    }

    // -- Event handling --

    pub fn handle_click(&mut self, x: f32, y: f32) -> WidgetEvent {
        if !self.visible {
            return WidgetEvent::Ignored;
        }

        // Click outside -> dismiss
        if !self.contains(x, y) {
            self.dismiss();
            return WidgetEvent::PopupDismissed;
        }

        // Click inside filter input area -- consume but don't select
        if self.show_filter_input && y < self.rect.y + self.input_height() {
            return WidgetEvent::Consumed;
        }

        // Find which item was clicked
        let mut cy = self.rect.y + self.input_height() - self.scroll_offset;
        for &item_idx in &self.filtered_indices {
            let item = &self.items[item_idx];
            let h = if item.separator {
                SEPARATOR_HEIGHT
            } else {
                ITEM_HEIGHT
            };
            if y >= cy && y < cy + h && !item.separator && !item.disabled {
                self.dismiss();
                return WidgetEvent::PopupItemSelected { index: item_idx };
            }
            cy += h;
        }

        WidgetEvent::Consumed
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) -> WidgetEvent {
        if !self.visible {
            return WidgetEvent::Ignored;
        }
        if !self.contains(x, y) {
            if self.highlighted_index.is_some() {
                self.highlighted_index = None;
                return WidgetEvent::Consumed;
            }
            return WidgetEvent::Ignored;
        }

        // Inside filter input area -- no item highlight change
        if self.show_filter_input && y < self.rect.y + self.input_height() {
            return WidgetEvent::Consumed;
        }

        let mut cy = self.rect.y + self.input_height() - self.scroll_offset;
        for (fi, &item_idx) in self.filtered_indices.iter().enumerate() {
            let item = &self.items[item_idx];
            let h = if item.separator {
                SEPARATOR_HEIGHT
            } else {
                ITEM_HEIGHT
            };
            if y >= cy && y < cy + h && !item.separator && !item.disabled {
                if self.highlighted_index != Some(fi) {
                    self.highlighted_index = Some(fi);
                    return WidgetEvent::Consumed;
                }
                return WidgetEvent::Ignored;
            }
            cy += h;
        }
        WidgetEvent::Ignored
    }

    pub fn handle_key(&mut self, key: KeyCode) -> WidgetEvent {
        if !self.visible {
            return WidgetEvent::Ignored;
        }

        let actionable: Vec<usize> = self
            .filtered_indices
            .iter()
            .enumerate()
            .filter(|&(_, &i)| !self.items[i].separator && !self.items[i].disabled)
            .map(|(fi, _)| fi)
            .collect();

        if actionable.is_empty() {
            if key == KeyCode::Escape {
                self.dismiss();
                return WidgetEvent::PopupDismissed;
            }
            return WidgetEvent::Consumed;
        }

        match key {
            KeyCode::ArrowDown => {
                let current = self.highlighted_index.unwrap_or(usize::MAX);
                let next = actionable
                    .iter()
                    .find(|&&fi| fi > current)
                    .copied()
                    .unwrap_or(actionable[0]);
                self.highlighted_index = Some(next);
                WidgetEvent::Consumed
            }
            KeyCode::ArrowUp => {
                let current = self.highlighted_index.unwrap_or(0);
                let prev = actionable
                    .iter()
                    .rev()
                    .find(|&&fi| fi < current)
                    .copied()
                    .unwrap_or(*actionable.last().unwrap());
                self.highlighted_index = Some(prev);
                WidgetEvent::Consumed
            }
            KeyCode::Enter => {
                if let Some(fi) = self.highlighted_index
                    && let Some(&item_idx) = self.filtered_indices.get(fi)
                    && !self.items[item_idx].disabled
                {
                    self.dismiss();
                    return WidgetEvent::PopupItemSelected { index: item_idx };
                }
                WidgetEvent::Consumed
            }
            KeyCode::Escape => {
                self.dismiss();
                WidgetEvent::PopupDismissed
            }
            _ => WidgetEvent::Ignored,
        }
    }

    pub fn handle_text_input(&mut self, text: &str) -> WidgetEvent {
        if !self.visible {
            return WidgetEvent::Ignored;
        }
        self.filter_input.push_str(text);
        self.apply_filter();
        WidgetEvent::Consumed
    }

    pub fn handle_backspace(&mut self) -> WidgetEvent {
        if !self.visible || self.filter_input.is_empty() {
            return WidgetEvent::Ignored;
        }
        self.filter_input.pop_char();
        self.apply_filter();
        WidgetEvent::Consumed
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x < self.rect.x + self.rect.w
            && y >= self.rect.y
            && y < self.rect.y + self.rect.h
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_items() -> Vec<MenuItem> {
        vec![
            MenuItem::action("New File"),
            MenuItem::action("Open File").with_shortcut("Cmd+O"),
            MenuItem::separator(),
            MenuItem::action("Save").with_shortcut("Cmd+S"),
            MenuItem::action("Disabled").disabled(),
        ]
    }

    #[test]
    fn placement_below_right() {
        let mut menu = PopupMenu::new(sample_items());
        menu.show(
            [100.0, 100.0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );
        assert!(menu.rect.x >= 100.0);
        assert!(menu.rect.y >= 100.0);
    }

    #[test]
    fn placement_flips_up() {
        let mut menu = PopupMenu::new(sample_items());
        menu.show(
            [100.0, 580.0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );
        // Should flip up since 580 + content_height > 600
        assert!(menu.rect.y < 580.0);
    }

    #[test]
    fn placement_flips_left() {
        let mut menu = PopupMenu::new(sample_items());
        menu.show(
            [780.0, 100.0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );
        // Should flip left since 780 + width > 800
        assert!(menu.rect.x < 780.0);
    }

    #[test]
    fn filter_narrows_items() {
        let mut menu = PopupMenu::new(sample_items());
        menu.show(
            [0.0, 0.0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );
        assert_eq!(menu.filtered_indices.len(), 5);

        menu.handle_text_input("save");
        // Only "Save" should match (separator is excluded from filter)
        assert_eq!(menu.filtered_indices.len(), 1);
        assert_eq!(menu.items[menu.filtered_indices[0]].label, "Save");
    }

    #[test]
    fn keyboard_navigation() {
        let mut menu = PopupMenu::new(sample_items());
        menu.show(
            [0.0, 0.0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );

        menu.handle_key(KeyCode::ArrowDown);
        assert_eq!(menu.highlighted_index, Some(0));

        menu.handle_key(KeyCode::ArrowDown);
        assert_eq!(menu.highlighted_index, Some(1));

        // Skip separator (index 2) and disabled (index 4)
        menu.handle_key(KeyCode::ArrowDown);
        assert_eq!(menu.highlighted_index, Some(3)); // "Save"
    }

    #[test]
    fn escape_dismisses() {
        let mut menu = PopupMenu::new(sample_items());
        menu.show(
            [0.0, 0.0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );
        assert!(menu.visible);

        let ev = menu.handle_key(KeyCode::Escape);
        assert_eq!(ev, WidgetEvent::PopupDismissed);
        assert!(!menu.visible);
    }

    #[test]
    fn click_outside_dismisses() {
        let mut menu = PopupMenu::new(sample_items());
        menu.show(
            [100.0, 100.0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );

        let ev = menu.handle_click(0.0, 0.0);
        assert_eq!(ev, WidgetEvent::PopupDismissed);
        assert!(!menu.visible);
    }

    #[test]
    fn backspace_removes_filter_char() {
        let mut menu = PopupMenu::new(sample_items());
        menu.show(
            [0.0, 0.0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );
        menu.handle_text_input("sav");
        assert_eq!(menu.filtered_indices.len(), 1);

        menu.handle_backspace();
        // "sa" matches "Save" and "Disabled" (di-sa-bled)
        assert_eq!(menu.filtered_indices.len(), 2);

        menu.handle_backspace();
        menu.handle_backspace();
        // empty filter -> all items
        assert_eq!(menu.filtered_indices.len(), 5);
    }
}
