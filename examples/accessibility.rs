use textslabs::*;
use std::{sync::Arc, time::Duration, error::Error};
use wgpu::*;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState},
    window::{Window, WindowId},
};
use accesskit::{Action, Node, NodeId, Role, Tree, TreeUpdate};
use accesskit_winit::{Adapter, Event as AccessKitEvent, WindowEvent as AccessKitWindowEvent};

const WINDOW_TITLE: &str = "Accessibility";

const WINDOW_ID: NodeId = NodeId(0);
const TEXT_EDIT_ID: NodeId = NodeId(1);
const INFO_TEXT_ID: NodeId = NodeId(2);
const INITIAL_FOCUS: NodeId = TEXT_EDIT_ID;


struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,
    
    text_renderer: TextRenderer,
    text: Text,
    text_edit_handle: TextEditHandle,
    info_text_handle: TextBoxHandle,
    
    adapter: Adapter,
    focus: NodeId,
    
    modifiers: ModifiersState,
}

impl State {
    fn new(
        event_loop: &ActiveEventLoop, 
        event_loop_proxy: EventLoopProxy<AccessKitEvent>
    ) -> Result<Self, Box<dyn Error>> {
        let window_attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(600, 400))
            .with_visible(false);

        let window = Arc::new(event_loop.create_window(window_attributes)?);
        let adapter = Adapter::with_event_loop_proxy(event_loop, &window, event_loop_proxy);

        // wgpu boilerplate
        let instance = Instance::new(InstanceDescriptor::default());
        let wgpu_adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(wgpu_adapter.request_device(&DeviceDescriptor::default(), None)).unwrap();
        let surface = instance.create_surface(window.clone()).expect("Create surface");
        let surface_config = surface.get_default_config(&wgpu_adapter, window.inner_size().width, window.inner_size().height).unwrap();
        surface.configure(&device, &surface_config);

        let mut text = Text::new_without_auto_wakeup();
        let text_edit_handle = text.add_text_edit("".to_string(), (50.0, 100.0), (300.0, 35.0), 0.0);
        text.get_text_edit_mut(&text_edit_handle).set_single_line(true);
        text.get_text_edit_mut(&text_edit_handle).set_placeholder("Type here".to_string());
        
        let info_text_handle = text.add_text_box(
            "This is a Textslabs accessibility demo. Try typing and using Tab to navigate.".to_string(),
            (50.0, 200.0), (400.0, 100.0), 0.0
        );

        let text_renderer = TextRenderer::new(&device, &queue, surface_config.format);
        
        window.set_visible(true);
        window.set_ime_allowed(true);

        Ok(Self {
            window,
            device,
            queue,
            surface,
            surface_config,
            text_renderer,
            text,
            text_edit_handle,
            info_text_handle,
            adapter,
            focus: INITIAL_FOCUS,
            modifiers: ModifiersState::default(),
        })
    }

    fn build_initial_tree(&mut self) -> TreeUpdate {
        let mut root = Node::new(Role::Window);
        root.set_children(vec![TEXT_EDIT_ID, INFO_TEXT_ID]);
        root.set_label(WINDOW_TITLE);

        // In a GUI library, the accesskit nodes would usually correspond to an element in the GUI library's tree, not to the text boxes themselves.
        // configure_text_edit_node() fills a node with the data corresponding to a text edit.
        let mut text_edit = Node::new(Role::TextInput);
        self.text.configure_text_edit_node(&self.text_edit_handle, &mut text_edit);

        let mut info_text = Node::new(Role::Label);
        self.text.configure_text_box_node(&self.info_text_handle, &mut info_text);

        let tree = Tree::new(WINDOW_ID);
        let result = TreeUpdate {
            nodes: vec![
                (WINDOW_ID, root),
                (TEXT_EDIT_ID, text_edit),
                (INFO_TEXT_ID, info_text),
            ],
            tree: Some(tree),
            focus: self.focus,
        };

        result
    }

    // Again, in a gui library, the mapping would have to be from accesskit NodeIds to GUI library element ids, not to text handles directly.
    fn map_accesskit_node_id_to_text_handle(&mut self, node_id: NodeId) -> AnyBox {
        match node_id {
            TEXT_EDIT_ID => self.text_edit_handle.into_anybox(),
            INFO_TEXT_ID => self.info_text_handle.into_anybox(),
            _ => panic!(),
        }
    }

    fn set_focus(&mut self, focus: NodeId) {
        self.focus = focus;
        
        let focused_text_handle = self.map_accesskit_node_id_to_text_handle(focus);
        self.text.set_focus(&focused_text_handle);

        self.adapter.update_if_active(|| TreeUpdate {
            nodes: vec![],
            tree: None,
            focus,
        });
    }

    fn handle_window_event(&mut self, event: &WindowEvent) {
        self.text.handle_event(event, &self.window);
        
        self.adapter.process_event(&self.window, event);

        match event {
            WindowEvent::Resized(size) => {
                self.surface_config.width = size.width;
                self.surface_config.height = size.height;
                self.surface.configure(&self.device, &self.surface_config);
                self.text_renderer.update_resolution(size.width as f32, size.height as f32);
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let frame = self.surface.get_current_texture().unwrap();
                let view = frame.texture.create_view(&TextureViewDescriptor::default());

                self.text.prepare_all(&mut self.text_renderer);
                self.text_renderer.gpu_load(&self.device, &self.queue);

                let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor { label: None });
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });

                    self.text_renderer.render(&mut pass);
                }

                self.queue.submit(Some(encoder.finish()));
                frame.present();

                std::thread::sleep(Duration::from_millis(16));
                self.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.logical_key {
                        Key::Named(winit::keyboard::NamedKey::Tab) => {
                            let new_focus = if self.focus == TEXT_EDIT_ID {
                                INFO_TEXT_ID
                            } else {
                                TEXT_EDIT_ID
                            };
                            self.set_focus(new_focus);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

struct Application {
    event_loop_proxy: EventLoopProxy<AccessKitEvent>,
    state: Option<State>,
}

impl Application {
    fn new(event_loop_proxy: EventLoopProxy<AccessKitEvent>) -> Self {
        Self {
            event_loop_proxy,
            state: None,
        }
    }
}

impl ApplicationHandler<AccessKitEvent> for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.state = Some(State::new(event_loop, self.event_loop_proxy.clone())
            .expect("failed to create initial window"));
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Some(state) = &mut self.state {
            state.adapter.process_event(&state.window, &event);
            
            match event {
                WindowEvent::CloseRequested => {
                    self.state = None;
                }
                _ => {
                    state.handle_window_event(&event);
                }
            }
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, user_event: AccessKitEvent) {
        if let Some(state) = &mut self.state {
            match user_event.window_event {
                AccessKitWindowEvent::InitialTreeRequested => {
                    let initial_tree = state.build_initial_tree();
                    state.adapter.update_if_active(move || initial_tree);
                }
                AccessKitWindowEvent::ActionRequested(request) => {
                    let handled = state.text.handle_accessibility_action(&request);
                    
                    if !handled {
                        // Handle actions not covered by the library (like focus changes)
                        match request.action {
                            Action::Focus => {
                                state.set_focus(request.target);
                            }
                            Action::ReplaceSelectedText => {
                                if request.target == TEXT_EDIT_ID {
                                    println!("Screen reader requested text replacement");
                                }
                            }
                            _ => {}
                        }
                    }
                }
                AccessKitWindowEvent::AccessibilityDeactivated => {}
            }
        }
    }
}

fn main() {
    let event_loop = EventLoop::with_user_event().build().unwrap();
    let mut app = Application::new(event_loop.create_proxy());
    event_loop.run_app(&mut app).unwrap();
}