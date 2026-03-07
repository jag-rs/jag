use jag_text::layout::TextLayout;
use jag_text::{Cursor, CursorPosition, FontCache};

fn main() {
    // Load a font
    let mut font_cache = FontCache::new();
    let font = font_cache
        .get_or_load("/System/Library/Fonts/Helvetica.ttc", 0)
        .expect("Failed to load font");

    // Create a text layout
    let text = "Hello, World!\nThis is a test of cursor management.";
    let layout = TextLayout::new(text, &font, 16.0);

    println!("Text: {:?}", text);
    println!("Number of lines: {}", layout.lines().len());
    println!();

    // Create a cursor at the start
    let mut cursor = layout.cursor_at_start();
    println!("Cursor at start:");
    println!("  Position: {} bytes", cursor.byte_offset());
    if let Some(rect) = layout.cursor_rect(&cursor) {
        println!(
            "  Rectangle: x={:.2}, y={:.2}, w={:.2}, h={:.2}",
            rect.x, rect.y, rect.width, rect.height
        );
    }
    println!();

    // Move cursor to position 7 (after "Hello, ")
    cursor.set_byte_offset(7);
    println!("Cursor at position 7:");
    println!("  Position: {} bytes", cursor.byte_offset());
    if let Some(rect) = layout.cursor_rect(&cursor) {
        println!(
            "  Rectangle: x={:.2}, y={:.2}, w={:.2}, h={:.2}",
            rect.x, rect.y, rect.width, rect.height
        );
    }
    println!();

    // Create a cursor at the end
    let cursor_end = layout.cursor_at_end();
    println!("Cursor at end:");
    println!("  Position: {} bytes", cursor_end.byte_offset());
    if let Some(rect) = layout.cursor_rect(&cursor_end) {
        println!(
            "  Rectangle: x={:.2}, y={:.2}, w={:.2}, h={:.2}",
            rect.x, rect.y, rect.width, rect.height
        );
    }
    println!();

    // Test cursor blinking
    println!("Testing cursor blink animation:");
    let mut blink_cursor = Cursor::new();
    for i in 0..5 {
        println!("  Frame {}: visible={}", i, blink_cursor.is_visible());
        blink_cursor.update_blink(0.5); // 0.5 second per frame
    }
    println!();

    // Test grapheme boundary snapping
    println!("Testing grapheme boundary snapping:");
    let emoji_text = "Hello 👨‍👩‍👧‍👦 World";
    let emoji_layout = TextLayout::new(emoji_text, &font, 16.0);

    // Try to position cursor in the middle of the emoji (should snap to boundary)
    let pos = CursorPosition::new(10);
    let snapped = emoji_layout.snap_cursor_to_boundary(pos);
    println!("  Original position: {} bytes", pos.byte_offset);
    println!("  Snapped position: {} bytes", snapped.byte_offset);
    println!(
        "  Text at position: {:?}",
        &emoji_text[..snapped.byte_offset.min(emoji_text.len())]
    );
}
