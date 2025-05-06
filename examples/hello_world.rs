use parley_atlas_renderer::*;
use image::Rgba;
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
    text_layout: Layout<ColorBrush>,
    show_atlas: bool,
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

        let layout = text_layout();

        let text_renderer_params = TextRendererParams {
            atlas_page_size: AtlasPageSize::Flat(256), // tiny page to test out multi-page stuff
        };
        let text_renderer = TextRenderer::new_with_params(&device, &queue, text_renderer_params);

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_renderer,
            text_layout: layout,
            show_atlas: false,
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if let winit::keyboard::Key::Named(winit::keyboard::NamedKey::F1) = event.logical_key {
                    if event.state.is_pressed() && ! event.repeat {
                        self.window.request_redraw();
                        self.show_atlas = ! self.show_atlas;
                    }
                }
            }
            WindowEvent::Resized(size) => {
                self.surface_config.width = size.width;
                self.surface_config.height = size.height;
                self.surface.configure(&self.device, &self.surface_config);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                // todo: move
                self.text_renderer.text_renderer.params.screen_resolution = Resolution {
                    width: self.surface_config.width as f32,
                    height: self.surface_config.height as f32,
                };

                let frame = self.surface.get_current_texture().unwrap();
                let view = frame.texture.create_view(&TextureViewDescriptor::default());

                let now = std::time::Instant::now();
                self.text_renderer.clear();
                self.text_renderer.prepare_layout(&self.text_layout);
                println!("prepare(): {:?}", now.elapsed());

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

                    if self.show_atlas {
                        self.text_renderer.gpu_load_atlas_debug(&self.device, &self.queue);
                        self.text_renderer.render_atlas_debug(&mut pass);
                    } else {
                        self.text_renderer.render(&mut pass);
                    }
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

fn text_layout() -> Layout<ColorBrush> {
    let text = String::from("Press F1 to see the atlas
    Lorem ipsum\tdolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. O algo.
    ヘッケはこれらのL-函数が全複素平面へ有理型接続を持ち、指標が自明であるときには s = 1 でオーダー 1 である極を持ち、それ以外では解析的であることを証明した。原始ヘッケ指標（原始ディリクレ指標に同じ方法である modulus に相対的に定義された）に対し、ヘッケは、これらのL-函数が指標の L-函数の函数等式を満たし、L-函数の複素共役指標であることを示した。 主イデアル上の座と、無限での座を含む全ての例外有限集合の上で 1 である単円の上への写像を取ることで、イデール類群の指標 ψ を考える。すると、ψ はイデアル群 IS の指標 χ を生成し、イデアル群は S 上に入らない素イデアル上の自由アーベル群となる。"); // here1

    let display_scale = 1.0;

    let max_advance = Some(500.0 * display_scale);

    let text_color = Rgba([0, 0, 0, 255]);

    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();

    let text_brush = ColorBrush { color: text_color };
    let mut builder = layout_cx.ranged_builder(&mut font_cx, &text, display_scale);

    builder.push_default(StyleProperty::Brush(text_brush));

    builder.push_default(FontStack::from("system-ui"));
    builder.push_default(StyleProperty::LineHeight(0.5));
    builder.push_default(StyleProperty::FontSize(24.0));

    builder.push(StyleProperty::FontWeight(parley::FontWeight::new(600.0)), 0..30);

    // builder.push(StyleProperty::Underline(true), 141..150);
    // builder.push(StyleProperty::Strikethrough(true), 155..168);

    // builder.push_inline_box(InlineBox {
    //     id: 0,
    //     index: 40,
    //     width: 50.0,
    //     height: 50.0,
    // });
    // builder.push_inline_box(InlineBox {
    //     id: 1,
    //     index: 50,
    //     width: 50.0,
    //     height: 30.0,
    // });

    // Build the builder into a Layout
    // let mut layout: Layout<ColorBrush> = builder.build(&text);
    let mut layout: Layout<ColorBrush> = builder.build(&text);

    // Perform layout (including bidi resolution and shaping) with start alignment
    layout.break_all_lines(max_advance);
    layout.align(max_advance, Alignment::Start, AlignmentOptions::default());

    return layout;
}
