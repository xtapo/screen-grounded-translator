use tungstenite::{connect, Message};
use tungstenite::stream::MaybeTlsStream;
use url::Url;
use std::sync::{Arc, Mutex};
use std::thread;
use std::sync::mpsc::{Sender, Receiver, channel};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct SetupMessage {
    setup: SetupData,
}

#[derive(Serialize)]
struct SetupData {
    model: String,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction>,
}

#[derive(Serialize)]
struct SystemInstruction {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(rename = "responseModalities")]
    response_modalities: Vec<String>,
}

#[derive(Serialize)]
struct RealtimeInputMessage {
    #[serde(rename = "realtimeInput")]
    realtime_input: RealtimeInputData,
}

#[derive(Serialize)]
struct RealtimeInputData {
    #[serde(rename = "mediaChunks")]
    media_chunks: Vec<MediaChunk>,
}

#[derive(Serialize)]
struct MediaChunk {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

pub struct GeminiLiveClient {
    audio_sender: Sender<Vec<u8>>,
    stop_signal: Arc<Mutex<bool>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl GeminiLiveClient {
    pub fn new(api_key: String, system_instruction_text: Option<String>, on_text_received: Box<dyn Fn(String) + Send + Sync>) -> Result<Self, String> {
        let (audio_sender, audio_receiver): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel();
        let stop_signal = Arc::new(Mutex::new(false));
        let stop_clone = stop_signal.clone();

        let handle = thread::spawn(move || {
            let url_str = format!(
                "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1alpha.GenerativeService.BidiGenerateContent?key={}",
                api_key
            );
            
            // Tungstenite connect takes a string or Uri, NOT a Url struct directly if not implemented.
            // Converting to string is safest.
            
            log::info!("Connecting to Gemini Live API...");
            
            let (mut socket, _) = match connect(&url_str) {
                Ok(s) => s,
                Err(e) => {
                    let err = format!("Failed to connect to Gemini Live: {}", e);
                    log::error!("{}", err);
                    on_text_received(format!("[ERROR] {}", err));
                    return;
                }
            };
            
            log::info!("Connected to Gemini Live API");
            on_text_received("[INFO] Connected to Gemini Live.".to_string());

            // 1. Send Setup Message
            // ... (setup msg creation) ...
            let setup_msg = SetupMessage {
                setup: SetupData {
                    model: "models/gemini-2.0-flash-exp".to_string(),
                    generation_config: GenerationConfig {
                        response_modalities: vec!["TEXT".to_string()],
                    },
                    system_instruction: system_instruction_text.map(|text| SystemInstruction {
                        parts: vec![Part { text }],
                    }),
                },
            };
            
            let setup_json = serde_json::to_string(&setup_msg).unwrap();
            // Message::Text takes Utf8Bytes in newer tungstenite, so use into()
            if let Err(e) = socket.write_message(Message::Text(setup_json.into())) {
                let err = format!("Failed to send setup message: {}", e);
                log::error!("{}", err);
                on_text_received(format!("[ERROR] {}", err));
                return;
            }

            // Set non-blocking based on stream type
            // socket.get_mut() returns &mut Stream
            match socket.get_mut() {
                MaybeTlsStream::Plain(s) => {
                    let _ = s.set_nonblocking(true);
                },
                MaybeTlsStream::Rustls(s) => {
                     // Attempt to set non-blocking on underlying socket if possible
                     // Simplest way is let implicit deref or method handle it if available
                     // But RustlsStream wraps TcpStream. 
                     // We can try:
                     if let Err(e) = s.get_mut().set_nonblocking(true) {
                         log::warn!("Failed to set non-blocking: {}", e);
                     }
                },
                _ => {
                    // Ignore other cases (e.g. NativeTls if enabled, but we used rustls)
                    log::warn!("Unknown stream type, non-blocking might fail");
                }
            }

            loop {
                if *stop_clone.lock().unwrap() {
                    break;
                }

                // 2. Send Audio
                while let Ok(data) = audio_receiver.try_recv() {
                    // Use engine instead of deprecated encode
                    use base64::{Engine as _, engine::general_purpose};
                    let b64_data = general_purpose::STANDARD.encode(&data);
                    
                    let msg = RealtimeInputMessage {
                        realtime_input: RealtimeInputData {
                            media_chunks: vec![MediaChunk {
                                mime_type: "audio/pcm; rate=16000".to_string(),
                                data: b64_data,
                            }],
                        },
                    };
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if let Err(e) = socket.write_message(Message::Text(json.into())) {
                             log::error!("Send error: {}", e);
                             on_text_received(format!("[ERROR] Send error: {}", e));
                             break;
                        }
                    }
                }

                // 3. Read Messages (Non-blocking attempt)
                match socket.read_message() {
                    Ok(msg) => {
                        if let Message::Text(text) = msg {
                            // Text is Utf8Bytes, implements Display/Deref
                            let text_str = text.to_string(); 
                            // Parse JSON
                            if let Ok(v) = serde_json::from_str::<Value>(&text_str) {
                                // Extract text
                                if let Some(parts) = v.get("serverContent")
                                    .and_then(|sc| sc.get("modelTurn"))
                                    .and_then(|mt| mt.get("parts"))
                                    .and_then(|p| p.as_array()) 
                                {
                                    for part in parts {
                                        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                                            on_text_received(t.to_string());
                                        }
                                    }
                                }
                            }
                        } else if let Message::Close(_) = msg {
                            on_text_received("[INFO] Connection closed server-side.".to_string());
                            break;
                        }
                    },
                    Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No message, sleep briefly
                        thread::sleep(std::time::Duration::from_millis(10));
                    },
                    Err(e) => {
                         // Only break on serious errors. ConnectionClosed is one.
                         // But we might get other errors?
                         // Log and break for now.
                         log::error!("WebSocket error: {}", e);
                         on_text_received(format!("[ERROR] WebSocket error: {}", e));
                         break;
                    }
                }
            }
            let _ = socket.close(None);
        });

        Ok(GeminiLiveClient {
            audio_sender,
            stop_signal,
            handle: Some(handle),
        })
    }

    pub fn send_audio(&self, pcm_data: Vec<u8>) {
        let _ = self.audio_sender.send(pcm_data);
    }
}

impl Drop for GeminiLiveClient {
    fn drop(&mut self) {
        *self.stop_signal.lock().unwrap() = true;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
