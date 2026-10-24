//! Choosing files to import.
//!
//! On desktop the system file dialog is used. On Android a NativeActivity
//! can't receive the result of the system picker without Java code, so the
//! app has its own browser over shared storage (Downloads, Music, Pictures,
//! DCIM…), using the READ_MEDIA_* permissions, plus the app's own
//! `importar` folder for files that aren't media (brushes).

use crate::i18n::tr;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::RichText;

use crate::app::TrazoApp;
use crate::icons::{Icon, IconButton};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickPurpose {
    Audio,
    Brushes,
    ImageLayer,
    ImageReference,
    ImageSequence,
    Video,
}

impl PickPurpose {
    pub fn title(self) -> &'static str {
        match self {
            PickPurpose::Audio => tr("Importar audio"),
            PickPurpose::Brushes => tr("Importar pinceles"),
            PickPurpose::ImageLayer => tr("Importar imagen"),
            PickPurpose::ImageReference => tr("Imagen de referencia"),
            PickPurpose::ImageSequence => tr("Secuencia de imágenes (rotoscopia)"),
            PickPurpose::Video => tr("Importar video"),
        }
    }

    pub fn exts(self) -> &'static [&'static str] {
        match self {
            PickPurpose::Audio => &crate::panels::audio::AUDIO_EXTS,
            PickPurpose::Brushes => &[trazo_core::brush_io::BRUSH_EXTENSION, "png"],
            PickPurpose::Video => &crate::panels::video_import::VIDEO_EXTS,
            _ => &crate::import::IMAGE_EXTS,
        }
    }

    fn multi(self) -> bool {
        matches!(self, PickPurpose::Audio | PickPurpose::Brushes | PickPurpose::ImageSequence)
    }
}

/// Ask for files; the import runs when the user has chosen them.
pub fn request(app: &mut TrazoApp, ctx: &egui::Context, purpose: PickPurpose) {
    #[cfg(not(target_os = "android"))]
    {
        let files = app.platform.pick_files(purpose.title(), purpose.exts());
        if !files.is_empty() {
            deliver(app, ctx, purpose, files);
        }
    }
    #[cfg(target_os = "android")]
    {
        let _ = ctx;
        let perms = crate::android::media_permissions();
        if !perms.iter().all(|p| crate::android::has_permission(p)) {
            let _ = crate::android::request_permissions(&perms);
        }
        let start = app.file_browser.as_ref().map(|b| b.dir.clone()).unwrap_or_else(|| shared_root().join("Download"));
        app.file_browser = Some(FileBrowser::new(purpose, start));
    }
}

/// Run the import the files were chosen for.
pub fn deliver(app: &mut TrazoApp, ctx: &egui::Context, purpose: PickPurpose, files: Vec<PathBuf>) {
    match purpose {
        PickPurpose::Audio => crate::panels::audio::import_audio_files(app, ctx, files),
        PickPurpose::Brushes => crate::panels::brushes::import_brush_files(app, ctx, files),
        PickPurpose::ImageLayer => crate::import::image_as_layer(app, ctx, files),
        PickPurpose::ImageReference => crate::import::reference_image(app, ctx, files),
        PickPurpose::ImageSequence => crate::import::image_sequence(app, ctx, files),
        PickPurpose::Video => crate::panels::video_import::open(app, ctx, files),
    }
}

/// Root of the user's shared storage on Android.
pub fn shared_root() -> PathBuf {
    PathBuf::from("/storage/emulated/0")
}

pub struct FileBrowser {
    purpose: PickPurpose,
    pub dir: PathBuf,
    selected: BTreeSet<PathBuf>,
    entries: Vec<(PathBuf, bool)>,
    listed_for: Option<PathBuf>,
    error: Option<String>,
}

impl FileBrowser {
    pub fn new(purpose: PickPurpose, dir: PathBuf) -> Self {
        Self { purpose, dir, selected: BTreeSet::new(), entries: Vec::new(), listed_for: None, error: None }
    }

    fn refresh(&mut self) {
        if self.listed_for.as_ref() == Some(&self.dir) {
            return;
        }
        self.listed_for = Some(self.dir.clone());
        self.entries.clear();
        self.error = None;
        let exts = self.purpose.exts();
        match std::fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let p = e.path();
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if name.starts_with('.') {
                        continue;
                    }
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    let ok_ext = p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.iter().any(|e| e.eq_ignore_ascii_case(x)));
                    if is_dir || ok_ext {
                        self.entries.push((p, is_dir));
                    }
                }
                self.entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.file_name().cmp(&b.0.file_name())));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

/// The browser window (Android). Returns nothing; imports run on "Abrir".
pub fn show_browser(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(browser) = app.file_browser.as_mut() else { return };
    browser.refresh();
    let mut open = true;
    let mut chosen: Option<Vec<PathBuf>> = None;
    let mut go_to: Option<PathBuf> = None;
    #[allow(unused_mut)]
    let mut ask_permission = false;
    let import_dir = app.platform.import_dir.clone();
    let purpose = browser.purpose;
    crate::widgets::backdrop(ctx);
    egui::Window::new(purpose.title())
        .order(egui::Order::Foreground)
        .open(&mut open)
        .collapsible(false)
        .default_size([crate::widgets::dialog_width(ctx, 720.0), (ctx.content_rect().height() - 100.0).min(560.0)])
        .max_width(crate::widgets::dialog_width(ctx, 720.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            // Places.
            ui.horizontal_wrapped(|ui| {
                let root = shared_root();
                let places: [(&str, PathBuf); 8] = [
                    (tr("Descargas"), root.join("Download")),
                    (tr("Música"), root.join("Music")),
                    (tr("Grabaciones"), root.join("Recordings")),
                    (tr("Imágenes"), root.join("Pictures")),
                    (tr("Cámara"), root.join("DCIM")),
                    (tr("Películas"), root.join("Movies")),
                    (tr("Documentos"), root.join("Documents")),
                    (tr("Carpeta de la app"), import_dir.clone()),
                ];
                for (label, path) in places {
                    if ui.selectable_label(browser.dir == path, label).clicked() {
                        go_to = Some(path);
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let can_up = browser.dir.parent().is_some_and(|p| p.starts_with(shared_root()) || p.starts_with(&import_dir));
                if ui.add_enabled(can_up, IconButton::new(Icon::Up).keep_text().text(tr("Subir de carpeta"))).clicked() {
                    go_to = browser.dir.parent().map(Path::to_path_buf);
                }
                ui.label(RichText::new(browser.dir.display().to_string()).small().weak());
            });

            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                if let Some(e) = &browser.error {
                    ui.label(RichText::new(tf!("No se puede abrir esta carpeta ({}).", crate::i18n::tr_err(&e.to_string()))).weak());
                }
                if browser.entries.iter().all(|(_, d)| *d) {
                    ui.label(RichText::new(tr("No hay archivos compatibles aquí.")).weak());
                }
                for (path, is_dir) in browser.entries.clone() {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if is_dir {
                        if ui.add(IconButton::new(Icon::Open).keep_text().text(&name)).clicked() {
                            go_to = Some(path.clone());
                        }
                    } else {
                        let sel = browser.selected.contains(&path);
                        let r = ui.add_sized([ui.available_width(), 36.0], egui::Button::new(name).selected(sel));
                        if r.clicked() {
                            if purpose.multi() {
                                if !browser.selected.remove(&path) {
                                    browser.selected.insert(path.clone());
                                }
                            } else {
                                browser.selected.clear();
                                browser.selected.insert(path.clone());
                            }
                        }
                        if r.double_clicked() {
                            chosen = Some(vec![path.clone()]);
                        }
                    }
                }
            });
            ui.separator();
            #[cfg(target_os = "android")]
            {
                let perms = crate::android::media_permissions();
                if !perms.iter().all(|p| crate::android::has_permission(p)) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr("Para ver tu música e imágenes, Trazo necesita permiso."));
                        if ui.button(tr("Dar permiso")).clicked() {
                            ask_permission = true;
                        }
                    });
                }
            }
            if purpose == PickPurpose::Brushes {
                ui.label(RichText::new(tr("Los pinceles .animbrush deben estar en «Carpeta de la app» (cópialos ahí con el cable USB o un gestor de archivos). Las imágenes PNG pueden estar en cualquier carpeta.")).small().weak());
            }
            ui.horizontal(|ui| {
                let n = browser.selected.len();
                let label = if n > 1 { tf!("Abrir {} archivos", n) } else { tr("Abrir").to_string() };
                if ui.add_enabled(n > 0, IconButton::new(Icon::Check).keep_text().text(&label).selected(n > 0)).clicked() {
                    chosen = Some(browser.selected.iter().cloned().collect());
                }
                if purpose.multi() && ui.button(tr("Elegir todos")).clicked() {
                    browser.selected = browser.entries.iter().filter(|(_, d)| !d).map(|(p, _)| p.clone()).collect();
                }
            });
        });
    if let Some(dir) = go_to {
        if let Some(b) = app.file_browser.as_mut() {
            b.dir = dir;
            b.selected.clear();
        }
    }
    if ask_permission {
        #[cfg(target_os = "android")]
        {
            let _ = crate::android::request_permissions(&crate::android::media_permissions());
        }
        if let Some(b) = app.file_browser.as_mut() {
            b.listed_for = None; // list again once granted
        }
    }
    // Re-list periodically while waiting for a permission answer.
    if let Some(b) = app.file_browser.as_mut() {
        if b.entries.is_empty() {
            b.listed_for = None;
            ctx.request_repaint_after(std::time::Duration::from_millis(700));
        }
    }
    if let Some(files) = chosen {
        app.file_browser = None;
        deliver(app, ctx, purpose, files);
    } else if !open {
        app.file_browser = None;
    }
}
