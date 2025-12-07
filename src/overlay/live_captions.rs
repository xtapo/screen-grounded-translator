// Live Captions Overlay Window
// Displays real-time translated captions from Windows Live Captions

use crate::config::LiveCaptionsConfig;
use crate::api::translate_text_streaming;
use crate::live_captions::{
    launch_live_captions, run_live_captions_loop, stop_live_captions,
    hide_live_captions, show_live_captions, LIVE_CAPTIONS_ACTIVE, 
    extract_latest_sentence,
};
use crate::APP;

use std::sync::{Arc, Mutex, atomic::Ordering};
use std::collections::VecDeque;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::core::*;

const OVERLAY_CLASS_NAME: &str = "LiveCaptionsOverlayWindow";
const OVERLAY_WIDTH: i32 = 800;
const OVERLAY_HEIGHT: i32 = 150;

lazy_static::lazy_static! {
    static ref OVERLAY_HWND: Arc<Mutex<Option<HWND>>> = Arc::new(Mutex::new(None));
    static ref CAPTION_LINES: Arc<Mutex<VecDeque<CaptionLine>>> = Arc::new(Mutex::new(VecDeque::new()));
    static ref MAX_LINES: Arc<Mutex<usize>> = Arc::new(Mutex::new(2));
}

#[derive(Clone)]
struct CaptionLine {
    original: String,
    translated: String,
}

/// Start the Live Captions overlay system
pub fn start_live_captions_overlay(config: LiveCaptionsConfig) {
    // Reset state
    if let Ok(mut lines) = CAPTION_LINES.lock() {
        lines.clear();
    }
    
    // Update max lines
    if let Ok(mut max) = MAX_LINES.lock() {
        *max = config.overlay_sentences.max(1).min(5);
    }
    
    // Start overlay window thread (with its own message loop)
    std::thread::spawn(move || {
        if let Err(e) = run_overlay_window_thread(config) {
            log::error!("Live Captions overlay thread error: {}", e);
        }
    });
}

/// Stop the Live Captions overlay
pub fn stop_live_captions_overlay() {
    stop_live_captions();
    
    // Post WM_CLOSE to overlay window (from any thread)
    if let Ok(hwnd_guard) = OVERLAY_HWND.lock() {
        if let Some(hwnd) = *hwnd_guard {
            if hwnd.0 != 0 {
                unsafe {
                    let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
    
    // Clear caption lines
    if let Ok(mut lines) = CAPTION_LINES.lock() {
        lines.clear();
    }
}

/// Check if Live Captions is currently active
pub fn is_live_captions_active() -> bool {
    LIVE_CAPTIONS_ACTIVE.load(Ordering::SeqCst)
}

/// Main thread for overlay window with proper message loop
fn run_overlay_window_thread(config: LiveCaptionsConfig) -> anyhow::Result<()> {
    // Launch Live Captions first
    let lc_hwnd = launch_live_captions()?;
    
    // Create overlay window
    let overlay_hwnd = create_overlay_window()?;
    
    if let Ok(mut hwnd_guard) = OVERLAY_HWND.lock() {
        *hwnd_guard = Some(overlay_hwnd);
    }
    
    // Get API keys
    let (groq_key, gemini_key, model) = {
        let app = APP.lock().map_err(|_| anyhow::anyhow!("Failed to lock APP"))?;
        (
            app.config.api_key.clone(),
            app.config.gemini_api_key.clone(),
            config.translation_model.clone(),
        )
    };
    
    let target_lang = config.target_language.clone();
    let show_original = config.show_original;
    let auto_hide = config.auto_hide_live_captions;
    
    // Start capture thread separately
    let overlay_hwnd_for_capture = overlay_hwnd;
    std::thread::spawn(move || {
        if let Err(e) = run_live_captions_loop(lc_hwnd, auto_hide, move |text| {
            // Extract latest sentence
            if let Some(sentence) = extract_latest_sentence(&text) {
                if sentence.trim().is_empty() {
                    return;
                }
                
                log::info!("Live caption captured: {}", sentence);
                
                // Translate in a blocking way (TODO: could optimize with async)
                let translated = match translate_text_streaming(
                    &groq_key,
                    &gemini_key,
                    sentence.clone(),
                    target_lang.clone(),
                    model.clone(),
                    "groq".to_string(),
                    false,
                    false,
                    |_| {},
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        log::error!("Translation error: {}", e);
                        format!("[Error: {}]", e)
                    }
                };
                
                // Add to caption lines
                if let Ok(mut lines) = CAPTION_LINES.lock() {
                    let max_lines = MAX_LINES.lock().map(|m| *m).unwrap_or(2);
                    
                    lines.push_back(CaptionLine {
                        original: if show_original { sentence } else { String::new() },
                        translated,
                    });
                    
                    // Keep only max_lines
                    while lines.len() > max_lines {
                        lines.pop_front();
                    }
                }
                
                // Trigger redraw via PostMessage (thread-safe)
                unsafe {
                    let _ = PostMessageW(overlay_hwnd_for_capture, WM_USER + 1, WPARAM(0), LPARAM(0));
                }
            }
        }) {
            log::error!("Live Captions capture loop error: {}", e);
        }
    });
    
    // Run message loop for overlay window
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            if msg.message == WM_QUIT {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    
    // Cleanup
    if let Ok(mut hwnd_guard) = OVERLAY_HWND.lock() {
        *hwnd_guard = None;
    }
    
    Ok(())
}

fn create_overlay_window() -> anyhow::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        
        let class_wide: Vec<u16> = OVERLAY_CLASS_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        
        let wc = WNDCLASSW {
            lpfnWndProc: Some(overlay_wnd_proc),
            hInstance: instance,
            lpszClassName: PCWSTR::from_raw(class_wide.as_ptr()),
            hbrBackground: HBRUSH::default(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        
        let _ = RegisterClassW(&wc);
        
        // Get screen dimensions
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        
        // Position at bottom center
        let x = (screen_width - OVERLAY_WIDTH) / 2;
        let y = screen_height - OVERLAY_HEIGHT - 100; // 100px from bottom
        
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR::from_raw(class_wide.as_ptr()),
            w!("Live Captions Translation"),
            WS_POPUP | WS_VISIBLE,
            x, y,
            OVERLAY_WIDTH, OVERLAY_HEIGHT,
            None, None, instance, None,
        );
        
        if hwnd.0 == 0 {
            return Err(anyhow::anyhow!("Failed to create overlay window"));
        }
        
        // Set transparency
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 230, LWA_ALPHA);
        
        // Set rounded corners if available
        let _ = set_rounded_corners(hwnd);
        
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        
        log::info!("Live Captions overlay window created");
        
        Ok(hwnd)
    }
}

fn set_rounded_corners(hwnd: HWND) {
    unsafe {
        // DWMWCP_ROUND = 2
        let preference: u32 = 2;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

unsafe extern "system" fn overlay_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            
            paint_overlay(hwnd, hdc);
            
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        // Custom message to trigger repaint from capture thread
        msg if msg == WM_USER + 1 => {
            let _ = InvalidateRect(hwnd, None, true);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // Allow dragging the window
            let _ = ReleaseCapture();
            SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            // Right click to close/stop
            stop_live_captions();
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_CLOSE => {
            stop_live_captions();
            DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn paint_overlay(hwnd: HWND, hdc: HDC) {
    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        
        // Create dark semi-transparent background
        let bg_brush = CreateSolidBrush(COLORREF(0x302020)); // Dark gray
        FillRect(hdc, &rect, bg_brush);
        let _ = DeleteObject(bg_brush);
        
        // Set text properties
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0xFFFFFF)); // White text
        
        // Create font
        let font = CreateFontW(
            22, 0, 0, 0,
            FW_NORMAL.0 as i32,
            0, 0, 0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (VARIABLE_PITCH.0 | FF_SWISS.0) as u32,
            w!("Segoe UI"),
        );
        let old_font = SelectObject(hdc, font);
        
        // Draw caption lines
        if let Ok(lines) = CAPTION_LINES.lock() {
            if lines.is_empty() {
                // Show waiting message
                SetTextColor(hdc, COLORREF(0x888888));
                let mut waiting_text: Vec<u16> = "Waiting for Live Captions...".encode_utf16().chain(std::iter::once(0)).collect();
                let mut text_rect = RECT {
                    left: 10,
                    top: 10,
                    right: rect.right - 10,
                    bottom: rect.bottom - 10,
                };
                DrawTextW(hdc, &mut waiting_text, &mut text_rect, DT_LEFT | DT_SINGLELINE);
            } else {
                let line_height = 28;
                let padding = 10;
                let mut y = padding;
                
                for line in lines.iter() {
                    // Draw original text (dimmer)
                    if !line.original.is_empty() {
                        SetTextColor(hdc, COLORREF(0xAAAAAA)); // Light gray
                        let mut original_wide: Vec<u16> = line.original.encode_utf16().chain(std::iter::once(0)).collect();
                        let mut text_rect = RECT {
                            left: padding,
                            top: y,
                            right: rect.right - padding,
                            bottom: y + line_height,
                        };
                        DrawTextW(hdc, &mut original_wide, &mut text_rect, DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS);
                        y += line_height;
                    }
                    
                    // Draw translated text (brighter)
                    SetTextColor(hdc, COLORREF(0xFFFFFF)); // White
                    let mut translated_wide: Vec<u16> = line.translated.encode_utf16().chain(std::iter::once(0)).collect();
                    let mut text_rect = RECT {
                        left: padding,
                        top: y,
                        right: rect.right - padding,
                        bottom: y + line_height,
                    };
                    DrawTextW(hdc, &mut translated_wide, &mut text_rect, DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS);
                    y += line_height + 5;
                }
            }
        }
        
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(font);
    }
}

/// Toggle Live Captions window visibility
#[allow(dead_code)]
pub fn toggle_live_captions_visibility(lc_hwnd: HWND, visible: bool) {
    if visible {
        let _ = show_live_captions(lc_hwnd);
    } else {
        let _ = hide_live_captions(lc_hwnd);
    }
}
