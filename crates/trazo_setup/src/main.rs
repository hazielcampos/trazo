//! Trazo installer for Windows.
//!
//! Installs for the current user in `%LOCALAPPDATA%\Programs\Trazo` (no
//! administrator needed, and Trazo can update itself there), adds shortcuts to
//! the Start menu (and the desktop if wanted) and registers Trazo in
//! "Aplicaciones instaladas" so it can be uninstalled from Windows' settings.
//! Uninstalling is done by `trazo.exe --uninstall`.

#![cfg_attr(windows, windows_subsystem = "windows")]

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(not(windows))]
fn main() {
    eprintln!("The Trazo installer is for Windows. On Linux, run the `trazo` binary directly.");
}

#[cfg(windows)]
fn main() -> eframe::Result {
    win::run()
}

#[cfg(windows)]
mod win {
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, Vec2};

    use super::{PAYLOAD, VERSION};

    const NO_WINDOW: u32 = 0x0800_0000;
    const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Trazo";
    const BG: Color32 = Color32::from_rgb(14, 14, 15);
    const SURFACE: Color32 = Color32::from_rgb(27, 27, 29);
    const ELEVATED: Color32 = Color32::from_rgb(37, 37, 40);
    const TEXT: Color32 = Color32::from_rgb(242, 242, 240);
    const TEXT2: Color32 = Color32::from_rgb(161, 161, 166);
    const ACCENT: Color32 = Color32::from_rgb(217, 244, 90);
    const DANGER: Color32 = Color32::from_rgb(255, 122, 110);

    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }

    fn spanish() -> bool {
        (unsafe { GetUserDefaultUILanguage() } & 0x3ff) == 0x0a
    }

    /// Spanish or English, as Windows is set.
    fn t(es: &'static str, en: &'static str) -> &'static str {
        if spanish() { es } else { en }
    }

    pub fn install_dir() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"));
        base.join("Programs").join("Trazo")
    }

    #[derive(Clone, PartialEq)]
    enum Step {
        Ready,
        Working,
        Done,
        Failed(String),
    }

    struct Setup {
        desktop: bool,
        step: Arc<Mutex<Step>>,
        icon: Option<egui::TextureHandle>,
    }

    pub fn run() -> eframe::Result {
        let mut viewport = egui::ViewportBuilder::default().with_title("Trazo").with_inner_size([520.0, 380.0]).with_resizable(false);
        if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon/icon-256.png")) {
            viewport = viewport.with_icon(Arc::new(icon));
        }
        let options = eframe::NativeOptions { viewport, ..Default::default() };
        eframe::run_native(
            "Trazo setup",
            options,
            Box::new(|cc| {
                fonts(&cc.egui_ctx);
                let mut v = egui::Visuals::dark();
                v.panel_fill = BG;
                v.window_fill = BG;
                cc.egui_ctx.set_visuals(v);
                let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon/icon-256.png")).ok().map(|d| {
                    let img = egui::ColorImage::from_rgba_unmultiplied([d.width as usize, d.height as usize], &d.rgba);
                    cc.egui_ctx.load_texture("icon", img, egui::TextureOptions::LINEAR)
                });
                Ok(Box::new(Setup { desktop: true, step: Arc::new(Mutex::new(Step::Ready)), icon }))
            }),
        )
    }

    fn fonts(ctx: &egui::Context) {
        let mut f = FontDefinitions::default();
        f.font_data.insert("regular".into(), Arc::new(FontData::from_static(include_bytes!("../../../assets/fonts/HankenGrotesk-Regular.ttf"))));
        f.font_data.insert("bold".into(), Arc::new(FontData::from_static(include_bytes!("../../../assets/fonts/HankenGrotesk-Bold.ttf"))));
        f.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "regular".into());
        f.families.insert(FontFamily::Name("bold".into()), vec!["bold".into(), "regular".into()]);
        ctx.set_fonts(f);
    }

    fn bold(size: f32) -> FontId {
        FontId::new(size, FontFamily::Name("bold".into()))
    }

    impl eframe::App for Setup {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let step = self.step.lock().map(|s| s.clone()).unwrap_or(Step::Ready);
            egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(32))).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.horizontal(|ui| {
                    if let Some(icon) = &self.icon {
                        ui.add(egui::Image::new((icon.id(), Vec2::splat(56.0))).corner_radius(14));
                    }
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Trazo").font(bold(30.0)).color(TEXT));
                        ui.label(RichText::new(format!("{} {VERSION}", t("Versión", "Version"))).size(14.0).color(TEXT2));
                    });
                });
                ui.add_space(6.0);
                match &step {
                    Step::Ready => {
                        ui.label(RichText::new(t("Anima a mano, cuadro a cuadro. Se instala solo para tu usuario y se actualiza solo.", "Hand-drawn, frame-by-frame animation. Installs just for you and keeps itself up to date.")).size(15.0).color(TEXT2));
                        ui.checkbox(&mut self.desktop, RichText::new(t("Crear un acceso directo en el escritorio", "Create a desktop shortcut")).size(14.0).color(TEXT));
                        ui.label(RichText::new(format!("{} {}", t("Carpeta:", "Folder:"), install_dir().display())).size(12.0).color(TEXT2));
                    }
                    Step::Working => {
                        ui.label(RichText::new(t("Instalando…", "Installing…")).size(16.0).color(TEXT));
                        ui.spinner();
                    }
                    Step::Done => {
                        ui.label(RichText::new(t("Trazo está instalado.", "Trazo is installed.")).font(bold(18.0)).color(ACCENT));
                        ui.label(RichText::new(t("Lo encuentras en el menú Inicio. Tus animaciones se guardan aparte y no se borran al actualizar.", "You'll find it in the Start menu. Your animations are kept apart and are never removed by updates.")).size(14.0).color(TEXT2));
                    }
                    Step::Failed(e) => {
                        ui.label(RichText::new(t("No se pudo instalar", "Couldn't install")).font(bold(18.0)).color(DANGER));
                        ui.label(RichText::new(e).size(13.0).color(TEXT2));
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    let (label, enabled) = match step {
                        Step::Ready => (t("Instalar", "Install"), true),
                        Step::Working => (t("Instalando…", "Installing…"), false),
                        Step::Done => (t("Abrir Trazo", "Open Trazo"), true),
                        Step::Failed(_) => (t("Reintentar", "Try again"), true),
                    };
                    let b = egui::Button::new(RichText::new(label).font(bold(16.0)).color(Color32::from_rgb(18, 18, 18))).fill(TEXT).corner_radius(14).min_size(Vec2::new(200.0, 48.0));
                    if ui.add_enabled(enabled, b).clicked() {
                        if step == Step::Done {
                            let _ = std::process::Command::new(install_dir().join("trazo.exe")).spawn();
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        } else {
                            start(self.step.clone(), self.desktop, ui.ctx().clone());
                        }
                    }
                    if PAYLOAD.is_empty() {
                        ui.label(RichText::new("(instalador de prueba sin Trazo dentro)").size(12.0).color(DANGER));
                    }
                    let _ = (SURFACE, ELEVATED);
                });
            });
        }
    }

    fn start(step: Arc<Mutex<Step>>, desktop: bool, ctx: egui::Context) {
        if let Ok(mut s) = step.lock() {
            *s = Step::Working;
        }
        std::thread::spawn(move || {
            let r = install(desktop);
            if let Ok(mut s) = step.lock() {
                *s = match r {
                    Ok(()) => Step::Done,
                    Err(e) => Step::Failed(e),
                };
            }
            ctx.request_repaint();
        });
    }

    fn install(desktop: bool) -> Result<(), String> {
        if PAYLOAD.is_empty() {
            return Err("Este instalador no lleva Trazo dentro.".into());
        }
        let dir = install_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let exe = dir.join("trazo.exe");
        std::fs::write(&exe, PAYLOAD).map_err(|e| {
            if e.raw_os_error() == Some(32) || e.kind() == std::io::ErrorKind::PermissionDenied {
                t("Trazo está abierto: ciérralo y vuelve a intentar.", "Trazo is open: close it and try again.").to_string()
            } else {
                e.to_string()
            }
        })?;
        shortcuts(&exe, &dir, desktop)?;
        register(&exe, &dir)?;
        Ok(())
    }

    fn ps_quote(p: &Path) -> String {
        format!("'{}'", p.display().to_string().replace('\'', "''"))
    }

    fn shortcuts(exe: &Path, dir: &Path, desktop: bool) -> Result<(), String> {
        let mut script = String::from("$ws = New-Object -ComObject WScript.Shell;");
        let mut folders = vec!["Programs"];
        if desktop {
            folders.push("Desktop");
        }
        for f in folders {
            script.push_str(&format!(
                "$s = $ws.CreateShortcut([Environment]::GetFolderPath('{f}') + '\\Trazo.lnk'); $s.TargetPath = {exe}; $s.WorkingDirectory = {dir}; $s.IconLocation = {exe} + ',0'; $s.Description = 'Trazo'; $s.Save();",
                exe = ps_quote(exe),
                dir = ps_quote(dir),
            ));
        }
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }

    fn reg(name: &str, kind: &str, value: &str) -> Result<(), String> {
        let out = std::process::Command::new("reg")
            .args(["add", UNINSTALL_KEY, "/v", name, "/t", kind, "/d", value, "/f"])
            .creation_flags(NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
    }

    /// "Aplicaciones instaladas": name, version, icon and how to uninstall.
    fn register(exe: &Path, dir: &Path) -> Result<(), String> {
        let exe_s = exe.display().to_string();
        reg("DisplayName", "REG_SZ", "Trazo")?;
        reg("DisplayVersion", "REG_SZ", VERSION)?;
        reg("Publisher", "REG_SZ", "Inerza Dynamics")?;
        reg("DisplayIcon", "REG_SZ", &exe_s)?;
        reg("InstallLocation", "REG_SZ", &dir.display().to_string())?;
        reg("UninstallString", "REG_SZ", &format!("\"{exe_s}\" --uninstall"))?;
        reg("URLInfoAbout", "REG_SZ", "https://github.com/hazielcampos/trazo")?;
        reg("NoModify", "REG_DWORD", "1")?;
        reg("NoRepair", "REG_DWORD", "1")?;
        reg("EstimatedSize", "REG_DWORD", &(PAYLOAD.len() / 1024).to_string())?;
        Ok(())
    }
}
