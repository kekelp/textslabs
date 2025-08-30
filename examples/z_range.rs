use textslabs::*;
use std::sync::Arc;
use wgpu::*;
use wgpu::util::DeviceExt;
use winit::{
    dpi::LogicalSize,
    event::{WindowEvent, KeyEvent, ElementState},
    event_loop::EventLoop,
    window::Window,
    keyboard::{KeyCode, PhysicalKey},
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
    surface_config: SurfaceConfiguration,
    window: Arc<Window>,

    text_renderer: TextRenderer,
    text: Text,
    
    custom_pipeline: RenderPipeline,
    custom_vertex_buffer: Buffer,
    custom_element_z: f32,
    
    background_vertex_buffer: Buffer,
    
    show_layered_rendering: bool,
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
        let instance = Instance::new(InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            required_features: Features::PUSH_CONSTANTS,
            required_limits: Limits {
                max_push_constant_size: 8,
                ..Default::default()
            },
            ..Default::default()
        }, None)).unwrap();
        
        let surface = instance.create_surface(window.clone()).expect("Create surface");
        let surface_config = surface.get_default_config(&adapter, physical_size.width, physical_size.height).unwrap();
        surface.configure(&device, &surface_config);

        // Create text renderer with z-range filtering enabled
        let text_renderer = TextRenderer::new_with_params(
            &device, 
            &queue, 
            surface_config.format,
            None, // No depth testing
            TextRendererParams {
                enable_z_range_filtering: true,
                ..Default::default()
            }
        );

        let mut text = Text::new();
        
        // Background UI elements (behind the custom element)
        let _background_title = text.add_text_box(
            "Background UI Elements (z=0.9)",
            (50.0, 50.0), (800.0, 40.0), 0.9
        );
        
        let _background_panel = text.add_text_box(
            "• Background panel text\n• Menu items\n• Status indicators\n\nThis text is rendered BEHIND the custom element",
            (50.0, 100.0), (350.0, 150.0), 0.9
        );
        
        // Foreground UI elements (in front of the custom element)  
        let _foreground_title = text.add_text_box(
            "Foreground UI Elements (z=0.3)",
            (500.0, 50.0), (350.0, 40.0), 0.3
        );
        
        let _foreground_panel = text.add_text_box(
            "• Tooltips\n• Modal dialogs\n• Popup menus\n\nThis text is rendered IN FRONT of the custom element",
            (500.0, 100.0), (350.0, 150.0), 0.3
        );
        
        let _instructions = text.add_text_box(
            "Press SPACE to toggle between:\n• Layered rendering (proper blending)\n• Normal rendering (incorrect blending)\n\nLayered rendering uses render_z_range to split text\ninto layers around the custom element for correct\nsemitransparent blending.",
            (50.0, 350.0), (800.0, 200.0), 0.1
        );

        // Create custom element (semitransparent rectangle at z=0.6)
        let custom_element_z = 0.6;
        
        // Custom element shader for a semitransparent colored rectangle
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
            depth_stencil: None, // No depth testing
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Create a bright semitransparent colored rectangle (simulating a color picker, canvas, etc.)
        let custom_vertices = [
            Vertex { position: [-0.5, -0.5, custom_element_z], color: [1.0, 0.0, 1.0, 0.7] }, // Bright magenta
            Vertex { position: [ 0.5, -0.5, custom_element_z], color: [0.0, 1.0, 1.0, 0.7] }, // Bright cyan  
            Vertex { position: [-0.5,  0.5, custom_element_z], color: [1.0, 1.0, 0.0, 0.7] }, // Bright yellow
            Vertex { position: [ 0.5,  0.5, custom_element_z], color: [1.0, 0.5, 0.0, 0.7] }, // Bright orange
        ];

        let custom_vertex_buffer = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("custom element vertex buffer"),
            contents: bytemuck::cast_slice(&custom_vertices),
            usage: BufferUsages::VERTEX,
        });

        // Create opaque background rectangle (behind everything)
        let background_vertices = [
            Vertex { position: [-1.0, -1.0, 1.0], color: [0.2, 0.2, 0.3, 1.0] }, // Dark blue opaque
            Vertex { position: [ 1.0, -1.0, 1.0], color: [0.2, 0.2, 0.3, 1.0] }, 
            Vertex { position: [-1.0,  1.0, 1.0], color: [0.2, 0.2, 0.3, 1.0] },
            Vertex { position: [ 1.0,  1.0, 1.0], color: [0.2, 0.2, 0.3, 1.0] },
        ];

        let background_vertex_buffer = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("background vertex buffer"),
            contents: bytemuck::cast_slice(&background_vertices),
            usage: BufferUsages::VERTEX,
        });

        Self {
            device,
            queue,
            surface,
            surface_config,
            window,
            text_renderer,
            text,
            custom_pipeline,
            custom_vertex_buffer,
            custom_element_z,
            background_vertex_buffer,
            show_layered_rendering: true,
        }
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    fn render(&mut self) -> Result<(), SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&TextureViewDescriptor::default());

        // Prepare text rendering
        self.text.prepare_all(&mut self.text_renderer);
        self.text_renderer.load_to_gpu(&self.device, &self.queue);

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("render encoder"),
        });

        // Update window title
        let mode = if self.show_layered_rendering { "Layered (Correct)" } else { "Normal (Incorrect)" };
        self.window.set_title(&format!("Z-Range Blending Demo - {} - Press SPACE to toggle", mode));

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
                })],
                depth_stencil_attachment: None, // No depth testing
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Always draw opaque background first (behind everything)
            render_pass.set_pipeline(&self.custom_pipeline);
            render_pass.set_vertex_buffer(0, self.background_vertex_buffer.slice(..));
            render_pass.draw(0..4, 0..1);

            if self.show_layered_rendering {
                // Layered rendering for correct blending:
                self.text_renderer.render_z_range(&mut render_pass, [1.0, self.custom_element_z]);
                
                // Draw the semitransparent custom element
                render_pass.set_pipeline(&self.custom_pipeline);
                render_pass.set_vertex_buffer(0, self.custom_vertex_buffer.slice(..));
                render_pass.draw(0..4, 0..1);
                
                self.text_renderer.render_z_range(&mut render_pass, [self.custom_element_z, 0.0]);
            } else {
                // Incorrect rendering: all text in one go
                // No matter what we do here with z-buffers, we'll never get correct results.
                render_pass.set_pipeline(&self.custom_pipeline);
                render_pass.set_vertex_buffer(0, self.custom_vertex_buffer.slice(..));
                render_pass.draw(0..4, 0..1);                
                
                self.text_renderer.render(&mut render_pass);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn window_event(&mut self, event: WindowEvent) {
        self.text.handle_event(&event, &self.window);

        match event {
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    physical_key: PhysicalKey::Code(KeyCode::Space),
                    state: ElementState::Pressed,
                    ..
                },
                ..
            } => {
                self.show_layered_rendering = !self.show_layered_rendering;
                let mode = if self.show_layered_rendering { "Layered (correct blending)" } else { "Normal (incorrect blending)" };
                println!("Switched to: {}", mode);
            }
            WindowEvent::Resized(physical_size) => {
                self.resize(physical_size);
            }
            WindowEvent::RedrawRequested => {
                match self.render() {
                    Ok(_) => {}
                    Err(SurfaceError::Lost) => self.resize(self.window.inner_size()),
                    Err(SurfaceError::OutOfMemory) => panic!("Out of memory"),
                    Err(e) => eprintln!("Render error: {:?}", e),
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
            .with_title("Z-Range Blending Demo - Press SPACE to toggle")
            .with_resizable(true);
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

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