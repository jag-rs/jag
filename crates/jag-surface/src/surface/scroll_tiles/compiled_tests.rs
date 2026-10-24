use super::*;

fn rect(z: i32) -> Command {
    Command::DrawRect {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        },
        brush: jag_draw::Brush::Solid(jag_draw::ColorLinPremul::from_srgba_u8([
            255, 255, 255, 255,
        ])),
        z,
        transform: Transform2D::identity(),
    }
}

#[test]
fn run_splits_where_it_crosses_another_layers_z() {
    let thresholds = [4, 8].into_iter().collect();
    let bands = split_at_thresholds(&[rect(4), rect(10)], &thresholds);
    assert_eq!(bands.len(), 2);
    assert_eq!(bands[1][0].z_index(), Some(10));
}

#[test]
fn run_without_crossing_stays_whole() {
    let thresholds = [4, 20].into_iter().collect();
    assert_eq!(
        split_at_thresholds(&[rect(4), rect(10)], &thresholds).len(),
        1
    );
}

#[test]
fn split_keeps_transforms_balanced() {
    let thresholds = [4, 8].into_iter().collect();
    let push = Command::PushTransform(Transform2D::translate(5.0, 0.0));
    let bands = split_at_thresholds(
        &[push.clone(), rect(4), rect(10), Command::PopTransform],
        &thresholds,
    );
    assert_eq!(bands.len(), 2);
    for band in &bands {
        let depth = band.iter().fold(0i32, |d, c| match c {
            Command::PushTransform(_) => d + 1,
            Command::PopTransform => d - 1,
            _ => d,
        });
        assert_eq!(depth, 0, "unbalanced band {band:?}");
    }
    assert!(matches!(bands[1][0], Command::PushTransform(_)));
}

#[test]
fn equal_z_commands_stay_in_one_band() {
    let thresholds = [4, 8].into_iter().collect();
    assert_eq!(
        split_at_thresholds(&[rect(4), rect(4), rect(5)], &thresholds).len(),
        1
    );
}
