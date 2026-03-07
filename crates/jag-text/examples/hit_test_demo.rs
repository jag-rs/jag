use jag_text::layout::TextLayout;
use jag_text::{FontCache, HitTestPolicy, Point};

fn main() {
    // Load a font
    let mut font_cache = FontCache::new();
    let font = font_cache
        .get_or_load("/System/Library/Fonts/Helvetica.ttc", 0)
        .expect("Failed to load font");

    // Create a multi-line text layout
    let text = "Hello, World!\nThis is line 2.\nAnd line 3.";
    let layout = TextLayout::new(text, &font, 16.0);

    println!("=== Hit Testing Demo (Zone-Local Coordinates) ===\n");
    println!("Text: {:?}", text);
    println!("Number of lines: {}\n", layout.lines().len());

    // Demonstrate zone-local coordinates
    println!("Zone-local coordinates are relative to the layout origin (0, 0).");
    println!("This ensures hit testing works regardless of screen position.\n");

    // Test hit testing at various points
    let test_points = vec![
        (0.0, 0.0, "Top-left corner (start of text)"),
        (50.0, 0.0, "Middle of first line"),
        (0.0, 20.0, "Start of second line"),
        (30.0, 20.0, "Middle of second line"),
        (0.0, 40.0, "Start of third line"),
        (100.0, 100.0, "Far beyond text (clamped)"),
        (-10.0, -10.0, "Before text (clamped)"),
    ];

    println!("--- Hit Testing Results ---");
    for (x, y, description) in test_points {
        let point = Point::new(x, y);

        if let Some(result) = layout.hit_test(point, HitTestPolicy::Clamp) {
            let char_at = if result.byte_offset < text.len() {
                text[result.byte_offset..].chars().next().unwrap_or('∅')
            } else {
                '∅'
            };

            println!("\nPoint ({:.1}, {:.1}) - {}:", x, y, description);
            println!("  Byte offset: {}", result.byte_offset);
            println!("  Line index: {}", result.line_index);
            println!("  Character: '{}'", char_at);
            println!("  Affinity: {:?}", result.affinity);
        }
    }

    // Demonstrate offset to position mapping
    println!("\n--- Offset to Position Mapping ---");
    let test_offsets = vec![
        (0, "Start of text"),
        (7, "After 'Hello, '"),
        (14, "Start of line 2"),
        (text.len(), "End of text"),
    ];

    for (offset, description) in &test_offsets {
        if let Some(pos) = layout.offset_to_position(*offset) {
            println!("\nOffset {} - {}:", offset, description);
            println!("  Zone-local position: ({:.2}, {:.2})", pos.x, pos.y);
            println!("  Line index: {}", pos.line_index);
        }
    }

    // Demonstrate baseline position (useful for IME)
    println!("\n--- Baseline Positions (for IME) ---");
    for (offset, description) in &test_offsets[..3] {
        if let Some(pos) = layout.offset_to_baseline_position(*offset) {
            println!("\nOffset {} - {}:", offset, description);
            println!("  Baseline position: ({:.2}, {:.2})", pos.x, pos.y);
            println!("  Line index: {}", pos.line_index);
        }
    }

    // Demonstrate strict policy
    println!("\n--- Strict Policy (returns None for out-of-bounds) ---");
    let out_of_bounds = Point::new(1000.0, 1000.0);
    match layout.hit_test(out_of_bounds, HitTestPolicy::Strict) {
        Some(result) => println!("Unexpected result: {:?}", result),
        None => println!("Point (1000, 1000) is out of bounds - correctly returned None"),
    }

    // Round-trip test: offset -> position -> hit test -> offset
    println!("\n--- Round-Trip Test ---");
    let test_offset = 7;
    if let Some(pos) = layout.offset_to_position(test_offset) {
        let point = pos.to_point();
        if let Some(result) = layout.hit_test(point, HitTestPolicy::Clamp) {
            println!("Original offset: {}", test_offset);
            println!("Position: ({:.2}, {:.2})", pos.x, pos.y);
            println!("Hit test result: {}", result.byte_offset);
            println!(
                "Round-trip successful: {}",
                result.byte_offset == test_offset
            );
        }
    }
}
