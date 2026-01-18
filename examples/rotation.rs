use std::sync::Arc;
use winit::{event::WindowEvent, event_loop::EventLoop, window::Window};
use wgpu::*;
use textslabs::*;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut Application { state: None }).unwrap();
}

struct State {
    window: Arc<Window>,
    device: Device,
    queue: Queue,
    surface: Surface<'static>,
    surface_config: SurfaceConfiguration,
    text: Text,
    text_renderer: TextRenderer,
    text_edit_handle: TextEditHandle,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let instance = Instance::new(&InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
        let surface = instance.create_surface(window.clone()).unwrap();
        let surface_config = surface.get_default_config(&adapter, window.inner_size().width, window.inner_size().height).unwrap();
        surface.configure(&device, &surface_config);

        let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);
        let mut text = Text::new();
        let text_edit_handle = text.add_text_edit("Type here. Rotations after the cpu-side rasterization by just rotating the gpu quads, so this is not a high quality way to render highly dynamic text. If you just need to rotate text by 90 degrees, as in an axis label in a lot, it's good.".to_string(), (50.0, 50.0), (400.0, 200.0), 0.0);
        


        Self { device, queue, surface, surface_config, window, text, text_renderer, text_edit_handle }
    }
}

struct Application { state: Option<State> }

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_none() {
            let window = Arc::new(event_loop.create_window(
                Window::default_attributes()
            ).unwrap());
            window.set_ime_allowed(true);
            self.state = Some(State::new(window));
        }
    }

    fn window_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, _: winit::window::WindowId, event: WindowEvent) {
        let state = self.state.as_mut().unwrap();

        state.text.handle_event(&event, &state.window);
        
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                (state.surface_config.width, state.surface_config.height) = (size.width, size.height);
                state.surface.configure(&state.device, &state.surface_config);
            }
            WindowEvent::RedrawRequested => {

                let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            
                let period_seconds = 5.0;
                let rotation = ((t % period_seconds) / period_seconds * std::f64::consts::TAU) as f32;
                
                state.text.get_text_edit_mut(&state.text_edit_handle).set_transform(
                    Transform2D {
                        translation: (300.0, 300.0),
                        rotation,
                        scale: 1.0,
                    }
                );

                state.text.prepare_all(&mut state.text_renderer);
                state.text_renderer.load_to_gpu(&state.device, &state.queue);
        
                let surface_texture = state.surface.get_current_texture().unwrap();
                let mut encoder = state.device.create_command_encoder(&CommandEncoderDescriptor::default());
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &surface_texture.texture.create_view(&TextureViewDescriptor::default()),
                            resolve_target: None,
                            ops: Operations { load: LoadOp::Clear(Color::BLACK), store: StoreOp::Store },
                            depth_slice: None,
                        })],
                        ..Default::default()
                    });
                    state.text_renderer.render(&mut pass);
                }
        
                state.queue.submit(Some(encoder.finish()));
                surface_texture.present();

                state.window.request_redraw();
            },
            _ => {}
        }
    }
}
