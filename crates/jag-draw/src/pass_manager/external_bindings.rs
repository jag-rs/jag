use super::PassManager;
use std::sync::Arc;
impl PassManager {
    /// Register an externally-rendered texture for compositing in the current frame.
    pub fn register_external_texture(
        &mut self,
        id: crate::display_list::ExternalTextureId,
        view: wgpu::TextureView,
    ) {
        self.register_shared_external_texture(id, Arc::new(view));
    }

    /// Register a retained surface without transferring its cache ownership.
    pub fn register_shared_external_texture(
        &mut self,
        id: crate::display_list::ExternalTextureId,
        view: Arc<wgpu::TextureView>,
    ) {
        self.external_texture_owners.remove(&id);
        self.linear_external_textures.remove(&id);
        self.external_textures.insert(id, view);
    }

    /// Register a texture whose sampling yields linear values (an `*Srgb`
    /// format), such as a 3D viewport; the image shader encodes it for the
    /// target's blend space. [`Self::register_external_texture`] textures
    /// already hold blend-space values.
    pub fn register_linear_external_texture(
        &mut self,
        id: crate::display_list::ExternalTextureId,
        view: wgpu::TextureView,
    ) {
        self.external_texture_owners.remove(&id);
        self.external_textures.insert(id, Arc::new(view));
        self.linear_external_textures.insert(id);
    }

    /// Copy current-frame bindings when a host changes composition precision.
    pub fn copy_external_textures_to(&self, destination: &mut Self) {
        destination
            .external_textures
            .clone_from(&self.external_textures);
        destination
            .linear_external_textures
            .clone_from(&self.linear_external_textures);
        destination
            .external_texture_owners
            .clone_from(&self.external_texture_owners);
    }

    /// Clear all registered external textures (call after frame).
    pub fn clear_external_textures(&mut self) {
        self.external_textures.clear();
        self.external_texture_owners.clear();
        self.linear_external_textures.clear();
        self.composite_direct.release_expired_textures();
        self.composite_offscreen.release_expired_textures();
    }
}

impl PassManager {
    /// Keep a frame lease for exactly as long as its native binding is retained.
    /// Callers clear registrations only after submitting the frame on the shared queue.
    pub fn register_linear_external_texture_with_owner(
        &mut self,
        id: crate::ExternalTextureId,
        view: wgpu::TextureView,
        owner: Arc<dyn std::any::Any + Send + Sync>,
    ) {
        self.register_linear_external_texture(id, view);
        self.external_texture_owners.insert(id, owner);
    }
}
