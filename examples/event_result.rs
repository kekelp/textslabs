use parley::TextStyle;
use parley2::*;
use std::{sync::Arc, time::Instant};
use wgpu::*;
use winit::{
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::EventLoop,
    window::Window,
};

fn main() {
    println!("EventResult Test Example Starting...");
    println!("- Type in the text edit to see events being consumed");
    println!("- Move mouse around to see events NOT being consumed");
    println!("- Watch render statistics every second");
    
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
    text: Text,

    _text_edit: TextEditHandle,
    _text_box: TextBoxHandle,
    
    // Track render statistics
    last_render_time: Instant,
    render_count: u32,
    frame_skip_count: u32,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
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

        let mut text_renderer = TextRenderer::new(&device, &queue, surface_format);

        let mut text = Text::new();
        
        let text_edit = text.add_text_edit("Type here to test EventResult!".to_string(), (50.0, 50.0), (400.0, 30.0), 0.0,);
        let text_box = text.add_text_box("This text box shows EventResult in action. Watch the console for render statistics!", (50.0, 120.0), (500.0, 100.0), 0.0,);

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
            last_render_time: Instant::now(),
            render_count: 0,
            frame_skip_count: 0,
        }
    }

    fn handle_window_event(&mut self, event: &WindowEvent) -> bool {
        // Handle the event and get the result
        let result = self.text.handle_event(event, &self.window);
        
        // Log all events for debugging
        match event {
            WindowEvent::CursorMoved { .. } => {
                // Don't log cursor moves, too noisy
            }
            _ => {
                println!("Event: {:?} -> consumed: {}, need_rerender: {}", 
                    std::mem::discriminant(event), result.consumed, result.need_rerender);
            }
        }
        
        // Return whether a redraw is needed
        result.need_rerender
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
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
                        load: LoadOp::Clear(Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
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

        // Update render statistics
        self.render_count += 1;
        let now = Instant::now();
        if now.duration_since(self.last_render_time).as_secs() >= 1 {
            println!(
                "Rendered {} frames, skipped {} frames in the last second",
                self.render_count, self.frame_skip_count
            );
            self.render_count = 0;
            self.frame_skip_count = 0;
            self.last_render_time = now;
        }

        Ok(())
    }

    fn resize(&mut self, new_size: LogicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
            self.text_renderer
                .update_resolution(new_size.width as f32, new_size.height as f32);
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

        let window_attributes = Window::default_attributes()
            .with_title("EventResult Test - Selective Rendering")
            .with_inner_size(LogicalSize::new(800, 600));
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        self.state = Some(State::new(window));
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let state = self.state.as_mut().unwrap();
        
        let mut should_redraw = false;

        match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(physical_size) => {
                let logical_size = LogicalSize::new(physical_size.width, physical_size.height);
                state.resize(logical_size);
                should_redraw = true;
            }
            WindowEvent::RedrawRequested => {
                // Always prepare text when redraw is requested
                state.text.prepare_all(&mut state.text_renderer);
                
                // Handle the redraw event and check if we need to keep redrawing
                let result = state.text.handle_event(&event, &state.window);
                should_redraw = result.need_rerender;
                
                // Render
                match state.render() {
                    Ok(()) => {}
                    Err(wgpu::SurfaceError::Lost) => {
                        let logical_size = LogicalSize::new(
                            state.surface_config.width,
                            state.surface_config.height,
                        );
                        state.resize(logical_size);
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        event_loop.exit();
                    }
                    Err(e) => {
                        eprintln!("{:?}", e);
                    }
                }
            }
            _ => {
                // Handle other events through the text system
                should_redraw = state.handle_window_event(&event);
            }
        }

        // Only request redraw if EventResult says we need to  
        if should_redraw {
            state.window.request_redraw();
        } else {
            // Count skipped frames for statistics
            state.frame_skip_count += 1;
        }
    }
}