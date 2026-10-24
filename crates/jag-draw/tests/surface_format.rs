use jag_draw::{select_srgb_surface_format, wgpu::TextureFormat};

#[test]
fn surface_format_prefers_srgb_over_capability_order() {
    let formats = [
        TextureFormat::Bgra8Unorm,
        TextureFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba8UnormSrgb,
    ];

    assert_eq!(
        select_srgb_surface_format(&formats),
        Some(TextureFormat::Bgra8UnormSrgb)
    );
}

#[test]
fn surface_format_falls_back_to_first_non_srgb_format() {
    let formats = [TextureFormat::Bgra8Unorm, TextureFormat::Rgba16Float];

    assert_eq!(
        select_srgb_surface_format(&formats),
        Some(TextureFormat::Bgra8Unorm)
    );
    assert_eq!(select_srgb_surface_format(&[]), None);
}
