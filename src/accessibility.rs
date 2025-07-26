use crate::*;
use accesskit::{Node, NodeId, Role, Rect as AccessRect};
use std::sync::atomic::{AtomicU64, Ordering};

/// Generate unique node IDs for accessibility nodes
pub fn next_node_id() -> NodeId {
    static NEXT: AtomicU64 = AtomicU64::new(1000);
    NodeId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Accessibility support for Textslabs
impl Text {
    /// Configure accessibility node for a text box using its handle
    pub fn configure_text_box_node(&self, handle: &TextBoxHandle, node: &mut Node) {
        let text_box = self.get_text_box(handle);
        
        let text_content = text_box.text().to_string();
        node.set_value(text_content.clone());
        node.set_description(text_content);
        
        let (left, top) = text_box.pos();
        let bounds = AccessRect::new(
            left,
            top,
            left + text_box.inner.max_advance as f64,
            top + text_box.inner.height as f64
        );
        node.set_bounds(bounds);
        
        node.set_role(Role::Label);
    }

    /// Configure accessibility node for a text edit using its handle
    pub fn configure_text_edit_node(&mut self, handle: &TextEditHandle, node: &mut Node) {
        let text_edit = self.get_text_edit(handle);
        
        let text_content = text_edit.raw_text().to_string();
        node.set_value(text_content.clone());
        
        if text_edit.showing_placeholder() && !text_content.is_empty() {
            node.set_description(text_content);
        }
        
        let (left, top) = text_edit.text_box.pos();
        let bounds = AccessRect::new(
            left,
            top,
            left + text_edit.text_box.inner.max_advance as f64,
            top + text_edit.text_box.inner.height as f64
        );
        node.set_bounds(bounds);
        
        node.set_role(Role::TextInput);

        if text_edit.disabled() {
            node.set_disabled();
        }
        
        node.add_action(accesskit::Action::Focus);
        node.add_action(accesskit::Action::SetTextSelection);
        
        if !text_edit.disabled() {
            node.add_action(accesskit::Action::ReplaceSelectedText);
        }
    }
    
    
    
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
                                let mut text_edit = self.get_text_edit_mut(&handle);
                                text_edit.raw_text_mut().push_str(&text);
                                self.shared.text_changed = true;
                                self.decorations_changed = true;
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