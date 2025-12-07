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
                let vision_res = translate_image_streaming(
                    &groq_api_key, 
                    &gemini_api_key, 
                    final_prompt, 
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
                            update_window_text(primary_hwnd, &text);
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
                                update_window_text(primary_hwnd, &vision_text);
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
                             let gemini_key_for_retrans = gemini_api_key.clone(); // Capture key
                             
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
                                         &gemini_key_for_retrans, // Pass Gemini Key
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
        }

        if retranslate && !text.trim().is_empty() {
            let rect_sec = retrans_rect.unwrap();
            let text_for_retrans = text.clone();
            let (groq_key, gemini_key) = {
                let app = crate::APP.lock().unwrap();
                (app.config.api_key.clone(), app.config.gemini_api_key.clone())
            };
            
            std::thread::spawn(move || {
                let secondary_hwnd = create_result_window(rect_sec, WindowType::SecondaryExplicit);
                link_windows(primary_hwnd, secondary_hwnd);
                
                if !hide_overlay {
                    unsafe { ShowWindow(secondary_hwnd, SW_SHOW); }
                    update_window_text(secondary_hwnd, "");
                }

                // API Call for Retranslation
                std::thread::spawn(move || {
                    let acc_text = Arc::new(Mutex::new(String::new()));
                    let acc_text_clone = acc_text.clone();
                    
                    let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                    let (tm_name, tm_provider) = match tm_config {
                        Some(m) => (m.full_name, m.provider),
                        None => ("openai/gpt-oss-20b".to_string(), "groq".to_string())
                    };

                    let text_res = translate_text_streaming(
                        &groq_key,
                        &gemini_key,
                        text_for_retrans,
                        retranslate_to,
                        tm_name,
                        tm_provider,
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

    let (groq_api_key, gemini_api_key, ui_language) = {
        let app = crate::APP.lock().unwrap();
        (app.config.api_key.clone(), app.config.gemini_api_key.clone(), app.config.ui_language.clone())
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
        
        // Indicate processing start
        if !hide_overlay {
            unsafe { 
                // Close the recording overlay first
                if IsWindow(overlay_hwnd).as_bool() {
                    PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); 
                }
                ShowWindow(primary_hwnd, SW_SHOW); 
            }
            // Use local string loading if possible, or hardcode simplified for now
            update_window_text(primary_hwnd, "Processing...");
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
                    
                    // Retranslate Logic
                    if retranslate && !full_text.trim().is_empty() {
                        let rect_sec = retranslate_rect.unwrap();
                        let text_for_retrans = full_text.clone();
                        
                        let (groq_key_r, gemini_key_r) = {
                            let app = crate::APP.lock().unwrap();
                            (app.config.api_key.clone(), app.config.gemini_api_key.clone())
                        };

                        // Spawn Secondary Window Thread
                        std::thread::spawn(move || {
                            let secondary_hwnd = create_result_window(rect_sec, WindowType::SecondaryExplicit);
                            link_windows(primary_hwnd, secondary_hwnd);

                             if !hide_overlay {
                                unsafe { ShowWindow(secondary_hwnd, SW_SHOW); }
                                update_window_text(secondary_hwnd, "");
                            }
                            
                            // Retranslate API
                             std::thread::spawn(move || {
                                 let acc_retrans = Arc::new(Mutex::new(String::new()));
                                 let acc_retrans_clone = acc_retrans.clone();
                                 
                                let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                                let (tm_name, tm_provider) = match tm_config {
                                    Some(m) => (m.full_name, m.provider),
                                    None => ("openai/gpt-oss-20b".to_string(), "groq".to_string())
                                };
                                
                                let _ = translate_text_streaming(
                                      &groq_key_r,
                                      &gemini_key_r,
                                      text_for_retrans,
                                      retranslate_to,
                                      tm_name,
                                      tm_provider,
                                      retranslate_streaming_enabled,
                                      false,
                                      |chunk| {
                                          let mut t = acc_retrans_clone.lock().unwrap();
                                          t.push_str(chunk);
                                          if !hide_overlay {
                                              update_window_text(secondary_hwnd, &t);
                                          }
                                      }
                                );
                                
                                let final_retrans = acc_retrans_clone.lock().unwrap().clone();
                                if !hide_overlay {
                                     update_window_text(secondary_hwnd, &final_retrans);
                                }
                                if retranslate_auto_copy {
                                     std::thread::spawn(move || {
                                        std::thread::sleep(std::time::Duration::from_millis(100));
                                        copy_to_clipboard(&final_retrans, HWND(0));
                                    });
                                }
                             });
                             
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

    let (groq_api_key, gemini_api_key, ui_language) = {
        let app = crate::APP.lock().unwrap();
        (app.config.api_key.clone(), app.config.gemini_api_key.clone(), app.config.ui_language.clone())
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
            while let Ok(wav_data) = rx.recv() {
                // 1. Transcribe
                let res: anyhow::Result<String> = if provider == "google" {
                    if gemini_api_key.trim().is_empty() { Err(anyhow::anyhow!("NO_API_KEY")) }
                    else {
                        transcribe_audio_gemini(
                            &gemini_api_key,
                            final_prompt.clone(),
                            model_name.clone(),
                            wav_data,
                            |chunk| { 
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

                if let Ok(text) = res {
                    if !text.trim().is_empty() {
                        let mut full = full_transcript.lock().unwrap();
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
