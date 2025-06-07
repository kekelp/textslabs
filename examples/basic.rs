use parley::TextStyle;
use parley2::*;
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

    text_renderer: TextRenderer,
    text_boxes: Vec<TextBox<String>>,
    static_text_boxes: Vec<TextBox<&'static str>>,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let instance = Instance::new(InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default(), None)).unwrap();
        let surface = instance
            .create_surface(window.clone())
            .expect("Create surface");
        let surface_config = surface
            .get_default_config(&adapter, physical_size.width, physical_size.height)
            .unwrap();
        surface.configure(&device, &surface_config);

        let big_text_style: SharedStyle = SharedStyle::new(TextStyle {
            font_size: 64.0,
            brush: ColorBrush([255,0,0,255]),
            ..Default::default()
        });

        let mut text_boxes = vec![
            TextBox::new("Text box".to_string(), (10.0, 15.0), (100.0, 50.0), 0.0, true),
            TextBox::new("Saddy (rare) ".to_string(), (100.0, 200.0), (400.0, 200.0), 0.0, true),
            TextBox::new("Words words words ".to_string(), (20.0, 20.0), (100.0, 50.0), 0.0, false),
            TextBox::new(
                "Amogus (non selectable)".to_string(),
                (10.0, 110.0),
                (100.0, 50.0),
                0.0,
                false,
            ),
        ];
        text_boxes[3].set_selectable(false);

        text_boxes[1].set_shared_style(&big_text_style);
        text_boxes[2].set_shared_style(&big_text_style);
        text_boxes[3].set_unique_style(TextStyle {
            font_size: 24.0,
            ..Default::default()
        });

        big_text_style.with_borrow_mut(|style| style.font_size = 32.0);

        let mut static_text_boxes = vec![
            TextBox::new("&'static str", (400.0, 500.0), (100.0, 50.0), 0.0, false),
            TextBox::new("Static words ", (200.0, 400.0), (100.0, 50.0), 0.0, false),
        ];
        static_text_boxes[1].set_shared_style(&big_text_style);


        let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_renderer,
            text_boxes,
            static_text_boxes,
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: WindowEvent,
    ) {
        let mut already_grabbed = false;
        for text_box in &mut self.text_boxes {
            if text_box.handle_event(&event, &self.window, already_grabbed) {
                already_grabbed = true;
            }
        }
        
        for text_box in &mut self.static_text_boxes {
            if text_box.static_handle_event(&event, already_grabbed) {
                already_grabbed = true;
            }
        }

        match event {
            WindowEvent::Resized(size) => {
                self.surface_config.width = size.width;
                self.surface_config.height = size.height;
                self.surface.configure(&self.device, &self.surface_config);
                self.text_renderer.update_resolution(size.width as f32, size.height as f32);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let frame = self.surface.get_current_texture().unwrap();
                let view = frame.texture.create_view(&TextureViewDescriptor::default());

                self.text_renderer.clear();
                for text_box in &mut self.text_boxes {
                    self.text_renderer.prepare_text_box(text_box);
                }
                for text_box in &mut self.static_text_boxes {
                    self.text_renderer.prepare_text_box(text_box);
                }
                self.text_renderer.gpu_load(&self.device, &self.queue);

                let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor { label: None });
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::GREEN),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });

                    self.text_renderer.render(&mut pass);
                }

                self.queue.submit(Some(encoder.finish()));
                frame.present();

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
