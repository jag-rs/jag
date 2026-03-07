//! Minimal jag demo: proves the full pipeline works end-to-end.
//!
//! Window -> GPU init -> UI elements -> layout -> paint -> render -> display.

use std::sync::Arc;

use jag_draw::{
    Brush, ColorLinPremul, DetirTextProvider, Rect, SubpixelOrientation, make_surface_config,
};
use jag_surface::JagSurface;
use jag_ui::elements::{Button, Checkbox, Element, Text};
use jag_ui::Theme;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::Window;

fn main() -> anyhow::Result<()> {
    // --- Window setup ---
    let event_loop = EventLoop::new()?;
    let window_attrs = Window::default_attributes().with_title("Jag Demo");
    #[allow(deprecated)]
    let window = event_loop.create_window(window_attrs)?;
    let window: &'static Window = Box::leak(Box::new(window));

    // --- GPU init ---
    let instance = jag_draw::wgpu::Instance::default();
    let surface = instance.create_surface(window)?;
    let adapter = pollster::block_on(
        instance.request_adapter(&jag_draw::wgpu::RequestAdapterOptions {
            power_preference: jag_draw::wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }),
    )
    .expect("No suitable GPU adapter found");

    let (device, queue) = pollster::block_on(
        adapter.request_device(&jag_draw::wgpu::DeviceDescriptor::default(), None),
    )?;

    let mut size = window.inner_size();
    let scale_factor = window.scale_factor() as f32;
    let mut config = make_surface_config(&adapter, &surface, size.width, size.height);
    surface.configure(&device, &config);

    // --- JagSurface wrapper ---
    let mut surf = JagSurface::new(Arc::new(device), Arc::new(queue), config.format);
    surf.set_use_intermediate(true);
    surf.set_direct(true);
    surf.set_logical_pixels(true);
    surf.set_dpi_scale(scale_factor);

    // --- Text provider (system fonts) ---
    let text_provider: Arc<dyn jag_draw::TextProvider + Send + Sync> = Arc::new(
        DetirTextProvider::from_system_fonts(SubpixelOrientation::RGB)
            .expect("Failed to load system fonts"),
    );

    // --- Build UI elements ---
    let theme = Theme::default();

    // Event loop
    #[allow(deprecated)]
    event_loop.run(move |event, elwt| match event {
        Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
            WindowEvent::CloseRequested => elwt.exit(),
            WindowEvent::Resized(new_size) => {
                size = new_size;
                if size.width > 0 && size.height > 0 {
                    config.width = size.width;
                    config.height = size.height;
                    surface.configure(surf.device().as_ref(), &config);
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                let frame = match surface.get_current_texture() {
                    Ok(f) => f,
                    Err(_) => {
                        window.request_redraw();
                        return;
                    }
                };

                let mut canvas = surf.begin_frame(size.width, size.height);
                let bg = ColorLinPremul::from_srgba_u8([26, 31, 51, 255]);
                canvas.clear(bg);
                canvas.set_text_provider(text_provider.clone());

                let vp_w = size.width as f32;
                let _vp_h = size.height as f32;

                // Fill background
                canvas.fill_rect(0.0, 0.0, vp_w, _vp_h, Brush::Solid(bg), 0);

                // --- Title ---
                let mut title = Text::new("Jag Demo - UI Elements", 24.0);
                title.pos = [40.0, 60.0];
                title.render(&mut canvas, 10);

                // --- Buttons ---
                let mut y = 100.0;

                let mut btn1 = Button::with_theme("Primary Button", &theme);
                btn1.rect = Rect {
                    x: 40.0,
                    y,
                    w: 160.0,
                    h: 40.0,
                };
                btn1.render(&mut canvas, 10);

                let mut btn2 = Button::with_theme("Secondary", &theme);
                btn2.rect = Rect {
                    x: 220.0,
                    y,
                    w: 140.0,
                    h: 40.0,
                };
                btn2.bg = ColorLinPremul::from_srgba_u8([75, 85, 99, 255]);
                btn2.render(&mut canvas, 10);

                y += 60.0;

                // --- Checkboxes ---
                let mut cb1 = Checkbox::new();
                cb1.rect = Rect {
                    x: 40.0,
                    y,
                    w: 18.0,
                    h: 18.0,
                };
                cb1.label = Some("Unchecked".into());
                cb1.render(&mut canvas, 10);

                let mut cb2 = Checkbox::new();
                cb2.rect = Rect {
                    x: 200.0,
                    y,
                    w: 18.0,
                    h: 18.0,
                };
                cb2.checked = true;
                cb2.label = Some("Checked".into());
                cb2.render(&mut canvas, 10);

                y += 50.0;

                // --- More text ---
                let mut info = Text::new("Pipeline: Window -> GPU -> Canvas -> Elements -> Display", 14.0);
                info.pos = [40.0, y];
                info.color = ColorLinPremul::from_srgba_u8([156, 163, 175, 255]);
                info.render(&mut canvas, 10);

                // Present
                if let Err(e) = surf.end_frame(frame, canvas) {
                    eprintln!("end_frame error: {e}");
                }
            }
            _ => {}
        },
        Event::AboutToWait => {
            window.request_redraw();
        }
        _ => {}
    })?;

    Ok(())
}
