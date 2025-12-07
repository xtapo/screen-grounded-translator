// Live Captions Handler Module
// Uses Windows UI Automation via PowerShell to capture text from Windows Live Captions
//
// Note: Since the windows crate 0.48 has limited UIA support for VARIANT types,
// we use PowerShell as a bridge to access UI Automation functionality.

use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

lazy_static::lazy_static! {
    pub static ref LIVE_CAPTIONS_ACTIVE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref LIVE_CAPTIONS_STOP_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref LAST_ERROR: Arc<std::sync::Mutex<String>> = Arc::new(std::sync::Mutex::new(String::new()));
}

const LIVE_CAPTIONS_WINDOW_CLASS: &str = "LiveCaptionsDesktopWindow";

// PowerShell script to get caption text via UI Automation
const PS_GET_CAPTION_SCRIPT: &str = r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$root = [System.Windows.Automation.AutomationElement]::RootElement
$condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, "LiveCaptionsDesktopWindow")
$lcWindow = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)

if ($lcWindow) {
    $textCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, "CaptionsTextBlock")
    $textElement = $lcWindow.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $textCondition)
    
    if ($textElement) {
        $text = $textElement.Current.Name
        if ($text) {
            Write-Output $text
        }
    }
}
"#;

// PowerShell script to check if Live Captions window exists
const PS_CHECK_LC_SCRIPT: &str = r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$root = [System.Windows.Automation.AutomationElement]::RootElement
$condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, "LiveCaptionsDesktopWindow")
$lcWindow = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)

if ($lcWindow) {
    Write-Output "OK"
} else {
    Write-Output "NOT_FOUND"
}
"#;

/// Check if Live Captions is currently running and accessible
pub fn check_live_captions_running() -> Result<bool> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", PS_CHECK_LC_SCRIPT])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| anyhow!("PowerShell execution failed: {}", e))?;
    
    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(result == "OK")
}

/// Get the last error message
pub fn get_last_error() -> String {
    LAST_ERROR.lock().map(|e| e.clone()).unwrap_or_default()
}

/// Set an error message
fn set_error(msg: &str) {
    if let Ok(mut err) = LAST_ERROR.lock() {
        *err = msg.to_string();
    }
    log::error!("Live Captions error: {}", msg);
}

/// Launch Windows Live Captions and return the window handle
pub fn launch_live_captions() -> Result<HWND> {
    // Try to find existing Live Captions window first
    let existing_hwnd = find_window_by_class(LIVE_CAPTIONS_WINDOW_CLASS);
    if existing_hwnd.0 != 0 {
        log::info!("Found existing LiveCaptions window: {:?}", existing_hwnd);
        return Ok(existing_hwnd);
    }
    
    // Try different methods to launch Live Captions
    // Method 1: ms-settings URI
    let _ = Command::new("cmd")
        .args(["/C", "start", "ms-settings:easeofaccess-livecaptions"])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn();
    
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    // Method 2: Direct executable (if Method 1 didn't work)
    let hwnd = find_window_by_class(LIVE_CAPTIONS_WINDOW_CLASS);
    if hwnd.0 != 0 {
        return Ok(hwnd);
    }
    
    // Try launching via LiveCaptions.exe
    let _ = Command::new("LiveCaptions")
        .spawn();
    
    // Wait for the window to appear (with timeout)
    let mut hwnd = HWND::default();
    for i in 0..50 {
        hwnd = find_window_by_class(LIVE_CAPTIONS_WINDOW_CLASS);
        if hwnd.0 != 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        
        if i == 25 {
            // Halfway through, try another launch method
            let _ = Command::new("explorer")
                .arg("shell:AppsFolder\\Microsoft.LowLevelCaptions_8wekyb3d8bbwe!App")
                .creation_flags(0x08000000)
                .spawn();
        }
    }
    
    if hwnd.0 == 0 {
        set_error("Không tìm thấy cửa sổ Live Captions. Vui lòng bật Live Captions thủ công bằng Win + Ctrl + L hoặc vào Settings > Accessibility > Captions");
        return Err(anyhow!("Failed to find LiveCaptions window"));
    }
    
    log::info!("LiveCaptions window found: {:?}", hwnd);
    Ok(hwnd)
}

/// Find a window by its class name
fn find_window_by_class(class_name: &str) -> HWND {
    unsafe {
        let class_wide: Vec<u16> = class_name.encode_utf16().chain(std::iter::once(0)).collect();
        FindWindowW(PCWSTR::from_raw(class_wide.as_ptr()), PCWSTR::null())
    }
}

/// Hide the Live Captions window (minimize + remove from taskbar)
pub fn hide_live_captions(hwnd: HWND) -> Result<()> {
    unsafe {
        // Get current extended style
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        
        // Minimize and add WS_EX_TOOLWINDOW to hide from taskbar
        ShowWindow(hwnd, SW_MINIMIZE);
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_TOOLWINDOW.0 as i32);
        
        log::info!("LiveCaptions window hidden");
        Ok(())
    }
}

/// Show/restore the Live Captions window  
pub fn show_live_captions(hwnd: HWND) -> Result<()> {
    unsafe {
        // Get current extended style
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        
        // Remove WS_EX_TOOLWINDOW and restore
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style & !(WS_EX_TOOLWINDOW.0 as i32));
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
        
        log::info!("LiveCaptions window restored");
        Ok(())
    }
}

/// Stop Live Captions
pub fn stop_live_captions() {
    LIVE_CAPTIONS_STOP_SIGNAL.store(true, Ordering::SeqCst);
    LIVE_CAPTIONS_ACTIVE.store(false, Ordering::SeqCst);
    log::info!("LiveCaptions stopped");
}

/// Get caption text using PowerShell UI Automation
fn get_caption_text_via_powershell() -> Result<String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", PS_GET_CAPTION_SCRIPT])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW - hide console
        .output()
        .map_err(|e| anyhow!("PowerShell execution failed: {}", e))?;
    
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(text)
    } else {
        Ok(String::new())
    }
}

/// Simple reader that uses PowerShell for UIA access
pub struct LiveCaptionsReader {
    _main_hwnd: HWND,
    last_text: String,
    error_count: u32,
}

impl LiveCaptionsReader {
    /// Create a new LiveCaptionsReader
    pub fn new(hwnd: HWND) -> Self {
        Self {
            _main_hwnd: hwnd,
            last_text: String::new(),
            error_count: 0,
        }
    }
    
    /// Read the current caption text via PowerShell UIA
    pub fn get_caption_text(&mut self) -> Result<String> {
        match get_caption_text_via_powershell() {
            Ok(text) => {
                self.error_count = 0;
                Ok(text)
            }
            Err(e) => {
                self.error_count += 1;
                if self.error_count > 5 {
                    set_error("Không thể đọc text từ Live Captions. Vui lòng kiểm tra Live Captions đang chạy.");
                }
                Err(e)
            }
        }
    }
    
    /// Check if text has changed and return the new text if so
    pub fn get_text_if_changed(&mut self) -> Option<String> {
        if let Ok(current_text) = self.get_caption_text() {
            let trimmed = current_text.trim().to_string();
            if !trimmed.is_empty() && trimmed != self.last_text {
                log::info!("Caption text: {}", trimmed);
                self.last_text = trimmed.clone();
                return Some(trimmed);
            }
        }
        None
    }
}

/// Main loop for capturing Live Captions and translating
/// This runs in its own thread
pub fn run_live_captions_loop<F>(
    hwnd: HWND,
    auto_hide: bool,
    mut on_caption: F,
) -> Result<()> 
where
    F: FnMut(String) + Send + 'static,
{
    LIVE_CAPTIONS_ACTIVE.store(true, Ordering::SeqCst);
    LIVE_CAPTIONS_STOP_SIGNAL.store(false, Ordering::SeqCst);
    
    // Clear any previous errors
    if let Ok(mut err) = LAST_ERROR.lock() {
        err.clear();
    }
    
    // Hide Live Captions window if requested
    if auto_hide {
        let _ = hide_live_captions(hwnd);
    }
    
    // Initialize reader
    let mut reader = LiveCaptionsReader::new(hwnd);
    
    // Wait a bit for LiveCaptions to fully initialize
    std::thread::sleep(std::time::Duration::from_millis(2000));
    
    log::info!("Live Captions capture loop started");
    
    // Main capture loop - poll slower since PowerShell has overhead
    while !LIVE_CAPTIONS_STOP_SIGNAL.load(Ordering::SeqCst) {
        if let Some(new_text) = reader.get_text_if_changed() {
            on_caption(new_text);
        }
        
        // Poll every 300ms (PowerShell overhead)
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    
    LIVE_CAPTIONS_ACTIVE.store(false, Ordering::SeqCst);
    log::info!("Live Captions capture loop ended");
    
    Ok(())
}

/// Check if Live Captions is available on this system
#[allow(dead_code)]
pub fn is_live_captions_available() -> bool {
    // Check Windows version (11 22H2+)
    let result = Command::new("powershell")
        .args(["-NoProfile", "-Command", "[System.Environment]::OSVersion.Version.Build"])
        .stdout(Stdio::piped())
        .creation_flags(0x08000000)
        .output();
    
    match result {
        Ok(output) => {
            let build_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(build) = build_str.parse::<u32>() {
                // Windows 11 22H2 is build 22621+
                build >= 22621
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Helper: Extract the latest complete sentence from Live Captions text
pub fn extract_latest_sentence(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}
