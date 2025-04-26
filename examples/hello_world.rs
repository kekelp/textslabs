use std::sync::Arc;
use image::Rgba;
use parley::{Alignment, AlignmentOptions, FontContext, FontStack, FontWeight, InlineBox, Layout, LayoutContext, StyleProperty, TextStyle};
use wgpu::{
    CommandEncoderDescriptor, CompositeAlphaMode, DeviceDescriptor, Instance, InstanceDescriptor,
    LoadOp, MultisampleState, Operations, PresentMode, RenderPassColorAttachment,
    RenderPassDescriptor, RequestAdapterOptions, SurfaceConfiguration, TextureFormat,
    TextureUsages, TextureViewDescriptor,
};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Scaler, Source, StrikeWith};
use swash::zeno;
use winit::{dpi::LogicalSize, event::WindowEvent, event_loop::EventLoop, window::Window};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application { window_state: None })
        .unwrap();
}

struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,

    // Make sure that the winit window is last in the struct so that
    // it is dropped after the wgpu surface is dropped, otherwise the
    // program may crash when closed. This is probably a bug in wgpu.
    window: Arc<Window>,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let scale_factor = window.scale_factor();

        // Set up surface
        let instance = Instance::new(&InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();

        let surface = instance
            .create_surface(window.clone())
            .expect("Create surface");
        let swapchain_format = TextureFormat::Bgra8UnormSrgb;
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

        let text_layout = text_layout();
    
        // The display scale for HiDPI rendering
        let display_scale = 1.0;
    
        // The width for line wrapping
        let max_advance = Some(200.0 * display_scale);
    
        // Colours for rendering
        let text_color = Rgba([0, 0, 0, 255]);
        let bg_color = Rgba([255, 255, 255, 255]);
    
        // Padding around the output image
        let padding = 20;

        let physical_width = (physical_size.width as f64 * scale_factor) as f32;
        let physical_height = (physical_size.height as f64 * scale_factor) as f32;

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
        }
    }
}

struct Application {
    window_state: Option<State>,
}

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window_state.is_some() {
            return;
        }

        // Set up window
        let (width, height) = (800, 600);
        let window_attributes = Window::default_attributes()
            .with_inner_size(LogicalSize::new(width as f64, height as f64))
            .with_title("glyphon hello world");
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

        self.window_state = Some(State::new(window));
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.window_state else {
            return;
        };

        let State {
            window,
            device,
            queue,
            surface,
            surface_config,
            ..
        } = state;

        match event {
            WindowEvent::Resized(size) => {
                surface_config.width = size.width;
                surface_config.height = size.height;
                surface.configure(&device, &surface_config);
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let frame = surface.get_current_texture().unwrap();
                let view = frame.texture.create_view(&TextureViewDescriptor::default());
                let mut encoder =
                    device.create_command_encoder(&CommandEncoderDescriptor { label: None });
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
                }

                queue.submit(Some(encoder.finish()));
                frame.present();

                // atlas.trim();
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ColorBrush {
    color: Rgba<u8>,
}
impl Default for ColorBrush {
    fn default() -> Self {
        Self {
            color: Rgba([0, 0, 0, 255]),
        }
    }
}

fn text_layout() -> Layout<ColorBrush> {
    let text = String::from(
        "Some text here. Let's make it a bit longer so that line wrapping kicks in 😊. And also some اللغة العربية arabic text.\nThis is underline and strikethrough text",
    );

    // The display scale for HiDPI rendering
    let display_scale = 1.0;

    // The width for line wrapping
    let max_advance = Some(200.0 * display_scale);

    // Colours for rendering
    let text_color = Rgba([0, 0, 0, 255]);
    let bg_color = Rgba([255, 255, 255, 255]);

    // Padding around the output image
    let padding = 20;

    // Create a FontContext, LayoutContext and ScaleContext
    //
    // These are all intended to be constructed rarely (perhaps even once per app (or once per thread))
    // and provide caches and scratch space to avoid allocations
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();
    let mut scale_cx = ScaleContext::new();

    // Setup some Parley text styles
    let text_brush = ColorBrush { color: text_color };
    let brush_style = StyleProperty::Brush(text_brush);
    let font_stack = FontStack::from("system-ui");
    let bold_style = StyleProperty::FontWeight(FontWeight::new(600.0));
    let underline_style = StyleProperty::Underline(true);
    let strikethrough_style = StyleProperty::Strikethrough(true);

    let mut layout = if std::env::args().any(|arg| arg == "--tree") {
        // TREE BUILDER
        // ============

        // TODO: cleanup API

        let root_style = TextStyle {
            brush: text_brush,
            font_stack,
            line_height: 1.3,
            font_size: 16.0,
            ..Default::default()
        };

        let mut builder = layout_cx.tree_builder(&mut font_cx, display_scale, &root_style);

        builder.push_style_modification_span(&[bold_style]);
        builder.push_text(&text[0..5]);
        builder.pop_style_span();

        builder.push_text(&text[5..40]);

        builder.push_inline_box(InlineBox {
            id: 0,
            index: 0,
            width: 50.0,
            height: 50.0,
        });

        builder.push_text(&text[40..50]);

        builder.push_inline_box(InlineBox {
            id: 1,
            index: 50,
            width: 50.0,
            height: 30.0,
        });

        builder.push_text(&text[50..141]);

        // Set the underline style
        builder.push_style_modification_span(&[underline_style]);
        builder.push_text(&text[141..150]);

        builder.pop_style_span();
        builder.push_text(&text[150..155]);

        // Set the strikethrough style
        builder.push_style_modification_span(&[strikethrough_style]);
        builder.push_text(&text[155..168]);

        // Build the builder into a Layout
        // let mut layout: Layout<ColorBrush> = builder.build(&text);
        let (layout, _text): (Layout<ColorBrush>, String) = builder.build();
        layout
    } else {
        // RANGE BUILDER
        // ============

        // Creates a RangedBuilder
        let mut builder = layout_cx.ranged_builder(&mut font_cx, &text, display_scale);

        // Set default text colour styles (set foreground text color)
        builder.push_default(brush_style);

        // Set default font family
        builder.push_default(font_stack);
        builder.push_default(StyleProperty::LineHeight(1.3));
        builder.push_default(StyleProperty::FontSize(16.0));

        // Set the first 4 characters to bold
        builder.push(bold_style, 0..4);

        // Set the underline & strikethrough style
        builder.push(underline_style, 141..150);
        builder.push(strikethrough_style, 155..168);

        builder.push_inline_box(InlineBox {
            id: 0,
            index: 40,
            width: 50.0,
            height: 50.0,
        });
        builder.push_inline_box(InlineBox {
            id: 1,
            index: 50,
            width: 50.0,
            height: 30.0,
        });

        // Build the builder into a Layout
        // let mut layout: Layout<ColorBrush> = builder.build(&text);
        let layout: Layout<ColorBrush> = builder.build(&text);
        layout
    };

    // Perform layout (including bidi resolution and shaping) with start alignment
    layout.break_all_lines(max_advance);
    layout.align(max_advance, Alignment::Start, AlignmentOptions::default());

    return layout;
}