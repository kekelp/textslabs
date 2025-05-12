use std::{cell::RefCell, time::Instant};

use parley::{Affinity, Alignment, AlignmentOptions, Rect, Selection, StyleProperty};
use winit::{event::{Modifiers, WindowEvent}, keyboard::{Key, NamedKey}};

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
    focused: bool,
    last_click_time: Option<Instant>,
    click_count: u32,
    cursor_pos: (f32, f32),
}
impl SelectionState {
    pub fn new() -> Self {
        Self {
            pointer_down: false,
            focused: false,
            last_click_time: None,
            click_count: 0,
            cursor_pos: (0.0, 0.0),
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
            with_text_cx(|text_cx| {
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

                self.layout = layout;
                self.needs_relayout = false;
            });
        }
    }

    pub fn handle_event(&mut self, event: &winit::event::WindowEvent, modifiers: &Modifiers) {       
        // do we really need relayout for all events?
        self.refresh_layout();
        
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                let shift = modifiers.state().shift_key();
                if *button == winit::event::MouseButton::Left {

                    let cursor_pos = (self.selection.cursor_pos.0 as f64 - self.rect.x0, self.selection.cursor_pos.1 as f64 - self.rect.y0);
                    
                    if state.is_pressed() {

                        // dbg!( cursor_pos.0 > self.rect.x0
                        //     , cursor_pos.0 < self.rect.x0 + self.layout.max_content_width() as f64
                        //     , cursor_pos.1 > self.rect.y0
                        //     , cursor_pos.1 < self.rect.y0 + self.layout.height() as f64,
                        // );

                        // dbg!( cursor_pos.0, self.rect.x0
                        //     , cursor_pos.0, self.rect.x0 + self.layout.max_content_width() as f64
                        //     , cursor_pos.1, self.rect.y0
                        //     , cursor_pos.1, self.rect.y0 + self.layout.height() as f64,
                        // );
                        // println!();

                        let x_tolerance = 7.0;

                        if cursor_pos.0 > - x_tolerance
                            && cursor_pos.0 < self.layout.max_content_width() as f64 + x_tolerance
                            && cursor_pos.1 > 0.0
                            && cursor_pos.1 < self.layout.height() as f64 {
                            self.selection.pointer_down = true;
                            self.selection.focused = true;
                        } else {
                            self.selection.set_selection(self.selection.selection.collapse());
                            self.selection.focused = false;
                            // todo: this will get messed up with overlapping text boxes
                            // we need to always take in the event to be able to run this
                            // I guess the caller could pass a "already_absorbed" bool where we just do nothing except this
                            // Or just centralize...
                        }

                    } else {
                        self.selection.pointer_down = false;
                    }

                    let cursor_pos = (cursor_pos.0 as f32, cursor_pos.1 as f32);

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
            WindowEvent::CursorMoved { position, .. } => {
                let prev_pos = self.selection.cursor_pos;
                
                let cursor_pos = (position.x as f32, position.y as f32);
                self.selection.cursor_pos = cursor_pos;
                
                // macOS seems to generate a spurious move after selecting word?
                if self.selection.pointer_down && prev_pos != self.selection.cursor_pos {
                    let cursor_pos = (cursor_pos.0 - self.rect.x0 as f32, cursor_pos.1 - self.rect.y0 as f32);                    
                    self.selection.extend_selection_to_point(&self.layout, cursor_pos.0, cursor_pos.1, true);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if ! self.selection.focused {
                    return;
                }
                if !event.state.is_pressed() {
                    return;
                }
                #[allow(unused)]

                let mods_state = modifiers.state();
                let shift = mods_state.shift_key();
                let action_mod = if cfg!(target_os = "macos") {
                    mods_state.super_key()
                } else {
                    mods_state.control_key()
                };

                match &event.logical_key {
                    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                    // Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
                    //     use clipboard_rs::{Clipboard, ClipboardContext};
                    //     match c.to_lowercase().as_str() {
                    //         "c" => {
                    //             if let Some(text) = self.selection.editor.&self.layoutselected_text() {
                    //                 let cb = ClipboardContext::new().unwrap();
                    //                 cb.set_text(text.to_owned()).ok();
                    //             }
                    //         }
                    //         "x" => {
                    //             if let Some(text) = self.selection.editor.&self.layoutselected_text() {
                    //                 let cb = ClipboardContext::new().unwrap();
                    //                 cb.set_text(text.to_owned()).ok();
                    //                 self.selection.delete_selection(&self.layout);
                    //             }
                    //         }
                    //         "v" => {
                    //             let cb = ClipboardContext::new().unwrap();
                    //             let text = cb.get_text().unwrap_or_default();
                    //             self.selection.insert_or_replace_selection(&self.layout&text);
                    //         }
                    //         _ => (),
                    //     }
                    // }
                    Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                        if shift {
                            self.selection.selection = self.selection.selection.collapse();
                        } else {
                            // todo move somewhere
                            self.selection.selection = Selection::from_byte_index(&self.layout, 0_usize, Affinity::default())
                            .move_lines(&self.layout, isize::MAX, true);
                        }
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        if action_mod {
                            if shift {
                                self.selection.select_word_left(&self.layout);
                            }
                        } else if shift {
                            self.selection.select_left(&self.layout);
                        }
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        if action_mod {
                            if shift {
                                self.selection.select_word_right(&self.layout);
                            }
                        } else if shift {
                            self.selection.select_right(&self.layout);
                        }
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        if shift {
                            self.selection.select_up(&self.layout);
                        }
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if shift {
                            self.selection.select_down(&self.layout);
                        }
                    }
                    Key::Named(NamedKey::Home) => {
                        if action_mod {
                            if shift {
                                self.selection.select_to_text_start(&self.layout);
                            }
                        } else if shift {
                            self.selection.select_to_line_start(&self.layout);
                        }
                    }
                    Key::Named(NamedKey::End) => {
                        if action_mod {
                            if shift {
                                self.selection.select_to_text_end(&self.layout);
                            }
                        } else if shift {
                            self.selection.select_to_line_end(&self.layout);
                        }
                    }
                    _ => (),
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


    /// Move the selection focus point to the start of the buffer.
    pub fn select_to_text_start(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.move_lines(
            layout,
            isize::MIN,
            true,
        );
    }

    /// Move the selection focus point to the start of the physical line.
    pub fn select_to_line_start(&mut self, layout: &Layout<ColorBrush>) {
        self.selection =self.selection.line_start(layout, true);
    }

    /// Move the selection focus point to the end of the buffer.
    pub fn select_to_text_end(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = (self.selection.move_lines(
            layout,
            isize::MAX,
            true,
        ));
    }

    /// Move the selection focus point to the end of the physical line.
    pub fn select_to_line_end(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.line_end(layout, true);
    }

    /// Move the selection focus point up to the nearest cluster boundary on the previous line, preserving the horizontal position for repeated movements.
    pub fn select_up(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_line(layout, true);
    }

    /// Move the selection focus point down to the nearest cluster boundary on the next line, preserving the horizontal position for repeated movements.
    pub fn select_down(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_line(layout, true);
    }

    /// Move the selection focus point to the next cluster left in visual order.
    pub fn select_left(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_visual(layout, true);
    }

    /// Move the selection focus point to the next cluster right in visual order.
    pub fn select_right(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_visual(layout, true);
    }

    /// Move the selection focus point to the next word boundary left.
    pub fn select_word_left(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_visual_word(layout, true);
    }

    /// Move the selection focus point to the next word boundary right.
    pub fn select_word_right(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_visual_word(layout, true);
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