use std::{
    fmt::Display,
    time::{Duration, Instant},
};

use parley::*;
use winit::{
    event::{Ime, Touch, WindowEvent},
    keyboard::{Key, NamedKey},
    platform::modifier_supplement::KeyEventExtModifierSupplement,
};

const INSET: f32 = 2.0;

use crate::*;

/// A string which is potentially discontiguous in memory.
///
/// This is returned by [`PlainEditor::text`], as the IME preedit
/// area needs to be efficiently excluded from its return value.
#[derive(Debug, Clone, Copy)]
pub struct SplitString<'source>([&'source str; 2]);

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

    pub fn handle_event_edit(&mut self, event: &WindowEvent) {
        if !self.selectable {
            self.selection.focused = false;
            return;
        }
        if !self.focused() {
            return;
        }

        self.refresh_layout();

        self.handle_event(event, &self.modifiers.clone());

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
                            use clipboard_rs::{Clipboard, ClipboardContext};
                            match c.as_str() {
                                "c" if !shift => {
                                    if let Some(text) =
                                        self.text.get(self.selection.selection.text_range())
                                    {
                                        let cb = ClipboardContext::new().unwrap();
                                        cb.set_text(text.to_owned()).ok();
                                    }
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

        match event {
            WindowEvent::Resized(size) => {
                self.set_width(Some(size.width as f32 - 2_f32 * INSET));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
            }
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

                match &event.logical_key {
                    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                    Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
                        use clipboard_rs::{Clipboard, ClipboardContext};
                        match c.to_lowercase().as_str() {
                            "c" => {
                                if let Some(text) = self.selected_text() {
                                    let cb = ClipboardContext::new().unwrap();
                                    cb.set_text(text.to_owned()).ok();
                                }
                            }
                            "x" => {
                                if let Some(text) = self.selected_text() {
                                    let cb = ClipboardContext::new().unwrap();
                                    cb.set_text(text.to_owned()).ok();
                                    self.delete_selection();
                                }
                            }
                            "v" => {
                                let cb = ClipboardContext::new().unwrap();
                                let text = cb.get_text().unwrap_or_default();
                                self.insert_or_replace_selection(&text);
                            }
                            _ => (),
                        }
                    }
                    Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                        if !shift {
                            // todo: one single if shift
                            self.select_all();
                        }
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        if action_mod {
                            if !shift {
                                self.move_word_left();
                            }
                        } else if !shift {
                            self.move_left();
                        }
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        if action_mod {
                            if !shift {
                                self.move_word_right();
                            }
                        } else if !shift {
                            self.move_right();
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
                        if action_mod {
                            if !shift {
                                self.move_to_text_start();
                            }
                        } else if !shift {
                            self.move_to_line_start();
                        }
                    }
                    Key::Named(NamedKey::End) => {
                        if action_mod {
                            if !shift {
                                self.move_to_text_end();
                            }
                        } else if !shift {
                            self.move_to_line_end();
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
                        self.insert_or_replace_selection("\n");
                    }
                    Key::Named(NamedKey::Space) => {
                        self.insert_or_replace_selection(" ");
                    }
                    Key::Character(s) => {
                        self.insert_or_replace_selection(&s);
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
                dbg!(text, cursor);
                if text.is_empty() {
                    self.clear_compose();
                } else {
                    self.set_compose(&text, *cursor);
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
}

impl TextBox<String> {
    // --- MARK: Forced relayout ---
    /// Insert at cursor, or replace selection.
    pub fn insert_or_replace_selection(&mut self, s: &str) {
        assert!(!self.is_composing());

        self.replace_selection(s);
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
                self.text.replace_range(range, "");
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
                self.text.replace_range(start..end, "");
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
                self.text.replace_range(start..end, "");
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
                self.text.replace_range(start..end, "");
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

    // --- MARK: Cursor Movement ---
    /// Move the cursor to the cluster boundary nearest this point in the layout.
    pub fn move_to_point(&mut self, x: f32, y: f32) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(Selection::from_point(&self.layout, x, y));
    }

    /// Move the cursor to a byte index.
    ///
    /// No-op if index is not a char boundary.
    pub fn move_to_byte(&mut self, index: usize) {
        assert!(!self.is_composing());

        if self.text.is_char_boundary(index) {
            self.refresh_layout();
            self.set_selection(self.cursor_at(index).into());
        }
    }

    /// Move the cursor to the start of the text.
    pub fn move_to_text_start(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .move_lines(&self.layout, isize::MIN, false),
        );
    }

    /// Move the cursor to the start of the physical line.
    pub fn move_to_line_start(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(self.selection.selection.line_start(&self.layout, false));
    }

    /// Move the cursor to the end of the text.
    pub fn move_to_text_end(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .move_lines(&self.layout, isize::MAX, false),
        );
    }

    /// Move the cursor to the end of the physical line.
    pub fn move_to_line_end(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(self.selection.selection.line_end(&self.layout, false));
    }

    /// Move up to the closest physical cluster boundary on the previous line, preserving the horizontal position for repeated movements.
    pub fn move_up(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(self.selection.selection.previous_line(&self.layout, false));
    }

    /// Move down to the closest physical cluster boundary on the next line, preserving the horizontal position for repeated movements.
    pub fn move_down(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(self.selection.selection.next_line(&self.layout, false));
    }

    /// Move to the next cluster left in visual order.
    pub fn move_left(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .previous_visual(&self.layout, false),
        );
    }

    /// Move to the next cluster right in visual order.
    pub fn move_right(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(self.selection.selection.next_visual(&self.layout, false));
    }

    /// Move to the next word boundary left.
    pub fn move_word_left(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .previous_visual_word(&self.layout, false),
        );
    }

    /// Move to the next word boundary right.
    pub fn move_word_right(&mut self) {
        assert!(!self.is_composing());

        self.refresh_layout();
        self.set_selection(
            self.selection
                .selection
                .next_visual_word(&self.layout, false),
        );
    }

    /// Select the whole text.
    pub fn select_all(&mut self) {
        assert!(!self.is_composing());

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
        assert!(!self.is_composing());

        self.set_selection(self.selection.selection.collapse());
    }

    /// Move the selection focus point to the cluster boundary closest to point.
    pub fn extend_selection_to_point(&mut self, x: f32, y: f32, keep_granularity: bool) {
        assert!(!self.is_composing());

        self.refresh_layout();

        self.selection
            .extend_selection_to_point(&self.layout, x, y, keep_granularity);
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
}

impl TextBox<String> {
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
        if self.is_composing() {
            return None;
        }
        if !self.selection.selection.is_collapsed() {
            self.text.get(self.selection.selection.text_range())
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

    /// Borrow the text content of the text, including the IME preedit
    /// region if any.
    ///
    /// Application authors should generally prefer [`text`](Self::text). That method excludes the
    /// IME preedit contents, which are not meaningful for applications to access; the
    /// in-progress IME content is not itself what the user intends to write.
    pub fn raw_text(&self) -> &str {
        &self.text
    }

    /// Replace the whole text text.
    pub fn set_text(&mut self, is: &str) {
        assert!(!self.is_composing());

        self.text.clear();
        self.text.push_str(is);
        self.needs_relayout = true;
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

    /// Whether the editor is currently in IME composing mode.
    pub fn is_composing(&self) -> bool {
        self.compose.is_some()
    }

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
    fn cursor_at(&self, index: usize) -> Cursor {
        // TODO: Do we need to be non-dirty?
        // FIXME: `Selection` should make this easier
        if index >= self.text.len() {
            Cursor::from_byte_index(&self.layout, self.text.len(), Affinity::Upstream)
        } else {
            Cursor::from_byte_index(&self.layout, index, Affinity::Downstream)
        }
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

    /// Update the selection, and nudge the `Generation` if something other than `h_pos` changed.
    fn set_selection(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
        self.selection.prev_anchor = None;
    }

    /// Update the selection without resetting the previous anchor.
    fn set_selection_with_old_anchor(&mut self, new_sel: Selection) {
        self.set_selection_inner(new_sel);
    }

    fn set_selection_inner(&mut self, new_sel: Selection) {
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
                cluster[0].as_ref().map(|c| &self.text[c.text_range()]),
                focus.index(),
                focus.affinity(),
                cluster[1].as_ref().map(|c| &self.text[c.text_range()]),
            );
            eprint!("{dbg:?}");
            let cluster = focus.visual_clusters(&self.layout);
            let dbg = (
                cluster[0].as_ref().map(|c| &self.text[c.text_range()]),
                cluster[0]
                    .as_ref()
                    .map(|c| if c.is_word_boundary() { " W" } else { "" })
                    .unwrap_or_default(),
                focus.index(),
                focus.affinity(),
                cluster[1].as_ref().map(|c| &self.text[c.text_range()]),
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
    fn accessibility_unchecked(
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
}
