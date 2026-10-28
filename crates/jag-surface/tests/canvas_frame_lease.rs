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

#[test]
fn canvas_lifetime_revokes_cached_precision_and_effect_bindings_but_preserves_peer() {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let (device, queue) = (Arc::new(device), Arc::new(queue));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("revocable frame"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &[255, 0, 0, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let mut surface = JagSurface::new(device.clone(), queue, wgpu::TextureFormat::Rgba8UnormSrgb);
    surface.set_frame_cache_enabled(false);
    let lifetime = Arc::new(());
    let count = Arc::new(AtomicUsize::new(0));
    let owner = Arc::new(Lease(count.clone()));
    let weak = Arc::downgrade(&owner);
    let id = ExternalTextureId(42);
    let peer = ExternalTextureId(43);
    surface
        .pass_manager()
        .register_linear_external_texture_with_lifetime(
            id,
            texture.create_view(&Default::default()),
            owner,
            Arc::downgrade(&lifetime),
        );
    surface
        .pass_manager()
        .register_linear_external_texture(peer, texture.create_view(&Default::default()));
    surface.enable_extended_color_composition();
    let mut other = PassManager::new(device, wgpu::TextureFormat::Rgba8UnormSrgb);
    surface.pass_manager().copy_external_textures_to(&mut other);
    let render = |surface: &mut JagSurface| {
        let mut canvas = surface.begin_frame(8, 8);
        canvas.clear(ColorLinPremul::rgba(255, 255, 255, 255));
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
        surface.end_frame_headless(canvas).unwrap().2
    };
    let before = render(&mut surface);
    assert!(before[0] > 240 && before[1] < 240);
    drop(lifetime);
    assert!(
        weak.upgrade().is_some(),
        "binding owners must not keep validity alive"
    );
    let after = render(&mut surface);
    assert_eq!(&after[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4], &[255; 4]);
    // Effects also register their output texture for the parent frame.
    surface.pass_manager().clear_compositor_textures();
    assert_eq!(surface.pass_manager().external_texture_count(), 1);
    assert!(
        weak.upgrade().is_some(),
        "copied registration still owns its in-flight lease"
    );
    assert_eq!(other.prune_expired_external_textures(), 1);
    assert_eq!(other.external_texture_count(), 1);
    assert!(weak.upgrade().is_none());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    other.remove_external_texture(peer);
    assert_eq!(other.external_texture_count(), 0);
    surface.pass_manager().remove_external_texture(peer);
    assert_eq!(surface.pass_manager().external_texture_count(), 0);
}
