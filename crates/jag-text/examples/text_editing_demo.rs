/// Demonstration of text insertion and deletion functionality.
///
/// This example shows how to use the TextLayout API for basic text editing
/// operations including insertion, deletion, and selection replacement.
use jag_text::font::FontFace;
use jag_text::layout::{Selection, TextLayout, WrapMode};

fn main() {
    // Load a font
    let font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fonts/Geist/Geist-VariableFont_wght.ttf");

    let font = FontFace::from_path(&font_path, 0).expect("Failed to load font");
    let font_size = 16.0;

    println!("=== Text Editing Demo ===\n");

    // 1. Basic text insertion
    println!("1. Basic Text Insertion:");
    let mut layout = TextLayout::new("Hello", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let cursor = layout.insert_str(5, " World", &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After insert: '{}' (cursor at {})",
        layout.text(),
        cursor
    );
    println!();

    // 2. Character insertion
    println!("2. Character Insertion:");
    let mut layout = TextLayout::new("Hello World", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let cursor = layout.insert_char(11, '!', &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After insert '!': '{}' (cursor at {})",
        layout.text(),
        cursor
    );
    println!();

    // 3. Newline insertion
    println!("3. Newline Insertion:");
    let mut layout = TextLayout::new("HelloWorld", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let cursor = layout.insert_newline(5, &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After newline: '{}' (cursor at {})",
        layout.text().replace('\n', "\\n"),
        cursor
    );
    println!("   Lines: {}", layout.lines().len());
    println!();

    // 4. Selection replacement
    println!("4. Selection Replacement:");
    let mut layout = TextLayout::new("Hello World", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let selection = Selection::new(6, 11); // Select "World"
    let cursor =
        layout.replace_selection(&selection, "Rust", &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After replacing 'World' with 'Rust': '{}' (cursor at {})",
        layout.text(),
        cursor
    );
    println!();

    // 5. Backspace deletion
    println!("5. Backspace Deletion:");
    let mut layout = TextLayout::new("Hello!", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let cursor = layout.delete_backward(6, &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After backspace: '{}' (cursor at {})",
        layout.text(),
        cursor
    );
    println!();

    // 6. Forward deletion
    println!("6. Forward Deletion:");
    let mut layout = TextLayout::new("Hello World", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let cursor = layout.delete_forward(5, &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After delete: '{}' (cursor at {})",
        layout.text(),
        cursor
    );
    println!();

    // 7. Word deletion
    println!("7. Word Deletion:");
    let mut layout = TextLayout::new("Hello World", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let cursor = layout.delete_word_backward(11, &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After delete word backward: '{}' (cursor at {})",
        layout.text(),
        cursor
    );
    println!();

    // 8. Selection deletion
    println!("8. Selection Deletion:");
    let mut layout = TextLayout::new("Hello World", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let selection = Selection::new(5, 11); // Select " World"
    let cursor = layout.delete_selection(&selection, &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After deleting selection: '{}' (cursor at {})",
        layout.text(),
        cursor
    );
    println!();

    // 9. Line deletion
    println!("9. Line Deletion:");
    let mut layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, font_size);
    println!("   Initial: '{}'", layout.text().replace('\n', "\\n"));

    let cursor = layout.delete_line(10, &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After deleting line 2: '{}' (cursor at {})",
        layout.text().replace('\n', "\\n"),
        cursor
    );
    println!();

    // 10. Unicode handling
    println!("10. Unicode Handling:");
    let mut layout = TextLayout::new("Hello", &font, font_size);
    println!("   Initial: '{}'", layout.text());

    let cursor = layout.insert_str(5, " 👋🌍", &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After inserting emoji: '{}' (cursor at {})",
        layout.text(),
        cursor
    );

    let cursor = layout.delete_backward(cursor, &font, font_size, None, WrapMode::NoWrap);
    println!(
        "   After deleting one emoji: '{}' (cursor at {})",
        layout.text(),
        cursor
    );
    println!();

    // 11. Text wrapping with insertion
    println!("11. Text Wrapping:");
    let mut layout = TextLayout::new("Short", &font, font_size);
    println!(
        "   Initial: '{}' ({} line)",
        layout.text(),
        layout.lines().len()
    );

    layout.insert_str(
        5,
        " text that should wrap to multiple lines",
        &font,
        font_size,
        Some(150.0),
        WrapMode::BreakWord,
    );
    println!(
        "   After insert with wrapping: '{}' ({} lines)",
        layout.text(),
        layout.lines().len()
    );
    println!();

    println!("=== Demo Complete ===");
}
