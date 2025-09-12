fn main() {
    // Ensure an RGBA PNG exists at icons/icon.png for Tauri context
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let icon_dir = std::path::Path::new(&manifest_dir).join("icons");
    let icon_png = icon_dir.join("icon.png");
    if std::fs::metadata(&icon_png).is_err() {
        let _ = std::fs::create_dir_all(&icon_dir);
        // Write a 1x1 RGBA PNG (opaque white)
        let file = std::fs::File::create(&icon_png).expect("failed to create icons/icon.png");
        let w = std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(w, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png write header");
        let data: [u8; 4] = [255, 255, 255, 255];
        writer.write_image_data(&data).expect("png write data");
    }

    tauri_build::build();
}
