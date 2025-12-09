//! Conversation Memory Module
//! 
//! Manages conversation history for AI Chat feature, allowing follow-up questions
//! with context from previous messages.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// A single message in a conversation
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConversationMessage {
    pub role: String,       // "user" or "assistant"
    pub content: String,    // Message content
    pub timestamp: u64,
    pub has_image: bool,    // Whether this message included an image
}

/// A conversation session with image context
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Conversation {
    pub id: String,
    pub messages: Vec<ConversationMessage>,
    pub image_base64: Option<String>, // Base64 of the original captured image
    pub created_at: u64,
    pub updated_at: u64,
}

impl Conversation {
    pub fn new(image_base64: Option<String>) -> Self {
        let now = get_timestamp();
        Self {
            id: generate_conversation_id(),
            messages: Vec::new(),
            image_base64,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_message(&mut self, role: &str, content: &str, has_image: bool) {
        self.messages.push(ConversationMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: get_timestamp(),
            has_image,
        });
        self.updated_at = get_timestamp();
    }

    /// Get messages formatted for API call
    /// Returns Vec of (role, content) tuples
    pub fn get_api_messages(&self) -> Vec<(String, String)> {
        self.messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect()
    }
}

lazy_static::lazy_static! {
    /// Current active conversation
    static ref CURRENT_CONVERSATION: Mutex<Option<Conversation>> = Mutex::new(None);
}

/// Start a new conversation, optionally with an image context
pub fn start_conversation(image_base64: Option<String>) -> String {
    let conversation = Conversation::new(image_base64);
    let id = conversation.id.clone();
    *CURRENT_CONVERSATION.lock().unwrap() = Some(conversation);
    id
}

/// Get the current conversation, if any
pub fn get_current_conversation() -> Option<Conversation> {
    CURRENT_CONVERSATION.lock().unwrap().clone()
}

/// Check if there's an active conversation
pub fn has_active_conversation() -> bool {
    CURRENT_CONVERSATION.lock().unwrap().is_some()
}

/// Add a user message to the current conversation
pub fn add_user_message(content: &str) {
    if let Some(ref mut conv) = *CURRENT_CONVERSATION.lock().unwrap() {
        conv.add_message("user", content, false);
    }
}

/// Add an assistant message to the current conversation
pub fn add_assistant_message(content: &str) {
    if let Some(ref mut conv) = *CURRENT_CONVERSATION.lock().unwrap() {
        conv.add_message("assistant", content, false);
    }
}

/// Get the image context (base64) from current conversation
pub fn get_image_context() -> Option<String> {
    CURRENT_CONVERSATION
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|c| c.image_base64.clone())
}

/// Get conversation history for API calls
pub fn get_context_messages() -> Vec<(String, String)> {
    CURRENT_CONVERSATION
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| c.get_api_messages())
        .unwrap_or_default()
}

/// Get message count in current conversation
pub fn get_message_count() -> usize {
    CURRENT_CONVERSATION
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| c.messages.len())
        .unwrap_or(0)
}

/// Clear the current conversation
pub fn clear_conversation() {
    *CURRENT_CONVERSATION.lock().unwrap() = None;
}

/// Update the image context for the current conversation
pub fn update_image_context(image_base64: String) {
    if let Some(ref mut conv) = *CURRENT_CONVERSATION.lock().unwrap() {
        conv.image_base64 = Some(image_base64);
        conv.updated_at = get_timestamp();
    }
}

// --- Helper functions ---

fn get_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn generate_conversation_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("conv_{:x}", now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_flow() {
        // Start new conversation
        let id = start_conversation(Some("test_base64".to_string()));
        assert!(!id.is_empty());
        assert!(has_active_conversation());

        // Add messages
        add_user_message("What is in this image?");
        add_assistant_message("This image shows a code editor.");
        add_user_message("What language is it?");
        add_assistant_message("It appears to be Rust.");

        // Check message count
        assert_eq!(get_message_count(), 4);

        // Check context
        let messages = get_context_messages();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].0, "user");
        assert_eq!(messages[1].0, "assistant");

        // Check image context
        assert_eq!(get_image_context(), Some("test_base64".to_string()));

        // Clear
        clear_conversation();
        assert!(!has_active_conversation());
    }
}
