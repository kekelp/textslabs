use textslabs::*;
use std::sync::Arc;
use wgpu::*;
use wgpu::util::DeviceExt;
use winit::{
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::EventLoop,
    window::Window,
};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop
        .run_app(&mut Application { state: None })
        .unwrap();
}

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 600;

struct State {
    device: Device,
    queue: Queue,
    surface: Surface<'static>,
    window: Arc<Window>,
    text_renderer: TextRenderer,
    text: Text,
    custom_pipeline: RenderPipeline,
    custom_vertex_buffer: Buffer,
    custom_element_z: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
}

impl State {
    fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let instance = Instance::new(&InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
        
        let surface = instance.create_surface(window.clone()).expect("Create surface");
        let surface_config = surface.get_default_config(&adapter, physical_size.width, physical_size.height).unwrap();
        surface.configure(&device, &surface_config);

        // Create text renderer with z-range filtering enabled
        let text_renderer = TextRenderer::new_with_params(
            &device, 
            &queue, 
            surface_config.format,
            None,
            TextRendererParams {
                enable_z_range_filtering: true,
                ..Default::default()
            }
        );

        let mut text = Text::new();
        
        let _ = text.add_text_box(
            "This text is below the colorful rectangle. This text is below the colorful rectangle. This text is below the colorful rectangle. This text is below the colorful rectangle. ",
            (70.0, 200.0), (380.0, 180.0), 0.8
        );
        
        let _ = text.add_text_box(
            "This text is in front of the colorful rectangle. This text is in front of the colorful rectangle. This text is in front of the colorful rectangle. This text is in front of the colorful rectangle. ",
            (460.0, 200.0), (380.0, 180.0), 0.1
        );

        let custom_shader_source = r#"
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

        let custom_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("custom element shader"),
            source: ShaderSource::Wgsl(custom_shader_source.into()),
        });

        let custom_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("custom element pipeline"),
            layout: None,
            vertex: VertexState {
                module: &custom_shader,
                entry_point: Some("vs_main"),
                buffers: &[VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as BufferAddress,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                }],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &custom_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: surface_config.format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
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

        // Create a bright semitransparent colored rectangle (front layer)
        let custom_vertices = [
            Vertex { position: [-0.5, -0.5, 0.4], color: [1.0, 0.0, 1.0, 0.9] },
            Vertex { position: [ 0.5, -0.5, 0.4], color: [0.0, 1.0, 1.0, 0.9] },
            Vertex { position: [-0.5,  0.5, 0.4], color: [1.0, 1.0, 0.0, 0.9] },
            Vertex { position: [ 0.5,  0.5, 0.4], color: [1.0, 0.5, 0.0, 0.9] },
        ];

        let custom_vertex_buffer = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("custom element vertex buffer"),
            contents: bytemuck::cast_slice(&custom_vertices),
            usage: BufferUsages::VERTEX,
        });


        Self {
            device,
            queue,
            surface,
            window,
            text_renderer,
            text,
            custom_pipeline,
            custom_vertex_buffer,
            custom_element_z: 0.4,
        }
    }

    fn render(&mut self) -> Result<(), SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&TextureViewDescriptor::default());

        self.text.prepare_all(&mut self.text_renderer);
        self.text_renderer.load_to_gpu(&self.device, &self.queue);

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("render encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("render pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color { r: 0.05, g: 0.05, b: 0.1, a: 1.0 }),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Use render_z_range to draw the custom elements and the text inbetween them in-order.
            // We can't really rely on the depth buffer when elements are semitransparent and blend in the background, such as text glyphs.
            
            // First, draw background text (z=1.0 to custom_element_z=0.4)
            self.text_renderer.render_z_range(&mut render_pass, [1.0, self.custom_element_z]);
            
            // Then draw the colored rectangle (z=0.4)
            render_pass.set_pipeline(&self.custom_pipeline);
            render_pass.set_vertex_buffer(0, self.custom_vertex_buffer.slice(..));
            render_pass.draw(0..4, 0..1);
            
            // Finally draw the foreground text (z=0.4 to 0.0)
            self.text_renderer.render_z_range(&mut render_pass, [self.custom_element_z, 0.0]);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn window_event(&mut self, event: WindowEvent) {
        self.text.handle_event(&event, &self.window);

        match event {
            WindowEvent::RedrawRequested => {
                let r = self.render();
                if let Err(e) = r {
                    panic!("Render error: {:?}", e);
                }
            }
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

        let window_attributes = Window::default_attributes()
            .with_inner_size(LogicalSize::new(WINDOW_WIDTH as f64, WINDOW_HEIGHT as f64))
            .with_title("Z-Range example")
            .with_resizable(false);
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        window.set_ime_allowed(true);

        self.state = Some(State::new(window));
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            event => {
                if let Some(state) = &mut self.state {
                    state.window_event(event);
                    state.window.request_redraw();
                }
            }
        }
    }
}