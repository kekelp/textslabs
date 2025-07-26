use crate::*;
use accesskit::{Node, NodeId, TreeUpdate, Role, Rect as AccessRect};
use std::sync::atomic::{AtomicU64, Ordering};

/// Generate unique node IDs for accessibility nodes
pub fn next_node_id() -> NodeId {
    static NEXT: AtomicU64 = AtomicU64::new(1000);
    NodeId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Accessibility support for Textslabs
impl Text {
    /// Build accessibility tree for all visible text boxes and text edits.
    /// 
    /// This should be called once per frame to update the accessibility tree.
    /// The root node represents the entire text area, with individual text boxes
    /// and text edits as child nodes.
    /// 
    /// # Arguments
    /// * `update` - AccessKit tree update to populate
    /// * `root_node` - Root accessibility node to configure
    /// * `window_bounds` - Physical bounds of the window for coordinate transformation
    pub fn build_accessibility_tree(
        &mut self, 
        update: &mut TreeUpdate, 
        root_node: &mut Node,
        _window_bounds: AccessRect
    ) {
        root_node.set_role(Role::Group);
        root_node.set_description("Text Area");
        
        let mut child_ids = Vec::new();
        
        // Add accessibility nodes for text boxes
        for (_key, text_box) in &self.text_boxes {
            if text_box.hidden {
                continue;
            }
            
            let node_id = next_node_id();
            child_ids.push(node_id);
            
            let mut node = Node::new(Role::Label);
            self.build_text_box_accessibility(&text_box, &mut node, _window_bounds);
            update.nodes.push((node_id, node));
        }
        
        // Add accessibility nodes for text edits
        for (_key, (text_edit, text_box)) in &self.text_edits {
            if text_box.hidden {
                continue;
            }
            
            let node_id = next_node_id();
            child_ids.push(node_id);
            
            let mut node = Node::new(Role::TextInput);
            self.build_text_edit_accessibility(&text_edit, &text_box, &mut node, _window_bounds);
            update.nodes.push((node_id, node));
        }
        
        root_node.set_children(child_ids);
    }
    
    /// Build accessibility node for a text box
    fn build_text_box_accessibility(
        &self,
        text_box: &TextBoxInner,
        node: &mut Node,
        _window_bounds: AccessRect
    ) {
        // Set the text content
        let text_content = text_box.text.to_string();
        node.set_value(text_content.clone());
        node.set_description(text_content);
        
        // Set bounds relative to window - create a basic rect from position and dimensions
        let bounds = AccessRect::new(
            text_box.left as f64,
            text_box.top as f64,
            text_box.left as f64 + text_box.max_advance as f64,
            text_box.top as f64 + text_box.height as f64
        );
        node.set_bounds(bounds);
        
        // Add role-specific properties
        node.set_role(Role::Label);
    }
    
    /// Build accessibility node for a text edit
    fn build_text_edit_accessibility(
        &self,
        text_edit: &TextEditInner,
        text_box: &TextBoxInner,
        node: &mut Node,
        _window_bounds: AccessRect
    ) {
        // Set the text content
        let text_content = if text_edit.showing_placeholder {
            text_edit.placeholder_text.as_ref().map(|s| s.as_ref()).unwrap_or("")
        } else {
            text_box.text.as_ref()
        };
        
        node.set_value(text_content.to_string());
        
        // Set placeholder as description if showing placeholder
        if text_edit.showing_placeholder {
            if let Some(placeholder) = &text_edit.placeholder_text {
                if !placeholder.is_empty() {
                    node.set_description(placeholder.to_string());
                }
            }
        }
        
        // Set bounds relative to window - create a basic rect from position and dimensions  
        let bounds = AccessRect::new(
            text_box.left as f64,
            text_box.top as f64,
            text_box.left as f64 + text_box.max_advance as f64,
            text_box.top as f64 + text_box.height as f64
        );
        node.set_bounds(bounds);
        
        // Add text input specific properties
        node.set_role(Role::TextInput);
        
        // Set state
        if text_edit.disabled {
            node.set_disabled();
        }
        
        if text_edit.single_line {
            // Single line text input
            node.add_action(accesskit::Action::Focus);
        } else {
            // Multi-line text input
            node.add_action(accesskit::Action::Focus);
        }
        
        // Add common text input actions
        node.add_action(accesskit::Action::SetTextSelection);
        
        if !text_edit.disabled {
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