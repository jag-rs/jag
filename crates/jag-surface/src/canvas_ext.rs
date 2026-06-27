//! Extension methods for [`Canvas`] that support paint-cache replay.

use crate::Canvas;

impl Canvas {
    /// Replay cached display list commands.
    pub fn extend_commands(&mut self, commands: &[jag_draw::Command]) {
        self.painter.extend_commands(commands);
    }

    /// Emit an analytic outer box-shadow for `rrect`, rendered by the GPU
    /// `ShadowInstanceRenderer`. The painter captures the current transform.
    /// CSS `blur-radius` (= 2σ) goes in `spec.blur_radius`.
    pub fn box_shadow(
        &mut self,
        rrect: jag_draw::RoundedRect,
        spec: jag_draw::BoxShadowSpec,
        z: i32,
    ) {
        self.painter
            .box_shadow_clipped(rrect, spec, z, self.rounded_clip_local());
    }
}
