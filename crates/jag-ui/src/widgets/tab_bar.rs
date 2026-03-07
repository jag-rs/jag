//! Horizontal tab bar widget.
//!
//! Displays a row of tabs with active indicator, dirty-dot, close button, and
//! scroll support.

use jag_draw::{Brush, Rect};
use jag_surface::Canvas;

use super::types::{WidgetColors, WidgetEvent};

// Layout constants
const TAB_HEIGHT: f32 = 36.0;
const TEXT_SIZE: f32 = 12.0;
const INDICATOR_HEIGHT: f32 = 2.0;
const CLOSE_SIZE: f32 = 18.0;
const CLOSE_PADDING: f32 = 6.0;
const TAB_PADDING_H: f32 = 12.0;
const MIN_TAB_WIDTH: f32 = 80.0;
const MAX_TAB_WIDTH: f32 = 200.0;

// Hit region ID ranges (local to jag-ui widgets).
const TAB_HIT_BASE: u32 = 7100;
const TAB_HIT_MAX: u32 = 7199;
const TAB_CLOSE_HIT_BASE: u32 = 7200;
const TAB_CLOSE_HIT_MAX: u32 = 7299;

/// A single tab in the bar.
#[derive(Debug, Clone)]
pub struct Tab {
    pub label: String,
    pub dirty: bool,
}

/// Horizontal tab bar widget.
#[derive(Debug, Clone)]
pub struct TabBar {
    pub tabs: Vec<Tab>,
    pub active_index: usize,
    pub rect: Rect,
    pub scroll_offset: f32,
    hovered_tab: Option<usize>,
    hovered_close: Option<usize>,
}

impl TabBar {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_index: 0,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: TAB_HEIGHT,
            },
            scroll_offset: 0.0,
            hovered_tab: None,
            hovered_close: None,
        }
    }

    pub fn height() -> f32 {
        TAB_HEIGHT
    }

    /// Add a tab at the end.
    pub fn push(&mut self, tab: Tab) {
        self.tabs.push(tab);
    }

    /// Remove a tab by index. Adjusts `active_index` to stay in bounds.
    pub fn remove(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.active_index = 0;
            } else if self.active_index >= self.tabs.len() {
                self.active_index = self.tabs.len() - 1;
            } else if self.active_index > index {
                self.active_index -= 1;
            }
        }
    }

    /// Set the active tab, clamping to valid range.
    pub fn set_active(&mut self, index: usize) {
        if !self.tabs.is_empty() {
            self.active_index = index.min(self.tabs.len() - 1);
        }
    }

    // -- Layout helpers --

    fn tab_width(&self) -> f32 {
        if self.tabs.is_empty() {
            return MIN_TAB_WIDTH;
        }
        let equal = self.rect.w / self.tabs.len() as f32;
        equal.clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH)
    }

    fn total_content_width(&self) -> f32 {
        self.tab_width() * self.tabs.len() as f32
    }

    fn tab_rect(&self, index: usize) -> Rect {
        let tw = self.tab_width();
        Rect {
            x: self.rect.x + index as f32 * tw - self.scroll_offset,
            y: self.rect.y,
            w: tw,
            h: TAB_HEIGHT,
        }
    }

    fn close_rect(&self, tab_rect: &Rect) -> Rect {
        Rect {
            x: tab_rect.x + tab_rect.w - CLOSE_SIZE - CLOSE_PADDING,
            y: tab_rect.y + (TAB_HEIGHT - CLOSE_SIZE) / 2.0,
            w: CLOSE_SIZE,
            h: CLOSE_SIZE,
        }
    }

    // -- Rendering --

    pub fn render(&self, canvas: &mut Canvas, colors: &WidgetColors, z: i32) {
        // Background
        canvas.fill_rect(
            self.rect.x,
            self.rect.y,
            self.rect.w,
            self.rect.h,
            Brush::Solid(colors.bg),
            z,
        );

        // Clip to the tab bar area.
        canvas.push_clip_rect(self.rect);

        for (i, tab) in self.tabs.iter().enumerate() {
            let tr = self.tab_rect(i);
            // Skip tabs fully outside the visible area.
            if tr.x + tr.w < self.rect.x || tr.x > self.rect.x + self.rect.w {
                continue;
            }

            let is_active = i == self.active_index;
            let is_hovered = self.hovered_tab == Some(i);

            // Tab background
            let bg = if is_active {
                colors.surface
            } else if is_hovered {
                colors.hover
            } else {
                colors.bg
            };
            canvas.fill_rect(tr.x, tr.y, tr.w, tr.h, Brush::Solid(bg), z);

            // Active indicator underline
            if is_active {
                canvas.fill_rect(
                    tr.x,
                    tr.y + TAB_HEIGHT - INDICATOR_HEIGHT,
                    tr.w,
                    INDICATOR_HEIGHT,
                    Brush::Solid(colors.accent),
                    z + 1,
                );
            }

            // Dirty dot (before label so label shifts right)
            let dirty_offset = if tab.dirty {
                let dot_x = tr.x + TAB_PADDING_H + 3.0;
                let dot_y = tr.y + TAB_HEIGHT / 2.0;
                canvas.circle([dot_x, dot_y], 3.0, Brush::Solid(colors.dirty_dot), z + 1);
                10.0
            } else {
                0.0
            };

            // Label
            let label_x = tr.x + TAB_PADDING_H + dirty_offset;
            let label_y = tr.y + TAB_HEIGHT / 2.0 + TEXT_SIZE * 0.35;
            let label_color = if is_active {
                colors.text
            } else {
                colors.text_muted
            };
            canvas.draw_text_run(
                [label_x, label_y],
                tab.label.clone(),
                TEXT_SIZE,
                label_color,
                z + 1,
            );

            // Close button (only when hovered or active)
            if is_active || is_hovered {
                let cr = self.close_rect(&tr);
                let close_hover = self.hovered_close == Some(i);
                if close_hover {
                    canvas.fill_rect(
                        cr.x - 2.0,
                        cr.y - 2.0,
                        cr.w + 4.0,
                        cr.h + 4.0,
                        Brush::Solid(colors.hover),
                        z + 1,
                    );
                }
                // Draw X (multiplication sign)
                let close_font = 18.0;
                let cx = cr.x + cr.w / 2.0;
                let cy = cr.y + cr.h / 2.0;
                canvas.draw_text_run_weighted(
                    [cx - close_font * 0.28, cy + close_font * 0.35],
                    "\u{00D7}".to_string(),
                    close_font,
                    700.0,
                    colors.close_icon,
                    z + 2,
                );

                // Hit region for close button
                let hit_id = TAB_CLOSE_HIT_BASE + i as u32;
                if hit_id <= TAB_CLOSE_HIT_MAX {
                    canvas.hit_region_rect(hit_id, cr, z + 2);
                }
            }

            // Hit region for the tab body
            let hit_id = TAB_HIT_BASE + i as u32;
            if hit_id <= TAB_HIT_MAX {
                canvas.hit_region_rect(hit_id, tr, z);
            }
        }

        // Bottom border
        canvas.fill_rect(
            self.rect.x,
            self.rect.y + TAB_HEIGHT - 1.0,
            self.rect.w,
            1.0,
            Brush::Solid(colors.border),
            z + 1,
        );

        canvas.pop_clip();
    }

    // -- Event handling --

    pub fn handle_click(&mut self, x: f32, y: f32) -> WidgetEvent {
        if !self.contains(x, y) {
            return WidgetEvent::Ignored;
        }

        for i in 0..self.tabs.len() {
            let tr = self.tab_rect(i);
            let cr = self.close_rect(&tr);

            // Check close button first (it's on top).
            if x >= cr.x && x < cr.x + cr.w && y >= cr.y && y < cr.y + cr.h {
                return WidgetEvent::TabClose { index: i };
            }

            if x >= tr.x && x < tr.x + tr.w && y >= tr.y && y < tr.y + tr.h {
                self.active_index = i;
                return WidgetEvent::TabSelected { index: i };
            }
        }
        WidgetEvent::Consumed
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) -> WidgetEvent {
        if !self.contains(x, y) {
            let changed = self.hovered_tab.is_some() || self.hovered_close.is_some();
            self.hovered_tab = None;
            self.hovered_close = None;
            return if changed {
                WidgetEvent::Consumed
            } else {
                WidgetEvent::Ignored
            };
        }

        let mut new_tab = None;
        let mut new_close = None;

        for i in 0..self.tabs.len() {
            let tr = self.tab_rect(i);
            if x >= tr.x && x < tr.x + tr.w && y >= tr.y && y < tr.y + tr.h {
                new_tab = Some(i);
                let cr = self.close_rect(&tr);
                if x >= cr.x && x < cr.x + cr.w && y >= cr.y && y < cr.y + cr.h {
                    new_close = Some(i);
                }
                break;
            }
        }

        let changed = new_tab != self.hovered_tab || new_close != self.hovered_close;
        self.hovered_tab = new_tab;
        self.hovered_close = new_close;

        if changed {
            WidgetEvent::Consumed
        } else {
            WidgetEvent::Ignored
        }
    }

    pub fn handle_scroll(&mut self, delta_x: f32) -> WidgetEvent {
        let max_scroll = (self.total_content_width() - self.rect.w).max(0.0);
        let old = self.scroll_offset;
        self.scroll_offset = (self.scroll_offset - delta_x).clamp(0.0, max_scroll);
        if (self.scroll_offset - old).abs() > 0.01 {
            WidgetEvent::Consumed
        } else {
            WidgetEvent::Ignored
        }
    }

    /// Returns the tab index at the given point, if any.
    pub fn tab_index_at(&self, x: f32, y: f32) -> Option<usize> {
        if !self.contains(x, y) {
            return None;
        }
        for i in 0..self.tabs.len() {
            let tr = self.tab_rect(i);
            if x >= tr.x && x < tr.x + tr.w && y >= tr.y && y < tr.y + tr.h {
                return Some(i);
            }
        }
        None
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x < self.rect.x + self.rect.w
            && y >= self.rect.y
            && y < self.rect.y + self.rect.h
    }
}

impl Default for TabBar {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bar(n: usize, width: f32) -> TabBar {
        let mut bar = TabBar::new();
        bar.rect.w = width;
        for i in 0..n {
            bar.push(Tab {
                label: format!("Tab {i}"),
                dirty: false,
            });
        }
        bar
    }

    #[test]
    fn tab_width_equal() {
        let bar = make_bar(4, 800.0);
        // 800 / 4 = 200, clamped to MAX_TAB_WIDTH = 200
        assert!((bar.tab_width() - 200.0).abs() < 0.01);
    }

    #[test]
    fn tab_width_clamped_min() {
        let bar = make_bar(20, 800.0);
        // 800 / 20 = 40 < MIN_TAB_WIDTH = 80
        assert!((bar.tab_width() - MIN_TAB_WIDTH).abs() < 0.01);
    }

    #[test]
    fn remove_adjusts_active() {
        let mut bar = make_bar(3, 600.0);
        bar.active_index = 2;
        bar.remove(2);
        assert_eq!(bar.active_index, 1);
    }

    #[test]
    fn remove_keeps_active_when_before() {
        let mut bar = make_bar(3, 600.0);
        bar.active_index = 2;
        bar.remove(0);
        // active was 2, removed index 0, so active becomes 1
        assert_eq!(bar.active_index, 1);
    }

    #[test]
    fn click_selects_tab() {
        let mut bar = make_bar(3, 600.0);
        // Tab width = 200, so tab 1 is at x=200..400
        let event = bar.handle_click(250.0, 18.0);
        assert_eq!(event, WidgetEvent::TabSelected { index: 1 });
        assert_eq!(bar.active_index, 1);
    }

    #[test]
    fn scroll_clamps() {
        let mut bar = make_bar(2, 400.0);
        // Total = 400, viewport = 400 -> max_scroll = 0
        let _event = bar.handle_scroll(100.0);
        assert_eq!(bar.scroll_offset, 0.0);
    }

    #[test]
    fn tab_index_at_returns_correct() {
        let bar = make_bar(3, 600.0);
        assert_eq!(bar.tab_index_at(50.0, 18.0), Some(0));
        assert_eq!(bar.tab_index_at(250.0, 18.0), Some(1));
        assert_eq!(bar.tab_index_at(450.0, 18.0), Some(2));
        assert_eq!(bar.tab_index_at(650.0, 18.0), None); // outside
    }
}
