use std::cell::RefCell;

use parley::{Alignment, AlignmentOptions};

use crate::*;

struct TextContext {
    layout_cx: LayoutContext<ColorBrush>,
    font_cx: FontContext,
}
impl TextContext {
    pub fn new() -> Self {
        Self {
            layout_cx: LayoutContext::new(),
            font_cx: FontContext::new(),
        }
    }
}

thread_local! {
    static TEXT_CX: RefCell<TextContext> = RefCell::new(TextContext::new());
}

fn with_text_cx<R>(f: impl FnOnce(&mut TextContext) -> R) -> R {
    let res = TEXT_CX.with_borrow_mut(|text_cx| f(text_cx));
    res
}

pub struct TextBox {
    text: String,
    layout: Layout<ColorBrush>,
    needs_relayout: bool,
    left: f32,
    top: f32,
    max_width: f32,
    pub depth: f32,
}

impl TextBox {
    pub fn new(text: String, left: f32, top: f32, max_width: f32, depth: f32) -> Self {
        Self {
            text,
            layout: Layout::new(),
            needs_relayout: true,
            left,
            top,
            max_width,
            depth,
        }
    }

    pub fn layout(&mut self) -> &Layout<ColorBrush> {
        self.relayout_if_needed();
        &self.layout
    }

    fn relayout_if_needed(&mut self) {
        if self.needs_relayout {
            self.layout = with_text_cx(|text_cx| {
                let mut builder =
                    text_cx
                        .layout_cx
                        .ranged_builder(&mut text_cx.font_cx, &self.text, 1.0);
                let mut layout: Layout<ColorBrush> = builder.build(&self.text);
                let max_advance = 200.0;
                layout.break_all_lines(Some(max_advance));
                layout.align(
                    Some(max_advance),
                    Alignment::Start,
                    AlignmentOptions::default(),
                );
                layout
            });
        }
    }
}

impl TextBox {
    pub fn text(&self) -> &String {
        &self.text
    }
    pub fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }
    
    pub fn pos(&self) -> (f32, f32) {
        (self.left, self.top)
    }
    
    pub fn set_position(&mut self, left: f32, top: f32) {
        (self.left, self.top) = (left, top)
    }

    pub fn max_width(&self) -> f32 {
        self.max_width
    }
    pub fn set_max_width(&mut self, max_width: f32) {
        if self.max_width != max_width {
            self.max_width = max_width;
            self.needs_relayout = true;
        }
    }
}