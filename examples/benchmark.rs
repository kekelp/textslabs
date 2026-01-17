use std::{sync::Arc, time::{Duration, Instant}};
use winit::{event::WindowEvent, event_loop::EventLoop, window::Window};
use wgpu::*;
use textslabs::*;
use wgpu_profiler::{GpuProfiler, GpuProfilerSettings};

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
    _text_edit_handles: Vec<TextEditHandle>,
    first_frame_stats: TextBoxHandle,
    avg_stats: TextBoxHandle,
    gpu_profiler: GpuProfiler,
    frame_count: u32,
    last_print_time: Instant,
    total_prepare_time: Duration,
    total_render_time: Duration,
    total_frame_time: Duration,
    first_frame: bool,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let instance = Instance::new(&InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let device_desc = DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&device_desc)).unwrap();
        let surface = instance.create_surface(window.clone()).unwrap();
        let mut surface_config = surface.get_default_config(&adapter, window.inner_size().width, window.inner_size().height).unwrap();
        surface_config.present_mode = PresentMode::Immediate;
        surface.configure(&device, &surface_config);

        let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);
        let mut text = Text::new();

        // Create stats display text boxes at the top
        let first_frame_stats = text.add_text_box(
            "First frame: ...".to_string(),
            (10.0, 10.0),
            (400.0, 100.0),
            0.0
        );

        let avg_stats = text.add_text_box(
            "Average: computing...".to_string(),
            (420.0, 10.0),
            (400.0, 100.0),
            0.0
        );

        // Create multiple text boxes in a grid layout (moved down to make room for stats)
        let mut text_edit_handles = Vec::new();
        let rows = 10;
        let cols = 5;
        let box_width = 150.0;
        let box_height = 60.0;
        let spacing_x = 150.0;
        let spacing_y = 60.0;
        let start_x = 10.0;
        let start_y = 220.0;

        for row in 0..rows {
            for col in 0..cols {
                let x = start_x + col as f64 * spacing_x;
                let y = start_y + row as f64 * spacing_y;
                let handle = text.add_text_edit(
                    format!("Text box {},{}", row, col),
                    (x, y),
                    (box_width, box_height),
                    0.0
                );
                text_edit_handles.push(handle);
            }
        }

        let gpu_profiler = GpuProfiler::new(&device, GpuProfilerSettings {
            enable_timer_queries: true,
            enable_debug_groups: true,
            max_num_pending_frames: 3,
        }).unwrap();

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text,
            text_renderer,
            _text_edit_handles: text_edit_handles,
            first_frame_stats,
            avg_stats,
            gpu_profiler,
            frame_count: 0,
            last_print_time: Instant::now(),
            total_prepare_time: Duration::ZERO,
            total_render_time: Duration::ZERO,
            total_frame_time: Duration::ZERO,
            first_frame: true,
        }
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
                let frame_start = Instant::now();

                // Measure prepare time
                let prepare_start = Instant::now();
                state.text.prepare_all(&mut state.text_renderer);
                let prepare_time = prepare_start.elapsed();
                state.total_prepare_time += prepare_time;

                // Load 
                state.text_renderer.load_to_gpu(&state.device, &state.queue);

                let surface_texture = state.surface.get_current_texture().unwrap();
                let mut encoder = state.device.create_command_encoder(&CommandEncoderDescriptor::default());

                let query = state.gpu_profiler.begin_query("Render", &mut encoder);

                // Measure render pass time
                let render_start = Instant::now();
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
                let render_time = render_start.elapsed();
                state.total_render_time += render_time;

                state.gpu_profiler.end_query(&mut encoder, query);
                state.gpu_profiler.resolve_queries(&mut encoder);

                state.queue.submit(Some(encoder.finish()));

                state.gpu_profiler.end_frame().unwrap();

                if let Some(profiling_data) = state.gpu_profiler.process_finished_frame(state.queue.get_timestamp_period()) {
                    for p in profiling_data {
                        if let Some(time) = p.time {
                            let dur = Duration::from_secs_f64(time.end - time.start);
                            println!("GPU time ({}): {:?}", p.label, dur);
                        }
                    }
                }

                surface_texture.present();

                let frame_time = frame_start.elapsed();

                // Update first frame stats
                if state.first_frame {
                    let stats_text = format!(
                        "First frame:\nPrepare: {:?}\nRender:  {:?}\nFrame:   {:?}",
                        prepare_time, render_time, frame_time
                    );
                    *state.text.get_text_box_mut(&state.first_frame_stats).text_mut() = stats_text.into();
                    state.first_frame = false;
                }

                state.total_frame_time += frame_time;
                state.frame_count += 1;

                // Update average statistics every second
                if state.last_print_time.elapsed() >= Duration::from_secs(1) {
                    let avg_prepare = state.total_prepare_time / state.frame_count;
                    let avg_render = state.total_render_time / state.frame_count;
                    let avg_frame = state.total_frame_time / state.frame_count;
                    let fps = state.frame_count as f64 / state.last_print_time.elapsed().as_secs_f64();

                    let stats_text = format!(
                        "Average ({} frames):\nFPS: {:.1}\nPrepare: {:?}\nRender:  {:?}\nFrame:   {:?}",
                        state.frame_count, fps, avg_prepare, avg_render, avg_frame
                    );
                    *state.text.get_text_box_mut(&state.avg_stats).text_mut() = stats_text.into();

                    // Reset counters
                    state.frame_count = 0;
                    state.last_print_time = Instant::now();
                    state.total_prepare_time = Duration::ZERO;
                    state.total_render_time = Duration::ZERO;
                    state.total_frame_time = Duration::ZERO;
                }

                state.window.request_redraw();
            },
            _ => {}
        }
    }
}
