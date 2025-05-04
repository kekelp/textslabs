use bytemuck::{Pod, Zeroable};
use etagere::euclid::{Size2D, UnknownUnit};
use etagere::{Allocation, BucketedAtlasAllocator, size2};
use lru::LruCache;
use parley_atlas_renderer::*;
use rustc_hash::FxHasher;
use swash::zeno::{Format, Vector};

use wgpu::*;

use image::{GrayImage, Luma, Rgba, RgbaImage};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontStack, Glyph, GlyphRun,
    Layout, LayoutContext, PositionedLayoutItem, StyleProperty,
};
use std::borrow::Cow;
use std::hash::BuildHasherDefault;
use std::mem;
use std::num::NonZeroU64;
use std::sync::Arc;
use swash::scale::image::{Content, Image};
use swash::scale::{Render, ScaleContext, Scaler, Source, StrikeWith};
use swash::{FontRef, GlyphId};
use wgpu::{
    CommandEncoderDescriptor, CompositeAlphaMode, DeviceDescriptor, Instance, InstanceDescriptor,
    LoadOp, MultisampleState, Operations, PresentMode, RenderPassColorAttachment,
    RenderPassDescriptor, RequestAdapterOptions, SurfaceConfiguration, Texture, TextureFormat,
    TextureUsages, TextureView, TextureViewDescriptor,
};
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

    // Make sure that the winit window is last in the struct so that
    // it is dropped after the wgpu surface is dropped, otherwise the
    // program may crash when closed. This is probably a bug in wgpu.
    window: Arc<Window>,

    text_renderer: TextRenderer,

    text_layout: Layout<ColorBrush>,
    show_atlas: bool,
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

        let text_renderer = TextRenderer::new(&device, &queue);

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
                self.text_renderer.prepare(&self.text_layout);
                println!("prepare(): {:?}", now.elapsed());

                self.text_renderer.gpu_load(&self.queue);

                if self.show_atlas {
                    let atlas_size = self.text_renderer.text_renderer.atlas_size;
                    let big_quad = vec![Quad {
                        pos: [9999, 0],
                        dim: [atlas_size as u16, atlas_size as u16],
                        uv: [0, 0],
                        color: 0,
                        depth: 0.0,
                    }];
                    let bytes: &[u8] = bytemuck::cast_slice(&big_quad);
                    self.queue.write_buffer(&self.text_renderer.text_renderer.vertex_buffer, 0, &bytes);
                }

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

fn text_layout() -> Layout<ColorBrush> {
    let text = String::from("zsdfuasdhkssssssssssssssssssssssssssssssssssss"); // here1

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
    builder.push_default(StyleProperty::FontSize(24.0));

    // builder.push(StyleProperty::FontWeight(FontWeight::new(600.0)), 0..4);

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

type Hasher = BuildHasherDefault<FxHasher>;

struct TextRenderer {
    text_renderer: ContextlessTextRenderer,
    scale_cx: ScaleContext,
}

impl TextRenderer {
    pub fn new(device: &Device, _queue: &Queue) -> Self {
        Self {
            scale_cx: ScaleContext::new(),
            text_renderer: ContextlessTextRenderer::new(device, _queue),
        }
    }

    fn prepare(&mut self, layout: &Layout<ColorBrush>) {
        self.text_renderer
            .prepare(layout, &mut self.scale_cx);
    }

    fn gpu_load(&mut self, queue: &Queue) {
        self.text_renderer.gpu_load(queue);
    }

    fn render(&self, pass: &mut RenderPass<'_>) {
        self.text_renderer.render(pass);
    }

}

struct ContextlessTextRenderer {
    tmp_image: Image,
    font_cx: FontContext,
    layout_cx: LayoutContext<ColorBrush>,
    
    color_atlas: Atlas<RgbaImage>,
    mask_atlas: Atlas<GrayImage>,
    
    bind_group: BindGroup,
    
    params: Params,
    params_buffer: Buffer,
    params_bind_group: BindGroup,

    vertex_buffer: Buffer,
    vertex_buffer_size: u64,
    pipeline: RenderPipeline,
    quads: Vec<Quad>,
    atlas_size: u32,
}

const SOURCES: &[Source; 3] = &[
    Source::ColorOutline(0),
    Source::ColorBitmap(StrikeWith::BestFit),
    Source::Outline,
];

struct Atlas<ImageType> {
    packer: BucketedAtlasAllocator,
    glyph_cache: LruCache<GlyphKey, Allocation, Hasher>,
    image: ImageType,
    texture: Texture, // the format here has to match the image type...
    texture_view: TextureView,
}

impl<ImageType> Atlas<ImageType> {
    pub fn allocate_and_remember() {}
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Zeroable, Pod)]
pub(crate) struct Quad {
    pos: [i32; 2],
    dim: [u16; 2],
    uv: [u16; 2],
    color: u32,
    depth: f32,
}

impl ContextlessTextRenderer {
    fn new(device: &Device, _queue: &Queue) -> Self {
        let bg_color = Rgba([255, 0, 255, 255]);

        // todo
        // unused memory is wasted memory...?
        let atlas_size = Limits::downlevel_defaults().max_texture_dimension_2d;

        let mask_texture = device.create_texture(&TextureDescriptor {
            label: Some("atlas"),
            size: Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mask_texture_view = mask_texture.create_view(&TextureViewDescriptor::default());

        let mut mask_atlas = Atlas {
            image: GrayImage::from_pixel(atlas_size, atlas_size, Luma([0])),
            texture: mask_texture,
            texture_view: mask_texture_view,
            packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
            glyph_cache: LruCache::unbounded_with_hasher(Hasher::default()),
        };

        let color_texture = device.create_texture(&TextureDescriptor {
            label: Some("atlas"),
            size: Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let color_texture_view = color_texture.create_view(&TextureViewDescriptor::default());

        let mut color_atlas = Atlas {
            image: RgbaImage::from_pixel(atlas_size, atlas_size, bg_color),
            texture: color_texture,
            texture_view: color_texture_view,
            packer: BucketedAtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
            glyph_cache: LruCache::unbounded_with_hasher(Hasher::default()),
        };

        let vertex_buffer_size = 4096;
        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("vertices"),
            size: vertex_buffer_size,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("sampler"),
            min_filter: FilterMode::Nearest,
            mag_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            lod_min_clamp: 0f32,
            lod_max_clamp: 0f32,
            ..Default::default()
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("shader"),
            source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Quad>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Sint32x2,
                1 => Uint32,
                2 => Uint32,
                3 => Uint32,
                4 => Float32,
            ],
        };

        let params = Params {
            screen_resolution: Resolution {
                width: 0.0,
                height: 0.0,
            },
            _pad: [0, 0],
        };

        let params_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("params"),
            size: mem::size_of::<Params>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(mem::size_of::<Params>() as u64),
                },
                count: None,
            }],
            label: Some("uniforms bind group layout"),
        });

        let params_bind_group = device.create_bind_group(&BindGroupDescriptor {
            layout: &params_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            }],
            label: Some("uniforms bind group"),
        });

        let uniforms_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(mem::size_of::<Params>() as u64),
                },
                count: None,
            }],
            label: Some("uniforms bind group layout"),
        });

        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        view_dimension: TextureViewDimension::D2,
                        sample_type: TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        view_dimension: TextureViewDimension::D2,
                        sample_type: TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("atlas bind group layout"),
        });

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            layout: &atlas_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&color_atlas.texture_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&mask_atlas.texture_view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
            label: Some("atlas bind group"),
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&atlas_layout, &uniforms_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: TextureFormat::Bgra8Unorm,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::default(),
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            atlas_size,
            tmp_image: Image::new(),
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            color_atlas,
            mask_atlas,
            vertex_buffer,
            vertex_buffer_size,
            pipeline,
            bind_group,

            params,
            params_buffer,
            params_bind_group,
            quads: Vec::<Quad>::with_capacity(300),
        }
    }

    pub fn render(&self, pass: &mut RenderPass<'_>) {
        // if self.quads.is_empty() { return }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_bind_group(1, &self.params_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..4, 0..self.quads.len() as u32);
    }

    fn prepare(&mut self, layout: &Layout<ColorBrush>, scale_cx: &mut ScaleContext) {
        // Iterate over laid out lines
        for line in layout.lines() {
            // Iterate over GlyphRun's within each line
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        self.prepare_glyph_run(&glyph_run, scale_cx);
                    }
                    PositionedLayoutItem::InlineBox(_inline_box) => {}
                }
            }
        }
    }

    fn gpu_load(&mut self, queue: &Queue) {
        // todo: check what is actually needed
        queue.write_buffer(&self.params_buffer, 0, unsafe {
            core::slice::from_raw_parts(
                &self.params as *const Params as *const u8,
                mem::size_of::<Params>(),
            )
        });

        let bytes: &[u8] = bytemuck::cast_slice(&self.quads);
        queue.write_buffer(&self.vertex_buffer, 0, &bytes);
        
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.mask_atlas.texture,
                mip_level: 0,
                origin: Origin3d { x: 0, y: 0, z: 0 },
                aspect: TextureAspect::All,
            },
            &self.mask_atlas.image.as_raw(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.mask_atlas.image.width()),
                rows_per_image: None,
            },
            Extent3d {
                width: self.mask_atlas.image.width(),
                height: self.mask_atlas.image.height(),
                depth_or_array_layers: 1,
            },
        );

        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.color_atlas.texture,
                mip_level: 0,
                origin: Origin3d { x: 0, y: 0, z: 0 },
                aspect: TextureAspect::All,
            },
            &self.color_atlas.image.as_raw(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.color_atlas.image.width() * 4),
                rows_per_image: None,
            },
            Extent3d {
                width: self.color_atlas.image.width(),
                height: self.color_atlas.image.height(),
                depth_or_array_layers: 1,
            },
        );
    }

    fn prepare_glyph_run(
        &mut self,
        glyph_run: &GlyphRun<'_, ColorBrush>,
        scale_cx: &mut ScaleContext,
    ) {
        // Resolve properties of the GlyphRun
        let mut run_x = glyph_run.offset();
        let run_y = glyph_run.baseline();
        let style = glyph_run.style();
        let color_u8s = style.brush.color;
        let color = ((color_u8s[0] as u32) << 0)
            + ((color_u8s[1] as u32) << 8)
            + ((color_u8s[2] as u32) << 16)
            + ((color_u8s[3] as u32) << 24);

        let run = glyph_run.run();

        let font = run.font();
        let font_size = run.font_size();
        let font_ref = FontRef::from_index(font.data.as_ref(), font.index as usize).unwrap();
        let font_key = font.data.id();

        let mut scaler = scale_cx
            .builder(font_ref)
            .size(font_size)
            .hint(true)
            .normalized_coords(run.normalized_coords())
            .build();

        for glyph in glyph_run.glyphs() {
            let glyph_x = run_x + glyph.x;
            let glyph_y = run_y - glyph.y;
            run_x += glyph.advance;

            let (cache_key, pos_x, pos_y) =
                GlyphKey::new(font_key, glyph.id, font_size, (glyph_x, glyph_y));

            match self.tmp_image.content {
                Content::Mask => {
                    if let Some(alloc) = self.mask_atlas.glyph_cache.get(&cache_key) {
                        // eprintln!("cache hit {:?}", cache_key);
                    } else {
                        // eprintln!("cache miss {:?}", cache_key);

                        self.render_glyph(glyph, cache_key, &mut scaler);

                        let size = self.tmp_image.size();

                        if let Some(alloc) = self.mask_atlas.packer.allocate(size) {
                            self.copy_glyph_to_atlas(size, &alloc);
                            self.mask_atlas.glyph_cache.push(cache_key, alloc);

                            let scale_factor = 1.0; // todo, what is this
                            let line_y = (run_y * scale_factor).round() as i32;
                            let y = line_y + pos_y - self.tmp_image.placement.top as i32;
                            let x = pos_x + self.tmp_image.placement.left as i32;
        
                            let quad = Quad {
                                pos: [x, y],
                                dim: [size.width as u16, size.height as u16],
                                uv: [alloc.rectangle.min.x as u16, alloc.rectangle.min.y as u16],
                                color,
                                depth: 0.0,
                            };
                            self.quads.push(quad);
                        } else {
                            panic!("Grow o algo");
                        }
                    }
                }
                Content::SubpixelMask => unimplemented!(),
                Content::Color => {
                    // let alloc = self.color_atlas.allocate(size, cache_key);
                    // let glyph_x = alloc.rectangle.min.x;
                    // let glyph_y = alloc.rectangle.min.y;

                    // let row_size = glyph_width as usize * 4;
                    // for (pixel_y, row) in
                    //     self.tmp_swash_image.data.chunks_exact(row_size).enumerate()
                    // {
                    //     // todo: surely this could just be a single memcpy?
                    //     for (pixel_x, pixel) in row.chunks_exact(4).enumerate() {
                    //         let (pixel_x, pixel_y) = (pixel_x as i32, pixel_y as i32);
                    //         let x = glyph_x + pixel_x;
                    //         let y = glyph_y + pixel_y;
                    //         let color = Rgba(pixel.try_into().expect("Not RGBA"));
                    //         *self.color_atlas.image.get_pixel_mut(x as u32, y as u32) = color;
                    //     }
                    // }
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

    fn copy_glyph_to_atlas(&mut self, size: Size2D<i32, UnknownUnit>, alloc: &Allocation) {
        for y in 0..size.height as i32 {
            let src_start = (y as usize) * (size.width as usize);
            let src_slice =
                &self.tmp_image.data[src_start..(src_start + size.width as usize)];

            let dst_y = (alloc.rectangle.min.y + y) as u32;
            let dst_x = alloc.rectangle.min.x as u32;

            let layout = self.mask_atlas.image.as_flat_samples().layout;
            let mut samples = self.mask_atlas.image.as_flat_samples_mut();
            let samples = samples.as_mut_slice();

            let dst_start =
                (dst_y as usize) * layout.height_stride + (dst_x as usize) * layout.width_stride;

            samples[dst_start..(dst_start + size.width as usize)].copy_from_slice(src_slice);
        }
    }

    /// Render a glyph into the `self.tmp_swash_image` buffer
    fn render_glyph(&mut self, glyph: Glyph, cache_key: GlyphKey, scaler: &mut Scaler) {
        self.tmp_image.clear();
        Render::new(SOURCES)
            .format(Format::Alpha)
            .offset(cache_key.frac_offset())
            .render_into(scaler, glyph.id, &mut self.tmp_image);
    }
}

/// Key for building a glyph cache
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GlyphKey {
    /// Font ID
    pub font_id: u64,
    /// Glyph ID
    pub glyph_id: GlyphId,
    /// `f32` bits of font size
    pub font_size_bits: u32,
    /// Binning of fractional X offset
    pub x_bin: SubpixelBin,
    /// Binning of fractional Y offset
    pub y_bin: SubpixelBin,
    // /// [`CacheKeyFlags`]
    // pub flags: CacheKeyFlags,
}

impl GlyphKey {
    pub fn new(font_id: u64, glyph_id: u16, font_size: f32, pos: (f32, f32)) -> (Self, i32, i32) {
        let (x, x_bin) = SubpixelBin::new(pos.0);
        let (y, y_bin) = SubpixelBin::new(pos.1);
        (
            Self {
                font_id,
                glyph_id,
                font_size_bits: font_size.to_bits(),
                x_bin,
                y_bin,
            },
            x,
            y,
        )
    }

    pub fn frac_offset(&self) -> Vector {
        Vector {
            x: self.x_bin.as_float(),
            y: self.y_bin.as_float(),
        }
    }
}

/// Binning of subpixel position for cache optimization
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SubpixelBin {
    Zero,
    One,
    Two,
    Three,
}

impl SubpixelBin {
    pub fn new(pos: f32) -> (i32, Self) {
        let trunc = pos as i32;
        let fract = pos - trunc as f32;

        if pos.is_sign_negative() {
            if fract > -0.125 {
                (trunc, Self::Zero)
            } else if fract > -0.375 {
                (trunc - 1, Self::Three)
            } else if fract > -0.625 {
                (trunc - 1, Self::Two)
            } else if fract > -0.875 {
                (trunc - 1, Self::One)
            } else {
                (trunc - 1, Self::Zero)
            }
        } else {
            #[allow(clippy::collapsible_else_if)]
            if fract < 0.125 {
                (trunc, Self::Zero)
            } else if fract < 0.375 {
                (trunc, Self::One)
            } else if fract < 0.625 {
                (trunc, Self::Two)
            } else if fract < 0.875 {
                (trunc, Self::Three)
            } else {
                (trunc + 1, Self::Zero)
            }
        }
    }

    pub fn as_float(&self) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::One => 0.25,
            Self::Two => 0.5,
            Self::Three => 0.75,
        }
    }
}

trait UselessTrait2 {
    fn size(&self) -> Size2D<i32, UnknownUnit>;
}
impl UselessTrait2 for Image {
    fn size(&self) -> Size2D<i32, UnknownUnit> {
        size2(self.placement.width as i32, self.placement.height as i32)
    }
}
