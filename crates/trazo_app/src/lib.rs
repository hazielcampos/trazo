//! Trazo: frame-by-frame animation and drawing app (desktop + Android).
//!
//! The engine lives in `trazo_core`; this crate is the egui front end.

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
mod android_mp4;
#[cfg(target_os = "android")]
mod android_video;
mod app;
mod audio_engine;
mod canvas;
pub(crate) mod panels;
mod platform;
mod save;
mod selection;
mod icons;
mod files;
mod import;
mod theme;
mod widgets;
mod tools;

pub use app::TrazoApp;
pub use platform::Platform;

pub const APP_ID: &str = "trazo";

/// Shared entry point for every platform.
pub fn run(options: eframe::NativeOptions, platform: Platform) -> eframe::Result {
    eframe::run_native(
        "Trazo",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(TrazoApp::new(cc, platform)))
        }),
    )
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    android::set_activity(app.activity_as_ptr());
    // External app storage is reachable from a PC / file manager, so the
    // "importar" folder and exported files are usable; fall back to internal.
    let data_dir = app
        .external_data_path()
        .or_else(|| app.internal_data_path())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    if let Err(e) = run(options, Platform::android(data_dir)) {
        log::error!("Trazo terminó con error: {e}");
    }
    // The activity was destroyed (the project was saved on exit). Android may
    // start a new activity in this same process, but winit's event loop can't
    // be created twice: end the process so the next launch starts clean.
    // Let a running export finish (up to a minute) so the file isn't left broken.
    let started = std::time::Instant::now();
    while panels::export::EXPORT_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) && started.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    log::info!("actividad cerrada: terminando el proceso");
    std::process::exit(0);
}
