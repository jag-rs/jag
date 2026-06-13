//! GPU-free validation that the analytic shadow shader parses and type-checks
//! through naga (the same front end wgpu uses), so pipeline creation won't fail
//! on a malformed shader. Pixel-level correctness vs the CPU coverage twin is
//! checked in a later GPU slice.

fn validate(src: &str) {
    let module =
        naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("WGSL parse failed: {e:?}"));
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|e| panic!("WGSL validation failed: {e:?}"));
}

#[test]
fn shadow_instance_shader_parses_and_validates() {
    // Panics with a diagnostic if the shader is malformed; passing is the assertion.
    validate(jag_shaders::SHADOW_INSTANCE_WGSL);
}

#[test]
fn shadow_instance_shader_has_both_entry_points() {
    let module = naga::front::wgsl::parse_str(jag_shaders::SHADOW_INSTANCE_WGSL).unwrap();
    let stages: Vec<_> = module
        .entry_points
        .iter()
        .map(|ep| (ep.name.clone(), ep.stage))
        .collect();
    assert!(
        stages
            .iter()
            .any(|(n, s)| n == "vs_main" && *s == naga::ShaderStage::Vertex),
        "missing vertex entry point, got {stages:?}"
    );
    assert!(
        stages
            .iter()
            .any(|(n, s)| n == "fs_main" && *s == naga::ShaderStage::Fragment),
        "missing fragment entry point, got {stages:?}"
    );
}
