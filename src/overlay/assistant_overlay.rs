
use eframe::egui;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use std::sync::{Arc, Mutex};
use crate::config::Config;
use crate::assistant::{ConversationHistory, send_chat_streaming};
use std::sync::atomic::{AtomicBool, Ordering};
use arboard; // Added for clipboard context

pub struct AssistantOverlay {
    pub visible: bool,
    pub history: Arc<Mutex<ConversationHistory>>,
    pub input: String,
    pub response_buffer: String,
    pub is_generating: bool,
    pub config: Config,
    pub window_hwnd: Option<HWND>,
}

impl AssistantOverlay {
    pub fn new(config: Config, history: Arc<Mutex<ConversationHistory>>) -> Self {
        Self {
            visible: false,
            history,
            input: String::new(),
            response_buffer: String::new(),
            is_generating: false,
            config,
            window_hwnd: None,
        }
    }

    pub fn toggle_visibility(&mut self) {
        self.visible = !self.visible;
    }
}

// Component for eframe viewport
pub struct AssistantWindow {
    history: Arc<Mutex<ConversationHistory>>,
    input: String,
    is_generating: bool,
    config: Config,
    scroll_to_bottom: bool,
}

impl AssistantWindow {
    pub fn new(config: Config, history: Arc<Mutex<ConversationHistory>>) -> Self {
        Self {
            history,
            input: String::new(),
            is_generating: false,
            config,
            scroll_to_bottom: true
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        let panel_frame = egui::Frame::window(&ctx.style())
            .fill(if self.config.dark_mode { egui::Color32::from_gray(30) } else { egui::Color32::from_gray(245) })
            .inner_margin(10.0);

        egui::CentralPanel::default().frame(panel_frame).show(ctx, |ui| {
             // 1. Title Bar (Draggable)
             let app_rect = ui.max_rect();
             let title_bar_height = 32.0;
             let title_bar_rect = egui::Rect::from_min_size(app_rect.min, egui::vec2(app_rect.width(), title_bar_height));
             
             // Draw Title background
             ui.painter().rect_filled(title_bar_rect, egui::Rounding::same(8.0), egui::Color32::from_black_alpha(50));
             
             let title_response = ui.interact(title_bar_rect, egui::Id::new("title_bar"), egui::Sense::click_and_drag());
             if title_response.dragged() {
                 ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
             }

             // Title Text
             ui.allocate_ui_at_rect(title_bar_rect, |ui| {
                 ui.horizontal_centered(|ui| {
                     ui.add_space(10.0);
                     ui.heading(egui::RichText::new("AI Assistant").color(egui::Color32::WHITE));
                     ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                         if ui.button("❌").clicked() {
                             ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                         }
                     });
                 });
             });
             
             ui.add_space(title_bar_height + 5.0);

            // 2. Chat Area
            egui::ScrollArea::vertical().stick_to_bottom(true).max_height(ui.available_height() - 50.0).show(ui, |ui| {
                // Scope lock
                let messages = {
                     if let Ok(h) = self.history.lock() {
                         h.messages.clone()
                     } else { Vec::new() }
                };

                for msg in messages {
                    let is_user = msg.role == "user";
                    let align = if is_user { egui::Align::Max } else { egui::Align::Min };
                    ui.with_layout(egui::Layout::top_down(align), |ui| {
                        let bg_color = if is_user { 
                            egui::Color32::from_rgb(0, 120, 215) // Blue
                        } else { 
                            egui::Color32::from_gray(60) 
                        };
                        
                        egui::Frame::none()
                            .fill(bg_color)
                            .rounding(8.0)
                            .inner_margin(8.0)
                            // Limit width
                             .show(ui, |ui| {
                                ui.set_max_width(ui.available_width() * 0.85);
                                if let Some(_img) = &msg.context_image {
                                    ui.label(egui::RichText::new("📷 [Image]").italics().small());
                                }
                                if let Some(txt) = &msg.context_text {
                                     let snippet: String = txt.chars().take(40).collect();
                                     ui.label(egui::RichText::new(format!("📋 {}", snippet)).italics().small());
                                }
                                ui.label(egui::RichText::new(&msg.content).color(egui::Color32::WHITE));
                            });
                    });
                    ui.add_space(4.0);
                }
                
                if self.is_generating {
                    ui.spinner();
                }
            });

            ui.separator();

            // 3. Input Area
            ui.horizontal(|ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut self.input).hint_text("Ask AI...").desired_width(ui.available_width() - 50.0));
                
                let send_clicked = ui.button("Send").clicked();
                
                if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || send_clicked {
                    let input = self.input.trim().to_string();
                    if !input.is_empty() {
                        self.input.clear();
                        self.is_generating = true; // Local UI state

                        let history_arc = self.history.clone();
                        let config_clone = self.config.clone();
                        
                        // Add user message
                        {
                             if let Ok(mut h) = history_arc.lock() {
                                 // Context Logic (Simple for Overlay: Clipboard)
                                 let mut context_text = None;
                                 if config_clone.assistant.auto_include_context {
                                      if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                          if let Ok(text) = clipboard.get_text() {
                                              if !text.is_empty() && text.len() < 5000 {
                                                  context_text = Some(text);
                                              }
                                          }
                                      }
                                 }
                                 h.add_user_message(input.clone(), None, context_text);
                                 h.add_assistant_message(String::new());
                             }
                        }
                        
                        std::thread::spawn(move || {
                            let mut current_resp = String::new();
                            let history_snap = if let Ok(h) = history_arc.lock() { h.messages.clone() } else { Vec::new() };
                            let temp_h = ConversationHistory { messages: history_snap, max_messages: 20 };

                            let _ = send_chat_streaming(&config_clone, &temp_h, input, None, None, |chunk| {
                                current_resp.push_str(chunk);
                                if let Ok(mut h) = history_arc.lock() {
                                    if let Some(last) = h.messages.last_mut() {
                                        if last.role == "assistant" {
                                            last.content = current_resp.clone();
                                        }
                                    }
                                }
                            });
                        });
                        self.is_generating = false; // Reset UI spinner immediately
                    }
                    response.request_focus();
                }
            });
        });
    }
}

impl eframe::App for AssistantWindow {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // Transparent clear color
    }
}

pub fn show_assistant_overlay(history: Arc<Mutex<ConversationHistory>>) {
    log::info!("show_assistant_overlay called. Starting eframe...");
    let config = crate::config::load_config();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_decorations(false) // Frameless
            .with_transparent(true)  // Transparent
            .with_always_on_top() 
            .with_inner_size([400.0, 600.0])
            .with_position([100.0, 100.0]),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "XT Assistant Overlay",
        options,
        Box::new(move |_cc| {
             // Note: can configure fonts here if needed
             Box::new(AssistantWindow::new(config, history))
        }),
    );
}
