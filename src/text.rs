use crate::*;
use slab::Slab;
use winit::{event::WindowEvent, window::Window};

pub struct Text {
    pub(crate) text_boxes: Slab<TextBox<String>>,
    pub(crate) static_text_boxes: Slab<TextBox<&'static str>>,
    pub(crate) text_edits: Slab<TextEdit>,
    pub(crate) styles: Slab<SharedStyle>,
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

impl Text {
    pub fn new() -> Self {
        Self {
            text_boxes: Slab::new(),
            static_text_boxes: Slab::new(),
            text_edits: Slab::new(),
            styles: Slab::new(),
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
        for (_, text_edit) in self.text_edits.iter_mut() {
            text_renderer.prepare_text_edit(text_edit);
        }
        for (_, text_box) in self.text_boxes.iter_mut() {
            text_renderer.prepare_text_box(text_box);
        }
        for (_, text_box) in self.static_text_boxes.iter_mut() {
            text_renderer.prepare_text_box(text_box);
        }
    }

    pub fn handle_events(&mut self, event: &WindowEvent, window: &Window) -> TextEventResult {
        let mut combined_result = TextEventResult::new(false);
        let mut focus_already_grabbed = false;

        for (_, text_edit) in self.text_edits.iter_mut() {
            let result = text_edit.handle_event(event, window, focus_already_grabbed);
            if result.focus_grabbed {
                focus_already_grabbed = true;
                combined_result.focus_grabbed = true;
            }
            if result.text_changed {
                combined_result.text_changed = true;
            }
            if result.decorations_changed {
                combined_result.decorations_changed = true;
            }
        }

        for (_, text_box) in self.text_boxes.iter_mut() {
            let result = text_box.handle_event(event, window, focus_already_grabbed);
            if result.focus_grabbed {
                focus_already_grabbed = true;
                combined_result.focus_grabbed = true;
            }
            if result.text_changed {
                combined_result.text_changed = true;
            }
            if result.decorations_changed {
                combined_result.decorations_changed = true;
            }
        }

        for (_, text_box) in self.static_text_boxes.iter_mut() {
            let result = text_box.handle_event(event, window, focus_already_grabbed);
            if result.focus_grabbed {
                focus_already_grabbed = true;
                combined_result.focus_grabbed = true;
            }
            if result.text_changed {
                combined_result.text_changed = true;
            }
            if result.decorations_changed {
                combined_result.decorations_changed = true;
            }
        }
        combined_result
    }
}
