use std::{
    fmt::Display, ops::Range, time::{Duration, Instant}
};

use parley::*;
use winit::{
    event::{Ime, Touch, WindowEvent}, keyboard::{Key, NamedKey}, platform::modifier_supplement::KeyEventExtModifierSupplement, window::Window
};

const INSET: f32 = 2.0;

use crate::*;

// I love partial borrows!
macro_rules! clear_placeholder {
    ($self:expr) => {
        if $self.inner.showing_placeholder {
            $self.text_box.text_mut().clear();
            $self.inner.showing_placeholder = false;
            $self.text_box.inner.needs_relayout = true;
            $self.text_box.inner.selection.selection = Selection::zero();
        }
    };
}

/// Defines how newlines are entered in a text edit box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewlineMode {
    /// Enter key inserts newlines (default for multi-line)
    Enter,
    /// Shift+Enter inserts newlines, Enter is ignored
    ShiftEnter,
    /// Ctrl+Enter inserts newlines, Enter is ignored (or Cmd+Enter on macOS)
    CtrlEnter,
    /// No newlines allowed (used automatically for single-line mode)
    None,
}

impl Default for NewlineMode {
    fn default() -> Self {
        NewlineMode::Enter
    }
}

/// Result of handling a window event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextEventResult {
    /// Whether the text content changed
    pub text_changed: bool,
    /// Whether visual decorations (selection, cursor position, etc.) changed
    pub decorations_changed: bool,
}

impl TextEventResult {
    pub(crate) fn nothing() -> Self {
        Self {
            text_changed: false,
            decorations_changed: false,
        }
    }
}

/// A string that may be split into two parts (used for IME composition).
#[derive(Debug, Clone, Copy)]
pub struct SplitString<'source>(pub(crate) [&'source str; 2]);

impl<'source> SplitString<'source> {
    /// Get the characters of this string.
    pub fn chars(self) -> impl Iterator<Item = char> + 'source {
        self.into_iter().flat_map(str::chars)
    }
}

impl PartialEq<&'_ str> for SplitString<'_> {
    fn eq(&self, other: &&'_ str) -> bool {
        let [a, b] = self.0;
        let mid = a.len();
        // When our MSRV is 1.80 or above, use split_at_checked instead.
        // is_char_boundary checks bounds
        let (a_1, b_1) = if other.is_char_boundary(mid) {
            other.split_at(mid)
        } else {
            return false;
        };

        a_1 == a && b_1 == b
    }
}

impl Display for SplitString<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b] = self.0;
        write!(f, "{a}{b}")
    }
}

/// Iterate through the source strings.
impl<'source> IntoIterator for SplitString<'source> {
    type Item = &'source str;
    type IntoIter = <[&'source str; 2] as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

pub(crate) fn selection_decorations_changed(initial_selection: Selection, new_selection: Selection, initial_show_cursor: bool, new_show_cursor: bool, is_editable: bool) -> bool {
    if initial_show_cursor != new_show_cursor {
        return true;
    }
    
    // For non-editable boxes, if both selections are collapsed, no decoration change
    if !is_editable && initial_selection.is_collapsed() && new_selection.is_collapsed() {
        return false;
    }
    
    // Compare selections ignoring affinity-only changes
    let initial_range = initial_selection.text_range();
    let new_range = new_selection.text_range();
    
    initial_range != new_range
}

/// A text edit box.
/// 
/// This struct can't be created directly. Instead, use [`Text::add_text_edit()`] or similar functions to create one within [`Text`] and get a [`TextEditHandle`] back.
/// 
/// Then, the handle can be used to get a reference to the `TextEdit` with [`Text::get_text_edit()`] or [`Text::get_text_edit_mut()`].
pub(crate) struct TextEditInner {
    pub(crate) compose: Option<Range<usize>>,
    pub(crate) show_cursor: bool,
    pub(crate) start_time: Option<Instant>,
    pub(crate) blink_period: Duration,
    pub(crate) history: TextEditHistory,
    pub(crate) single_line: bool,
    pub(crate) newline_mode: NewlineMode,
    pub(crate) disabled: bool,
    pub(crate) showing_placeholder: bool,
    pub(crate) placeholder_text: Option<Cow<'static, str>>,
}

impl TextEditInner {
    pub fn new(text: String, pos: (f64, f64), size: (f32, f32), depth: f32) -> (Self, TextBoxInner) {
        let mut text_box = TextBoxInner::new(text, pos, size, depth);
        text_box.auto_clip = true;
        let text_edit = Self {
            compose: Default::default(),
            show_cursor: true,
            start_time: Default::default(),
            blink_period: Default::default(),
            history: TextEditHistory::new(),
            single_line: false,
            newline_mode: NewlineMode::default(),
            disabled: false,
            showing_placeholder: false,
            placeholder_text: None,
        };
        (text_edit, text_box)
    }
}


impl<'a> TextEdit<'a> {


    #[must_use]
    // todo remove this function
    pub(crate) fn handle_event(&mut self, event: &WindowEvent, window: &Window, input_state: &TextInputState) -> TextEventResult {
        if !self.inner.disabled {
            self.handle_event_editable(event, window, input_state)
        } else {
            return TextEventResult::nothing();
        }
    }

    pub fn is_composing(&self) -> bool {
        self.inner.compose.is_some()
    }

    pub fn set_single_line(&mut self, single_line: bool) {
        if self.inner.single_line != single_line {
            self.inner.single_line = single_line;
            
            // If switching to single line mode, remove any existing newlines and set newline mode to None
            if single_line {
                self.inner.newline_mode = NewlineMode::None;
            } else {
                // When switching back to multi-line, restore default newline mode
                self.inner.newline_mode = NewlineMode::Enter;
            }
        }
    }

    pub fn is_single_line(&self) -> bool {
        self.inner.single_line
    }

    pub fn set_newline_mode(&mut self, mode: NewlineMode) {
        // Don't allow changing newline mode in single line mode (it's always None)
        if !self.inner.single_line {
            self.inner.newline_mode = mode;
        }
    }

    pub fn newline_mode(&self) -> NewlineMode {
        self.inner.newline_mode
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.inner.disabled = disabled;
    }

    pub fn disabled(&self) -> bool {
        self.inner.disabled
    }



    /// Check if placeholder text is currently being shown
    pub fn showing_placeholder(&self) -> bool {
        self.inner.showing_placeholder
    }





    // Cursor blinking methods
    pub fn cursor_reset(&mut self) {
        self.inner.start_time = Some(Instant::now());
        // TODO: for real world use, this should be reading from the system settings
        self.inner.blink_period = Duration::from_millis(500);
        self.inner.show_cursor = true;
    }

    pub fn disable_blink(&mut self) {
        self.inner.start_time = None;
    }

    pub fn cursor_blink(&mut self) {
        self.inner.show_cursor = self.inner.start_time.is_some_and(|start_time| {
            let elapsed = Instant::now().duration_since(start_time);
            (elapsed.as_millis() / self.inner.blink_period.as_millis()) % 2 == 0
        });
    }

    pub fn next_blink_time(&self) -> Option<Instant> {
        self.inner.start_time.map(|start_time| {
            let phase = Instant::now().duration_since(start_time);

            start_time
                + Duration::from_nanos(
                    ((phase.as_nanos() / self.inner.blink_period.as_nanos() + 1)
                        * self.inner.blink_period.as_nanos()) as u64,
                )
        })
    }

    #[must_use]
    pub(crate) fn handle_event_editable(&mut self, event: &WindowEvent, window: &Window, input_state: &TextInputState) -> TextEventResult {
        if self.text_box.hidden() {
            return TextEventResult::nothing();
        }
        
        // Capture initial state for comparison
        let initial_selection = self.text_box.selection();
        let initial_show_cursor = self.inner.show_cursor;
        
        let mut result = TextEventResult::nothing();
        let mut scroll_to_cursor = false;
        let mut manually_scrolled = false;

        let showing_placeholder = self.inner.showing_placeholder;
        if ! self.inner.showing_placeholder {
            self.text_box.handle_event_no_edit_inner(event, input_state, showing_placeholder);
        }

        match event {
            WindowEvent::KeyboardInput { event, .. } if !self.is_composing() => {
                if !event.state.is_pressed() {
                    return result;
                }
                #[allow(unused)]
                let mods_state = input_state.modifiers.state();
                let shift = mods_state.shift_key();
                let action_mod = if cfg!(target_os = "macos") {
                    mods_state.super_key()
                } else {
                    mods_state.control_key()
                };

                // edit action mods
                if action_mod {
                    match event.key_without_modifiers() {
                        Key::Character(c) => {
                            match c.as_str() {
                                "x" if !shift => {
                                    with_clipboard(|cb| {
                                        if let Some(text) = self.text_box.selected_text() {
                                            cb.set_text(text.to_owned()).ok();
                                            self.delete_selection();
                                            result.text_changed = true;
                                        }
                                    });
                                }
                                "v" if !shift => {
                                    with_clipboard(|cb| {
                                        let text = cb.get_text().unwrap_or_default();
                                        self.insert_or_replace_selection(&text);
                                        result.text_changed = true;
                                    });
                                }
                                "z" => {
                                    if shift {
                                        self.redo();
                                        result.text_changed = true;
                                    } else {
                                        self.undo();
                                        result.text_changed = true;
                                    }
                                }
                                _ => (),
                            }
                        }
                        _ => (),
                    };
                }

                match &event.logical_key {
                    Key::Named(NamedKey::ArrowLeft) => {
                        if !shift && ! self.inner.showing_placeholder {
                            scroll_to_cursor = true;
                            if action_mod {
                                self.text_box.move_word_left();
                            } else {
                                self.text_box.move_left();
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        if !shift && ! self.inner.showing_placeholder {
                            scroll_to_cursor = true;
                            if action_mod {
                                self.text_box.move_word_right();
                            } else {
                                self.text_box.move_right();
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        if !shift && ! self.inner.showing_placeholder {
                            if self.inner.single_line {
                                scroll_to_cursor = true;
                                self.text_box.move_to_text_start();
                            } else {
                                self.text_box.move_up();
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if !shift && ! self.inner.showing_placeholder {
                            scroll_to_cursor = true;
                            if self.inner.single_line {
                                self.text_box.move_to_text_end();
                            } else {
                                self.text_box.move_down();
                            }
                        }
                    }
                    Key::Named(NamedKey::Home) => {
                        if !shift && ! self.inner.showing_placeholder {
                            scroll_to_cursor = true;
                            if action_mod {
                                self.text_box.move_to_text_start();
                            } else {
                                self.text_box.move_to_line_start();
                            }
                        }
                    }
                    Key::Named(NamedKey::End) => {
                        if !shift && ! self.inner.showing_placeholder {
                            scroll_to_cursor = true;
                            if action_mod {
                                self.text_box.move_to_text_end();
                            } else {
                                self.text_box.move_to_line_end();
                            }
                        }
                    }
                    Key::Named(NamedKey::Delete) => {
                        if ! self.inner.showing_placeholder {
                            scroll_to_cursor = true;
                            if action_mod {
                                self.delete_word();
                            } else {
                                self.delete();
                            }
                            result.text_changed = true;
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        if ! self.inner.showing_placeholder {
                            scroll_to_cursor = true;
                            if action_mod {
                                self.backdelete_word();
                            } else {
                                self.backdelete();
                            }
                            result.text_changed = true;
                        }
                    }
                    Key::Named(NamedKey::Enter) => {
                        scroll_to_cursor = true;
                        let newline_mode_matches = match self.inner.newline_mode {
                            NewlineMode::Enter => !action_mod && !shift,
                            NewlineMode::ShiftEnter => shift && !action_mod,
                            NewlineMode::CtrlEnter => action_mod && !shift,
                            NewlineMode::None => false,
                        };
                        
                        if newline_mode_matches && ! self.inner.single_line {
                            self.insert_or_replace_selection("\n");
                            result.text_changed = true;
                        }
                    }
                    Key::Named(NamedKey::Space) => {
                        if ! action_mod {
                            self.insert_or_replace_selection(" ");
                            result.text_changed = true;
                        }
                    }
                    Key::Character(s) => {
                        if ! action_mod {
                            self.insert_or_replace_selection(&s);
                            result.text_changed = true;
                        }
                    }
                    _ => (),
                }
            }
            WindowEvent::Touch(Touch {
                phase, location, ..
            }) if !self.is_composing() => {
                // todo, this is all wrong (should probably scroll), but nobody cares
                use winit::event::TouchPhase::*;
                if ! self.inner.showing_placeholder {
                    match phase {
                        Started => {
                            let cursor_pos = (
                                location.x - self.text_box.inner.left as f64,
                                location.y - self.text_box.inner.top as f64,
                            );
                            self.text_box.move_to_point(cursor_pos.0 as f32, cursor_pos.1 as f32);
                        }
                        Cancelled => {
                            self.text_box.collapse_selection();
                        }
                        Moved => {
                            self.text_box.extend_selection_to_point(
                                location.x as f32 - INSET,
                                location.y as f32 - INSET,
                            );
                        }
                        Ended => (),
                    }
                } 
            }
            WindowEvent::Ime(Ime::Disabled) => {
                self.clear_compose();
                result.text_changed = true;
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if self.inner.showing_placeholder {
                    self.clear_placeholder()
                }
                scroll_to_cursor = true;
                self.insert_or_replace_selection(&text);
                result.text_changed = true;
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                scroll_to_cursor = true;
                if self.inner.showing_placeholder {
                    self.clear_placeholder()
                }
                if text.is_empty() {
                    self.clear_compose();
                    result.text_changed = true;
                } else {
                    self.set_compose(&text, *cursor);
                    result.text_changed = true;
                    self.set_ime_cursor_area(window);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.inner.single_line => {
                let cursor_pos = input_state.mouse.cursor_pos;
                if self.text_box.hit_full_rect(cursor_pos) {
                    let scroll_amount = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, _y) => x * 30.0,
                        winit::event::MouseScrollDelta::PixelDelta(pos) => pos.x as f32,
                    };
                    
                    if scroll_amount.abs() > 0.1 {
                        let old_scroll = self.text_box.inner.scroll_offset;
                        let new_scroll = old_scroll - scroll_amount;
                        
                        let total_text_width = self.text_box.inner.layout.full_width();
                        let text_width = self.text_box.inner.max_advance;
                        let max_scroll = (total_text_width - text_width).max(0.0);
                        let new_scroll = new_scroll.clamp(0.0, max_scroll);
                        
                        if (new_scroll - old_scroll).abs() > 0.1 {
                            self.text_box.inner.scroll_offset = new_scroll;
                            manually_scrolled = true;
                        }
                    }
                }
            }
            _ => {}
        }

        self.restore_placeholder_if_any();

        if selection_decorations_changed(initial_selection, self.text_box.selection(), initial_show_cursor, self.inner.show_cursor, !self.inner.disabled) {
            {
                let this = &mut result;
                this.decorations_changed = true;
            };
        }

        self.refresh_layout();

        if scroll_to_cursor || result.text_changed  {
            let did_scroll = self.update_scroll_to_cursor();
            if did_scroll {
                result.text_changed = true;
            }
        }

        if manually_scrolled {
            result.text_changed = true;
        }



        return result;
    }

    // #[cfg(feature = "accesskit")]
    // pub(crate) fn handle_accesskit_action_request(&mut self, req: &accesskit::ActionRequest) {
    //     if req.action == accesskit::Action::SetTextSelection {
    //         if let Some(accesskit::ActionData::SetTextSelection(selection)) = &req.data {
    //             self.inner.select_from_accesskit(selection);
    //         }
    //     }
    // }

    // --- MARK: Forced relayout ---
    /// Insert at cursor, or replace selection.
    fn replace_range_and_record(&mut self, range: Range<usize>, old_selection: Selection, s: &str) {
        let old_text = &self.text_box.text()[range.clone()];

        let new_range_start = range.start;
        let new_range_end = range.start + s.len();

        self.inner.history
            .record(&old_text, s, old_selection, new_range_start..new_range_end);

        self.text_box.text_mut().replace_range(range, s);
        
        if self.inner.single_line {
            self.remove_newlines();
        }
    }

    fn replace_selection_and_record(&mut self, s: &str) {
        let old_selection = self.text_box.selection();

        let range = self.text_box.selection().text_range();
        let old_text = &self.text_box.text()[range.clone()];

        let new_range_start = range.start;
        let new_range_end = range.start + s.len();

        self.inner.history.record(&old_text, s, old_selection, new_range_start..new_range_end);

        self.replace_selection(s);
    }

    // --- MARK: Forced relayout ---
    /// Insert at cursor, or replace selection.
    pub(crate) fn insert_or_replace_selection(&mut self, s: &str) {
        assert!(!self.is_composing());

        self.clear_placeholder();

        self.replace_selection_and_record(s);
    }

    pub(crate) fn clear_placeholder(&mut self) {
        // I love partial borrows!
        clear_placeholder!(self);
    }

    pub(crate) fn restore_placeholder_if_any(&mut self) {
        if let Some(placeholder) = &self.inner.placeholder_text {
            if self.text_box.text().is_empty() && !self.inner.showing_placeholder {
                self.text_box.text_mut().clear();
                self.text_box.text_mut().push_str(&placeholder);
                self.inner.showing_placeholder = true;
                self.text_box.inner.needs_relayout = true;
                self.text_box.set_selection(Selection::zero());
            }
        }
    }

    /// Delete the selection.
    pub(crate) fn delete_selection(&mut self) {
        assert!(!self.is_composing());

        self.insert_or_replace_selection("");
    }

    /// Delete the selection or the next cluster (typical ‘delete’ behavior).
    pub(crate) fn delete(&mut self) {
        assert!(!self.is_composing());

        if self.text_box.selection().is_collapsed() {
            // Upstream cluster range
            if let Some(range) = self
                .text_box.selection()
                .focus()
                .logical_clusters(&self.text_box.layout())[1]
                .as_ref()
                .map(|cluster| cluster.text_range())
                .and_then(|range| (!range.is_empty()).then_some(range))
            {
                self.replace_range_and_record(range, self.text_box.selection(), "");
                // seems ok to not do the relayout immediately
                self.text_box.inner.needs_relayout = true;
            }
        } else {
            self.delete_selection();
        }
    }

    /// Delete the selection or up to the next word boundary (typical ‘ctrl + delete’ behavior).
    pub(crate) fn delete_word(&mut self) {
        assert!(!self.is_composing());

        if self.text_box.selection().is_collapsed() {
            let focus = self.text_box.selection().focus();
            let start = focus.index();
            let end = focus.next_logical_word(&self.text_box.layout()).index();
            if self.text_box.text().get(start..end).is_some() {
                self.replace_range_and_record(start..end, self.text_box.selection(), "");
                // seems ok to not do the relayout immediately
                self.text_box.inner.needs_relayout = true;
                self.text_box.set_selection(
                    Cursor::from_byte_index(&self.text_box.layout(), start, Affinity::Downstream).into(),
                );
            }
        } else {
            self.delete_selection();
        }
    }

    /// Delete the selection or the previous cluster (typical ‘backspace’ behavior).
    pub(crate) fn backdelete(&mut self) {
        assert!(!self.is_composing());

        if self.text_box.selection().is_collapsed() {
            // Upstream cluster
            if let Some(cluster) = self
                .text_box.selection()
                .focus()
                .logical_clusters(&self.text_box.layout())[0]
                .clone()
            {
                let range = cluster.text_range();
                let end = range.end;
                let start = if cluster.is_hard_line_break() || cluster.is_emoji() {
                    // For newline sequences and emoji, delete the previous cluster
                    range.start
                } else {
                    // Otherwise, delete the previous character
                    let Some((start, _)) = self
                        .text_box.text()
                        .get(..end)
                        .and_then(|str| str.char_indices().next_back())
                    else {
                        return;
                    };
                    start
                };
                self.replace_range_and_record(start..end, self.text_box.selection(), "");
                // seems ok to not do the relayout immediately
                self.text_box.inner.needs_relayout = true;
                self.text_box.set_selection(
                    Cursor::from_byte_index(&self.text_box.layout(), start, Affinity::Downstream).into(),
                );
            }
        } else {
            self.delete_selection();
        }
    }

    /// Delete the selection or back to the previous word boundary (typical ‘ctrl + backspace’ behavior).
    pub(crate) fn backdelete_word(&mut self) {
        assert!(!self.is_composing());

        if self.text_box.selection().is_collapsed() {
            let focus = self.text_box.selection().focus();
            let end = focus.index();
            let start = focus.previous_logical_word(&self.text_box.layout()).index();
            if self.text_box.text().get(start..end).is_some() {
                self.replace_range_and_record(start..end, self.text_box.selection(), "");
                // seems ok to not do the relayout immediately
                self.text_box.inner.needs_relayout = true;
                self.text_box.set_selection(
                    Cursor::from_byte_index(&self.text_box.layout(), start, Affinity::Downstream).into(),
                );
            }
        } else {
            self.delete_selection();
        }
    }

    // --- MARK: IME ---
    /// Set the IME preedit composing text.
    ///
    /// This starts composing. Composing is reset by calling [`clear_compose`](Self::clear_compose).
    /// While composing, it is a logic error to call anything other than
    /// [`Self::set_compose()`] or [`Self::clear_compose()`].
    ///
    /// The preedit text replaces the current selection if this call starts composing.
    ///
    /// The selection is updated based on `cursor`, which contains the byte offsets relative to the
    /// start of the preedit text. If `cursor` is `None`, the selection and caret are hidden.
    pub(crate) fn set_compose(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        debug_assert!(!text.is_empty());
        debug_assert!(cursor.map(|cursor| cursor.1 <= text.len()).unwrap_or(true));

        let start = if let Some(preedit_range) = &self.inner.compose {
            self.text_box.text_mut().replace_range(preedit_range.clone(), text);
            preedit_range.start
        } else {
            let selection_start = self.text_box.selection().text_range().start;
            if self.text_box.selection().is_collapsed() {
                self.text_box.text_mut()
                    .insert_str(selection_start, text);
                
                if self.inner.single_line {
                    self.remove_newlines();
                }
            } else {
                let range = self.text_box.selection().text_range();
                self.text_box.text_mut()
                    .replace_range(range, text);
            }
            selection_start
        };
        self.inner.compose = Some(start..start + text.len());
        self.inner.show_cursor = cursor.is_some();

        // Select the location indicated by the IME. If `cursor` is none, collapse the selection to
        // a caret at the start of the preedit text.

        self.refresh_layout();

        let cursor = cursor.unwrap_or((0, 0));
        self.text_box.set_selection(Selection::new(
            // In parley, the layout is updated first, then the checked version is used. This should be fine too.
            Cursor::from_byte_index_unchecked(start + cursor.0, Affinity::Downstream),
            Cursor::from_byte_index_unchecked(start + cursor.1, Affinity::Downstream),
        ));

        self.text_box.inner.needs_relayout = true;
    }

    /// Stop IME composing.
    ///
    /// This removes the IME preedit text.
    pub(crate) fn clear_compose(&mut self) {
        if let Some(preedit_range) = self.inner.compose.take() {
            self.text_box.text_mut().replace_range(preedit_range.clone(), "");
            self.inner.show_cursor = true;

            let (index, affinity) = if preedit_range.start >= self.text_box.text().len() {
                (self.text_box.text().len(), Affinity::Upstream)
            } else {
                (preedit_range.start, Affinity::Downstream)
            };

            // In parley, the layout is updated first, then the checked version is used. This should be fine too.
            self.text_box.inner.selection.selection = Cursor::from_byte_index_unchecked(index, affinity).into();
        }
    }

    // #[cfg(feature = "accesskit")]
    // /// Select inside the editor based on the selection provided by accesskit.
    // pub(crate) fn select_from_accesskit(&mut self, selection: &accesskit::TextSelection) {
    //     assert!(!self.inner.is_composing());

    //     self.inner.refresh_layout();
    //     if let Some(selection) =
    //         Selection::from_access_selection(selection, &self.inner.layout, &self.inner.layout_access)
    //     {
    //         self.inner.set_selection(selection);
    //     }
    // }

    // // --- MARK: Rendering ---
    // #[cfg(feature = "accesskit")]
    // /// Perform an accessibility update.
    // pub(crate) fn accessibility(
    //     &mut self,
    //     update: &mut TreeUpdate,
    //     node: &mut Node,
    //     next_node_id: impl FnMut() -> NodeId,
    //     x_offset: f64,
    //     y_offset: f64,
    // ) -> Option<()> {
    //     self.inner.refresh_layout();
    //     self.inner.accessibility_unchecked(update, node, next_node_id, x_offset, y_offset);
    //     Some(())
    // }

    pub(crate) fn undo(&mut self) {
        if self.is_composing() {
            return;
        }

        if let Some(op) = self.inner.history.undo(self.text_box.text_mut()) {

            if ! op.text_to_restore.is_empty() {
                clear_placeholder!(self);
            }

            self
                .text_box.text_mut()
                .replace_range(op.range_to_clear.clone(), "");
            self
                .text_box.text_mut()
                .insert_str(op.range_to_clear.start, op.text_to_restore);

            let prev_selection = op.prev_selection;
            self.text_box.set_selection(prev_selection);
            
            if self.inner.single_line {
                self.remove_newlines();
            }
        }
    }

    pub(crate) fn redo(&mut self) {
        if self.is_composing() {
            return;
        }

        if let Some(op) = self.inner.history.redo() {
            self
                .text_box.text_mut()
                .replace_range(op.range_to_clear.clone(), "");

            if ! op.text_to_restore.is_empty() {
                clear_placeholder!(self);
            }

            self
                .text_box.text_mut()
                .insert_str(op.range_to_clear.start, op.text_to_restore);

            let end = op.range_to_clear.start + op.text_to_restore.len();

            // In parley, the layout is updated first, then the checked version is used. This should be fine too.
            self.text_box.inner.selection.selection = Cursor::from_byte_index_unchecked(end, Affinity::Upstream).into();
            
            if self.inner.single_line {
                self.remove_newlines();
            }
        }
    }

    fn replace_selection(&mut self, s: &str) {
        let range = self.text_box.selection().text_range();
        let start = range.start;
        if self.text_box.selection().is_collapsed() {
            self.text_box.text_mut().insert_str(start, s);
            
            if self.inner.single_line {
                self.remove_newlines();
            }
        } else {
            self.text_box.text_mut().replace_range(range, s);
        
        if self.inner.single_line {
            self.remove_newlines();
        }
        }

        let index = start.saturating_add(s.len());
        let affinity = if s.ends_with("\n") {
            Affinity::Downstream
        } else {
            Affinity::Upstream
        };

        // In parley, the layout is updated first, then the checked version is used. This should be fine too.
        self.text_box.inner.selection.selection = Cursor::from_byte_index_unchecked(index, affinity).into();
    }

}


#[derive(Clone, Debug)]
pub(crate) struct TextEditHistory {
    undo_text: String,
    redo_text: String,
    history: Vec<RecordedOp>,
    current_position: usize,
    can_grow: GrowHint,
}

#[derive(Clone, Copy, Debug)]
enum GrowHint {
    CannotGrow,
    GrowableInsert(usize),
    GrowableInsertWhitespace(usize),
    GrowableDelete(usize),
    GrowableDeleteWhitespace(usize),
}

#[derive(Debug, Clone)]
struct RecordedOp {
    /// Data needed to undo this history element.
    undo: Ranges,
    /// Data needed to redo this history element.
    /// To save memory, the redo data only gets populated when the element is undone.
    redo: Option<Ranges>,
    /// State of the selection right before this operation.
    prev_selection: Selection,
}

/// Internal Data for an undo or redo operation.
#[derive(Debug, Clone)]
struct Ranges {
    /// A range into the editor's main buffer for text that was inserted as part of a replace.
    inserted_range: Range<usize>,
    /// A range into the `TextEditHistory`'s internal buffer for text was deleted as part of a replace and stored.
    deleted_range: Range<usize>,
}

impl Ranges {
    fn is_delete_only(&self) -> bool {
        return self.inserted_range.is_empty();
    }
    fn is_insert_only(&self) -> bool {
        return self.deleted_range.is_empty();
    }
}

/// The result of undoing or redoing a text replace operation.
#[derive(Debug, Clone)]
struct TextRestore<'a> {
    /// A range into the original buffer that should be cleared.
    range_to_clear: Range<usize>,
    /// Text that should be inserted in the place of the cleared range.
    text_to_restore: &'a str,
    /// The state of selection right before the operation was made.
    /// Typically, undo operations restore the selection to this stored value,
    /// while redo operations ignore it and place a collapsed selection at the end of the newly restored text.
    prev_selection: Selection,
}

impl TextEditHistory {
    pub(crate) fn new() -> TextEditHistory {
        Self {
            undo_text: String::with_capacity(64),
            redo_text: String::with_capacity(64),
            history: Vec::with_capacity(64),
            current_position: 0,
            can_grow: GrowHint::CannotGrow,
        }
    }
}

trait StringBuffer {
    fn store_str(&mut self, text: &str) -> Range<usize>;
}
impl StringBuffer for String {
    fn store_str(&mut self, text: &str) -> Range<usize> {
        let start = self.len();
        self.push_str(text);
        start..self.len()
    }
}
trait WhitespaceStr {
    fn is_whitespace(&self) -> bool;
}
impl WhitespaceStr for &str {
    fn is_whitespace(&self) -> bool {
        self.chars().all(|c| c.is_whitespace() || c.is_ascii_punctuation())
    }
}

impl TextEditHistory {
    const MAX_GROWABLE_SIZE: usize = 20;

    #[rustfmt::skip]
    pub fn record(
        &mut self,
        old_str: &str,
        new_str: &str,
        selection: Selection,
        inserted_range: Range<usize>,
    ) {
        if self.current_position < self.history.len() {
            let undo_trunc = self.history[self.current_position].undo.deleted_range.start;
            self.undo_text.truncate(undo_trunc);
            self.redo_text.clear();
            self.history.truncate(self.current_position);
        }

        if let Some(last) = self.history.last_mut() {
            match self.can_grow {
                GrowHint::GrowableInsert(size) 
                    if old_str.is_empty() && size < Self::MAX_GROWABLE_SIZE =>
                        last.undo.inserted_range.end = inserted_range.end,

                GrowHint::GrowableInsertWhitespace(size) 
                    if old_str.is_empty() && new_str.is_whitespace() && size < Self::MAX_GROWABLE_SIZE =>
                        last.undo.inserted_range.end = inserted_range.end,

                GrowHint::GrowableDelete(size)
                    if inserted_range.is_empty() && size < Self::MAX_GROWABLE_SIZE =>
                        self.merge_delete(old_str, inserted_range),

                GrowHint::GrowableDeleteWhitespace(size)
                    if inserted_range.is_empty() && old_str.is_whitespace() && size < Self::MAX_GROWABLE_SIZE =>
                        self.merge_delete(old_str, inserted_range),

                _ => {
                    self.push_new(old_str, selection, inserted_range);
                },
            };
        } else {
            self.push_new(old_str, selection, inserted_range);
        }

        self.set_grow_hint(new_str, old_str);
    }

    pub fn push_new(&mut self, old_str: &str, selection: Selection, inserted_range: Range<usize>) {
        let undo_range = self.undo_text.store_str(old_str);

        self.history.push(RecordedOp {
            prev_selection: selection,
            undo: Ranges {
                inserted_range,
                deleted_range: undo_range,
            },
            redo: None,
        });

        self.current_position += 1;
    }

    fn merge_delete(&mut self, old_str: &str, inserted_range: Range<usize>) {
        let last = self.history.last_mut().unwrap();
        let start = last.undo.deleted_range.start;
        // To keep the text stored in the proper order, the old text has to be shifted.
        self.undo_text.insert_str(start, old_str);
        let end = self.undo_text.len();
        last.undo.deleted_range = start..end;
        last.undo.inserted_range = inserted_range.clone();
    }

    fn set_grow_hint(&mut self, new_str: &str, old_str: &str) {
        let last_op = &self.history.last().unwrap().undo;

        self.can_grow = if last_op.is_insert_only() {
            let len = new_str.len();
            match new_str.chars().last() {
                Some(c) if c.is_whitespace() => GrowHint::GrowableInsertWhitespace(len),
                Some(_) => GrowHint::GrowableInsert(len),
                None => GrowHint::CannotGrow,
            }
        } else if last_op.is_delete_only() {
            let len = old_str.len();
            match old_str.chars().last() {
                Some(c) if c.is_whitespace() => GrowHint::GrowableDeleteWhitespace(len),
                Some(_) => GrowHint::GrowableDelete(len),
                None => GrowHint::CannotGrow,
            }
        } else {
            GrowHint::CannotGrow
        };
    }

    fn undo(&mut self, buffer: &String) -> Option<TextRestore<'_>> {
        if self.current_position > 0 {
            self.current_position -= 1;
            let last = &mut self.history[self.current_position];

            // Prepare the undo to return
            let undo_text = last.undo.deleted_range.clone();
            let undo = TextRestore {
                prev_selection: last.prev_selection,
                range_to_clear: last.undo.inserted_range.clone(),
                text_to_restore: &self.undo_text[undo_text.clone()],
            };

            // Fill the last element with the data that will be needed for the redo
            if last.redo.is_none() {
                let redo_text = &buffer[undo.range_to_clear.clone()];
                let a = undo.range_to_clear.start;
                let redo_range = self.redo_text.store_str(redo_text);

                last.redo = Some(Ranges {
                    inserted_range: a..(a + undo_text.len()),
                    deleted_range: redo_range,
                });
            }
            // todo: if possible, put a nice prev_selection here so the caller doesn't have to think about it

            Some(undo)
        } else {
            None
        }
    }

    fn redo(&mut self) -> Option<TextRestore<'_>> {
        let last = self.history.get_mut(self.current_position)?;

        self.current_position += 1;

        let redo = last.redo.as_ref().unwrap().clone();
        let old_text = redo.deleted_range;

        Some(TextRestore {
            prev_selection: last.prev_selection,
            range_to_clear: redo.inserted_range,
            text_to_restore: &self.redo_text[old_text],
        })
    }
}

/// Replace newlines with spaces in-place. This probably doesn't allocate.
fn remove_newlines_inplace(text: &mut String) -> bool {
    let mut changed = false;
    for i in 0..text.len() {
        let b = text.as_bytes()[i];
        if b == b'\n' || b == b'\r' {
            text.replace_range(i..=i, " ");
            changed = true;
        }
    }

    return changed;
}

/// A text edit with access to both inner data and style.
/// 
/// This struct provides a convenient interface for working with text edits
/// while having access to the style for operations like layout refresh.
/// 
/// Instances of this struct are returned by [`Text::get_text_edit()`] and [`Text::get_text_edit_mut()`].
/// It provides methods to access and modify the text edit's content, styling, and positioning.
pub struct TextEdit<'a> {
    pub(crate) inner: &'a mut TextEditInner,
    pub(crate) edit_style: &'a mut TextEditStyle,
    pub(crate) text_box: TextBox<'a>,
}

impl<'a> TextEdit<'a> {
    // Delegate all methods to inner for now - we'll move implementations later
    pub fn raw_text(&self) -> &str {
        self.text_box.text()
    }
    
    pub fn selected_text(&self) -> Option<&str> {
        self.text_box.selected_text()
    }
    
    pub fn pos(&self) -> (f64, f64) {
        self.text_box.pos()
    }
    
    pub fn hidden(&self) -> bool {
        self.text_box.hidden()
    }
    
    pub fn depth(&self) -> f32 {
        self.text_box.depth()
    }
    
    pub fn clip_rect(&self) -> Option<parley::Rect> {
        self.text_box.clip_rect()
    }
    
    pub fn fadeout_clipping(&self) -> bool {
        self.text_box.fadeout_clipping()
    }
    
    pub fn auto_clip(&self) -> bool {
        self.text_box.auto_clip()
    }
    
    pub fn scroll_offset(&self) -> f32 {
        self.text_box.scroll_offset()
    }
    
    pub fn selection(&self) -> Selection {
        self.text_box.selection()
    }
    
    
    pub fn set_pos(&mut self, pos: (f64, f64)) {
        self.text_box.set_pos(pos);
    }
    
    pub fn set_hidden(&mut self, hidden: bool) {
        self.text_box.set_hidden(hidden);
    }
    
    pub fn set_depth(&mut self, value: f32) {
        self.text_box.set_depth(value);
    }
    
    pub fn set_clip_rect(&mut self, clip_rect: Option<parley::Rect>) {
        self.text_box.set_clip_rect(clip_rect);
    }
    
    pub fn set_fadeout_clipping(&mut self, fadeout_clipping: bool) {
        self.text_box.set_fadeout_clipping(fadeout_clipping);
    }
    
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.text_box.set_scroll_offset(offset);
    }
    
    /// Updates scroll offset to ensure cursor is visible for single-line edits
    /// Returns true if the scroll offset changed
    pub fn update_scroll_to_cursor(&mut self) -> bool {
        if !self.inner.single_line {
            return false;
        }

        if let Some(cursor_rect) = self.cursor_geometry(1.0) {
            let text_width = self.text_box.inner.max_advance;
            let cursor_x = cursor_rect.x0 as f32;
            let current_scroll = self.text_box.scroll_offset();
            
            // Get the total text width to check if we're overflowing
            let total_text_width = self.text_box.inner.layout.full_width();
            
            // Calculate visible range
            let visible_start = current_scroll;
            let visible_end = current_scroll + text_width;
            
            // Margin for cursor visibility - small buffer zone
            let margin = text_width * 0.05; // 5% margin
            
            // Check if cursor is outside visible range
            if cursor_x < visible_start + margin {
                // Cursor is too far left, scroll left
                let new_scroll = (cursor_x - margin).max(0.0);
                if (new_scroll - current_scroll).abs() > 0.5 {
                    self.text_box.set_scroll_offset(new_scroll);
                    return true;
                }
            } else if cursor_x > visible_end - margin {
                // Cursor is too far right, scroll right
                let new_scroll = cursor_x - text_width + margin;
                let max_scroll = (total_text_width - text_width).max(0.0);
                let new_scroll = new_scroll.min(max_scroll);
                if (new_scroll - current_scroll).abs() > 0.5 {
                    self.text_box.set_scroll_offset(new_scroll);
                    return true;
                }
            }
        }
        
        false
    }
    
    pub fn set_style(&mut self, style: &StyleHandle) {
        self.text_box.set_style(style);
    }
    
    pub fn cursor_geometry(&mut self, size: f32) -> Option<Rect> {
        if !self.inner.show_cursor {
            return None;
        }
        
        self.refresh_layout();
        Some(self.text_box.selection().focus().geometry(&self.text_box.inner.layout, size))
    }
    
    pub fn selection_geometry(&mut self) -> Vec<(Rect, usize)> {
        self.refresh_layout();
        self.text_box.selection_geometry()
    }
    
    pub fn selection_geometry_with(&mut self, f: impl FnMut(Rect, usize)) {
        self.refresh_layout();
        self.text_box.selection_geometry_with(f)
    }

    pub fn refresh_layout(&mut self) {
        let color_override = if self.inner.disabled {
            Some(self.edit_style.disabled_text_color)
        } else if self.inner.showing_placeholder {
            Some(self.edit_style.placeholder_text_color)
        } else {
            None
        };

        if self.text_box.inner.needs_relayout {
            self.text_box.rebuild_layout(color_override, self.inner.single_line);
        }
    }

    /// Programmatically set the text content of this text edit.
    /// This will replace all text and move the cursor to the end.
    pub fn set_text(&mut self, new_text: String) {
        self.text_box.text_mut().clear();
        self.text_box.text_mut().push_str(&new_text);
        self.text_box.inner.needs_relayout = true;
        self.text_box.move_to_text_end();
        // Clear any composition state
        self.inner.compose = None;
        // Reset cursor blinking
        self.cursor_reset();
        // Not showing placeholder anymore since we have real text
        self.inner.showing_placeholder = false;
    }

    /// Set placeholder text that will be shown when the text edit is empty
    pub fn set_placeholder(&mut self, placeholder: impl Into<Cow<'static, str>>) {
        let placeholder_cow = placeholder.into();
        self.inner.placeholder_text = Some(placeholder_cow.clone());
        if self.text_box.text().is_empty() || self.inner.showing_placeholder {
            self.text_box.text_mut().clear();
            self.text_box.text_mut().push_str(&placeholder_cow);
            self.text_box.inner.needs_relayout = true;
            self.inner.showing_placeholder = true;
            self.text_box.reset_selection();
        }
    }

    // todo: we could also pass a range to check only the newly inserted part.
    fn remove_newlines(&mut self) {
        let removed = remove_newlines_inplace(self.text_box.text_mut());
        if removed {
            self.text_box.inner.needs_relayout = true;
        }
    }

    pub fn set_ime_cursor_area(&mut self, window: &Window) {
        if let Some(area) = self.cursor_geometry(1.0) {
            // Note: on X11 `set_ime_cursor_area` may cause the exclusion area to be obscured
            // until https://github.com/rust-windowing/winit/pull/3966 is in the Winit release
            // used by this example.
            window.set_ime_cursor_area(
                winit::dpi::PhysicalPosition::new(
                    area.x0 + self.text_box.inner.left as f64,
                    area.y0 + self.text_box.inner.top as f64,
                ),
                winit::dpi::PhysicalSize::new(area.width(), area.height()),
            );
        }
    }
}

impl<'a> TextEdit<'a> {
    pub fn get_text_box_mut(&mut self) -> &mut TextBox<'a> {
        return &mut self.text_box;
    }

    pub fn get_text_box(&self) -> &TextBox<'a> {
        return &self.text_box;
    }
}