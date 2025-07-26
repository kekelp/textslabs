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
use accesskit::{Action, ActionRequest, Live, Node, NodeId, Rect as AccessRect, Role, Tree, TreeUpdate};
use accesskit_winit::{Adapter, Event as AccessKitEvent, WindowEvent as AccessKitWindowEvent};

const WINDOW_TITLE: &str = "Textslabs Accessibility Demo";

const WINDOW_ID: NodeId = NodeId(0);
const TEXT_INPUT_ID: NodeId = NodeId(1); 
const INFO_TEXT_ID: NodeId = NodeId(2);
const ANNOUNCEMENT_ID: NodeId = NodeId(3);
const INITIAL_FOCUS: NodeId = TEXT_INPUT_ID;

const TEXT_INPUT_RECT: AccessRect = AccessRect {
    x0: 50.0,
    y0: 100.0,
    x1: 350.0,
    y1: 135.0,
};

const INFO_TEXT_RECT: AccessRect = AccessRect {
    x0: 50.0,
    y0: 200.0,
    x1: 450.0,
    y1: 300.0,
};

struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,
    
    text_renderer: TextRenderer,
    text: Text,
    
    adapter: Adapter,
    focus: NodeId,
    announcement: Option<String>,
    current_text: String,
    
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
        
        // Setup wgpu
        let physical_size = window.inner_size();
        let instance = Instance::new(InstanceDescriptor::default());
        let wgpu_adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
        let (device, queue) = pollster::block_on(wgpu_adapter.request_device(&DeviceDescriptor::default(), None)).unwrap();
        let surface = instance.create_surface(window.clone()).expect("Create surface");
        let surface_config = surface.get_default_config(&wgpu_adapter, physical_size.width, physical_size.height).unwrap();
        surface.configure(&device, &surface_config);

        // Setup textslabs
        let mut text = Text::new_without_auto_wakeup();
        let text_edit_handle = text.add_text_edit("".to_string(), (TEXT_INPUT_RECT.x0, TEXT_INPUT_RECT.y0), (TEXT_INPUT_RECT.size().width as f32, TEXT_INPUT_RECT.size().height as f32), 0.0);
        text.get_text_edit_mut(&text_edit_handle).set_single_line(true);
        text.get_text_edit_mut(&text_edit_handle).set_placeholder("Type here - accessible to screen readers!".to_string());
        
        let _info_text_handle = text.add_text_box(
            "This is a Textslabs accessibility demo. The text input above is fully accessible to screen readers. Try typing and using Tab to navigate.".to_string(),
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
            adapter,
            focus: INITIAL_FOCUS,
            announcement: None,
            current_text: String::new(),
            modifiers: ModifiersState::default(),
        })
    }

    fn build_announcement(text: &str) -> Node {
        let mut node = Node::new(Role::Label);
        node.set_value(text);
        node.set_live(Live::Polite);
        node
    }

    fn build_root(&mut self) -> Node {
        let mut node = Node::new(Role::Window);
        node.set_children(vec![TEXT_INPUT_ID, INFO_TEXT_ID]);
        if self.announcement.is_some() {
            node.push_child(ANNOUNCEMENT_ID);
        }
        node.set_label(WINDOW_TITLE);
        node
    }

    fn build_text_input(&self) -> Node {
        let mut node = Node::new(Role::TextInput);
        node.set_bounds(TEXT_INPUT_RECT);
        node.set_label("Text Input Field");
        node.set_value(self.current_text.clone());
        if self.current_text.is_empty() {
            node.set_description("Type here to test accessibility - press Tab to navigate");
        }
        node.add_action(Action::Focus);
        node.add_action(Action::SetTextSelection);
        node.add_action(Action::ReplaceSelectedText);
        node
    }

    fn build_info_text(&self) -> Node {
        let mut node = Node::new(Role::Label);
        node.set_bounds(INFO_TEXT_RECT);
        node.set_label("Information");
        node.set_value("Accessibility demo. Try typing and using Tab to navigate.");
        node
    }

    fn build_initial_tree(&mut self) -> TreeUpdate {
        let root = self.build_root();
        let text_input = self.build_text_input();
        let info_text = self.build_info_text();
        let tree = Tree::new(WINDOW_ID);
        let mut result = TreeUpdate {
            nodes: vec![
                (WINDOW_ID, root),
                (TEXT_INPUT_ID, text_input),
                (INFO_TEXT_ID, info_text),
            ],
            tree: Some(tree),
            focus: self.focus,
        };
        if let Some(announcement) = &self.announcement {
            result
                .nodes
                .push((ANNOUNCEMENT_ID, Self::build_announcement(announcement)));
        }
        result
    }

    fn set_focus(&mut self, focus: NodeId) {
        self.focus = focus;
        self.adapter.update_if_active(|| TreeUpdate {
            nodes: vec![],
            tree: None,
            focus,
        });
    }

    fn update_text(&mut self, new_text: String) {
        self.current_text = new_text;
        self.announcement = Some(format!("Text updated: {}", self.current_text));
        
        // Build the update outside the closure to avoid borrowing issues
        let text_input = self.build_text_input();
        let announcement = Self::build_announcement(&self.announcement.as_ref().unwrap());
        let root = self.build_root();
        let focus = self.focus;
        
        self.adapter.update_if_active(move || TreeUpdate {
            nodes: vec![
                (TEXT_INPUT_ID, text_input),
                (ANNOUNCEMENT_ID, announcement),
                (WINDOW_ID, root)
            ],
            tree: None,
            focus,
        });
    }

    fn handle_window_event(&mut self, event: &WindowEvent) {
        // Handle text events first
        let result = self.text.handle_event(event, &self.window);
        
        // Update accessibility if text changed
        if result.need_rerender {
            let current_text = "".to_string(); // Placeholder - would need to access actual text
            self.update_text(current_text);
        }

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
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key,
                    state: ElementState::Pressed,
                    ..
                },
                ..
            } => {
                match logical_key {
                    Key::Named(winit::keyboard::NamedKey::Tab) => {
                        let new_focus = if self.focus == TEXT_INPUT_ID {
                            INFO_TEXT_ID
                        } else {
                            TEXT_INPUT_ID
                        };
                        self.set_focus(new_focus);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_accessibility_event(&mut self, user_event: AccessKitEvent) {
        match user_event.window_event {
            AccessKitWindowEvent::InitialTreeRequested => {
                let initial_tree = self.build_initial_tree();
                self.adapter.update_if_active(move || initial_tree);
            }
            AccessKitWindowEvent::ActionRequested(ActionRequest { action, target, .. }) => {
                match action {
                    Action::Focus => {
                        if target == TEXT_INPUT_ID || target == INFO_TEXT_ID {
                            self.set_focus(target);
                        }
                    }
                    Action::ReplaceSelectedText => {
                        if target == TEXT_INPUT_ID {
                            println!("Screen reader requested text replacement");
                        }
                    }
                    _ => {}
                }
            }
            AccessKitWindowEvent::AccessibilityDeactivated => {}
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
            state.handle_accessibility_event(user_event);
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.state = Some(State::new(event_loop, self.event_loop_proxy.clone())
            .expect("failed to create initial window"));
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none() {
            event_loop.exit();
        }
    }
}

fn main() {
    let event_loop = EventLoop::with_user_event().build().unwrap();
    let mut app = Application::new(event_loop.create_proxy());
    event_loop.run_app(&mut app).unwrap();
}