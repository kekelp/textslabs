use handleslab::{Handle, HandleSlab};

use crate::*;

pub struct Text {
    text_boxes: HandleSlab<TextBox<String>>,
    static_text_boxes: HandleSlab<TextBox<&'static str>>,
}

impl Text {
    pub fn new() -> Self {
        Self {
            text_boxes: HandleSlab::with_capacity(50),
            static_text_boxes: HandleSlab::with_capacity(50),
        }
    }

    pub fn new_text_box(&mut self, text: String, pos: (f64, f64), depth: f32) -> Handle<TextBox<String>> {
        self.text_boxes.insert(TextBox::new(text, pos, depth))
    }

    pub fn prepare(&mut self, text_renderer: &mut TextRenderer, device: &Device, queue: &Queue) {
        text_renderer.clear();
        for (_i, text_box) in &mut self.text_boxes.iter_mut() {
            text_renderer.prepare_text_box(text_box);
        }
        for (_i, text_box) in &mut self.static_text_boxes.iter_mut() {
            text_renderer.prepare_text_box(text_box);
        }
        text_renderer.gpu_load(device, queue);
    }

    pub fn handle_event(&mut self, event: &winit::event::WindowEvent) {

    }
}
