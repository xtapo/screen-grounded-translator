use std::path::Path;
use std::fs;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    
    // Ensure assets directory exists in project root
    let assets_dir = Path::new(&manifest_dir).join("assets");
    let _ = fs::create_dir_all(&assets_dir);
    
    // Copy icon file
    let icon_path = Path::new(&manifest_dir).join("icon.png");
    if icon_path.exists() {
        let tray_icon_path = assets_dir.join("tray_icon.png");
        let _ = fs::copy(&icon_path, &tray_icon_path);
        println!("cargo:warning=Icon copied to assets/tray_icon.png");
    }
    
    // Also notify cargo to rerun if icon changes
    println!("cargo:rerun-if-changed=icon.png");
}
