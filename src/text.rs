use crate::*;
use slab::Slab;
use std::time::Instant;
use winit::{event::{Modifiers, MouseButton, WindowEvent}, window::Window};

const MULTICLICK_DELAY: f64 = 0.4;
const MULTICLICK_TOLERANCE_SQUARED: f64 = 26.0;

pub struct Text {
    pub(crate) text_boxes: Slab<TextBox<String>>,
    pub(crate) static_text_boxes: Slab<TextBox<&'static str>>,
    pub(crate) text_edits: Slab<TextEdit>,
    pub(crate) styles: Slab<SharedStyle>,
    pub(crate) input_state: TextInputState,

    pub(crate) focused: Option<AnyBox>,
    pub(crate) mouse_hit_stack: Vec<(AnyBox, f32)>,
}

pub struct TextEditHandle {
    pub(crate) i: u32,
}

pub struct TextBoxHandle {
    pub(crate) i: u32,
    pub(crate) kind: TextBoxKind,
}

pub(crate) enum TextBoxKind {
    StringBox,
    StaticBox,
}

pub struct StyleHandle {
    pub(crate) i: u32,
}

#[derive(Debug, Clone)]
pub struct MouseState {
    pub(crate) pointer_down: bool,
    pub(crate) cursor_pos: (f64, f64),
    pub(crate) last_click_time: Option<Instant>,
    pub(crate) last_click_pos: Option<(f64, f64)>,
    pub(crate) click_count: u32,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            pointer_down: false,
            cursor_pos: (0.0, 0.0),
            last_click_time: None,
            last_click_pos: None,
            click_count: 0,
        }
    }

    pub fn reset(&mut self) {
        self.pointer_down = false;
        self.cursor_pos = (0.0, 0.0);
        self.last_click_time = None;
        self.last_click_pos = None;
        self.click_count = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnyBox {
    TextEdit(u32),
    TextBox(u32),
    StaticTextBox(u32),
}

#[derive(Debug, Clone)]
pub struct TextInputState {
    pub(crate) mouse: MouseState,
    pub(crate) modifiers: Modifiers,
}

impl TextInputState {
    pub fn new() -> Self {
        Self {
            mouse: MouseState::new(),
            modifiers: Modifiers::default(),
        }
    }

    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_pos = (position.x, position.y);
                self.mouse.cursor_pos = cursor_pos;
            },

            WindowEvent::MouseInput { state, .. } => {
                self.mouse.pointer_down = state.is_pressed();

                let now = Instant::now();
                let current_pos = self.mouse.cursor_pos;
                
                if let (Some(last_time), Some(last_pos)) = (self.mouse.last_click_time.take(), self.mouse.last_click_pos) {
                    let dx = current_pos.0 - last_pos.0;
                    let dy = current_pos.1 - last_pos.1;
                    let distance_squared = dx * dx + dy * dy;
                    
                    if now.duration_since(last_time).as_secs_f64() < MULTICLICK_DELAY 
                        && distance_squared <= MULTICLICK_TOLERANCE_SQUARED {
                        self.mouse.click_count = (self.mouse.click_count + 1) % 4;
                    } else {
                        self.mouse.click_count = 1;
                    }
                } else {
                    self.mouse.click_count = 1;
                }
                self.mouse.last_click_time = Some(now);
                self.mouse.last_click_pos = Some(current_pos);
            },
            _ => {}
        }
    }
}

impl Text {
    pub fn new() -> Self {
        Self {
            text_boxes: Slab::new(),
            static_text_boxes: Slab::new(),
            text_edits: Slab::new(),
            styles: Slab::new(),
            input_state: TextInputState::new(),
            focused: None,
            mouse_hit_stack: Vec::with_capacity(6),
        }
    }

    pub fn add_text_box(&mut self, text: String, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextBoxHandle {
        let text_box = TextBox::new(text, pos, size, depth);
        let i = self.text_boxes.insert(text_box) as u32;
        TextBoxHandle { i, kind: TextBoxKind::StringBox }
    }

    pub fn add_static_text_box(&mut self, text: &'static str, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextBoxHandle {
        let text_box = TextBox::new(text, pos, size, depth);
        let i = self.static_text_boxes.insert(text_box) as u32;
        TextBoxHandle { i, kind: TextBoxKind::StaticBox }
    }

    pub fn add_text_edit(&mut self, text: String, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextEditHandle {
        let text_edit = TextEdit::new(text, pos, size, depth);
        let i = self.text_edits.insert(text_edit) as u32;
        TextEditHandle { i }
    }

    pub fn add_single_line_edit(&mut self, text: String, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextEditHandle {
        let text_edit = TextEdit::new_single_line(text, pos, size, depth);
        let i = self.text_edits.insert(text_edit) as u32;
        TextEditHandle { i }
    }

    pub fn add_text_edit_with_newline_mode(&mut self, text: String, pos: (f64, f64), size: (f32, f32), depth: f32, newline_mode: NewlineMode) -> TextEditHandle {
        let text_edit = TextEdit::new_with_newline_mode(text, pos, size, depth, newline_mode);
        let i = self.text_edits.insert(text_edit) as u32;
        TextEditHandle { i }
    }

    pub fn get_text_box(&mut self, handle: &TextBoxHandle) -> Option<&mut TextBox<String>> {
        match handle.kind {
            TextBoxKind::StringBox => self.text_boxes.get_mut(handle.i as usize),
            TextBoxKind::StaticBox => None,
        }
    }

    pub fn get_static_text_box(&mut self, handle: &TextBoxHandle) -> Option<&mut TextBox<&'static str>> {
        match handle.kind {
            TextBoxKind::StringBox => None,
            TextBoxKind::StaticBox => self.static_text_boxes.get_mut(handle.i as usize),
        }
    }

    pub fn get_text_edit(&mut self, handle: &TextEditHandle) -> Option<&mut TextEdit> {
        self.text_edits.get_mut(handle.i as usize)
    }

    pub fn add_shared_style(&mut self, style: TextStyle2) -> StyleHandle {
        let shared_style = SharedStyle::new(style);
        let i = self.styles.insert(shared_style) as u32;
        StyleHandle { i }
    }

    pub fn get_shared_style(&self, handle: &StyleHandle) -> Option<&SharedStyle> {
        self.styles.get(handle.i as usize)
    }

    pub fn apply_shared_style_to_text_edit(&mut self, text_edit_handle: &TextEditHandle, style_handle: &StyleHandle) -> bool {
        if let (Some(text_edit), Some(style)) = (
            self.text_edits.get_mut(text_edit_handle.i as usize),
            self.styles.get(style_handle.i as usize)
        ) {
            text_edit.set_shared_style(style);
            true
        } else {
            false
        }
    }

    pub fn apply_shared_style_to_text_box(&mut self, text_box_handle: &TextBoxHandle, style_handle: &StyleHandle) -> bool {
        match text_box_handle.kind {
            TextBoxKind::StringBox => {
                if let (Some(text_box), Some(style)) = (
                    self.text_boxes.get_mut(text_box_handle.i as usize),
                    self.styles.get(style_handle.i as usize)
                ) {
                    text_box.set_shared_style(style);
                    true
                } else {
                    false
                }
            }
            TextBoxKind::StaticBox => {
                if let (Some(text_box), Some(style)) = (
                    self.static_text_boxes.get_mut(text_box_handle.i as usize),
                    self.styles.get(style_handle.i as usize)
                ) {
                    text_box.set_shared_style(style);
                    true
                } else {
                    false
                }
            }
        }
    }

    pub fn modify_shared_style<F>(&mut self, handle: &StyleHandle, f: F) -> bool 
    where 
        F: FnOnce(&mut TextStyle2)
    {
        if let Some(style) = self.styles.get(handle.i as usize) {
            style.with_borrow_mut(f);
            true
        } else {
            false
        }
    }

    pub fn remove_text_box(&mut self, handle: TextBoxHandle) -> bool {
        match handle.kind {
            TextBoxKind::StringBox => self.text_boxes.try_remove(handle.i as usize).is_some(),
            TextBoxKind::StaticBox => self.static_text_boxes.try_remove(handle.i as usize).is_some(),
        }
    }

    pub fn remove_text_edit(&mut self, handle: TextEditHandle) -> bool {
        self.text_edits.try_remove(handle.i as usize).is_some()
    }

    pub fn remove_shared_style(&mut self, handle: StyleHandle) -> bool {
        self.styles.try_remove(handle.i as usize).is_some()
    }

    pub fn prepare_all(&mut self, text_renderer: &mut TextRenderer) {
        for (_i, text_edit) in self.text_edits.iter_mut() {
            text_renderer.prepare_text_edit(text_edit);
        }
        for (_i, text_box) in self.text_boxes.iter_mut() {
            text_renderer.prepare_text_box(text_box);
        }
        for (_i, text_box) in self.static_text_boxes.iter_mut() {
            text_renderer.prepare_text_box(text_box);
        }

        if let Some(focused) = self.focused {

            match focused {
                AnyBox::TextEdit(i) => {
                    text_renderer.prepare_text_box_decorations(&self.text_edits[i as usize].text_box, true);
                },
                AnyBox::TextBox(i) => {
                    text_renderer.prepare_text_box_decorations(&self.text_boxes[i as usize], false);
                },
                AnyBox::StaticTextBox(i) => {
                    text_renderer.prepare_text_box_decorations(&self.static_text_boxes[i as usize], false);
                },
            }
        }
    }

    pub fn handle_events(&mut self, event: &WindowEvent, window: &Window) {

        self.input_state.handle_event(event);

        if let WindowEvent::MouseInput { state, button, .. } = event {
            if state.is_pressed() && *button == MouseButton::Left {
                self.refocus();
            }
        }


        if let Some(focused) = self.focused {    
            self.handle_focused_event(focused, event, window);
        }
    }

    fn refocus(&mut self) {
        self.mouse_hit_stack.clear();

        let cursor_pos = self.input_state.mouse.cursor_pos;

        for (i, text_edit) in self.text_edits.iter_mut() {
            if text_edit.text_box.hit_full_rect(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::TextEdit(i as u32), text_edit.depth()));
            }
        }
        for (i, text_box) in self.text_boxes.iter_mut() {
            if text_box.hit_full_rect(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::TextBox(i as u32), text_box.depth()));
            }
        }
        for (i, text_box) in self.static_text_boxes.iter_mut() {
            if text_box.hit_full_rect(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::StaticTextBox(i as u32), text_box.depth()));
            }
        }

        let mut new_focus = None;
        let mut top_z = f32::MAX;
        for (id, z) in self.mouse_hit_stack.iter().rev() {
            if *z < top_z {
                top_z = *z;
                new_focus = Some(id.clone());
            }
        }

        if new_focus != self.focused {
            if let Some(old_focus) = self.focused {
                self.remove_focus(old_focus)
            }
        }

        self.focused = new_focus;
    }
    
    fn remove_focus(&mut self, old_focus: AnyBox) {
        match old_focus {
            AnyBox::TextEdit(i) => {
                self.text_edits[i as usize].text_box.reset_selection();
                self.text_edits[i as usize].show_cursor = false;
            },
            AnyBox::TextBox(i) => {
                self.text_boxes[i as usize].reset_selection();
            },
            AnyBox::StaticTextBox(i) => {
                self.static_text_boxes[i as usize].reset_selection();
            },
        }
    }
    
    fn handle_focused_event(&mut self, focused: AnyBox, event: &WindowEvent, window: &Window) {
        match focused {
            AnyBox::TextEdit(i) => {
                self.text_edits[i as usize].handle_event(event, window, &self.input_state);
            },
            AnyBox::TextBox(i) => {
                self.text_boxes[i as usize].handle_event(event, window, &self.input_state);
            },
            AnyBox::StaticTextBox(i) => {
                self.static_text_boxes[i as usize].handle_event(event, window, &self.input_state);
            },
        }
    }
}
