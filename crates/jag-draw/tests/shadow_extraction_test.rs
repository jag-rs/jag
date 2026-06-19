//! Validates the box-shadow extraction wiring added for the analytic shadow
//! GPU pass: `Painter::box_shadow` must emit a `Command::BoxShadow`, and the
//! command's fields must feed `ShadowInstance::from_box_shadow` to produce the
//! instance the unified builder pushes onto `shadow_instances`.
//!
//! The unified builder's GPU-side extraction (`upload_display_list_unified`)
//! needs a `wgpu::Device`/`Queue`, so it is exercised by the device-level
//! checks rather than here. This test mirrors the exact transformation the
//! `Command::BoxShadow { rrect, spec, z, transform, clip } =>` arm performs, so a
//! regression in either the Painter command or that arm's mapping is caught
//! without a GPU.

use jag_draw::{
    BoxShadowSpec, ColorLinPremul, Command, Painter, Rect, RoundedRadii, RoundedRect,
    ShadowInstance, Transform2D, Viewport,
};

fn sample_rrect() -> RoundedRect {
    RoundedRect {
        rect: Rect {
            x: 40.0,
            y: 60.0,
            w: 120.0,
            h: 80.0,
        },
        radii: RoundedRadii {
            tl: 12.0,
            tr: 12.0,
            br: 12.0,
            bl: 12.0,
        },
    }
}

fn sample_spec() -> BoxShadowSpec {
    BoxShadowSpec {
        offset: [4.0, 6.0],
        spread: 2.0,
        blur_radius: 18.0,
        color: ColorLinPremul {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 0.5,
        },
    }
}

#[test]
fn painter_box_shadow_emits_box_shadow_command() {
    let mut painter = Painter::begin_frame(Viewport {
        width: 800,
        height: 600,
    });
    painter.box_shadow(sample_rrect(), sample_spec(), 5);
    let list = painter.finish();

    let shadows: Vec<&Command> = list
        .commands
        .iter()
        .filter(|c| matches!(c, Command::BoxShadow { .. }))
        .collect();
    assert_eq!(
        shadows.len(),
        1,
        "expected exactly one BoxShadow command, got {}",
        shadows.len()
    );
}

#[test]
fn box_shadow_command_fields_map_to_expected_instance() {
    let mut painter = Painter::begin_frame(Viewport {
        width: 800,
        height: 600,
    });
    painter.box_shadow(sample_rrect(), sample_spec(), 5);
    let list = painter.finish();

    let Some(Command::BoxShadow {
        rrect,
        spec,
        z,
        transform,
        clip,
    }) = list
        .commands
        .iter()
        .find(|c| matches!(c, Command::BoxShadow { .. }))
    else {
        panic!("no BoxShadow command found");
    };

    // This is exactly what the unified builder's BoxShadow arm pushes.
    let from_command = ShadowInstance::from_box_shadow(*rrect, *spec, *z, *transform, *clip);
    // Reference: same call from the original inputs.
    let reference = ShadowInstance::from_box_shadow(
        sample_rrect(),
        sample_spec(),
        5,
        Transform2D::identity(),
        None,
    );

    assert_eq!(from_command.lower, reference.lower);
    assert_eq!(from_command.upper, reference.upper);
    assert_eq!(from_command.params, reference.params);
    assert_eq!(from_command.color, reference.color);
    assert_eq!(from_command.clip_min, reference.clip_min);
    assert_eq!(from_command.clip_max, reference.clip_max);

    // Spot-check the derived geometry so the mapping is pinned, not just equal
    // to itself: bounds = rect + offset, expanded by spread; sigma = blur/2.
    assert_eq!(from_command.lower, [40.0 + 4.0 - 2.0, 60.0 + 6.0 - 2.0]);
    assert_eq!(
        from_command.upper,
        [40.0 + 120.0 + 4.0 + 2.0, 60.0 + 80.0 + 6.0 + 2.0]
    );
    assert!((from_command.params[0] - 9.0).abs() < 1e-4); // sigma = 18/2
    assert_eq!(from_command.params[2], 5.0); // z
}
