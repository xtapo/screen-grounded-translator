use crate::config::Config;
use crate::api::translate_image_streaming;
use crate::api::translate_text_streaming;
use crate::model_config::get_model_by_id;
use anyhow::Result;
use image::{ImageBuffer, Rgba};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "user", "assistant", "system"
    pub content: String,
    pub context_image: Option<Vec<u8>>, // PNG bytes if attached
    pub context_text: Option<String>,   // Clipboard text if attached
    pub timestamp: u64,
}

pub struct ConversationHistory {
    pub messages: Vec<Message>,
    pub max_messages: usize,
}

impl Default for ConversationHistory {
    fn default() -> Self {
        Self { messages: Vec::new(), max_messages: 20 }
    }
}

impl ConversationHistory {
    pub fn new(max_messages: usize) -> Self {
        Self { messages: Vec::new(), max_messages }
    }

    pub fn add_user_message(&mut self, content: String, context_image: Option<Vec<u8>>, context_text: Option<String>) {
        self.messages.push(Message {
            role: "user".to_string(),
            content,
            context_image,
            context_text,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        });
    }

    pub fn add_assistant_message(&mut self, content: String) {
        self.messages.push(Message {
            role: "assistant".to_string(),
            content,
            context_image: None,
            context_text: None,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        });
    }
    
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    // Build prompt with history
    pub fn build_prompt(&self, new_input: &str, system_prompt: &str) -> String {
        let mut full_prompt = String::new();
        // Simple history construction for now (appending last N messages)
        // Note: For real chat, we should use the "messages" array in API payload if supported, 
        // but current api.rs functions mostly take a single prompt string or handle messages internally differently.
        // We will construct a text prompt for now, or updating api.rs to support chat history is better.
        // Given api.rs structure, we will format history into the prompt for text models.
        
        full_prompt.push_str(&format!("System: {}\n\n", system_prompt));
        
        for msg in self.messages.iter().rev().take(10).rev() { // Take last 10 messages
            let role_name = if msg.role == "user" { "User" } else { "Assistant" };
            full_prompt.push_str(&format!("{}: {}\n", role_name, msg.content));
            if let Some(ctx_text) = &msg.context_text {
                full_prompt.push_str(&format!("[Context: {}]\n", ctx_text));
            }
        }
        
        full_prompt.push_str(&format!("User: {}\nAssistant:", new_input));
        full_prompt
    }
}

pub fn send_chat_streaming<F>(
    config: &Config,
    history: &ConversationHistory,
    user_input: String,
    image_context: Option<Vec<u8>>,
    clipboard_context: Option<String>,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    // Determine provider from model
    let model_id = &config.assistant.model;
    let model_config = get_model_by_id(model_id).ok_or_else(|| anyhow::anyhow!("Model not found"))?;
    
    // Construct prompt
    let prompt = history.build_prompt(&user_input, &config.assistant.system_prompt);
    
    // If image attached, use vision API
    if let Some(img_bytes) = image_context {
        // Load image from bytes
        let img = image::load_from_memory(&img_bytes)?.to_rgba8();
        
        translate_image_streaming(
            &config.api_key, // Groq
            &config.gemini_api_key,
            &config.openrouter_api_key,
            prompt,
            model_config.full_name,
            model_config.provider,
            img,
            true, // Streaming
            false, // JSON format
            on_chunk
        )
    } else {
        // Text API with generic chat support
        crate::api::chat_streaming_raw(
            &config.api_key,
            &config.gemini_api_key,
            &config.openrouter_api_key,
            prompt, // Use the full history built as prompt
            model_config.full_name,
            model_config.provider,
            on_chunk
        )
    }
}
