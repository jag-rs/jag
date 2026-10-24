//! Opt-in extended linear composition for native WebGPU canvas content.
use super::{JagSurface, opacity::LayerGeometry};
use anyhow::Result;
use jag_draw::{Command, ExternalTextureId, PassManager, SurfaceEffect, TextProvider, wgpu};
use std::sync::Arc;

impl JagSurface {
    /// Upgrade this surface's intermediate buffers when native content may
    /// contain colors outside sRGB. The final display format stays unchanged.
    /// Ordinary surfaces retain the existing bounded SDR rendering path.
    pub fn enable_extended_color_composition(&mut self) {
        if self.extended_color_composition {
            return;
        }
        let mut pass = PassManager::new_with_offscreen_format(
            self.device.clone(),
            self.surface_format,
            wgpu::TextureFormat::Rgba16Float,
        );
        self.pass.copy_external_textures_to(&mut pass);
        pass.set_scroll_offset(self.pass.scroll_offset());
        std::mem::swap(&mut self.pass.image_cache, &mut pass.image_cache);
        std::mem::swap(&mut self.pass.svg_cache, &mut pass.svg_cache);
        self.pass = pass;
        self.extended_color_composition = true;
        self.frame_cache = None;
        self.retained_layers.clear();
        self.scroll_tiles = Default::default();
    }

    pub(super) fn render_effect_group_layer(
        &mut self,
        geometry: LayerGeometry,
        commands: Vec<Command>,
        effect: SurfaceEffect,
        text_provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
        logical_scale: f32,
        stable_id: Option<ExternalTextureId>,
    ) -> Result<ExternalTextureId> {
        if !self.extended_color_composition {
            return self.render_effect_group_layer_inner(
                geometry,
                commands,
                effect,
                text_provider,
                logical_scale,
                stable_id,
            );
        }
        // Effect-group targets must preserve the same extended values as the
        // root. A cached float-target pass keeps its shader formats consistent.
        let mut group_pass = self.extended_group_pass.take().unwrap_or_else(|| {
            PassManager::new_with_offscreen_format(
                self.device.clone(),
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureFormat::Rgba16Float,
            )
        });
        group_pass.begin_composite_frame();
        group_pass.set_scale_factor(self.dpi_scale);
        group_pass.set_logical_pixels(self.logical_pixels);
        group_pass.set_ui_scale(self.ui_scale);
        self.pass.copy_external_textures_to(&mut group_pass);
        std::mem::swap(&mut self.pass.image_cache, &mut group_pass.image_cache);
        std::mem::swap(&mut self.pass.svg_cache, &mut group_pass.svg_cache);
        std::mem::swap(&mut self.pass, &mut group_pass);
        let display_format =
            std::mem::replace(&mut self.surface_format, wgpu::TextureFormat::Rgba16Float);
        let result = self.render_effect_group_layer_inner(
            geometry,
            commands,
            effect,
            text_provider,
            logical_scale,
            stable_id,
        );
        // Restore both success and failure paths. Newly created group views
        // belong to the parent frame; the cached group pass retains no views.
        self.surface_format = display_format;
        self.pass.copy_external_textures_to(&mut group_pass);
        self.pass.clear_external_textures();
        std::mem::swap(&mut self.pass, &mut group_pass);
        std::mem::swap(&mut self.pass.image_cache, &mut group_pass.image_cache);
        std::mem::swap(&mut self.pass.svg_cache, &mut group_pass.svg_cache);
        self.extended_group_pass = Some(group_pass);
        result
    }
}
