use eframe::egui;
use crate::config::{Config, save_config, ISO_LANGUAGES, UiLanguage};
use std::sync::{Arc, Mutex};

// --- Font Configuration for I18n ---
pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 1. Load Segoe UI (Standard Windows font, perfect for English & Vietnamese)
    let viet_font_name = "segoe_ui";
    let viet_font_path = "C:\\Windows\\Fonts\\segoeui.ttf";
    let viet_fallback_path = "C:\\Windows\\Fonts\\arial.ttf";

    let viet_data = std::fs::read(viet_font_path)
        .or_else(|_| std::fs::read(viet_fallback_path));

    // 2. Load Malgun Gothic (Standard Windows font, perfect for Korean)
    let korean_font_name = "malgun_gothic";
    let korean_font_path = "C:\\Windows\\Fonts\\malgun.ttf";
    let korean_data = std::fs::read(korean_font_path);

    // 3. Register Fonts
    if let Ok(data) = viet_data {
        fonts.font_data.insert(
            viet_font_name.to_owned(),
            egui::FontData::from_owned(data),
        );
        
        // PRIORITY 1: Insert Segoe UI at the very top
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            vec.insert(0, viet_font_name.to_owned());
        }
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            vec.insert(0, viet_font_name.to_owned());
        }
    }

    if let Ok(data) = korean_data {
        fonts.font_data.insert(
            korean_font_name.to_owned(),
            egui::FontData::from_owned(data),
        );
        
        // PRIORITY 2: Insert Malgun Gothic AFTER Segoe UI safely
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            // Check existence using the mutable ref 'vec' directly
            let has_viet = vec.contains(&viet_font_name.to_string());
            let idx = if has_viet { 1 } else { 0 };
            vec.insert(idx, korean_font_name.to_owned());
        }
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            let has_viet = vec.contains(&viet_font_name.to_string());
            let idx = if has_viet { 1 } else { 0 };
            vec.insert(idx, korean_font_name.to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

// --- Localization Struct ---
struct LocaleText {
    window_title: &'static str,
    api_section: &'static str,
    api_key_label: &'static str,
    get_key_link: &'static str,
    lang_section: &'static str,
    search_placeholder: &'static str,
    hotkey_section: &'static str,
    hotkey_label: &'static str,
    restart_note: &'static str,
    appearance_section: &'static str,
    dark_mode: &'static str,
    ui_lang_label: &'static str,
    footer_note: &'static str,
}

impl LocaleText {
    fn get(lang: &UiLanguage) -> Self {
        match lang {
            UiLanguage::English => Self {
                window_title: "Screen Translator Settings",
                api_section: "API Configuration",
                api_key_label: "Groq API Key:",
                get_key_link: "Get API Key at console.groq.com",
                lang_section: "Translation Target",
                search_placeholder: "Search language...",
                hotkey_section: "Controls",
                hotkey_label: "Activation Hotkey:",
                restart_note: "Note: Restart app to apply hotkey changes.",
                appearance_section: "Appearance & Language",
                dark_mode: "Dark Mode",
                ui_lang_label: "Interface Language:",
                footer_note: "Minimize this window to keep running in background.",
            },
            UiLanguage::Vietnamese => Self {
                window_title: "Cài Đặt Dịch Màn Hình",
                api_section: "Cấu Hình API",
                api_key_label: "Mã API Groq:",
                get_key_link: "Lấy mã tại console.groq.com",
                lang_section: "Ngôn Ngữ Đích",
                search_placeholder: "Tìm kiếm ngôn ngữ...",
                hotkey_section: "Điều Khiển",
                hotkey_label: "Phím Tắt Kích Hoạt:",
                restart_note: "Lưu ý: Khởi động lại để áp dụng phím tắt mới.",
                appearance_section: "Giao Diện & Ngôn Ngữ",
                dark_mode: "Chế Độ Tối",
                ui_lang_label: "Ngôn Ngữ Hiển Thị:",
                footer_note: "Thu nhỏ cửa sổ này để ứng dụng chạy ngầm.",
            },
            UiLanguage::Korean => Self {
                window_title: "화면 번역기 설정",
                api_section: "API 구성",
                api_key_label: "Groq API 키:",
                get_key_link: "console.groq.com에서 키 발급",
                lang_section: "번역 대상 언어",
                search_placeholder: "언어 검색...",
                hotkey_section: "단축키 설정",
                hotkey_label: "활성화 키:",
                restart_note: "참고: 단축키 변경은 앱을 재시작해야 적용됩니다.",
                appearance_section: "화면 및 언어",
                dark_mode: "다크 모드",
                ui_lang_label: "인터페이스 언어:",
                footer_note: "백그라운드에서 실행하려면 창을 최소화하세요.",
            },
        }
    }
}

pub struct SettingsApp {
    config: Config,
    app_state_ref: Arc<Mutex<crate::AppState>>,
    search_query: String,
}

impl SettingsApp {
    pub fn new(config: Config, app_state: Arc<Mutex<crate::AppState>>) -> Self {
        Self {
            config,
            app_state_ref: app_state,
            search_query: String::new(),
        }
    }

    fn save_and_sync(&mut self) {
        save_config(&self.config);
        let mut state = self.app_state_ref.lock().unwrap();
        state.config = self.config.clone();
        if state.config.hotkey_code != self.config.hotkey_code {
             state.hotkey_updated = true;
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Apply Theme
        if self.config.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        let text = LocaleText::get(&self.config.ui_language);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(text.window_title);
            ui.add_space(10.0);

            // --- Appearance & Interface Language ---
            ui.group(|ui| {
                ui.heading(text.appearance_section);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.config.dark_mode, text.dark_mode).clicked() {
                        self.save_and_sync();
                    }
                    
                    ui.separator();
                    ui.label(text.ui_lang_label);
                    
                    let original_lang = self.config.ui_language.clone();
                    
                    egui::ComboBox::from_id_source("ui_lang")
                        .selected_text(format!("{:?}", self.config.ui_language))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.config.ui_language, UiLanguage::English, "English");
                            ui.selectable_value(&mut self.config.ui_language, UiLanguage::Vietnamese, "Vietnamese");
                            ui.selectable_value(&mut self.config.ui_language, UiLanguage::Korean, "Korean");
                        });
                    
                    if original_lang != self.config.ui_language {
                        self.save_and_sync();
                    }
                });
            });
            
            ui.add_space(10.0);

            // --- API Key Section ---
            ui.group(|ui| {
                ui.heading(text.api_section);
                ui.label(text.api_key_label);
                let response = ui.add(egui::TextEdit::singleline(&mut self.config.api_key).password(true).desired_width(f32::INFINITY));
                if response.changed() {
                    self.save_and_sync();
                }
                
                if ui.link(text.get_key_link).clicked() {
                    let _ = open::that("https://console.groq.com/keys");
                }
            });

            ui.add_space(10.0);

            // --- Language Section (ISO Search) ---
            ui.group(|ui| {
                ui.heading(text.lang_section);
                // Use hint_text instead of tooltip so it doesn't block the list
                ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text(text.search_placeholder));
                
                ui.add_space(5.0);
                
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    let q = self.search_query.to_lowercase();
                    let filtered = ISO_LANGUAGES.iter().filter(|l| l.to_lowercase().contains(&q));

                    for lang in filtered {
                        if ui.radio_value(&mut self.config.target_language, lang.to_string(), *lang).clicked() {
                            self.save_and_sync();
                        }
                    }
                });
                
                ui.label(format!("Current: {}", self.config.target_language));
            });

            ui.add_space(10.0);

            // --- Hotkey Section ---
            ui.group(|ui| {
                ui.heading(text.hotkey_section);
                ui.label(text.hotkey_label);
                
                egui::ComboBox::from_id_source("hotkey_selector")
                    .selected_text(&self.config.hotkey_name)
                    .show_ui(ui, |ui| {
                        let keys = [
                            ("Tilde (~)", 192, "` / ~"),
                            ("F2", 113, "F2"),
                            ("F4", 115, "F4"),
                            ("F6", 117, "F6"),
                            ("F7", 118, "F7"),
                            ("F8", 119, "F8"),
                            ("F9", 120, "F9"),
                            ("F10", 121, "F10"),
                        ];

                        for (label, code, short) in keys {
                            if ui.selectable_label(self.config.hotkey_code == code, label).clicked() {
                                self.config.hotkey_code = code;
                                self.config.hotkey_name = short.to_string();
                                self.save_and_sync();
                            }
                        }
                    });
                 
                 let warn_color = if self.config.dark_mode {
                     egui::Color32::YELLOW
                 } else {
                     egui::Color32::from_rgb(200, 0, 0)
                 };
                 ui.small(egui::RichText::new(text.restart_note).color(warn_color));
            });

            ui.add_space(20.0);
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(text.footer_note).italics().weak());
            });
        });
    }
}