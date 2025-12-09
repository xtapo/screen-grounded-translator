use anyhow::Result;
use serde::{Deserialize, Serialize};
use image::{ImageBuffer, Rgba};
use base64::{Engine as _, engine::general_purpose};
use std::io::{Cursor, BufRead, BufReader};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc};
use image::GenericImageView;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::config::Preset;

use crate::APP;

#[derive(Serialize, Deserialize)]
struct StreamChunk {
    choices: Vec<Choice>,
}

#[derive(Serialize, Deserialize)]
struct Choice {
    delta: Delta,
}

#[derive(Serialize, Deserialize)]
struct Delta {
    content: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Serialize, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    content: String,
}

lazy_static::lazy_static! {
    static ref UREQ_AGENT: ureq::Agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(30))
        .timeout_write(std::time::Duration::from_secs(30))
        .build();

    pub static ref VISION_STOP_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref VISION_ACTIVE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub fn translate_image_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    openrouter_api_key: &str,
    prompt: String,
    model: String,
    provider: String,
    image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    streaming_enabled: bool,
    use_json_format: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    log::info!("Starting image translation. Provider: {}, Model: {}, Stream: {}", provider, model, streaming_enabled);

    // FIX 6: Resize image if too large to save bandwidth
    let processed_image = if image.width() > 1920 {
        let ratio = 1920.0 / image.width() as f32;
        let new_h = (image.height() as f32 * ratio) as u32;
        image::imageops::resize(&image, 1920, new_h, image::imageops::FilterType::Triangle)
    } else {
        image
    };

    let mut image_data = Vec::new();
    // Use PNG for resized image (JPEG support requires feature flag)
    // Resizing from original size to 1920px width already saves ~75% payload
    processed_image.write_to(&mut Cursor::new(&mut image_data), image::ImageFormat::Png)?;
    let b64_image = general_purpose::STANDARD.encode(&image_data);

    let mut full_content = String::new();

    if provider == "google" {
        // Gemini API
        if gemini_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let method = if streaming_enabled { "streamGenerateContent" } else { "generateContent" };
        let url = if streaming_enabled {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse",
                model, method
            )
        } else {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
                model, method
            )
        };

        let payload = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [
                    { "text": prompt },
                    {
                        "inline_data": {
                            "mime_type": "image/png",
                            "data": b64_image
                        }
                    }
                ]
            }]
        });

        let resp = UREQ_AGENT.post(&url)
            .set("x-goog-api-key", gemini_api_key)
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") || err_str.contains("403") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("{}", err_str)
                }
            })?;

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());

            for line in reader.lines() {
                let line = line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))?;
                if line.starts_with("data: ") {
                    let json_str = &line["data: ".len()..];
                    if json_str.trim() == "[DONE]" { break; }

                    if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                            if let Some(first_candidate) = candidates.first() {
                                if let Some(parts) = first_candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                    if let Some(first_part) = parts.first() {
                                        if let Some(text) = first_part.get("text").and_then(|t| t.as_str()) {
                                            full_content.push_str(text);
                                            on_chunk(text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            let chat_resp: serde_json::Value = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(candidates) = chat_resp.get("candidates").and_then(|c| c.as_array()) {
                if let Some(first_choice) = candidates.first() {
                    if let Some(parts) = first_choice.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                        full_content = parts.iter()
                            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                            .collect::<String>();
                        
                        on_chunk(&full_content);
                    }
                }
            }
        }
    } else if provider == "openrouter" {
        // OpenRouter API
        if openrouter_api_key.trim().is_empty() {
             return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let payload = if streaming_enabled {
            serde_json::json!({
                "model": model,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": prompt },
                            { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64_image) } }
                        ]
                    }
                ],
                "stream": true
            })
        } else {
             serde_json::json!({
                "model": model,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": prompt },
                            { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64_image) } }
                        ]
                    }
                ],
                "stream": false
            })
        };

        let mut resp_result = Err(anyhow::anyhow!("Request not started"));
        for retry in 0..3 {
            let r = UREQ_AGENT.post("https://openrouter.ai/api/v1/chat/completions")
                .set("Authorization", &format!("Bearer {}", openrouter_api_key))
                .set("HTTP-Referer", "https://github.com/nhanh-vo/screen-grounded-translator")
                .set("X-Title", "XT Screen Translator")
                .send_json(payload.clone());

            match r {
                Ok(res) => {
                    resp_result = Ok(res);
                    break;
                }
                Err(ureq::Error::Status(429, _)) => {
                    log::warn!("OpenRouter 429 Rate Limit. Retrying...");
                    if retry < 2 {
                        std::thread::sleep(std::time::Duration::from_secs(2u64.pow(retry + 1)));
                        continue;
                    }
                    resp_result = Err(anyhow::anyhow!("OpenRouter: Rate limit exceeded (429). Please try a different model or wait."));
                }
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("401") {
                         resp_result = Err(anyhow::anyhow!("INVALID_API_KEY"));
                    } else if err_str.contains("402") {
                         resp_result = Err(anyhow::anyhow!("OpenRouter: Insufficient credits or not free."));
                    } else {
                         resp_result = Err(anyhow::anyhow!("OpenRouter API Error: {}", err_str));
                    }
                    break;
                }
            }
        }
        let resp = resp_result?;

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            for line in reader.lines() {
                let line = line?;
                if line.starts_with("data: ") {
                     let data = &line[6..];
                     if data == "[DONE]" { break; }
                     match serde_json::from_str::<StreamChunk>(data) {
                         Ok(chunk) => {
                             if let Some(content) = chunk.choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                                 full_content.push_str(content);
                                 on_chunk(content);
                             }
                         }
                         Err(_) => continue,
                     }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;
             if let Some(choice) = chat_resp.choices.first() {
                 full_content = choice.message.content.clone();
                 on_chunk(&full_content);
             }
        }
    } else {
        // Groq API (default)
        if groq_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let payload = if streaming_enabled {
            serde_json::json!({
                "model": model,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": prompt },
                            { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64_image) } }
                        ]
                    }
                ],
                "temperature": 0.1,
                "max_completion_tokens": 1024,
                "stream": true
            })
        } else {
            let payload_obj = serde_json::json!({
                "model": model,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": prompt },
                            { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64_image) } }
                        ]
                    }
                ],
                "temperature": 0.1,
                "max_completion_tokens": 1024,
                "stream": false
            });
            
            payload_obj
        };

        let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
            .set("Authorization", &format!("Bearer {}", groq_api_key))
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else if err_str.contains("400") {
                    anyhow::anyhow!("Groq API 400: Bad request. Check model availability or API request format.")
                } else {
                    anyhow::anyhow!("{}", err_str)
                }
            })?;

        // --- CAPTURE RATE LIMITS ---
        if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
             let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
             let usage_str = format!("{} / {}", remaining, limit);
             
             if let Ok(mut app) = APP.lock() {
                 app.model_usage_stats.insert(model.clone(), usage_str);
             }
        }
        // ---------------------------

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            for line in reader.lines() {
                let line = line?;

                if line.starts_with("data: ") {
                    let data = &line[6..];

                    if data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            if let Some(content) = chunk.choices.get(0)
                                .and_then(|c| c.delta.content.as_ref()) {
                                full_content.push_str(content);
                                on_chunk(content);
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(choice) = chat_resp.choices.first() {
                let content_str = &choice.message.content;
                
                if use_json_format {
                    if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(content_str) {
                        if let Some(translation) = json_obj.get("translation").and_then(|v| v.as_str()) {
                            full_content = translation.to_string();
                        } else {
                            full_content = content_str.clone();
                        }
                    } else {
                        full_content = content_str.clone();
                    }
                } else {
                    full_content = content_str.clone();
                }
                
                on_chunk(&full_content);
            }
        }
    }

    if full_content.is_empty() {
        return Err(anyhow::anyhow!("No content received from API"));
    }

    Ok(full_content)
}

pub fn translate_text_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    openrouter_api_key: &str,
    text: String,
    target_lang: String,
    model: String,
    provider: String,
    streaming_enabled: bool,
    use_json_format: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    log::info!("Starting text translation. Provider: {}, Model: {}, Target: {}", provider, model, target_lang);
    let mut full_content = String::new();
    let prompt = format!(
        "Translate the following text to {}. Output ONLY the translation. Text:\n\n{}",
        target_lang, text
    );

    if provider == "google" {
        // --- GEMINI TEXT API ---
        if gemini_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let method = if streaming_enabled { "streamGenerateContent" } else { "generateContent" };
        let url = if streaming_enabled {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse",
                model, method
            )
        } else {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
                model, method
            )
        };

        let payload = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }]
        });

        let resp = UREQ_AGENT.post(&url)
            .set("x-goog-api-key", gemini_api_key)
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") || err_str.contains("403") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("Gemini Text API Error: {}", err_str)
                }
            })?;

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            for line in reader.lines() {
                let line = line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))?;
                if line.starts_with("data: ") {
                    let json_str = &line["data: ".len()..];
                    if json_str.trim() == "[DONE]" { break; }

                    if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                            if let Some(first_candidate) = candidates.first() {
                                if let Some(parts) = first_candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                    if let Some(first_part) = parts.first() {
                                        if let Some(text) = first_part.get("text").and_then(|t| t.as_str()) {
                                            full_content.push_str(text);
                                            on_chunk(text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            let chat_resp: serde_json::Value = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(candidates) = chat_resp.get("candidates").and_then(|c| c.as_array()) {
                if let Some(first_choice) = candidates.first() {
                    if let Some(parts) = first_choice.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                        full_content = parts.iter()
                            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                            .collect::<String>();
                        on_chunk(&full_content);
                    }
                }
            }
        }

    } else if provider == "openrouter" {
        // --- OPENROUTER TEXT API ---
        if openrouter_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let payload = if streaming_enabled {
             serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "stream": true
            })
        } else {
             serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "stream": false
            })
        };

        let mut resp_result = Err(anyhow::anyhow!("Request not started"));
        for retry in 0..3 {            
            let r = UREQ_AGENT.post("https://openrouter.ai/api/v1/chat/completions")
                .set("Authorization", &format!("Bearer {}", openrouter_api_key))
                .set("HTTP-Referer", "https://github.com/nhanh-vo/screen-grounded-translator")
                .set("X-Title", "XT Screen Translator")
                .send_json(payload.clone());

            match r {
                Ok(res) => {
                     resp_result = Ok(res);
                     break;
                }
                Err(ureq::Error::Status(429, _)) => {
                     log::warn!("OpenRouter 429 Rate Limit. Retrying...");
                     if retry < 2 {
                         std::thread::sleep(std::time::Duration::from_secs(2u64.pow(retry + 1)));
                         continue;
                     }
                     resp_result = Err(anyhow::anyhow!("OpenRouter: Rate limit exceeded (429). Please try a different model or wait."));
                }
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("401") {
                        resp_result = Err(anyhow::anyhow!("INVALID_API_KEY"));
                    } else {
                        resp_result = Err(anyhow::anyhow!("OpenRouter API Error: {}", err_str));
                    }
                    break;
                }
            }
        }
        let resp = resp_result?;

         if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            for line in reader.lines() {
                let line = line?;
                if line.starts_with("data: ") {
                     let data = &line[6..];
                     if data == "[DONE]" { break; }
                     match serde_json::from_str::<StreamChunk>(data) {
                         Ok(chunk) => {
                             if let Some(content) = chunk.choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                                 full_content.push_str(content);
                                 on_chunk(content);
                             }
                         }
                         Err(_) => continue,
                     }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;
             if let Some(choice) = chat_resp.choices.first() {
                 full_content = choice.message.content.clone();
                 on_chunk(&full_content);
             }
        }

    } else {
        // --- GROQ API (Default) ---
        if groq_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let payload = if streaming_enabled {
            serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "stream": true
            })
        } else {
            let mut payload_obj = serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "stream": false
            });
            
            if use_json_format {
                payload_obj["response_format"] = serde_json::json!({ "type": "json_object" });
            }
            
            payload_obj
        };

        let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
            .set("Authorization", &format!("Bearer {}", groq_api_key))
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("{}", err_str)
                }
            })?;

        // --- CAPTURE RATE LIMITS ---
        if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
             let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
             let usage_str = format!("{} / {}", remaining, limit);
             
             if let Ok(mut app) = APP.lock() {
                 app.model_usage_stats.insert(model.clone(), usage_str);
             }
        }
        // ---------------------------

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            
            for line in reader.lines() {
                let line = line?;
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" { break; }
                    
                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            if let Some(content) = chunk.choices.get(0)
                                .and_then(|c| c.delta.content.as_ref()) {
                                full_content.push_str(content);
                                on_chunk(content);
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(choice) = chat_resp.choices.first() {
                let content_str = &choice.message.content;
                
                if use_json_format {
                    if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(content_str) {
                        if let Some(translation) = json_obj.get("translation").and_then(|v| v.as_str()) {
                            full_content = translation.to_string();
                        } else {
                            full_content = content_str.clone();
                        }
                    } else {
                        full_content = content_str.clone();
                    }
                } else {
                    full_content = content_str.clone();
                }
                
            on_chunk(&full_content);
            }
        }
    }

    Ok(full_content)
}

/// Chat with AI using image context and conversation history
/// This function supports multi-turn conversations for the AI Chat feature
pub fn chat_with_image_context<F>(
    gemini_api_key: &str,
    image_base64: Option<&str>,      // Image context (base64 PNG)
    conversation_history: Vec<(String, String)>, // (role, content) tuples
    user_question: String,
    model: String,
    streaming_enabled: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    log::info!("Starting AI chat. Model: {}, History messages: {}, Has image: {}", 
               model, conversation_history.len(), image_base64.is_some());

    if gemini_api_key.trim().is_empty() {
        return Err(anyhow::anyhow!("NO_API_KEY"));
    }

    let mut full_content = String::new();

    // Build the contents array with conversation history
    let mut contents: Vec<serde_json::Value> = Vec::new();

    // Add image context in the first message if available
    let mut first_message_added = false;
    
    for (role, content) in &conversation_history {
        let role_str = if role == "user" { "user" } else { "model" };
        
        if !first_message_added && role == "user" && image_base64.is_some() {
            // First user message with image
            contents.push(serde_json::json!({
                "role": role_str,
                "parts": [
                    { "text": content },
                    {
                        "inline_data": {
                            "mime_type": "image/png",
                            "data": image_base64.unwrap()
                        }
                    }
                ]
            }));
            first_message_added = true;
        } else {
            contents.push(serde_json::json!({
                "role": role_str,
                "parts": [{ "text": content }]
            }));
        }
    }

    // Add the new user question
    if !first_message_added && image_base64.is_some() {
        // This is the first message and we have an image
        contents.push(serde_json::json!({
            "role": "user",
            "parts": [
                { "text": user_question },
                {
                    "inline_data": {
                        "mime_type": "image/png",
                        "data": image_base64.unwrap()
                    }
                }
            ]
        }));
    } else {
        contents.push(serde_json::json!({
            "role": "user",
            "parts": [{ "text": user_question }]
        }));
    }

    let method = if streaming_enabled { "streamGenerateContent" } else { "generateContent" };
    let url = if streaming_enabled {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse",
            model, method
        )
    } else {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
            model, method
        )
    };

    let payload = serde_json::json!({
        "contents": contents,
        "generationConfig": {
            "temperature": 0.7,
            "maxOutputTokens": 2048
        }
    });

    let resp = UREQ_AGENT.post(&url)
        .set("x-goog-api-key", gemini_api_key)
        .send_json(payload)
        .map_err(|e| {
            let err_str = e.to_string();
            if err_str.contains("401") || err_str.contains("403") {
                anyhow::anyhow!("INVALID_API_KEY")
            } else {
                anyhow::anyhow!("Gemini Chat API Error: {}", err_str)
            }
        })?;

    if streaming_enabled {
        let reader = BufReader::new(resp.into_reader());

        for line in reader.lines() {
            let line = line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))?;
            if line.starts_with("data: ") {
                let json_str = &line["data: ".len()..];
                if json_str.trim() == "[DONE]" { break; }

                if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                        if let Some(first_candidate) = candidates.first() {
                            if let Some(parts) = first_candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                if let Some(first_part) = parts.first() {
                                    if let Some(text) = first_part.get("text").and_then(|t| t.as_str()) {
                                        full_content.push_str(text);
                                        on_chunk(text);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        let chat_resp: serde_json::Value = resp.into_json()
            .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

        if let Some(candidates) = chat_resp.get("candidates").and_then(|c| c.as_array()) {
            if let Some(first_choice) = candidates.first() {
                if let Some(parts) = first_choice.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                    full_content = parts.iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<String>();
                    
                    on_chunk(&full_content);
                }
            }
        }
    }

    if full_content.is_empty() {
        return Err(anyhow::anyhow!("No content received from AI Chat API"));
    }

    Ok(full_content)
}

pub fn transcribe_audio_gemini<F>(
    gemini_api_key: &str,
    prompt: String,
    model: String,
    wav_data: Vec<u8>,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    if gemini_api_key.trim().is_empty() {
        return Err(anyhow::anyhow!("NO_API_KEY"));
    }

    let b64_audio = general_purpose::STANDARD.encode(&wav_data);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
        model
    );

    let payload = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": [
                { "text": prompt },
                {
                    "inline_data": {
                        "mime_type": "audio/wav",
                        "data": b64_audio
                    }
                }
            ]
        }]
    });

    let resp = UREQ_AGENT.post(&url)
        .set("x-goog-api-key", gemini_api_key)
        .send_json(payload)
        .map_err(|e| {
            let err_str = e.to_string();
            if err_str.contains("401") || err_str.contains("403") {
                anyhow::anyhow!("INVALID_API_KEY")
            } else {
                anyhow::anyhow!("Gemini Audio API Error: {}", err_str)
            }
        })?;

    let mut full_content = String::new();
    let reader = BufReader::new(resp.into_reader());

    for line in reader.lines() {
        let line = line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))?;
        if line.starts_with("data: ") {
            let json_str = &line["data: ".len()..];
            if json_str.trim() == "[DONE]" { break; }

            if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                    if let Some(first_candidate) = candidates.first() {
                        if let Some(parts) = first_candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                            if let Some(first_part) = parts.first() {
                                if let Some(text) = first_part.get("text").and_then(|t| t.as_str()) {
                                    full_content.push_str(text);
                                    on_chunk(text);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if full_content.is_empty() {
        return Err(anyhow::anyhow!("No content received from Gemini Audio API"));
    }
    
    Ok(full_content)
}

pub fn record_audio_and_transcribe(
    preset: Preset, 
    stop_signal: Arc<AtomicBool>, 
    pause_signal: Arc<AtomicBool>,
    // FIX: New argument to handle user cancellation
    abort_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND
) {
    log::info!("Starting audio recording. Source: {}", preset.audio_source);
    // FIX 5: Host Selection (WASAPI for loopback, default for mic)
    #[cfg(target_os = "windows")]
    let host = if preset.audio_source == "device" {
        cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or(cpal::default_host())
    } else {
        cpal::default_host()
    };
    #[cfg(not(target_os = "windows"))]
    let host = cpal::default_host();

    // Improved Device Selection logic
    let device = if preset.audio_source == "device" {
        #[cfg(target_os = "windows")]
        {
            // For device audio (loopback), prefer WASAPI default output device
            match host.default_output_device() {
                Some(d) => d,
                None => {
                    eprintln!("Error: No default output device found for loopback.");
                    host.default_input_device().expect("No input device available")
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            host.default_input_device().expect("No input device available")
        }
    } else {
        host.default_input_device().expect("No input device available")
    };

    // Robust Audio Configuration
    let config = if preset.audio_source == "device" {
        // Try output config first for loopback accuracy
        match device.default_output_config() {
            Ok(c) => c,
            Err(_) => {
                 device.default_input_config().expect("Failed to get audio config")
            }
        }
    } else {
        device.default_input_config().expect("Failed to get audio config")
    };

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    // FIX 2: Use channel instead of Mutex for better lock-free audio handling
    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    // Stream Setup
    let err_fn = |err| eprintln!("Audio stream error: {}", err);
    
    let stream_res = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                if !pause_signal.load(Ordering::Relaxed) {
                    // Send chunk as f32 vector. If receiver disconnects, just stop sending.
                    let _ = tx.send(data.to_vec());
                }
            },
            err_fn,
            None
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                if !pause_signal.load(Ordering::Relaxed) {
                    // Convert i16 to f32 immediately to standardize
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let _ = tx.send(f32_data);
                }
            },
            err_fn,
            None
        ),
        _ => {
            eprintln!("Unsupported audio sample format: {:?}", config.sample_format());
             Err(cpal::BuildStreamError::StreamConfigNotSupported)
        },
    };

    if let Err(e) = stream_res {
        eprintln!("Failed to build stream: {}", e);
        unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
        return;
    }
    let stream = stream_res.unwrap();

    stream.play().expect("Failed to start audio stream");

    let mut collected_samples: Vec<f32> = Vec::new();

    // Wait loop with channel draining
    while !stop_signal.load(Ordering::SeqCst) {
        // Drain channel
        while let Ok(chunk) = rx.try_recv() {
            collected_samples.extend(chunk);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Also check if UI died
        if !preset.hide_recording_ui {
             if !unsafe { IsWindow(overlay_hwnd).as_bool() } {
                return; // Aborted via window close
            }
        }
    }

    drop(stream);

    // FIX: Check if we should ABORT instead of submitting
    if abort_signal.load(Ordering::SeqCst) {
        println!("Audio recording aborted by user.");
        unsafe {
            if IsWindow(overlay_hwnd).as_bool() {
                 PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    // Final drain of any remaining samples
    while let Ok(chunk) = rx.try_recv() {
        collected_samples.extend(chunk);
    }

    // Convert f32 samples to i16 for WAV
    let samples: Vec<i16> = collected_samples.iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    
    if samples.is_empty() {
        println!("Warning: Recorded audio buffer is empty.");
        unsafe {
            PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    // OPTIMIZATION: Write directly to in-memory buffer instead of disk
    let mut wav_cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut wav_cursor, spec).expect("Failed to create memory writer");
        for sample in &samples {
            writer.write_sample(*sample).expect("Failed to write sample");
        }
        writer.finalize().expect("Failed to finalize WAV");
    }
    let wav_data = wav_cursor.into_inner();
    
    // Delegate processing to overlay module (handles streaming UI)
    crate::overlay::process::process_audio_post_record(preset, wav_data, overlay_hwnd);
}

pub fn record_audio_continuous(
    preset: crate::config::Preset,
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
    abort_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
) {
    let host = cpal::default_host();
    let device = if preset.audio_source == "device" {
        #[cfg(target_os = "windows")]
        {
            match host.default_output_device() {
                Some(d) => d,
                None => host.default_input_device().expect("No input device available")
            }
        }
        #[cfg(not(target_os = "windows"))]
        host.default_input_device().expect("No input device available")
    } else {
        host.default_input_device().expect("No input device available")
    };

    let config = if preset.audio_source == "device" {
        match device.default_output_config() {
            Ok(c) => c,
            Err(_) => device.default_input_config().expect("Failed to get audio config")
        }
    } else {
        device.default_input_config().expect("Failed to get audio config")
    };

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    
    // Start the persistent result session
    let session = crate::overlay::process::start_live_translation_session(preset.clone(), overlay_hwnd);

    let err_fn = |err| eprintln!("Audio stream error: {}", err);
    let stream_res = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                if !pause_signal.load(Ordering::Relaxed) {
                    let _ = tx.send(data.to_vec());
                }
            },
            err_fn,
            None
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                if !pause_signal.load(Ordering::Relaxed) {
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let _ = tx.send(f32_data);
                }
            },
            err_fn,
            None
        ),
        _ => return,
    };

    if let Err(e) = stream_res {
        eprintln!("Failed to build stream: {}", e);
        return;
    }
    let stream = stream_res.unwrap();
    stream.play().expect("Failed to start audio stream");

    let mut collected_samples: Vec<f32> = Vec::new();
    let chunk_duration_samples = (sample_rate as usize) * 2; // 2 seconds chunks (faster response)

    while !stop_signal.load(Ordering::SeqCst) {
        // Drain incoming audio to buffer
        while let Ok(chunk) = rx.try_recv() {
            collected_samples.extend(chunk);
        }

        // Process full chunks
        while collected_samples.len() >= chunk_duration_samples {
            let chunk: Vec<f32> = collected_samples.drain(0..chunk_duration_samples).collect();
            
            let samples: Vec<i16> = chunk.iter()
                .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect();
            
            let mut wav_cursor = Cursor::new(Vec::new());
            let success = {
                if let Ok(mut writer) = hound::WavWriter::new(&mut wav_cursor, spec) {
                    for sample in samples { let _ = writer.write_sample(sample); }
                    let _ = writer.finalize();
                    true
                } else {
                    false
                }
            };
            if success {
                let _ = session.tx.send(wav_cursor.into_inner());
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
        
        // Abort check
        if abort_signal.load(Ordering::SeqCst) {
            break;
        }
        if !preset.hide_recording_ui && !unsafe { IsWindow(overlay_hwnd).as_bool() } {
            break;
        }
    }

    drop(stream);

    // Process remaining partial chunk if it has meaningful data (> 1 second)
    while let Ok(chunk) = rx.try_recv() {
        collected_samples.extend(chunk);
    }
    if collected_samples.len() > sample_rate as usize {
        let samples: Vec<i16> = collected_samples.iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        let mut wav_cursor = Cursor::new(Vec::new());
        let success = {
            if let Ok(mut writer) = hound::WavWriter::new(&mut wav_cursor, spec) {
                for sample in samples { let _ = writer.write_sample(sample); }
                let _ = writer.finalize();
                true
            } else {
                false
            }
        };
        if success {
            let _ = session.tx.send(wav_cursor.into_inner());
        }
    }

    unsafe {
        if IsWindow(overlay_hwnd).as_bool() {
             PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn capture_screen_continuous(
    preset: crate::config::Preset,
    rect: RECT, // The selection region
    overlay_hwnd: HWND, // The result window (or we create it here? No, session creates it)
    // Actually, session creates result window.
    // The `overlay_hwnd` passed to process_and_close is the SELECtION overlay.
    // We should probably close selection overlay immediately?
    // capture_screen_continuous needs to know where to send images.
) {
    // 1. Setup Session
    // We need a dummy HWND or handle for session?
    // start_live_vision_session takes overlay_hwnd mainly to close it (if it's recording overlay).
    // Here we can pass HWND(0) if we handle closing separately.
    let session = crate::overlay::process::start_live_vision_session(preset.clone(), HWND(0)); 

    // 2. State
    VISION_ACTIVE.store(true, Ordering::SeqCst);
    VISION_STOP_SIGNAL.store(false, Ordering::SeqCst);

    let x_virt = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y_virt = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let crop_x = (rect.left - x_virt).max(0) as u32;
    let crop_y = (rect.top - y_virt).max(0) as u32;
    let crop_w = (rect.right - rect.left).abs() as u32;
    let crop_h = (rect.bottom - rect.top).abs() as u32;

    log::info!("Starting Live Vision Loop. Region: {}x{} at {},{}", crop_w, crop_h, crop_x, crop_y);

    let mut last_processed_image: Option<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>> = None;
    
    // ADAPTIVE POLLING: Start with base interval, speed up on change, slow down when static
    let min_interval = 50u64; // Fastest possible (50ms)
    let max_interval = preset.capture_interval_ms.max(200); // Use user setting as slow interval
    let mut current_interval = preset.capture_interval_ms;
    let mut static_streak = 0u32; // How many consecutive frames were static

    loop {
        if VISION_STOP_SIGNAL.load(Ordering::SeqCst) {
            break;
        }

        // Capture
        if let Ok(img) = crate::capture::capture_full_screen() {
             let img_w = img.width();
             let img_h = img.height();
             let valid_w = crop_w.min(img_w.saturating_sub(crop_x));
             let valid_h = crop_h.min(img_h.saturating_sub(crop_y));

             if valid_w > 0 && valid_h > 0 {
                 let cropped = img.view(crop_x, crop_y, valid_w, valid_h).to_image();
                 
                 // IMAGE DIFF CHECK
                 let is_duplicate = if let Some(last) = &last_processed_image {
                     *last == cropped
                 } else {
                     false
                 };

                 if !is_duplicate {
                     // CHANGE DETECTED! Speed up polling
                     current_interval = min_interval;
                     static_streak = 0;
                     
                     // Store last (original size for comparison)
                     last_processed_image = Some(cropped.clone());
                     
                     // RESIZE for faster API processing
                     let resized = if cropped.width() > 800 {
                         let ratio = 800.0 / cropped.width() as f32;
                         let new_h = (cropped.height() as f32 * ratio) as u32;
                         image::imageops::resize(&cropped, 800, new_h, image::imageops::FilterType::Triangle)
                     } else {
                         cropped
                     };
                     
                     // Send to session
                     let _ = session.tx.send(resized);
                 } else {
                     // NO CHANGE - gradually slow down polling to save resources
                     static_streak += 1;
                     if static_streak > 3 {
                         // Slowly increase interval back to max
                         current_interval = (current_interval + 25).min(max_interval);
                     }
                 }
             }
        }

        // Use adaptive interval
        std::thread::sleep(std::time::Duration::from_millis(current_interval));
    }

    VISION_ACTIVE.store(false, Ordering::SeqCst);
    log::info!("Live Vision Loop Ended");
}

pub fn upload_audio_to_whisper(api_key: &str, model: &str, audio_data: Vec<u8>) -> anyhow::Result<String> {
    // Create multipart form data
    let boundary = format!("----SGTBoundary{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis());
    
    let mut body = Vec::new();
    
    // Add model field
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"model\"\r\n\r\n");
    body.extend_from_slice(model.as_bytes());
    body.extend_from_slice(b"\r\n");
    
    // Add file field
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n");
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(&audio_data);
    body.extend_from_slice(b"\r\n");
    
    // End boundary
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
    
    // Make API request
    let response = UREQ_AGENT.post("https://api.groq.com/openai/v1/audio/transcriptions")
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Content-Type", &format!("multipart/form-data; boundary={}", boundary))
        .send_bytes(&body)
        .map_err(|e| anyhow::anyhow!("API request failed: {}", e))?;
    
    // --- CAPTURE RATE LIMITS ---
    if let Some(remaining) = response.header("x-ratelimit-remaining-requests") {
         let limit = response.header("x-ratelimit-limit-requests").unwrap_or("?");
         let usage_str = format!("{} / {}", remaining, limit);
         if let Ok(mut app) = APP.lock() {
             app.model_usage_stats.insert(model.to_string(), usage_str);
         }
    }
    // ---------------------------

    // Parse response
    let json: serde_json::Value = response.into_json()
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;
    
    let text = json.get("text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("No text in response"))?;
    
    Ok(text.to_string())
}
