use parley_atlas_renderer::*;
use std::sync::Arc;
use parley::*;
use wgpu::*;
use winit::{dpi::LogicalSize, event::WindowEvent, event_loop::EventLoop, window::Window};

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
    text_boxes: Vec<TextBox>,
    current_text: usize,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let instance = Instance::new(&InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
        let surface = instance
            .create_surface(window.clone())
            .expect("Create surface");
        let swapchain_format = TextureFormat::Bgra8Unorm;
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: physical_size.width,
            height: physical_size.height,
            present_mode: PresentMode::Fifo,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let text_boxes = vec![
            TextBox::new("Text box???????????????????????????????????????????????????????".to_string()),
            TextBox::new("Saddy".to_string()),
            TextBox::new("Okayeg".to_string()),
        ];

        let text_renderer_params = TextRendererParams {
            atlas_page_size: AtlasPageSize::Flat(300), // tiny page to test out multi-page stuff
        };
        let text_renderer = TextRenderer::new_with_params(&device, &queue, text_renderer_params);

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_renderer,
            text_boxes,
            current_text: 0,
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowLeft) = event.logical_key {
                    if event.state.is_pressed() && !event.repeat {
                        let len = self.text_boxes.len();
                        self.current_text = (self.current_text + len - 1) % len;
                        self.window.request_redraw();
                    }
                }
                if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowRight) = event.logical_key {
                    if event.state.is_pressed() && ! event.repeat {
                        self.current_text = (self.current_text + 1) % self.text_boxes.len();
                        self.window.request_redraw();
                    }
                }
            }
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
                    self.text_renderer.prepare_layout(text_box.layout());
                    // self.text_renderer.prepare_layout(&rich_layout());
                    dbg!(&text_box.text);
                }
                self.text_renderer.gpu_load(&self.device, &self.queue);

                let mut encoder = self
                    .device
                    .create_command_encoder(&CommandEncoderDescriptor { label: None });
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::GREEN),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });

                    self.text_renderer.render(&mut pass);
                }

                self.queue.submit(Some(encoder.finish()));
                frame.present();
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

        // Set up window
        let (width, height) = (800, 600);
        let window_attributes = Window::default_attributes()
            .with_inner_size(LogicalSize::new(width as f64, height as f64))
            .with_title("hello world");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

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
