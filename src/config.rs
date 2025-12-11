use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Hotkey {
    pub code: u32,
    pub name: String,
    pub modifiers: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub selected_language: String, 
    #[serde(default)]
    pub language_vars: HashMap<String, String>,
    pub model: String,
    pub streaming_enabled: bool,
    pub auto_copy: bool,
    pub hotkeys: Vec<Hotkey>,
    pub retranslate: bool,
    pub retranslate_to: String,
    pub retranslate_model: String,
    pub retranslate_streaming_enabled: bool,
    #[serde(default)]
    pub retranslate_auto_copy: bool,
    pub hide_overlay: bool,
    #[serde(default = "default_preset_type")]
    pub preset_type: String, // "image", "audio", "video", "chat"
    
    // --- Audio Fields ---
    #[serde(default = "default_audio_source")]
    pub audio_source: String, // "mic" or "device"
    #[serde(default)]
    pub hide_recording_ui: bool,
    #[serde(default)]
    pub live_mode: bool, // "Chế độ hội thoại"
    #[serde(default = "default_skip_frames")]
    pub skip_frames: bool, // "Nhảy cóc" - skip old frames in queue
    #[serde(default = "default_capture_interval")]
    pub capture_interval_ms: u64, // Capture interval in milliseconds for Live Mode

    // --- Video Fields ---
    #[serde(default)]
    pub video_capture_method: String, // "region" or "monitor:DeviceName"

    #[serde(default)]
    pub is_upcoming: bool,

    // --- AI Chat Fields ---
    #[serde(default)]
    pub enable_chat_mode: bool, // Allow asking follow-up questions
    #[serde(default)]
    pub show_quick_actions: bool, // Show action menu after selection
}

fn default_preset_type() -> String { "image".to_string() }
fn default_audio_source() -> String { "mic".to_string() }
fn default_skip_frames() -> bool { true } // Enabled by default for faster response
fn default_capture_interval() -> u64 { 200 } // 200ms default capture interval

impl Default for Preset {
    fn default() -> Self {
        Self {
            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
            name: "New Preset".to_string(),
            prompt: "Extract text from this image.".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: HashMap::new(),
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        }
    }
}

/// Configuration for Live Captions integration
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum AudioSource {
    Microphone,
    SystemLoopback,
}

fn default_lc_audio_source() -> AudioSource {
    AudioSource::Microphone
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LiveCaptionsConfig {
    pub enabled: bool,
    pub target_language: String,
    pub translation_model: String,
    pub overlay_sentences: usize,
    pub show_original: bool,
    pub auto_hide_live_captions: bool,
    #[serde(default = "default_lc_audio_source")]
    pub audio_source: AudioSource,
}

impl Default for LiveCaptionsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_language: "Vietnamese".to_string(),
            translation_model: "fast_text".to_string(),
            overlay_sentences: 2,
            show_original: true,
            auto_hide_live_captions: true,
            audio_source: AudioSource::Microphone,
        }
    }
}

// --- Quick Actions Configuration ---

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QuickAction {
    pub id: String,        // "translate", "ocr", "chat", "summarize"
    pub name: String,      // Display name
    pub preset_id: String, // Which preset to trigger
    pub icon: String,      // Icon/emoji identifier
    pub enabled: bool,
    #[serde(default)]
    pub model: String,     // Model to use (empty = use preset's model)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QuickActionsConfig {
    pub enabled: bool,
    pub actions: Vec<QuickAction>,
}

impl Default for QuickActionsConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default, user can enable
            actions: vec![
                QuickAction {
                    id: "translate".to_string(),
                    name: "Translate".to_string(),
                    preset_id: "preset_translate".to_string(),
                    icon: "🌐".to_string(),
                    enabled: true,
                    model: "gemini-flash".to_string(),
                },
                QuickAction {
                    id: "ocr".to_string(),
                    name: "Extract text".to_string(),
                    preset_id: "preset_ocr".to_string(),
                    icon: "📝".to_string(),
                    enabled: true,
                    model: "gemini-flash".to_string(),
                },
                QuickAction {
                    id: "chat".to_string(),
                    name: "Ask AI".to_string(),
                    preset_id: "preset_chat".to_string(),
                    icon: "💬".to_string(),
                    enabled: true,
                    model: "gemini-flash".to_string(),
                },
                QuickAction {
                    id: "summarize".to_string(),
                    name: "Summarize".to_string(),
                    preset_id: "preset_summarize".to_string(),
                    icon: "📋".to_string(),
                    enabled: true,
                    model: "gemini-flash".to_string(),
                },
            ],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub gemini_api_key: String,
    #[serde(default)]
    pub openrouter_api_key: String,
    pub presets: Vec<Preset>,
    pub active_preset_idx: usize, // For UI selection
    pub dark_mode: bool,
    pub ui_language: String,
    #[serde(default)]
    pub live_captions: LiveCaptionsConfig,
    #[serde(default)]
    pub quick_actions: QuickActionsConfig,
}

    impl Default for Config {
    fn default() -> Self {
        let default_lang = "Vietnamese".to_string(); 
        
        // 1. Translation Preset
        let mut trans_lang_vars = HashMap::new();
        trans_lang_vars.insert("language1".to_string(), default_lang.clone());
        
        let trans_preset = Preset {
            id: "preset_translate".to_string(),
            name: "Translate".to_string(),
            prompt: "Extract text from this image and translate it to {language1}. Output ONLY the translation text directly.".to_string(),
            selected_language: default_lang.clone(),
            language_vars: trans_lang_vars.clone(),
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![Hotkey { code: 192, name: "` / ~".to_string(), modifiers: 0 }], // Tilde
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 1.5. Translate+Retranslate Preset
        let mut trans_retrans_lang_vars = HashMap::new();
        trans_retrans_lang_vars.insert("language1".to_string(), "Korean".to_string());

        let trans_retrans_preset = Preset {
            id: "preset_translate_retranslate".to_string(),
            name: "Translate+Retranslate".to_string(),
            prompt: "Extract text from this image and translate it to {language1}. Output ONLY the translation text directly.".to_string(),
            selected_language: "Korean".to_string(),
            language_vars: trans_retrans_lang_vars,
            model: "gemini-flash-lite".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 2. OCR Preset
        let ocr_preset = Preset {
            id: "preset_ocr".to_string(),
            name: "Extract text (OCR)".to_string(),
            prompt: "Extract all text from this image exactly as it appears. Output ONLY the text.".to_string(),
            selected_language: "English".to_string(),
            language_vars: HashMap::new(), // No language tags
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: true, 
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 2.5. Extract text+Retranslate Preset
        let extract_retrans_preset = Preset {
            id: "preset_extract_retranslate".to_string(),
            name: "Extract text+Retranslate".to_string(),
            prompt: "Extract all text from this image exactly as it appears. Output ONLY the text.".to_string(),
            selected_language: "English".to_string(),
            language_vars: HashMap::new(),
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 3. Summarize Preset
        let mut sum_lang_vars = HashMap::new();
        sum_lang_vars.insert("language1".to_string(), default_lang.clone());
        
        let sum_preset = Preset {
            id: "preset_summarize".to_string(),
            name: "Summarize content".to_string(),
            prompt: "Analyze this image and summarize its content in {language1}. Only return the summary text, super concisely.".to_string(),
            selected_language: default_lang.clone(),
            language_vars: sum_lang_vars,
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 4. Description Preset
        let mut desc_lang_vars = HashMap::new();
        desc_lang_vars.insert("language1".to_string(), default_lang.clone());
        
        let desc_preset = Preset {
            id: "preset_desc".to_string(),
            name: "Image description".to_string(),
            prompt: "Describe this image in {language1}.".to_string(),
            selected_language: default_lang.clone(),
            language_vars: desc_lang_vars,
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 5. Transcribe (Audio)
        let audio_preset = Preset {
            id: "preset_transcribe".to_string(),
            name: "Transcribe speech".to_string(),
            prompt: "".to_string(),
            selected_language: default_lang.clone(),
            language_vars: HashMap::new(),
            model: "whisper-fast".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "audio".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 6. Study language Preset
        let study_lang_preset = Preset {
            id: "preset_study_language".to_string(),
            name: "Study language".to_string(),
            prompt: "".to_string(),
            selected_language: default_lang.clone(),
            language_vars: HashMap::new(),
            model: "whisper-fast".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "audio".to_string(),
            audio_source: "device".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 7. Quick foreigner reply
        let transcribe_retrans_preset = Preset {
            id: "preset_transcribe_retranslate".to_string(),
            name: "Quick foreigner reply".to_string(),
            prompt: "".to_string(),
            selected_language: "Korean".to_string(),
            language_vars: HashMap::new(),
            model: "whisper-fast".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: "Korean".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: true,
            hide_overlay: false,
            preset_type: "audio".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 8. Quicker foreigner reply Preset (new 4th audio preset with gemini-audio)
        let mut quicker_reply_lang_vars = HashMap::new();
        quicker_reply_lang_vars.insert("language1".to_string(), "Korean".to_string());

        let quicker_reply_preset = Preset {
            id: "preset_quicker_foreigner_reply".to_string(),
            name: "Quicker foreigner reply".to_string(),
            prompt: "Translate the audio to {language1}. Only output the translated text.".to_string(),
            selected_language: "Korean".to_string(),
            language_vars: quicker_reply_lang_vars,
            model: "gemini-audio".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: true,
            preset_type: "audio".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 9. Ask AI (Chat) Preset - NEW
        let mut chat_lang_vars = HashMap::new();
        chat_lang_vars.insert("language1".to_string(), default_lang.clone());

        let chat_preset = Preset {
            id: "preset_chat".to_string(),
            name: "Ask AI".to_string(),
            prompt: "Analyze this image and answer the user's question in {language1}. Be helpful, accurate and concise.".to_string(),
            selected_language: default_lang.clone(),
            language_vars: chat_lang_vars,
            model: "gemini-flash".to_string(),
            streaming_enabled: true,
            auto_copy: false,
            hotkeys: vec![Hotkey { code: 81, name: "Q".to_string(), modifiers: 0x0002 }], // Ctrl+Q
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "chat".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            enable_chat_mode: true, // Enable chat mode for follow-up questions
            show_quick_actions: false,
        };

        // 10. Video Summarize Placeholder
        let video_placeholder_preset = Preset {
            id: "preset_video_summary_placeholder".to_string(),
            name: "Summarize video (upcoming)".to_string(),
            prompt: "".to_string(),
            selected_language: default_lang.clone(),
            language_vars: HashMap::new(),
            model: "".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "".to_string(),
            retranslate_streaming_enabled: false,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "video".to_string(),
            audio_source: "".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: true, // Mark as upcoming to gray out in sidebar
            live_mode: false,
            skip_frames: true,
            capture_interval_ms: 200,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        // 11. Screenshot Preset
        let screenshot_preset = Preset {
            id: "preset_screenshot".to_string(),
            name: "Screenshot".to_string(),
            prompt: "".to_string(),
            selected_language: "".to_string(),
            language_vars: HashMap::new(),
            model: "".to_string(), // No AI model needed
            streaming_enabled: false,
            auto_copy: true, // Copy to clipboard by default
            hotkeys: vec![Hotkey { code: 83, name: "S".to_string(), modifiers: 0x0002 }], // Ctrl+S
            retranslate: false,
            retranslate_to: "".to_string(),
            retranslate_model: "".to_string(),
            retranslate_streaming_enabled: false,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "screenshot".to_string(),
            audio_source: "".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
            live_mode: false,
            skip_frames: false,
            capture_interval_ms: 200,
            enable_chat_mode: false,
            show_quick_actions: false,
        };

        Self {
            api_key: "".to_string(),
            gemini_api_key: "".to_string(),
            openrouter_api_key: "".to_string(),
            presets: vec![
                trans_preset, trans_retrans_preset, ocr_preset, extract_retrans_preset, 
                sum_preset, desc_preset, chat_preset, audio_preset, study_lang_preset, 
                transcribe_retrans_preset, quicker_reply_preset, screenshot_preset, video_placeholder_preset
            ],
            active_preset_idx: 0,
            dark_mode: true,
            ui_language: "vi".to_string(),
            live_captions: LiveCaptionsConfig::default(),
            quick_actions: QuickActionsConfig::default(),
        }
    }
}

pub fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("xt-screen-translator");
    let _ = std::fs::create_dir_all(&config_dir);
    config_dir.join("config_v2.json") // Changed filename to avoid conflict/migration issues for now
}

pub fn load_config() -> Config {
    let path = get_config_path();
    if path.exists() {
        let data = std::fs::read_to_string(path).unwrap_or_default();
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Config::default()
    }
}

pub fn save_config(config: &Config) {
    let path = get_config_path();
    let data = serde_json::to_string_pretty(config).unwrap();
    let _ = std::fs::write(path, data);
}

lazy_static::lazy_static! {
    static ref ALL_LANGUAGES: Vec<String> = {
        let mut languages = Vec::new();
        for i in 0..10000 {
            if let Some(lang) = isolang::Language::from_usize(i) {
                languages.push(lang.to_name().to_string());
            }
        }
        languages.sort();
        languages.dedup();
        languages
    };
}

/// Get all available languages as a vector of language name strings
pub fn get_all_languages() -> &'static Vec<String> {
    &ALL_LANGUAGES
}
