/// Minimal DPI helpers used across the engine to keep scaling consistent.
///
/// This module intentionally does not depend on winit; callers provide the
/// platform scale factor (logical→physical) as `f32`.

/// Combined logical scale used for mapping authored logical pixels to physical pixels.
/// When `logical_pixels` is true, return `(scale_factor * ui_scale)` clamped to a sane minimum.
#[inline]
pub fn logical_multiplier(logical_pixels: bool, scale_factor: f32, ui_scale: f32) -> f32 {
    if logical_pixels {
        let s = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let u = if ui_scale.is_finite() && ui_scale > 0.0 {
            ui_scale
        } else {
            1.0
        };
        (s * u).max(0.0001)
    } else {
        1.0
    }
}

/// Snap a coordinate to the nearest device pixel for crisp edges at a given scale factor.
#[inline]
pub fn snap_to_device(v: f32, scale_factor: f32) -> f32 {
    let sf = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (v * sf).round() / sf
}
