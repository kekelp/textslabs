use std::{cell::RefCell, time::Instant};

use parley::{Alignment, AlignmentOptions, Selection};
use winit::event::{Modifiers, WindowEvent};

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
    selection: SelectionState,
}

pub struct SelectionState {
    selection: Selection,
    prev_anchor: Option<Selection>,

    pointer_down: bool,
    last_click_time: Option<Instant>,
    click_count: u32,
    cursor_pos: (f32, f32),
}
impl SelectionState {
    pub fn new() -> Self {
        Self {
            pointer_down: false,
            last_click_time: None,
            click_count: 0,
            cursor_pos: (0.0, 0.0),
            selection: Default::default(),
            prev_anchor: Default::default(),
        }
    }
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
            selection: SelectionState::new(),
        }
    }

    pub fn layout(&mut self) -> &Layout<ColorBrush> {
        self.refresh_layout();
        &self.layout
    }

    fn refresh_layout(&mut self) {
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

    pub fn handle_event(&mut self, event: &winit::event::WindowEvent, modifiers: &Modifiers) {
        self.refresh_layout();
        self.selection.handle_event(&self.layout, event, modifiers);
    }
}

impl SelectionState {
    pub fn handle_event(&mut self, layout: &Layout<ColorBrush>, event: &winit::event::WindowEvent, modifiers: &Modifiers) {
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                let shift = modifiers.state().shift_key();

                if *button == winit::event::MouseButton::Left {
                    self.pointer_down = state.is_pressed();
                    if self.pointer_down {
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
                        match click_count {
                            2 => self.select_word_at_point(layout, cursor_pos.0, cursor_pos.1),
                            3 => self.select_line_at_point(layout, cursor_pos.0, cursor_pos.1),
                            _ => if shift {
                                self.extend_selection_with_anchor(layout, cursor_pos.0, cursor_pos.1)
                            } else {
                                self.move_to_point(layout, cursor_pos.0, cursor_pos.1)
                            }
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let prev_pos = self.cursor_pos;
                self.cursor_pos = (position.x as f32, position.y as f32);
                // self.cursor_pos = (position.x as f32 - INSET, position.y as f32 - INSET);
                // macOS seems to generate a spurious move after selecting word?
                if self.pointer_down && prev_pos != self.cursor_pos {
                    let cursor_pos = self.cursor_pos;
                    self.extend_selection_to_point(layout, cursor_pos.0, cursor_pos.1, true);
                }
            }
            _ => {}
        }

        dbg!(self.selection);
    }

    /// Move the cursor to the cluster boundary nearest this point in the layout.
    pub fn move_to_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        self.set_selection(Selection::from_point(layout, x, y));
    }

    pub fn select_word_at_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        self.set_selection(Selection::word_from_point(layout, x, y));
    }

    /// Select the physical line at the point.
    pub fn select_line_at_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        let line = Selection::line_from_point(layout, x, y);
        self.set_selection(line);
    }

    /// Move the selection focus point to the cluster boundary closest to point.
    pub fn extend_selection_to_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32, keep_granularity: bool) {
        // FIXME: This is usually the wrong way to handle selection extension for mouse moves, but not a regression.
        self.set_selection(
            self.selection.extend_to_point(layout, x, y, keep_granularity),
        );
    }

    /// Extend the selection starting from the previous anchor, moving the selection focus point to the cluster boundary closest to point.
    /// 
    /// Used for shift-click behavior. 
    pub fn extend_selection_with_anchor(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        if let Some(prev_selection) = self.prev_anchor {
            self.set_selection_with_old_anchor(prev_selection);
        } else {
            self.prev_anchor = Some(self.selection);
        }
        // FIXME: This is usually the wrong way to handle selection extension for mouse moves, but not a regression.
        self.set_selection_with_old_anchor(
            self.selection.extend_to_point(layout, x, y, false),
        );
    }


    /// Update the selection, and nudge the `Generation` if something other than `h_pos` changed.
    fn set_selection(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
        self.prev_anchor = None;
    }

    /// Update the selection without resetting the previous anchor.
    fn set_selection_with_old_anchor(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
    }

    fn set_selection_inner(&mut self, new_sel: Selection) {
        self.selection = new_sel;
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