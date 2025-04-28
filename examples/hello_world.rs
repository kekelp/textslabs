use parley_atlas_renderer::*;
use swash::zeno::{Format, Vector};

use std::sync::Arc;
use image::{Pixel, Rgba, RgbaImage};
use parley::{Alignment, AlignmentOptions, FontContext, FontStack, FontWeight, Glyph, GlyphRun, InlineBox, Layout, LayoutContext, PositionedLayoutItem, StyleProperty, TextStyle};
use parley_atlas_renderer::swash_render::ColorBrush;
use wgpu::{
    CommandEncoderDescriptor, CompositeAlphaMode, DeviceDescriptor, Instance, InstanceDescriptor,
    LoadOp, MultisampleState, Operations, PresentMode, RenderPassColorAttachment,
    RenderPassDescriptor, RequestAdapterOptions, SurfaceConfiguration, TextureFormat,
    TextureUsages, TextureViewDescriptor,
};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Scaler, Source, StrikeWith};
use swash::{zeno, FontRef};
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

    text_layout: Layout<ColorBrush>,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();

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

 

        let layout = text_layout();

        let mut text_renderer = TextRenderer::new();

        text_renderer.prepare(&layout);

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_layout: layout,
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

fn text_layout() -> Layout<ColorBrush> {
    let text = String::from(
        "Some text here. Let's make it a bit longer so that line wrapping kicks in 😊. And also some اللغة العربية arabic text.\nThis is underline and strikethrough text",
    );

    let display_scale = 1.0;

    let max_advance = Some(200.0 * display_scale);

    let text_color = Rgba([0, 0, 0, 255]);


    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();

    let text_brush = ColorBrush { color: text_color };
    let mut builder = layout_cx.ranged_builder(&mut font_cx, &text, display_scale);

    builder.push_default(StyleProperty::Brush(text_brush));

    builder.push_default(FontStack::from("system-ui"));
    builder.push_default(StyleProperty::LineHeight(1.3));
    builder.push_default(StyleProperty::FontSize(16.0));

    builder.push(StyleProperty::FontWeight(FontWeight::new(600.0)), 0..4);

    builder.push(StyleProperty::Underline(true), 141..150);
    builder.push(StyleProperty::Strikethrough(true), 155..168);

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
    let mut layout: Layout<ColorBrush> = builder.build(&text);    

    // Perform layout (including bidi resolution and shaping) with start alignment
    layout.break_all_lines(max_advance);
    layout.align(max_advance, Alignment::Start, AlignmentOptions::default());

    return layout;
}



struct TextRenderer {
    atlas: RgbaImage,
    tmp_glyph: RgbaImage,
    font_cx: FontContext,
    layout_cx: LayoutContext<ColorBrush>,
    scale_cx: ScaleContext,
}

impl TextRenderer {
    pub fn new() -> Self {
        let bg_color = Rgba([255, 255, 255, 255]);

        Self {
            atlas: RgbaImage::from_pixel(2000, 2000, bg_color),
            tmp_glyph: RgbaImage::from_pixel(100, 100, bg_color),
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            scale_cx: ScaleContext::new(),   
        }
    }

    fn prepare(&mut self, layout: &Layout<ColorBrush>) {
        // Iterate over laid out lines
        for line in layout.lines() {
            // Iterate over GlyphRun's within each line
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        self.prepare_glyph_run(&glyph_run);
                    }
                    PositionedLayoutItem::InlineBox(_inline_box) => {}
                }
            }
        }
    }

    fn prepare_glyph_run(
        &mut self,
        glyph_run: &GlyphRun<'_, ColorBrush>,
    ) {
        // Resolve properties of the GlyphRun
        let mut run_x = glyph_run.offset();
        let run_y = glyph_run.baseline();
        let style = glyph_run.style();
        let color_brush = style.brush;

        // Get the "Run" from the "GlyphRun"
        let run = glyph_run.run();

        // Resolve properties of the Run
        let font = run.font();
        let font_size = run.font_size();
        let normalized_coords = run.normalized_coords();

        // Convert from parley::Font to swash::FontRef
        let font_ref = FontRef::from_index(font.data.as_ref(), font.index as usize).unwrap();

        // Build a scaler. As the font properties are constant across an entire run of glyphs
        // we can build one scaler for the run and reuse it for each glyph.
        let mut scaler = self.scale_cx
            .builder(font_ref)
            .size(font_size)
            .hint(true)
            .normalized_coords(normalized_coords)
            .build();

        // Iterates over the glyphs in the GlyphRun
        for glyph in glyph_run.glyphs() {
            let glyph_x = run_x + glyph.x;
            let glyph_y = run_y - glyph.y;
            run_x += glyph.advance;

            // Compute the fractional offset
            // You'll likely want to quantize this in a real renderer
            let offset = Vector::new(glyph_x.fract(), glyph_y.fract());

            // Render the glyph using swash
            let rendered_glyph = Render::new(
                // Select our source order
                &[
                    Source::ColorOutline(0),
                    Source::ColorBitmap(StrikeWith::BestFit),
                    Source::Outline,
                ],
            )
            // Select the simple alpha (non-subpixel) format
            .format(Format::Alpha)
            // Apply the fractional offset
            .offset(offset)
            // Render the image
            .render(&mut scaler, glyph.id)
            .unwrap();

            let glyph_width = rendered_glyph.placement.width;
            let glyph_height = rendered_glyph.placement.height;
            let glyph_x = (glyph_x.floor() as i32 + rendered_glyph.placement.left) as u32;
            let glyph_y = (glyph_y.floor() as i32 - rendered_glyph.placement.top) as u32;

            match rendered_glyph.content {
                Content::Mask => {
                    let mut i = 0;
                    let bc = color_brush.color;
                    for pixel_y in 0..glyph_height {
                        for pixel_x in 0..glyph_width {
                            let x = glyph_x + pixel_x;
                            let y = glyph_y + pixel_y;
                            let alpha = rendered_glyph.data[i];
                            let color = Rgba([bc[0], bc[1], bc[2], alpha]);
                            self.tmp_glyph.get_pixel_mut(x, y).blend(&color);
                            i += 1;
                        }
                    }
                }
                Content::SubpixelMask => unimplemented!(),
                Content::Color => {
                    let row_size = glyph_width as usize * 4;
                    for (pixel_y, row) in rendered_glyph.data.chunks_exact(row_size).enumerate() {
                        for (pixel_x, pixel) in row.chunks_exact(4).enumerate() {
                            let x = glyph_x + pixel_x as u32;
                            let y = glyph_y + pixel_y as u32;
                            let color = Rgba(pixel.try_into().expect("Not RGBA"));
                            self.tmp_glyph.get_pixel_mut(x, y).blend(&color);
                        }
                    }
                }
            }
        }

        // Draw decorations: underline & strikethrough
        // let style = glyph_run.style();
        // let run_metrics = run.metrics();
        // if let Some(decoration) = &style.underline {
        //     let offset = decoration.offset.unwrap_or(run_metrics.underline_offset);
        //     let size = decoration.size.unwrap_or(run_metrics.underline_size);
        //     render_decoration(img, glyph_run, decoration.brush, offset, size, padding);
        // }
        // if let Some(decoration) = &style.strikethrough {
        //     let offset = decoration
        //         .offset
        //         .unwrap_or(run_metrics.strikethrough_offset);
        //     let size = decoration.size.unwrap_or(run_metrics.strikethrough_size);
        //     render_decoration(img, glyph_run, decoration.brush, offset, size, padding);
        // }
    }



    
}