// This example shows how to integrate the library with a smarter event loop in applications that pause their event loops when nothing is happening.
//
// Usually, this just means checking the result of `Text::handle_event()`, and calling `Window::request_redraw()` only if `result.need_rerender` is true.
// This covers normal updates, as well as smooth scroll animations.
// 
// For cursor blinking, winit supports a `ControlFlow::WaitUntil` mode that should be ideal for this, but I couldn't get it to work. Instead, for the moment, another method is supported:
//
// - Create an event `EventLoopProxy<T>` for your winit event loop
// - Create the text struct with the `Text::with_event_loop_waker()` function, passing in the event loop proxy, as well as the value of a custom event.
// - in winit's ApplicationHandler, implement `user_event()` and make it call `redraw_requested()` when receiving the custom event passed before.
// 
// The text struct will spawn a thread that will wake up the event loop when needed.


use textslabs::*;
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
    let event_proxy = event_loop.create_proxy();
    
    event_loop
        .run_app(&mut Application { 
            state: None, 
            event_proxy,
        })
        .unwrap();
}

struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,
    window: Arc<Window>,

    text_renderer: TextRenderer,
    text: Text,

    _text_edit: TextEditHandle,
    _text_box: TextBoxHandle,
}

impl State {
    fn new(window: Arc<Window>, event_proxy: winit::event_loop::EventLoopProxy<()>) -> Self {
        let physical_size = window.inner_size();
        let instance = Instance::new(InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .unwrap();

        let (device, queue) = pollster::block_on(adapter.request_device(
            &DeviceDescriptor {
                required_features: Features::empty(),
                required_limits: Limits::default(),
                label: None,
                memory_hints: MemoryHints::default(),
            },
            None,
        ))
        .unwrap();

        let surface = instance.create_surface(window.clone()).unwrap();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: physical_size.width,
            height: physical_size.height,
            present_mode: PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        let text_renderer = TextRenderer::new(&device, &queue, surface_format);

        let wakeup_event_value = ();
        let mut text = Text::new_with_blink_wakeup(event_proxy, wakeup_event_value);
        
        let text_edit = text.add_text_edit("This is a text edit box with a bunch of text that can be scrolled. Use the mouse wheel to get a smooth scroll animation. And you can check the console output to see that we're only rerendering when needed.".to_string(), (50.0, 50.0), (400.0, 80.0), 0.0,);
        let text_box = text.add_text_box("This is a regular non-editable text box.", (50.0, 180.0), (500.0, 120.0), 0.0,);

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_renderer,
            text,
            _text_edit: text_edit,
            _text_box: text_box,
        }
    }

    fn render(&mut self) {
        println!("Rerender at {:?}", std::time::Instant::now());
        
        self.text.prepare_all(&mut self.text_renderer);
        self.text_renderer.gpu_load(&self.device, &self.queue);

        let output = self.surface.get_current_texture().unwrap();
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::BLUE),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.text_renderer.render(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    fn resize(&mut self, new_size: LogicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
            self.text_renderer.update_resolution(new_size.width as f32, new_size.height as f32);
        }
    }
}

struct Application {
    state: Option<State>,
    event_proxy: winit::event_loop::EventLoopProxy<()>,
}

impl winit::application::ApplicationHandler<()> for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attributes = Window::default_attributes()
            .with_title("Smart render loop")
            .with_inner_size(LogicalSize::new(800, 600));
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        self.state = Some(State::new(window, self.event_proxy.clone()));
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let state = self.state.as_mut().unwrap();
        let result = state.text.handle_event(&event, &state.window);

        if event == winit::event::WindowEvent::RedrawRequested {
            _ = state.render();
        }

        match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(physical_size) => {
                let logical_size = LogicalSize::new(physical_size.width, physical_size.height);
                state.resize(logical_size);
            }
            _ => {}
        }

        if result.need_rerender {
            state.window.request_redraw();
        }

    }

    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, _event: ()) {
        if let Some(state) = &mut self.state {
            // If we were using user events for other things, we would do this only when _event and matches the wakeup value that we passed to Text::new().
            state.window.request_redraw();
        }
    }
}
