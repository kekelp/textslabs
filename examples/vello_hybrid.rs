use parley::TextStyle;
use textslabs::*;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Scene};
use vello_common::{kurbo::{Circle, Shape}, paint::PaintType};
use peniko::color::AlphaColor;
use std::sync::Arc;
use wgpu::*;
use winit::{
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::EventLoop,
    window::Window,
};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application { state: None })
        .unwrap();
}

struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,
    window: Arc<Window>,

    text: Text,
    text_edit_handles: Vec<TextEditHandle>,

    scene: Scene,
    renderer: Renderer,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let instance = Instance::new(&InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();

        let surface = instance
            .create_surface(window.clone())
            .expect("Create surface");
        let surface_config = surface
            .get_default_config(&adapter, physical_size.width, physical_size.height)
            .unwrap();
        surface.configure(&device, &surface_config);

        let mut text = Text::new();

        // Create text style
        let white = [255, 255, 255, 255];
        let text_style_handle = text.add_style(TextStyle {
            font_size: 24.0,
            brush: ColorBrush(white),
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        }, None);

        // Create multiple text edit boxes with different text
        let mut text_edit_handles = Vec::new();

        let texts = vec![
            "ヘッケはこれらのL-函数が全複素平面へ有理型接続を持ち、指標が自明であるときZ = 1でオーダー1",
            "Мунди деленит молестиæ усу ад, перципиах глормату диссентиас",
            "Εσσεν οβρανώ δινιζι εν το δρονινη τού θεού και εν τώ καθ'ημών",
            "δις ναλιδισ ινγενθεμ υιριβυς ηασταμ ιν λατυς ινγυε φερι χυρυαμ χομπαγιβυσ αλυυμ χοντορσιτ.",
        ];

        let positions = [
            (100.0 +  50.0, 100.0 + 50.0),
            (100.0 + 200.0, 100.0 + 200.0),
            (100.0 + 350.0, 100.0 + 350.0),
            (100.0 + 500.0, 100.0 + 500.0),
        ];

        for (_i, (content, pos)) in texts.iter().zip(positions.iter()).enumerate() {
            let handle = text.add_text_edit(
                content.to_string(),
                *pos,
                (250.0, 150.0),
                0.0
            );
            text.get_text_edit_mut(&handle).set_style(&text_style_handle);
            text_edit_handles.push(handle);
        }

        let renderer = Renderer::new(
            &device,
            &RenderTargetConfig {
                format: surface_config.format,
                width: surface_config.width,
                height: surface_config.height,
            },
        );

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text,
            text_edit_handles,
            renderer,
            scene: Scene::new(physical_size.width as u16, physical_size.height as u16),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: WindowEvent,
    ) {
        self.text.handle_event(&event, &self.window);

        match event {
            WindowEvent::Resized(size) => {
                self.surface_config.width = size.width;
                self.surface_config.height = size.height;
                self.surface.configure(&self.device, &self.surface_config);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                self.scene.reset();

                // Define circle colors and positions
                let circles = [
                    (150.0, 150.0, 120.0, AlphaColor::from_rgba8(255, 100, 100, 200)), // Red
                    (300.0, 300.0, 120.0, AlphaColor::from_rgba8(100, 255, 100, 200)), // Green
                    (450.0, 450.0, 120.0, AlphaColor::from_rgba8(100, 100, 255, 200)), // Blue
                    (600.0, 600.0, 120.0, AlphaColor::from_rgba8(255, 100, 255, 200)), // Magenta
                ];

                // Draw circles and text alternating
                for (i, (cx, cy, radius, color)) in circles.iter().enumerate() {
                    // Draw circle
                    let circle = Circle::new((*cx, *cy), *radius);
                    self.scene.set_paint(PaintType::Solid(*color));
                    self.scene.fill_path(&circle.to_path(0.1));

                    // Draw corresponding text box
                    if i < self.text_edit_handles.len() {
                        let handle = &self.text_edit_handles[i];
                        self.text.get_text_edit_mut(handle).render_to_scene(&mut self.scene);
                    }
                }

                self.text.clear_changes();

                // Render to screen
                let surface_texture = self.surface.get_current_texture().unwrap();
                let view = surface_texture.texture.create_view(&TextureViewDescriptor::default());
                let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor { label: None });

                let render_size = RenderSize {
                    width: self.surface_config.width,
                    height: self.surface_config.height,
                };
                self.renderer.render(&self.scene, &self.device, &self.queue, &mut encoder, &render_size, &view).unwrap();

                self.queue.submit([encoder.finish()]);
                surface_texture.present();
                self.window.request_redraw();
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}

struct Application {
    state: Option<State>,
}

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let (width, height) = (800, 600);
        let window_attributes = Window::default_attributes()
            .with_inner_size(LogicalSize::new(width as f64, height as f64))
            .with_title("hello world");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        window.set_ime_allowed(true);

        self.state = Some(State::new(window));
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let Some(state) = &mut self.state {
            state.window_event(event_loop, event);
        };
    }
}
