use std::sync::Arc;
use wgpu::DeviceDescriptor;
use winit::{dpi::PhysicalSize, event::WindowEvent, event_loop::EventLoop, window::Window};
use textslabs::*;

const ELLIPSE: u32 = 0;
const TEXT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Shape {
    shape_kind: u32,
    shape_offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Ellipse {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
}
impl Ellipse {
    fn new(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Ellipse {
        Ellipse { x, y, w, h, color }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut Application { state: None }).unwrap();
}

#[allow(dead_code)]
struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    pipeline: wgpu::RenderPipeline,

    shape_buffer: wgpu::Buffer,
    ellipse_buffer: wgpu::Buffer,

    text_bind_group: wgpu::BindGroup,
    params_bind_group: wgpu::BindGroup,
    ellipse_bind_group: wgpu::BindGroup,

    text_edit_1: TextEditHandle,
    text_edit_2: TextEditHandle,
    text_edit_3: TextEditHandle,
    text_edit_4: TextEditHandle,

    ellipses: Vec<Ellipse>,
    shapes: Vec<Shape>,
    
    // Text rendering
    text: Text,
    text_renderer: TextRenderer,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = pollster::block_on(instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
            .unwrap();

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();

        let config = surface.get_default_config(&adapter, size.width, size.height).unwrap();
        surface.configure(&device, &config);

        // Initialize text renderer
        let mut text_renderer = TextRenderer::new(&device, &queue, config.format);
        text_renderer.update_resolution(size.width as f32, size.height as f32);
        
        let mut text = Text::new();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("megashader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("megashader.wgsl").into()),
        });

        let ellipse_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
            label: Some("ellipse_bind_group_layout"),
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            size: 64 * 1024,
            mapped_at_creation: false,
        });

        let ellipse_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Ellipse Buffer"),
            size: 64 * 1024,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ellipse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &ellipse_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ellipse_buffer.as_entire_binding(),
                },
            ],
            label: Some("bind_group_0"),
        });

        let text_bind_group = text_renderer.atlas_bind_group();
        let params_bind_group = text_renderer.params_bind_group();


        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&ellipse_bind_group_layout, &text_renderer.params_bind_group_layout(), &text_renderer.atlas_bind_group_layout()],
            ..Default::default()
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Shape>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Uint32,
                        1 => Uint32,
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // This functional stuff looked better in my head. Many such cases, I think.
        let pos: Vec<(f64, f64)> = (0..5).map(|x| 30.0 + x as f64 * 130.0).map(|x| (x,x)).collect();
        let depths: Vec<f32> = (0..5).map(|x| 1.0 - 0.1 * x as f32).collect();
        let texts = [
            "ヘッケはこれらのL-函数が全複素平面へ有理型接続を持ち、指標が自明であるときには s = 1 でオーダー 1 である極を持ち、それ以外では解析的であることを証明した。原始ヘッケ指標（原始ディリクレ指標に同じ方法である modulus に相対的に定義された）に対し、ヘッケは、これらのL-函数が指標の L-函数の函数等式を満たし、L-函",
            "Мунди деленит молестиае усу ад, пертинах глориатур диссентиас ет нец. Ессент иудицабит маиестатис яуи ад, про ут дицо лорем легере. Вис те цоммодо сцрипта цорпора, тритани интеллегат аргументум цу еум, меи те яуем феугаит. При дисцере интеллегат ат, аеяуе афферт ф",
            " “ , A “-“ ‘ » “ ΄ οὐρανῷ ὀμνύει ἐν τῷ θρόνῳ τοῦ θεοῦ καὶ ἐν τῷ καθημένῳ Ἢ ς .« « ͵ : . ’ 3 ~ + ee, e ~ “ A ΄- -23 ἐπάνω αὐτοῦ. Ουαὶ υμῖν, γραμματεῖς καὶ Φαρισαῖοι ὑποOe 3 9° “ ν ἐᾳφ Ν i Ν  κριταί, OTL ἀποδεκατοῦτε TO ἡδύοσμον Kal TO ἄνηθον καὶ TO ,ὔ A Ε] a A , ~ ’ ‘ ’ κύμινον, καὶ ἀφήκατε τὰ βαρύτερα τοῦ νόμου, τὴν κρίσιν Ν  om” ‘ XN 4 “",
            "s ualidis ingentem viribus hastam  in latus inque feri curvam compagibus alvum contorsit. stetit illa tremens, uteroque recusso insonuere cavae gemitumque dedere cavernae. et, si fata deum, si mens non laeva fuisset, impulerat ferro Argolicas foedare latebras, Troiaque nunc staret, Priamique arx alta maneres. Ecce, manus iuvenem interea post terga revinctum pastores magno ad regem clamore trahebant Dardanidae, qui se ignotum venientibus ultro, hoc ipsum ut ",
            "...",
        ];

        let text_edit_1 = text.add_text_edit(texts[0].to_string(), pos[0], (200.0, 200.0), depths[0]);
        let text_edit_2 = text.add_text_edit(texts[1].to_string(), pos[1], (200.0, 200.0), depths[1]);
        let text_edit_3 = text.add_text_edit(texts[2].to_string(), pos[2], (200.0, 200.0), depths[2]);
        let text_edit_4 = text.add_text_edit(texts[3].to_string(), pos[3], (200.0, 200.0), depths[3]);

        let mut ellipses = Vec::with_capacity(5);
        
        let pos: Vec<(f32, f32)> = (0..6).map(|x| 70.0 + x as f32 * 130.0).map(|x| (x,x)).collect();

        ellipses.push(Ellipse::new(pos[0].0, pos[0].1, 260.0, 260.0, [1.0, 0.1, 0.2, 0.8]));
        ellipses.push(Ellipse::new(pos[1].0, pos[1].1, 260.0, 260.0, [0.1, 1.0, 0.2, 0.8]));
        ellipses.push(Ellipse::new(pos[2].0, pos[2].1, 260.0, 260.0, [0.2, 0.3, 1.0, 0.8]));
        ellipses.push(Ellipse::new(pos[3].0, pos[3].1, 260.0, 260.0, [1.0, 0.2, 0.8, 0.8]));  
        
        let shapes = Vec::with_capacity(20);
        
        Self { window, device, queue, surface, pipeline, shape_buffer: vertex_buffer, ellipse_buffer, text_bind_group, params_bind_group, ellipse_bind_group, ellipses, shapes, text, text_renderer, text_edit_1, text_edit_2, text_edit_3, text_edit_4 }
    }

    // Partial borrows moment. It's so stupid it's not even funny anymore.
    fn draw_text_edit(text_box: &TextEdit, shapes: &mut Vec<Shape>) {
        let QuadRanges { glyph_range, decorations_range } = text_box.quad_range();
        for q in (glyph_range.0)..(glyph_range.1) {
            shapes.push( Shape { shape_kind: TEXT, shape_offset: q as u32 } );
        }
        for q in (decorations_range.0)..(decorations_range.1) {
            shapes.push( Shape { shape_kind: TEXT, shape_offset: q as u32 } );
        }
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.text.prepare_all_for_window(&mut self.text_renderer, &self.window);
        self.text_renderer.load_to_gpu(&self.device, &self.queue);

        self.shapes.clear();

        // Push everything to the shape buffer in order.

        Self::draw_text_edit(&self.text.get_text_edit(&self.text_edit_1), &mut self.shapes);
        self.shapes.push( Shape { shape_kind: ELLIPSE, shape_offset: 0 } );
        Self::draw_text_edit(&self.text.get_text_edit(&self.text_edit_2), &mut self.shapes);
        self.shapes.push( Shape { shape_kind: ELLIPSE, shape_offset: 1 } );
        Self::draw_text_edit(&self.text.get_text_edit(&self.text_edit_3), &mut self.shapes);
        self.shapes.push( Shape { shape_kind: ELLIPSE, shape_offset: 2 } );
        Self::draw_text_edit(&self.text.get_text_edit(&self.text_edit_4), &mut self.shapes);
        self.shapes.push( Shape { shape_kind: ELLIPSE, shape_offset: 3 } );


        self.queue.write_buffer(&self.shape_buffer, 0, bytemuck::cast_slice(&self.shapes));
        self.queue.write_buffer(&self.ellipse_buffer, 0, bytemuck::cast_slice(&self.ellipses));

        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&Default::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.1, g: 0.1, b: 0.1, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            let n = self.shapes.len() as u32;

            render_pass.set_pipeline(&self.pipeline);

            render_pass.set_bind_group(0, &self.ellipse_bind_group, &[]);
            render_pass.set_bind_group(1, &self.text_renderer.params_bind_group(), &[]);
            render_pass.set_bind_group(2, &self.text_renderer.atlas_bind_group(), &[]);
            
            render_pass.set_vertex_buffer(0, self.shape_buffer.slice(..));
            render_pass.draw(0..4, 0..n);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

struct Application { 
    state: Option<State> 
}

impl winit::application::ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.state.is_none() {
            let window = Arc::new(event_loop.create_window(
                Window::default_attributes().with_inner_size(PhysicalSize::new(800, 800)).with_resizable(false)
            ).unwrap());
            self.state = Some(State::new(window));
        }
    }

    fn window_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, _: winit::window::WindowId, event: WindowEvent) {
        let state = &mut self.state.as_mut().unwrap();

        state.text.handle_event(&event, &state.window);

        match event {
            WindowEvent::CloseRequested => {
                std::process::exit(0);
            }
            WindowEvent::RedrawRequested => {
                state.render().unwrap();
                state.window.request_redraw();
            }
            _ => {}
        }
    }
}