use std::sync::atomic::{AtomicU64, Ordering};

use crate::*;
use accesskit::NodeId;

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
    /// This should be called when AccessKit sends action requests to the application.
    /// Returns true if the action was handled.
    pub fn handle_accessibility_action(&mut self, request: &accesskit::ActionRequest) -> bool {
        match request.action {
            accesskit::Action::SetTextSelection => {
                // todo: this requires every textbox to have a LayoutAccessibility
                // if let Some(focused) = self.focused {
                //     if let Some(accesskit::ActionData::SetTextSelection(selection)) = &request.data {
                        // if let Some(selection) = Selection::from_access_selection(selection) {
                        //     match focused {
                        //         AnyBox::TextEdit(i) => {
                        //             let handle = TextEditHandle { i };
                        //             let mut text_edit = self.get_text_edit_mut(&handle);
                        //             text_edit.text_box.set_selection(selection);
                        //             self.decorations_changed = true;
                        //             return true;
                        //         }
                        //         AnyBox::TextBox(i) => {
                        //             let handle = TextBoxHandle { i };
                        //             let mut text_box = self.get_text_box_mut(&handle);
                        //             text_box.set_selection(selection);
                        //             self.decorations_changed = true;
                        //             return true;
                        //         }
                        //     }
                        // }
                    // }
                // }
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