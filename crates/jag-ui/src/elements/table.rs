//! Simple table element with headers and data rows.

use jag_draw::{Brush, ColorLinPremul, Rect};
use jag_surface::Canvas;

use crate::event::{
    EventHandler, EventResult, KeyboardEvent, MouseClickEvent, MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A simple table displaying headers and rows of text cells with
/// grid lines and optional zebra striping.
pub struct Table {
    pub rect: Rect,
    /// Column headers.
    pub headers: Vec<String>,
    /// Data rows (each row is a vec of cell strings).
    pub rows: Vec<Vec<String>>,
    /// Header background color.
    pub header_bg: ColorLinPremul,
    /// Header text color.
    pub header_text_color: ColorLinPremul,
    /// Cell text color.
    pub cell_text_color: ColorLinPremul,
    /// Grid line color.
    pub grid_color: ColorLinPremul,
    /// Grid line width.
    pub grid_width: f32,
    /// Background color.
    pub bg: ColorLinPremul,
    /// Font size for headers.
    pub header_font_size: f32,
    /// Font size for cells.
    pub cell_font_size: f32,
    /// Row height in logical pixels.
    pub row_height: f32,
    /// Horizontal cell padding.
    pub cell_padding_x: f32,
    /// Enable alternating row colors.
    pub zebra_striping: bool,
    /// Alternate row color.
    pub zebra_color: ColorLinPremul,
}

impl Table {
    /// Create a table with headers and default styling.
    pub fn new(headers: Vec<String>) -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 600.0,
                h: 300.0,
            },
            headers,
            rows: Vec::new(),
            header_bg: ColorLinPremul::from_srgba_u8([248, 248, 248, 255]),
            header_text_color: ColorLinPremul::from_srgba_u8([60, 60, 60, 255]),
            cell_text_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            grid_color: ColorLinPremul::from_srgba_u8([224, 224, 224, 255]),
            grid_width: 1.0,
            bg: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            header_font_size: 14.0,
            cell_font_size: 14.0,
            row_height: 40.0,
            cell_padding_x: 12.0,
            zebra_striping: false,
            zebra_color: ColorLinPremul::from_srgba_u8([249, 249, 249, 255]),
        }
    }

    /// Set data rows.
    pub fn with_rows(mut self, rows: Vec<Vec<String>>) -> Self {
        self.rows = rows;
        self
    }

    /// Add a single row.
    pub fn add_row(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }

    /// Enable or disable zebra striping.
    pub fn with_zebra(mut self, enabled: bool) -> Self {
        self.zebra_striping = enabled;
        self
    }

    /// Calculate equal column widths.
    fn column_width(&self) -> f32 {
        let n = self.headers.len().max(1) as f32;
        self.rect.w / n
    }

    /// Hit-test.
    pub fn hit_test(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Table {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        let col_w = self.column_width();
        let mut y = self.rect.y;

        // Background
        canvas.fill_rect(
            self.rect.x,
            self.rect.y,
            self.rect.w,
            self.rect.h,
            Brush::Solid(self.bg),
            z,
        );

        // Header row background
        if !self.headers.is_empty() {
            canvas.fill_rect(
                self.rect.x,
                y,
                self.rect.w,
                self.row_height,
                Brush::Solid(self.header_bg),
                z + 1,
            );

            // Header text
            for (i, header) in self.headers.iter().enumerate() {
                let tx = self.rect.x + i as f32 * col_w + self.cell_padding_x;
                let ty = y + self.row_height * 0.5 + self.header_font_size * 0.35;
                canvas.draw_text_run_weighted(
                    [tx, ty],
                    header.clone(),
                    self.header_font_size,
                    600.0,
                    self.header_text_color,
                    z + 3,
                );
            }

            // Horizontal line under header
            canvas.fill_rect(
                self.rect.x,
                y + self.row_height,
                self.rect.w,
                self.grid_width,
                Brush::Solid(self.grid_color),
                z + 2,
            );

            y += self.row_height;
        }

        // Data rows
        for (row_idx, row) in self.rows.iter().enumerate() {
            if y > self.rect.y + self.rect.h {
                break;
            }

            // Zebra stripe
            if self.zebra_striping && row_idx % 2 == 1 {
                canvas.fill_rect(
                    self.rect.x,
                    y,
                    self.rect.w,
                    self.row_height,
                    Brush::Solid(self.zebra_color),
                    z + 1,
                );
            }

            // Cell text
            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx >= self.headers.len() {
                    break;
                }
                let tx = self.rect.x + col_idx as f32 * col_w + self.cell_padding_x;
                let ty = y + self.row_height * 0.5 + self.cell_font_size * 0.35;
                canvas.draw_text_run_weighted(
                    [tx, ty],
                    cell.clone(),
                    self.cell_font_size,
                    400.0,
                    self.cell_text_color,
                    z + 3,
                );
            }

            // Horizontal grid line
            if row_idx < self.rows.len() - 1 {
                canvas.fill_rect(
                    self.rect.x,
                    y + self.row_height,
                    self.rect.w,
                    self.grid_width,
                    Brush::Solid(self.grid_color),
                    z + 2,
                );
            }

            y += self.row_height;
        }

        // Vertical grid lines
        for i in 1..self.headers.len() {
            let lx = self.rect.x + i as f32 * col_w;
            canvas.fill_rect(
                lx,
                self.rect.y,
                self.grid_width,
                (y - self.rect.y).min(self.rect.h),
                Brush::Solid(self.grid_color),
                z + 2,
            );
        }

        // Outer border
        if self.grid_width > 0.0 {
            let used_h = (y - self.rect.y).min(self.rect.h);
            // Top
            canvas.fill_rect(
                self.rect.x,
                self.rect.y,
                self.rect.w,
                self.grid_width,
                Brush::Solid(self.grid_color),
                z + 4,
            );
            // Bottom
            canvas.fill_rect(
                self.rect.x,
                self.rect.y + used_h - self.grid_width,
                self.rect.w,
                self.grid_width,
                Brush::Solid(self.grid_color),
                z + 4,
            );
            // Left
            canvas.fill_rect(
                self.rect.x,
                self.rect.y,
                self.grid_width,
                used_h,
                Brush::Solid(self.grid_color),
                z + 4,
            );
            // Right
            canvas.fill_rect(
                self.rect.x + self.rect.w - self.grid_width,
                self.rect.y,
                self.grid_width,
                used_h,
                Brush::Solid(self.grid_color),
                z + 4,
            );
        }
    }

    fn focus_id(&self) -> Option<FocusId> {
        None
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait
// ---------------------------------------------------------------------------

impl EventHandler for Table {
    fn handle_mouse_click(&mut self, _event: &MouseClickEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_keyboard(&mut self, _event: &KeyboardEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_mouse_move(&mut self, _event: &MouseMoveEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_scroll(&mut self, _event: &ScrollEvent) -> EventResult {
        EventResult::Ignored
    }

    fn is_focused(&self) -> bool {
        false
    }

    fn set_focused(&mut self, _focused: bool) {}

    fn contains_point(&self, x: f32, y: f32) -> bool {
        self.hit_test(x, y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_new_defaults() {
        let t = Table::new(vec!["Name".into(), "Age".into()]);
        assert_eq!(t.headers.len(), 2);
        assert!(t.rows.is_empty());
        assert!(!t.zebra_striping);
    }

    #[test]
    fn table_add_rows() {
        let mut t = Table::new(vec!["A".into(), "B".into()]);
        t.add_row(vec!["1".into(), "2".into()]);
        t.add_row(vec!["3".into(), "4".into()]);
        assert_eq!(t.rows.len(), 2);
    }

    #[test]
    fn table_with_rows_builder() {
        let t = Table::new(vec!["X".into()]).with_rows(vec![vec!["a".into()], vec!["b".into()]]);
        assert_eq!(t.rows.len(), 2);
    }

    #[test]
    fn table_column_width() {
        let mut t = Table::new(vec!["A".into(), "B".into(), "C".into()]);
        t.rect.w = 300.0;
        let cw = t.column_width();
        assert!((cw - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn table_hit_test() {
        let t = Table::new(vec!["H".into()]);
        assert!(t.hit_test(300.0, 150.0));
        assert!(!t.hit_test(700.0, 0.0));
    }

    #[test]
    fn table_not_focusable() {
        let t = Table::new(vec!["H".into()]);
        assert!(t.focus_id().is_none());
    }
}
