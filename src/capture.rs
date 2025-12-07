use image::ImageBuffer;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn capture_full_screen() -> anyhow::Result<ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        
        // Validate dimensions
        if width <= 0 || height <= 0 {
            return Err(anyhow::anyhow!("GDI Error: Invalid screen dimensions ({} x {})", width, height));
        }

        let hdc_screen = GetDC(None);
        if hdc_screen.0 == 0 {
            return Err(anyhow::anyhow!("GDI Error: Failed to get screen device context"));
        }
        
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem.0 == 0 {
            ReleaseDC(None, hdc_screen);
            return Err(anyhow::anyhow!("GDI Error: Failed to create compatible device context"));
        }
        
        let hbitmap = CreateCompatibleBitmap(hdc_screen, width, height);
        
        if hbitmap.0 == 0 {
             DeleteDC(hdc_mem);
             ReleaseDC(None, hdc_screen);
             return Err(anyhow::anyhow!("GDI Error: Failed to create compatible bitmap."));
        }
        
        SelectObject(hdc_mem, hbitmap);

        BitBlt(hdc_mem, 0, 0, width, height, hdc_screen, x, y, SRCCOPY).ok()?;

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut buffer: Vec<u8> = vec![0; (width * height * 4) as usize];
        GetDIBits(hdc_mem, hbitmap, 0, height as u32, Some(buffer.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);

        for chunk in buffer.chunks_exact_mut(4) {
            chunk.swap(0, 2);
            chunk[3] = 255;
        }

        DeleteObject(hbitmap);
        DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        let img = ImageBuffer::from_raw(width as u32, height as u32, buffer)
            .ok_or_else(|| anyhow::anyhow!("Buffer creation failed"))?;
        
        Ok(img)
    }
}
