use std::{cell::RefCell, time::Instant};

use parley::{Alignment, AlignmentOptions, Rect, Selection, StyleProperty};
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
    // has to be pub(crate) because of partial borrows. Terrible!
    pub(crate) layout: Layout<ColorBrush>,
    needs_relayout: bool,
    rect: Rect,
    pub depth: f32,
    selection: SelectionState,
}

pub struct SelectionState {
    selection: Selection,
    prev_anchor: Option<Selection>,

    pointer_down: bool,
    last_click_time: Option<Instant>,
    click_count: u32,
    cursor_pos: Option<(f32, f32)>,
}
impl SelectionState {
    pub fn new() -> Self {
        Self {
            pointer_down: false,
            last_click_time: None,
            click_count: 0,
            cursor_pos: None,
            selection: Default::default(),
            prev_anchor: Default::default(),
        }
    }
}

impl TextBox {
    pub fn new(text: String, rect: Rect, depth: f32) -> Self {
        Self {
            text,
            layout: Layout::new(),
            needs_relayout: true,
            rect,
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

                builder.push_default(StyleProperty::FontSize(32.0));
                builder.push_default(StyleProperty::LineHeight(2.0));

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

    // pub fn handle_event(&mut self, event: &winit::event::WindowEvent, modifiers: &Modifiers) {
    //     // do we really need relayout for all events?
    //     self.refresh_layout();
    //     self.selection.handle_event(&self.layout, event, modifiers);
    // }

    pub fn handle_event(&mut self, event: &winit::event::WindowEvent, modifiers: &Modifiers) {       
        // do we really need relayout for all events?
        self.refresh_layout();
        
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                let shift = modifiers.state().shift_key();

                if let Some(cursor_pos) = self.selection.cursor_pos {
                    if *button == winit::event::MouseButton::Left {
                        self.selection.pointer_down = state.is_pressed();
                        if self.selection.pointer_down {
                            let now = Instant::now();
                            if let Some(last) = self.selection.last_click_time.take() {
                                if now.duration_since(last).as_secs_f64() < 0.25 {
                                    self.selection.click_count = (self.selection.click_count + 1) % 4;
                                } else {
                                    self.selection.click_count = 1;
                                }
                            } else {
                                self.selection.click_count = 1;
                            }
                            self.selection.last_click_time = Some(now);
                            let click_count = self.selection.click_count;
                            match click_count {
                                2 => self.selection.select_word_at_point(&self.layout, cursor_pos.0, cursor_pos.1),
                                3 => self.selection.select_line_at_point(&self.layout, cursor_pos.0, cursor_pos.1),
                                _ => if shift {
                                    self.selection.extend_selection_with_anchor(&self.layout, cursor_pos.0, cursor_pos.1)
                                } else {
                                    self.selection.move_to_point(&self.layout, cursor_pos.0, cursor_pos.1)
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let prev_pos = self.selection.cursor_pos;
                let cursor_pos = (position.x as f32 - self.rect.x0 as f32, position.y as f32 - self.rect.y0 as f32);
                self.selection.cursor_pos = Some(cursor_pos);
                
                // macOS seems to generate a spurious move after selecting word?
                if self.selection.pointer_down && prev_pos != self.selection.cursor_pos {
                    self.selection.extend_selection_to_point(&self.layout, cursor_pos.0, cursor_pos.1, true);
                }
            }
            _ => {}
        }
    }
}   
    
impl SelectionState {


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
    
    pub fn selection(&self) -> &Selection {
        &self.selection.selection
    }

    pub fn pos(&self) -> (f64, f64) {
        (self.rect.x0, self.rect.y0)
    }

    pub fn width(&self) -> f64 {
        self.rect.x1 - self.rect.x0
    }
}