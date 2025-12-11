use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::Graphics::Gdi::*;
use image::{ImageBuffer, Rgba};

pub fn to_wstring(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// --- CLIPBOARD SUPPORT ---
pub fn copy_to_clipboard(text: &str, hwnd: HWND) {
    unsafe {
        if OpenClipboard(hwnd).as_bool() {
            EmptyClipboard();
            
            // Convert text to UTF-16
            let wide_text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let mem_size = wide_text.len() * 2;
            
            // Allocate global memory
            if let Ok(h_mem) = GlobalAlloc(GMEM_MOVEABLE, mem_size) {
                let ptr = GlobalLock(h_mem) as *mut u16;
                std::ptr::copy_nonoverlapping(wide_text.as_ptr(), ptr, wide_text.len());
                GlobalUnlock(h_mem);
                
                // Set clipboard data (CF_UNICODETEXT = 13)
                let h_mem_handle = HANDLE(h_mem.0);
                let _ = SetClipboardData(13u32, h_mem_handle);
            }
            
            CloseClipboard();
        }
    }
}

/// Copies an RGBA image to the Windows Clipboard using CF_DIB format.
pub fn copy_image_to_clipboard(image: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> bool {
    let width = image.width() as i32;
    let height = image.height() as i32;
    
    // Calculate row stride (must be 4-byte aligned for DIB)
    let row_size = (width * 4) as usize;
    let padded_row_size = (row_size + 3) & !3; // Align to 4 bytes
    
    // BITMAPINFOHEADER is 40 bytes
    let header_size = std::mem::size_of::<BITMAPINFOHEADER>();
    let pixel_data_size = padded_row_size * height as usize;
    let total_size = header_size + pixel_data_size;
    
    unsafe {
        if !OpenClipboard(HWND(0)).as_bool() {
            log::error!("Failed to open clipboard for image copy");
            return false;
        }
        EmptyClipboard();
        
        let h_mem = match GlobalAlloc(GMEM_MOVEABLE, total_size) {
            Ok(h) => h,
            Err(e) => {
                log::error!("GlobalAlloc failed: {:?}", e);
                CloseClipboard();
                return false;
            }
        };
        
        let ptr = GlobalLock(h_mem) as *mut u8;
        if ptr.is_null() {
            log::error!("GlobalLock returned null");
            let _ = GlobalFree(h_mem);
            CloseClipboard();
            return false;
        }
        
        // Write BITMAPINFOHEADER
        let header = BITMAPINFOHEADER {
            biSize: header_size as u32,
            biWidth: width,
            biHeight: height, // Positive = bottom-up DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            biSizeImage: pixel_data_size as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        std::ptr::copy_nonoverlapping(&header as *const _ as *const u8, ptr, header_size);
        
        // Write pixel data (BGRA, bottom-up)
        let pixel_ptr = ptr.add(header_size);
        for y in 0..height {
            let src_y = (height - 1 - y) as u32; // Flip vertically for bottom-up
            for x in 0..width {
                let pixel = image.get_pixel(x as u32, src_y);
                let offset = (y as usize * padded_row_size) + (x as usize * 4);
                // RGBA -> BGRA
                *pixel_ptr.add(offset) = pixel[2];     // B
                *pixel_ptr.add(offset + 1) = pixel[1]; // G
                *pixel_ptr.add(offset + 2) = pixel[0]; // R
                *pixel_ptr.add(offset + 3) = pixel[3]; // A
            }
        }
        
        GlobalUnlock(h_mem);
        
        // CF_DIB = 8
        let h_mem_handle = HANDLE(h_mem.0);
        if SetClipboardData(8u32, h_mem_handle).is_err() {
            log::error!("SetClipboardData failed");
            let _ = GlobalFree(h_mem);
            CloseClipboard();
            return false;
        }
        
        CloseClipboard();
        log::info!("Image copied to clipboard ({}x{})", width, height);
        true
    }
}

pub fn get_error_message(error: &str, lang: &str) -> String {
    match error {
        "NO_API_KEY" => {
            match lang {
                "vi" => "Bạn chưa nhập API key!".to_string(),
                _ => "You haven't entered an API key!".to_string(),
            }
        }
        "INVALID_API_KEY" => {
            match lang {
                "vi" => "API key không hợp lệ!".to_string(),
                _ => "Invalid API key!".to_string(),
            }
        }
        _ => {
            match lang {
                "vi" => format!("Lỗi: {}", error),
                _ => format!("Error: {}", error),
            }
        }
    }
}

/// Convert markdown to cleaner plain text for display
/// Removes formatting symbols but preserves structure
pub fn clean_markdown_for_display(text: &str) -> String {
    let mut result = String::new();
    let lines = text.lines();
    
    for line in lines {
        let trimmed = line.trim();
        
        // Convert headers to emphasized text
        if trimmed.starts_with("### ") {
            result.push_str("▸ ");
            result.push_str(&trimmed[4..]);
        } else if trimmed.starts_with("## ") {
            result.push_str("■ ");
            result.push_str(&trimmed[3..].to_uppercase());
        } else if trimmed.starts_with("# ") {
            result.push_str("◆ ");
            result.push_str(&trimmed[2..].to_uppercase());
        } else if trimmed.starts_with("- ") {
            // Bullet points
            result.push_str("  • ");
            result.push_str(&trimmed[2..]);
        } else if trimmed.starts_with("* ") {
            result.push_str("  • ");
            result.push_str(&trimmed[2..]);
        } else if trimmed.starts_with("```") {
            // Code block markers - skip or convert
            if trimmed.len() > 3 {
                result.push_str("〈Code〉");
            }
        } else {
            result.push_str(trimmed);
        }
        result.push('\n');
    }
    
    // Clean up bold/italic markers
    let result = result.replace("**", "");
    let result = result.replace("__", "");
    // Keep single asterisk for emphasis but remove if at word boundaries
    let result = result.replace(" *", " ");
    let result = result.replace("* ", " ");
    let result = result.replace("`", "'");
    
    // Remove excessive newlines
    let mut prev_empty = false;
    let mut final_result = String::new();
    for line in result.lines() {
        let is_empty = line.trim().is_empty();
        if is_empty && prev_empty {
            continue; // Skip consecutive empty lines
        }
        final_result.push_str(line);
        final_result.push('\n');
        prev_empty = is_empty;
    }
    
    final_result.trim().to_string()
}
