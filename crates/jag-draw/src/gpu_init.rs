use std::sync::Arc;

use thiserror::Error;

use crate::wgpu;

#[derive(Debug, Error)]
pub enum DesktopGpuError {
    #[error("desktop GPU initialization is unsupported on this target")]
    UnsupportedPlatform,
    #[error("failed to create the desktop presentation surface: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("no native GPU adapter supports the desktop presentation surface")]
    AdapterUnavailable,
    #[error("failed to create the desktop GPU device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("surface capabilities contain no {0}")]
    MissingCapability(&'static str),
    #[error("surface dimensions must be nonzero, got {width}x{height}")]
    ZeroSize { width: u32, height: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceResize {
    Suspended,
    Unchanged,
    Reconfigured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceSelection {
    format: wgpu::TextureFormat,
    present_mode: wgpu::PresentMode,
    alpha_mode: wgpu::CompositeAlphaMode,
}

pub struct DesktopGpu<'window> {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'window>,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    config: wgpu::SurfaceConfiguration,
}

impl<'window> DesktopGpu<'window> {
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'window>>,
        width: u32,
        height: u32,
    ) -> Result<Self, DesktopGpuError> {
        let backends = desktop_backends();
        if backends.is_empty() {
            return Err(DesktopGpuError::UnsupportedPlatform);
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let surface = instance.create_surface(target)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .ok_or(DesktopGpuError::AdapterUnavailable)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await?;
        let config = try_make_surface_config(&adapter, &surface, width, height)?;
        surface.configure(&device, &config);
        Ok(Self {
            instance,
            surface,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            config,
        })
    }

    pub fn surface(&self) -> &wgpu::Surface<'window> {
        &self.surface
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn device(&self) -> Arc<wgpu::Device> {
        self.device.clone()
    }

    pub fn queue(&self) -> Arc<wgpu::Queue> {
        self.queue.clone()
    }

    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    pub fn resize(&mut self, width: u32, height: u32) -> SurfaceResize {
        let outcome = update_surface_size(&mut self.config, width, height);
        if outcome == SurfaceResize::Reconfigured {
            self.surface.configure(&self.device, &self.config);
        }
        outcome
    }

    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }
}

pub fn desktop_backends() -> wgpu::Backends {
    #[cfg(target_os = "macos")]
    return wgpu::Backends::METAL;
    #[cfg(target_os = "windows")]
    return wgpu::Backends::DX12;
    #[cfg(target_os = "linux")]
    return wgpu::Backends::VULKAN;
    #[allow(unreachable_code)]
    wgpu::Backends::empty()
}

pub fn try_make_surface_config(
    adapter: &wgpu::Adapter,
    surface: &wgpu::Surface,
    width: u32,
    height: u32,
) -> Result<wgpu::SurfaceConfiguration, DesktopGpuError> {
    surface_config_from_capabilities(&surface.get_capabilities(adapter), width, height)
}

pub fn choose_srgb_surface_format(
    adapter: &wgpu::Adapter,
    surface: &wgpu::Surface,
) -> wgpu::TextureFormat {
    select_surface_format(&surface.get_capabilities(adapter))
        .expect("surface must expose at least one format")
}

pub fn make_surface_config(
    adapter: &wgpu::Adapter,
    surface: &wgpu::Surface,
    width: u32,
    height: u32,
) -> wgpu::SurfaceConfiguration {
    try_make_surface_config(adapter, surface, width, height)
        .expect("surface must support a nonzero presentation configuration")
}

pub fn update_surface_size(
    config: &mut wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
) -> SurfaceResize {
    if width == 0 || height == 0 {
        return SurfaceResize::Suspended;
    }
    if config.width == width && config.height == height {
        return SurfaceResize::Unchanged;
    }
    config.width = width;
    config.height = height;
    SurfaceResize::Reconfigured
}

fn surface_config_from_capabilities(
    capabilities: &wgpu::SurfaceCapabilities,
    width: u32,
    height: u32,
) -> Result<wgpu::SurfaceConfiguration, DesktopGpuError> {
    if width == 0 || height == 0 {
        return Err(DesktopGpuError::ZeroSize { width, height });
    }
    let selected = select_capabilities(capabilities)?;
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: selected.format,
        width,
        height,
        present_mode: selected.present_mode,
        alpha_mode: selected.alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 1,
    })
}

fn select_capabilities(
    capabilities: &wgpu::SurfaceCapabilities,
) -> Result<SurfaceSelection, DesktopGpuError> {
    let format = select_surface_format(capabilities)?;
    let present_mode = capabilities
        .present_modes
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::PresentMode::Fifo)
        .or_else(|| capabilities.present_modes.first().copied())
        .ok_or(DesktopGpuError::MissingCapability("present modes"))?;
    let alpha_mode = capabilities
        .alpha_modes
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
        .or_else(|| capabilities.alpha_modes.first().copied())
        .ok_or(DesktopGpuError::MissingCapability("alpha modes"))?;
    Ok(SurfaceSelection {
        format,
        present_mode,
        alpha_mode,
    })
}

fn select_surface_format(
    capabilities: &wgpu::SurfaceCapabilities,
) -> Result<wgpu::TextureFormat, DesktopGpuError> {
    capabilities
        .formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .or_else(|| capabilities.formats.first().copied())
        .ok_or(DesktopGpuError::MissingCapability("formats"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats: vec![
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            ],
            present_modes: vec![wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo],
            alpha_modes: vec![
                wgpu::CompositeAlphaMode::Inherit,
                wgpu::CompositeAlphaMode::Opaque,
            ],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
        }
    }

    #[test]
    fn surface_policy_prefers_srgb_fifo_and_opaque() {
        let config = surface_config_from_capabilities(&capabilities(), 800, 600).unwrap();
        assert_eq!(config.format, wgpu::TextureFormat::Bgra8UnormSrgb);
        assert_eq!(config.present_mode, wgpu::PresentMode::Fifo);
        assert_eq!(config.alpha_mode, wgpu::CompositeAlphaMode::Opaque);
        assert_eq!((config.width, config.height), (800, 600));
    }

    #[test]
    fn surface_policy_rejects_missing_capabilities_and_zero_size() {
        for (field, clear) in [("formats", 0), ("present modes", 1), ("alpha modes", 2)] {
            let mut caps = capabilities();
            match clear {
                0 => caps.formats.clear(),
                1 => caps.present_modes.clear(),
                2 => caps.alpha_modes.clear(),
                _ => unreachable!(),
            }
            assert!(matches!(
                surface_config_from_capabilities(&caps, 800, 600),
                Err(DesktopGpuError::MissingCapability(missing)) if missing == field
            ));
        }
        assert!(matches!(
            surface_config_from_capabilities(&capabilities(), 0, 600),
            Err(DesktopGpuError::ZeroSize {
                width: 0,
                height: 600
            })
        ));
    }

    #[test]
    fn resize_suspends_at_zero_and_reconfigures_only_on_change() {
        let mut config = surface_config_from_capabilities(&capabilities(), 800, 600).unwrap();
        assert_eq!(
            update_surface_size(&mut config, 0, 600),
            SurfaceResize::Suspended
        );
        assert_eq!((config.width, config.height), (800, 600));
        assert_eq!(
            update_surface_size(&mut config, 800, 600),
            SurfaceResize::Unchanged
        );
        assert_eq!(
            update_surface_size(&mut config, 1024, 768),
            SurfaceResize::Reconfigured
        );
        assert_eq!((config.width, config.height), (1024, 768));
    }

    #[test]
    fn desktop_backend_is_exact_for_the_host() {
        #[cfg(target_os = "macos")]
        assert_eq!(desktop_backends(), wgpu::Backends::METAL);
        #[cfg(target_os = "windows")]
        assert_eq!(desktop_backends(), wgpu::Backends::DX12);
        #[cfg(target_os = "linux")]
        assert_eq!(desktop_backends(), wgpu::Backends::VULKAN);
    }
}
