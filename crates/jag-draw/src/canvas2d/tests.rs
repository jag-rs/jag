use super::{geometry::*, raster::*};
use std::collections::HashMap;
fn rect(x: f64, y: f64, w: f64, h: f64) -> Commands {
    vec![
        vec![0., x, y],
        vec![1., x + w, y],
        vec![1., x + w, y + h],
        vec![1., x, y + h],
        vec![4.],
    ]
}
fn draw(path: Commands) -> Draw {
    Draw {
        effects: Default::default(),
        path,
        rule: "nonzero".into(),
        stroke: None,
        transform: IDENTITY,
        clips: vec![],
        paint: Paint::Solid {
            color: [1., 0., 0., 1.],
        },
        alpha: 1.,
        clear: false,
    }
}
#[test]
fn canvas2d_path_holes_boundary_svg_and_stroke_outline() {
    let mut p = rect(0., 0., 20., 20.);
    p.extend(rect(5., 5., 10., 10.));
    let p = path(&p).unwrap();
    assert!(contains(&p, 10., 10., false));
    assert!(!contains(&p, 10., 10., true));
    assert!(contains(&p, 0., 0., true));
    let p = path(&svg("M0 0h10v10h-10z")).unwrap();
    assert!(contains(&p, 5., 5., false));
    let line = path(&vec![vec![0., 2., 10.], vec![1., 20., 10.]]).unwrap();
    let s = StrokeStyle {
        width: 4.,
        cap: "round".into(),
        join: "miter".into(),
        miter: 10.,
        dash: vec![],
        offset: 0.,
    };
    let outline = outline(line, &s, IDENTITY).unwrap();
    assert!(contains(&outline, 1., 10., false));
    assert!(!contains(&outline, -1., 10., false));
}
#[test]
fn canvas2d_raster_clip_snapshot_order_and_opaque_clear() {
    let mut raster = Raster::new(16, 16, true).unwrap();
    let mut d = draw(rect(0., 0., 16., 16.));
    d.clips.push(Clip {
        path: rect(2., 2., 10., 10.),
        rule: "nonzero".into(),
    });
    raster.draw(&d, &HashMap::new());
    let snapshot = raster.snapshot();
    assert_eq!(&snapshot.pixels[0..4], &[0; 4]);
    assert_eq!(
        &snapshot.pixels[(4 * 16 + 4) * 4..(4 * 16 + 4) * 4 + 4],
        &[255, 0, 0, 255]
    );
    raster.clear();
    assert_eq!(
        &raster.pixels()[(4 * 16 + 4) * 4..(4 * 16 + 4) * 4 + 4],
        &[0; 4]
    );
    assert_eq!(snapshot.pixels[(4 * 16 + 4) * 4], 255);
    let mut opaque = Raster::new(16, 16, false).unwrap();
    d.clips.clear();
    opaque.draw(&d, &HashMap::new());
    d.clear = true;
    d.path = rect(0., 0., 8., 8.);
    opaque.draw(&d, &HashMap::new());
    assert_eq!(&opaque.pixels()[0..4], &[0, 0, 0, 255]);
}
#[test]
fn canvas2d_gradients_interpolation_and_invalid_descriptors() {
    let p = Paint::Linear {
        args: vec![0., 0., 10., 0.],
        matrix: IDENTITY,
        stops: vec![(0., [1., 0., 0., 1.]), (1., [0., 0., 1., 0.])],
    };
    assert!(p.valid());
    assert_eq!(p.sample(5., 0., &HashMap::new()), [0.5, 0., 0.5, 0.5]);
    let bad = Paint::Linear {
        args: vec![],
        matrix: IDENTITY,
        stops: vec![],
    };
    assert!(!bad.valid());
    let mut d = draw(rect(0., 0., 2., 2.));
    d.paint = bad;
    let mut r = Raster::new(2, 2, true).unwrap();
    r.draw(&d, &HashMap::new());
    assert!(r.pixels().iter().all(|p| *p == 0));
}
#[test]
fn canvas2d_dash_changes_geometric_stroke_hits() {
    let p = path(&vec![vec![0., 0., 2.], vec![1., 20., 2.]]).unwrap();
    let s = StrokeStyle {
        width: 2.,
        cap: "butt".into(),
        join: "miter".into(),
        miter: 10.,
        dash: vec![4., 4.],
        offset: 0.,
    };
    let p = outline(p, &s, IDENTITY).unwrap();
    assert!(contains(&p, 2., 2., false));
    assert!(!contains(&p, 6., 2., false));
}
