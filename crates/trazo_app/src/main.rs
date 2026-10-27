//! Desktop entry point (Windows, macOS, Linux).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    #[cfg(windows)]
    if std::env::args().any(|a| a == "--uninstall") {
        uninstall::run();
        return Ok(());
    }
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

/// `trazo.exe --uninstall`, run by "Aplicaciones instaladas": removes the
/// program, its shortcuts and its entry. Animations and settings are kept.
#[cfg(windows)]
mod uninstall {
    use std::os::windows::process::CommandExt;

    const NO_WINDOW: u32 = 0x0800_0000;
    const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Trazo";

    pub fn run() {
        let yes = rfd::MessageDialog::new()
            .set_title("Trazo")
            .set_description("¿Desinstalar Trazo? Tus animaciones no se borran.\n\nUninstall Trazo? Your animations are kept.")
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if yes != rfd::MessageDialogResult::Yes {
            return;
        }
        let script = "foreach ($f in 'Programs','Desktop') { Remove-Item -LiteralPath ([Environment]::GetFolderPath($f) + '\\Trazo.lnk') -ErrorAction SilentlyContinue }";
        let _ = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script]).creation_flags(NO_WINDOW).status();
        let _ = std::process::Command::new("reg").args(["delete", UNINSTALL_KEY, "/f"]).creation_flags(NO_WINDOW).status();
        // The folder is removed once this exe has closed — only if it is the
        // installer's folder, never anywhere else.
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) {
            let installed = std::env::var_os("LOCALAPPDATA").map(|b| std::path::PathBuf::from(b).join("Programs").join("Trazo"));
            if installed.is_some_and(|i| i == dir) {
                let cmd = format!("timeout /t 2 /nobreak > nul & rmdir /s /q \"{}\"", dir.display());
                let _ = std::process::Command::new("cmd").args(["/c", &cmd]).creation_flags(NO_WINDOW).spawn();
            }
        }
        let _ = rfd::MessageDialog::new().set_title("Trazo").set_description("Trazo se desinstaló. · Trazo was uninstalled.").show();
    }
}
