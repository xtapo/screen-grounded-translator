//! Chat Input Popup Module
//!
//! Displays a popup with an input field for the user to type their question
//! about the captured screenshot.

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::*;
use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};

lazy_static::lazy_static! {
    static ref USER_INPUT: Mutex<Option<String>> = Mutex::new(None);
    static ref INPUT_DISMISSED: AtomicBool = AtomicBool::new(false);
    static ref EDIT_HWND: Mutex<HWND> = Mutex::new(HWND(0));
}

// Layout constants
const POPUP_WIDTH: i32 = 400;
const POPUP_HEIGHT: i32 = 120;
const EDIT_HEIGHT: i32 = 36;
const BTN_WIDTH: i32 = 80;
const BTN_HEIGHT: i32 = 32;
const PADDING: i32 = 16;

const ID_EDIT: u16 = 100;
const ID_SEND_BTN: u16 = 101;
const ID_CANCEL_BTN: u16 = 102;

/// Show chat input popup and return user's question, or None if cancelled
pub fn show_chat_input_popup(selection_rect: RECT) -> Option<String> {
    // Reset state
    *USER_INPUT.lock().unwrap() = None;
    INPUT_DISMISSED.store(false, Ordering::SeqCst);
    *EDIT_HWND.lock().unwrap() = HWND(0);

    // Calculate popup position (centered below selection)
    let selection_center_x = (selection_rect.left + selection_rect.right) / 2;
    let popup_x = selection_center_x - POPUP_WIDTH / 2;
    let popup_y = selection_rect.bottom + 20;

    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("ChatInputPopupClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(chat_input_wnd_proc),
            hInstance: instance,
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: CreateSolidBrush(COLORREF(0x00282828)),
            ..Default::default()
        };

        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name,
            w!("Ask AI"),
            WS_POPUP | WS_BORDER,
            popup_x,
            popup_y,
            POPUP_WIDTH,
            POPUP_HEIGHT,
            None,
            None,
            instance,
            None,
        );

        if hwnd.0 == 0 {
            return None;
        }

        // Create Edit control (text input)
        let edit_hwnd = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x0080), // ES_AUTOHSCROLL
            PADDING,
            PADDING,
            POPUP_WIDTH - PADDING * 2,
            EDIT_HEIGHT,
            hwnd,
            HMENU(ID_EDIT as isize),
            instance,
            None,
        );
        *EDIT_HWND.lock().unwrap() = edit_hwnd;

        // Create Send button
        let btn_y = PADDING + EDIT_HEIGHT + 12;
        let send_btn_x = POPUP_WIDTH - PADDING - BTN_WIDTH * 2 - 10;
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            w!("Send"),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x0001), // BS_DEFPUSHBUTTON
            send_btn_x,
            btn_y,
            BTN_WIDTH,
            BTN_HEIGHT,
            hwnd,
            HMENU(ID_SEND_BTN as isize),
            instance,
            None,
        );

        // Create Cancel button
        let cancel_btn_x = POPUP_WIDTH - PADDING - BTN_WIDTH;
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            w!("Cancel"),
            WS_CHILD | WS_VISIBLE,
            cancel_btn_x,
            btn_y,
            BTN_WIDTH,
            BTN_HEIGHT,
            hwnd,
            HMENU(ID_CANCEL_BTN as isize),
            instance,
            None,
        );

        // Show window and set focus to edit
        ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(edit_hwnd);
        let _ = UpdateWindow(hwnd);

        // Message loop
        let mut msg = MSG::default();
        while !INPUT_DISMISSED.load(Ordering::SeqCst) {
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                if msg.message == WM_QUIT {
                    break;
                }
                // Handle Enter key in edit control
                if msg.message == WM_KEYDOWN && msg.wParam.0 == VK_RETURN.0 as usize {
                    submit_input(hwnd);
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        // Cleanup
        let _ = DestroyWindow(hwnd);
    }

    // Return user input
    USER_INPUT.lock().unwrap().take()
}

unsafe fn submit_input(hwnd: HWND) {
    let edit_hwnd = *EDIT_HWND.lock().unwrap();
    log::info!("submit_input called, edit_hwnd: {:?}", edit_hwnd.0);
    
    if edit_hwnd.0 != 0 {
        let len = GetWindowTextLengthW(edit_hwnd) + 1;
        let mut buf = vec![0u16; len as usize];
        GetWindowTextW(edit_hwnd, &mut buf);
        let text = String::from_utf16_lossy(&buf[..len as usize - 1]);
        
        log::info!("Chat input text: '{}' (len={})", text, text.len());
        
        if !text.trim().is_empty() {
            *USER_INPUT.lock().unwrap() = Some(text.trim().to_string());
            log::info!("Chat input accepted");
        } else {
            log::info!("Chat input was empty");
        }
    }
    INPUT_DISMISSED.store(true, Ordering::SeqCst);
}

unsafe fn cancel_input() {
    *USER_INPUT.lock().unwrap() = None;
    INPUT_DISMISSED.store(true, Ordering::SeqCst);
}

unsafe extern "system" fn chat_input_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as u16;
            let notification = ((wparam.0 >> 16) & 0xFFFF) as u16;
            
            // Button click (BN_CLICKED = 0)
            if notification == 0 {
                match id {
                    ID_SEND_BTN => {
                        submit_input(hwnd);
                    }
                    ID_CANCEL_BTN => {
                        cancel_input();
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }

        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                cancel_input();
            } else if wparam.0 == VK_RETURN.0 as usize {
                submit_input(hwnd);
            }
            LRESULT(0)
        }

        WM_CLOSE => {
            cancel_input();
            LRESULT(0)
        }

        WM_ACTIVATEAPP => {
            // Don't auto-dismiss - user must click Cancel or press Escape
            // This prevents the popup from closing when it first appears
            LRESULT(0)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            
            // Draw title text
            let _ = SetBkMode(hdc, TRANSPARENT);
            let _ = SetTextColor(hdc, COLORREF(0x00DDDDDD));
            
            // Get locale text
            let title = "💬 What would you like to ask about this image?";
            let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            
            // This won't show well because we don't have proper font setup
            // But the edit control and buttons will work
            
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
