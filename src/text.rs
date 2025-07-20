use crate::*;
use slab::Slab;
use std::time::Instant;
use winit::{event::{Modifiers, MouseButton, WindowEvent}, window::Window};

const MULTICLICK_DELAY: f64 = 0.4;
const MULTICLICK_TOLERANCE_SQUARED: f64 = 26.0;


#[derive(Debug)]
pub(crate) struct StyleInner {
    pub(crate) text_style: TextStyle2,
    pub(crate) text_edit_style: TextEditStyle,
    pub(crate) version: u64,
}

/// Centralized struct that holds collections of [`TextBox`]es, [`TextEdit`]s, [`TextStyle2`]s.
/// 
/// For rendering, a [`TextRenderer`] is also needed.
pub struct Text {
    pub(crate) text_boxes: Slab<TextBoxInner>,
    pub(crate) text_edits: Slab<(TextEditInner, TextBoxInner)>,

    pub(crate) styles: Slab<StyleInner>,
    pub(crate) style_version_id_counter: u64,

    pub(crate) input_state: TextInputState,

    pub(crate) focused: Option<AnyBox>,
    pub(crate) mouse_hit_stack: Vec<(AnyBox, f32)>,
    
    pub(crate) text_changed: bool,
    pub(crate) using_frame_based_visibility: bool,
    pub(crate) decorations_changed: bool,

    pub(crate) current_frame: u64,
}

/// Handle for a text edit box.
/// 
/// Obtained when creating a text edit box with [`Text::add_text_edit()`].
/// 
/// Use with [`Text::get_text_edit()`] to get a reference to the corresponding [`TextEdit`]. 
#[derive(Debug)]
pub struct TextEditHandle {
    pub(crate) i: u32,
}

/// Handle for a text box.
/// 
/// Obtained when creating a text box with [`Text::add_text_box()`].
/// 
/// Use with [`Text::get_text_box()`] to get a reference to the corresponding [`TextBox`]
#[derive(Debug)]
pub struct TextBoxHandle {
    pub(crate) i: u32,
}


#[cfg(feature = "panic_on_handle_drop")]
impl Drop for TextEditHandle {
    fn drop(&mut self) {
        panic!(
            "TextEditHandle was dropped without being consumed! \
            This means that the corresponding text edit wasn't removed. To avoid leaking it, you should call Text::remove_text_edit(handle). \
            If you're intentionally leaking this text edit, you can use \
            std::mem::forget(handle) to skip the handle's drop() call and avoid this panic. \
            You can also disable this check by disabling the \"panic_on_handle_drop\" feature in Cargo.toml."
        );
    }
}

#[cfg(feature = "panic_on_handle_drop")]
impl Drop for TextBoxHandle {
    fn drop(&mut self) {
        panic!(
            "TextBoxHandle was dropped without being consumed! \
            This means that the corresponding text box wasn't removed. To avoid leaking it, you should call Text::remove_text_box(handle). \
            If you're intentionally leaking this text box, you can use \
            std::mem::forget(handle) to skip the handle's drop() call and avoid this panic. \
            You can also disable this check by disabling the \"panic_on_handle_drop\" feature in Cargo.toml."
        );
    }
}


/// Handle for a text style. Use with Text methods to apply styles to text.
#[derive(Debug, Clone, Copy)]
pub struct StyleHandle {
    pub(crate) i: u32,
}
impl StyleHandle {
    #[allow(dead_code)]
    pub(crate) fn sneak_clone(&self) -> Self {
        Self { i: self.i }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LastClickInfo {
    pub(crate) time: Instant,
    pub(crate) pos: (f64, f64),
    pub(crate) focused: Option<AnyBox>,
}

#[derive(Debug, Clone)]
pub(crate) struct MouseState {
    pub pointer_down: bool,
    pub cursor_pos: (f64, f64),
    pub last_click_info: Option<LastClickInfo>,
    pub click_count: u32,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            pointer_down: false,
            cursor_pos: (0.0, 0.0),
            last_click_info: None,
            click_count: 0,
        }
    }
}

/// Enum that can represent any type of text box (text box or text edit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyBox {
    TextEdit(u32),
    TextBox(u32),
}

#[derive(Debug, Clone)]
pub(crate) struct TextInputState {
    pub(crate) mouse: MouseState,
    pub(crate) modifiers: Modifiers,
}

impl TextInputState {
    pub fn new() -> Self {
        Self {
            mouse: MouseState::new(),
            modifiers: Modifiers::default(),
        }
    }

    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_pos = (position.x, position.y);
                self.mouse.cursor_pos = cursor_pos;
            },

            WindowEvent::MouseInput { state, .. } => {
                self.mouse.pointer_down = state.is_pressed();
            },
            _ => {}
        }
    }
}

pub(crate) const DEFAULT_STYLE_I: usize = 0;
/// Pre-defined handle for the default text style.
pub const DEFAULT_STYLE_HANDLE: StyleHandle = StyleHandle { i: DEFAULT_STYLE_I as u32 };

impl Text {
    pub fn new() -> Self {
        let mut styles = Slab::with_capacity(10);
        let i = styles.insert(StyleInner {
            text_style: original_default_style(),
            text_edit_style: TextEditStyle::default(),
            version: 0,
        });
        debug_assert!(i == DEFAULT_STYLE_I);

        Self {
            text_boxes: Slab::with_capacity(10),
            text_edits: Slab::with_capacity(10),
            styles,
            style_version_id_counter: 0,
            input_state: TextInputState::new(),
            focused: None,
            mouse_hit_stack: Vec::with_capacity(6),
            text_changed: true,
            decorations_changed: true,
            current_frame: 1,
            using_frame_based_visibility: false,
        }
    }

    pub(crate) fn new_style_version(&mut self) -> u64 {
        self.style_version_id_counter += 1;
        self.style_version_id_counter
    }

    /// Add a text box and return a handle.
    /// 
    /// The handle can be used with [`Text::get_text_box()`] to get a reference to the [`TextBox`] that was added.
    /// 
    /// The [`TextBox`] must be manually removed by calling [`Text::remove_text_box()`].
    /// 
    /// `text` can be a `String`, a `&'static str`, or a `Cow<'static, str>`.
    #[must_use]
    pub fn add_text_box(&mut self, text: impl Into<Cow<'static, str>>, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextBoxHandle {
        let mut text_box = TextBoxInner::new(text, pos, size, depth);
        text_box.last_frame_touched = self.current_frame;
        let i = self.text_boxes.insert(text_box) as u32;
        self.text_changed = true;
        TextBoxHandle { i }
    }

    /// Add a text edit and return a handle.
    /// 
    /// The handle can be used with [`Text::get_text_edit()`] to get a reference to the [`TextEdit`] that was added.
    /// 
    /// The [`TextEdit`] must be manually removed by calling [`Text::remove_text_edit()`].
    #[must_use]
    pub fn add_text_edit(&mut self, text: String, pos: (f64, f64), size: (f32, f32), depth: f32) -> TextEditHandle {
        let (text_edit, mut text_box) = TextEditInner::new(text, pos, size, depth);
        text_box.last_frame_touched = self.current_frame;
        let i = self.text_edits.insert((text_edit, text_box)) as u32;
        self.text_changed = true;
        TextEditHandle { i }
    }




    /// Get a mutable reference to a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    ///    
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_edit_mut(&mut self, handle: &TextEditHandle) -> TextEdit {
        self.text_changed = true;
        self.get_full_text_edit(handle)
    }

    /// Get a reference to a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    ///    
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_edit(&mut self, handle: &TextEditHandle) -> TextEdit {
        self.get_full_text_edit(handle)
    }

    #[must_use]
    pub fn add_style(&mut self, text_style: TextStyle2, text_edit_style: Option<TextEditStyle>) -> StyleHandle {
        let text_edit_style = text_edit_style.unwrap_or_default();
        let new_version = self.new_style_version();
        let i = self.styles.insert(StyleInner {
            text_style,
            text_edit_style,
            version: new_version,
        }) as u32;
        StyleHandle { i }
    }

    pub fn get_text_style(&self, handle: &StyleHandle) -> &TextStyle2 {
        &self.styles[handle.i as usize].text_style
    }

    pub fn get_text_style_mut(&mut self, handle: &StyleHandle) -> &mut TextStyle2 {
        self.styles[handle.i as usize].version = self.new_style_version();
        self.text_changed = true;
        &mut self.styles[handle.i as usize].text_style
    }

    pub fn get_text_edit_style(&self, handle: &StyleHandle) -> &TextEditStyle {
        &self.styles[handle.i as usize].text_edit_style
    }

    pub fn get_text_edit_style_mut(&mut self, handle: &StyleHandle) -> &mut TextEditStyle {
        self.styles[handle.i as usize].version = self.new_style_version();
        self.text_changed = true;
        &mut self.styles[handle.i as usize].text_edit_style
    }

    pub fn get_default_text_style(&self) -> &TextStyle2 {
        self.get_text_style(&DEFAULT_STYLE_HANDLE)
    }

    pub fn get_default_text_style_mut(&mut self) -> &mut TextStyle2 {
        self.get_text_style_mut(&DEFAULT_STYLE_HANDLE)
    }

    pub fn get_default_text_edit_style(&self) -> &TextEditStyle {
        self.get_text_edit_style(&DEFAULT_STYLE_HANDLE)
    }

    pub fn get_default_text_edit_style_mut(&mut self) -> &mut TextEditStyle {
        self.get_text_edit_style_mut(&DEFAULT_STYLE_HANDLE)
    }

    pub fn original_default_style(&self) -> TextStyle2 {
        original_default_style()
    }

    /// Advance an internal global frame counter that causes all text boxes to be implicitly marked as outdated and hidden.
    /// 
    /// You can then use [`Text::refresh_text_box()`] to "refresh" only the text boxes that should stay visible.
    /// 
    /// This allows to control the visibility of text boxes in a more "declarative" way.
    /// 
    /// Additionally, you can also use [`TextBox::set_can_hide()`] to decide if boxes should stay hidden in the background, or if they should marked as "to delete". You can the call [`Text::remove_old_nodes()`] to remove all the outdated text boxes that were marked as "to delete". 
    pub fn advance_frame_and_hide_boxes(&mut self) {
        self.current_frame += 1;
        self.using_frame_based_visibility = true;
    }

    /// Refresh a text box, causing it to stay visible even if [`Text::advance_frame_and_hide_boxes()`] was called.
    /// 
    /// Part of the "declarative" interface.  
    pub fn refresh_text_box(&mut self, handle: &TextBoxHandle) {
        if let Some(text_box) = self.text_boxes.get_mut(handle.i as usize) {
            text_box.last_frame_touched = self.current_frame;
        }
    }


    /// Refresh a text edit box, causing it to stay visible even if [`Text::advance_frame_and_hide_boxes()`] was called.
    /// 
    /// Part of the "declarative" interface.
    pub fn refresh_text_edit(&mut self, handle: &TextEditHandle) {
        if let Some((_text_edit, text_box)) = self.text_edits.get_mut(handle.i as usize) {
            text_box.last_frame_touched = self.current_frame;
        }
    }


    /// Remove all text boxes that were made outdated by [`Text::advance_frame_and_hide_boxes()`], were not refreshed with [`Text::refresh_text_box()`], and were not set to remain as hidden with [`TextBox::set_can_hide()`].
    /// 
    /// Because [`Text::remove_old_nodes()`] mass-removes text boxes without consuming their handles, the handles become "dangling" and should not be reused. Using them in functions like [`Text::get_text_box()`] or [`Text::remove_text_box()`] will cause panics or incorrect results.
    /// 
    /// Only use this function if the structs holding the handles are managed in a way where you can be confident that the handles won't be kept around and reused.
    /// 
    /// On the other hand, it's fine to use the declarative system for *hiding* text boxes, but sticking to imperative [`Text::remove_text_box()`] calls to remove them.
    /// 
    /// [`Text::remove_old_nodes()`] is the only function that breaks the "no dangling handles" promise. If you use imperative [`Text::remove_text_box()`] calls and avoid `remove_old_nodes()`, then there is no way for the handle system to break.
    /// 

    pub fn remove_old_nodes(&mut self) {
        // Clear focus if the focused text box will be removed
        if let Some(focused) = self.focused {
            let should_clear_focus = match focused {
                AnyBox::TextBox(i) => {
                    if let Some(text_box) = self.text_boxes.get(i as usize) {
                        text_box.last_frame_touched != self.current_frame && !text_box.can_hide
                    } else {
                        true // Text box doesn't exist
                    }
                }
                AnyBox::TextEdit(i) => {
                    if let Some((_text_edit, text_box)) = self.text_edits.get(i as usize) {
                        text_box.last_frame_touched != self.current_frame && !text_box.can_hide
                    } else {
                        true // Text edit doesn't exist
                    }
                }
            };
            
            if should_clear_focus {
                self.focused = None;
            }
        }

        // Remove text boxes that are outdated and allowed to be removed
        self.text_boxes.retain(|_, text_box| {
            text_box.last_frame_touched == self.current_frame || text_box.can_hide
        });


        self.text_edits.retain(|_, (_text_edit, text_box)| {
            text_box.last_frame_touched == self.current_frame || text_box.can_hide
        });
    }

    /// Remove a text box.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    pub fn remove_text_box(&mut self, handle: TextBoxHandle) {
        self.text_changed = true;
        if let Some(AnyBox::TextBox(i)) = self.focused {
            if i == handle.i {
                self.focused = None;
            }
        }
        self.text_boxes.remove(handle.i as usize);
        std::mem::forget(handle);
    }


    /// Remove a text edit.
    /// 
    /// `handle` is the handle that was returned when first creating the text edit with [`Text::add_text_edit()`] or similar functions.
    pub fn remove_text_edit(&mut self, handle: TextEditHandle) {
        self.text_changed = true;
        if let Some(AnyBox::TextEdit(i)) = self.focused {
            if i == handle.i {
                self.focused = None;
            }
        }
        self.text_edits.remove(handle.i as usize);
        std::mem::forget(handle);
    }

    /// Remove a text style.
    /// 
    /// If any text boxes are set to this style, they will revert to the default style.
    pub fn remove_style(&mut self, handle: StyleHandle) {
        self.styles.remove(handle.i as usize);
    }

    pub fn prepare_all(&mut self, text_renderer: &mut TextRenderer) {

        if ! self.text_changed && self.using_frame_based_visibility {
            // see if any text boxes were just hidden
            for (_i, (_text_edit, text_box)) in self.text_edits.iter_mut() {
                if text_box.last_frame_touched == self.current_frame - 1 {
                    self.text_changed = true;
                }
            }
            for (_i, text_box) in self.text_boxes.iter_mut() {
                if text_box.last_frame_touched == self.current_frame - 1 {
                    self.text_changed = true;
                }

            }
        }

        if self.text_changed {
            text_renderer.clear();
        } else if self.decorations_changed {
            text_renderer.clear_decorations_only();
        }

        let current_frame = self.current_frame;
        if self.text_changed {
            // todo: figure out some other way to cheat partial borrowing

            for i in 0..self.text_edits.capacity() {
                if self.text_edits.contains(i) {
                    let handle = TextEditHandle { i: i as u32 };
                    let mut text_edit = self.get_full_text_edit(&handle);
                    if !text_edit.hidden() && text_edit.text_box.inner.last_frame_touched == current_frame {
                        text_renderer.prepare_text_edit_layout(&mut text_edit);
                    }
                }
            }

            for i in 0..self.text_boxes.capacity() {
                if self.text_boxes.contains(i) {
                    let handle = TextBoxHandle { i: i as u32 };
                    let mut text_edit = self.get_full_text_box(&handle);
                    if !text_edit.hidden() && text_edit.inner.last_frame_touched == current_frame {
                        text_renderer.prepare_text_box_layout(&mut text_edit);
                    }
                }
            }
        }

        if self.decorations_changed || self.text_changed {
            if let Some(focused) = self.focused {
                match focused {
                    AnyBox::TextEdit(i) => {
                        let handle = TextEditHandle { i: i as u32 };
                        let text_edit = self.get_full_text_edit(&handle);
                        text_renderer.prepare_text_box_decorations(&text_edit.text_box, true);
                    },
                    AnyBox::TextBox(i) => {
                        let handle = TextBoxHandle { i: i as u32 };
                        let text_box = self.get_full_text_box(&handle);
                        text_renderer.prepare_text_box_decorations(&text_box, true);
                    },
                }
            }
        }

        self.text_changed = false;
        self.decorations_changed = false;

        self.using_frame_based_visibility = false;
    }

    /// Handle window events for text widgets.
    /// 
    /// This is the simple interface that works when text widgets aren't occluded by other objects.
    /// For complex z-ordering, use [`Text::find_topmost_text_box()`] and [`Text::handle_event_with_topmost()`], as described in the crate-level docs and shown in the `occlusion.rs` example.
    /// 
    /// Any events other than `winit::WindowEvent::MouseInput` can use either this method or the occlusion method interchangeably.
    pub fn handle_event(&mut self, event: &WindowEvent, window: &Window) {
        self.input_state.handle_event(event);

        if let WindowEvent::Resized(_) = event {
            self.text_changed = true;
        }

        if let WindowEvent::MouseInput { state, button, .. } = event {
            if state.is_pressed() && *button == MouseButton::Left {
                let new_focus = self.find_topmost_at_pos(self.input_state.mouse.cursor_pos);
                self.refocus(new_focus);
                self.handle_click_counting();
            }
        }

        if let Some(focused) = self.focused {
            // todo remove
            self.handle_focused_event(focused, event, window);
        }
    }

    /// Find the topmost text box that would receive mouse events, if it wasn't occluded by any non-text-box objects.
    /// 
    /// Returns the handle of the topmost text widget at the event position, or None if no widget is hit.
    /// Use this with [`Text::handle_event_with_topmost()`] for complex z-ordering scenarios.
    pub fn find_topmost_text_box(&mut self, event: &WindowEvent) -> Option<AnyBox> {
        // Only handle mouse events that have a position
        let cursor_pos = match event {
            WindowEvent::MouseInput { .. } => self.input_state.mouse.cursor_pos,
            WindowEvent::CursorMoved { position, .. } => (position.x, position.y),
            _ => return None,
        };

        self.find_topmost_at_pos(cursor_pos)
    }

    /// Get the depth of a text box by its handle.
    /// 
    /// Used for comparing depths when integrating with other objects that might occlude text boxs.
    pub fn get_text_box_depth(&self, text_box_id: &AnyBox) -> f32 {
        match text_box_id {
            AnyBox::TextEdit(i) => self.text_edits.get(*i as usize).map(|(_te, tb)| tb.depth).unwrap_or(f32::MAX),
            AnyBox::TextBox(i) => self.text_boxes.get(*i as usize).map(|tb| tb.depth).unwrap_or(f32::MAX),
        }
    }

    /// Handle window events with a pre-determined topmost text box.
    /// 
    /// Use this for complex z-ordering scenarios where text boxs might be occluded by other objects.
    /// Pass `Some(text_box_id)` if a text box should receive the event, or `None` if it's occluded.
    /// 
    /// If the text box is occluded, this function should still be called with `None`, so that text boxes can defocus.
    pub fn handle_event_with_topmost(&mut self, event: &WindowEvent, window: &Window, topmost_text_box: Option<AnyBox>) {
        self.input_state.handle_event(event);

        if let WindowEvent::MouseInput { state, button, .. } = event {
            if state.is_pressed() && *button == MouseButton::Left {
                self.refocus(topmost_text_box);
                self.handle_click_counting();
            }
        }

        if let Some(focused) = self.focused {
            self.handle_focused_event(focused, event, window);
        }
    }

    fn find_topmost_at_pos(&mut self, cursor_pos: (f64, f64)) -> Option<AnyBox> {
        self.mouse_hit_stack.clear();

        // Find all text widgets at this position
        for (i, (_text_edit, text_box)) in self.text_edits.iter_mut() {
            if !text_box.hidden && text_box.last_frame_touched == self.current_frame && text_box.hit_full_rect(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::TextEdit(i as u32), text_box.depth));
            }
        }
        for (i, text_box) in self.text_boxes.iter_mut() {
            if !text_box.hidden && text_box.last_frame_touched == self.current_frame && text_box.hit_bounding_box(cursor_pos) {
                self.mouse_hit_stack.push((AnyBox::TextBox(i as u32), text_box.depth));
            }
        }

        // Find the topmost (lowest depth value)
        let mut topmost = None;
        let mut top_z = f32::MAX;
        for (id, z) in self.mouse_hit_stack.iter() {
            if *z < top_z {
                top_z = *z;
                topmost = Some(*id);
            }
        }

        topmost
    }

    fn refocus(&mut self, new_focus: Option<AnyBox>) {
        if new_focus != self.focused {
            if let Some(old_focus) = self.focused {
                self.remove_focus(old_focus);
            }
        }
        self.focused = new_focus;
        // todo: could skip some rerenders here if the old focus wasn't editable and had collapsed selection.
        self.decorations_changed = true;
    }

    fn handle_click_counting(&mut self) {
        let now = Instant::now();
        let current_pos = self.input_state.mouse.cursor_pos;
        
        if let Some(last_info) = self.input_state.mouse.last_click_info.take() {
            if now.duration_since(last_info.time).as_secs_f64() < MULTICLICK_DELAY 
                && last_info.focused == self.focused {
                let dx = current_pos.0 - last_info.pos.0;
                let dy = current_pos.1 - last_info.pos.1;
                let distance_squared = dx * dx + dy * dy;
                if distance_squared <= MULTICLICK_TOLERANCE_SQUARED {
                    self.input_state.mouse.click_count = (self.input_state.mouse.click_count + 1) % 4;
                } else {
                    self.input_state.mouse.click_count = 1;
                }
            } else {
                self.input_state.mouse.click_count = 1;
            }
        } else {
            self.input_state.mouse.click_count = 1;
        }
        
        self.input_state.mouse.last_click_info = Some(LastClickInfo {
            time: now,
            pos: current_pos,
            focused: self.focused,
        });
    }
    
    fn remove_focus(&mut self, old_focus: AnyBox) {
        match old_focus {
            AnyBox::TextEdit(i) => {
                let handle = TextEditHandle { i: i as u32 };
                let mut text_edit = self.get_full_text_edit(&handle);
                text_edit.text_box.reset_selection();
                text_edit.inner.show_cursor = false;
            },
            AnyBox::TextBox(i) => {
                let handle = TextBoxHandle { i: i as u32 };
                let mut text_box = self.get_full_text_box(&handle);
                text_box.reset_selection();
            },
        }
    }
    
    fn handle_focused_event(&mut self, focused: AnyBox, event: &WindowEvent, window: &Window) {
        match focused {
            AnyBox::TextEdit(i) => {
                let handle = TextEditHandle { i: i as u32 };
                let mut text_edit = get_full_text_edit_free(&mut self.text_edits, &mut self.styles, &handle);

                let result = text_edit.handle_event(event, window, &self.input_state);
                if result.text_changed {
                    self.text_changed = true;
                }
                if result.decorations_changed {
                    self.decorations_changed = true;
                }
            },
            AnyBox::TextBox(i) => {
                let handle = TextBoxHandle { i: i as u32 };
                let mut text_box = get_full_text_box_free(&mut self.text_boxes, &mut self.styles, &handle);

                let result = text_box.handle_event(event, window, &self.input_state);
                if result.text_changed {
                    self.text_changed = true;
                }
                if result.decorations_changed {
                    self.decorations_changed = true;
                }
            },
        }
    }

    /// Set the disabled state of a text edit box.
    /// 
    /// When disabled, the text edit will not respond to events and will be rendered with greyed out text.
    pub fn set_text_edit_disabled(&mut self, handle: &TextEditHandle, disabled: bool) {
        let text_edit_inner = &mut self.text_edits[handle.i as usize].0;
        text_edit_inner.disabled = disabled;
        if disabled {
            if let Some(AnyBox::TextEdit(e)) = self.focused {
                if e == handle.i {
                    self.get_full_text_edit(&handle).text_box.reset_selection();
                    self.focused = None;
                }
            }
        }

    }

    /// Returns whether any text was changed in the last frame.
    pub fn get_text_changed(&self) -> bool {
        self.text_changed
    }

    /// Get a mutable reference to a text box wrapped with its style.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    /// 
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_box_mut(&mut self, handle: &TextBoxHandle) -> TextBox {
        let text_box_inner = &mut self.text_boxes[handle.i as usize];
        TextBox { inner: text_box_inner, styles: &self.styles }
    }

    /// Get a mutable reference to a text box wrapped with its style.
    /// 
    /// `handle` is the handle that was returned when first creating the text box with [`Text::add_text_box()`].
    /// 
    /// This is a fast lookup operation that does not require any hashing.
    pub fn get_text_box(&mut self, handle: &TextBoxHandle) -> TextBox {
        let text_box_inner = &mut self.text_boxes[handle.i as usize];
        TextBox { inner: text_box_inner, styles: &self.styles }
    }

    pub(crate) fn get_full_text_box(&mut self, i: &TextBoxHandle) -> TextBox<'_> {
        get_full_text_box_free(&mut self.text_boxes, &mut self.styles, i)
    }

    pub(crate) fn get_full_text_edit(&mut self, i: &TextEditHandle) -> TextEdit<'_> {
        get_full_text_edit_free(&mut self.text_edits, &mut self.styles, i)
    }
}

// I LOVE PARTIAL BORROWS!
pub(crate) fn get_full_text_box_free<'a>(
    text_boxes: &'a mut Slab<TextBoxInner>,
    styles: &'a Slab<StyleInner>,
    i: &TextBoxHandle,
) -> TextBox<'a> {
    let text_box_inner = &mut text_boxes[i.i as usize];
    TextBox { inner: text_box_inner, styles }
}

// I LOVE PARTIAL BORROWS!
pub(crate) fn get_full_text_edit_free<'a>(
    text_edits: &'a mut Slab<(TextEditInner, TextBoxInner)>,
    styles: &'a Slab<StyleInner>,
    i: &TextEditHandle,
) -> TextEdit<'a> {
    let (text_edit_inner, text_box_inner) = text_edits.get_mut(i.i as usize).unwrap();
    let text_box = TextBox { inner: text_box_inner, styles };
    TextEdit { inner: text_edit_inner, styles, text_box }
}
