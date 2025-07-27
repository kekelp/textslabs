use crate::*;
use accesskit::NodeId;

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