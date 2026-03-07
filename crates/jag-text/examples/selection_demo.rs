use jag_text::font::FontFace;
use jag_text::layout::{Selection, TextLayout, WrapMode};

fn main() {
    println!("=== Selection Management Demo ===\n");

    // Load a font
    let font_path = "../../fonts/Geist/static/Geist-Regular.ttf";
    let font = FontFace::from_path(font_path, 0).expect("Failed to load font");
    let font_size = 16.0;

    // Test 1: Basic Selection
    println!("Test 1: Basic Selection");
    println!("-----------------------");
    let text = "Hello, World!";
    let layout = TextLayout::new(text, &font, font_size);

    let sel = Selection::new(0, 5);
    println!("Text: \"{}\"", text);
    println!("Selection: {:?}", sel.range());
    println!("Selected text: \"{}\"", sel.text(text));
    println!("Is collapsed: {}", sel.is_collapsed());
    println!("Length: {} bytes\n", sel.len());

    // Test 2: Selection Direction
    println!("Test 2: Selection Direction");
    println!("---------------------------");
    let sel_forward = Selection::new(0, 5);
    let sel_backward = Selection::new(5, 0);

    println!("Forward selection (0 -> 5):");
    println!(
        "  Anchor: {}, Active: {}",
        sel_forward.anchor(),
        sel_forward.active()
    );
    println!("  Range: {:?}", sel_forward.range());
    println!("  Is forward: {}", sel_forward.is_forward());

    println!("\nBackward selection (5 -> 0):");
    println!(
        "  Anchor: {}, Active: {}",
        sel_backward.anchor(),
        sel_backward.active()
    );
    println!("  Range: {:?}", sel_backward.range());
    println!("  Is backward: {}\n", sel_backward.is_backward());

    // Test 3: Selection Extension
    println!("Test 3: Selection Extension");
    println!("---------------------------");
    let mut sel = Selection::new(5, 5); // Collapsed at position 5
    println!(
        "Initial (collapsed): anchor={}, active={}",
        sel.anchor(),
        sel.active()
    );

    sel.extend_to(12);
    println!(
        "After extend_to(12): anchor={}, active={}",
        sel.anchor(),
        sel.active()
    );
    println!("Selected text: \"{}\"", sel.text(text));

    sel.extend_to(3);
    println!(
        "After extend_to(3): anchor={}, active={}",
        sel.anchor(),
        sel.active()
    );
    println!("Selected text: \"{}\"", sel.text(text));
    println!("Is backward: {}\n", sel.is_backward());

    // Test 4: Word Selection
    println!("Test 4: Word Selection");
    println!("----------------------");
    let text = "The quick brown fox jumps over the lazy dog.";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);
    println!("\nDouble-click word selection:");

    // Simulate double-clicking at different positions
    let positions = [0, 4, 10, 16, 20, 30, 40];
    for &pos in &positions {
        let sel = layout.select_word_at(pos);
        let word = sel.text(text);
        println!(
            "  Position {}: \"{}\" (range: {:?})",
            pos,
            word,
            sel.range()
        );
    }

    // Test 5: Line Selection
    println!("\n\nTest 5: Line Selection");
    println!("----------------------");
    let text = "Line 1\nLine 2 is longer\nLine 3";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text:");
    for (i, line) in layout.lines().iter().enumerate() {
        let line_text = &text[line.text_range.clone()];
        println!("  Line {}: \"{}\"", i, line_text);
    }

    println!("\nTriple-click line selection:");
    let positions = [3, 10, 20];
    for &pos in &positions {
        let sel = layout.select_line_at(pos);
        let line_text = sel.text(text);
        println!(
            "  Position {}: \"{}\" (range: {:?})",
            pos,
            line_text,
            sel.range()
        );
    }

    // Test 6: Paragraph Selection
    println!("\n\nTest 6: Paragraph Selection");
    println!("---------------------------");
    let text = "First paragraph.\n\nSecond paragraph with more text.\n\nThird paragraph.";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);
    println!("\nParagraph selection:");

    let positions = [5, 20, 50];
    for &pos in &positions {
        let sel = layout.select_paragraph_at(pos);
        let para = sel.text(text);
        println!(
            "  Position {}: \"{}\" (range: {:?})",
            pos,
            para,
            sel.range()
        );
    }

    // Test 7: Selection Rectangles (Multi-line)
    println!("\n\nTest 7: Selection Rectangles");
    println!("-----------------------------");
    let text = "This is a long line of text that will wrap across multiple lines.";
    let max_width = 200.0;
    let layout =
        TextLayout::with_wrap(text, &font, font_size, Some(max_width), WrapMode::BreakWord);

    println!("Text: \"{}\"", text);
    println!("Max width: {}", max_width);
    println!("Number of lines: {}\n", layout.lines().len());

    // Select across multiple lines
    let sel = Selection::new(10, 50);
    let rects = layout.selection_rects(&sel);

    println!("Selection: {:?}", sel.range());
    println!("Selected text: \"{}\"", sel.text(text));
    println!("Number of selection rectangles: {}", rects.len());

    for (i, rect) in rects.iter().enumerate() {
        println!(
            "  Rect {}: x={:.2}, y={:.2}, width={:.2}, height={:.2}",
            i, rect.x, rect.y, rect.width, rect.height
        );
    }

    // Test 8: Selection Extension with Movement
    println!("\n\nTest 8: Selection Extension with Movement");
    println!("-----------------------------------------");
    let text = "The quick brown fox";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);

    // Start with collapsed selection
    let mut sel = Selection::collapsed(4); // At 'q' in "quick"
    println!("\nInitial position: {}", sel.active());

    // Extend right by character
    sel = layout.extend_selection(&sel, |offset| layout.move_cursor_right(offset));
    println!(
        "After Shift+Right: \"{}\" (range: {:?})",
        sel.text(text),
        sel.range()
    );

    // Extend right by word
    sel = layout.extend_selection(&sel, |offset| layout.move_cursor_right_word(offset));
    println!(
        "After Shift+Ctrl+Right: \"{}\" (range: {:?})",
        sel.text(text),
        sel.range()
    );

    // Extend to end of line
    sel = layout.extend_selection(&sel, |offset| layout.move_cursor_line_end(offset));
    println!(
        "After Shift+End: \"{}\" (range: {:?})",
        sel.text(text),
        sel.range()
    );

    // Test 9: Selection Collapse
    println!("\n\nTest 9: Selection Collapse");
    println!("--------------------------");
    let mut sel = Selection::new(5, 15);
    println!("Original selection: {:?}", sel.range());

    let mut sel_copy = sel;
    sel_copy.collapse_to_start();
    println!(
        "Collapse to start: anchor={}, active={}",
        sel_copy.anchor(),
        sel_copy.active()
    );

    let mut sel_copy = sel;
    sel_copy.collapse_to_end();
    println!(
        "Collapse to end: anchor={}, active={}",
        sel_copy.anchor(),
        sel_copy.active()
    );

    // Test 10: Multi-line Selection Extension
    println!("\n\nTest 10: Multi-line Selection Extension");
    println!("---------------------------------------");
    let text = "Line 1\nLine 2\nLine 3\nLine 4";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text:");
    for (i, line) in layout.lines().iter().enumerate() {
        let line_text = &text[line.text_range.clone()];
        println!("  Line {}: \"{}\"", i, line_text);
    }

    // Start selection on line 1
    let mut sel = Selection::collapsed(3); // Middle of "Line 1"
    println!("\nStarting at position: {}", sel.active());

    // Extend down
    let (new_sel, x) = layout.extend_selection_vertical(
        &sel,
        |offset, x| layout.move_cursor_down(offset, x),
        None,
    );
    sel = new_sel;
    println!(
        "After Shift+Down: \"{}\"",
        sel.text(text).replace('\n', "\\n")
    );

    // Extend down again
    let (new_sel, _) = layout.extend_selection_vertical(
        &sel,
        |offset, _| layout.move_cursor_down(offset, Some(x)),
        Some(x),
    );
    sel = new_sel;
    println!(
        "After Shift+Down again: \"{}\"",
        sel.text(text).replace('\n', "\\n")
    );

    // Test 11: BiDi Selection
    println!("\n\nTest 11: BiDi Text Selection");
    println!("----------------------------");
    let text = "Hello مرحبا World";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);
    println!("(Contains Arabic RTL text)\n");

    let sel = Selection::new(0, 10);
    println!("Selection (0..10): \"{}\"", sel.text(text));

    let rects = layout.selection_rects(&sel);
    println!("Number of selection rectangles: {}", rects.len());

    // Test 12: Selection with Emoji
    println!("\n\nTest 12: Selection with Emoji");
    println!("-----------------------------");
    let text = "Hello 👨‍👩‍👧‍👦 World";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);
    println!("Text length: {} bytes\n", text.len());

    // Select the emoji
    let emoji_start = 6;
    let emoji_end = text.find(" World").unwrap();
    let sel = Selection::new(emoji_start, emoji_end);

    println!("Emoji selection: \"{}\"", sel.text(text));
    println!("Range: {:?}", sel.range());
    println!("Length: {} bytes", sel.len());

    // Test 13: Selection Snapping
    println!("\n\nTest 13: Selection Boundary Snapping");
    println!("------------------------------------");
    let text = "Hello 世界";
    let layout = TextLayout::new(text, &font, font_size);

    println!("Text: \"{}\"", text);

    // Try to create selection in the middle of a multi-byte character
    let sel = Selection::new(0, 8); // 8 is in the middle of "世" (bytes 7-10)
    println!("Original selection: {:?}", sel.range());

    let snapped = layout.snap_selection_to_boundaries(&sel);
    println!("Snapped selection: {:?}", snapped.range());
    println!("Selected text: \"{}\"", snapped.text(text));

    println!("\n=== Demo Complete ===");
}
