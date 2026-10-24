use super::*;
impl Painter {
    /// Draw an externally-rendered texture at the given rect.
    pub fn external_texture(&mut self, rect: Rect, texture_id: ExternalTextureId, z: i32) {
        self.external_texture_with_params(rect, texture_id, z, 1.0, false);
    }

    pub fn external_texture_with_params(
        &mut self,
        rect: Rect,
        texture_id: ExternalTextureId,
        z: i32,
        opacity: f32,
        premultiplied: bool,
    ) {
        self.external_texture_clipped(rect, texture_id, z, opacity, premultiplied, None, None);
    }

    pub fn external_texture_clipped(
        &mut self,
        rect: Rect,
        texture_id: ExternalTextureId,
        z: i32,
        opacity: f32,
        premultiplied: bool,
        rounded_clip: Option<crate::scene::RoundedRectClipGpu>,
        clip: Option<Rect>,
    ) {
        let transform = self.current_transform();
        self.list.commands.push(Command::DrawExternalTexture {
            rect,
            texture_id,
            z,
            transform,
            opacity: opacity.clamp(0.0, 1.0),
            premultiplied,
            rounded_clip,
            clip,
        });
    }
}
