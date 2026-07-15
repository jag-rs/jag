use jag_draw::{
    Brush, ExternalTextureId, FilterEffect, MaskCompositeLayer, MaskEffect, MaskGroupEffect,
    MaskMode, MaskTextureMapping, Rect,
};

use super::{Canvas, GeneratedMaskTexture, UrlMaskTexture};

impl Canvas {
    /// Resolve a generated-gradient brush into a texture and begin an owned mask scope.
    /// Returns false for unsupported brushes or singular transforms.
    pub fn push_generated_mask(&mut self, rect: Rect, brush: &Brush, mode: MaskMode) -> bool {
        self.push_generated_mask_pattern(rect, rect, [rect.w, rect.h], [false; 2], brush, mode)
    }

    /// Resolve a positioned and optionally repeated generated-gradient mask pattern.
    pub fn push_generated_mask_pattern(
        &mut self,
        paint_rect: Rect,
        tile_rect: Rect,
        tile_step: [f32; 2],
        repeat_axes: [bool; 2],
        brush: &Brush,
        mode: MaskMode,
    ) -> bool {
        let Some(mask) = self.resolve_generated_mask_pattern(
            paint_rect,
            tile_rect,
            tile_step,
            repeat_axes,
            brush,
            mode,
        ) else {
            return false;
        };
        self.push_filter(FilterEffect::Mask(mask));
        true
    }

    /// Resolve a generated mask layer without beginning an effect scope.
    pub fn resolve_generated_mask_pattern(
        &mut self,
        paint_rect: Rect,
        tile_rect: Rect,
        tile_step: [f32; 2],
        repeat_axes: [bool; 2],
        brush: &Brush,
        mode: MaskMode,
    ) -> Option<MaskEffect> {
        let stops = gradient_stops(brush)?;
        let [a, b, c, d, e, f] = self.current_transform().m;
        let transform = [a, b, c, d, e, f];
        if (a * d - b * c).abs() <= f32::EPSILON {
            return None;
        }
        let mapped_rect = transformed_bounds(paint_rect, transform);
        if mapped_rect.w <= 0.0 || mapped_rect.h <= 0.0 || stops.is_empty() {
            return None;
        }
        let width = mapped_rect.w.ceil().max(1.0) as u32;
        let height = mapped_rect.h.ceil().max(1.0) as u32;
        let pixels = raster_generated_gradient(
            mapped_rect,
            paint_rect,
            tile_rect,
            tile_step,
            repeat_axes,
            width,
            height,
            brush,
            transform,
            stops,
        );
        let id = ExternalTextureId(self.next_generated_mask_texture_id);
        self.next_generated_mask_texture_id = self.next_generated_mask_texture_id.wrapping_add(1);
        self.generated_mask_textures.push(GeneratedMaskTexture {
            id,
            width,
            height,
            pixels,
        });
        Some(MaskEffect {
            texture_id: id,
            mode,
            rect: mapped_rect,
            mapping: None,
        })
    }

    /// Begin a GPU-native URL mask. Missing or loading images resolve to transparent.
    pub fn push_url_mask(
        &mut self,
        path: impl Into<std::path::PathBuf>,
        paint_rect: Rect,
        tile_rect: Rect,
        tile_step: [f32; 2],
        repeat_axes: [bool; 2],
        mode: MaskMode,
    ) -> bool {
        let Some(mask) =
            self.resolve_url_mask(path, paint_rect, tile_rect, tile_step, repeat_axes, mode)
        else {
            return false;
        };
        self.push_filter(FilterEffect::Mask(mask));
        true
    }

    /// Resolve a URL mask layer without beginning an effect scope.
    pub fn resolve_url_mask(
        &mut self,
        path: impl Into<std::path::PathBuf>,
        paint_rect: Rect,
        tile_rect: Rect,
        tile_step: [f32; 2],
        repeat_axes: [bool; 2],
        mode: MaskMode,
    ) -> Option<MaskEffect> {
        let transform = self.current_transform().m;
        let inverse_transform = inverse_transform(transform)?;
        if paint_rect.w <= 0.0
            || paint_rect.h <= 0.0
            || tile_rect.w <= 0.0
            || tile_rect.h <= 0.0
            || repeat_axes[0] && tile_step[0] <= f32::EPSILON
            || repeat_axes[1] && tile_step[1] <= f32::EPSILON
        {
            return None;
        }
        let rect = transformed_bounds(paint_rect, transform);
        let id = ExternalTextureId(self.next_generated_mask_texture_id);
        self.next_generated_mask_texture_id = self.next_generated_mask_texture_id.wrapping_add(1);
        self.url_mask_textures.push(UrlMaskTexture {
            id,
            path: path.into(),
        });
        Some(MaskEffect {
            texture_id: id,
            mode,
            rect,
            mapping: Some(MaskTextureMapping {
                inverse_transform,
                paint_rect,
                tile_rect,
                tile_step,
                repeat_axes,
                flip_y: true,
            }),
        })
    }

    /// Begin one owned scope whose resolved layers are composited before application.
    pub fn push_mask_group(&mut self, layers: Vec<MaskCompositeLayer>) -> bool {
        if layers.is_empty() {
            return false;
        }
        self.push_filter(FilterEffect::MaskGroup(MaskGroupEffect { layers }));
        true
    }
}

fn transformed_bounds(rect: Rect, transform: [f32; 6]) -> Rect {
    let [a, b, c, d, e, f] = transform;
    let map = |x, y| [a * x + c * y + e, b * x + d * y + f];
    let corners = [
        map(rect.x, rect.y),
        map(rect.x + rect.w, rect.y),
        map(rect.x + rect.w, rect.y + rect.h),
        map(rect.x, rect.y + rect.h),
    ];
    let min_x = corners.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let max_x = corners
        .iter()
        .map(|p| p[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_y = corners
        .iter()
        .map(|p| p[1])
        .fold(f32::NEG_INFINITY, f32::max);
    Rect {
        x: min_x,
        y: min_y,
        w: max_x - min_x,
        h: max_y - min_y,
    }
}

fn gradient_stops(brush: &Brush) -> Option<&[(f32, jag_draw::ColorLinPremul)]> {
    match brush {
        Brush::LinearGradient { stops, .. }
        | Brush::RadialGradient { stops, .. }
        | Brush::ConicGradient { stops, .. } => Some(stops),
        Brush::Solid(_) => None,
    }
}

fn raster_generated_gradient(
    rect: Rect,
    paint_rect: Rect,
    tile_rect: Rect,
    tile_step: [f32; 2],
    repeat_axes: [bool; 2],
    width: u32,
    height: u32,
    brush: &Brush,
    transform: [f32; 6],
    stops: &[(f32, jag_draw::ColorLinPremul)],
) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let world = [
                rect.x + (x as f32 + 0.5) * rect.w / width as f32,
                // External textures are sampled with render-target UV orientation,
                // so upload rows bottom-to-top to preserve scene-space Y.
                rect.y + (height as f32 - y as f32 - 0.5) * rect.h / height as f32,
            ];
            let local = inverse_transform_point(world, transform);
            if local[0] < paint_rect.x
                || local[0] > paint_rect.x + paint_rect.w
                || local[1] < paint_rect.y
                || local[1] > paint_rect.y + paint_rect.h
            {
                pixels.extend_from_slice(&[0; 4]);
                continue;
            }
            let Some(sample) = pattern_point(local, tile_rect, tile_step, repeat_axes) else {
                pixels.extend_from_slice(&[0; 4]);
                continue;
            };
            let t = gradient_position(brush, sample);
            pixels.extend_from_slice(&sample_stops(stops, t));
        }
    }
    pixels
}

fn pattern_point(
    point: [f32; 2],
    tile: Rect,
    step: [f32; 2],
    repeat: [bool; 2],
) -> Option<[f32; 2]> {
    let axis = |value: f32, start: f32, size: f32, step: f32, repeat: bool| {
        if !repeat {
            return (value >= start && value <= start + size).then_some(value);
        }
        if step <= f32::EPSILON {
            return None;
        }
        let offset = (value - start).rem_euclid(step);
        (offset <= size).then_some(start + offset)
    };
    Some([
        axis(point[0], tile.x, tile.w, step[0], repeat[0])?,
        axis(point[1], tile.y, tile.h, step[1], repeat[1])?,
    ])
}

fn inverse_transform_point(point: [f32; 2], transform: [f32; 6]) -> [f32; 2] {
    let inverse = inverse_transform(transform).expect("validated mask transform must invert");
    let [a, b, c, d, e, f] = inverse;
    [
        a * point[0] + c * point[1] + e,
        b * point[0] + d * point[1] + f,
    ]
}

fn inverse_transform(transform: [f32; 6]) -> Option<[f32; 6]> {
    let [a, b, c, d, e, f] = transform;
    let determinant = a * d - b * c;
    (determinant.abs() > f32::EPSILON).then_some([
        d / determinant,
        -b / determinant,
        -c / determinant,
        a / determinant,
        (c * f - d * e) / determinant,
        (b * e - a * f) / determinant,
    ])
}

fn gradient_position(brush: &Brush, point: [f32; 2]) -> f32 {
    match brush {
        Brush::LinearGradient { start, end, .. } => {
            let delta = [end[0] - start[0], end[1] - start[1]];
            let length2 = (delta[0] * delta[0] + delta[1] * delta[1]).max(f32::EPSILON);
            ((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1]) / length2
        }
        Brush::RadialGradient { center, radius, .. } => {
            let delta = [point[0] - center[0], point[1] - center[1]];
            (delta[0] * delta[0] + delta[1] * delta[1]).sqrt() / radius.abs().max(f32::EPSILON)
        }
        Brush::ConicGradient {
            center,
            start_angle,
            ..
        } => {
            let angle = (point[0] - center[0]).atan2(-(point[1] - center[1])) - start_angle;
            angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU
        }
        Brush::Solid(_) => 0.0,
    }
}

fn sample_stops(stops: &[(f32, jag_draw::ColorLinPremul)], t: f32) -> [u8; 4] {
    let upper = stops
        .iter()
        .position(|stop| stop.0 >= t)
        .unwrap_or(stops.len() - 1);
    let lower = upper.saturating_sub(1);
    let span = stops[upper].0 - stops[lower].0;
    let mix = if span > f32::EPSILON {
        ((t - stops[lower].0) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let a = stops[lower].1.to_srgba_u8();
    let b = stops[upper].1.to_srgba_u8();
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * mix).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::{
        gradient_position, gradient_stops, pattern_point, raster_generated_gradient, sample_stops,
        transformed_bounds,
    };
    use jag_draw::{Brush, ColorLinPremul};

    #[test]
    fn samples_linear_gradient_midpoint_in_srgb() {
        let stops = [
            (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
            (1.0, ColorLinPremul::from_srgba_u8([255, 255, 255, 255])),
        ];
        assert_eq!(sample_stops(&stops, 0.5), [128; 4]);
    }

    #[test]
    fn radial_position_reaches_one_at_radius() {
        let brush = Brush::RadialGradient {
            center: [5.0, 6.0],
            radius: 4.0,
            stops: vec![],
        };
        assert_eq!(gradient_position(&brush, [5.0, 6.0]), 0.0);
        assert_eq!(gradient_position(&brush, [9.0, 6.0]), 1.0);
    }

    #[test]
    fn conic_position_runs_clockwise_from_north() {
        let brush = Brush::ConicGradient {
            center: [5.0, 5.0],
            start_angle: 0.0,
            stops: vec![],
        };
        assert_eq!(gradient_position(&brush, [5.0, 4.0]), 0.0);
        assert_eq!(gradient_position(&brush, [6.0, 5.0]), 0.25);
    }

    #[test]
    fn rotated_mask_raster_keeps_aabb_corners_transparent() {
        let rect = jag_draw::Rect {
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
        };
        let angle = std::f32::consts::FRAC_PI_4;
        let transform = [
            angle.cos(),
            angle.sin(),
            -angle.sin(),
            angle.cos(),
            4.0,
            0.0,
        ];
        let bounds = transformed_bounds(rect, transform);
        let brush = Brush::LinearGradient {
            start: [0.0, 0.0],
            end: [4.0, 0.0],
            stops: vec![
                (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
            ],
        };
        let width = bounds.w.ceil() as u32;
        let height = bounds.h.ceil() as u32;
        let pixels = raster_generated_gradient(
            bounds,
            rect,
            rect,
            [rect.w, rect.h],
            [false; 2],
            width,
            height,
            &brush,
            transform,
            gradient_stops(&brush).unwrap(),
        );
        let alphas = pixels
            .chunks_exact(4)
            .map(|pixel| pixel[3])
            .collect::<Vec<_>>();
        assert!(alphas.contains(&0));
        assert!(alphas.contains(&255));
    }

    #[test]
    fn repeated_pattern_maps_tiles_and_preserves_space_gaps() {
        let tile = jag_draw::Rect {
            x: 2.0,
            y: 3.0,
            w: 4.0,
            h: 5.0,
        };
        assert_eq!(
            pattern_point([8.0, 8.0], tile, [6.0, 7.0], [true; 2]),
            Some([2.0, 8.0])
        );
        assert_eq!(pattern_point([7.0, 8.0], tile, [6.0, 7.0], [true; 2]), None);
        assert_eq!(
            pattern_point([4.0, 8.0], tile, [6.0, 7.0], [false, true]),
            Some([4.0, 8.0])
        );
    }
}
