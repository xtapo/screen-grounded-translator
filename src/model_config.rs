/// Centralized Model Configuration

#[derive(Clone, Debug, PartialEq)]
pub enum ModelType {
    Vision,
    Text,
    Audio,
}

#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub id: String,
    pub provider: String,
    pub name_vi: String,
    pub name_ko: String,
    pub name_en: String,
    pub full_name: String,
    pub model_type: ModelType,
    pub enabled: bool,
    pub quota_limit: String, 
}

impl ModelConfig {
     pub fn new(
         id: &str,
         provider: &str,
         name_vi: &str,
         name_ko: &str,
         name_en: &str,
         full_name: &str,
         model_type: ModelType,
         enabled: bool,
         quota_limit: &str,
     ) -> Self {
         Self {
             id: id.to_string(),
             provider: provider.to_string(),
             name_vi: name_vi.to_string(),
             name_ko: name_ko.to_string(),
             name_en: name_en.to_string(),
             full_name: full_name.to_string(),
             model_type,
             enabled,
             quota_limit: quota_limit.to_string(),
         }
     }

     #[allow(dead_code)]
     pub fn get_label(&self, ui_language: &str) -> String {
         let name = match ui_language {
             "vi" => &self.name_vi,
             "ko" => &self.name_ko,
             _ => &self.name_en,
         };
         format!("{} ({})", name, self.full_name)
     }

     #[allow(dead_code)]
     pub fn get_name_only(&self, ui_language: &str) -> String {
         match ui_language {
             "vi" => self.name_vi.clone(),
             "ko" => self.name_ko.clone(),
             _ => self.name_en.clone(),
         }
     }
 }

lazy_static::lazy_static! {
    static ref ALL_MODELS: Vec<ModelConfig> = vec![
        ModelConfig::new(
            "scout",
            "groq",
            "Nhanh",
            "빠름",
            "Fast",
            "meta-llama/llama-4-scout-17b-16e-instruct",
            ModelType::Vision,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "maverick",
            "groq",
            "Chính xác",
            "정확함",
            "Accurate",
            "meta-llama/llama-4-maverick-17b-128e-instruct",
            ModelType::Vision,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "gemini-flash-lite",
            "google",
            "Chính xác hơn",
            "더 정확함",
            "More Accurate",
            "gemini-flash-lite-latest",
            ModelType::Vision,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "gemini-flash",
            "google",
            "Rất chính xác",
            "매우 정확함",
            "Very Accurate",
            "gemini-flash-latest",
            ModelType::Vision,
            true,
            "250 lượt/ngày"
        ),
        ModelConfig::new(
            "gemini-2.5-flash",
            "google",
            "Flash mới nhất",
            "Flash (최신)",
            "Flash (Latest)",
            "gemini-1.5-flash-latest", // Currently mapping to 1.5-flash-latest as 2.5 is not out/public yet for Flash
            ModelType::Vision,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "gemini-pro",
            "google",
            "Siêu chính xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-2.5-pro",
            ModelType::Vision,
            true,
            "50 lượt/ngày"
        ),
        ModelConfig::new(
            "fast_text",
            "groq",
            "Cực nhanh",
            "초고속",
            "Super Fast",
            "openai/gpt-oss-20b",
            ModelType::Text,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "text_fast_120b",
            "groq",
            "Nhanh",
            "빠름",
            "Fast",
            "openai/gpt-oss-120b",
            ModelType::Text,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "text_accurate_kimi",
            "groq",
            "Chính xác",
            "정확함",
            "Accurate",
            "moonshotai/kimi-k2-instruct-0905",
            ModelType::Text,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "text_gemini_flash_lite",
            "google",
            "Chính xác hơn",
            "더 정확함",
            "More Accurate",
            "gemini-flash-lite-latest",
            ModelType::Text,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "text_gemini_flash",
            "google",
            "Rất chính xác",
            "매우 정확함",
            "Very Accurate",
            "gemini-flash-latest",
            ModelType::Text,
            true,
            "250 lượt/ngày"
        ),
        ModelConfig::new(
            "text_gemini_pro",
            "google",
            "Siêu chính xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-2.5-pro",
            ModelType::Text,
            true,
            "50 lượt/ngày"
        ),
        ModelConfig::new(
            "whisper-fast",
            "groq",
            "Nhanh",
            "빠름",
            "Fast",
            "whisper-large-v3-turbo",
            ModelType::Audio,
            true,
            "8h audio/ngày"
        ),
        ModelConfig::new(
            "whisper-accurate",
            "groq",
            "Chính xác",
            "정확함",
            "Accurate",
            "whisper-large-v3",
            ModelType::Audio,
            true,
            "8h audio/ngày"
        ),
        ModelConfig::new(
            "gemini-audio",
            "google",
            "Chính xác hơn",
            "더 정확함",
            "More Accurate",
            "gemini-flash-lite-latest",
            ModelType::Audio,
            true,
            "1000 lượt/ngày"
        ),
        ModelConfig::new(
            "gemini-audio-flash",
            "google",
            "Rất chính xác",
            "매우 정확함",
            "Very Accurate",
            "gemini-flash-latest",
            ModelType::Audio,
            true,
            "250 lượt/ngày"
        ),
        ModelConfig::new(
            "gemini-audio-pro",
            "google",
            "Siêu chính xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-2.5-pro",
            ModelType::Audio,
            true,
            "50 lượt/ngày"
        ),
        ModelConfig::new(
            "gemma-3-27b-free",
            "openrouter",
            "Gemma 3 27B (Miễn phí)",
            "Gemma 3 27B (무료)",
            "Gemma 3 27B (Free)",
            "google/gemma-3-27b-it:free",
            ModelType::Text,
            true,
            "Check OpenRouter"
        ),
        ModelConfig::new(
            "gemini-2.0-flash-exp-free",
            "openrouter",
            "Gemini 2.0 Flash Exp (Miễn phí)",
            "Gemini 2.0 Flash Exp (무료)",
            "Gemini 2.0 Flash Exp (Free)",
            "google/gemini-2.0-flash-exp:free",
            ModelType::Vision,
            true,
            "Check OpenRouter"
        ),
        ModelConfig::new(
            "llama-3.2-11b-vision-free",
            "openrouter",
            "Llama 3.2 11B Vision (Miễn phí)",
            "Llama 3.2 11B Vision (무료)",
            "Llama 3.2 11B Vision (Free)",
            "meta-llama/llama-3.2-11b-vision-instruct:free",
            ModelType::Vision,
            true,
            "Check OpenRouter"
        ),
        ModelConfig::new(
            "gemini-exp-1206-free",
            "openrouter",
            "Gemini Exp 1206 (Miễn phí)",
            "Gemini Exp 1206 (무료)",
            "Gemini Exp 1206 (Free)",
            "google/gemini-exp-1206:free",
            ModelType::Vision,
            true,
            "Check OpenRouter"
        ),
        ModelConfig::new(
            "gemini-flash-1.5-8b-free",
            "openrouter",
            "Gemini Flash 1.5 8B (Miễn phí)",
            "Gemini Flash 1.5 8B (무료)",
            "Gemini Flash 1.5 8B (Free)",
            "google/gemini-flash-1.5-8b", 
            ModelType::Text, 
            true,
            "Check OpenRouter"
        ),
         ModelConfig::new(
             "zephyr-7b-free",
             "openrouter",
             "Zephyr 7B (Miễn phí)",
             "Zephyr 7B (무료)",
             "Zephyr 7B (Free)",
             "huggingfaceh4/zephyr-7b-beta:free",
             ModelType::Text,
             true,
             "Check OpenRouter"
         ),
         ModelConfig::new(
             "mistral-7b-free",
             "openrouter",
             "Mistral 7B (Miễn phí)",
             "Mistral 7B (무료)",
             "Mistral 7B (Free)",
             "mistralai/mistral-7b-instruct:free",
             ModelType::Text,
             true,
             "Check OpenRouter"
         ),
    ];
}

pub fn get_all_models() -> &'static [ModelConfig] {
    &ALL_MODELS
}

pub fn get_model_by_id(id: &str) -> Option<ModelConfig> {
    get_all_models().iter().find(|m| m.id == id).cloned()
}
