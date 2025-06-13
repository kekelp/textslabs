use std::{
    cell::RefCell, sync::{Arc, RwLock}, time::Instant
};

use parley::*;
use winit::{
    event::WindowEvent, keyboard::{Key, NamedKey}, platform::modifier_supplement::KeyEventExtModifierSupplement, window::Window
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
    pub(crate) height: f32, 
    pub(crate) alignment: Alignment,
    pub(crate) modifiers: Modifiers,
    pub(crate) scale: f32,
    pub(crate) clip_rect: Option<parley::Rect>,
    
    pub(crate) selectable: bool,

    pub(crate) hidden: bool,

}

lazy_static::lazy_static! {
    pub static ref DEFAULT_TEXT_STYLE: SharedStyle = SharedStyle::new(TextStyle { 
        brush: ColorBrush([255,255,255,255]),
        font_size: 24.0,
        ..Default::default()
    });
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
    pub(crate) fn with_text_style<F, R>(&mut self, f: F) -> R
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
    pub(crate) fn new() -> Self {
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
    pub fn new(text: T, pos: (f64, f64), size: (f32, f32), depth: f32) -> Self {
        Self {
            text,
            shared_style_version: 0,
            layout: Layout::new(),
            selectable: true,
            needs_relayout: true,
            left: pos.0,
            top: pos.1,
            max_advance: size.0,
            height: size.1,
            depth,
            selection: SelectionState::new(),
            style: Style::default(),
            width: Some(size.0), 
            alignment: Default::default(),
            modifiers: Default::default(),
            scale: Default::default(),
            clip_rect: None,
            hidden: false,
        }
    }

    pub fn layout(&mut self) -> &Layout<ColorBrush> {
        self.refresh_layout();
        &self.layout
    }

    pub(crate) fn hit_full_rect(&self, cursor_pos: (f64, f64)) -> bool {
        let hit = cursor_pos.0 > -X_TOLERANCE
            && cursor_pos.0 < self.max_advance as f64 + X_TOLERANCE
            && cursor_pos.1 > 0.0
            && cursor_pos.1 < self.height as f64;

        return hit;
    }

    pub(crate) fn refresh_layout(&mut self) {
        self.style.with_text_style(|style, version| {
            let shared_style_changed = if let Some(version) = version {
                self.shared_style_version != version
            } else { false };

            if self.needs_relayout || shared_style_changed {
                // todo: deduplicate
                with_text_cx(|layout_cx, font_cx| {
                    let mut builder = layout_cx.tree_builder(font_cx, 1.0, true, style);
        
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

    pub(crate) fn update_layout(&mut self) {
        self.style.with_text_style(|style, _version| {

            // todo: deduplicate
            with_text_cx(|layout_cx, font_cx| {
                let mut builder = layout_cx.tree_builder(font_cx, 1.0, true, style);
    
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

    pub fn handle_event(&mut self, event: &WindowEvent, _window: &Window, focus_already_grabbed: bool) -> TextEventResult {
        if self.hidden {
            return TextEventResult::new(false);
        }
        
        let initial_selection = self.selection.selection;
        
        let mut result = TextEventResult::new(false);
        
        self.update_mouse_state(event);
        
        if focus_already_grabbed {
            self.reset_selection();
            result.focus_grabbed = false;
        } else {
            let focus_grabbed = self.update_focus(event, focus_already_grabbed, false);
            result.focus_grabbed = focus_grabbed;
        }

        self.handle_event_no_edit_inner(event);

        if selection_decorations_changed(initial_selection, self.selection.selection, false, false, false) {
            result.set_decorations_changed();
        }

        return result;
    }

    pub(crate) fn handle_event_no_edit_inner(&mut self, event: &winit::event::WindowEvent) {
        if self.hidden {
            return;
        }
        if !self.selection.focused {
            return;
        }
        if !self.selectable {
            self.reset_selection();
            return;
        }

        self.refresh_layout();
        
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

    pub(crate) fn update_mouse_state(&mut self, event: &WindowEvent) {
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
                    // todo: don't do this multiple times o algo
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
    }

    pub(crate) fn reset_selection(&mut self) {
        self.set_selection(self.selection.selection.collapse());
        self.selection.focused = false;
    }

    pub(crate) fn update_focus(
        &mut self,
        event: &WindowEvent,
        focus_already_grabbed: bool,
        editable: bool,
    ) -> bool {

        if !self.selectable || focus_already_grabbed {
            self.reset_selection();
            return false;
        }

        // real focus handling

        self.refresh_layout();

        match event {
            WindowEvent::MouseInput { state, button, .. } => {
                if *button == winit::event::MouseButton::Left && state.is_pressed() {
                    let offset = (
                        self.selection.cursor_pos.0 as f64 - self.left,
                        self.selection.cursor_pos.1 as f64 - self.top,
                    );

                    let hit = if editable {
                        self.hit_full_rect(offset)
                    } else {
                        self.layout.hit_bounding_box(offset)
                    };

                    if hit {
                        self.selection.focused = true;
                        return true;
                    } else {
                        self.reset_selection();
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
    }

    pub fn set_unique_style(&mut self, style: TextStyle<'static, ColorBrush>) {
        self.style = Style::Unique(style);
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

    pub fn set_clip_rect(&mut self, clip_rect: Option<parley::Rect>) {
        self.clip_rect = clip_rect;
    }
    pub fn clip_rect(&self) -> Option<parley::Rect> {
        self.clip_rect
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        if self.hidden != hidden {
            self.hidden = hidden;
            if hidden {
                self.reset_selection();
            }
        }
    }
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn selection(&self) -> &Selection {
        &self.selection.selection
    }

    pub fn pos(&self) -> (f64, f64) {
        (self.left, self.top)
    }

    pub fn set_pos(&mut self, pos: (f64, f64)) {
        (self.left, self.top) = pos;
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

    pub fn raw_text_mut(&mut self) -> &mut T {
        self.needs_relayout = true;
        &mut self.text
    }

    /// Set the width of the layout.
    pub fn set_size(&mut self, size: (f32, f32)) {
        let relayout = (self.width != Some(size.0)) || (self.height != size.1) || (self.max_advance != size.0);
        self.width = Some(size.0);
        self.height = size.1;
        self.max_advance = size.0;
        if relayout {
            self.needs_relayout = true;
        }
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
    pub(crate) fn move_to_point(&mut self, x: f32, y: f32) {
        self.refresh_layout();
        self.set_selection(Selection::from_point(&self.layout, x, y));
    }

    /// Move the cursor to a byte index.
    ///
    /// No-op if index is not a char boundary.
    pub(crate) fn move_to_byte(&mut self, index: usize) {
        if self.text.as_ref().is_char_boundary(index) {
            self.refresh_layout();
            self.set_selection(self.cursor_at(index).into());
        }
    }

    /// Move the cursor to the start of the text.
    pub(crate) fn move_to_text_start(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .move_lines(&self.layout, isize::MIN, false),
        );
    }

    /// Move the cursor to the start of the physical line.
    pub(crate) fn move_to_line_start(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.line_start(&self.layout, false));
    }

    /// Move the cursor to the end of the text.
    pub(crate) fn move_to_text_end(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .move_lines(&self.layout, isize::MAX, false),
        );
    }

    /// Move the cursor to the end of the physical line.
    pub(crate) fn move_to_line_end(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.line_end(&self.layout, false));
    }

    /// Move up to the closest physical cluster boundary on the previous line, preserving the horizontal position for repeated movements.
    pub(crate) fn move_up(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.previous_line(&self.layout, false));
    }

    /// Move down to the closest physical cluster boundary on the next line, preserving the horizontal position for repeated movements.
    pub(crate) fn move_down(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.next_line(&self.layout, false));
    }

    /// Move to the next cluster left in visual order.
    pub(crate) fn move_left(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .previous_visual(&self.layout, false),
        );
    }

    /// Move to the next cluster right in visual order.
    pub(crate) fn move_right(&mut self) {
        self.refresh_layout();
        self.set_selection(self.selection.selection.next_visual(&self.layout, false));
    }

    /// Move to the next word boundary left.
    pub(crate) fn move_word_left(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .previous_visual_word(&self.layout, false),
        );
    }

    /// Move to the next word boundary right.
    pub(crate) fn move_word_right(&mut self) {
        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .next_visual_word(&self.layout, false),
        );
    }

    /// Select the whole text.
    pub(crate) fn select_all(&mut self) {
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
    pub(crate) fn collapse_selection(&mut self) {
        self.set_selection(self.selection.selection.collapse());
    }

    /// Move the selection focus point to the cluster boundary closest to point.
    pub(crate) fn extend_selection_to_point(&mut self, x: f32, y: f32) {
        self.refresh_layout();
        self.selection.extend_selection_to_point(&self.layout, x, y);
    }

}



pub use parley::Rect;

pub(crate) trait Ext1 {
    fn hit_bounding_box(&self, cursor_pos: (f64, f64)) -> bool;
}
impl Ext1 for Layout<ColorBrush> {
    fn hit_bounding_box(&self, cursor_pos: (f64, f64)) -> bool {
        let hit = cursor_pos.0 > -X_TOLERANCE
            && cursor_pos.0 < self.full_width() as f64 + X_TOLERANCE
            && cursor_pos.1 > 0.0
            && cursor_pos.1 < self.height() as f64;

        return hit;
    }
}

const MULTICLICK_DELAY: f64 = 0.4;

impl SelectionState {
    pub(crate) fn handle_event(
        &mut self,
        event: &winit::event::WindowEvent,
        modifiers: &Modifiers,
        layout: &Layout<ColorBrush>,
        left: f32,
        top: f32,
    ) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_pos = (position.x as f32, position.y as f32);
                // macOS seems to generate a spurious move after selecting word?
                if self.pointer_down {
                    let cursor_pos = (
                        cursor_pos.0 - left as f32,
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

    /// Extend the selection starting from the previous anchor, moving the selection focus point to the cluster boundary closest to point.
    ///
    /// Used for shift-click behavior.
    pub(crate) fn extend_selection_with_anchor(&mut self, layout: &Layout<ColorBrush>, x: f32, y: f32) {
        // restore prev anchor
        if let Some(prev_selection) = self.prev_anchor {
            self.set_selection_with_old_anchor(prev_selection);
        }
        // extend
        let cursor = Cursor::from_point(layout, x, y);
        self.set_selection_with_old_anchor(
            self.selection.extend(cursor),
        );
    }

    /// Update the selection, and nudge the `Generation` if something other than `h_pos` changed.
    pub(crate) fn set_selection(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
        self.prev_anchor = Some(self.selection);
    }

    /// Update the selection without resetting the previous anchor.
    fn set_selection_with_old_anchor(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
    }

    fn set_selection_inner(&mut self, new_sel: Selection) {
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
