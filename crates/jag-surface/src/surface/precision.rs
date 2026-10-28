//! Float composition with explicit native-linear or browser-sRGB blending.
use super::{JagSurface, opacity::LayerGeometry};
use anyhow::Result;
use jag_draw::{Command, ExternalTextureId, PassManager, SurfaceEffect, TextProvider, wgpu};
use std::sync::Arc;

impl JagSurface {
    /// Upgrade this surface's intermediate buffers when native content may
    /// contain colors outside sRGB. The final display format stays unchanged.
    /// Ordinary surfaces retain the existing bounded SDR rendering path.
    pub fn enable_extended_color_composition(&mut self) {
        if !self.web_srgb_composition {
            self.native_extended_color_composition = true;
        }
        if self.extended_color_composition {
            return;
        }
        self.rebuild_color_composition();
    }

    /// Browser CSS compositing uses encoded sRGB with exact authored alpha.
    /// Float intermediates preserve precision; sources project into the sRGB page gamut.
    pub fn enable_web_color_composition(&mut self) {
        self.set_web_color_composition(true);
    }

    /// Select the page blend space when a host switches between web and native IR.
    pub fn set_web_color_composition(&mut self, enabled: bool) {
        if self.web_srgb_composition == enabled {
            return;
        }
        self.web_srgb_composition = enabled;
        self.extended_group_pass = None;
        self.rebuild_color_composition();
    }

    fn rebuild_color_composition(&mut self) {
        self.extended_color_composition =
            self.web_srgb_composition || self.native_extended_color_composition;
        let mut pass = if self.extended_color_composition {
            PassManager::new_with_color_space(
                self.device.clone(),
                self.surface_format,
                wgpu::TextureFormat::Rgba16Float,
                self.web_srgb_composition,
            )
        } else {
            PassManager::new(self.device.clone(), self.surface_format)
        };
        self.pass.copy_external_textures_to(&mut pass);
        pass.set_scroll_offset(self.pass.scroll_offset());
        std::mem::swap(&mut self.pass.image_cache, &mut pass.image_cache);
        std::mem::swap(&mut self.pass.svg_cache, &mut pass.svg_cache);
        self.pass = pass;
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
        // Effect groups use the root's precision and blend space. A cached
        // float-target pass keeps its shader formats and transfer consistent.
        let mut group_pass = self.extended_group_pass.take().unwrap_or_else(|| {
            PassManager::new_with_color_space(
                self.device.clone(),
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureFormat::Rgba16Float,
                self.web_srgb_composition,
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
