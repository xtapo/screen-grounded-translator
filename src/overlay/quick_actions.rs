//! Quick Actions Overlay Module
//!
//! Displays a floating menu with quick action buttons after selecting a screen region.
//! Users can choose from predefined actions like Translate, OCR, Ask AI, or Summarize.

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::core::*;
use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};

use crate::APP;
use crate::config::QuickAction;

// --- State ---

lazy_static::lazy_static! {
    /// Currently visible quick actions menu state
    static ref MENU_STATE: Mutex<Option<QuickActionsState>> = Mutex::new(None);
    /// Selected action result (preset_id or None if cancelled)
    static ref SELECTED_ACTION: Mutex<Option<String>> = Mutex::new(None);
    /// Flag to indicate menu has been dismissed
    static ref MENU_DISMISSED: AtomicBool = AtomicBool::new(false);
}

pub struct QuickActionsState {
    pub hwnd: HWND,
    pub selection_rect: RECT,
    pub hovered_action: Option<usize>,
    pub actions: Vec<QuickAction>,
}

// Menu dimensions
const MENU_WIDTH: i32 = 180;
const ITEM_HEIGHT: i32 = 40;
const MENU_PADDING: i32 = 8;
const CORNER_RADIUS: i32 = 12;

/// Show the quick actions menu at the given position
/// Returns the selected preset_id, or None if cancelled
pub fn show_quick_actions_menu(
    selection_rect: RECT,
    _captured_image: Vec<u8>, // Reserved for future use (thumbnail preview)
) -> Option<String> {
    // Get enabled actions from config
    let actions: Vec<QuickAction> = {
        if let Ok(app) = APP.lock() {
            app.config.quick_actions.actions
                .iter()
                .filter(|a| a.enabled)
                .cloned()
                .collect()
        } else {
            return None;
        }
    };

    if actions.is_empty() {
        return None;
    }

    // Reset state
    *SELECTED_ACTION.lock().unwrap() = None;
    MENU_DISMISSED.store(false, Ordering::SeqCst);

    // Calculate menu position (below selection, centered)
    let menu_height = MENU_PADDING * 2 + (actions.len() as i32 * ITEM_HEIGHT);
    let selection_center_x = (selection_rect.left + selection_rect.right) / 2;
    let menu_x = selection_center_x - MENU_WIDTH / 2;
    let menu_y = selection_rect.bottom + 10;

    // Create window
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("QuickActionsMenuClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(quick_actions_wnd_proc),
            hInstance: instance,
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH(0),
            ..Default::default()
        };

        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
            class_name,
            w!("Quick Actions"),
            WS_POPUP,
            menu_x,
            menu_y,
            MENU_WIDTH,
            menu_height,
            None,
            None,
            instance,
            None,
        );

        if hwnd.0 == 0 {
            return None;
        }

        // Store state
        *MENU_STATE.lock().unwrap() = Some(QuickActionsState {
            hwnd,
            selection_rect,
            hovered_action: None,
            actions: actions.clone(),
        });

        // Make window semi-transparent with blur effect
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 250, LWA_ALPHA);

        ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
        let _ = UpdateWindow(hwnd);

        // Message loop - wait for selection or dismiss
        let mut msg = MSG::default();
        while !MENU_DISMISSED.load(Ordering::SeqCst) {
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                if msg.message == WM_QUIT {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        // Cleanup
        let _ = DestroyWindow(hwnd);
        *MENU_STATE.lock().unwrap() = None;
    }

    // Return selected action
    SELECTED_ACTION.lock().unwrap().take()
}

/// Dismiss the quick actions menu
pub fn dismiss_menu() {
    MENU_DISMISSED.store(true, Ordering::SeqCst);
}

/// Check if quick actions menu is currently visible
pub fn is_menu_visible() -> bool {
    MENU_STATE.lock().unwrap().is_some()
}

// --- Window Procedure ---

unsafe extern "system" fn quick_actions_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            
            if let Ok(state) = MENU_STATE.lock() {
                if let Some(ref menu_state) = *state {
                    paint_menu(hdc, hwnd, menu_state);
                }
            }
            
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let item_idx = get_item_at_y(y);
            
            if let Ok(mut state) = MENU_STATE.lock() {
                if let Some(ref mut menu_state) = *state {
                    if menu_state.hovered_action != item_idx {
                        menu_state.hovered_action = item_idx;
                        drop(state);
                        let _ = InvalidateRect(hwnd, None, true);
                    }
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            if let Some(item_idx) = get_item_at_y(y) {
                if let Ok(state) = MENU_STATE.lock() {
                    if let Some(ref menu_state) = *state {
                        if item_idx < menu_state.actions.len() {
                            let preset_id = menu_state.actions[item_idx].preset_id.clone();
                            *SELECTED_ACTION.lock().unwrap() = Some(preset_id);
                        }
                    }
                }
            }
            dismiss_menu();
            LRESULT(0)
        }

        WM_KEYDOWN => {
            let vk = wparam.0 as u32;
            match vk {
                0x31..=0x34 => { // Keys 1-4
                    let idx = (vk - 0x31) as usize;
                    if let Ok(state) = MENU_STATE.lock() {
                        if let Some(ref menu_state) = *state {
                            if idx < menu_state.actions.len() {
                                let preset_id = menu_state.actions[idx].preset_id.clone();
                                *SELECTED_ACTION.lock().unwrap() = Some(preset_id);
                            }
                        }
                    }
                    dismiss_menu();
                }
                0x1B => { // Escape
                    dismiss_menu();
                }
                _ => {}
            }
            LRESULT(0)
        }

        WM_KILLFOCUS => {
            // Don't auto-dismiss on focus loss - let user click outside or press Escape
            // Only dismiss if we're not the foreground window anymore after a delay
            LRESULT(0)
        }

        WM_ACTIVATEAPP => {
            // Dismiss when app loses activation (user clicked outside app)
            if wparam.0 == 0 {
                dismiss_menu();
            }
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// --- Painting ---

fn paint_menu(hdc: HDC, hwnd: HWND, state: &QuickActionsState) {
    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);

        // Background color (dark semi-transparent)
        let bg_color = RGB(35, 35, 45);
        let bg_brush = CreateSolidBrush(bg_color);
        
        // Create rounded rect region
        let region = CreateRoundRectRgn(0, 0, rect.right, rect.bottom, CORNER_RADIUS, CORNER_RADIUS);
        let _ = SelectClipRgn(hdc, region);
        
        let _ = FillRect(hdc, &rect, bg_brush);
        let _ = DeleteObject(bg_brush);
        let _ = DeleteObject(region);

        // Draw items
        let _ = SetBkMode(hdc, TRANSPARENT);
        
        for (idx, action) in state.actions.iter().enumerate() {
            let item_y = MENU_PADDING + (idx as i32 * ITEM_HEIGHT);
            let item_rect = RECT {
                left: MENU_PADDING,
                top: item_y,
                right: rect.right - MENU_PADDING,
                bottom: item_y + ITEM_HEIGHT,
            };

            // Hover highlight
            if state.hovered_action == Some(idx) {
                let hover_color = RGB(60, 60, 80);
                let hover_brush = CreateSolidBrush(hover_color);
                let _ = FillRect(hdc, &item_rect, hover_brush);
                let _ = DeleteObject(hover_brush);
            }

            // Icon and text
            let text = format!("{}  {}  [{}]", action.icon, action.name, idx + 1);
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();

            let _ = SetTextColor(hdc, RGB(230, 230, 240));
            
            let mut text_rect = item_rect;
            text_rect.left += 12;
            text_rect.top += 10;
            
            let _ = DrawTextW(hdc, &mut wide.clone(), &mut text_rect, DT_LEFT | DT_SINGLELINE);
        }
    }
}

fn get_item_at_y(y: i32) -> Option<usize> {
    let adjusted_y = y - MENU_PADDING;
    if adjusted_y < 0 {
        return None;
    }
    let idx = adjusted_y / ITEM_HEIGHT;
    
    if let Ok(state) = MENU_STATE.lock() {
        if let Some(ref menu_state) = *state {
            if (idx as usize) < menu_state.actions.len() {
                return Some(idx as usize);
            }
        }
    }
    None
}

fn RGB(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}
