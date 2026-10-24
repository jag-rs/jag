//! Native frame ownership follows registrations across precision and effect passes.
use jag_draw::{ColorLinPremul, ExternalTextureId, PassManager, Rect, wgpu};
use jag_surface::JagSurface;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Lease(Arc<AtomicUsize>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn canvas_frame_lease_survives_copy_precision_and_effects_until_last_binding_release() {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let (device, queue) = (Arc::new(device), Arc::new(queue));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lease fixture"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut surface = JagSurface::new(device.clone(), queue, wgpu::TextureFormat::Rgba8UnormSrgb);
    let count = Arc::new(AtomicUsize::new(0));
    let owner = Arc::new(Lease(count.clone()));
    let weak = Arc::downgrade(&owner);
    let id = ExternalTextureId(42);
    surface
        .pass_manager()
        .register_linear_external_texture_with_owner(
            id,
            texture.create_view(&Default::default()),
            owner,
        );
    surface.enable_extended_color_composition();
    assert!(weak.upgrade().is_some());
    let mut other = PassManager::new(device, wgpu::TextureFormat::Rgba8UnormSrgb);
    surface.pass_manager().copy_external_textures_to(&mut other);
    let mut canvas = surface.begin_frame(8, 8);
    canvas.clear(ColorLinPremul::default());
    canvas.push_opacity(0.5);
    canvas.external_texture_with_params(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 8.0,
            h: 8.0,
        },
        id,
        0,
        1.0,
        true,
    );
    canvas.pop_opacity();
    surface.end_frame_headless(canvas).unwrap();
    surface.pass_manager().clear_external_textures();
    assert!(weak.upgrade().is_some());
    // Replacing a binding releases its old owner even if its view remains alive on GPU.
    other.register_linear_external_texture(id, texture.create_view(&Default::default()));
    assert!(weak.upgrade().is_none());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
