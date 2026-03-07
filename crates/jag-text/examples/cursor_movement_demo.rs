use jag_text::font::FontFace;
use jag_text::layout::{Cursor, TextLayout, WrapMode};

fn main() {
    println!("=== Cursor Movement Demo ===\n");

    // Load a font
    let font_path = "../../fonts/Geist/static/Geist-Regular.ttf";
    let font = FontFace::from_path(font_path, 0).expect("Failed to load font");

    let font_size = 16.0;

    // Test 1: Character-by-character movement
    println!("Test 1: Character Movement");
    println!("---------------------------");
    let text = "Hello, 世界! 👨‍👩‍👧‍👦";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);
    println!(
        "Length: {} bytes, {} chars\n",
        text.len(),
        text.chars().count()
    );

    // Move right through the text
    let mut offset = 0;
    println!("Moving right by character:");
    while offset < text.len() {
        let next = layout.move_cursor_right(offset);
        let moved_text = &text[offset..next];
        println!(
            "  {} -> {}: \"{}\" ({} bytes)",
            offset,
            next,
            moved_text,
            next - offset
        );
        offset = next;
    }

    // Move left through the text
    println!("\nMoving left by character:");
    offset = text.len();
    while offset > 0 {
        let prev = layout.move_cursor_left(offset);
        let moved_text = &text[prev..offset];
        println!(
            "  {} -> {}: \"{}\" ({} bytes)",
            offset,
            prev,
            moved_text,
            offset - prev
        );
        offset = prev;
    }

    // Test 2: Word movement
    println!("\n\nTest 2: Word Movement");
    println!("---------------------");
    let text = "The quick brown fox jumps over the lazy dog.";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);

    // Move right by word
    let mut offset = 0;
    println!("\nMoving right by word:");
    while offset < text.len() {
        let next = layout.move_cursor_right_word(offset);
        if next == offset {
            break;
        }
        let word = &text[offset..next];
        println!("  {} -> {}: \"{}\"", offset, next, word);
        offset = next;
    }

    // Move left by word
    println!("\nMoving left by word:");
    offset = text.len();
    while offset > 0 {
        let prev = layout.move_cursor_left_word(offset);
        if prev == offset {
            break;
        }
        let word = &text[prev..offset];
        println!("  {} -> {}: \"{}\"", offset, prev, word);
        offset = prev;
    }

    // Test 3: Line movement with wrapping
    println!("\n\nTest 3: Line Movement (with wrapping)");
    println!("--------------------------------------");
    let text = "This is a long line of text that will wrap across multiple lines when we set a maximum width constraint.";
    let max_width = 200.0;
    let layout =
        TextLayout::with_wrap(text, &font, font_size, Some(max_width), WrapMode::BreakWord);

    println!("Text: \"{}\"", text);
    println!("Max width: {}", max_width);
    println!("Number of lines: {}\n", layout.lines().len());

    for (i, line) in layout.lines().iter().enumerate() {
        let line_text = &text[line.text_range.clone()];
        println!("Line {}: \"{}\"", i, line_text);
    }

    // Start at beginning and move down
    println!("\nMoving down from start:");
    let mut offset = 0;
    let mut preferred_x = None;
    for i in 0..layout.lines().len() {
        let (new_offset, x) = layout.move_cursor_down(offset, preferred_x);
        println!(
            "  Line {}: offset {} -> {} (x: {:.2})",
            i, offset, new_offset, x
        );
        offset = new_offset;
        preferred_x = Some(x);
    }

    // Move back up
    println!("\nMoving up from end:");
    preferred_x = None;
    for i in (0..layout.lines().len()).rev() {
        let (new_offset, x) = layout.move_cursor_up(offset, preferred_x);
        println!(
            "  Line {}: offset {} -> {} (x: {:.2})",
            i, offset, new_offset, x
        );
        offset = new_offset;
        preferred_x = Some(x);
    }

    // Test 4: Home/End movement
    println!("\n\nTest 4: Home/End Movement");
    println!("-------------------------");
    let text = "Line 1\nLine 2 is longer\nLine 3";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text:");
    for (i, line) in layout.lines().iter().enumerate() {
        let line_text = &text[line.text_range.clone()];
        println!("  Line {}: \"{}\"", i, line_text);
    }

    // Test Home/End on each line
    println!("\nHome/End on each line:");
    for (i, line) in layout.lines().iter().enumerate() {
        let mid_offset = (line.text_range.start + line.text_range.end) / 2;
        let start = layout.move_cursor_line_start(mid_offset);
        let end = layout.move_cursor_line_end(mid_offset);
        println!(
            "  Line {}: start={}, end={}, range={:?}",
            i, start, end, line.text_range
        );
    }

    // Test 5: Document start/end
    println!("\n\nTest 5: Document Start/End");
    println!("---------------------------");
    let doc_start = layout.move_cursor_document_start();
    let doc_end = layout.move_cursor_document_end();
    println!("Document start: {}", doc_start);
    println!("Document end: {}", doc_end);
    println!("Text length: {}", text.len());

    // Test 6: BiDi text movement
    println!("\n\nTest 6: BiDi Text Movement");
    println!("--------------------------");
    let text = "Hello مرحبا World";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);
    println!("(Contains Arabic RTL text)\n");

    // Character movement
    println!("Moving right by character:");
    let mut offset = 0;
    let mut count = 0;
    while offset < text.len() && count < 10 {
        let next = layout.move_cursor_right(offset);
        if next == offset {
            break;
        }
        let moved_text = &text[offset..next];
        println!("  {} -> {}: \"{}\"", offset, next, moved_text);
        offset = next;
        count += 1;
    }

    // Test 7: Cursor position tracking with movement
    println!("\n\nTest 7: Cursor with Movement");
    println!("-----------------------------");
    let text = "Test cursor movement";
    let layout = TextLayout::new(text, &font, font_size);
    let mut cursor = Cursor::new();

    println!("Text: \"{}\"", text);
    println!("Initial cursor position: {}\n", cursor.byte_offset());

    // Move right 5 times
    println!("Moving right 5 times:");
    for i in 0..5 {
        let new_offset = layout.move_cursor_right(cursor.byte_offset());
        cursor.set_byte_offset(new_offset);
        let char_at = &text[cursor.byte_offset()..].chars().next().unwrap_or(' ');
        println!(
            "  Step {}: offset={}, next char='{}'",
            i + 1,
            cursor.byte_offset(),
            char_at
        );
    }

    // Move left 2 times
    println!("\nMoving left 2 times:");
    for i in 0..2 {
        let new_offset = layout.move_cursor_left(cursor.byte_offset());
        cursor.set_byte_offset(new_offset);
        let char_at = &text[cursor.byte_offset()..].chars().next().unwrap_or(' ');
        println!(
            "  Step {}: offset={}, next char='{}'",
            i + 1,
            cursor.byte_offset(),
            char_at
        );
    }

    // Move to end
    println!("\nMoving to document end:");
    let end_offset = layout.move_cursor_document_end();
    cursor.set_byte_offset(end_offset);
    println!("  Cursor at: {}", cursor.byte_offset());

    // Move to start
    println!("\nMoving to document start:");
    let start_offset = layout.move_cursor_document_start();
    cursor.set_byte_offset(start_offset);
    println!("  Cursor at: {}", cursor.byte_offset());

    println!("\n=== Demo Complete ===");
}
