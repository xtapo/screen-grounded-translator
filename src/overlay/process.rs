use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender}; // ADDED
use image::GenericImageView;

use crate::{AppState, api::{translate_image_streaming, translate_text_streaming, transcribe_audio_gemini, upload_audio_to_whisper}};
use super::utils::{copy_to_clipboard, get_error_message};
use super::result::{create_result_window, update_window_text, WindowType, link_windows};

pub fn process_and_close(app: Arc<Mutex<AppState>>, rect: RECT, overlay_hwnd: HWND, preset_idx: usize) {
    // 1. Snapshot and Configuration Retrieval
    let (img, config, preset) = {
        let guard = app.lock().unwrap();
        if preset_idx >= guard.config.presets.len() {
            // Should not happen, but safety check
            unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            return;
        }
        (
            guard.original_screenshot.clone().unwrap(), 
            guard.config.clone(),
            guard.config.presets[preset_idx].clone()
        )
    };

    // Live Mode / Subtitle Mode Check
    if preset.live_mode {
        unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
        crate::api::capture_screen_continuous(preset, rect, overlay_hwnd);
        return;
    }

    let model_id = &preset.model;
    let model_config = crate::model_config::get_model_by_id(model_id);
    let model_config = model_config.expect("Model config not found for preset model");
    let model_name = model_config.full_name.clone();
    let provider = model_config.provider.clone();

    let x_virt = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y_virt = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    
    let crop_x = (rect.left - x_virt).max(0) as u32;
    let crop_y = (rect.top - y_virt).max(0) as u32;
    let crop_w = (rect.right - rect.left).abs() as u32;
    let crop_h = (rect.bottom - rect.top).abs() as u32;
    
    let img_w = img.width();
    let img_h = img.height();
    let crop_w = crop_w.min(img_w.saturating_sub(crop_x));
    let crop_h = crop_h.min(img_h.saturating_sub(crop_y));

    if crop_w > 0 && crop_h > 0 {
        log::info!("Processing region: {}x{} at ({}, {}). Preset: {}", crop_w, crop_h, crop_x, crop_y, preset.name);
        
        let cropped = img.view(crop_x, crop_y, crop_w, crop_h).to_image();
        
        let groq_api_key = config.api_key.clone();
        let gemini_api_key = config.gemini_api_key.clone();
        let openrouter_api_key = config.openrouter_api_key.clone();
        let ui_language = config.ui_language.clone();
        
        // Prepare Prompt - replace all {languageN} with actual languages
        let mut final_prompt = preset.prompt.clone();
        
        // Replace numbered language tags
        for (key, value) in &preset.language_vars {
            let pattern = format!("{{{}}}", key); // e.g., "{language1}"
            final_prompt = final_prompt.replace(&pattern, value);
        }
        
        // Backward compatibility: also replace old {language} tag
        final_prompt = final_prompt.replace("{language}", &preset.selected_language);
        
        // Settings for thread
        let streaming_enabled = preset.streaming_enabled;
        let retranslate_streaming_enabled = preset.retranslate_streaming_enabled;
        let auto_copy = preset.auto_copy;
        let retranslate_auto_copy = preset.retranslate_auto_copy;
        let do_retranslate = preset.retranslate;
        let retranslate_to = preset.retranslate_to.clone();
        let retranslate_model_id = preset.retranslate_model.clone();
        let use_json_format = preset.id == "preset_translate";
        let hide_overlay = preset.hide_overlay;
        
        // For History
        let preset_name_for_history = preset.name.clone();
        let input_summary = format!("Screenshot {}x{}", crop_w, crop_h);
        
        // Check if this is a chat preset - show input popup first
        let is_chat_mode = preset.preset_type == "chat" || preset.enable_chat_mode;
        
        let user_question = if is_chat_mode {
            // Close selection overlay
            unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            
            // Show chat input popup and wait for user input
            match super::chat_input::show_chat_input_popup(rect) {
                Some(question) => question,
                None => {
                    // User cancelled
                    log::info!("Chat input cancelled by user");
                    return;
                }
            }
        } else {
            String::new()
        };
        
        // Spawn UI Thread for Results
        std::thread::spawn(move || {
            // Create Primary Window (Hidden initially)
            let primary_hwnd = create_result_window(rect, WindowType::Primary);
            
            // Worker thread for API calls
            std::thread::spawn(move || {
                let accumulated_vision = Arc::new(Mutex::new(String::new()));
                let acc_vis_clone = accumulated_vision.clone();
                let mut first_chunk_received = false;

                // --- STEP 1: VISION API ---
                // For chat mode, combine system prompt with user question
                let effective_prompt = if is_chat_mode && !user_question.is_empty() {
                    format!("{}\n\nUser question: {}", final_prompt, user_question)
                } else {
                    final_prompt
                };
                
                let vision_res = translate_image_streaming(
                    &groq_api_key, 
                    &gemini_api_key, 
                    &openrouter_api_key,
                    effective_prompt, 
                    model_name, 
                    provider, 
                    cropped, 
                    streaming_enabled, 
                    use_json_format,
                    |chunk| {
                        let mut text = acc_vis_clone.lock().unwrap();
                        text.push_str(chunk);
                        
                        if !first_chunk_received {
                            first_chunk_received = true;
                            unsafe {
                                PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                                if !hide_overlay {
                                    ShowWindow(primary_hwnd, SW_SHOW);
                                }
                            }
                        }
                        if !hide_overlay {
                            // Apply markdown cleaning for chat mode
                            let display_text = if is_chat_mode {
                                super::utils::clean_markdown_for_display(&text)
                            } else {
                                text.to_string()
                            };
                            update_window_text(primary_hwnd, &display_text);
                        }
                    }
                );

                match vision_res {
                    Ok(vision_text) => {
                        // Ensure window is shown if it wasn't already (non-streaming or fast response)
                        if !first_chunk_received {
                             unsafe {
                                PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                                if !hide_overlay {
                                    ShowWindow(primary_hwnd, SW_SHOW);
                                }
                            }
                            if !hide_overlay {
                                // Apply markdown cleaning for chat mode
                                let display_text = if is_chat_mode {
                                    super::utils::clean_markdown_for_display(&vision_text)
                                } else {
                                    vision_text.clone()
                                };
                                update_window_text(primary_hwnd, &display_text);
                            }
                        }

                        // --- STEP 1.5: MAIN AUTO COPY ---
                        if auto_copy && !vision_text.trim().is_empty() {
                            let vt = vision_text.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(100));
                                copy_to_clipboard(&vt, HWND(0));
                            });
                        }
                        
                        // --- STEP 1.6: SAVE TO HISTORY ---
                        if !vision_text.trim().is_empty() {
                            let entry = crate::history::HistoryEntry {
                                id: crate::history::generate_entry_id(),
                                preset_name: preset_name_for_history.clone(),
                                preset_type: "image".to_string(),
                                input_summary: input_summary.clone(),
                                result_text: vision_text.clone(),
                                retrans_text: None, // Will be updated if retranslation happens
                                timestamp: crate::history::get_current_timestamp(),
                                is_favorite: false,
                            };
                            crate::history::add_history_entry(entry);
                        }

                        // --- STEP 2: RETRANSLATE (Optional) ---
                         if do_retranslate && !vision_text.trim().is_empty() {
                             // Create Secondary Window
                             // We need to do this on the UI thread? No, create_result_window handles it?
                             // Actually create_result_window creates a window on the CURRENT thread.
                             // The current thread is this worker thread? 
                             // NO. `create_result_window` creates a window. Windows must be pumped on the thread they are created.
                             // This worker thread DOES NOT pump messages. The PARENT thread (spawned above) pumps messages for `primary_hwnd`.
                             // So `primary_hwnd` was created on the parent thread.
                             // If we want a secondary window, it ALSO needs to be created on the parent thread to share the message loop.
                             // Solution: We cannot easily create the secondary window from THIS worker thread if we want the parent loop to handle it.
                             // However, we can use `PostMessage` to signal the parent thread to create it? 
                             // Or, simplified: Just spawn a NEW thread/loop for the secondary window?
                             // Yes, spawning a new thread for the secondary window is easiest and isolates it.
                             
                             let vision_text_for_retrans = vision_text.clone();
                             let groq_key_for_retrans = groq_api_key.clone();
                             let gemini_key_for_retrans = gemini_api_key.clone();
                             let openrouter_key_for_retrans = openrouter_api_key.clone();
                             
                             // Spawn Secondary UI Thread
                             std::thread::spawn(move || {
                                 let secondary_hwnd = create_result_window(rect, WindowType::Secondary);
                                 super::result::link_windows(primary_hwnd, secondary_hwnd);
                                 if !hide_overlay {
                                     unsafe { ShowWindow(secondary_hwnd, SW_SHOW); }
                                     update_window_text(secondary_hwnd, "");
                                 }

                                 // API Call for Retranslation (Blocking in this UI thread? No, need another worker or just block since it's simple text?)
                                 // Better to block here? If we block, the window won't repaint.
                                 // So spawn a worker for text API too.
                                 
                                 std::thread::spawn(move || {
                                     let acc_text = Arc::new(Mutex::new(String::new()));
                                     let acc_text_clone = acc_text.clone();
                                     
                                     // Resolve text model
                                     let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                                     let (tm_name, tm_provider) = match tm_config {
                                         Some(m) => (m.full_name, m.provider),
                                         None => ("openai/gpt-oss-20b".to_string(), "groq".to_string())
                                     };

                                     let text_res = translate_text_streaming(
                                         &groq_key_for_retrans,
                                         &gemini_key_for_retrans, 
                                         &openrouter_key_for_retrans,
                                         vision_text_for_retrans,
                                         retranslate_to,
                                         tm_name,
                                         tm_provider, // Pass Provider
                                         retranslate_streaming_enabled,
                                         false,
                                         |chunk| {
                                             let mut t = acc_text_clone.lock().unwrap();
                                             t.push_str(chunk);
                                             if !hide_overlay {
                                                 update_window_text(secondary_hwnd, &t);
                                             }
                                         }
                                     );
                                    
                                    if let Ok(final_text) = text_res {
                                        if !hide_overlay {
                                            update_window_text(secondary_hwnd, &final_text);
                                        }
                                        if retranslate_auto_copy {
                                            std::thread::spawn(move || {
                                                std::thread::sleep(std::time::Duration::from_millis(100));
                                                copy_to_clipboard(&final_text, HWND(0));
                                            });
                                        }
                                    } else if let Err(e) = text_res {
                                         if !hide_overlay {
                                            update_window_text(secondary_hwnd, &format!("Error: {}", e));
                                         }
                                    }
                                });

                                // Message Loop for Secondary
                                unsafe {
                                    let mut msg = MSG::default();
                                    while GetMessageW(&mut msg, None, 0, 0).into() {
                                        TranslateMessage(&msg);
                                        DispatchMessageW(&msg);
                                        if !IsWindow(secondary_hwnd).as_bool() { break; }
                                    }
                                }
                            });
                        }
                    }
                    Err(e) => {
                        if !first_chunk_received {
                            unsafe {
                                PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                                ShowWindow(primary_hwnd, SW_SHOW);
                            }
                        }
                        let error_msg = get_error_message(&e.to_string(), &ui_language);
                        update_window_text(primary_hwnd, &error_msg);
                    }
                }
            });

            // Message Loop for Primary
            unsafe {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).into() {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    if !IsWindow(primary_hwnd).as_bool() { break; }
                }
            }
        });

    } else {
        unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
    }
}

pub fn show_audio_result(preset: crate::config::Preset, text: String, rect: RECT, retrans_rect: Option<RECT>) {
    let hide_overlay = preset.hide_overlay;
    let auto_copy = preset.auto_copy;
    let retranslate = preset.retranslate && retrans_rect.is_some();
    let retranslate_to = preset.retranslate_to.clone();
    let retranslate_model_id = preset.retranslate_model.clone();
    let retranslate_streaming_enabled = preset.retranslate_streaming_enabled;
    let retranslate_auto_copy = preset.retranslate_auto_copy;
    let preset_name_for_history = preset.name.clone();
    
    let (groq_key, gemini_key, openrouter_key) = {
        let app = crate::APP.lock().unwrap();
        (app.config.api_key.clone(), app.config.gemini_api_key.clone(), app.config.openrouter_api_key.clone())
    };
    
    std::thread::spawn(move || {
        let primary_hwnd = create_result_window(rect, WindowType::Primary);
        if !hide_overlay {
            unsafe { ShowWindow(primary_hwnd, SW_SHOW); }
            update_window_text(primary_hwnd, &text);
        }
        
        if auto_copy {
            copy_to_clipboard(&text, HWND(0));
        }
        
        // Save to history
        if !text.trim().is_empty() {
            let entry = crate::history::HistoryEntry {
                id: crate::history::generate_entry_id(),
                preset_name: preset_name_for_history.clone(),
                preset_type: "audio".to_string(),
                input_summary: "Audio recording".to_string(),
                result_text: text.clone(),
                retrans_text: None,
                timestamp: crate::history::get_current_timestamp(),
                is_favorite: false,
            };
            crate::history::add_history_entry(entry);

            if retranslate {
                let text_for_retrans = text.clone();
                let groq_key_r = groq_key.clone();
                let gemini_key_r = gemini_key.clone();
                let openrouter_key_r = openrouter_key.clone();
                let rect_r = retrans_rect.unwrap();
                
                std::thread::spawn(move || {
                    let secondary_hwnd = create_result_window(rect_r, WindowType::SecondaryExplicit);
                    link_windows(primary_hwnd, secondary_hwnd);
                    if !hide_overlay {
                        unsafe { ShowWindow(secondary_hwnd, SW_SHOW); }
                        update_window_text(secondary_hwnd, "...");
                    }

                    // Worker for Retranslation API
                    std::thread::spawn(move || {
                        let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                        let (tm_name, tm_provider) = match tm_config {
                            Some(m) => (m.full_name, m.provider),
                            None => ("openai/gpt-oss-20b".to_string(), "groq".to_string())
                        };

                        let accumulated = Arc::new(Mutex::new(String::new()));
                        let acc_clone = accumulated.clone();

                        let _ = translate_text_streaming(
                            &groq_key_r,
                            &gemini_key_r,
                            &openrouter_key_r,
                            text_for_retrans,
                            retranslate_to,
                            tm_name,
                            tm_provider,
                            retranslate_streaming_enabled,
                            false,
                            |chunk| {
                                let mut t = acc_clone.lock().unwrap();
                                t.push_str(chunk);
                                if !hide_overlay {
                                    update_window_text(secondary_hwnd, &t);
                                }
                            }
                        );
                        
                        let final_text = accumulated.lock().unwrap().clone();
                        if !hide_overlay {
                            update_window_text(secondary_hwnd, &final_text);
                        }
                        if retranslate_auto_copy {
                             std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(100));
                                copy_to_clipboard(&final_text, HWND(0));
                            });
                        }
                    });

                    // Message Loop for Secondary
                    unsafe {
                        let mut msg = MSG::default();
                        while GetMessageW(&mut msg, None, 0, 0).into() {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                            if !IsWindow(secondary_hwnd).as_bool() { break; }
                        }
                    }
                });
            }
        }
        
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if !IsWindow(primary_hwnd).as_bool() { break; }
            }
        }
    });
}

pub fn process_audio_post_record(
    preset: crate::config::Preset,
    wav_data: Vec<u8>,
    overlay_hwnd: HWND,
) {
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };

    // Determine window positions (Main + Retranslate)
    let (rect, retranslate_rect) = if preset.retranslate {
        let w = 600;
        let h = 300;
        let gap = 20;
        let total_w = w * 2 + gap;
        let start_x = (screen_w - total_w) / 2;
        let y = (screen_h - h) / 2;
        
        (
            RECT { left: start_x, top: y, right: start_x + w, bottom: y + h },
            Some(RECT { left: start_x + w + gap, top: y, right: start_x + w + gap + w, bottom: y + h })
        )
    } else {
        let w = 700;
        let h = 300;
        let x = (screen_w - w) / 2;
        let y = (screen_h - h) / 2;
        (RECT { left: x, top: y, right: x + w, bottom: y + h }, None)
    };

    let model_config = crate::model_config::get_model_by_id(&preset.model).expect("Model not found");
    let model_name = model_config.full_name;
    let provider = model_config.provider;

    let (groq_api_key, gemini_api_key, openrouter_api_key, ui_language) = {
        let app = crate::APP.lock().unwrap();
        (app.config.api_key.clone(), app.config.gemini_api_key.clone(), app.config.openrouter_api_key.clone(), app.config.ui_language.clone())
    };

    let mut final_prompt = preset.prompt.clone();
    for (key, value) in &preset.language_vars {
        let pattern = format!("{{{}}}", key);
        final_prompt = final_prompt.replace(&pattern, value);
    }
    final_prompt = final_prompt.replace("{language}", &preset.selected_language);

    let streaming_enabled = preset.streaming_enabled;
    let hide_overlay = preset.hide_overlay;
    let auto_copy = preset.auto_copy;
    
    // Retranslate settings
    let retranslate = preset.retranslate && retranslate_rect.is_some();
    let retranslate_streaming_enabled = preset.retranslate_streaming_enabled;
    let retranslate_auto_copy = preset.retranslate_auto_copy;
    let retranslate_to = preset.retranslate_to.clone();
    let retranslate_model_id = preset.retranslate_model.clone();

    // History
    let preset_name = preset.name.clone();

    // --- Spawn UI Thread ---
    std::thread::spawn(move || {
        let primary_hwnd = create_result_window(rect, WindowType::Primary);
        
        let secondary_hwnd = if retranslate {
            if let Some(r) = retranslate_rect {
                let hwnd = create_result_window(r, WindowType::SecondaryExplicit);
                link_windows(primary_hwnd, hwnd);
                Some(hwnd)
            } else { None }
        } else { None };

        // Indicate processing start
        if !hide_overlay {
            unsafe { 
                // Close the recording overlay first
                if IsWindow(overlay_hwnd).as_bool() {
                    PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); 
                }
                ShowWindow(primary_hwnd, SW_SHOW); 
                if let Some(sec) = secondary_hwnd {
                    ShowWindow(sec, SW_SHOW);
                }
            }
            update_window_text(primary_hwnd, "Processing...");
            if let Some(sec) = secondary_hwnd {
                 update_window_text(sec, "...");
            }
        } else {
             unsafe { 
                if IsWindow(overlay_hwnd).as_bool() {
                    PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); 
                }
             }
        }

        // --- Spawn Worker Thread for API ---
        std::thread::spawn(move || {
            let accumulated_text = Arc::new(Mutex::new(String::new()));
            let acc_text_clone = accumulated_text.clone();
            
            // Logic Split: Gemini (Streaming) vs Whisper (Batch)
            let res: anyhow::Result<String> = if provider == "google" {
                 if gemini_api_key.trim().is_empty() {
                    Err(anyhow::anyhow!("NO_API_KEY"))
                } else {
                    transcribe_audio_gemini(
                        &gemini_api_key,
                        final_prompt,
                        model_name,
                        wav_data,
                        |chunk| {
                            let mut t = acc_text_clone.lock().unwrap();
                            if t.is_empty() {
                                // Clear "Processing..." on first chunk
                                if !hide_overlay { update_window_text(primary_hwnd, ""); }
                            }
                            t.push_str(chunk);
                            if streaming_enabled && !hide_overlay {
                                update_window_text(primary_hwnd, &t);
                            }
                        }
                    )
                }
            } else {
                 // GROQ / WHISPER
                 if groq_api_key.trim().is_empty() {
                    Err(anyhow::anyhow!("NO_API_KEY"))
                } else {
                    let r = upload_audio_to_whisper(&groq_api_key, &model_name, wav_data);
                    r
                }
            };

            match res {
                Ok(full_text) => {
                    let mut t = acc_text_clone.lock().unwrap();
                    *t = full_text.clone(); 
                    if !hide_overlay {
                        update_window_text(primary_hwnd, &full_text);
                    }
                    
                    if auto_copy {
                         copy_to_clipboard(&full_text, HWND(0));
                    }
                    
                    // History
                    if !full_text.trim().is_empty() {
                        let entry = crate::history::HistoryEntry {
                            id: crate::history::generate_entry_id(),
                            preset_name: preset_name.clone(),
                            preset_type: "audio".to_string(),
                            input_summary: "Audio recording".to_string(),
                            result_text: full_text.clone(),
                            retrans_text: None,
                            timestamp: crate::history::get_current_timestamp(),
                            is_favorite: false,
                        };
                        crate::history::add_history_entry(entry);
                    }
                    
                    // Retranslate API
                    if let Some(sec_hwnd) = secondary_hwnd {
                        std::thread::spawn(move || {
                             let acc_retrans = Arc::new(Mutex::new(String::new()));
                             let acc_retrans_clone = acc_retrans.clone();
                             
                            let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                            let (tm_name, tm_provider) = match tm_config {
                                Some(m) => (m.full_name, m.provider),
                                None => ("openai/gpt-oss-20b".to_string(), "groq".to_string())
                            };

                            let text_res = translate_text_streaming(
                                &groq_api_key,
                                &gemini_api_key,
                                &openrouter_api_key,
                                full_text.clone(),
                                retranslate_to,
                                tm_name,
                                tm_provider,
                                retranslate_streaming_enabled,
                                false,
                                |chunk| {
                                    let mut t = acc_retrans_clone.lock().unwrap();
                                    t.push_str(chunk);
                                    if !hide_overlay {
                                        update_window_text(sec_hwnd, &t);
                                    }
                                }
                            );
                            
                            let final_retrans = acc_retrans_clone.lock().unwrap().clone();
                            if !hide_overlay {
                                 update_window_text(sec_hwnd, &final_retrans);
                            }
                            if retranslate_auto_copy {
                                 std::thread::spawn(move || {
                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                    copy_to_clipboard(&final_retrans, HWND(0));
                                });
                            }
                         });
                         
                        // Secondary Window Message Loop
                        unsafe {
                            let mut msg = MSG::default();
                            while GetMessageW(&mut msg, None, 0, 0).into() {
                                TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                                if !IsWindow(sec_hwnd).as_bool() { break; }
                            }
                        }
                    }
                }
                Err(e) => {
                     let error_msg = get_error_message(&e.to_string(), &ui_language);
                     if !hide_overlay { update_window_text(primary_hwnd, &error_msg); }
                }
            }
        });

        unsafe {
            let mut msg = MSG::default();
             while GetMessageW(&mut msg, None, 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if !IsWindow(primary_hwnd).as_bool() { break; }
            }
        }
    });
}

pub struct LiveSession {
    pub tx: Sender<Vec<u8>>,
}

pub fn start_live_translation_session(
    preset: crate::config::Preset,
    overlay_hwnd: HWND,
) -> LiveSession {
    let (tx, rx) = channel::<Vec<u8>>();

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };

    // Determine window positions
    let (rect, retranslate_rect) = if preset.retranslate {
        let w = 600;
        let h = 300;
        let gap = 20;
        let total_w = w * 2 + gap;
        let start_x = (screen_w - total_w) / 2;
        let y = (screen_h - h) / 2;
        
        (
            RECT { left: start_x, top: y, right: start_x + w, bottom: y + h },
            Some(RECT { left: start_x + w + gap, top: y, right: start_x + w + gap + w, bottom: y + h })
        )
    } else {
        let w = 700;
        let h = 300;
        let x = (screen_w - w) / 2;
        let y = (screen_h - h) / 2;
        (RECT { left: x, top: y, right: x + w, bottom: y + h }, None)
    };

    let model_config = crate::model_config::get_model_by_id(&preset.model).expect("Model not found");
    let model_name = model_config.full_name;
    let provider = model_config.provider;

    let (groq_api_key, gemini_api_key, openrouter_api_key, ui_language) = {
        let app = crate::APP.lock().unwrap();
        (app.config.api_key.clone(), app.config.gemini_api_key.clone(), app.config.openrouter_api_key.clone(), app.config.ui_language.clone())
    };

    let mut final_prompt = preset.prompt.clone();
    for (key, value) in &preset.language_vars {
        let pattern = format!("{{{}}}", key);
        final_prompt = final_prompt.replace(&pattern, value);
    }
    final_prompt = final_prompt.replace("{language}", &preset.selected_language);

    let streaming_enabled = preset.streaming_enabled;
    let hide_overlay = preset.hide_overlay;
    let retranslate = preset.retranslate && retranslate_rect.is_some();
    let retranslate_streaming_enabled = preset.retranslate_streaming_enabled;
    let retranslate_to = preset.retranslate_to.clone();
    let skip_frames = preset.skip_frames; // Frame skipping (queue drain) setting
    let retranslate_model_id = preset.retranslate_model.clone();

    // Spawn Window Thread
    std::thread::spawn(move || {
        let primary_hwnd = create_result_window(rect, WindowType::Primary);
        
        // In Live Mode, we DO NOT close the recording overlay, because it contains the Stop button!
        // The recording overlay will close itself when the recording loop finishes.

        if !hide_overlay {
            unsafe { ShowWindow(primary_hwnd, SW_SHOW); }
            update_window_text(primary_hwnd, "Đang khởi tạo hội thoại...");
        }

        let secondary_hwnd = if retranslate {
            let rect_sec = retranslate_rect.unwrap();
            let sec_hwnd = create_result_window(rect_sec, WindowType::SecondaryExplicit);
            link_windows(primary_hwnd, sec_hwnd);
            if !hide_overlay {
                unsafe { ShowWindow(sec_hwnd, SW_SHOW); }
                update_window_text(sec_hwnd, "...");
            }
            Some(sec_hwnd)
        } else {
            None
        };

        // Spawn Processor Thread
        std::thread::spawn(move || {
            let full_transcript = Arc::new(Mutex::new(String::new()));
            let full_translation = Arc::new(Mutex::new(String::new()));
            
            // Loop for chunks
            while let Ok(mut wav_data) = rx.recv() {
                // LATENCY OPTIMIZATION: Drain queue to get the LATEST audio chunk (if skip_frames is enabled)
                // Skip old audio chunks to stay in sync with real-time
                if skip_frames {
                    let mut skipped_count = 0;
                    while let Ok(next_chunk) = rx.try_recv() {
                        wav_data = next_chunk;
                        skipped_count += 1;
                    }
                    if skipped_count > 0 {
                        log::info!("Live Audio: Skipped {} old chunk(s) to stay in sync", skipped_count);
                    }
                }

                // 1. Transcribe
                log::info!("Live Audio: Processing chunk ({} bytes)", wav_data.len());
                let res: anyhow::Result<String> = if provider == "google" {
                    if gemini_api_key.trim().is_empty() { Err(anyhow::anyhow!("NO_API_KEY")) }
                    else {
                        transcribe_audio_gemini(
                            &gemini_api_key,
                            final_prompt.clone(),
                            model_name.clone(),
                            wav_data,
                            |_chunk| { 
                                // Intermediate stream update? 
                                // Hard with accumulation. Maybe just wait for final per chunk?
                                // Let's simplify: Wait for full chunk result before updating main text
                            }
                        )
                    }
                } else {
                    if groq_api_key.trim().is_empty() { Err(anyhow::anyhow!("NO_API_KEY")) }
                    else {
                        upload_audio_to_whisper(&groq_api_key, &model_name, wav_data)
                    }
                };

                match &res {
                    Ok(text) => log::info!("Live Audio: Transcription SUCCESS ({} chars)", text.len()),
                    Err(e) => log::error!("Live Audio: Transcription FAILED - {}", e),
                }

                if let Ok(text) = res {
                    if !text.trim().is_empty() {
                        let mut full = full_transcript.lock().unwrap();
                        
                        // LIMIT TEXT BUFFER to ~1000 chars (approx. 10-15 sentences)
                        // If buffer is too long, truncate the beginning
                        if full.len() > 1000 {
                            // Find the first space after the cut point to keep words intact
                            if let Some(cut_idx) = full.char_indices().skip(200).find(|(_, c)| c.is_whitespace()).map(|(i, _)| i) {
                                *full = full[cut_idx+1..].to_string();
                            } else {
                                // Fallback if no space found (unlikely)
                                *full = full.chars().skip(200).collect();
                            }
                        }

                        if !full.is_empty() { full.push(' '); }
                        full.push_str(&text);
                        let current_full = full.clone();
                        
                        // Update Primary
                        if !hide_overlay {
                            update_window_text(primary_hwnd, &current_full);
                        }

                        // 2. Retranslate (Chunk-based)
                        if let Some(sec_hwnd) = secondary_hwnd {
                            let text_to_trans = text.clone();
                            
                            let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                            let (tm_name, tm_provider) = match tm_config {
                                Some(m) => (m.full_name, m.provider.clone()),
                                None => ("openai/gpt-oss-20b".to_string(), "groq".to_string())
                            };
                            
                            // Streaming retranslation for this chunk
                            // We need to append to the existing translation
                            let _ = translate_text_streaming(
                                &groq_api_key,
                                &gemini_api_key,
                                &openrouter_api_key,
                                text_to_trans,
                                retranslate_to.clone(),
                                tm_name,
                                tm_provider,
                                retranslate_streaming_enabled, // Use streaming?
                                false,
                                |chunk| {
                                    // Intermediate chunk updates? Hard to sync with "append".
                                    // Just collect full translation
                                }
                            ).map(|trans_text| {
                                let mut full_trans = full_translation.lock().unwrap();
                                
                                // Limit Translation Buffer as well
                                if full_trans.len() > 1000 {
                                     if let Some(cut_idx) = full_trans.char_indices().skip(200).find(|(_, c)| c.is_whitespace()).map(|(i, _)| i) {
                                        *full_trans = full_trans[cut_idx+1..].to_string();
                                     } else {
                                        *full_trans = full_trans.chars().skip(200).collect();
                                     }
                                }

                                if !full_trans.is_empty() { full_trans.push(' '); }
                                full_trans.push_str(&trans_text);
                                
                                if !hide_overlay {
                                    update_window_text(sec_hwnd, &full_trans);
                                }
                            });
                        }
                    }
                }
            }
        });

        // Message Loop
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if !IsWindow(primary_hwnd).as_bool() { break; }
            }
        }
    });

    LiveSession { tx }
}

pub struct LiveVisionSession {
    pub tx: Sender<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>>,
}

pub fn start_live_vision_session(
    preset: crate::config::Preset,
    overlay_hwnd: HWND,
) -> LiveVisionSession {
    let (tx, rx) = channel::<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>>();

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };

    // Determine window positions
    let (rect, retranslate_rect) = if preset.retranslate {
        let w = 600;
        let h = 300;
        let gap = 20;
        let total_w = w * 2 + gap;
        let start_x = (screen_w - total_w) / 2;
        let y = (screen_h - h) / 2;
        
        (
            RECT { left: start_x, top: y, right: start_x + w, bottom: y + h },
            Some(RECT { left: start_x + w + gap, top: y, right: start_x + w + gap + w, bottom: y + h })
        )
    } else {
        let w = 700;
        let h = 300;
        let x = (screen_w - w) / 2;
        let y = (screen_h - h) / 2;
        (RECT { left: x, top: y, right: x + w, bottom: y + h }, None)
    };

    let model_config = crate::model_config::get_model_by_id(&preset.model).expect("Model not found");
    let model_name = model_config.full_name;
    let provider = model_config.provider;

    let (groq_api_key, gemini_api_key, openrouter_api_key, ui_language) = {
        let app = crate::APP.lock().unwrap();
        (app.config.api_key.clone(), app.config.gemini_api_key.clone(), app.config.openrouter_api_key.clone(), app.config.ui_language.clone())
    };

    let mut final_prompt = preset.prompt.clone();
    for (key, value) in &preset.language_vars {
        let pattern = format!("{{{}}}", key);
        final_prompt = final_prompt.replace(&pattern, value);
    }
    final_prompt = final_prompt.replace("{language}", &preset.selected_language);
    // STRICT INSTRUCTION for Live Mode
    final_prompt.push_str("\n\nIf the image does not contain any text, output EXACTLY '[NO_TEXT]' and nothing else.");

    let streaming_enabled = preset.streaming_enabled;
    let hide_overlay = preset.hide_overlay;
    let _retranslate = preset.retranslate && retranslate_rect.is_some(); // retranslate flag
    let retranslate_streaming_enabled = preset.retranslate_streaming_enabled;
    let retranslate_to = preset.retranslate_to.clone();
    let skip_frames = preset.skip_frames; // Frame skipping (queue drain) setting
    let retranslate_model_id = preset.retranslate_model.clone();

    // Spawn Window Thread
    std::thread::spawn(move || {
        let primary_hwnd = create_result_window(rect, WindowType::Primary);
        
        // In Live Mode (Vision), we keep the overlay (if it's the selection overlay, strictly speaking it closes after selection?)
        // Actually, for Vision, the overlay provided is likely the SELECTION overlay which closes after selection.
        // But we want to indicate "Live Mode Active". 
        // We will manage the capture loop externally. 
        // Here we just manage the result window.

        if !hide_overlay {
            unsafe { ShowWindow(primary_hwnd, SW_SHOW); }
            update_window_text(primary_hwnd, "Đang khởi tạo chế độ Live Subtitle...");
        }

        let secondary_hwnd = if preset.retranslate && retranslate_rect.is_some() {
            let rect_sec = retranslate_rect.unwrap();
            let sec_hwnd = create_result_window(rect_sec, WindowType::SecondaryExplicit);
            link_windows(primary_hwnd, sec_hwnd);
            if !hide_overlay {
                unsafe { ShowWindow(sec_hwnd, SW_SHOW); }
                update_window_text(sec_hwnd, "...");
            }
            Some(sec_hwnd)
        } else {
            None
        };

        // Spawn Processor Thread
        std::thread::spawn(move || {
            let full_transcript = Arc::new(Mutex::new(String::new()));
            let full_translation = Arc::new(Mutex::new(String::new()));
            
            let mut last_processed_text = String::new();

            // Loop for images
            while let Ok(mut img) = rx.recv() {
                // LATENCY OPTIMIZATION: Drain the channel to get the LATEST image (if skip_frames is enabled).
                // If processing took 1s, and capture is 0.2s, we have 4-5 images queued.
                // We should skip them and only process the newest one.
                if skip_frames {
                    while let Ok(next_img) = rx.try_recv() {
                        img = next_img;
                    }
                }

                // 1. Vision Translation
                let res: anyhow::Result<String> = translate_image_streaming(
                    &groq_api_key,
                    &gemini_api_key,
                    &openrouter_api_key,
                    final_prompt.clone(),
                    model_name.clone(),
                    provider.clone(),
                    img,
                    streaming_enabled, 
                    false, // json format? assume no for general
                    |chunk| { 
                        // Intermediate logging?
                    }
                );

                if let Ok(text) = res {
                    let text_clean = text.trim();
                    if !text_clean.is_empty() {
                        // FILTER: Ignore "No Text" messages from AI
                        let lower = text_clean.to_lowercase();
                        if lower.contains("no text") 
                            || lower.contains("cannot see") 
                            || lower.contains("cannot read")
                            || lower.contains("doesn't contain text")
                            || lower.contains("image does not contain")
                            || lower.contains("[no_text]") { // Check for the strict token
                            continue;
                        }

                        // FILTER: Deduplicate (Historical Check)
                        // Normalize: Lowercase + Alphanumeric only
                        let normalize = |s: &str| -> String {
                            s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
                        };
                        
                        let norm_current = normalize(text_clean);
                        let mut is_dup = false;

                        // Check against last raw processed
                        if normalize(&last_processed_text) == norm_current {
                            is_dup = true;
                        }

                        // Check against current display buffer (max 2 lines)
                        if !is_dup {
                             let history_lock = full_transcript.lock().unwrap();
                             let lines: Vec<&str> = history_lock.split('\n').collect();
                             for line in lines {
                                 if normalize(line) == norm_current {
                                     is_dup = true;
                                     break;
                                 }
                             }
                        }

                        if is_dup { continue; }
                        
                        // Update last processed
                        last_processed_text = text_clean.to_string();

                        // --- UPDATE TRANSCRIPT HISTORY (Max 2 lines) ---
                        let mut full_history_str = full_transcript.lock().unwrap();
                        let mut lines: Vec<&str> = full_history_str.split('\n').filter(|s| !s.trim().is_empty()).collect();
                        
                        let current_line = text_clean.to_string();
                        lines.push(&current_line);
                        
                        // Keep only last 2
                        if lines.len() > 2 {
                            lines.remove(0);
                        }
                        
                        let new_full_str = lines.join("\n");
                        *full_history_str = new_full_str.clone();

                        // Update Primary
                        if !hide_overlay {
                            update_window_text(primary_hwnd, &new_full_str);
                        }

                        // 2. Retranslate (Chunk-based)
                        if let Some(sec_hwnd) = secondary_hwnd {
                            let text_to_trans = text_clean.to_string();
                            
                            let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                            let (tm_name, tm_provider) = match tm_config {
                                Some(m) => (m.full_name, m.provider.clone()),
                                None => ("openai/gpt-oss-20b".to_string(), "groq".to_string())
                            };
                            
                            let _ = translate_text_streaming(
                                &groq_api_key,
                                &gemini_api_key,
                                &openrouter_api_key,
                                text_to_trans,
                                retranslate_to.clone(),
                                tm_name,
                                tm_provider,
                                retranslate_streaming_enabled,
                                false,
                                |chunk| {}
                            ).map(|trans_text| {
                                let mut full_trans_str = full_translation.lock().unwrap();
                                let mut trans_lines: Vec<&str> = full_trans_str.split('\n').filter(|s| !s.trim().is_empty()).collect();
                                
                                // Logic: We want to match the transcript structure. 
                                // Since transcript added 1 line, we append 1 translated line.
                                // But translation is streaming/async.
                                // Simplified approach: Just append to history and trim independently?
                                // Better: Treating this entire block as processing "one transcript line".
                                
                                // Wait, we can't easily sync streaming chunks to a clean "line list" if we stream.
                                // BUT the request here calls `translate_text_streaming`... let's assume it returns whole text at end of map.
                                
                                let current_trans_line = trans_text.trim().to_string();
                                trans_lines.push(&current_trans_line);
                                
                                if trans_lines.len() > 2 {
                                    trans_lines.remove(0);
                                }
                                
                                let new_trans_str = trans_lines.join("\n");
                                *full_trans_str = new_trans_str.clone();
                                
                                if !hide_overlay {
                                    update_window_text(sec_hwnd, &new_trans_str);
                                }
                            });
                        }
                    }
                }
            }
        });

        // Message Loop
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if !IsWindow(primary_hwnd).as_bool() { break; }
            }
        }
    });

    LiveVisionSession { tx }
}
