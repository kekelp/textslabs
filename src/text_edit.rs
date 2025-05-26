use std::{
    fmt::Display, ops::Range, time::{Duration, Instant}
};

use parley::*;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize}, event::{Ime, Touch, WindowEvent}, keyboard::{Key, NamedKey}, platform::modifier_supplement::KeyEventExtModifierSupplement, window::Window
};

const INSET: f32 = 2.0;

use crate::*;

/// A string which is potentially discontiguous in memory.
///
/// This is returned by [`PlainEditor::text`], as the IME preedit
/// area needs to be efficiently excluded from its return value.
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
// We intentionally choose not to:
// impl PartialEq<Self> for SplitString<'_> {}
// for simplicity, as the impl wouldn't be useful and is non-trivial

impl Display for SplitString<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

impl TextBox<String> {
    pub fn cursor_reset(&mut self) {
        self.start_time = Some(Instant::now());
        // TODO: for real world use, this should be reading from the system settings
        self.blink_period = Duration::from_millis(500);
        self.show_cursor = true;
    }

    pub fn disable_blink(&mut self) {
        self.start_time = None;
    }

    pub fn next_blink_time(&self) -> Option<Instant> {
        self.start_time.map(|start_time| {
            let phase = Instant::now().duration_since(start_time);

            start_time
                + Duration::from_nanos(
                    ((phase.as_nanos() / self.blink_period.as_nanos() + 1)
                        * self.blink_period.as_nanos()) as u64,
                )
        })
    }

    pub fn cursor_blink(&mut self) {
        self.show_cursor = self.start_time.is_some_and(|start_time| {
            let elapsed = Instant::now().duration_since(start_time);
            (elapsed.as_millis() / self.blink_period.as_millis()) % 2 == 0
        });
    }

    pub fn handle_event(&mut self, event: &WindowEvent, window: &Window) {
        if !self.editable || !self.focused() {
            self.show_cursor = false;
        }
        
        if !self.selectable {
            self.selection.focused = false;
            return;
        }
        if !self.focused() {
            return;
        }

        self.refresh_layout();

        self.handle_event_no_edit(event);

        if ! self.editable {
            return
        }

        match event {
            WindowEvent::KeyboardInput { event, .. } if !self.is_composing() => {
                if !event.state.is_pressed() {
                    return;
                }
                self.cursor_reset();
                #[allow(unused)]
                let mods_state = self.modifiers.state();
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
                            use clipboard_rs::{Clipboard, ClipboardContext};
                            match c.as_str() {
                                "x" if !shift => {
                                    if let Some(text) = self.selected_text() {
                                        let cb = ClipboardContext::new().unwrap();
                                        cb.set_text(text.to_owned()).ok();
                                        self.delete_selection();
                                    }
                                }
                                "v" if !shift => {
                                    let cb = ClipboardContext::new().unwrap();
                                    let text = cb.get_text().unwrap_or_default();
                                    self.insert_or_replace_selection(&text);
                                }
                                "z" => {
                                    if shift {
                                        self.redo();
                                    } else {
                                        self.undo();
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
                        if !shift {
                            if action_mod {
                                self.move_word_left();
                            } else {
                                self.move_left();
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        if !shift {                            
                            if action_mod {
                                self.move_word_right();
                            } else {
                                self.move_right();
                            }
                        }
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        if !shift {
                            self.move_up();
                        }
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if !shift {
                            self.move_down();
                        }
                    }
                    Key::Named(NamedKey::Home) => {
                        if !shift {
                            if action_mod {
                                self.move_to_text_start();
                            } else {
                                self.move_to_line_start();
                            }
                        }
                    }
                    Key::Named(NamedKey::End) => {
                        if !shift {
                            if action_mod {
                                self.move_to_text_end();
                            } else {
                                self.move_to_line_end();
                            }
                        }
                    }
                    Key::Named(NamedKey::Delete) => {
                        if action_mod {
                            self.delete_word();
                        } else {
                            self.delete();
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        if action_mod {
                            self.backdelete_word();
                        } else {
                            self.backdelete();
                        }
                    }
                    Key::Named(NamedKey::Enter) => {
                        // todo: make shift-enter, ctrl-enter configurable
                        if ! action_mod {
                            self.insert_or_replace_selection("\n");
                        }
                    }
                    Key::Named(NamedKey::Space) => {
                        if ! action_mod {
                            self.insert_or_replace_selection(" ");
                        }
                    }
                    Key::Character(s) => {
                        if ! action_mod {
                            self.insert_or_replace_selection(&s);
                        }
                    }
                    _ => (),
                }
            }
            WindowEvent::Touch(Touch {
                phase, location, ..
            }) if !self.is_composing() => {
                use winit::event::TouchPhase::*;
                match phase {
                    Started => {
                        // todo: use left and top. I can't test this though
                        // TODO: start a timer to convert to a SelectWordAtPoint
                        self.move_to_point(location.x as f32 - INSET, location.y as f32 - INSET);
                    }
                    Cancelled => {
                        self.collapse_selection();
                    }
                    Moved => {
                        // TODO: cancel SelectWordAtPoint timer
                        self.extend_selection_to_point(
                            location.x as f32 - INSET,
                            location.y as f32 - INSET,
                            true,
                        );
                    }
                    Ended => (),
                }
            }
            WindowEvent::Ime(Ime::Disabled) => {
                self.clear_compose();
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.insert_or_replace_selection(&text);
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                if text.is_empty() {
                    self.clear_compose();
                } else {
                    self.set_compose(&text, *cursor);
                    // todo: no idea if it's correct to call this here.
                    self.set_ime_cursor_area(window);
                }
            }
            _ => {}
        }
    }

    #[cfg(feature = "accesskit")]
    pub fn handle_accesskit_action_request(&mut self, req: &accesskit::ActionRequest) {
        if req.action == accesskit::Action::SetTextSelection {
            if let Some(accesskit::ActionData::SetTextSelection(selection)) = &req.data {
                self.select_from_accesskit(selection);
            }
        }
    }

    // --- MARK: Forced relayout ---
    /// Insert at cursor, or replace selection.
    fn replace_range_and_record(&mut self, range: Range<usize>, old_selection: Selection, s: &str) {
        let old_text = &self.text[range.clone()];

        let new_range_start = range.start;
        let new_range_end = range.start + s.len();

        self.history
            .record(&old_text, s, old_selection, new_range_start..new_range_end);

        self.text.replace_range(range, s);
    }

    fn replace_selection_and_record(&mut self, s: &str) {
        let old_selection = self.selection.selection;

        let range = self.selection.selection.text_range();
        let old_text = &self.text[range.clone()];

        let new_range_start = range.start;
        let new_range_end = range.start + s.len();

        self.history.record(&old_text, s, old_selection, new_range_start..new_range_end);

        self.replace_selection(s);
    }

    // --- MARK: Forced relayout ---
    /// Insert at cursor, or replace selection.
    pub fn insert_or_replace_selection(&mut self, s: &str) {
        assert!(!self.is_composing());

        self.replace_selection_and_record(s);
    }

    /// Delete the selection.
    pub fn delete_selection(&mut self) {
        assert!(!self.is_composing());

        self.insert_or_replace_selection("");
    }

    /// Delete the selection or the next cluster (typical ‘delete’ behavior).
    pub fn delete(&mut self) {
        assert!(!self.is_composing());

        if self.selection.selection.is_collapsed() {
            // Upstream cluster range
            if let Some(range) = self
                .selection
                .selection
                .focus()
                .logical_clusters(&self.layout)[1]
                .as_ref()
                .map(|cluster| cluster.text_range())
                .and_then(|range| (!range.is_empty()).then_some(range))
            {
                self.replace_range_and_record(range, self.selection.selection, "");
                // seems ok to not do the relayout immediately
                self.needs_relayout = true;
            }
        } else {
            self.delete_selection();
        }
    }

    /// Delete the selection or up to the next word boundary (typical ‘ctrl + delete’ behavior).
    pub fn delete_word(&mut self) {
        assert!(!self.is_composing());

        if self.selection.selection.is_collapsed() {
            let focus = self.selection.selection.focus();
            let start = focus.index();
            let end = focus.next_logical_word(&self.layout).index();
            if self.text.get(start..end).is_some() {
                self.replace_range_and_record(start..end, self.selection.selection, "");
                // seems ok to not do the relayout immediately
                self.needs_relayout = true;
                self.set_selection(
                    Cursor::from_byte_index(&self.layout, start, Affinity::Downstream).into(),
                );
            }
        } else {
            self.delete_selection();
        }
    }

    /// Delete the selection or the previous cluster (typical ‘backspace’ behavior).
    pub fn backdelete(&mut self) {
        assert!(!self.is_composing());

        if self.selection.selection.is_collapsed() {
            // Upstream cluster
            if let Some(cluster) = self
                .selection
                .selection
                .focus()
                .logical_clusters(&self.layout)[0]
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
                        .text
                        .get(..end)
                        .and_then(|str| str.char_indices().next_back())
                    else {
                        return;
                    };
                    start
                };
                self.replace_range_and_record(start..end, self.selection.selection, "");
                // seems ok to not do the relayout immediately
                self.needs_relayout = true;
                self.set_selection(
                    Cursor::from_byte_index(&self.layout, start, Affinity::Downstream).into(),
                );
            }
        } else {
            self.delete_selection();
        }
    }

    /// Delete the selection or back to the previous word boundary (typical ‘ctrl + backspace’ behavior).
    pub fn backdelete_word(&mut self) {
        assert!(!self.is_composing());

        if self.selection.selection.is_collapsed() {
            let focus = self.selection.selection.focus();
            let end = focus.index();
            let start = focus.previous_logical_word(&self.layout).index();
            if self.text.get(start..end).is_some() {
                self.replace_range_and_record(start..end, self.selection.selection, "");
                // seems ok to not do the relayout immediately
                self.needs_relayout = true;
                self.set_selection(
                    Cursor::from_byte_index(&self.layout, start, Affinity::Downstream).into(),
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
    /// [`Self::set_compose`] or [`Self::clear_compose`].
    ///
    /// The preedit text replaces the current selection if this call starts composing.
    ///
    /// The selection is updated based on `cursor`, which contains the byte offsets relative to the
    /// start of the preedit text. If `cursor` is `None`, the selection and caret are hidden.
    pub fn set_compose(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        debug_assert!(!text.is_empty());
        debug_assert!(cursor.map(|cursor| cursor.1 <= text.len()).unwrap_or(true));

        let start = if let Some(preedit_range) = &self.compose {
            self.text.replace_range(preedit_range.clone(), text);
            preedit_range.start
        } else {
            if self.selection.selection.is_collapsed() {
                self.text
                    .insert_str(self.selection.selection.text_range().start, text);
            } else {
                self.text
                    .replace_range(self.selection.selection.text_range(), text);
            }
            self.selection.selection.text_range().start
        };
        self.compose = Some(start..start + text.len());
        self.show_cursor = cursor.is_some();
        self.update_layout();

        // Select the location indicated by the IME. If `cursor` is none, collapse the selection to
        // a caret at the start of the preedit text. As `self.show_cursor` is `false`, it
        // won't show up.
        let cursor = cursor.unwrap_or((0, 0));
        self.set_selection(Selection::new(
            self.cursor_at(start + cursor.0),
            self.cursor_at(start + cursor.1),
        ));
    }

    /// Stop IME composing.
    ///
    /// This removes the IME preedit text.
    pub fn clear_compose(&mut self) {
        if let Some(preedit_range) = self.compose.take() {
            self.text.replace_range(preedit_range.clone(), "");
            self.show_cursor = true;
            self.update_layout();

            self.set_selection(self.cursor_at(preedit_range.start).into());
        }
    }

    #[cfg(feature = "accesskit")]
    /// Select inside the editor based on the selection provided by accesskit.
    pub fn select_from_accesskit(&mut self, selection: &accesskit::TextSelection) {
        assert!(!self.is_composing());

        self.refresh_layout();
        if let Some(selection) =
            Selection::from_access_selection(selection, &self.layout, &self.layout_access)
        {
            self.set_selection(selection);
        }
    }

    // --- MARK: Rendering ---
    #[cfg(feature = "accesskit")]
    /// Perform an accessibility update.
    pub fn accessibility(
        &mut self,
        update: &mut TreeUpdate,
        node: &mut Node,
        next_node_id: impl FnMut() -> NodeId,
        x_offset: f64,
        y_offset: f64,
    ) -> Option<()> {
        self.refresh_layout();
        self.accessibility_unchecked(update, node, next_node_id, x_offset, y_offset);
        Some(())
    }

    pub fn undo(&mut self) {
        if ! self.is_composing() {
            if let Some(op) = self.history.undo(&self.text) {
                self
                    .text
                    .replace_range(op.range_to_clear.clone(), "");
                self
                    .text
                    .insert_str(op.range_to_clear.start, op.text_to_restore);

                let prev_selection = op.prev_selection;
                self.set_selection(prev_selection);
                self.update_layout();
            }
        }
    }

    pub fn redo(&mut self) {
        if let Some(op) = self.history.redo() {
            self
                .text
                .replace_range(op.range_to_clear.clone(), "");
            self
                .text
                .insert_str(op.range_to_clear.start, op.text_to_restore);

            let end = op.range_to_clear.start + op.text_to_restore.len();

            self.update_layout();

            let new_selection =
                Selection::from_byte_index(&self.layout, end, Affinity::Upstream);
            self.set_selection(new_selection);
        }
    }

    /// Replace the whole text text.
    pub fn set_text(&mut self, is: &str) {
        assert!(!self.is_composing());

        self.text.clear();
        self.text.push_str(is);
        self.needs_relayout = true;
    }

    fn replace_selection(&mut self, s: &str) {
        let range = self.selection.selection.text_range();
        let start = range.start;
        if self.selection.selection.is_collapsed() {
            self.text.insert_str(start, s);
        } else {
            self.text.replace_range(range, s);
        }

        self.update_layout();
        let new_index = start.saturating_add(s.len());
        let affinity = if s.ends_with("\n") {
            Affinity::Downstream
        } else {
            Affinity::Upstream
        };
        self.set_selection(Cursor::from_byte_index(&self.layout, new_index, affinity).into());
    }

    /// Borrow the text content of the text.
    ///
    /// The return value is a `SplitString` because it
    /// excludes the IME preedit region.
    pub fn text(&self) -> SplitString<'_> {
        if let Some(preedit_range) = &self.compose {
            SplitString([
                &self.text[..preedit_range.start],
                &self.text[preedit_range.end..],
            ])
        } else {
            SplitString([&self.text, ""])
        }
    }


    /// Get a rectangle bounding the text the user is currently editing.
    ///
    /// This is useful for suggesting an exclusion area to the platform for, e.g., IME candidate
    /// box placement. This bounds the area of the preedit text if present, otherwise it bounds the
    /// selection on the focused line.
    pub fn ime_cursor_area(&self) -> Rect {
        let (area, focus) = if let Some(preedit_range) = &self.compose {
            let selection = Selection::new(
                self.cursor_at(preedit_range.start),
                self.cursor_at(preedit_range.end),
            );

            // Bound the entire preedit text.
            let mut area = None;
            selection.geometry_with(&self.layout, |rect, _| {
                let area = area.get_or_insert(rect);
                *area = area.union(rect);
            });

            (
                area.unwrap_or_else(|| selection.focus().geometry(&self.layout, 0.)),
                selection.focus(),
            )
        } else {
            // Bound the selected parts of the focused line only.
            let focus = self.selection.selection.focus().geometry(&self.layout, 0.);
            let mut area = focus;
            self.selection
                .selection
                .geometry_with(&self.layout, |rect, _| {
                    if rect.y0 == focus.y0 {
                        area = area.union(rect);
                    }
                });

            (area, self.selection.selection.focus())
        };

        // Ensure some context is captured even for tiny or collapsed selections by including a
        // region surrounding the selection. Doing this unconditionally, the IME candidate box
        // usually does not need to jump around when composing starts or the preedit is added to.
        let [upstream, downstream] = focus.logical_clusters(&self.layout);
        let font_size = downstream
            .or(upstream)
            .map(|cluster| cluster.run().font_size())
            // .unwrap_or(ResolvedStyle::<ColorBrush>::default().font_size);
            .unwrap_or(16.0);
        // Using 0.6 as an estimate of the average advance
        let inflate = 3. * 0.6 * font_size as f64;
        let editor_width = self.width.map(f64::from).unwrap_or(f64::INFINITY);
        Rect {
            x0: (area.x0 - inflate).max(0.),
            x1: (area.x1 + inflate).min(editor_width),
            y0: area.y0,
            y1: area.y1,
        }
    }

    pub fn set_ime_cursor_area(&self, window: &Window) {
        let area = self.ime_cursor_area();
        // Note: on X11 `set_ime_cursor_area` may cause the exclusion area to be obscured
        // until https://github.com/rust-windowing/winit/pull/3966 is in the Winit release
        // used by this example.
        window.set_ime_cursor_area(
            PhysicalPosition::new(
                area.x0 + self.left as f64,
                area.y0 + self.top as f64,
            ),
            PhysicalSize::new(area.width(), area.height()),
        );
    }

    /// Whether the editor is currently in IME composing mode.
    pub fn is_composing(&self) -> bool {
        self.compose.is_some()
    }

    /// Get a rectangle representing the current caret cursor position.
    ///
    /// There is not always a caret. For example, the IME may have indicated the caret should be
    /// hidden.
    pub fn cursor_geometry(&self, size: f32) -> Option<Rect> {
        self.show_cursor.then(|| {
            self.selection
                .selection
                .focus()
                .geometry(&self.layout, size)
        })
    }
}


#[derive(Clone, Debug)]
pub struct TextEditHistory {
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

/// The result of undoing of redoing a text replace operation.
#[derive(Debug, Clone)]
pub struct TextRestore<'a> {
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
    pub(crate) fn empty() -> TextEditHistory {
        Self {
            undo_text: String::new(),
            redo_text: String::new(),
            history: Vec::new(),
            current_position: 0,
            can_grow: GrowHint::CannotGrow,
        }
    }
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