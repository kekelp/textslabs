use std::{cell::RefCell, ptr::NonNull};

#[cfg(feature = "accessibility")]
use accesskit::{Node, NodeId, Rect as AccessRect, Role, TreeUpdate};

use parley::*;
use winit::{
    event::WindowEvent, keyboard::{Key, NamedKey}, platform::modifier_supplement::KeyEventExtModifierSupplement, window::Window
};
use arboard::Clipboard;

use parley::{Affinity, Alignment, Selection};

use crate::*;
use slotmap::DefaultKey;

const X_TOLERANCE: f64 = 35.0;

pub(crate) struct TextBoxInner {
    pub(crate) text: Cow<'static, str>,
    pub(crate) style: StyleHandle,
    pub(crate) style_version: u64,
    pub(crate) layout: Layout<ColorBrush>,

    #[cfg(feature = "accessibility")]
    pub(crate) layout_access: LayoutAccessibility,
    #[cfg(feature = "accessibility")]
    pub(crate) accesskit_id: Option<accesskit::NodeId>,

    pub(crate) needs_relayout: bool,
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) max_advance: f32,
    pub(crate) depth: f32,
    pub(crate) selection: SelectionState,
    pub(crate) width: f32,
    pub(crate) height: f32, 
    pub(crate) alignment: Alignment,
    pub(crate) scale: f32,
    pub(crate) clip_rect: Option<parley::BoundingBox>,
    pub(crate) fadeout_clipping: bool,
    pub(crate) auto_clip: bool,
    pub(crate) scroll_offset: (f32, f32),
    
    pub(crate) selectable: bool,

    pub(crate) hidden: bool,
    pub(crate) last_frame_touched: u64,
    pub(crate) can_hide: bool,
    
    // Multi-window support
    pub(crate) window_id: Option<winit::window::WindowId>,
    
    /// Tracks quad storage for fast scrolling
    pub(crate) quad_storage: QuadStorage,
    pub(crate) shared_backref: NonNull<Shared> 
}

/// A struct that refers to a text box stored inside a [`Text`] struct.
/// 
/// This struct can't be created directly. Instead, use [`Text::add_text_box()`] to create one within [`Text`] and get a [`TextBoxHandle`] back.
/// 
/// Then, the handle can be used to get a `TextBoxMut` with [`Text::get_text_box_mut()`].
pub struct TextBoxMut<'a> {
    pub(crate) inner: &'a mut TextBoxInner,
    pub(crate) shared: &'a mut Shared,
    // This key is only useful for set_focus. Yikes!
    pub(crate) key: DefaultKey,
}

/// A struct that refers to a text box stored inside a [`Text`] struct.
/// 
/// This struct can't be created directly. Instead, use [`Text::add_text_box()`] to create one within [`Text`] and get a [`TextBoxHandle`] back.
/// 
/// Then, the handle can be used to get a `TextBox` with [`Text::get_text_box()`].
#[derive(Clone, Copy)]
pub struct TextBox<'a> {
    pub(crate) inner: &'a TextBoxInner,
    pub(crate) shared: &'a Shared,
    // This key is only useful for checking the box is focused.
    pub(crate) key: DefaultKey,
}


/// Remembers the location of the glyph quads corresponding to the text in this text box, in order to allow fast scrolling without relayouting.
#[derive(Debug, Clone, Default)]
pub(crate) struct QuadStorage {
    /// Range into the text renderer quads. If None, it doesn't mean that there are no quads, but rather that the text box was never prepared. 
    pub quad_range: Option<(usize, usize)>,
    /// The scroll offset used when this quad data was generated
    pub last_offset: (f32, f32),
}


thread_local! {
    static CLIPBOARD: RefCell<Clipboard> = RefCell::new(Clipboard::new().unwrap());
}

/// Runs the given closure with mutable access to the thread-local `Clipboard`.
pub fn with_clipboard<R>(f: impl FnOnce(&mut Clipboard) -> R) -> R {
    let res = CLIPBOARD.with_borrow_mut(|clipboard| f(clipboard));
    res
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
    pub(crate) fn new(text: impl Into<Cow<'static, str>>, pos: (f64, f64), size: (f32, f32), depth: f32, default_style_key: DefaultKey, shared_backref: NonNull<Shared>) -> Self {
        Self {
            text: text.into(),
            style_version: 0,
            layout: Layout::new(),
            #[cfg(feature = "accessibility")]
            layout_access: LayoutAccessibility::default(),
            #[cfg(feature = "accessibility")]
            accesskit_id: None,
            selectable: true,
            needs_relayout: true,
            left: pos.0,
            top: pos.1,
            max_advance: size.0,
            height: size.1,
            depth,
            selection: SelectionState::new(),
            style: StyleHandle { key: default_style_key },
            width: size.0, 
            alignment: Default::default(),
            scale: Default::default(),
            clip_rect: None,
            fadeout_clipping: false,
            auto_clip: false,
            scroll_offset: (0.0, 0.0),
            hidden: false,
            last_frame_touched: 0,
            can_hide: false,
            window_id: None,
            quad_storage: QuadStorage::default(),
            shared_backref,
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


macro_rules! impl_for_textbox_and_textboxmut {
    ($($items:tt)*) => {
        impl<'a> TextBox<'a> {
            $($items)*
        }       
        impl<'a> TextBoxMut<'a> {
            $($items)*
        }
    };
}

#[cfg(feature = "accessibility")]
impl_for_textbox_and_textboxmut! {
    pub fn accesskit_node(&self) -> Node {
        let mut node = Node::new(Role::Label);
        // let mut node = Node::new(Role::Paragraph);
        let text_content = self.inner.text.to_string();
        node.set_value(text_content.clone());
        node.set_description(text_content);
        
        let (left, top) = self.pos();
        let bounds = AccessRect::new(
            left,
            top,
            left + self.inner.layout.width() as f64,
            top + self.inner.layout.height() as f64
        );

        node.set_bounds(bounds);

        return node;
    }
}

impl_for_textbox_and_textboxmut! {
    /// Returns a reference to the current style of the text box.
    pub fn style(&'a self) -> &'a TextStyle2 {
        &self.shared.styles[self.inner.style.key].text_style
    }

    /// Returns `true` if the text box is currently hidden.
    pub fn hidden(&self) -> bool {
        self.inner.hidden
    }

    /// Returns the current depth (z) of the text box.
    pub fn depth(&self) -> f32 {
        self.inner.depth
    }

    /// Returns a reference to the text in the text nox. 
    pub fn text(self) -> &'a str {
        &self.inner.text
    }

    /// Returns the current position of the text box.
    pub fn position(&self) -> (f64, f64) {
        (self.inner.left, self.inner.top)
    }

    /// Returns the current clip rect of the text box.
    pub fn clip_rect(&self) -> Option<parley::BoundingBox> {
        self.inner.clip_rect
    }
    
    /// Returns `true` if the text box is set to use a fade effect when the contained text overflows its clip rect.
    pub fn fadeout_clipping(&self) -> bool {
        self.inner.fadeout_clipping
    }

    /// Returns the currently selected text, or `None` if no text is currently selected.
    pub fn selected_text(&self) -> Option<&str> {
        if !self.inner.selection.selection.is_collapsed() {
            self.inner.text.get(self.inner.selection.selection.text_range())
        } else {
            None
        }
    }

    /// Returns the current selection of the text box.
    pub fn selection(&self) -> Selection {
        self.inner.selection.selection
    }

    /// Returns the current scroll offset of the text box.
    pub fn scroll_offset(&self) -> (f32, f32) {
        self.inner.scroll_offset
    }

    /// Returns `true` if the text in the text box is currently selectable.
    pub fn selectable(&self) -> bool {
        self.inner.selectable
    }

    #[doc(hidden)] 
    pub fn can_hide(&self) -> bool {
        self.inner.can_hide
    }

    /// Returns the range of prepared quads in the [`TextRenderer`]'s buffer.
    /// 
    /// Must be called after [`Text::prepare_all()`].
    pub fn quad_range(&self) -> QuadRanges {
        self.quad_range_impl(false)
    }

    pub(crate) fn quad_range_impl(&self, edit: bool) -> QuadRanges {
        debug_assert!(self.inner.quad_storage.quad_range.is_some(), "Quad range called before this text box was prepared.");
        let glyph_range = self.inner.quad_storage.quad_range.unwrap_or_else(|| (0,0));
        let is_focused = match self.shared.focused {
            Some(AnyBox::TextBox(f)) if !edit => f == self.key,
            Some(AnyBox::TextEdit(f)) if edit => f == self.key,
            _ => false,
        };
        let decorations_range = if is_focused {
            self.shared.decorations_range
        } else {
            (0,0)
        };
        return QuadRanges { glyph_range, decorations_range }
    }
}

/// Ranges of this texbox's quads in the [`TextRenderer`]'s buffer.
pub struct QuadRanges {
    /// Range of the glyph quads.
    pub glyph_range: (usize, usize),
    /// Range of the decoration quads.
    pub decorations_range: (usize, usize),
}

impl<'a> TextBoxMut<'a> {

    pub(crate) fn effective_clip_rect(&self) -> Option<parley::BoundingBox> {
        let auto_clip_rect = if self.inner.auto_clip {
            Some(parley::BoundingBox {
                x0: self.inner.scroll_offset.0 as f64,
                y0: self.inner.scroll_offset.1 as f64,
                x1: (self.inner.scroll_offset.0 + self.inner.max_advance) as f64,
                y1: (self.inner.scroll_offset.1 + self.inner.height) as f64,
            })
        } else {
            None
        };

        let clip_rect = self.inner.clip_rect.map(|explicit| {
            parley::BoundingBox {
                x0: explicit.x0 + self.inner.scroll_offset.0 as f64,
                y0: explicit.y0 + self.inner.scroll_offset.1 as f64,
                x1: explicit.x1 + self.inner.scroll_offset.0 as f64,
                y1: explicit.y1 + self.inner.scroll_offset.1 as f64,
            }
        });

        match (auto_clip_rect, clip_rect) {
            (None, None) => None,
            (Some(auto), None) => Some(auto),
            (None, Some(explicit)) => Some(explicit),
            (Some(auto), Some(explicit)) => {
                let x0 = auto.x0.max(explicit.x0);
                let y0 = auto.y0.max(explicit.y0);
                let x1 = auto.x1.min(explicit.x1);
                let y1 = auto.y1.min(explicit.y1);
                
                if x0 < x1 && y0 < y1 {
                    Some(parley::BoundingBox { x0, y0, x1, y1 })
                } else {
                    Some(parley::BoundingBox { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 })
                }
            }
        }
    }

    #[cfg(feature = "accessibility")]
    /// Pushes an accessibility update for this text box.
    pub fn push_accesskit_update(&mut self, tree_update: &mut TreeUpdate) {
        let accesskit_id = self.inner.accesskit_id;
        let node = self.accesskit_node();
        let (left, top) = self.pos();
        
        push_accesskit_update_text_box_partial_borrows(
            accesskit_id,
            node,
            &mut self.inner,
            tree_update,
            left,
            top,
            self.shared.node_id_generator,
        );
    }

    #[cfg(feature = "accessibility")]
    pub(crate) fn push_accesskit_update_to_self(&mut self) {
        let accesskit_id = self.inner.accesskit_id;
        let node = self.accesskit_node();
        let (left, top) = self.pos();
        
        push_accesskit_update_text_box_partial_borrows(
            accesskit_id,
            node,
            &mut self.inner,
            &mut self.shared.accesskit_tree_update,
            left,
            top,
            self.shared.node_id_generator,
        );
    }

    pub(crate) fn handle_event(&mut self, event: &WindowEvent, _window: &Window, input_state: &TextInputState) -> bool {
        if self.inner.hidden {
            return false;
        }

        let initial_selection = self.inner.selection.selection;

        let mut consumed = self.handle_event_no_edit(event, input_state, false);

        // Handle mouse wheel scrolling for multi-line text boxes with auto_clip
        if let WindowEvent::MouseWheel { delta, .. } = event {
            if self.inner.auto_clip {
                let cursor_pos = input_state.mouse.cursor_pos;
                if self.hit_full_rect(cursor_pos) {
                    let scroll_amount = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_x, y) => y * 30.0,
                        winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                    };

                    if scroll_amount.abs() > 0.1 {
                        let old_scroll = self.inner.scroll_offset.1;
                        let new_scroll = old_scroll - scroll_amount;

                        self.refresh_layout();
                        let total_text_height = self.inner.layout.height();
                        let text_height = self.inner.height;
                        let max_scroll = (total_text_height - text_height).max(0.0).round();
                        let new_scroll = new_scroll.clamp(0.0, max_scroll).round();

                        if (new_scroll - old_scroll).abs() > 0.1 {
                            self.inner.scroll_offset.1 = new_scroll;
                            self.shared.scrolled = true;
                            consumed = true;
                        }
                    }
                }
            }
        }

        if selection_decorations_changed(initial_selection, self.inner.selection.selection, false, false, false) {
            self.shared.decorations_changed = true;
        }

        return consumed;
    }

    /// The output bool says if the event was consumed by this text box.
    pub(crate) fn handle_event_no_edit(&mut self, event: &WindowEvent, input_state: &TextInputState, enable_auto_scroll: bool) -> bool {
        if self.inner.hidden {
            return false;
        }
        if !self.inner.selectable {
            self.reset_selection();
            return false;
        }

        let mut consumed = false;

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_pos = (position.x as f32, position.y as f32);
                // macOS seems to generate a spurious move after selecting word?
                if input_state.mouse.pointer_down {
                    let left = self.inner.left as f32;
                    let top = self.inner.top as f32;
                    let scroll_offset_x = self.inner.scroll_offset.0;
                    let scroll_offset_y = self.inner.scroll_offset.1;
                    let max_advance = self.inner.max_advance;
                    let height = self.inner.height;

                    // Check for auto-scroll when dragging near borders (only for text edits)
                    let mut new_scroll_x = scroll_offset_x;
                    let mut new_scroll_y = scroll_offset_y;

                    if enable_auto_scroll {
                        let scroll_margin = 20.0; // Distance from border to trigger auto-scroll
                        let scroll_speed = 5.0; // Scroll speed in pixels
                        let mut did_scroll = false;

                        // Check horizontal auto-scroll
                        if cursor_pos.0 - left < scroll_margin {
                            // Near left border - scroll left
                            new_scroll_x = (scroll_offset_x - scroll_speed).max(0.0);
                            if new_scroll_x != scroll_offset_x {
                                did_scroll = true;
                            }
                        } else if cursor_pos.0 > (left + max_advance) - scroll_margin {
                            // Near right border - scroll right
                            let total_text_width = self.inner.layout.full_width();
                            let max_scroll_x = (total_text_width - max_advance).max(0.0);
                            new_scroll_x = (scroll_offset_x + scroll_speed).min(max_scroll_x);
                            if new_scroll_x != scroll_offset_x {
                                did_scroll = true;
                            }
                        }

                        // Check vertical auto-scroll
                        if cursor_pos.1 - top < scroll_margin {
                            // Near top border - scroll up
                            new_scroll_y = (scroll_offset_y - scroll_speed).max(0.0);
                            if new_scroll_y != scroll_offset_y {
                                did_scroll = true;
                            }
                        } else if cursor_pos.1 > (top + height) - scroll_margin {
                            // Near bottom border - scroll down
                            let total_text_height = self.inner.layout.height();
                            let max_scroll_y = (total_text_height - height).max(0.0);
                            new_scroll_y = (scroll_offset_y + scroll_speed).min(max_scroll_y);
                            if new_scroll_y != scroll_offset_y {
                                did_scroll = true;
                            }
                        }
                        
                        // Apply scroll if needed
                        if did_scroll {
                            self.set_scroll_offset((new_scroll_x, new_scroll_y));
                            self.shared.scrolled = true;
                        }
                    }

                    let cursor_pos = (
                        cursor_pos.0 - left + new_scroll_x,
                        cursor_pos.1 - top + new_scroll_y,
                    );
                    self.inner.selection.extend_selection_to_point(
                        &self.inner.layout,
                        cursor_pos.0,
                        cursor_pos.1,
                    );
                    consumed = true;
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let shift = input_state.modifiers.state().shift_key();
                if *button == winit::event::MouseButton::Left {
                    let cursor_pos = (
                        input_state.mouse.cursor_pos.0 as f32 - self.inner.left as f32 + self.inner.scroll_offset.0,
                        input_state.mouse.cursor_pos.1 as f32 - self.inner.top as f32 + self.inner.scroll_offset.1,
                    );

                    if state.is_pressed() {
                        let click_count = input_state.mouse.click_count;
                        match click_count {
                            2 => self.inner.selection.select_word_at_point(&self.inner.layout, cursor_pos.0, cursor_pos.1),
                            3 => self.inner.selection.select_line_at_point(&self.inner.layout, cursor_pos.0, cursor_pos.1),
                            _ => {
                                if shift {
                                    self.inner.selection.shift_click_extension(
                                        &self.inner.layout,
                                        cursor_pos.0,
                                        cursor_pos.1,
                                    )
                                } else {
                                    self.inner.selection.move_to_point(&self.inner.layout, cursor_pos.0, cursor_pos.1);
                                    self.shared.reset_cursor_blink();
                                }
                            }
                        }
                        consumed = true;
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {
                    return consumed;
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
                                self.inner.selection.select_word_left(&self.inner.layout);
                            } else {
                                self.inner.selection.select_left(&self.inner.layout);
                            }
                            consumed = true;
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if action_mod {
                                self.inner.selection.select_word_right(&self.inner.layout);
                            } else {
                                self.inner.selection.select_right(&self.inner.layout);
                            }
                            consumed = true;
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            self.inner.selection.select_up(&self.inner.layout);
                            consumed = true;
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            self.inner.selection.select_down(&self.inner.layout);
                            consumed = true;
                        }
                        Key::Named(NamedKey::Home) => {
                            if action_mod {
                                self.inner.selection.select_to_text_start(&self.inner.layout);
                            } else {
                                self.inner.selection.select_to_line_start(&self.inner.layout);
                            }
                            consumed = true;
                        }
                        Key::Named(NamedKey::End) => {
                            if action_mod {
                                self.inner.selection.select_to_text_end(&self.inner.layout);
                            } else {
                                self.inner.selection.select_to_line_end(&self.inner.layout);
                            }
                            consumed = true;
                        }
                        _ => (),
                    }
                }

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
                                    });
                                    consumed = true;
                                }
                                "a" => {
                                    self.select_all();
                                    consumed = true;
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

        return consumed;
    }

    pub(crate) fn reset_selection(&mut self) {
        self.set_selection(self.inner.selection.selection.collapse());
    }

    /// Returns a mutable reference to the text content.
    /// 
    /// This returns a `Cow<'static, str>`, which can be set to a `String` or to `'static str`
    /// 
    /// To manipulate the text as a `String`, call `Cow::to_mut()` on the result, or use [`Self::text_mut_string()`]
    pub fn text_mut(&mut self) -> &mut Cow<'static, str> {
        self.inner.needs_relayout = true;
        self.shared.text_changed = true;
        &mut self.inner.text
    }

    /// Returns a mutable reference to the text content as a `String`. If the text was a borrowed `&str`, it will be cloned.
    /// 
    /// This is a convenience method over [`Self::text_mut()`]
    pub fn text_mut_string(&mut self) -> &mut String {
        self.text_mut().to_mut()
    }

    #[cfg(feature = "accessibility")]
    /// Sets the accessibility node ID for this text box.
    pub fn set_accesskit_id(&mut self, accesskit_id: NodeId) {
        self.inner.accesskit_id = Some(accesskit_id);
    }

    #[cfg(feature = "accessibility")]
    /// Returns the accessibility node ID for this text box.
    pub fn accesskit_id(&self) -> Option<NodeId> {
        self.inner.accesskit_id
    }

    /// Sets the position of the text box.
    pub fn set_pos(&mut self, pos: (f64, f64)) {
        (self.inner.left, self.inner.top) = pos;
        self.shared.text_changed = true;
    }

    /// Hides or unhides the text box.
    pub(crate) fn set_hidden(&mut self, hidden: bool) {
        if self.inner.hidden != hidden {
            self.inner.hidden = hidden;

            if hidden {
                self.reset_selection();
            }
        }
        self.shared.text_changed = true;
    }

    /// Sets the depth (z-order) of the text box.
    pub fn set_depth(&mut self, depth: f32) {
        self.inner.depth = depth;
        self.shared.text_changed = true;
    }

    /// Sets the clipping rectangle for the text box.
    pub fn set_clip_rect(&mut self, clip_rect: Option<parley::BoundingBox>) {
        self.inner.clip_rect = clip_rect;
        self.shared.text_changed = true;
    }

    /// Sets whether the text is rendered with an alpha fade when it overflows the clip rectangle.
    pub fn set_fadeout_clipping(&mut self, fadeout_clipping: bool) {
        self.inner.fadeout_clipping = fadeout_clipping;
        self.shared.text_changed = true;
    }

    /// Sets the scroll offset for the text box.
    pub fn set_scroll_offset(&mut self, offset: (f32, f32)) {
        self.inner.scroll_offset = offset;
        self.shared.text_changed = true;
    }

    /// Sets the style for the text box.
    pub fn set_style(&mut self, style: &StyleHandle) {
        self.inner.style = style.sneak_clone();
        self.inner.style_version = self.style_version();
        self.inner.needs_relayout = true;
        self.shared.text_changed = true;
    }

    pub(crate) fn style_version(&self) -> u64 {
        self.shared.styles[self.inner.style.key].version
    }

    pub(crate) fn style_version_changed(&self) -> bool {
        self.style_version() != self.inner.style_version
    }

    #[must_use]
    pub(crate) fn hit_full_rect(&self, cursor_pos: (f64, f64)) -> bool {
        self.inner.hit_full_rect(cursor_pos)
    }

    pub(crate) fn text_inner(&self) -> &str {
        &self.inner.text
    }

    pub(crate) fn get_scale_factor(&self) -> f64 {
        let scale_factor = if let Some(window_id) = self.inner.window_id {           
            self.shared.windows.iter().find(|info| info.window_id == window_id)
                .map(|info| info.scale_factor).unwrap_or(1.0)
        } else {
            self.shared.windows.first().map(|w| w.scale_factor).unwrap_or(1.0)
        };

        scale_factor
    }

    pub(crate) fn rebuild_layout(
        &mut self,
        color_override: Option<ColorBrush>,
        single_line: bool,
    ) {
        let scale_factor = self.get_scale_factor();
        
        // partial_borrows
        let style = &mut &self.shared.styles[self.inner.style.key].text_style;
        
        let layout_cx = &mut self.shared.layout_cx;
        let font_cx = &mut self.shared.font_cx;
        
        let mut builder = layout_cx.tree_builder(font_cx, scale_factor as f32, true, style);

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
    }



    // Note: This used to be a problem when TextEdit couldn't call refresh_layout() directly.
    // Now that TextEdit has access to refresh_layout(), this is no longer an issue. 
    /// Returns a mutable reference to the text box's text buffer as a Cow.
    /// This provides full access to the underlying storage type.


    /// Sets the text to a static string reference.
    pub fn set_static(&mut self, text: &'static str) {
        self.inner.needs_relayout = true;
        self.inner.text = Cow::Borrowed(text);
    }

    /// Sets the size of the text box.
    pub fn set_size(&mut self, size: (f32, f32)) {
        let relayout = (self.inner.width != size.0) || (self.inner.height != size.1) || (self.inner.max_advance != size.0);
        self.inner.width = size.0;
        self.inner.height = size.1;
        self.inner.max_advance = size.0;
        if relayout {
            self.inner.needs_relayout = true;
        }
    }

    /// Sets the text alignment.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.inner.alignment = alignment;
        self.inner.needs_relayout = true;
        self.shared.text_changed = true;
    }

    /// Sets the scale factor for the text.
    pub fn set_scale(&mut self, scale: f32) {
        self.inner.scale = scale;
        self.inner.needs_relayout = true;
        self.shared.text_changed = true;
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

    /// Returns the layout, refreshing it if needed.
    pub fn layout(&mut self) -> &Layout<ColorBrush> {
        self.refresh_layout();
        &self.inner.layout
    }

    pub(crate) fn refresh_layout(&mut self) {
        if self.inner.needs_relayout || self.style_version_changed() {
            if self.style_version_changed() {
                self.inner.style_version = self.style_version();
            }
            self.rebuild_layout(None, false);
        }
    }

    /// Sets whether the text is selectable.
    pub fn set_selectable(&mut self, selectable: bool) {
        self.inner.selectable = selectable;
    }
    
    #[cfg(feature = "accessibility")]
    /// Sets the text selection based on an accesskit selection.
    pub fn select_from_accesskit(&mut self, selection: &accesskit::TextSelection) {
        self.refresh_layout();
        if let Some(selection) = Selection::from_access_selection(
            selection,
            &self.inner.layout,
            &self.inner.layout_access,
        ) {
            self.set_selection(selection);
        }
    }

    /// Sets focus to this text box.
    pub fn set_focus(&mut self) {
        self.shared.focused = Some(AnyBox::TextBox(self.key));
    }

    /// Render this text box to a `vello_hybrid` `Scene`.
    #[cfg(feature = "vello_hybrid")]
    pub fn render_to_scene(&mut self, scene: &mut vello_hybrid::Scene) {
        use crate::AnyBox;
        use parley::PositionedLayoutItem;
        use peniko::color::AlphaColor;
        use vello_common::{kurbo::{Rect, Shape}, paint::PaintType};

        self.refresh_layout();

        let (left, top) = self.position();
        let (left, top) = (left as f32, top as f32);

        // Account for scroll offset
        let content_left = left - self.scroll_offset().0;
        let content_top = top - self.scroll_offset().1;

        // Set up clipping if a clip rect is defined
        let clip_rect = self.effective_clip_rect();
        if let Some(clip) = clip_rect {
            let clip_x0 = content_left + clip.x0 as f32;
            let clip_y0 = content_top + clip.y0 as f32;
            let clip_x1 = content_left + clip.x1 as f32;
            let clip_y1 = content_top + clip.y1 as f32;
            let clip_rect = Rect::new(
                clip_x0 as f64,
                clip_y0 as f64,
                clip_x1 as f64,
                clip_y1 as f64,
            );
            scene.push_clip_layer(&clip_rect.to_path(0.1));
        }

        // Check if this text box is focused
        let is_focused = match self.shared.focused {
            Some(AnyBox::TextBox(f)) => f == self.key,
            _ => false,
        };

        if is_focused {
            // Render selection rectangles
            let selection_color = AlphaColor::from_rgba8(0x33, 0x33, 0xff, 0xaa);
            self.inner.selection.selection.geometry_with(&self.inner.layout, |rect, _line_i| {
                let x = content_left + rect.x0 as f32;
                let y = content_top + rect.y0 as f32;
                let width = (rect.x1 - rect.x0) as f32;
                let height = (rect.y1 - rect.y0) as f32;
                let rect = Rect::new(x as f64, y as f64, (x + width) as f64, (y + height) as f64);
                scene.set_paint(PaintType::Solid(selection_color));
                scene.fill_rect(&rect);
            });
        }

        // Render text
        for line in self.inner.layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                    render_glyph_run_to_scene_textbox(scene, &glyph_run, content_left, content_top);
                }
            }
        }

        // Pop the clip layer if we pushed one
        if clip_rect.is_some() {
            scene.pop_layer();
        }
    }
}


pub use parley::BoundingBox;

pub(crate) trait Ext1 {
    fn hit_bounding_box(&mut self, cursor_pos: (f64, f64)) -> bool;
}
impl<'a> Ext1 for TextBox<'a> {
    fn hit_bounding_box(&mut self, cursor_pos: (f64, f64)) -> bool {
        let offset = (
            cursor_pos.0 as f64 - self.inner.left,
            cursor_pos.1 as f64 - self.inner.top,
        );

        assert!(!self.inner.needs_relayout);
        let hit = offset.0 > -X_TOLERANCE
            && offset.0 < self.inner.layout.full_width() as f64 + X_TOLERANCE
            && offset.1 > 0.0
            && offset.1 < self.inner.layout.height() as f64;

        return hit;
    }
}

impl SelectionState {

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

        assert!(!self.needs_relayout);
        let hit = offset.0 > -X_TOLERANCE
            && offset.0 < self.layout.full_width() as f64 + X_TOLERANCE
            && offset.1 > 0.0
            && offset.1 < self.layout.height() as f64;

        return hit;
    }
}

/// Helper function to render a glyph run to a vello_hybrid Scene for TextBox.
#[cfg(feature = "vello_hybrid")]
fn render_glyph_run_to_scene_textbox(
    ctx: &mut vello_hybrid::Scene,
    glyph_run: &parley::GlyphRun<'_, ColorBrush>,
    left: f32,
    top: f32,
) {
    use peniko::color::AlphaColor;
    use vello_common::{glyph::Glyph, paint::PaintType};

    let mut run_x = glyph_run.offset();
    let run_y = glyph_run.baseline();
    let glyphs = glyph_run.glyphs().map(|glyph| {
        let glyph_x = run_x + glyph.x + left;
        let glyph_y = run_y - glyph.y + top;
        run_x += glyph.advance;

        Glyph {
            id: glyph.id as u32,
            x: glyph_x,
            y: glyph_y,
        }
    });

    let run = glyph_run.run();
    let font = run.font();
    let font_size = run.font_size();
    let normalized_coords = bytemuck::cast_slice(run.normalized_coords());

    let style = glyph_run.style();
    let r = style.brush.0[0];
    let g = style.brush.0[1];
    let b = style.brush.0[2];
    let a = style.brush.0[3];

    ctx.set_paint(PaintType::Solid(AlphaColor::from_rgba8(r, g, b, a)));
    ctx.glyph_run(font)
        .font_size(font_size)
        .normalized_coords(normalized_coords)
        .hint(true)
        .fill_glyphs(glyphs);
}

#[cfg(feature = "accessibility")]
fn push_accesskit_update_text_box_partial_borrows(
    accesskit_id: Option<accesskit::NodeId>,
    mut node: accesskit::Node,
    inner: &mut TextBoxInner,
    tree_update: &mut accesskit::TreeUpdate,
    left: f64,
    top: f64,
    node_id_generator: fn() -> accesskit::NodeId,
) {
    if let Some(id) = accesskit_id {
        inner.layout_access.build_nodes(
            &inner.text,
            &inner.layout,
            tree_update,
            &mut node,
            node_id_generator,
            left,
            top,
        );

        if let Some(ak_sel) = inner.selection.selection.to_access_selection(&inner.layout, &inner.layout_access) {
            node.set_text_selection(ak_sel);
        }

        tree_update.nodes.push((id, node))
    }
}


