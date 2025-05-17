use core::default::Default;
use parley::{GenericFamily, StyleProperty, editor::SplitString};
use std::time::{Duration, Instant};

use winit::{
    event::{Ime, Modifiers, Touch, WindowEvent},
    keyboard::{Key, NamedKey},
};

pub use parley::layout::editor::Generation;
use parley::{FontContext, LayoutContext, PlainEditor, PlainEditorDriver};

use accesskit::NodeId;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::{with_text_cx, ColorBrush};

pub const WINDOW_ID: NodeId = NodeId(0);
pub const TEXT_INPUT_ID: NodeId = NodeId(1);

pub fn next_node_id() -> NodeId {
    static NEXT: AtomicU64 = AtomicU64::new(2);
    NodeId(NEXT.fetch_add(1, Ordering::Relaxed))
}

pub const INSET: f32 = 32.0;

pub struct Editor {
    editor: PlainEditor<ColorBrush>,
    last_click_time: Option<Instant>,
    click_count: u32,
    pointer_down: bool,
    cursor_pos: (f32, f32),
    cursor_visible: bool,
    modifiers: Modifiers,
    start_time: Option<Instant>,
    blink_period: Duration,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        let mut editor = PlainEditor::new(32.0);
        editor.set_text(text);
        editor.set_scale(1.0);
        let styles = editor.edit_styles();
        styles.insert(StyleProperty::LineHeight(1.2));
        styles.insert(GenericFamily::SystemUi.into());
        styles.insert(StyleProperty::Brush(ColorBrush::default()));
        Self {
            editor,
            last_click_time: Default::default(),
            click_count: Default::default(),
            pointer_down: Default::default(),
            cursor_pos: Default::default(),
            cursor_visible: Default::default(),
            modifiers: Default::default(),
            start_time: Default::default(),
            blink_period: Default::default(),
        }
    }

    pub fn editor(&mut self) -> &mut PlainEditor<ColorBrush> {
        &mut self.editor
    }

    pub fn text(&self) -> SplitString<'_> {
        self.editor.text()
    }

    pub fn cursor_reset(&mut self) {
        self.start_time = Some(Instant::now());
        // TODO: for real world use, this should be reading from the system settings
        self.blink_period = Duration::from_millis(500);
        self.cursor_visible = true;
    }

    pub fn disable_blink(&mut self) {
        self.start_time = None;
    }

    pub fn next_blink_time(&self) -> Option<Instant> {
        self.start_time.map(|start_time| {
            let phase = Instant::now().duration_since(start_time);

            start_time
                + Duration::from_nanos(
                    ((phase.as_nanos() / self.blink_period.as_nanos() + 1)
                        * self.blink_period.as_nanos()) as u64,
                )
        })
    }

    pub fn cursor_blink(&mut self) {
        self.cursor_visible = self.start_time.is_some_and(|start_time| {
            let elapsed = Instant::now().duration_since(start_time);
            (elapsed.as_millis() / self.blink_period.as_millis()) % 2 == 0
        });
    }

    pub fn handle_event(&mut self, event: WindowEvent) {
        with_text_cx(|mut layout_cx, mut font_cx| {
            match event {
                WindowEvent::Resized(size) => {
                    self.editor
                        .set_width(Some(size.width as f32 - 2_f32 * INSET));
                }
                WindowEvent::ModifiersChanged(modifiers) => {
                    self.modifiers = modifiers;
                }
                WindowEvent::KeyboardInput { event, .. } if !self.editor.is_composing() => {
                    if !event.state.is_pressed() {
                        return;
                    }
                    self.cursor_reset();
                    let mut drv = self.editor.driver(&mut font_cx, &mut layout_cx);
                    #[allow(unused)]

                    let mods_state = self.modifiers.state();
                    let shift = mods_state.shift_key();
                    let action_mod = if cfg!(target_os = "macos") {
                        mods_state.super_key()
                    } else {
                        mods_state.control_key()
                    };

                    match event.logical_key {
                        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                        Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
                            use clipboard_rs::{Clipboard, ClipboardContext};
                            match c.to_lowercase().as_str() {
                                "c" => {
                                    if let Some(text) = drv.editor.selected_text() {
                                        let cb = ClipboardContext::new().unwrap();
                                        cb.set_text(text.to_owned()).ok();
                                    }
                                }
                                "x" => {
                                    if let Some(text) = drv.editor.selected_text() {
                                        let cb = ClipboardContext::new().unwrap();
                                        cb.set_text(text.to_owned()).ok();
                                        drv.delete_selection();
                                    }
                                }
                                "v" => {
                                    let cb = ClipboardContext::new().unwrap();
                                    let text = cb.get_text().unwrap_or_default();
                                    drv.insert_or_replace_selection(&text);
                                }
                                _ => (),
                            }
                        }
                        Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                            if shift {
                                drv.collapse_selection();
                            } else {
                                drv.select_all();
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            if action_mod {
                                if shift {
                                    drv.select_word_left();
                                } else {
                                    drv.move_word_left();
                                }
                            } else if shift {
                                drv.select_left();
                            } else {
                                drv.move_left();
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if action_mod {
                                if shift {
                                    drv.select_word_right();
                                } else {
                                    drv.move_word_right();
                                }
                            } else if shift {
                                drv.select_right();
                            } else {
                                drv.move_right();
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            if shift {
                                drv.select_up();
                            } else {
                                drv.move_up();
                            }
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            if shift {
                                drv.select_down();
                            } else {
                                drv.move_down();
                            }
                        }
                        Key::Named(NamedKey::Home) => {
                            if action_mod {
                                if shift {
                                    drv.select_to_text_start();
                                } else {
                                    drv.move_to_text_start();
                                }
                            } else if shift {
                                drv.select_to_line_start();
                            } else {
                                drv.move_to_line_start();
                            }
                        }
                        Key::Named(NamedKey::End) => {
                            let this = &mut *self;
                            let mut drv = self.editor.driver(font_cx, layout_cx);

                            if action_mod {
                                if shift {
                                    drv.select_to_text_end();
                                } else {
                                    drv.move_to_text_end();
                                }
                            } else if shift {
                                drv.select_to_line_end();
                            } else {
                                drv.move_to_line_end();
                            }
                        }
                        Key::Named(NamedKey::Delete) => {
                            if action_mod {
                                drv.delete_word();
                            } else {
                                drv.delete();
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            if action_mod {
                                drv.backdelete_word();
                            } else {
                                drv.backdelete();
                            }
                        }
                        Key::Named(NamedKey::Enter) => {
                            drv.insert_or_replace_selection("\n");
                        }
                        Key::Named(NamedKey::Space) => {
                            drv.insert_or_replace_selection(" ");
                        }
                        Key::Character(s) => {
                            drv.insert_or_replace_selection(&s);
                        }
                        _ => (),
                    }
                }
                WindowEvent::Touch(Touch {
                    phase, location, ..
                }) if !self.editor.is_composing() => {
                    let mut drv = self.editor.driver(&mut font_cx, &mut layout_cx);
                    use winit::event::TouchPhase::*;
                    match phase {
                        Started => {
                            // TODO: start a timer to convert to a SelectWordAtPoint
                            drv.move_to_point(location.x as f32 - INSET, location.y as f32 - INSET);
                        }
                        Cancelled => {
                            drv.collapse_selection();
                        }
                        Moved => {
                            // TODO: cancel SelectWordAtPoint timer
                            drv.extend_selection_to_point(
                                location.x as f32 - INSET,
                                location.y as f32 - INSET,
                                true,
                            );
                        }
                        Ended => (),
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    let shift = self.modifiers.state().shift_key();

                    if button == winit::event::MouseButton::Left {
                        self.pointer_down = state.is_pressed();
                        self.cursor_reset();
                        if self.pointer_down && !self.editor.is_composing() {
                            let now = Instant::now();
                            if let Some(last) = self.last_click_time.take() {
                                if now.duration_since(last).as_secs_f64() < 0.25 {
                                    self.click_count = (self.click_count + 1) % 4;
                                } else {
                                    self.click_count = 1;
                                }
                            } else {
                                self.click_count = 1;
                            }
                            self.last_click_time = Some(now);
                            let click_count = self.click_count;
                            let cursor_pos = self.cursor_pos;
                            let mut drv = self.editor.driver(&mut font_cx, &mut layout_cx);
                            match click_count {
                                2 => drv.select_word_at_point(cursor_pos.0, cursor_pos.1),
                                3 => drv.select_line_at_point(cursor_pos.0, cursor_pos.1),
                                _ => if shift {
                                    drv.extend_selection_with_anchor(cursor_pos.0, cursor_pos.1)
                                } else {
                                    drv.move_to_point(cursor_pos.0, cursor_pos.1)
                                }
                            }
                        }
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let prev_pos = self.cursor_pos;
                    self.cursor_pos = (position.x as f32 - INSET, position.y as f32 - INSET);
                    // macOS seems to generate a spurious move after selecting word?
                    if self.pointer_down && prev_pos != self.cursor_pos && !self.editor.is_composing() {
                        self.cursor_reset();
                        let cursor_pos = self.cursor_pos;
                        let mut drv = self.editor.driver(font_cx, layout_cx);
                        drv
                            .extend_selection_to_point(cursor_pos.0, cursor_pos.1, true);
                    }
                }
                WindowEvent::Ime(Ime::Disabled) => {
                    let mut drv = self.editor.driver(font_cx, layout_cx);
                    drv.clear_compose();
                }
                WindowEvent::Ime(Ime::Commit(text)) => {
                    let mut drv = self.editor.driver(font_cx, layout_cx);
                    drv.insert_or_replace_selection(&text);
                }
                WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                    if text.is_empty() {
                        let mut drv = self.editor.driver(font_cx, layout_cx);
                        drv.clear_compose();
                    } else {
                        let mut drv = self.editor.driver(font_cx, layout_cx);
                        drv.set_compose(&text, cursor);
                    }
                }
                _ => {}
            }
        });

    }

    // pub fn handle_accesskit_action_request(&mut self, req: &accesskit::ActionRequest) {
    //     if req.action == accesskit::Action::SetTextSelection {
    //         if let Some(accesskit::ActionData::SetTextSelection(selection)) = &req.data {
    //             self.driver().select_from_accesskit(selection);
    //         }
    //     }
    // }

    /// Return the current `Generation` of the layout.
    pub fn generation(&self) -> Generation {
        self.editor.generation()
    }

    // pub fn accessibility(&mut self, update: &mut TreeUpdate, node: &mut Node) {
    //     let mut drv = self.editor.driver(&mut font_cx, &mut layout_cx);
    //     drv.accessibility(update, node, next_node_id, INSET.into(), INSET.into());
    // }
}

pub const LOREM: &str = r" Lorem ipsum dolor sit amet, consectetur adipiscing elit. Morbi cursus mi sed euismod euismod. Orci varius natoque penatibus et magnis dis parturient montes, nascetur ridiculus mus. Nullam placerat efficitur tellus at semper. Morbi ac risus magna. Donec ut cursus ex. Etiam quis posuere tellus. Mauris posuere dui et turpis mollis, vitae luctus tellus consectetur. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Curabitur eu facilisis nisl.

Phasellus in viverra dolor, vitae facilisis est. Maecenas malesuada massa vel ultricies feugiat. Vivamus venenatis et gהתעשייה בנושא האינטרנטa nibh nec pharetra. Phasellus vestibulum elit enim, nec scelerisque orci faucibus id. Vivamus consequat purus sit amet orci egestas, non iaculis massa porttitor. Vestibulum ut eros leo. In fermentum convallis magna in finibus. Donec justo leo, maximus ac laoreet id, volutpat ut elit. Mauris sed leo non neque laoreet faucibus. Aliquam orci arcu, faucibus in molestie eget, ornare non dui. Donec volutpat nulla in fringilla elementum. Aliquam vitae ante egestas ligula tempus vestibulum sit amet sed ante. ";
