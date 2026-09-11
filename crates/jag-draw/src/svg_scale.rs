use crate::Transform2D;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RasterScaleKey(u32);

impl RasterScaleKey {
    pub(crate) fn from_scale(scale: f32) -> Option<Self> {
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        // Quantizing the raster scale moves authored edges off the pixel grid
        // even when the requested icon size is an exact number of pixels.
        // The cache is already bounded by its byte budget.
        Some(Self(scale.to_bits()))
    }

    pub(crate) fn as_f32(self) -> f32 {
        f32::from_bits(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SvgRasterPlan {
    pub(crate) layout_scale: f32,
    pub(crate) raster_scale: f32,
}

/// Resolve logical fit separately from physical raster resolution.
pub(crate) fn svg_raster_plan(
    base_size: [f32; 2],
    max_size: [f32; 2],
    device_scale: f32,
    transform: Transform2D,
) -> Option<SvgRasterPlan> {
    if base_size.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || max_size.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || !device_scale.is_finite()
        || device_scale <= 0.0
    {
        return None;
    }
    let layout_scale = (max_size[0] / base_size[0]).min(max_size[1] / base_size[1]);
    let [a, b, c, d, _, _] = transform.m;
    let transform_scale = a.hypot(b).max(c.hypot(d));
    let raster_scale = layout_scale * device_scale * transform_scale;
    (layout_scale.is_finite()
        && layout_scale > 0.0
        && raster_scale.is_finite()
        && raster_scale > 0.0)
        .then_some(SvgRasterPlan {
            layout_scale,
            raster_scale,
        })
}

#[cfg(test)]
mod tests {
    use super::{RasterScaleKey, svg_raster_plan};
    use crate::Transform2D;

    #[test]
    fn raster_plan_and_cache_key_track_css_size_dpr_and_transform() {
        for size in [16.0, 20.0, 24.0] {
            for dpr in [1.0, 2.0, 3.0] {
                let plan =
                    svg_raster_plan([24.0, 24.0], [size, size], dpr, Transform2D::identity())
                        .expect("valid SVG raster plan");
                let cached_scale = RasterScaleKey::from_scale(plan.raster_scale)
                    .expect("valid cache scale")
                    .as_f32();
                assert_eq!((24.0 * plan.layout_scale).round(), size);
                assert_eq!((24.0 * cached_scale).round(), size * dpr);
            }
        }

        let enlarged = svg_raster_plan(
            [24.0, 24.0],
            [16.0, 16.0],
            3.0,
            Transform2D::scale(2.0, 2.0),
        )
        .expect("transformed SVG raster plan");
        assert_eq!((24.0 * enlarged.raster_scale).round(), 96.0);
    }
}
