use std::sync::atomic::{AtomicU64, Ordering};

use crate::*;
use accesskit::NodeId;
use parley::Selection;

// Maybe we can get away with this? Just grab a range in the u64 space?
// Nodoby else would be dumb enough to try this, so it probably works
pub(crate) fn next_node_id() -> NodeId {
static NEXT: AtomicU64 = AtomicU64::new(16075019835661180680);
NodeId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Accessibility support for Textslabs
impl Text {
    /// Handle accessibility action requests for the focused text box or text edit
    /// 
    /// This is mostly untested.
    pub fn handle_accessibility_action(&mut self, request: &accesskit::ActionRequest) -> bool {
        match request.action {
            accesskit::Action::SetTextSelection => {
                if let Some(focused) = self.focused {
                    if let Some(accesskit::ActionData::SetTextSelection(selection)) = &request.data {

                        let mut text_box = match focused {
                            AnyBox::TextEdit(i) => {
                                let handle = TextEditHandle { i };
                                self.get_text_edit_mut(&handle).text_box
                            }
                            AnyBox::TextBox(i) => {
                                let handle = TextBoxHandle { i };
                                self.get_text_box_mut(&handle)
                            }
                        };

                        if let Some(selection) = Selection::from_access_selection(
                            selection,
                            &text_box.inner.layout,
                            &text_box.inner.layout_access

                        ) {
                            text_box.set_selection(selection);
                        }
                    }
                }
                false
            }
            accesskit::Action::ReplaceSelectedText => {
                if let Some(focused) = self.focused {
                    if let Some(accesskit::ActionData::Value(text)) = &request.data {
                        match focused {
                            AnyBox::TextEdit(i) => {
                                let handle = TextEditHandle { i };
                                self.get_text_edit_mut(&handle).replace_selection(&text);
                                return true;
                            }
                            _ => {}
                        }
                    }
                }
                false
            }
            // todo: we can at least deal with the scroll ones, if a text edit is focused
            _ => false
        }
    }
}

#[derive(PartialEq)]
pub enum FocusUpdate {
    FocusedNewNode(NodeId),
    Defocused,
    Unchanged,
}