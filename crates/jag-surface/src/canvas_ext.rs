//! Extension methods for [`Canvas`] that support paint-cache replay.

use crate::Canvas;

impl Canvas {
    /// Record a document overlay for replay after that document's content.
    /// Preserve active transforms, scroll bindings and opacity, but escape
    /// clips introduced after `scope_start`. The caller must append the result
    /// at the same outer scope, after balancing the document's own scopes.
    /// `paint` must emit balanced display-list drawing commands, not side channels.
    pub fn capture_overlay_commands(
        &mut self,
        scope_start: usize,
        clip_count: usize,
        paint: impl FnOnce(&mut Self),
    ) -> Vec<jag_draw::Command> {
        use jag_draw::Command;
        let mut scopes = Vec::new();
        for command in &self.display_list().commands[scope_start..] {
            match command {
                Command::PushTransform(_)
                | Command::PushScrollLayer { .. }
                | Command::PushOpacity(_) => scopes.push(command.clone()),
                Command::PopTransform | Command::PopScrollLayer | Command::PopOpacity => {
                    scopes.pop();
                }
                _ => {}
            }
        }
        let start = self.command_count();
        let side_channels = self.capture_side_channels();
        // Only the CPU clip state is suspended. The document's recorded clip
        // pushes stay intact, so restoring an ancestor never rebases its clip
        // through a descendant's scroll transform.
        let clips = self.clip_stack.clone();
        let rounded = self.rounded_clip_stack.clone();
        assert!(clip_count < clips.len());
        self.clip_stack.truncate(clips.len() - clip_count);
        self.rounded_clip_stack.truncate(rounded.len() - clip_count);
        // Raw document paints (for example drag previews) may not have a
        // scroll/effect scope to select deferred text automatically.
        let needs_recording_scope = !self.painter.has_active_effect();
        if needs_recording_scope {
            self.push_opacity(1.0);
        }
        paint(self);
        if needs_recording_scope {
            self.pop_opacity();
        }
        debug_assert_eq!(
            self.capture_side_channels(),
            side_channels,
            "overlay paint must use the display list"
        );
        let mut commands = self.display_list().commands[start..].to_vec();
        self.painter.truncate_commands(start);
        self.clip_stack = clips;
        self.rounded_clip_stack = rounded;
        let mut result = Vec::new();
        for scope in &scopes {
            let mut scope = scope.clone();
            if let Command::PushScrollLayer { key, .. } = &mut scope {
                // Overlay tiles must not share the content layer's tile IDs.
                key.push_str(&format!("/overlay/{start}"));
            }
            result.push(scope);
        }
        result.append(&mut commands);
        for scope in scopes.iter().rev() {
            result.push(match scope {
                Command::PushTransform(_) => Command::PopTransform,
                Command::PushScrollLayer { .. } => Command::PopScrollLayer,
                Command::PushOpacity(_) => Command::PopOpacity,
                _ => unreachable!(),
            });
        }
        result
    }

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
