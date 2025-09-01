use std::sync::Arc;
use winit::{dpi::PhysicalSize, event::WindowEvent, event_loop::EventLoop, window::Window};

const ELLIPSE: u32 = 0;

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



fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut Application { state: None }).unwrap();
}

struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    pipeline: wgpu::RenderPipeline,

    vertex_buffer: wgpu::Buffer,
    ellipse_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,

    ellipses: Vec<Ellipse>,
    shapes: Vec<Shape>,
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
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

        let (device, queue) = pollster::block_on(adapter
            .request_device(&Default::default(), None))
            .unwrap();

        let config = surface.get_default_config(&adapter, size.width, size.height).unwrap();
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Megashader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("megashader.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("bind_group_layout"),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
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

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            size: 1024,
            mapped_at_creation: false,
        });

        let ellipse_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Storage Buffer"),
            size: 64 * 1024,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ellipse_buffer.as_entire_binding(),
            }],
            label: Some("bind_group"),
        });

        Self {
            window,
            device,
            queue,
            surface,
            pipeline,
            vertex_buffer,
            ellipse_buffer,
            bind_group,
            ellipses: Vec::new(),
            shapes: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.ellipses.clear();
        self.shapes.clear();
    }

    fn add_ellipse(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let ellipse = Ellipse { x, y, w, h, color };
        self.ellipses.push(ellipse);
        
        let shape = Shape { shape_kind: ELLIPSE, shape_offset: self.shapes.len() as u32 };
        self.shapes.push(shape);
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        if !self.ellipses.is_empty() {
            self.queue.write_buffer(&self.ellipse_buffer, 0, bytemuck::cast_slice(&self.ellipses));
        }
        self.queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.shapes));

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
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            if !self.ellipses.is_empty() {
                let n = self.ellipses.len() as u32;

                render_pass.set_pipeline(&self.pipeline);
                render_pass.set_bind_group(0, &self.bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass.draw(0..4, 0..n);
            }
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
                Window::default_attributes().with_inner_size(PhysicalSize::new(800, 600))
            ).unwrap());
            self.state = Some(State::new(window));
        }
    }

    fn window_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, _: winit::window::WindowId, event: WindowEvent) {
        let state = &mut self.state.as_mut().unwrap();

        match event {
            WindowEvent::CloseRequested => {
                std::process::exit(0);
            }
            WindowEvent::RedrawRequested => {
                state.clear();

                state.add_ellipse(100.0, 100.0, 80.0, 80.0, [1.0, 0.0, 0.0, 1.0]);
                state.add_ellipse(300.0, 150.0, 120.0, 60.0, [0.0, 1.0, 0.0, 0.8]);
                state.add_ellipse(200.0, 300.0, 100.0, 100.0, [0.0, 0.0, 1.0, 0.6]);
                state.add_ellipse(450.0, 250.0, 90.0, 140.0, [1.0, 1.0, 0.0, 0.9]);
                state.add_ellipse(50.0, 400.0,  160.0, 80.0, [1.0, 0.0, 1.0, 0.7]);

                state.render().unwrap();
                state.window.request_redraw();
            }
            _ => {}
        }
    }
}