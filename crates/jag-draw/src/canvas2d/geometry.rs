//! Shared native path construction, outline and geometric hit tests.
use serde::{Deserialize, Serialize};
use tiny_skia::{Path, PathBuilder, PathSegment, Point, Stroke, StrokeDash, Transform};
pub type Commands = Vec<Vec<f64>>;
pub const IDENTITY: [f64; 6] = [1., 0., 0., 1., 0., 0.];
pub fn transform(m: [f64; 6]) -> Transform {
    Transform::from_row(
        m[0] as f32,
        m[1] as f32,
        m[2] as f32,
        m[3] as f32,
        m[4] as f32,
        m[5] as f32,
    )
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StrokeStyle {
    pub width: f64,
    pub cap: String,
    pub join: String,
    pub miter: f64,
    pub dash: Vec<f64>,
    pub offset: f64,
}
pub fn path(commands: &Commands) -> Option<Path> {
    let mut b = PathBuilder::new();
    for c in commands {
        if c.iter()
            .any(|n| !n.is_finite() || n.abs() > f32::MAX as f64)
        {
            return None;
        }
        match (c.first().copied()? as u8, c.len()) {
            (0, 3) => b.move_to(c[1] as f32, c[2] as f32),
            (1, 3) => b.line_to(c[1] as f32, c[2] as f32),
            (2, 5) => b.quad_to(c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32),
            (3, 7) => b.cubic_to(
                c[1] as f32,
                c[2] as f32,
                c[3] as f32,
                c[4] as f32,
                c[5] as f32,
                c[6] as f32,
            ),
            (4, 1) => b.close(),
            _ => return None,
        }
    }
    b.finish()
}
pub fn outline(p: Path, style: &StrokeStyle, m: [f64; 6]) -> Option<Path> {
    let ts = transform(m);
    let inverse = ts.invert()?;
    let mut p = p.transform(inverse)?;
    if style.dash.iter().any(|n| *n > 0.) {
        p = p.dash(
            &StrokeDash::new(
                style.dash.iter().map(|n| *n as f32).collect(),
                style.offset as f32,
            )?,
            1.,
        )?;
    }
    let stroke = Stroke {
        width: style.width as f32,
        miter_limit: style.miter as f32,
        line_cap: match style.cap.as_str() {
            "round" => tiny_skia::LineCap::Round,
            "square" => tiny_skia::LineCap::Square,
            _ => tiny_skia::LineCap::Butt,
        },
        line_join: match style.join.as_str() {
            "round" => tiny_skia::LineJoin::Round,
            "bevel" => tiny_skia::LineJoin::Bevel,
            _ => tiny_skia::LineJoin::Miter,
        },
        ..Stroke::default()
    };
    p.stroke(&stroke, ts.sx.hypot(ts.ky).max(ts.kx.hypot(ts.sy)).max(1.))?
        .transform(ts)
}
pub fn svg(source: &str) -> Commands {
    use svgtypes::{SimplePathSegment as S, SimplifyingPathParser};
    let mut result = Vec::new();
    for item in SimplifyingPathParser::from(source) {
        let Ok(item) = item else { break };
        result.push(match item {
            S::MoveTo { x, y } => vec![0., x, y],
            S::LineTo { x, y } => vec![1., x, y],
            S::Quadratic { x1, y1, x, y } => vec![2., x1, y1, x, y],
            S::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => vec![3., x1, y1, x2, y2, x, y],
            S::ClosePath => vec![4.],
        });
    }
    result
}
/// Geometric containment (not bitmap sampling); boundary points are included.
pub fn contains(p: &Path, x: f64, y: f64, even_odd: bool) -> bool {
    let mut edges = Vec::new();
    let mut start = Point::from_xy(0., 0.);
    let mut last = start;
    for s in p.segments() {
        match s {
            PathSegment::MoveTo(v) => {
                edges.push((last, start));
                start = v;
                last = v;
            }
            PathSegment::LineTo(v) => {
                edges.push((last, v));
                last = v;
            }
            PathSegment::QuadTo(a, b) => {
                let curve = lyon_geom::QuadraticBezierSegment {
                    from: lyon_geom::point(last.x as f64, last.y as f64),
                    ctrl: lyon_geom::point(a.x as f64, a.y as f64),
                    to: lyon_geom::point(b.x as f64, b.y as f64),
                };
                curve.for_each_flattened(0.00001, &mut |s| {
                    edges.push((
                        Point::from_xy(s.from.x as f32, s.from.y as f32),
                        Point::from_xy(s.to.x as f32, s.to.y as f32),
                    ))
                });
                last = b;
            }
            PathSegment::CubicTo(a, b, c) => {
                let curve = lyon_geom::CubicBezierSegment {
                    from: lyon_geom::point(last.x as f64, last.y as f64),
                    ctrl1: lyon_geom::point(a.x as f64, a.y as f64),
                    ctrl2: lyon_geom::point(b.x as f64, b.y as f64),
                    to: lyon_geom::point(c.x as f64, c.y as f64),
                };
                curve.for_each_flattened(0.00001, &mut |s| {
                    edges.push((
                        Point::from_xy(s.from.x as f32, s.from.y as f32),
                        Point::from_xy(s.to.x as f32, s.to.y as f32),
                    ))
                });
                last = c;
            }
            PathSegment::Close => {
                edges.push((last, start));
                last = start;
            }
        }
    }
    edges.push((last, start));
    let mut winding = 0i32;
    for (a, b) in edges {
        let (ax, ay, bx, by) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64);
        let cross = (bx - ax) * (y - ay) - (by - ay) * (x - ax);
        if (ax != bx || ay != by)
            && cross.abs() < 1e-6
            && x >= ax.min(bx)
            && x <= ax.max(bx)
            && y >= ay.min(by)
            && y <= ay.max(by)
        {
            return true;
        }
        if ay <= y && by > y && cross > 0. {
            winding += 1;
        }
        if by <= y && ay > y && cross < 0. {
            winding -= 1;
        }
    }
    if even_odd {
        winding % 2 != 0
    } else {
        winding != 0
    }
}
