//! Independent full-canvas mask oracle for bounded pixel visits/global paint.
use super::*;
use std::collections::HashMap;
fn full_canvas(op: &Draw, width: u32, height: u32) -> Vec<u8> {
    let mut path = geometry::path(&op.path).unwrap();
    if let Some(stroke) = &op.stroke {
        path = geometry::outline(path, stroke, op.transform).unwrap();
    }
    let mut mask = Mask::new(width, height).unwrap();
    mask.fill_path(&path, rule(&op.rule), true, Transform::identity());
    for clip in &op.clips {
        mask.intersect_path(
            &geometry::path(&clip.path).unwrap(),
            rule(&clip.rule),
            true,
            Transform::identity(),
        );
    }
    let images = HashMap::new();
    mask.data()
        .iter()
        .enumerate()
        .flat_map(|(i, &coverage)| {
            let color = op.paint.sample(
                (i % width as usize) as f64 + 0.5,
                (i / width as usize) as f64 + 0.5,
                &images,
            );
            let alpha = color[3] * op.alpha * (f64::from(coverage) / 255.);
            [color[0] * alpha, color[1] * alpha, color[2] * alpha, alpha]
                .map(|v| (v * 255.).round() as u8)
        })
        .collect()
}
#[test]
fn bounded_pixel_visits_match_full_canvas_for_edges_curves_strokes_and_clips() {
    for origin in [-8.25, 0., 3.125, 25.7, 58.5, 100.] {
        for stroke in [false, true] {
            let path = vec![
                vec![0., origin, 7.25],
                vec![3., origin + 4., -2., origin + 30., 30., origin + 12., 44.],
                vec![1., origin + 2., 48.75],
                vec![4.],
            ];
            let clip = Clip {
                path: vec![
                    vec![0., 0.5, 0.5],
                    vec![1., 60.25, 4.],
                    vec![1., 55., 52.],
                    vec![1., 1., 48.],
                    vec![4.],
                ],
                rule: "evenodd".into(),
            };
            let op = Draw {
                path,
                rule: "nonzero".into(),
                stroke: stroke.then(|| StrokeStyle {
                    width: 2.25,
                    cap: "round".into(),
                    join: "round".into(),
                    miter: 10.,
                    dash: vec![4., 2.],
                    offset: 1.,
                }),
                transform: geometry::IDENTITY,
                clips: vec![clip],
                paint: Paint::Linear {
                    args: vec![0., 0., 0., 60.],
                    matrix: [1.25, 0., 0., 1.5, 7., -3.],
                    stops: vec![(0., [0.2, 0.5, 0.9, 1.]), (1., [0.8, 0.3, 0.1, 0.5])],
                },
                alpha: 0.7,
                clear: false,
            };
            let mut raster = Raster::new(64, 56, true).unwrap();
            raster.draw(&op, &HashMap::new());
            let expected = full_canvas(&op, 64, 56);
            let differences: Vec<_> = raster
                .pixels()
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, (a, b))| (i, *a, *b))
                .collect();
            assert!(
                differences.is_empty(),
                "origin={origin} stroke={stroke}, {} differing channels; first={:?}",
                differences.len(),
                &differences[..differences.len().min(12)]
            );
        }
    }
}
#[test]
fn cropped_clear_preserves_outside_and_opaque_alpha() {
    let op = Draw {
        path: vec![
            vec![0., 3., 4.],
            vec![1., 7., 4.],
            vec![1., 7., 8.],
            vec![1., 3., 8.],
            vec![4.],
        ],
        rule: "nonzero".into(),
        stroke: None,
        transform: geometry::IDENTITY,
        clips: vec![],
        paint: Paint::Solid {
            color: [1., 0., 0., 1.],
        },
        alpha: 1.,
        clear: false,
    };
    for alpha in [true, false] {
        let mut r = Raster::new(16, 16, alpha).unwrap();
        r.draw(&op, &HashMap::new());
        assert_eq!(
            &r.pixels()[(5 * 16 + 4) * 4..(5 * 16 + 4) * 4 + 4],
            &[255, 0, 0, 255]
        );
        let mut clear = op.clone();
        clear.clear = true;
        r.draw(&clear, &HashMap::new());
        assert_eq!(
            &r.pixels()[(5 * 16 + 4) * 4..(5 * 16 + 4) * 4 + 4],
            &[0, 0, 0, if alpha { 0 } else { 255 }]
        );
        assert_eq!(&r.pixels()[0..4], &[0, 0, 0, if alpha { 0 } else { 255 }]);
    }
}

#[test]
fn reused_coverage_does_not_repaint_previous_path_or_survive_reset() {
    let mut raster = Raster::new(32, 24, true).unwrap();
    let mut op = Draw {
        path: vec![
            vec![0., 2., 2.],
            vec![1., 6., 2.],
            vec![1., 6., 6.],
            vec![1., 2., 6.],
            vec![4.],
        ],
        rule: "nonzero".into(),
        stroke: None,
        transform: geometry::IDENTITY,
        clips: vec![],
        paint: Paint::Solid {
            color: [1., 0., 0., 1.],
        },
        alpha: 1.,
        clear: false,
    };
    raster.draw(&op, &HashMap::new());
    for command in &mut op.path {
        for i in (1..command.len()).step_by(2) {
            command[i] += 16.;
        }
    }
    op.paint = Paint::Solid {
        color: [0., 1., 0., 1.],
    };
    raster.draw(&op, &HashMap::new());
    assert_eq!(
        &raster.pixels()[(3 * 32 + 3) * 4..(3 * 32 + 3) * 4 + 4],
        &[255, 0, 0, 255]
    );
    assert_eq!(
        &raster.pixels()[(3 * 32 + 19) * 4..(3 * 32 + 19) * 4 + 4],
        &[0, 255, 0, 255]
    );
    raster.clear();
    raster.draw(&op, &HashMap::new());
    assert_eq!(
        &raster.pixels()[(3 * 32 + 3) * 4..(3 * 32 + 3) * 4 + 4],
        &[0, 0, 0, 0]
    );
}
