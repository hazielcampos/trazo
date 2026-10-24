//! Desktop entry point (Windows, macOS, Linux).

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default().with_title("Trazo");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon/icon-256.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };
    trazo_app::run(options, trazo_app::Platform::desktop())
}
