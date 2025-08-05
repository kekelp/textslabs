use textslabs::*;
use std::sync::Arc;
use wgpu::*;
use winit::{
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application { state: None })
        .unwrap();
}

struct WindowState {
    surface: wgpu::Surface<'static>,
    _surface_config: SurfaceConfiguration,
    window: Arc<Window>,
    text_renderer: TextRenderer,
}

struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    text: Text,
    windows: Vec<WindowState>,
}

impl State {
    fn new(windows: Vec<Arc<Window>>) -> Self {
        let physical_size = windows[0].inner_size();
        let instance = Instance::new(InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default(), None)).unwrap();
        
        let mut text = Text::new_without_auto_wakeup();
        
        let mut window_states = Vec::new();
        for (i, window) in windows.into_iter().enumerate() {
            let window_id = window.id();
            text.register_window(window_id);
            
            let surface = instance.create_surface(window.clone()).expect("Create surface");
            let surface_config = surface.get_default_config(&adapter, physical_size.width, physical_size.height).unwrap();
            surface.configure(&device, &surface_config);
            
            let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);
            
            // Create window-specific text
            let _handle = text.add_text_edit_for_window(
                format!("Window {} text", i + 1),
                (50.0, 50.0),
                (400.0, 100.0),
                0.0,
                window_id
            );
            
            window_states.push(WindowState {
                surface,
                _surface_config: surface_config,
                window,
                text_renderer,
            });
        }

        Self { device, queue, text, windows: window_states }
    }
}

struct Application {
    state: Option<State>,
}

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_some() { return; }

        let window1 = Arc::new(event_loop.create_window(
            Window::default_attributes()
                .with_inner_size(LogicalSize::new(600, 400))
                .with_title("Window 1")
        ).unwrap());
        
        let window2 = Arc::new(event_loop.create_window(
            Window::default_attributes()
                .with_inner_size(LogicalSize::new(600, 400))
                .with_title("Window 2")
        ).unwrap());

        self.state = Some(State::new(vec![window1, window2]));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        
        let state = self.state.as_mut().unwrap();

        let window_index = state.windows.iter().position(|w| w.window.id() == window_id);
        
        if let Some(window_index) = window_index {
            let window_state = &mut state.windows[window_index];
            state.text.handle_event_for_window(&event, &window_state.window, window_id);
            match event {
                WindowEvent::RedrawRequested => {
                    
                    state.text.prepare_all_for_window(&mut window_state.text_renderer, window_id);
                    window_state.text_renderer.load_to_gpu(&state.device, &state.queue);

                    let surface_texture = window_state.surface.get_current_texture().unwrap();
                    let view = surface_texture.texture.create_view(&TextureViewDescriptor::default());
                    let mut encoder = state.device.create_command_encoder(&CommandEncoderDescriptor { label: None });
                    {
                        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                            color_attachments: &[Some(RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: Operations {
                                    load: LoadOp::Clear(wgpu::Color::BLACK),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            ..Default::default()
                        });
                        window_state.text_renderer.render(&mut pass);
                    }
                    state.queue.submit(Some(encoder.finish()));
                    surface_texture.present();
                    window_state.window.request_redraw();
                }
                WindowEvent::CloseRequested => {
                    event_loop.exit()
                }
                _ => {}
            }
        }
        
    }
}