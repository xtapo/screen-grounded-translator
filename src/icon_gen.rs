pub fn generate_icon() -> tray_icon::Icon {
    // Embedded tray icon data
    let icon_bytes = include_bytes!("../assets/tray-icon.png");
    
    match image::load_from_memory(icon_bytes) {
        Ok(img) => {
            let img_rgba = img.to_rgba8();
            let (width, height) = img_rgba.dimensions();
            let rgba = img_rgba.into_raw();
            tray_icon::Icon::from_rgba(rgba, width, height).unwrap()
        }
        Err(_) => {
            // Fallback: generate a simple blue icon with white dot if embedding fails
            let width = 32;
            let height = 32;
            let mut img = image::ImageBuffer::new(width, height);

            for (_x, _y, pixel) in img.enumerate_pixels_mut() {
                *pixel = image::Rgba([50, 100, 255, 255]);
            }
            
            for i in 12..20 {
                 for j in 12..20 {
                     img.put_pixel(i, j, image::Rgba([255, 255, 255, 255]));
                 }
            }

            let rgba = img.into_raw();
            tray_icon::Icon::from_rgba(rgba, width, height).unwrap()
        }
    }
}