use std::cell::RefCell;

use parley::*;
use winit::{
    event::WindowEvent, keyboard::{Key, NamedKey}, platform::modifier_supplement::KeyEventExtModifierSupplement, window::Window
};
use arboard::Clipboard;

use parley::{Affinity, Alignment, Selection};

use crate::*;

const X_TOLERANCE: f64 = 35.0;

pub(crate) struct TextContext {
    layout_cx: LayoutContext<ColorBrush>,
    font_cx: FontContext,
}
impl TextContext {
    pub(crate) fn new() -> Self {
        Self {
            layout_cx: LayoutContext::new(),
            font_cx: FontContext::new(),
        }
    }
}

thread_local! {
    static TEXT_CX: RefCell<TextContext> = RefCell::new(TextContext::new());
}

pub(crate) fn with_text_cx<R>(f: impl FnOnce(&mut LayoutContext<ColorBrush>, &mut FontContext) -> R) -> R {
    let res = TEXT_CX.with_borrow_mut(|text_cx| f(&mut text_cx.layout_cx, &mut text_cx.font_cx));
    res
}

thread_local! {
    static CLIPBOARD: RefCell<Clipboard> = RefCell::new(Clipboard::new().unwrap());
}

pub fn with_clipboard<R>(f: impl FnOnce(&mut Clipboard) -> R) -> R {
    let res = CLIPBOARD.with_borrow_mut(|clipboard| f(clipboard));
    res
}


pub(crate) struct TextBoxInner {
    pub(crate) text: Cow<'static, str>,
    pub(crate) style: StyleHandle,
    pub(crate) style_id: u64,
    pub(crate) layout: Layout<ColorBrush>,
    pub(crate) needs_relayout: bool,
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) max_advance: f32,
    pub(crate) depth: f32,
    pub(crate) selection: SelectionState,
    pub(crate) width: Option<f32>,
    pub(crate) height: f32, 
    pub(crate) alignment: Alignment,
    pub(crate) scale: f32,
    pub(crate) clip_rect: Option<parley::Rect>,
    // todo: the current implementation fadeout can't fade glyphs that get very close to the clip rect edge, but without touching it. Should just switch to passing the whole clip rect to the shader and doing all the math there.
    pub(crate) fadeout_clipping: bool,
    pub(crate) auto_clip: bool,
    pub(crate) scroll_offset: f32,
    
    pub(crate) selectable: bool,

    pub(crate) hidden: bool,
    pub(crate) last_frame_touched: u64,
    pub(crate) can_hide: bool,
}

/// A struct representing a text box.
/// 
/// This struct can't be created directly. Instead, use [`Text::add_text_box()`] to create one within [`Text`] and get a [`TextBoxHandle`] back.
/// 
/// Then, the handle can be used to get a reference to the `TextBox` with [`Text::get_text_box()`], or the equivalent `mut` functions.
pub struct TextBox<'a> {
    pub(crate) inner: &'a mut TextBoxInner,
    pub(crate) style: &'a TextStyle2,
}


pub(crate) fn original_default_style() -> TextStyle2 { 
    TextStyle2 { 
        brush: ColorBrush([255,255,255,255]),
        font_size: 24.0,
        overflow_wrap: OverflowWrap::Anywhere,
        ..Default::default()
    } 
}

// todo: this struct is now useless.
pub(crate) struct SelectionState {
    pub selection: Selection,
}
impl SelectionState {
    pub(crate) fn new() -> Self {
        Self {
            selection: Default::default(),
        }
    }

    fn shift_click_extension(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        self.selection = self.selection.shift_click_extension(layout, x, y);
    }
}

impl TextBoxInner {
    pub(crate) fn new(text: impl Into<Cow<'static, str>>, pos: (f64, f64), size: (f32, f32), depth: f32) -> Self {
        Self {
            text: text.into(),
            style_id: 0,
            layout: Layout::new(),
            selectable: true,
            needs_relayout: true,
            left: pos.0,
            top: pos.1,
            max_advance: size.0,
            height: size.1,
            depth,
            selection: SelectionState::new(),
            style: DEFAULT_STYLE_HANDLE,
            width: Some(size.0), 
            alignment: Default::default(),
            scale: Default::default(),
            clip_rect: None,
            fadeout_clipping: false,
            auto_clip: false,
            scroll_offset: 0.0,
            hidden: false,
            last_frame_touched: 0,
            can_hide: false,
        }
    }

    #[must_use]
    pub(crate) fn hit_full_rect(&self, cursor_pos: (f64, f64)) -> bool {
        let offset = (
            cursor_pos.0 as f64 - self.left,
            cursor_pos.1 as f64 - self.top,
        );

        let hit = offset.0 > -X_TOLERANCE
            && offset.0 < self.max_advance as f64 + X_TOLERANCE
            && offset.1 > 0.0
            && offset.1 < self.height as f64;

        return hit;
    }    
}

impl<'a> TextBox<'a> {
    #[must_use]
    pub(crate) fn hit_full_rect(&self, cursor_pos: (f64, f64)) -> bool {
        self.inner.hit_full_rect(cursor_pos)
    }


    pub(crate) fn handle_event(&mut self, event: &WindowEvent, _window: &Window, input_state: &TextInputState) -> TextEventResult {
        if self.inner.hidden {
            return TextEventResult::nothing();
        }
        
        let initial_selection = self.inner.selection.selection;
        
        let mut result = TextEventResult::nothing();

        self.handle_event_no_edit_inner(event, input_state, false);

        if selection_decorations_changed(initial_selection, self.inner.selection.selection, false, false, false) {
            {
                let this = &mut result;
                this.decorations_changed = true;
            };
        }

        return result;
    }

    pub(crate) fn handle_event_no_edit_inner(&mut self, event: &WindowEvent, input_state: &TextInputState, edit_showing_placeholder: bool) {
        if self.inner.hidden {
            return;
        }
        if !self.inner.selectable {
            self.reset_selection();
            return;
        }
        
        self.inner.selection.handle_event(
            event,
            input_state,
            &self.inner.layout,
            self.inner.left as f32,
            self.inner.top as f32,
            self.inner.scroll_offset,
            edit_showing_placeholder
        );
        
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {}
                #[allow(unused)]
                let mods_state = input_state.modifiers.state();
                let shift = mods_state.shift_key();
                let action_mod = if cfg!(target_os = "macos") {
                    mods_state.super_key()
                } else {
                    mods_state.control_key()
                };

                #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                if action_mod {
                    match event.key_without_modifiers() {
                        Key::Character(c) => {
                            match c.as_str() {
                                "c" if !shift => {
                                    with_clipboard(|cb| {
                                        if let Some(text) = self.selected_text() {
                                            cb.set_text(text.to_owned()).ok();
                                        }
                                    })
                                }
                                "a" => self.select_all(),
                                _ => (),
                            }
                        }
                        _ => (),
                    };
                }
            }
            _ => {}
        }
    }

    pub(crate) fn reset_selection(&mut self) {
        self.set_selection(self.inner.selection.selection.collapse());
    }

    pub fn hidden(&self) -> bool {
        self.inner.hidden
    }

    pub fn depth(&self) -> f32 {
        self.inner.depth
    }

    pub fn text(&self) -> &str {
        &self.inner.text
    }

    pub fn text_mut(&mut self) -> &mut String {
        self.inner.needs_relayout = true;
        self.inner.text.to_mut()
    }

    pub fn pos(&self) -> (f64, f64) {
        (self.inner.left, self.inner.top)
    }

    pub fn clip_rect(&self) -> Option<parley::Rect> {
        self.inner.clip_rect
    }

    pub fn fadeout_clipping(&self) -> bool {
        self.inner.fadeout_clipping
    }

    pub fn auto_clip(&self) -> bool {
        self.inner.auto_clip
    }

    pub fn set_auto_clip(&mut self, auto_clip: bool) {
        self.inner.auto_clip = auto_clip;
    }

    pub fn selected_text(&self) -> Option<&str> {
        if !self.inner.selection.selection.is_collapsed() {
            self.inner.text.get(self.inner.selection.selection.text_range())
        } else {
            None
        }
    }

    pub fn selection(&self) -> Selection {
        self.inner.selection.selection
    }

    pub fn scroll_offset(&self) -> f32 {
        self.inner.scroll_offset
    }

    pub fn selection_geometry(&self) -> Vec<(Rect, usize)> {
        self.inner.selection.selection.geometry(&self.inner.layout)
    }

    pub fn selection_geometry_with(&self, f: impl FnMut(Rect, usize)) {
        self.inner.selection.selection.geometry_with(&self.inner.layout, f);
    }

    pub fn set_pos(&mut self, pos: (f64, f64)) {
        (self.inner.left, self.inner.top) = pos;
    }

    pub fn can_hide(&mut self) -> bool {
        self.inner.can_hide
    }

    pub fn set_can_hide(&mut self, can_hide: bool) {
        self.inner.can_hide = can_hide;
    }

    pub(crate) fn set_hidden(&mut self, hidden: bool) {
        if self.inner.hidden != hidden {
            self.inner.hidden = hidden;

            if hidden {
                self.reset_selection();
            }
        }
    }

    pub fn set_depth(&mut self, depth: f32) {
        self.inner.depth = depth;
    }

    pub fn set_clip_rect(&mut self, clip_rect: Option<parley::Rect>) {
        self.inner.clip_rect = clip_rect;
    }

    pub fn set_fadeout_clipping(&mut self, fadeout_clipping: bool) {
        self.inner.fadeout_clipping = fadeout_clipping;
    }

    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.inner.scroll_offset = offset;
    }

    // todo: this isn't very good, it remains borrowed with the wrong style
    pub fn set_style(&mut self, style: &StyleHandle) {
        self.inner.style = style.sneak_clone();
        self.inner.needs_relayout = true;
    }

    /// Computes the effective clip rectangle, combining auto-clipping with explicit clip_rect
    pub fn effective_clip_rect(&self) -> Option<parley::Rect> {
        let auto_clip_rect = if self.inner.auto_clip {
            Some(parley::Rect {
                x0: self.inner.scroll_offset as f64,
                y0: 0.0,
                x1: (self.inner.scroll_offset + self.inner.max_advance) as f64,
                y1: self.inner.height as f64,
            })
        } else {
            None
        };

        match (auto_clip_rect, self.inner.clip_rect) {
            (None, None) => None,
            (Some(auto), None) => Some(auto),
            (None, Some(explicit)) => Some(explicit),
            (Some(auto), Some(explicit)) => {
                // Intersect the rectangles
                let x0 = auto.x0.max(explicit.x0);
                let y0 = auto.y0.max(explicit.y0);
                let x1 = auto.x1.min(explicit.x1);
                let y1 = auto.y1.min(explicit.y1);
                
                if x0 < x1 && y0 < y1 {
                    Some(parley::Rect { x0, y0, x1, y1 })
                } else {
                    // No intersection - empty rectangle
                    Some(parley::Rect { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 })
                }
            }
        }
    }



    pub(crate) fn rebuild_layout(
        &mut self,
        color_override: Option<ColorBrush>,
        single_line: bool,
    ) {
        if !self.inner.needs_relayout {
            return;
        }

        with_text_cx(|layout_cx, font_cx| {
            let mut builder = layout_cx.tree_builder(font_cx, 1.0, true, self.style);

            if let Some(color_override) = color_override {
                builder.push_style_modification_span(&[
                    StyleProperty::Brush(color_override)
                ]);
            }

            builder.push_text(&self.inner.text);

            let (mut layout, _) = builder.build();

            if ! single_line {
                layout.break_all_lines(Some(self.inner.max_advance));
                layout.align(
                    Some(self.inner.max_advance),
                    self.inner.alignment,
                    AlignmentOptions::default(),
                );
            } else {
                layout.break_all_lines(None);
            }

            self.inner.layout = layout;
            self.inner.needs_relayout = false;
            
            // todo: does this do anything?
            self.inner.selection.selection = self.inner.selection.selection.refresh(&self.inner.layout);
        });
    }



    // Note: This used to be a problem when TextEdit couldn't call refresh_layout() directly.
    // Now that TextEdit has access to refresh_layout(), this is no longer an issue. 
    /// Returns a mutable reference to the text box's text buffer as a Cow.
    /// This provides full access to the underlying storage type.


    /// Set the text to a static string reference.
    /// This is efficient for static strings and avoids allocation.
    pub fn set_static(&mut self, text: &'static str) {
        self.inner.needs_relayout = true;
        self.inner.text = Cow::Borrowed(text);
    }

    /// Set the width of the layout.
    pub fn set_size(&mut self, size: (f32, f32)) {
        let relayout = (self.inner.width != Some(size.0)) || (self.inner.height != size.1) || (self.inner.max_advance != size.0);
        self.inner.width = Some(size.0);
        self.inner.height = size.1;
        self.inner.max_advance = size.0;
        if relayout {
            self.inner.needs_relayout = true;
        }
    }

    /// Set the alignment of the layout.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.inner.alignment = alignment;
        self.inner.needs_relayout = true;
    }

    /// Set the scale for the layout.
    pub fn set_scale(&mut self, scale: f32) {
        self.inner.scale = scale;
        self.inner.needs_relayout = true;
    }

    // #[cfg(feature = "accesskit")]
    // #[inline]
    // /// Perform an accessibility update if the layout is valid.
    // ///
    // /// Returns `None` if the layout is not up-to-date.
    // /// You can call [`refresh_layout`](Self::refresh_layout) before using this method,
    // /// to ensure that the layout is up-to-date.
    // /// The [`accessibility`](PlainEditorDriver::accessibility) method on the driver type
    // /// should be preferred if the contexts are available, which will do this automatically.
    // pub fn try_accessibility(
    //     &mut self,
    //     update: &mut TreeUpdate,
    //     node: &mut Node,
    //     next_node_id: impl FnMut() -> NodeId,
    //     x_offset: f64,
    //     y_offset: f64,
    // ) -> Option<()> {
    //     if self.inner.needs_relayout {
    //         return None;
    //     }
    //     self.inner.accessibility_unchecked(update, node, next_node_id, x_offset, y_offset);
    //     Some(())
    // }

    /// Update the selection, and nudge the `Generation` if something other than `h_pos` changed.
    pub(crate) fn set_selection(&mut self, new_sel: Selection) {

        // This debug code is quite useful when diagnosing selection problems.
        #[allow(clippy::print_stderr)] // reason = "unreachable debug code"
        if false {
            let focus = new_sel.focus();
            let cluster = focus.logical_clusters(&self.inner.layout);
            let dbg = (
                cluster[0].as_ref().map(|c| &self.inner.text[c.text_range()]),
                focus.index(),
                focus.affinity(),
                cluster[1].as_ref().map(|c| &self.inner.text[c.text_range()]),
            );
            eprint!("{dbg:?}");
            let cluster = focus.visual_clusters(&self.inner.layout);
            let dbg = (
                cluster[0].as_ref().map(|c| &self.inner.text[c.text_range()]),
                cluster[0]
                    .as_ref()
                    .map(|c| if c.is_word_boundary() { " W" } else { "" })
                    .unwrap_or_default(),
                focus.index(),
                focus.affinity(),
                cluster[1].as_ref().map(|c| &self.inner.text[c.text_range()]),
                cluster[1]
                    .as_ref()
                    .map(|c| if c.is_word_boundary() { " W" } else { "" })
                    .unwrap_or_default(),
            );
            eprintln!(" | visual: {dbg:?}");
        }
        self.inner.selection.selection = new_sel;
    }

    // #[cfg(feature = "accesskit")]
    // /// Perform an accessibility update, assuming that the layout is valid.
    // ///
    // /// The wrapper [`accessibility`](PlainEditorDriver::accessibility) on the driver type should
    // /// be preferred.
    // ///
    // /// You should always call [`refresh_layout`](Self::refresh_layout) before using this method,
    // /// with no other modifying method calls in between.
    // pub(crate) fn accessibility_unchecked(
    //     &mut self,
    //     update: &mut TreeUpdate,
    //     node: &mut Node,
    //     next_node_id: impl FnMut() -> NodeId,
    //     x_offset: f64,
    //     y_offset: f64,
    // ) {
    //     self.inner.layout_access.build_nodes(
    //         &self.inner.text,
    //         &self.inner.layout,
    //         update,
    //         node,
    //         next_node_id,
    //         x_offset,
    //         y_offset,
    //     );
    //     if self.inner.show_cursor {
    //         if let Some(selection) = self
    //             .selection
    //             .to_access_selection(&self.inner.layout, &self.inner.layout_access)
    //         {
    //             node.set_text_selection(selection);
    //         }
    //     } else {
    //         node.clear_text_selection();
    //     }
    //     node.add_action(accesskit::Action::SetTextSelection);
    // }


    // --- MARK: Cursor Movement ---
    /// Move the cursor to the cluster boundary nearest this point in the layout.
    pub(crate) fn move_to_point(&mut self, x: f32, y: f32) {
        self.set_selection(Selection::from_point(&self.inner.layout, x, y));
    }

    /// Move the cursor to the start of the text.
    pub(crate) fn move_to_text_start(&mut self) {
        self.set_selection(
            self.inner.selection
                .selection
                .move_lines(&self.inner.layout, isize::MIN, false),
        );
    }

    /// Move the cursor to the start of the physical line.
    pub(crate) fn move_to_line_start(&mut self) {
        self.set_selection(self.inner.selection.selection.line_start(&self.inner.layout, false));
    }

    /// Move the cursor to the end of the text.
    pub(crate) fn move_to_text_end(&mut self) {
        self.set_selection(
            self.inner.selection
                .selection
                .move_lines(&self.inner.layout, isize::MAX, false),
        );
    }

    /// Move the cursor to the end of the physical line.
    pub(crate) fn move_to_line_end(&mut self) {
        self.set_selection(self.inner.selection.selection.line_end(&self.inner.layout, false));
    }

    /// Move up to the closest physical cluster boundary on the previous line, preserving the horizontal position for repeated movements.
    pub(crate) fn move_up(&mut self) {
        self.set_selection(self.inner.selection.selection.previous_line(&self.inner.layout, false));
    }

    /// Move down to the closest physical cluster boundary on the next line, preserving the horizontal position for repeated movements.
    pub(crate) fn move_down(&mut self) {
        self.set_selection(self.inner.selection.selection.next_line(&self.inner.layout, false));
    }

    /// Move to the next cluster left in visual order.
    pub(crate) fn move_left(&mut self) {
        self.set_selection(
            self.inner.selection
                .selection
                .previous_visual(&self.inner.layout, false),
        );
    }

    /// Move to the next cluster right in visual order.
    pub(crate) fn move_right(&mut self) {
        self.set_selection(self.inner.selection.selection.next_visual(&self.inner.layout, false));
    }

    /// Move to the next word boundary left.
    pub(crate) fn move_word_left(&mut self) {
        self.set_selection(
            self.inner.selection
                .selection
                .previous_visual_word(&self.inner.layout, false),
        );
    }

    /// Move to the next word boundary right.
    pub(crate) fn move_word_right(&mut self) {
        self.set_selection(
            self.inner.selection
                .selection
                .next_visual_word(&self.inner.layout, false),
        );
    }

    /// Select the whole text.
    pub(crate) fn select_all(&mut self) {
        self.set_selection(
            Selection::from_byte_index(&self.inner.layout, 0_usize, Affinity::default()).move_lines(
                &self.inner.layout,
                isize::MAX,
                true,
            ),
        );
    }

    /// Collapse selection into caret.
    pub(crate) fn collapse_selection(&mut self) {
        self.set_selection(self.inner.selection.selection.collapse());
    }

    /// Move the selection focus point to the cluster boundary closest to point.
    pub(crate) fn extend_selection_to_point(&mut self, x: f32, y: f32) {
        self.inner.selection.extend_selection_to_point(&self.inner.layout, x, y);
    }

    pub(crate) fn layout(&mut self) -> &Layout<ColorBrush> {
        self.refresh_layout();
        &self.inner.layout
    }

    pub(crate) fn refresh_layout(&mut self) {
        if self.inner.needs_relayout {
            self.rebuild_layout(None, false);
        }
    }
}



pub use parley::Rect;

pub(crate) trait Ext1 {
    fn hit_bounding_box(&mut self, cursor_pos: (f64, f64)) -> bool;
}
impl<'a> Ext1 for TextBox<'a> {
    fn hit_bounding_box(&mut self, cursor_pos: (f64, f64)) -> bool {
        let offset = (
            cursor_pos.0 as f64 - self.inner.left,
            cursor_pos.1 as f64 - self.inner.top,
        );

        // todo: does this need to refresh layout? if yes, also need to set the stupid thread local style
        assert!(!self.inner.needs_relayout);
        let hit = offset.0 > -X_TOLERANCE
            && offset.0 < self.inner.layout.full_width() as f64 + X_TOLERANCE
            && offset.1 > 0.0
            && offset.1 < self.inner.layout.height() as f64;

        return hit;
    }
}

impl SelectionState {
    pub(crate) fn handle_event(
        &mut self,
        event: &winit::event::WindowEvent,
        input_state: &TextInputState,
        layout: &Layout<ColorBrush>,
        left: f32,
        top: f32,
        scroll_offset_x: f32,
        edit_showing_placeholder: bool
    ) {
        if edit_showing_placeholder {
            if let WindowEvent::MouseInput { state, button, .. } = event {
                if *button == winit::event::MouseButton::Left && state.is_pressed() {
                    self.selection = Selection::zero();
                    return
                }
            }
        }

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_pos = (position.x as f32, position.y as f32);
                // macOS seems to generate a spurious move after selecting word?
                if input_state.mouse.pointer_down {
                    let cursor_pos = (
                        cursor_pos.0 - left as f32 + scroll_offset_x,
                        cursor_pos.1 - top as f32,
                    );
                    self.extend_selection_to_point(
                        &layout,
                        cursor_pos.0,
                        cursor_pos.1,
                    );
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let shift = input_state.modifiers.state().shift_key();
                if *button == winit::event::MouseButton::Left {

                    let cursor_pos = (
                        input_state.mouse.cursor_pos.0 as f32 - left + scroll_offset_x,
                        input_state.mouse.cursor_pos.1 as f32 - top,
                    );

                    if state.is_pressed() {
                        let click_count = input_state.mouse.click_count;
                        match click_count {
                            2 => self.select_word_at_point(layout, cursor_pos.0, cursor_pos.1),
                            3 => self.select_line_at_point(layout, cursor_pos.0, cursor_pos.1),
                            _ => {
                                if shift {
                                    self.shift_click_extension(
                                        layout,
                                        cursor_pos.0,
                                        cursor_pos.1,
                                    )
                                } else {
                                    self.move_to_point(layout, cursor_pos.0, cursor_pos.1)
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {
                    return;
                }
                let mods_state = input_state.modifiers.state();
                let shift = mods_state.shift_key();
                let action_mod = if cfg!(target_os = "macos") {
                    mods_state.super_key()
                } else {
                    mods_state.control_key()
                };

                if shift {
                    match &event.logical_key {
                        Key::Named(NamedKey::ArrowLeft) => {
                            if action_mod {
                                self.select_word_left(layout);
                            } else {
                                self.select_left(layout);
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if action_mod {
                                self.select_word_right(layout);
                            } else {
                                self.select_right(layout);
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            self.select_up(layout);
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            self.select_down(layout);
                        }
                        Key::Named(NamedKey::Home) => {
                            if action_mod {
                                self.select_to_text_start(layout);
                            } else {
                                self.select_to_line_start(layout);
                            }
                        }
                        Key::Named(NamedKey::End) => {
                            if action_mod {
                                self.select_to_text_end(layout);
                            } else {
                                self.select_to_line_end(layout);
                            }
                        }
                        _ => (),
                    }
                }
            }
            _ => {}
        }
    }

    /// Move the cursor to the cluster boundary nearest this point in the layout.
    pub(crate) fn move_to_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        self.set_selection(Selection::from_point(layout, x, y));
    }

    pub(crate) fn select_word_at_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        self.set_selection(Selection::word_from_point(layout, x, y));
    }

    /// Select the physical line at the point.
    pub(crate) fn select_line_at_point(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        let line = Selection::line_from_point(layout, x, y);
        self.set_selection(line);
    }

    /// Move the selection focus point to the cluster boundary closest to point.
    pub(crate) fn extend_selection_to_point(
        &mut self,
        layout: &Layout<ColorBrush>,
        x: f32,
        y: f32,
    ) {
        self.set_selection(
            self.selection.extend_to_point(layout, x, y),
        );
    }

    /// Update the selection, and nudge the `Generation` if something other than `h_pos` changed.
    pub(crate) fn set_selection(&mut self, new_sel: Selection) {
        self.selection = new_sel;
    }

    /// Move the selection focus point to the start of the buffer.
    pub(crate) fn select_to_text_start(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.move_lines(layout, isize::MIN, true);
    }

    /// Move the selection focus point to the start of the physical line.
    pub(crate) fn select_to_line_start(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.line_start(layout, true);
    }

    /// Move the selection focus point to the end of the buffer.
    pub(crate) fn select_to_text_end(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.move_lines(layout, isize::MAX, true);
    }

    /// Move the selection focus point to the end of the physical line.
    pub(crate) fn select_to_line_end(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.line_end(layout, true);
    }

    /// Move the selection focus point up to the nearest cluster boundary on the previous line, preserving the horizontal position for repeated movements.
    pub(crate) fn select_up(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_line(layout, true);
    }

    /// Move the selection focus point down to the nearest cluster boundary on the next line, preserving the horizontal position for repeated movements.
    pub(crate) fn select_down(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_line(layout, true);
    }

    /// Move the selection focus point to the next cluster left in visual order.
    pub(crate) fn select_left(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_visual(layout, true);
    }

    /// Move the selection focus point to the next cluster right in visual order.
    pub(crate) fn select_right(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_visual(layout, true);
    }

    /// Move the selection focus point to the next word boundary left.
    pub(crate) fn select_word_left(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.previous_visual_word(layout, true);
    }

    /// Move the selection focus point to the next word boundary right.
    pub(crate) fn select_word_right(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.next_visual_word(layout, true);
    }
}

impl Ext1 for TextBoxInner {
    fn hit_bounding_box(&mut self, cursor_pos: (f64, f64)) -> bool {
        let offset = (
            cursor_pos.0 as f64 - self.left,
            cursor_pos.1 as f64 - self.top,
        );

        // todo: does this need to refresh layout? if yes, also need to set the stupid thread local style
        assert!(!self.needs_relayout);
        let hit = offset.0 > -X_TOLERANCE
            && offset.0 < self.layout.full_width() as f64 + X_TOLERANCE
            && offset.1 > 0.0
            && offset.1 < self.layout.height() as f64;

        return hit;
    }
}

