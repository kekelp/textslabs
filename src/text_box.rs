use std::{
    cell::RefCell, ops::Range, sync::{Arc, RwLock}, time::{Duration, Instant}
};

use parley::*;
use winit::{
    event::WindowEvent, keyboard::{Key, NamedKey}, platform::modifier_supplement::KeyEventExtModifierSupplement
};
use arboard::Clipboard;

use parley::{Affinity, Alignment, AlignmentOptions, Selection, TextStyle};
use winit::event::Modifiers;

use crate::*;

const X_TOLERANCE: f64 = 35.0;

pub(crate) struct TextContext {
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

pub(crate) fn with_text_cx<R>(f: impl FnOnce(&mut LayoutContext<ColorBrush>, &mut FontContext) -> R) -> R {
    let res = TEXT_CX.with_borrow_mut(|text_cx| f(&mut text_cx.layout_cx, &mut text_cx.font_cx));
    res
}

thread_local! {
    static CLIPBOARD: RefCell<Clipboard> = RefCell::new(Clipboard::new().unwrap());
}

pub(crate) fn with_clipboard<R>(f: impl FnOnce(&mut Clipboard) -> R) -> R {
    let res = CLIPBOARD.with_borrow_mut(|clipboard| f(clipboard));
    res
}

pub struct TextBox<T: AsRef<str>> {
    pub(crate) text: T,
    pub(crate) style: Style,
    pub(crate) shared_style_version: u32,
    pub(crate) layout: Layout<ColorBrush>,
    pub(crate) needs_relayout: bool,
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) max_advance: f32,
    pub(crate) depth: f32,
    pub(crate) selection: SelectionState,
    pub(crate) width: Option<f32>,
    pub(crate) base_height: f32, 
    pub(crate) alignment: Alignment,
    pub(crate) modifiers: Modifiers,
    pub(crate) scale: f32,
    
    pub(crate) selectable: bool,

    pub(crate) editable: bool,

    pub(crate) compose: Option<Range<usize>>,
    pub(crate) show_cursor: bool,
    pub(crate) start_time: Option<Instant>,
    pub(crate) blink_period: Duration,
    pub(crate) history: TextEditHistory,
}

lazy_static::lazy_static! {
    pub static ref DEFAULT_TEXT_STYLE: SharedStyle = SharedStyle::new(TextStyle::default());
}

pub enum Style {
    Shared(SharedStyle),
    Unique(TextStyle<'static, ColorBrush>),
}
impl Default for Style {
    fn default() -> Self {
        Self::Shared(DEFAULT_TEXT_STYLE.clone())
    }
}
impl Style {
    pub fn with_text_style<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&TextStyle<'static, ColorBrush>, Option<u32>) -> R,
    {
        match self {
            Style::Shared(shared) => {
                let inner = shared.0.read().unwrap();
                f(&inner.style, Some(inner.version))
            }
            Style::Unique(style) => f(style, None),
        }
    }
}

// todo: this probably won't be needed actually
// when using it in a declarative library, after you change a style, you just redeclare everything and pass the new style to everyone that needs it
// (and they need to detect changes)
pub struct SharedStyle(Arc<RwLock<InnerStyle>>);
struct InnerStyle {
    style: TextStyle<'static, ColorBrush>,
    version: u32,
}
impl SharedStyle {
    pub fn new(style: TextStyle<'static, ColorBrush>) -> Self {
        Self(Arc::new(RwLock::new(InnerStyle { style, version: 0 })))
    }

    pub fn with_borrow_mut<R>(
        &self,
        f: impl FnOnce(&mut TextStyle<'static, ColorBrush>) -> R,
    ) -> R {
        let mut inner = self.0.write().unwrap();
        inner.version += 1;
        f(&mut inner.style)
    }
}
impl Clone for SharedStyle {
    fn clone(&self) -> Self {
        SharedStyle(self.0.clone())
    }
}

pub struct SelectionState {
    pub(crate) selection: Selection,
    pub(crate) prev_anchor: Option<Selection>,
    pub(crate) pointer_down: bool,
    pub(crate) focused: bool,
    pub(crate) last_click_time: Option<Instant>,
    pub(crate) click_count: u32,
    pub(crate) cursor_pos: (f32, f32),
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

impl<T: AsRef<str>> TextBox<T> {
    pub fn new(text: T, pos: (f64, f64), size: (f32, f32), depth: f32, editable: bool) -> Self {
        let history = if editable {
            TextEditHistory::new()
        } else {
            TextEditHistory::empty()
        };
        Self {
            text,
            shared_style_version: 0,
            layout: Layout::new(),
            selectable: true,
            needs_relayout: true,
            left: pos.0,
            top: pos.1,
            max_advance: size.0,
            base_height: size.1,
            depth,
            selection: SelectionState::new(),
            style: Style::default(),
            compose: Default::default(),
            show_cursor: true,
            width: Some(size.0), 
            alignment: Default::default(),
            start_time: Default::default(),
            blink_period: Default::default(),
            modifiers: Default::default(),
            scale: Default::default(),
            history,
            editable,
        }
    }

    pub fn layout(&mut self) -> &Layout<ColorBrush> {
        self.refresh_layout();
        &self.layout
    }

    pub fn hit_full_rect(&self, cursor_pos: (f64, f64)) -> bool {
        let hit = cursor_pos.0 > -X_TOLERANCE
            && cursor_pos.0 < self.max_advance as f64 + X_TOLERANCE
            && cursor_pos.1 > 0.0
            && cursor_pos.1 < self.base_height as f64;

        return hit;
    }

    pub fn refresh_layout(&mut self) {
        self.style.with_text_style(|style, version| {
            let shared_style_changed = if let Some(version) = version {
                self.shared_style_version == version
            } else { false };

            if self.needs_relayout || shared_style_changed {
                // todo: deduplicate
                with_text_cx(|layout_cx, font_cx| {
                    let mut builder =
                        layout_cx
                            .tree_builder(font_cx, 1.0, style);
        
                    builder.push_text(&self.text.as_ref());
        
                    let (mut layout, _) = builder.build();
        
                    layout.break_all_lines(Some(self.max_advance));
                    layout.align(
                        Some(self.max_advance),
                        self.alignment,
                        AlignmentOptions::default(),
                    );
        
                    self.layout = layout;
                    self.needs_relayout = false;
                });
            }
        });
    }

    pub fn update_layout(&mut self) {
        self.style.with_text_style(|style, _version| {

            // todo: deduplicate
            with_text_cx(|layout_cx, font_cx| {
                let mut builder =
                    layout_cx
                        .tree_builder(font_cx, 1.0, style);
    
                builder.push_text(&self.text.as_ref());
    
                let (mut layout, _) = builder.build();
    
                layout.break_all_lines(Some(self.max_advance));
                layout.align(
                    Some(self.max_advance),
                    self.alignment,
                    AlignmentOptions::default(),
                );
    
                self.layout = layout;
                self.needs_relayout = false;
            });
            
        });
    }

    pub fn handle_event_no_edit(&mut self, event: &winit::event::WindowEvent) {

    }

    pub fn handle_event_no_edit_inner(&mut self, event: &winit::event::WindowEvent) {
        if !self.selection.focused {
            self.show_cursor = false;
            return;
        }
        if !self.selectable {
            self.selection.focused = false;
            self.show_cursor = false;
            return;
        }

        self.refresh_layout();

        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                if state.is_pressed() {
                    if *button == winit::event::MouseButton::Left {
                        let offset = (
                            self.selection.cursor_pos.0 as f64 - self.left,
                            self.selection.cursor_pos.1 as f64 - self.top,
                        );

                        let hit = if self.editable {
                            self.hit_full_rect(offset)
                        } else {
                            self.layout.hit_bounding_box(offset)
                        };

                        if !hit {
                            self.selection.focused = false;
                            self.selection.set_selection(self.selection.selection.collapse());
                        }
                    }
                } else {
                    self.selection.pointer_down = false;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_pos = (position.x as f32, position.y as f32);
                // macOS seems to generate a spurious move after selecting word?
                if self.selection.pointer_down {
                    let cursor_pos = (
                        cursor_pos.0 - self.left as f32,
                        cursor_pos.1 - self.top as f32,
                    );
                    self.selection.extend_selection_to_point(
                        &self.layout,
                        cursor_pos.0,
                        cursor_pos.1,
                        true,
                    );
                }
            }
            _ => {}
        }

        // if !self.selection.focused {
        //     return;
        // }

        self.selection.handle_event(
            event,
            &self.modifiers,
            &self.layout,
            self.left as f32,
            self.top as f32,
        );
        
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {}
                #[allow(unused)]
                let mods_state = self.modifiers.state();
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
                                "a" => {
                                    self.selection.selection = Selection::from_byte_index(
                                        &self.layout,
                                        0_usize,
                                        Affinity::default(),
                                    )
                                    .move_lines(&self.layout, isize::MAX, true);
                                }
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

    pub fn update_focus(
        &mut self,
        event: &WindowEvent,
        focus_already_grabbed: bool,
    ) -> bool {
        // dumb bookkeeping
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if *button == winit::event::MouseButton::Left && state.is_pressed() {
                    let offset = (
                        self.selection.cursor_pos.0 as f64 - self.left,
                        self.selection.cursor_pos.1 as f64 - self.top,
                    );
                    if self.hit_full_rect(offset) {
                        self.selection.pointer_down = true;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_pos = (position.x as f32, position.y as f32);
                self.selection.cursor_pos = cursor_pos;
            }
            _ => {}
        }

        if !self.selectable || focus_already_grabbed {
            self.set_selection(self.selection.selection.collapse());
            self.selection.focused = false;
            return false;
        }

        self.refresh_layout();

        // real focus handling
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                if *button == winit::event::MouseButton::Left && state.is_pressed() {
                    let offset = (
                        self.selection.cursor_pos.0 as f64 - self.left,
                        self.selection.cursor_pos.1 as f64 - self.top,
                    );

                    let hit = if self.editable {
                        self.hit_full_rect(offset)
                    } else {
                        self.layout.hit_bounding_box(offset)
                    };

                    if hit {
                        self.selection.focused = true;
                        return true;
                    } else {
                        return false;
                    }
                }
            }
            _ => {}
        }
        return false;
    }

    pub fn focused(&self) -> bool {
        self.selection.focused
    }

    pub fn set_shared_style(&mut self, style: &SharedStyle) {
        self.style = Style::Shared(style.clone());
        self.needs_relayout = true;
    }

    pub fn set_unique_style(&mut self, style: TextStyle<'static, ColorBrush>) {
        self.style = Style::Unique(style);
        self.needs_relayout = true;
    }

    pub fn set_selectable(&mut self, value: bool) {
        self.selectable = value;
    }
    pub fn selectable(&self) -> bool {
        self.selectable
    }

    pub fn set_depth(&mut self, value: f32) {
        self.depth = value;
    }
    pub fn depth(&self) -> f32 {
        self.depth
    }
}

pub(crate) trait Ext1 {
    fn hit_bounding_box(&self, cursor_pos: (f64, f64)) -> bool;
}
impl Ext1 for Layout<ColorBrush> {
    fn hit_bounding_box(&self, cursor_pos: (f64, f64)) -> bool {
        let hit = cursor_pos.0 > -X_TOLERANCE
            && cursor_pos.0 < self.max_content_width() as f64 + X_TOLERANCE
            && cursor_pos.1 > 0.0
            && cursor_pos.1 < self.height() as f64;

        return hit;
    }
}

const MULTICLICK_DELAY: f64 = 0.4;

impl SelectionState {
    pub fn handle_event(
        &mut self,
        event: &winit::event::WindowEvent,
        modifiers: &Modifiers,
        layout: &Layout<ColorBrush>,
        left: f32,
        top: f32,
    ) {
        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                let shift = modifiers.state().shift_key();
                if *button == winit::event::MouseButton::Left {
                    self.pointer_down = state.is_pressed();

                    let cursor_pos = (
                        self.cursor_pos.0 as f32 - left,
                        self.cursor_pos.1 as f32 - top,
                    );

                    if self.pointer_down {
                        let now = Instant::now();
                        if let Some(last) = self.last_click_time.take() {
                            if now.duration_since(last).as_secs_f64() < MULTICLICK_DELAY {
                                self.click_count = (self.click_count + 1) % 4;
                            } else {
                                self.click_count = 1;
                            }
                        } else {
                            self.click_count = 1;
                        }
                        self.last_click_time = Some(now);
                        let click_count = self.click_count;
                        match click_count {
                            2 => self.select_word_at_point(layout, cursor_pos.0, cursor_pos.1),
                            3 => self.select_line_at_point(layout, cursor_pos.0, cursor_pos.1),
                            _ => {
                                if shift {
                                    self.extend_selection_with_anchor(
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
                #[allow(unused)]
                let mods_state = modifiers.state();
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
    pub fn extend_selection_to_point(
        &mut self,
        layout: &Layout<ColorBrush>,
        x: f32,
        y: f32,
        keep_granularity: bool,
    ) {
        // FIXME: This is usually the wrong way to handle selection extension for mouse moves, but not a regression.
        self.set_selection(
            self.selection
                .extend_to_point(layout, x, y, keep_granularity),
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
        self.set_selection_with_old_anchor(self.selection.extend_to_point(layout, x, y, false));
    }

    /// Update the selection, and nudge the `Generation` if something other than `h_pos` changed.
    pub(crate) fn set_selection(&mut self, new_sel: Selection) {
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
        self.selection = self.selection.move_lines(layout, isize::MIN, true);
    }

    /// Move the selection focus point to the start of the physical line.
    pub fn select_to_line_start(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.line_start(layout, true);
    }

    /// Move the selection focus point to the end of the buffer.
    pub fn select_to_text_end(&mut self, layout: &Layout<ColorBrush>) {
        self.selection = self.selection.move_lines(layout, isize::MAX, true);
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

impl<T: AsRef<str>> TextBox<T> {
    pub fn text_real(&self) -> &T {
        &self.text
    }
    pub fn text_mut(&mut self) -> &mut T {
        &mut self.text
    }

    pub fn selection(&self) -> &Selection {
        &self.selection.selection
    }

    pub fn pos(&self) -> (f64, f64) {
        (self.left, self.top)
    }


    /// Borrow the current selection. The indices returned by functions
    /// such as [`Selection::text_range`] refer to the raw text text,
    /// including the IME preedit region, which can be accessed via
    /// [`PlainEditor::raw_text`].
    pub fn raw_selection(&self) -> &Selection {
        &self.selection.selection
    }

    /// If the current selection is not collapsed, returns the text content of
    /// that selection.
    pub fn selected_text(&self) -> Option<&str> {
        if !self.selection.selection.is_collapsed() {
            self.text.as_ref().get(self.selection.selection.text_range())
        } else {
            None
        }
    }

    /// Get rectangles, and their corresponding line indices, representing the selected portions of
    /// text.
    pub fn selection_geometry(&self) -> Vec<(Rect, usize)> {
        // We do not check `self.show_cursor` here, as the IME handling code collapses the
        // selection to a caret in that case.
        self.selection.selection.geometry(&self.layout)
    }

    /// Invoke a callback with each rectangle representing the selected portions of text, and the
    /// indices of the lines to which they belong.
    pub fn selection_geometry_with(&self, f: impl FnMut(Rect, usize)) {
        // We do not check `self.show_cursor` here, as the IME handling code collapses the
        // selection to a caret in that case.
        self.selection.selection.geometry_with(&self.layout, f);
    }

    /// Borrow the text content of the text, including the IME preedit
    /// region if any.
    ///
    /// Application authors should generally prefer [`text`](Self::text). That method excludes the
    /// IME preedit contents, which are not meaningful for applications to access; the
    /// in-progress IME content is not itself what the user intends to write.
    pub fn raw_text(&self) -> &str {
        &self.text.as_ref()
    }

    /// Set the width of the layout.
    pub fn set_width(&mut self, width: Option<f32>) {
        self.width = width;
        self.needs_relayout = true;
    }

    /// Set the alignment of the layout.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
        self.needs_relayout = true;
    }

    /// Set the scale for the layout.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.needs_relayout = true;
    }

    // /// Modify the styles provided for this editor.
    // pub fn edit_styles(&mut self) -> &mut StyleSet<ColorBrush> {
    //     self.needs_relayout = true;
    //     &mut self.default_style
    // }

    // --- MARK: Raw APIs ---
    /// Get the full read-only details from the layout, if valid.
    ///
    /// Returns `None` if the layout is not up-to-date.
    /// You can call [`refresh_layout`](Self::refresh_layout) before using this method,
    /// to ensure that the layout is up-to-date.
    ///
    /// The [`layout`](Self::layout) method should generally be preferred.
    pub fn try_layout(&self) -> Option<&Layout<ColorBrush>> {
        if self.needs_relayout {
            None
        } else {
            Some(&self.layout)
        }
    }

    #[cfg(feature = "accesskit")]
    #[inline]
    /// Perform an accessibility update if the layout is valid.
    ///
    /// Returns `None` if the layout is not up-to-date.
    /// You can call [`refresh_layout`](Self::refresh_layout) before using this method,
    /// to ensure that the layout is up-to-date.
    /// The [`accessibility`](PlainEditorDriver::accessibility) method on the driver type
    /// should be preferred if the contexts are available, which will do this automatically.
    pub fn try_accessibility(
        &mut self,
        update: &mut TreeUpdate,
        node: &mut Node,
        next_node_id: impl FnMut() -> NodeId,
        x_offset: f64,
        y_offset: f64,
    ) -> Option<()> {
        if self.needs_relayout {
            return None;
        }
        self.accessibility_unchecked(update, node, next_node_id, x_offset, y_offset);
        Some(())
    }

    // --- MARK: Internal Helpers ---
    /// Make a cursor at a given byte index.
    pub(crate) fn cursor_at(&self, index: usize) -> Cursor {
        // TODO: Do we need to be non-dirty?
        // FIXME: `Selection` should make this easier
        if index >= self.text.as_ref().len() {
            Cursor::from_byte_index(&self.layout, self.text.as_ref().len(), Affinity::Upstream)
        } else {
            Cursor::from_byte_index(&self.layout, index, Affinity::Downstream)
        }
    }

    /// Update the selection, and nudge the `Generation` if something other than `h_pos` changed.
    pub(crate) fn set_selection(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
        self.selection.prev_anchor = None;
    }

    pub(crate) fn set_selection_inner(&mut self, new_sel: Selection) {
        // if new_sel.focus() != self.selection.selection.focus() || new_sel.anchor() != self.selection.selection.anchor()
        // {
        //     self.generation.nudge();
        // }

        // This debug code is quite useful when diagnosing selection problems.
        #[allow(clippy::print_stderr)] // reason = "unreachable debug code"
        if false {
            let focus = new_sel.focus();
            let cluster = focus.logical_clusters(&self.layout);
            let dbg = (
                cluster[0].as_ref().map(|c| &self.text.as_ref()[c.text_range()]),
                focus.index(),
                focus.affinity(),
                cluster[1].as_ref().map(|c| &self.text.as_ref()[c.text_range()]),
            );
            eprint!("{dbg:?}");
            let cluster = focus.visual_clusters(&self.layout);
            let dbg = (
                cluster[0].as_ref().map(|c| &self.text.as_ref()[c.text_range()]),
                cluster[0]
                    .as_ref()
                    .map(|c| if c.is_word_boundary() { " W" } else { "" })
                    .unwrap_or_default(),
                focus.index(),
                focus.affinity(),
                cluster[1].as_ref().map(|c| &self.text.as_ref()[c.text_range()]),
                cluster[1]
                    .as_ref()
                    .map(|c| if c.is_word_boundary() { " W" } else { "" })
                    .unwrap_or_default(),
            );
            eprintln!(" | visual: {dbg:?}");
        }
        self.selection.selection = new_sel;
    }

    #[cfg(feature = "accesskit")]
    /// Perform an accessibility update, assuming that the layout is valid.
    ///
    /// The wrapper [`accessibility`](PlainEditorDriver::accessibility) on the driver type should
    /// be preferred.
    ///
    /// You should always call [`refresh_layout`](Self::refresh_layout) before using this method,
    /// with no other modifying method calls in between.
    pub(crate) fn accessibility_unchecked(
        &mut self,
        update: &mut TreeUpdate,
        node: &mut Node,
        next_node_id: impl FnMut() -> NodeId,
        x_offset: f64,
        y_offset: f64,
    ) {
        self.layout_access.build_nodes(
            &self.text,
            &self.layout,
            update,
            node,
            next_node_id,
            x_offset,
            y_offset,
        );
        if self.show_cursor {
            if let Some(selection) = self
                .selection
                .to_access_selection(&self.layout, &self.layout_access)
            {
                node.set_text_selection(selection);
            }
        } else {
            node.clear_text_selection();
        }
        node.add_action(accesskit::Action::SetTextSelection);
    }


    // --- MARK: Cursor Movement ---
    /// Move the cursor to the cluster boundary nearest this point in the layout.
    pub fn move_to_point(&mut self, x: f32, y: f32) {
        self.refresh_layout();
        self.set_selection(Selection::from_point(&self.layout, x, y));
    }

    /// Move the cursor to a byte index.
    ///
    /// No-op if index is not a char boundary.
    pub fn move_to_byte(&mut self, index: usize) {
        if self.text.as_ref().is_char_boundary(index) {
            self.refresh_layout();
            self.set_selection(self.cursor_at(index).into());
        }
    }

    /// Move the cursor to the start of the text.
    pub fn move_to_text_start(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .move_lines(&self.layout, isize::MIN, false),
        );
    }

    /// Move the cursor to the start of the physical line.
    pub fn move_to_line_start(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.line_start(&self.layout, false));
    }

    /// Move the cursor to the end of the text.
    pub fn move_to_text_end(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .move_lines(&self.layout, isize::MAX, false),
        );
    }

    /// Move the cursor to the end of the physical line.
    pub fn move_to_line_end(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.line_end(&self.layout, false));
    }

    /// Move up to the closest physical cluster boundary on the previous line, preserving the horizontal position for repeated movements.
    pub fn move_up(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.previous_line(&self.layout, false));
    }

    /// Move down to the closest physical cluster boundary on the next line, preserving the horizontal position for repeated movements.
    pub fn move_down(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.next_line(&self.layout, false));
    }

    /// Move to the next cluster left in visual order.
    pub fn move_left(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .previous_visual(&self.layout, false),
        );
    }

    /// Move to the next cluster right in visual order.
    pub fn move_right(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.next_visual(&self.layout, false));
    }

    /// Move to the next word boundary left.
    pub fn move_word_left(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .previous_visual_word(&self.layout, false),
        );
    }

    /// Move to the next word boundary right.
    pub fn move_word_right(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .next_visual_word(&self.layout, false),
        );
    }

    /// Select the whole text.
    pub fn select_all(&mut self) {
        self.refresh_layout();
        self.set_selection(
            Selection::from_byte_index(&self.layout, 0_usize, Affinity::default()).move_lines(
                &self.layout,
                isize::MAX,
                true,
            ),
        );
    }

    /// Collapse selection into caret.
    pub fn collapse_selection(&mut self) {
        self.set_selection(self.selection.selection.collapse());
    }

    /// Move the selection focus point to the cluster boundary closest to point.
    pub fn extend_selection_to_point(&mut self, x: f32, y: f32, keep_granularity: bool) {
        self.refresh_layout();

        self.selection
            .extend_selection_to_point(&self.layout, x, y, keep_granularity);
    }

}

