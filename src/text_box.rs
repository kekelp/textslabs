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
    need_reyalout: bool,
    left: i32,
    top: i32,
    max_width: i32,
    pub depth: f32,
}

impl TextBox {
    pub fn new(text: String, left: i32, top: i32, max_width: i32, depth: f32) -> Self {
        Self {
            text,
            layout: Layout::new(),
            need_reyalout: true,
            left,
            top,
            max_width,
            depth,
        }
    }

    pub fn layout(&mut self) -> &Layout<ColorBrush> {
        if self.need_reyalout {
            self.relayout();
        }
        &self.layout
    }

    fn relayout(&mut self) {
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
