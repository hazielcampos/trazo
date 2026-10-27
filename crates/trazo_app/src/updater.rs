//! Updates on PC (Windows and Linux): Trazo checks GitHub Releases when it
//! opens, downloads a newer build in the background and offers "Reiniciar para
//! actualizar". The app settings can turn this off and install any earlier
//! release. Android updates come from Google Play, so none of this runs there.
//!
//! How the swap works: a running exe can be renamed (not deleted) on Windows,
//! so on restart the current file becomes `trazo.old.exe`, the downloaded one
//! takes its place and is launched; the old file is removed on the next start.
//! Debug builds never check unless `TRAZO_UPDATES=1`, so a test build is never
//! replaced by a release.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Align2, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use sha2::Digest;

use crate::app::TrazoApp;
use crate::i18n::tr;
use crate::theme::{self, font, ELEVATED, SEPARATOR, SURFACE, TEXT, TEXT2, TEXT3, W};

const RELEASES_URL: &str = "https://api.github.com/repos/hazielcampos/trazo/releases?per_page=40";
const SETTINGS_FILE: &str = "actualizaciones.txt";

pub type Version = (u32, u32, u32);

pub fn current_version() -> Version {
    // Test builds can pretend to be older to try the whole update.
    if cfg!(debug_assertions) {
        if let Some(v) = std::env::var("TRAZO_VERSION").ok().and_then(|v| parse_version(&v)) {
            return v;
        }
    }
    parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let mut it = s.split(['.', '-', '+']).map(|p| p.parse::<u32>());
    Some((it.next()?.ok()?, it.next()?.ok()?, it.next().and_then(|r| r.ok()).unwrap_or(0)))
}

pub fn version_text(v: Version) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

#[derive(Clone)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Clone)]
pub struct Release {
    pub version: Version,
    pub date: String,
    pub prerelease: bool,
    /// The build for this computer, if the release has one.
    pub asset: Option<Asset>,
}

#[derive(Clone)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Downloading { version: Version, done: u64, total: u64 },
    Ready { version: Version, path: PathBuf },
    Error(String),
}

#[derive(Default)]
struct Shared {
    status: Option<Status>,
    releases: Option<Result<Vec<Release>, String>>,
    busy: bool,
}

pub struct Updater {
    dir: PathBuf,
    data_dir: PathBuf,
    /// Check (and download) when Trazo opens.
    pub auto: bool,
    shared: Arc<Mutex<Shared>>,
    started: bool,
    /// The "restart to update" banner was closed for this session.
    pub dismissed: bool,
}

impl Updater {
    pub fn new(data_dir: &Path) -> Self {
        let auto = std::fs::read_to_string(data_dir.join(SETTINGS_FILE)).map(|s| !s.contains("auto=no")).unwrap_or(true);
        let dir = data_dir.join("actualizaciones");
        cleanup_after_update(&dir);
        Self { dir, data_dir: data_dir.to_path_buf(), auto, shared: Arc::default(), started: false, dismissed: false }
    }

    pub fn set_auto(&mut self, on: bool) {
        self.auto = on;
        let _ = std::fs::write(self.data_dir.join(SETTINGS_FILE), if on { "auto=si\n" } else { "auto=no\n" });
    }

    fn enabled_here() -> bool {
        !cfg!(debug_assertions) || std::env::var_os("TRAZO_UPDATES").is_some()
    }

    pub fn status(&self) -> Status {
        self.shared.lock().ok().and_then(|s| s.status.clone()).unwrap_or(Status::Idle)
    }

    pub fn releases(&self) -> Option<Result<Vec<Release>, String>> {
        self.shared.lock().ok().and_then(|s| s.releases.clone())
    }

    pub fn busy(&self) -> bool {
        self.shared.lock().map(|s| s.busy).unwrap_or(false)
    }

    /// Once per run: look for a newer release and download it quietly.
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.started {
            return;
        }
        self.started = true;
        if self.auto && Self::enabled_here() {
            self.check(ctx, true);
        }
    }

    /// Fetch the list of releases; with `download`, get the newest one if it is
    /// newer than this build.
    pub fn check(&self, ctx: &egui::Context, download: bool) {
        if self.busy() {
            return;
        }
        let (shared, ctx, dir) = (self.shared.clone(), ctx.clone(), self.dir.clone());
        set(&shared, |s| {
            s.busy = true;
            s.status = Some(Status::Checking);
        });
        std::thread::spawn(move || {
            let list = fetch_releases();
            set(&shared, |s| s.releases = Some(list.clone()));
            let newest = list.as_ref().ok().and_then(|l| l.iter().filter(|r| !r.prerelease && r.asset.is_some()).max_by_key(|r| r.version).cloned());
            match (list, newest) {
                (Err(e), _) => set(&shared, |s| s.status = Some(Status::Error(e))),
                (Ok(_), Some(r)) if r.version > current_version() && download => download_release(&shared, &ctx, &dir, &r),
                (Ok(_), _) => set(&shared, |s| s.status = Some(Status::UpToDate)),
            }
            set(&shared, |s| s.busy = false);
            ctx.request_repaint();
        });
    }

    /// Download a given release (newer or older) to install on restart.
    pub fn install(&self, ctx: &egui::Context, release: Release) {
        if self.busy() {
            return;
        }
        let (shared, ctx, dir) = (self.shared.clone(), ctx.clone(), self.dir.clone());
        set(&shared, |s| s.busy = true);
        std::thread::spawn(move || {
            download_release(&shared, &ctx, &dir, &release);
            set(&shared, |s| s.busy = false);
            ctx.request_repaint();
        });
    }

    pub fn ready(&self) -> Option<(Version, PathBuf)> {
        match self.status() {
            Status::Ready { version, path } => Some((version, path)),
            _ => None,
        }
    }
}

fn set(shared: &Arc<Mutex<Shared>>, f: impl FnOnce(&mut Shared)) {
    if let Ok(mut s) = shared.lock() {
        f(&mut s);
    }
}

/// Which file of a release is the build for this computer.
fn asset_for_this_pc(name: &str) -> bool {
    let n = name.to_lowercase();
    if cfg!(windows) {
        n.ends_with("-windows-x64.exe")
    } else if cfg!(target_os = "linux") {
        n.ends_with("-linux-x64") || n.ends_with("-linux-x86_64")
    } else {
        false
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(10)).timeout_read(std::time::Duration::from_secs(60)).build()
}

fn fetch_releases() -> Result<Vec<Release>, String> {
    let body = agent()
        .get(RELEASES_URL)
        .set("User-Agent", concat!("Trazo/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in json.as_array().into_iter().flatten() {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let Some(version) = r["tag_name"].as_str().and_then(parse_version) else { continue };
        let asset = r["assets"].as_array().into_iter().flatten().find(|a| a["name"].as_str().is_some_and(asset_for_this_pc)).map(|a| Asset {
            url: a["browser_download_url"].as_str().unwrap_or_default().to_string(),
            size: a["size"].as_u64().unwrap_or(0),
            sha256: a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(|d| d.to_lowercase()),
        });
        out.push(Release {
            version,
            date: r["published_at"].as_str().unwrap_or_default().chars().take(10).collect(),
            prerelease: r["prerelease"].as_bool().unwrap_or(false),
            asset,
        });
    }
    out.sort_by(|a, b| b.version.cmp(&a.version));
    Ok(out)
}

fn download_release(shared: &Arc<Mutex<Shared>>, ctx: &egui::Context, dir: &Path, r: &Release) {
    let Some(asset) = r.asset.clone() else {
        set(shared, |s| s.status = Some(Status::Error(tr("Esta versión no tiene descarga para este sistema").to_string())));
        return;
    };
    let version = r.version;
    let result = (|| -> Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let name = if cfg!(windows) { format!("trazo-{}.exe", version_text(version)) } else { format!("trazo-{}", version_text(version)) };
        let path = dir.join(&name);
        let part = dir.join(format!("{name}.part"));
        let resp = agent().get(&asset.url).set("User-Agent", concat!("Trazo/", env!("CARGO_PKG_VERSION"))).call().map_err(|e| e.to_string())?;
        let total = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(asset.size);
        let mut reader = resp.into_reader();
        let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut hasher = sha2::Sha256::new();
        let (mut done, mut buf) = (0u64, vec![0u8; 1 << 16]);
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            hasher.update(&buf[..n]);
            done += n as u64;
            set(shared, |s| s.status = Some(Status::Downloading { version, done, total }));
            ctx.request_repaint();
        }
        drop(file);
        if asset.size > 0 && done != asset.size {
            return Err(tr("La descarga quedó incompleta").to_string());
        }
        if let Some(want) = &asset.sha256 {
            let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if &got != want {
                let _ = std::fs::remove_file(&part);
                return Err(tr("La descarga está dañada (no coincide la firma)").to_string());
            }
        }
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
        Ok(path)
    })();
    set(shared, |s| {
        s.status = Some(match result {
            Ok(path) => Status::Ready { version, path },
            Err(e) => Status::Error(e),
        })
    });
}

/// Remove what a previous update left behind.
fn cleanup_after_update(dir: &Path) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let _ = std::fs::remove_file(e.path());
    }
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_extension(if cfg!(windows) { "old.exe" } else { "old" })
}

/// Put the downloaded build in place of this one and start it. Called while
/// Trazo closes (after the project was saved).
pub fn swap_and_launch(new: &Path) -> Result<(), String> {
    let cur = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = old_path(&cur);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&cur, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::copy(new, &cur) {
        let _ = std::fs::rename(&old, &cur);
        return Err(e.to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&cur, std::fs::Permissions::from_mode(0o755));
    }
    std::process::Command::new(&cur).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

// ---- UI ------------------------------------------------------------------------------

/// "Trazo X está listo · Reiniciar para actualizar", at the top of the window.
pub fn banner(app: &mut TrazoApp, ctx: &egui::Context) {
    app.updater.start(ctx);
    let Some((version, path)) = app.updater.ready() else { return };
    if app.updater.dismissed {
        return;
    }
    // Test builds: restart on their own (to try the whole swap from a script).
    if cfg!(debug_assertions) && std::env::var_os("TRAZO_REINICIAR").is_some() && app.restart_to.is_none() {
        app.restart_to = Some(path.clone());
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let newer = version > current_version();
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("update_banner")).order(egui::Order::Foreground).pivot(Align2::CENTER_TOP).fixed_pos(Pos2::new(screen.center().x, screen.top() + 52.0)).show(ctx, |ui| {
        egui::Frame::new().fill(ELEVATED).corner_radius(16).stroke(Stroke::new(1.0, Color32::from_white_alpha(18))).shadow(theme::island_shadow()).inner_margin(egui::Margin { left: 16, right: 6, top: 6, bottom: 6 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let text = if newer { tf!("Trazo {} está listo", version_text(version)) } else { tf!("Trazo {} descargado", version_text(version)) };
                ui.label(RichText::new(text).font(font(14.0, W::Semi)).color(TEXT));
                let label = tr("Reiniciar para actualizar");
                let g = ui.painter().layout_no_wrap(label.to_string(), font(13.0, W::Bold), theme::ON_ACCENT);
                let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 28.0, 36.0), Sense::click());
                ui.painter().rect_filled(r, 12.0, if resp.is_pointer_button_down_on() { theme::PRIMARY.gamma_multiply(0.85) } else { theme::PRIMARY });
                ui.painter().galley(Pos2::new(r.left() + 14.0, r.center().y - g.size().y / 2.0), g, theme::ON_ACCENT);
                if resp.on_hover_text(tr("Guarda tu trabajo, cierra Trazo y lo abre con la versión nueva")).clicked() {
                    app.restart_to = Some(path.clone());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if crate::widgets::icon_button(ui, crate::icons::Icon::Close, tr("Más tarde")).clicked() {
                    app.updater.dismissed = true;
                }
            });
        });
    });
}

/// "Ajustes de Trazo": language and updates (PC).
pub fn settings_dialog(app: &mut TrazoApp, ctx: &egui::Context) {
    if !app.ui.app_settings {
        return;
    }
    let mut open = true;
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Ajustes de Trazo"), 460.0).id(egui::Id::unique("app_settings")).open(&mut open).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(RichText::new(tr("Idioma")).font(font(13.0, W::Semi)).color(TEXT2));
        if crate::widgets::choice_row(ui, false, crate::i18n::lang().native_name(), tr("Cambiar el idioma de la app")).clicked() {
            app.ui.language_dialog = Some(false);
        }
        ui.add_space(6.0);
        ui.label(RichText::new(tr("Actualizaciones")).font(font(13.0, W::Semi)).color(TEXT2));
        let mut auto = app.updater.auto;
        if crate::widgets::switch_row(ui, tr("Buscar e instalar actualizaciones al abrir"), &mut auto) {
            app.updater.set_auto(auto);
        }
        ui.label(RichText::new(tf!("Tienes la versión {}.", version_text(current_version()))).font(font(13.0, W::Regular)).color(TEXT2));
        // What is happening now.
        let status = match app.updater.status() {
            Status::Idle => String::new(),
            Status::Checking => tr("Buscando versiones…").to_string(),
            Status::UpToDate => tr("Tienes la versión más reciente.").to_string(),
            Status::Downloading { version, done, total } => {
                let pct = if total > 0 { done * 100 / total } else { 0 };
                tf!("Descargando Trazo {}… {} %", version_text(version), pct)
            }
            Status::Ready { version, .. } => tf!("Trazo {} está listo: reinicia para usarlo.", version_text(version)),
            Status::Error(e) => tf!("No se pudo actualizar: {}", crate::i18n::tr_err(&e)),
        };
        if !status.is_empty() {
            ui.label(RichText::new(status).font(font(13.0, W::Medium)).color(TEXT));
        }
        if let Some((_, path)) = app.updater.ready() {
            if crate::widgets::primary_button(ui, tr("Reiniciar para actualizar")).clicked() {
                app.restart_to = Some(path);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if crate::widgets::secondary_button(ui, tr("Buscar ahora"), 44.0).clicked() {
            app.updater.check(ctx, true);
        }
        ui.add_space(6.0);
        // Every release: go back to one.
        egui::CollapsingHeader::new(RichText::new(tr("Versiones anteriores")).font(font(13.0, W::Semi))).id_salt("update_versions").default_open(true).show(ui, |ui| {
            match app.updater.releases() {
                None => {
                    if !app.updater.busy() {
                        app.updater.check(ctx, false);
                    }
                    ui.label(RichText::new(tr("Buscando versiones…")).color(TEXT2));
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(tf!("No se pudo consultar GitHub: {}", crate::i18n::tr_err(&e))).color(theme::DANGER));
                }
                Some(Ok(list)) => {
                    ui.label(RichText::new(tr("Volver a una versión anterior desactiva las actualizaciones automáticas, para que no se reemplace sola.")).font(font(12.0, W::Regular)).color(TEXT2));
                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        for r in list {
                            let current = r.version == current_version();
                            ui.horizontal(|ui| {
                                ui.set_min_height(40.0);
                                let mut t = version_text(r.version);
                                if r.prerelease {
                                    t.push_str(tr(" (prueba)"));
                                }
                                ui.label(RichText::new(t).font(font(14.0, W::Semi)).color(TEXT));
                                ui.label(RichText::new(&r.date).font(font(12.0, W::Regular)).color(TEXT3));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if current {
                                        ui.label(RichText::new(tr("La que tienes")).font(font(12.0, W::Medium)).color(theme::SAVED));
                                    } else if r.asset.is_none() {
                                        ui.label(RichText::new(tr("Sin descarga para este sistema")).font(font(12.0, W::Regular)).color(TEXT3));
                                    } else if ui.add_enabled(!app.updater.busy(), egui::Button::new(tr("Instalar")).fill(SURFACE).stroke(Stroke::new(1.0, SEPARATOR)).min_size(Vec2::new(90.0, 32.0))).clicked() {
                                        if r.version < current_version() {
                                            app.updater.set_auto(false);
                                        }
                                        app.updater.install(ctx, r.clone());
                                    }
                                });
                            });
                        }
                    });
                }
            }
        });
    });
    if !open {
        app.ui.app_settings = false;
    }
}
