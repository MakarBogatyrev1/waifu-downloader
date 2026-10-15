#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod api;
mod app;
mod assets;
mod settings;
mod system;
mod worker;

fn main() -> eframe::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut smoke = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                if let Some(dir) = args.next() {
                    base = dir.into();
                }
            }
            "--smoke-test" => smoke = true,
            _ => {}
        }
    }
    if let Err(error) = std::fs::create_dir_all(&base) {
        rfd::MessageDialog::new()
            .set_title("Waifu Gallery")
            .set_description(format!("Не удалось открыть папку данных: {error}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size([1040.0, 760.0])
            .with_title("Waifu Gallery"),
        ..Default::default()
    };
    eframe::run_native(
        "Waifu Gallery",
        options,
        Box::new(move |cc| Ok(Box::new(app::GalleryApp::new(cc, base, smoke)))),
    )
}
