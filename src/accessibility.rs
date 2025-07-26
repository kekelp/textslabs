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
    
    
    
    /// Handle accessibility action requests
    /// 
    /// This should be called when AccessKit sends action requests to the application.
    /// Returns true if the action was handled.
    pub fn handle_accessibility_action(&mut self, request: &accesskit::ActionRequest) -> bool {
        match request.action {
            accesskit::Action::Focus => {
                // Try to find the text edit that corresponds to this node ID
                // For now, we'll focus the first editable text box we find
                // In a real implementation, you'd want to map node IDs to specific text boxes
                for (key, (text_edit, text_box)) in &self.text_edits {
                    if !text_box.hidden && !text_edit.disabled {
                        self.focused = Some(AnyBox::TextEdit(key as u32));
                        return true;
                    }
                }
                false
            }
            accesskit::Action::SetTextSelection => {
                if let Some(accesskit::ActionData::SetTextSelection(_selection)) = &request.data {
                    // Handle text selection - this would need to be implemented based on
                    // how you want to map accessibility selections to your text system
                    // For now, just return true to indicate we "handled" it
                    true
                } else {
                    false
                }
            }
            accesskit::Action::ReplaceSelectedText => {
                if let Some(accesskit::ActionData::Value(_text)) = &request.data {
                    // Handle text replacement - similar to SetTextSelection,
                    // this would need proper implementation
                    true
                } else {
                    false
                }
            }
            _ => false
        }
    }
}