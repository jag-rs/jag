//! Every shader built through `with_blend_space` must parse and validate in
//! both blend modes, or pipeline creation fails at runtime.

fn validate(name: &str, src: &str) {
    let module = naga::front::wgsl::parse_str(src)
        .unwrap_or_else(|e| panic!("{name}: WGSL parse failed: {e:?}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{name}: WGSL validation failed: {e:?}"));
}

#[test]
fn blend_space_shaders_validate_in_both_modes() {
    let shaders = [
        ("SOLID_WGSL", jag_shaders::SOLID_WGSL),
        ("TEXT_WGSL", jag_shaders::TEXT_WGSL),
        ("IMAGE_WGSL", jag_shaders::IMAGE_WGSL),
        ("BACKGROUND_WGSL", jag_shaders::BACKGROUND_WGSL),
        ("SHADOW_COMPOSITE_WGSL", jag_shaders::SHADOW_COMPOSITE_WGSL),
        ("COLOR_FILTER_WGSL", jag_shaders::COLOR_FILTER_WGSL),
        (
            "DROP_SHADOW_FILTER_WGSL",
            jag_shaders::DROP_SHADOW_FILTER_WGSL,
        ),
    ];
    for (name, src) in shaders {
        for gamma_blend in [false, true] {
            validate(name, &jag_shaders::with_blend_space(src, gamma_blend));
        }
    }
}
