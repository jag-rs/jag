# fix: decode images by content, not file extension

## Summary

`ImageCache` now selects the image decoder from the file's **content (magic
bytes)** rather than its **file extension**. This makes image loading robust to
files whose name doesn't match their bytes, which previously failed silently and
rendered blank.

Commit: `16642d1` — *fix: decode images by content, not file extension*

## Problem

`decode_image_file` loaded filesystem images with `image::open(path)`, which
picks the decoder from the path extension. When a caller pointed at a file whose
extension lied about its format — e.g. a PNG cached under a `.jpg` name — the
JPEG decoder was run on PNG bytes, decoding failed, and the result was dropped.
The image element then painted nothing: **a blank/black image with no error
event**, which is very hard to diagnose.

This surfaced in the KilneOS **Gallery** app: full-screen photos showed black for
some items while their grid thumbnails rendered fine. The affected items were
PNG screenshots that an upstream media bridge had copied into the app cache under
a hardcoded `.jpg` name. JPEG camera photos and the (always-JPEG) thumbnails were
unaffected, which is why the failure looked intermittent.

## Fix

Introduce a single helper that sniffs the format from the content and use it at
both filesystem decode sites in `crates/jag-draw/src/image_cache.rs`:

```rust
fn decode_image_from_path(path: &Path) -> Option<image::DynamicImage> {
    image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()  // read magic bytes, ignore the extension
        .ok()?
        .decode()
        .ok()
}
```

- `decode_image_file` (async/off-thread decode path) — now calls the helper.
- The synchronous `ImageCache` decode path — now calls the helper.

Embedded/built-in assets (`builtin_image_bytes` → `image::load_from_memory`)
already sniffed content and are unchanged. The texture-size guard
(`max_texture_dimension_2d`) and LRU behaviour are unchanged.

## Tests

Added a unit test in `image_cache.rs`
(`decodes_png_bytes_written_with_jpg_extension`) that:

1. writes PNG bytes to a file named `*.jpg`,
2. asserts the old extension-based `image::open` **fails** on it (locks in the
   regression), and
3. asserts the new path decodes it to the correct dimensions.

`cargo test -p jag-draw` passes.

## Docs

- Added a module-level `//!` doc to `image_cache.rs` describing the cache and the
  content-based decoding decision.
- Updated `crates/jag-draw/README.md`: *"Image loading and caching (format
  sniffed from content, not file extension)."*

## Related changes (sibling repos)

This is the robust, root-level fix. The proximate cause was fixed in parallel so
cached files are also named honestly:

- **waid** — `DetirPhotosBridge.java` now derives the cached copy's extension
  from the content URI's MIME type instead of hardcoding `.jpg`
  (*fix: name cached full-res photos by their real image type*).

Either change alone resolves the Gallery symptom; together they fix both the
renderer's robustness and the cache's correctness.
