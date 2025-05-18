use std::{fmt::Display, time::{Duration, Instant}};

use winit::event::WindowEvent;

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

impl<> TextBox<String> {
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

    pub fn handle_event_edit(&mut self, event: WindowEvent) {
        with_text_cx(|mut layout_cx, mut font_cx| {

            match event {
                WindowEvent::Resized(size) => {
                    self.editor
                        .set_width(Some(size.width as f32 - 2_f32 * INSET));
                }
                WindowEvent::ModifiersChanged(modifiers) => {
                    self.modifiers = modifiers;
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

                    match event.logical_key {
                        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                        Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
                            use clipboard_rs::{Clipboard, ClipboardContext};
                            match c.to_lowercase().as_str() {
                                "c" => {
                                    if let Some(text) = drv.editor.selected_text() {
                                        let cb = ClipboardContext::new().unwrap();
                                        cb.set_text(text.to_owned()).ok();
                                    }
                                }
                                "x" => {
                                    if let Some(text) = drv.editor.selected_text() {
                                        let cb = ClipboardContext::new().unwrap();
                                        cb.set_text(text.to_owned()).ok();
                                        drv.delete_selection();
                                    }
                                }
                                "v" => {
                                    let cb = ClipboardContext::new().unwrap();
                                    let text = cb.get_text().unwrap_or_default();
                                    drv.insert_or_replace_selection(&text);
                                }
                                _ => (),
                            }
                        }
                        Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                            if shift {
                                drv.collapse_selection();
                            } else {
                                drv.select_all();
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            if action_mod {
                                if shift {
                                    drv.select_word_left();
                                } else {
                                    drv.move_word_left();
                                }
                            } else if shift {
                                drv.select_left();
                            } else {
                                drv.move_left();
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if action_mod {
                                if shift {
                                    drv.select_word_right();
                                } else {
                                    drv.move_word_right();
                                }
                            } else if shift {
                                drv.select_right();
                            } else {
                                drv.move_right();
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            if shift {
                                drv.select_up();
                            } else {
                                drv.move_up();
                            }
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            if shift {
                                drv.select_down();
                            } else {
                                drv.move_down();
                            }
                        }
                        Key::Named(NamedKey::Home) => {
                            if action_mod {
                                if shift {
                                    drv.select_to_text_start();
                                } else {
                                    drv.move_to_text_start();
                                }
                            } else if shift {
                                drv.select_to_line_start();
                            } else {
                                drv.move_to_line_start();
                            }
                        }
                        Key::Named(NamedKey::End) => {
                            let this = &mut *self;
                            let mut drv = this.driver();

                            if action_mod {
                                if shift {
                                    drv.select_to_text_end();
                                } else {
                                    drv.move_to_text_end();
                                }
                            } else if shift {
                                drv.select_to_line_end();
                            } else {
                                drv.move_to_line_end();
                            }
                        }
                        Key::Named(NamedKey::Delete) => {
                            if action_mod {
                                drv.delete_word();
                            } else {
                                drv.delete();
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            if action_mod {
                                drv.backdelete_word();
                            } else {
                                drv.backdelete();
                            }
                        }
                        Key::Named(NamedKey::Enter) => {
                            drv.insert_or_replace_selection("\n");
                        }
                        Key::Named(NamedKey::Space) => {
                            drv.insert_or_replace_selection(" ");
                        }
                        Key::Character(s) => {
                            drv.insert_or_replace_selection(&s);
                        }
                        _ => (),
                    }
                }
                WindowEvent::Touch(Touch {
                    phase, location, ..
                }) if !self.editor.is_composing() => {
                    let mut drv = self.editor.driver(&mut self.font_cx, &mut self.layout_cx);
                    use winit::event::TouchPhase::*;
                    match phase {
                        Started => {
                            // TODO: start a timer to convert to a SelectWordAtPoint
                            drv.move_to_point(location.x as f32 - INSET, location.y as f32 - INSET);
                        }
                        Cancelled => {
                            drv.collapse_selection();
                        }
                        Moved => {
                            // TODO: cancel SelectWordAtPoint timer
                            drv.extend_selection_to_point(
                                location.x as f32 - INSET,
                                location.y as f32 - INSET,
                                true,
                            );
                        }
                        Ended => (),
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    let shift = self.modifiers.state().shift_key();

                    if button == winit::event::MouseButton::Left {
                        self.pointer_down = state.is_pressed();
                        self.cursor_reset();
                        if self.pointer_down && !self.editor.is_composing() {
                            let now = Instant::now();
                            if let Some(last) = self.last_click_time.take() {
                                if now.duration_since(last).as_secs_f64() < 0.25 {
                                    self.click_count = (self.click_count + 1) % 4;
                                } else {
                                    self.click_count = 1;
                                }
                            } else {
                                self.click_count = 1;
                            }
                            self.last_click_time = Some(now);
                            let click_count = self.click_count;
                            let cursor_pos = self.cursor_pos;
                            let mut drv = self.editor.driver(&mut self.font_cx, &mut self.layout_cx);
                            match click_count {
                                2 => drv.select_word_at_point(cursor_pos.0, cursor_pos.1),
                                3 => drv.select_line_at_point(cursor_pos.0, cursor_pos.1),
                                _ => if shift {
                                    drv.extend_selection_with_anchor(cursor_pos.0, cursor_pos.1)
                                } else {
                                    drv.move_to_point(cursor_pos.0, cursor_pos.1)
                                }
                            }
                        }
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let prev_pos = self.cursor_pos;
                    self.cursor_pos = (position.x as f32 - INSET, position.y as f32 - INSET);
                    // macOS seems to generate a spurious move after selecting word?
                    if self.pointer_down && prev_pos != self.cursor_pos && !self.editor.is_composing() {
                        self.cursor_reset();
                        let cursor_pos = self.cursor_pos;
                        self.driver()
                            .extend_selection_to_point(cursor_pos.0, cursor_pos.1, true);
                    }
                }
                WindowEvent::Ime(Ime::Disabled) => {
                    self.driver().clear_compose();
                }
                WindowEvent::Ime(Ime::Commit(text)) => {
                    self.driver().insert_or_replace_selection(&text);
                }
                WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                    if text.is_empty() {
                        self.driver().clear_compose();
                    } else {
                        self.driver().set_compose(&text, cursor);
                    }
                }
                _ => {}
            }
        });
    }

    pub fn handle_accesskit_action_request(&mut self, req: &accesskit::ActionRequest) {
        if req.action == accesskit::Action::SetTextSelection {
            if let Some(accesskit::ActionData::SetTextSelection(selection)) = &req.data {
                self.driver().select_from_accesskit(selection);
            }
        }
    }

}



