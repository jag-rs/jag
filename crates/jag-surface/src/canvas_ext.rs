//! Extension methods for [`Canvas`] that support paint-cache replay.

use crate::Canvas;

impl Canvas {
    /// Replay cached display list commands.
    pub fn extend_commands(&mut self, commands: &[jag_draw::Command]) {
        self.painter.extend_commands(commands);
    }
}
