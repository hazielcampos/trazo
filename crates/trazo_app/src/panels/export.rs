//! Export dialog: social-network presets, format and options with a live
//! preview, background export with progress / time left / cancel, then save
//! to the gallery (Android) or a file (desktop) and share.

use crate::i18n::tr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use trazo_core::export::{estimate_bytes, plan, render_rgba, Aspect, ExportFormat, ExportSettings, Fit, OutSize, CANCELLED};
use trazo_core::Rgba;
use egui::{Color32, RichText};

use crate::app::TrazoApp;
use crate::icons::{Icon, IconButton};

/// True while an export thread is writing (Android waits for it before exiting).
pub static EXPORT_ACTIVE: AtomicBool = AtomicBool::new(false);

struct Job {
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Done, String>>,
    format: ExportFormat,
    started: Instant,
}

#[derive(Clone)]
pub struct Done {
    pub path: PathBuf,
    /// Content URI in the gallery (Android).
    pub uri: Option<String>,
    pub mime: &'static str,
    pub place: String,
}

pub struct ExportUi {
    pub open: bool,
    pub settings: ExportSettings,
    use_range: bool,
    file_name: String,
    job: Option<Job>,
    pub last: Option<Done>,
    preview: Option<(PreviewKey, egui::TextureHandle)>,
    /// Phone: the rest of the settings are shown.
    more: bool,
    /// Share as soon as the export finishes ("Guardar y compartir").
    share_after: bool,
}

#[derive(Clone, PartialEq)]
struct PreviewKey {
    settings: String,
    content: u64,
    frame: usize,
}

impl Default for ExportUi {
    fn default() -> Self {
        let mut settings = ExportSettings::default();
        if !mp4_available() {
            settings.format = ExportFormat::Gif;
        }
        Self { open: false, settings, use_range: false, file_name: String::new(), job: None, last: None, preview: None, more: false, share_after: false }
    }
}

impl ExportUi {
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Settings kept between sessions.
    pub fn saved(&self) -> ExportSettings {
        ExportSettings { range: None, frame: 0, ..self.settings }
    }

    pub fn restore(&mut self, s: ExportSettings) {
        self.settings = s;
        if s.format == ExportFormat::Mp4 && !mp4_available() {
            self.settings.format = ExportFormat::Gif;
        }
    }
}

/// MP4 uses the platform encoder (Android MediaCodec, Windows Media Foundation).
pub fn mp4_available() -> bool {
    cfg!(any(target_os = "android", windows))
}

const FORMATS: [ExportFormat; 6] = [ExportFormat::Mp4, ExportFormat::Gif, ExportFormat::Png, ExportFormat::PngSequence, ExportFormat::Avi, ExportFormat::Wav];

struct Preset {
    label: &'static str,
    hint: &'static str,
    apply: fn(&mut ExportSettings),
}

const PRESETS: [Preset; 5] = [
    Preset {
        label: "TikTok / Reels",
        hint: "9:16 vertical, 1080p, 30 fps",
        apply: |s| {
            s.format = ExportFormat::Mp4;
            s.aspect = Aspect::Vertical;
            s.size = OutSize::Short(1080);
            s.out_fps = Some(30);
        },
    },
    Preset {
        label: "Instagram",
        hint: "4:5, 1080p, 30 fps",
        apply: |s| {
            s.format = ExportFormat::Mp4;
            s.aspect = Aspect::Portrait;
            s.size = OutSize::Short(1080);
            s.out_fps = Some(30);
        },
    },
    Preset {
        label: "Cuadrado",
        hint: "1:1, 1080p, 30 fps",
        apply: |s| {
            s.format = ExportFormat::Mp4;
            s.aspect = Aspect::Square;
            s.size = OutSize::Short(1080);
            s.out_fps = Some(30);
        },
    },
    Preset {
        label: "YouTube",
        hint: "16:9, 1080p, 30 fps",
        apply: |s| {
            s.format = ExportFormat::Mp4;
            s.aspect = Aspect::Wide;
            s.size = OutSize::Short(1080);
            s.out_fps = Some(30);
        },
    },
    Preset {
        label: "GIF WhatsApp",
        hint: "GIF ligero a 480p",
        apply: |s| {
            s.format = ExportFormat::Gif;
            s.aspect = Aspect::Canvas;
            s.size = OutSize::Short(480);
            s.quality = 70;
        },
    },
];

fn human_bytes(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{} GB", crate::i18n::num((b as f64 / (1u64 << 30) as f64) as f32, 1)),
        b if b >= 1 << 20 => format!("{} MB", crate::i18n::num((b as f64 / (1u64 << 20) as f64) as f32, 1)),
        b => format!("{} KB", (b / 1024).max(1)),
    }
}

/// "nombre_AAAAMMDD_HHMM" from the system clock (UTC; no time-zone crate needed).
impl ExportUi {
    /// Another project was opened: its own name is suggested next time.
    pub fn reset_name(&mut self) {
        self.file_name.clear();
    }
}

fn default_file_name(project: &str) -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86_400) as i64;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (hh, mm) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    format!("{}_{y:04}{m:02}{d:02}_{hh:02}{mm:02}", safe_name(project))
}

fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        "animacion".into()
    } else {
        s
    }
}

pub fn show(app: &mut TrazoApp, ctx: &egui::Context) {
    poll(app, ctx);
    if !app.export_ui.open {
        return;
    }
    if app.export_ui.file_name.is_empty() {
        app.export_ui.file_name = default_file_name(&app.doc.project.name);
    }
    let mut open = true;
    let (mut start, mut cancel, mut share) = (false, false, false);
    let n_frames = app.doc.project.frames.len();
    let range = app.playback.range.map(|_| app.playback.bounds(n_frames));
    let mut settings = app.export_ui.settings;
    settings.range = if app.export_ui.use_range { range } else { None };
    settings.frame = app.doc.frame;
    let preview = preview_texture(app, ctx, &settings);

    if crate::widgets::SizeClass::of(ctx) == crate::widgets::SizeClass::Phone {
        phone_sheet(app, ctx, settings, preview, range, &mut start, &mut cancel, &mut open);
    } else if crate::widgets::SizeClass::of(ctx) != crate::widgets::SizeClass::Wide {
        phone_screen(app, ctx, settings, preview, range, &mut start, &mut cancel, &mut share, &mut open);
    } else {
    crate::widgets::backdrop(ctx);
    egui::Window::new(tr("Exportar"))
        .order(egui::Order::Foreground)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(crate::widgets::dialog_width(ctx, 620.0))
        .max_width(crate::widgets::dialog_width(ctx, 620.0))
        .max_height(ctx.content_rect().height() - 80.0)
        .vscroll(true)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let busy = app.export_ui.job.is_some();
            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr("Atajos")).strong());
                    for p in &PRESETS {
                        let enabled = mp4_available() || !matches!(p.label, "TikTok / Reels" | "Instagram" | "Cuadrado" | "YouTube");
                        if ui.add_enabled(enabled, egui::Button::new(tr(p.label))).on_hover_text(tr(p.hint)).clicked() {
                            (p.apply)(&mut app.export_ui.settings);
                        }
                    }
                });
                ui.separator();
                ui.columns(2, |cols| {
                    let ui = &mut cols[0];
                    let s = &mut app.export_ui.settings;
                    ui.label(RichText::new(tr("Formato")).strong());
                    for f in FORMATS {
                        let available = f != ExportFormat::Mp4 || mp4_available();
                        let label = if available { tr(f.label()).to_string() } else { tf!("{} (en Android)", tr(f.label())) };
                        ui.add_enabled_ui(available, |ui| {
                            ui.radio_value(&mut s.format, f, label);
                        });
                    }
                    let ui = &mut cols[1];
                    if let Some(tex) = &preview {
                        let size = tex.size_vec2();
                        let scale = (240.0 / size.x).min(170.0 / size.y);
                        ui.add(egui::Image::new((tex.id(), size * scale)).bg_fill(Color32::from_gray(40)));
                    }
                });
                let s = &mut app.export_ui.settings;
                ui.separator();
                if s.format.has_video() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr("Forma"));
                        for a in Aspect::ALL {
                            ui.selectable_value(&mut s.aspect, a, tr(a.label()));
                        }
                    });
                    if s.aspect != Aspect::Canvas {
                        ui.horizontal_wrapped(|ui| {
                            ui.radio_value(&mut s.fit, Fit::Pad, tr("Todo visible, con bandas"));
                            ui.radio_value(&mut s.fit, Fit::Crop, tr("Llenar recortando"));
                            if s.fit == Fit::Pad {
                                let c = s.pad_color.to_rgba8();
                                let mut c32 = Color32::from_rgb(c[0], c[1], c[2]);
                                if egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::Opaque).on_hover_text(tr("Color de las bandas")).changed() {
                                    s.pad_color = Rgba::from_rgba8([c32.r(), c32.g(), c32.b(), 255]);
                                }
                            }
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr("Tamaño"));
                        for (label, v) in [(tr("Original"), OutSize::Percent(1.0)), ("1080p", OutSize::Short(1080)), ("720p", OutSize::Short(720)), ("480p", OutSize::Short(480)), ("50 %", OutSize::Percent(0.5))] {
                            ui.selectable_value(&mut s.size, v, label);
                        }
                    });
                }
                if s.format.is_timed_video() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr("Velocidad de salida"));
                        ui.selectable_value(&mut s.out_fps, None, tf!("Del proyecto ({} fps)", app.doc.project.canvas.fps));
                        for f in [24, 30, 60] {
                            ui.selectable_value(&mut s.out_fps, Some(f), format!("{f} fps"));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Repeticiones"));
                        ui.add(egui::DragValue::new(&mut s.loops).range(1..=20)).on_hover_text(tr("Útil para animaciones cortas en redes sociales"));
                    });
                }
                if s.format != ExportFormat::Png {
                    if let Some((a, b)) = range {
                        ui.checkbox(&mut app.export_ui.use_range, tf!("Solo el rango de reproducción ({}–{})", a + 1, b + 1));
                    }
                }
                let s = &mut app.export_ui.settings;
                if s.format.supports_alpha() {
                    ui.checkbox(&mut s.background, tr("Incluir el fondo (desmarcado = transparente)"));
                }
                if matches!(s.format, ExportFormat::Mp4 | ExportFormat::Avi) {
                    ui.checkbox(&mut s.include_audio, tr("Incluir el audio"));
                }
                if matches!(s.format, ExportFormat::Mp4 | ExportFormat::Gif | ExportFormat::Avi) {
                    let mut q = s.quality as i32;
                    if ui.add(egui::Slider::new(&mut q, 10..=100).text(tr("Calidad"))).changed() {
                        s.quality = q as u8;
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(tr("Nombre"));
                    ui.add(egui::TextEdit::singleline(&mut app.export_ui.file_name).desired_width(260.0));
                    ui.label(format!(".{}", app.export_ui.settings.format.extension()));
                });
            });

            let pl = plan(&app.doc.project, &settings);
            let mut info = if settings.format.has_video() {
                format!("{}×{} px · {} fps · {} s", pl.width, pl.height, pl.fps, crate::i18n::num(pl.duration_secs() as f32, 1))
            } else {
                tf!("{} s de audio", crate::i18n::num(pl.duration_secs() as f32, 1))
            };
            if settings.format == ExportFormat::Png {
                info = tf!("{}×{} px · fotograma {}", pl.width, pl.height, app.doc.frame + 1);
            }
            info.push_str(&format!(" · ≈ {}", human_bytes(estimate_bytes(&app.doc.project, &settings))));
            ui.label(RichText::new(info).weak());
            if settings.format == ExportFormat::Wav && app.doc.project.audio.is_empty() {
                ui.label(RichText::new(tr("El proyecto no tiene audio: el WAV saldrá en silencio.")).color(Color32::from_rgb(240, 190, 80)));
            }
            ui.separator();

            if let Some(job) = &app.export_ui.job {
                let p = f32::from_bits(job.progress.load(Ordering::Relaxed));
                let elapsed = job.started.elapsed().as_secs_f32();
                let eta = if p > 0.03 { tf!(" · quedan ≈ {} s", format!("{:.0}", elapsed * (1.0 - p) / p)) } else { String::new() };
                ui.add(egui::ProgressBar::new(p).show_percentage().text(tf!("Exportando {}", tr(job.format.label())) + &eta));
                ui.label(RichText::new(tr("Puedes seguir dibujando; no cierres la app hasta que termine.")).small().weak());
                if ui.add(IconButton::new(Icon::Close).text(tr("Cancelar"))).clicked() {
                    cancel = true;
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            } else {
                ui.horizontal(|ui| {
                    if ui.add(IconButton::new(Icon::Export).text(tr("Exportar")).selected(true)).clicked() {
                        start = true;
                    }
                    if let Some(done) = &app.export_ui.last {
                        if done.uri.is_some() && ui.add(IconButton::new(Icon::ArrowRight).text(tr("Compartir"))).on_hover_text(tr("WhatsApp, TikTok, Instagram, correo…")).clicked() {
                            share = true;
                        }
                        ui.label(RichText::new(tf!("Último: {}", done.place)).small().weak());
                    }
                });
            }
        });
    }
    if start {
        start_export(app, ctx, settings);
    }
    if cancel {
        if let Some(job) = &app.export_ui.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
    if share {
        share_last(app, ctx);
    }
    if !open && app.export_ui.job.is_none() {
        app.export_ui.open = false;
        app.export_ui.file_name.clear();
        app.export_ui.preview = None;
    }
}

/// Small render of the current frame with the chosen shape and size.
fn preview_texture(app: &mut TrazoApp, ctx: &egui::Context, settings: &ExportSettings) -> Option<egui::TextureHandle> {
    if !settings.format.has_video() {
        return None;
    }
    let key = PreviewKey { settings: format!("{:?}", (settings.aspect, settings.fit, settings.pad_color, settings.background, settings.format)), content: app.doc.content_version, frame: app.doc.frame };
    if let Some((k, t)) = &app.export_ui.preview {
        if *k == key {
            return Some(t.clone());
        }
    }
    let small = ExportSettings { size: OutSize::Short(180), ..*settings };
    let pl = plan(&app.doc.project, &small);
    let rgba = render_rgba(&app.doc.project, app.doc.frame, &pl, settings.background || !settings.format.supports_alpha());
    let img = egui::ColorImage::from_rgba_unmultiplied([pl.width as usize, pl.height as usize], &rgba);
    let tex = ctx.load_texture("export_preview", img, egui::TextureOptions::LINEAR);
    app.export_ui.preview = Some((key, tex.clone()));
    Some(tex)
}

fn start_export(app: &mut TrazoApp, ctx: &egui::Context, settings: ExportSettings) {
    app.stop_playback();
    app.canvas.select.commit(&mut app.doc);
    let format = settings.format;
    let file_name = format!("{}.{}", safe_name(&app.export_ui.file_name), format.extension());

    // Desktop: ask where to save first. Android: render to a temp file, then publish to the gallery.
    #[cfg(not(target_os = "android"))]
    let path = match app.platform.save_file(tr("Exportar"), &file_name, format.extension()) {
        Some(p) => p,
        None => return,
    };
    #[cfg(target_os = "android")]
    let path = app.platform.export_dir.join(&file_name);

    let project = app.doc.project.clone(); // cheap: tiles and audio are shared
    let progress = Arc::new(AtomicU32::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (p2, c2, ctx2) = (progress.clone(), cancel.clone(), ctx.clone());
    EXPORT_ACTIVE.store(true, Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("exportar".into()).spawn(move || {
        let mut report = |v: f32| {
            p2.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            ctx2.request_repaint();
            !c2.load(Ordering::Relaxed)
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_export(&project, &settings, &path, &file_name, &mut report)))
            .unwrap_or_else(|_| Err(tr("error interno al exportar").into()));
        if result.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        let _ = tx.send(result);
        EXPORT_ACTIVE.store(false, Ordering::SeqCst);
        ctx2.request_repaint();
    });
    if spawned.is_err() {
        EXPORT_ACTIVE.store(false, Ordering::SeqCst);
        app.toast(ctx, tr("No se pudo iniciar la exportación"));
        return;
    }
    app.export_ui.job = Some(Job { progress, cancel, rx, format, started: Instant::now() });
}

fn run_export(
    project: &trazo_core::Project,
    settings: &ExportSettings,
    path: &std::path::Path,
    file_name: &str,
    report: &mut dyn FnMut(f32) -> bool,
) -> Result<Done, String> {
    let format = settings.format;
    match format {
        #[cfg(target_os = "android")]
        ExportFormat::Mp4 => crate::android_mp4::export_mp4(project, settings, path, report)?,
        #[cfg(windows)]
        ExportFormat::Mp4 => crate::windows_mp4::export_mp4(project, settings, path, report)?,
        _ => trazo_core::export::export(project, settings, path, report).map_err(|e| e.to_string())?,
    }
    #[cfg(target_os = "android")]
    {
        use crate::android::SharedKind;
        let kind = match format {
            ExportFormat::Mp4 => SharedKind::Video,
            ExportFormat::Gif | ExportFormat::Png => SharedKind::Image,
            ExportFormat::Wav => SharedKind::Audio,
            _ => SharedKind::Download,
        };
        match crate::android::save_to_shared(path, file_name, format.mime(), kind) {
            Ok(uri) => {
                let _ = std::fs::remove_file(path);
                Ok(Done { path: path.to_path_buf(), uri: Some(uri), mime: format.mime(), place: kind.folder().to_string() })
            }
            Err(e) => {
                log::warn!("no se pudo publicar en la galería: {e}");
                Ok(Done { path: path.to_path_buf(), uri: None, mime: format.mime(), place: path.display().to_string() })
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = file_name;
        Ok(Done { path: path.to_path_buf(), uri: None, mime: format.mime(), place: path.display().to_string() })
    }
}

fn poll(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(job) = &app.export_ui.job else { return };
    let result = match job.rx.try_recv() {
        Ok(r) => r,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => Err(tr("la exportación se detuvo").into()),
    };
    app.export_ui.job = None;
    match result {
        Ok(done) => {
            app.toast(ctx, tf!("Exportado en {}", done.place));
            app.export_ui.last = Some(done);
            app.export_ui.file_name = default_file_name(&app.doc.project.name);
            if std::mem::take(&mut app.export_ui.share_after) {
                share_last(app, ctx);
            }
        }
        Err(e) if e == CANCELLED => {
            app.export_ui.share_after = false;
            app.toast(ctx, tr("Exportación cancelada"));
        }
        Err(e) => app.toast(ctx, tf!("No se pudo exportar: {}", crate::i18n::tr_err(&e.to_string()))),
    }
}

fn share_last(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(done) = app.export_ui.last.clone() else { return };
    #[cfg(target_os = "android")]
    if let Some(uri) = &done.uri {
        if let Err(e) = crate::android::share(uri, done.mime) {
            app.toast(ctx, tf!("No se pudo compartir: {}", crate::i18n::tr_err(&e.to_string())));
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (done, ctx);
    }
}

// ---- phone layout ------------------------------------------------------------------------

/// Short name and shape icon size (w, h) of each preset card.
const CARDS: [(&str, f32, f32); 5] = [("Reels", 13.0, 22.0), ("4:5", 17.0, 21.0), ("1:1", 19.0, 19.0), ("YouTube", 26.0, 15.0), ("GIF", 0.0, 0.0)];

fn preset_selected(i: usize, s: &ExportSettings) -> bool {
    let mut probe = *s;
    (PRESETS[i].apply)(&mut probe);
    probe.format == s.format && probe.aspect == s.aspect && probe.size == s.size && (s.format == ExportFormat::Gif || probe.out_fps == s.out_fps)
}

/// A destination card: little frame of its shape and its name.
fn preset_card(ui: &mut egui::Ui, i: usize, selected: bool, enabled: bool, width: f32) -> egui::Response {
    use crate::theme::{ACCENT, ELEVATED, TEXT, TEXT2};
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 72.0), sense);
    let p = ui.painter();
    p.rect_filled(rect, crate::theme::R_ROW, if selected { crate::theme::accent_soft() } else { ELEVATED });
    if selected {
        p.rect_stroke(rect, crate::theme::R_ROW, egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
    }
    let (name, w, h) = CARDS[i];
    let fg = if !enabled { TEXT2.gamma_multiply(0.5) } else if selected { ACCENT } else { TEXT2 };
    let c = egui::pos2(rect.center().x, rect.top() + 26.0);
    if w > 0.0 {
        p.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(w, h)), 3.0, egui::Stroke::new(2.0, fg), egui::StrokeKind::Middle);
    } else {
        let r = p.text(c, egui::Align2::CENTER_CENTER, "GIF", egui::FontId::proportional(11.0), fg);
        p.rect_stroke(r.expand2(egui::vec2(3.0, 1.0)), 3.0, egui::Stroke::new(1.5, fg), egui::StrokeKind::Middle);
    }
    p.text(egui::pos2(rect.center().x, rect.bottom() - 16.0), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(12.0), if enabled { TEXT } else { TEXT2 });
    resp.on_hover_text(tr(PRESETS[i].hint))
}

/// Settings row with a value and a chevron; returns its response (for a menu).
fn value_row(ui: &mut egui::Ui, label: &str, value: &str) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 46.0), egui::Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(rect, 8.0, crate::theme::SEPARATOR);
    }
    p.text(egui::pos2(rect.left() + 4.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), crate::theme::TEXT);
    let chev = egui::Rect::from_center_size(egui::pos2(rect.right() - 10.0, rect.center().y), egui::vec2(14.0, 14.0));
    crate::icons::paint(p, chev, Icon::Next, crate::theme::TEXT3);
    p.text(egui::pos2(chev.left() - 6.0, rect.center().y), egui::Align2::RIGHT_CENTER, value, egui::FontId::proportional(15.0), crate::theme::TEXT2);
    resp
}

fn divider(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, crate::theme::SEPARATOR));
}

fn size_label(s: OutSize) -> &'static str {
    match s {
        OutSize::Short(1080) => "1080p",
        OutSize::Short(720) => "720p",
        OutSize::Short(480) => "480p",
        OutSize::Percent(f) if (f - 0.5).abs() < 0.01 => "50 %",
        _ => tr("Original"),
    }
}

/// Full-screen export page for phones (see the design canvas).
#[allow(clippy::too_many_arguments)]
fn phone_screen(
    app: &mut TrazoApp,
    ctx: &egui::Context,
    settings: ExportSettings,
    preview: Option<egui::TextureHandle>,
    range: Option<(usize, usize)>,
    start: &mut bool,
    cancel: &mut bool,
    share: &mut bool,
    open: &mut bool,
) {
    use crate::theme::{ACCENT, DANGER, ELEVATED, TEXT, TEXT2};
    let screen = ctx.content_rect();
    let (top, bottom) = crate::widgets::insets(app.platform.touch_first);
    let landscape = screen.width() > screen.height();
    let rect = egui::Rect::from_min_max(egui::pos2(screen.left(), screen.top() + top), egui::pos2(screen.right(), screen.bottom() - bottom));
    let busy = app.export_ui.job.is_some();
    egui::Area::new(egui::Id::unique("export_phone")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        egui::Frame::new().fill(crate::theme::BG).inner_margin(egui::Margin { left: 16, right: 16, top: 4, bottom: 10 }).show(ui, |ui| {
            ui.set_width(rect.width() - 32.0);
            ui.set_height(rect.height() - 14.0);
            // Header: Cancel · title.
            ui.horizontal(|ui| {
                ui.set_min_height(48.0);
                let label = if busy { tr("Ocultar") } else { tr("Cancelar") };
                if ui.add(egui::Button::new(RichText::new(label).color(ACCENT).size(16.0)).frame(false).min_size(egui::vec2(84.0, 44.0))).clicked() {
                    *open = false;
                }
                let title_w = ui.available_width() - 84.0;
                ui.add_sized([title_w, 44.0], egui::Label::new(RichText::new(tr("Exportar")).size(17.0).strong().color(TEXT)));
            });

            // Fixed footer height: button (+ share) and info.
            let footer = if landscape { 62.0 } else if app.export_ui.last.is_some() && !busy { 128.0 } else { 86.0 };
            let body_h = (ui.available_height() - footer).max(120.0);
            egui::ScrollArea::vertical().id_salt("export_phone_body").max_height(body_h).auto_shrink([false, false]).show(ui, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    if landscape {
                        let col = (ui.available_width() - 16.0) / 2.0;
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(col);
                                destinations(app, ui, &preview, egui::vec2(col, 80.0));
                            });
                            ui.add_space(16.0);
                            ui.vertical(|ui| {
                                ui.set_width(col);
                                export_settings(app, ui, range, settings);
                            });
                        });
                    } else {
                        let w = ui.available_width();
                        destinations(app, ui, &preview, egui::vec2(w, 200.0));
                        export_settings(app, ui, range, settings);
                    }
                });
            });

            // Footer.
            ui.add_space(6.0);
            let pl = plan(&app.doc.project, &settings);
            let info = if settings.format == ExportFormat::Png {
                tf!("{}×{} · fotograma {}", pl.width, pl.height, app.doc.frame + 1)
            } else if settings.format.has_video() {
                format!("{}×{} · {} fps · {} s · ≈ {}", pl.width, pl.height, pl.fps, crate::i18n::num(pl.duration_secs() as f32, 1), human_bytes(estimate_bytes(&app.doc.project, &settings)))
            } else {
                tf!("{} s de audio · ≈ {}", crate::i18n::num(pl.duration_secs() as f32, 1), human_bytes(estimate_bytes(&app.doc.project, &settings)))
            };
            if let Some(job) = &app.export_ui.job {
                let p = f32::from_bits(job.progress.load(Ordering::Relaxed));
                let elapsed = job.started.elapsed().as_secs_f32();
                let eta = if p > 0.03 { tf!(" · quedan ≈ {} s", format!("{:.0}", elapsed * (1.0 - p) / p)) } else { String::new() };
                ui.add(egui::ProgressBar::new(p).show_percentage().desired_height(10.0).corner_radius(5).fill(ACCENT));
                ui.label(RichText::new(tf!("Exportando {}", tr(job.format.label())) + &eta).size(13.0).color(TEXT2));
                if ui.add(egui::Button::new(RichText::new(tr("Cancelar exportación")).color(DANGER).size(16.0)).fill(ELEVATED).corner_radius(crate::theme::R_ROW).min_size(egui::vec2(ui.available_width(), 46.0))).clicked() {
                    *cancel = true;
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            } else {
                if !landscape {
                    ui.label(RichText::new(info.clone()).size(13.0).color(TEXT2));
                }
                let what = match settings.format {
                    ExportFormat::Mp4 | ExportFormat::Avi => tr("Exportar video"),
                    ExportFormat::Gif => tr("Exportar GIF"),
                    ExportFormat::Wav => tr("Exportar audio"),
                    _ => tr("Exportar imagen"),
                };
                if landscape {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(info).size(13.0).color(TEXT2));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let share_ok = app.export_ui.last.as_ref().is_some_and(|d| d.uri.is_some());
                            ui.allocate_ui(egui::vec2(260.0, 50.0), |ui| {
                                if crate::widgets::primary_button(ui, what).clicked() {
                                    *start = true;
                                }
                            });
                            if share_ok && ui.add(egui::Button::new(RichText::new(tr("Compartir")).size(15.0)).fill(ELEVATED).corner_radius(crate::theme::R_ROW).min_size(egui::vec2(120.0, 50.0))).clicked() {
                                *share = true;
                            }
                        });
                    });
                } else if crate::widgets::primary_button(ui, what).clicked() {
                    *start = true;
                }
                if let Some(done) = app.export_ui.last.as_ref().filter(|_| !landscape) {
                    if done.uri.is_some() && ui.add(egui::Button::new(RichText::new(tr("Compartir el último")).color(TEXT).size(15.0)).fill(ELEVATED).corner_radius(crate::theme::R_ROW).min_size(egui::vec2(ui.available_width(), 42.0))).clicked() {
                        *share = true;
                    }
                }
            }
        });
    });
}

/// Preview and destination cards.
fn destinations(app: &mut TrazoApp, ui: &mut egui::Ui, preview: &Option<egui::TextureHandle>, max: egui::Vec2) {
    if let Some(tex) = preview {
        let size = tex.size_vec2();
        let scale = (max.x / size.x).min(max.y / size.y);
        ui.vertical_centered(|ui| {
            ui.add(egui::Image::new((tex.id(), size * scale)).corner_radius(8).bg_fill(crate::theme::SURFACE));
        });
    }
    let n = PRESETS.len() as f32;
    let w = ((ui.available_width() - 8.0 * (n - 1.0)) / n).floor();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for i in 0..PRESETS.len() {
            let enabled = mp4_available() || CARDS[i].0 == "GIF";
            let sel = preset_selected(i, &app.export_ui.settings);
            if preset_card(ui, i, sel, enabled, w).clicked() {
                (PRESETS[i].apply)(&mut app.export_ui.settings);
            }
        }
    });
}

/// Settings list and file name.
fn export_settings(app: &mut TrazoApp, ui: &mut egui::Ui, range: Option<(usize, usize)>, settings: ExportSettings) {
    use crate::theme::{ELEVATED, TEXT2};
        // Settings list.
        let fps = app.doc.project.canvas.fps;
        egui::Frame::new().fill(ELEVATED).corner_radius(crate::theme::R_ROW).inner_margin(egui::Margin::symmetric(12, 2)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            let s = &mut app.export_ui.settings;
            let r = value_row(ui, tr("Formato"), tr(s.format.label()));
            egui::Popup::menu(&r).show(|ui| {
                ui.set_min_width(240.0);
                for f in FORMATS {
                    let ok = f != ExportFormat::Mp4 || mp4_available();
                    if ui.add_enabled(ok, egui::Button::selectable(s.format == f, tr(f.label())).min_size(egui::vec2(220.0, 40.0))).clicked() {
                        s.format = f;
                        ui.close();
                    }
                }
            });
            if s.format.has_video() {
                divider(ui);
                let r = value_row(ui, tr("Forma"), tr(s.aspect.label()));
                egui::Popup::menu(&r).show(|ui| {
                    ui.set_min_width(220.0);
                    for a in Aspect::ALL {
                        if ui.add(egui::Button::selectable(s.aspect == a, tr(a.label())).min_size(egui::vec2(200.0, 40.0))).clicked() {
                            s.aspect = a;
                            ui.close();
                        }
                    }
                });
                if s.aspect != Aspect::Canvas {
                    divider(ui);
                    ui.add_space(6.0);
                    let mut i = if s.fit == Fit::Pad { 0 } else { 1 };
                    if crate::widgets::segmented(ui, &mut i, &[tr("Todo visible"), tr("Llenar recortando")], ui.available_width()) {
                        s.fit = if i == 0 { Fit::Pad } else { Fit::Crop };
                    }
                    ui.add_space(6.0);
                }
                divider(ui);
                let r = value_row(ui, tr("Tamaño"), size_label(s.size));
                egui::Popup::menu(&r).show(|ui| {
                    ui.set_min_width(200.0);
                    for (label, v) in [(tr("Original"), OutSize::Percent(1.0)), ("1080p", OutSize::Short(1080)), ("720p", OutSize::Short(720)), ("480p", OutSize::Short(480)), ("50 %", OutSize::Percent(0.5))] {
                        if ui.add(egui::Button::selectable(s.size == v, label).min_size(egui::vec2(180.0, 40.0))).clicked() {
                            s.size = v;
                            ui.close();
                        }
                    }
                });
            }
            if s.format.is_timed_video() {
                divider(ui);
                let cur = s.out_fps.map(|f| format!("{f} fps")).unwrap_or_else(|| format!("{fps} fps"));
                let r = value_row(ui, tr("Velocidad"), &cur);
                egui::Popup::menu(&r).show(|ui| {
                    ui.set_min_width(220.0);
                    if ui.add(egui::Button::selectable(s.out_fps.is_none(), tf!("Del proyecto ({} fps)", fps)).min_size(egui::vec2(200.0, 40.0))).clicked() {
                        s.out_fps = None;
                        ui.close();
                    }
                    for f in [24, 30, 60] {
                        if ui.add(egui::Button::selectable(s.out_fps == Some(f), format!("{f} fps")).min_size(egui::vec2(200.0, 40.0))).clicked() {
                            s.out_fps = Some(f);
                            ui.close();
                        }
                    }
                });
                divider(ui);
                ui.horizontal(|ui| {
                    ui.set_min_height(46.0);
                    ui.add_space(4.0);
                    ui.label(tr("Repeticiones"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("+").size(18.0)).min_size(egui::vec2(40.0, 34.0))).clicked() {
                            s.loops = (s.loops + 1).min(20);
                        }
                        ui.label(RichText::new(format!("{}", s.loops)).strong());
                        if ui.add(egui::Button::new(RichText::new("−").size(18.0)).min_size(egui::vec2(40.0, 34.0))).clicked() {
                            s.loops = s.loops.saturating_sub(1).max(1);
                        }
                    });
                });
            }
            if matches!(s.format, ExportFormat::Mp4 | ExportFormat::Avi) {
                divider(ui);
                crate::widgets::switch_row(ui, tr("Incluir el audio"), &mut s.include_audio);
            }
            if s.format.supports_alpha() {
                divider(ui);
                crate::widgets::switch_row(ui, tr("Incluir el fondo"), &mut s.background);
            }
            if matches!(s.format, ExportFormat::Mp4 | ExportFormat::Gif | ExportFormat::Avi) {
                divider(ui);
                ui.horizontal(|ui| {
                    ui.set_min_height(46.0);
                    ui.add_space(4.0);
                    ui.label(tr("Calidad"));
                    let mut q = s.quality as i32;
                    ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(80.0);
                    if ui.add(egui::Slider::new(&mut q, 10..=100)).changed() {
                        s.quality = q as u8;
                    }
                });
            }
            if s.format != ExportFormat::Png {
                if let Some((a, b)) = range {
                    divider(ui);
                    crate::widgets::switch_row(ui, &tf!("Solo el rango {}–{}", a + 1, b + 1), &mut app.export_ui.use_range);
                }
            }
        });

        // File name.
        ui.label(RichText::new(tr("Nombre del archivo")).size(13.0).color(TEXT2));
        ui.horizontal(|ui| {
            let ext = format!(".{}", app.export_ui.settings.format.extension());
            let w = ui.available_width() - 50.0;
            ui.add(egui::TextEdit::singleline(&mut app.export_ui.file_name).desired_width(w).margin(egui::vec2(10.0, 10.0)));
            ui.label(RichText::new(ext).color(TEXT2));
        });
        if settings.format == ExportFormat::Wav && app.doc.project.audio.is_empty() {
            ui.label(RichText::new(tr("El proyecto no tiene audio: el WAV saldrá en silencio.")).color(Color32::from_rgb(255, 204, 0)));
        }
}

// ---- phone sheet (TelExportar) -------------------------------------------------------------

/// Destination cards of the phone sheet: preset index, label, shape (w, h).
const SHEET_CARDS: [(usize, &str, f32, f32); 4] = [(0, "TikTok y Reels", 16.0, 26.0), (1, "Instagram", 21.0, 26.0), (3, "YouTube", 32.0, 18.0), (4, "GIF", 0.0, 0.0)];

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn aspect_badge(s: &ExportSettings, w: u32, h: u32) -> String {
    match s.aspect {
        Aspect::Vertical => "9:16".into(),
        Aspect::Portrait => "4:5".into(),
        Aspect::Square => "1:1".into(),
        Aspect::Wide => "16:9".into(),
        Aspect::Canvas => {
            let g = gcd(w.max(1), h.max(1));
            if w / g <= 32 && h / g <= 32 { format!("{}:{}", w / g, h / g) } else { format!("{w}×{h}") }
        }
    }
}

/// Export as a sheet: picture of the result, where it goes, a few choices.
#[allow(clippy::too_many_arguments)]
fn phone_sheet(
    app: &mut TrazoApp,
    ctx: &egui::Context,
    settings: ExportSettings,
    preview: Option<egui::TextureHandle>,
    range: Option<(usize, usize)>,
    start: &mut bool,
    cancel: &mut bool,
    open: &mut bool,
) {
    use crate::theme::{self, font, ACCENT, DANGER, TEXT, TEXT2, W};
    let screen = ctx.content_rect();
    let (top, bottom) = crate::widgets::insets(app.platform.touch_first);
    let rect = egui::Rect::from_min_max(egui::pos2(screen.left(), screen.top() + top + 8.0), screen.max);
    let busy = app.export_ui.job.is_some();
    // Dim behind; a tap there closes (unless exporting).
    egui::Area::new(egui::Id::unique("export_dim")).order(egui::Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(140));
        if resp.clicked() && !busy {
            *open = false;
        }
    });
    let pl = plan(&app.doc.project, &settings);
    egui::Area::new(egui::Id::unique("export_sheet")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme::SHEET)
            .corner_radius(egui::CornerRadius { nw: theme::R_SHEET, ne: theme::R_SHEET, sw: 0, se: 0 })
            .inner_margin(egui::Margin { left: 16, right: 16, top: 8, bottom: (bottom as i8).saturating_add(14) })
            .show(ui, |ui| {
                ui.set_width(rect.width() - 32.0);
                ui.set_height(rect.height() - 22.0 - bottom);
                ui.spacing_mut().item_spacing.y = 0.0;
                // Grabber and title.
                let (g, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 5.0), egui::Sense::hover());
                ui.painter().rect_filled(egui::Rect::from_center_size(g.center(), egui::vec2(36.0, 5.0)), 3.0, theme::CONTROL);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.set_height(44.0);
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr("Exportar")).font(font(20.0, W::Semi)).color(TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::widgets::icon_button(ui, Icon::Close, if busy { tr("Ocultar") } else { tr("Cerrar") }).clicked() {
                            *open = false;
                        }
                    });
                });
                let footer = 92.0;
                let body_h = (ui.available_height() - footer).max(120.0);
                egui::ScrollArea::vertical().id_salt("export_sheet_body").max_height(body_h).auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add_space(8.0);
                        // The result, as it will look.
                        let k = (150.0 / pl.width as f32).min(266.0 / pl.height as f32).min(240.0 / pl.width as f32);
                        let (pr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), pl.height as f32 * k), egui::Sense::hover());
                        let box_r = egui::Rect::from_center_size(pr.center(), egui::vec2(pl.width as f32 * k, pl.height as f32 * k));
                        let p = ui.painter();
                        p.rect_filled(box_r, 12.0, Color32::BLACK);
                        if let Some(t) = &preview {
                            let mut mesh = egui::Mesh::with_texture(t.id());
                            mesh.add_rect_with_uv(box_r, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                            p.add(mesh);
                            p.with_clip_rect(box_r).rect_stroke(box_r.expand(6.0), 18.0, egui::Stroke::new(12.0, theme::SHEET), egui::StrokeKind::Middle);
                        }
                        p.rect_stroke(box_r, 12.0, egui::Stroke::new(1.0, Color32::from_white_alpha(20)), egui::StrokeKind::Outside);
                        let badge = aspect_badge(&settings, pl.width, pl.height);
                        let bg = p.layout_no_wrap(badge, font(10.0, W::Semi), TEXT);
                        let br = egui::Rect::from_min_size(box_r.min + egui::vec2(6.0, 6.0), egui::vec2(bg.size().x + 14.0, 20.0));
                        p.rect_filled(br, 6.0, theme::ELEVATED);
                        p.galley(egui::pos2(br.left() + 7.0, br.center().y - bg.size().y / 2.0), bg, TEXT);
                        ui.add_space(8.0);
                        let (cw, ch) = (app.doc.project.canvas.width as f32, app.doc.project.canvas.height as f32);
                        let taller = (pl.height as f32 / pl.width as f32) > ch / cw + 0.01;
                        let wider = (pl.width as f32 / pl.height as f32) > cw / ch + 0.01;
                        let caption = match (settings.aspect, settings.fit) {
                            (Aspect::Canvas, _) => tf!("Como el lienzo · {}×{}", pl.width, pl.height),
                            (_, Fit::Pad) if taller => tr("El papel se alarga arriba y abajo · no se corta nada").to_string(),
                            (_, Fit::Pad) if wider => tr("El papel se alarga a los lados · no se corta nada").to_string(),
                            (_, Fit::Crop) if taller => tr("Se recortan los lados para llenar la pantalla").to_string(),
                            (_, Fit::Crop) if wider => tr("Se recorta arriba y abajo para llenar la pantalla").to_string(),
                            _ => tr("Cabe justo · no se corta nada").to_string(),
                        };
                        ui.vertical_centered(|ui| ui.label(RichText::new(caption).font(font(12.0, W::Regular)).color(TEXT2)));
                        ui.add_space(16.0);
                        // Where it goes.
                        let n = SHEET_CARDS.len() as f32;
                        let w = ((ui.available_width() - 8.0 * (n - 1.0)) / n).floor();
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            for (i, label, sw, sh) in SHEET_CARDS {
                                let enabled = mp4_available() || label == "GIF";
                                let sel = preset_selected(i, &app.export_ui.settings);
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 92.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
                                let p = ui.painter();
                                p.rect_filled(r, 16.0, Color32::from_rgb(35, 35, 38));
                                if sel {
                                    p.rect_stroke(r, 16.0, egui::Stroke::new(2.0, TEXT), egui::StrokeKind::Inside);
                                }
                                let fg = if !enabled { crate::theme::TEXT3 } else if sel { TEXT } else { Color32::from_rgb(214, 214, 211) };
                                let mut job = egui::text::LayoutJob::simple(tr(label).to_string(), font(12.0, W::Semi), fg, w - 8.0);
                                job.halign = egui::Align::Center;
                                let g = p.layout_job(job);
                                let total = 26.0 + 8.0 + g.size().y;
                                let y0 = r.center().y - total / 2.0;
                                let c = egui::pos2(r.center().x, y0 + 13.0);
                                if sw > 0.0 {
                                    p.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(sw, sh)), 4.0, egui::Stroke::new(2.0, fg), egui::StrokeKind::Inside);
                                } else {
                                    let t = p.text(c, egui::Align2::CENTER_CENTER, "GIF", font(11.0, W::Semi), fg);
                                    p.rect_stroke(t.expand2(egui::vec2(5.0, 3.0)), 5.0, egui::Stroke::new(2.0, fg), egui::StrokeKind::Middle);
                                }
                                p.galley(egui::pos2(r.center().x, y0 + 34.0), g, fg);
                                if resp.on_hover_text(tr(PRESETS[i].hint)).clicked() {
                                    (PRESETS[i].apply)(&mut app.export_ui.settings);
                                }
                            }
                        });
                        ui.add_space(16.0);
                        egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 0)).show(ui, |ui| {
                            let s = &mut app.export_ui.settings;
                            if s.format.has_video() && s.aspect != Aspect::Canvas {
                                let mut i = if s.fit == Fit::Pad { 0 } else { 1 };
                                if crate::widgets::segmented(ui, &mut i, &[tr("Alargar el fondo"), tr("Recortar")], ui.available_width()) {
                                    s.fit = if i == 0 { Fit::Pad } else { Fit::Crop };
                                }
                                ui.add_space(6.0);
                            }
                            // Sound.
                            if matches!(s.format, ExportFormat::Mp4 | ExportFormat::Avi) && !app.doc.project.audio.is_empty() {
                                let name = app.doc.project.audio.assets.values().next().map(|a| a.name.clone()).unwrap_or_default();
                                sheet_row(ui, |ui| {
                                    let mut job = egui::text::LayoutJob::default();
                                    job.append(tr("Sonido"), 0.0, egui::TextFormat { font_id: font(15.0, W::Medium), color: TEXT, ..Default::default() });
                                    if !name.is_empty() {
                                        job.append(&format!(" · {name}"), 0.0, egui::TextFormat { font_id: font(15.0, W::Regular), color: TEXT2, ..Default::default() });
                                    }
                                    ui.label(job);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        crate::widgets::switch(ui, &mut s.include_audio);
                                    });
                                });
                            }
                            if s.format.is_timed_video() {
                                let r = sheet_value_row(ui, tr("Repetir"), &if s.loops == 1 { tr("Una vez").to_string() } else { tf!("{} veces", s.loops) });
                                egui::Popup::menu(&r).show(|ui| {
                                    ui.set_min_width(180.0);
                                    for n in [1u32, 2, 3, 5, 10] {
                                        let l = if n == 1 { tr("Una vez").to_string() } else { tf!("{} veces", n) };
                                        if ui.add(egui::Button::selectable(s.loops as u32 == n, l).min_size(egui::vec2(160.0, 40.0))).clicked() {
                                            s.loops = n as _;
                                            ui.close();
                                        }
                                    }
                                });
                            }
                            let r = sheet_value_row(ui, tr("Formato"), tr(s.format.label()));
                            egui::Popup::menu(&r).show(|ui| {
                                ui.set_min_width(240.0);
                                for f in FORMATS {
                                    let ok = f != ExportFormat::Mp4 || mp4_available();
                                    if ui.add_enabled(ok, egui::Button::selectable(s.format == f, tr(f.label())).min_size(egui::vec2(220.0, 40.0))).clicked() {
                                        s.format = f;
                                        ui.close();
                                    }
                                }
                            });
                            let more = app.export_ui.more;
                            if sheet_value_row(ui, tr("Más ajustes"), if more { tr("Ocultar") } else { tr("Tamaño, calidad, nombre…") }).clicked() {
                                app.export_ui.more = !more;
                            }
                        });
                        if app.export_ui.more {
                            ui.add_space(10.0);
                            ui.spacing_mut().item_spacing.y = 10.0;
                            export_settings(app, ui, range, settings);
                        }
                        ui.add_space(12.0);
                    });
                });
                // Footer: one white button and what it makes.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    let size = human_bytes(estimate_bytes(&app.doc.project, &settings));
                    if let Some(job) = &app.export_ui.job {
                        let p = f32::from_bits(job.progress.load(Ordering::Relaxed));
                        let elapsed = job.started.elapsed().as_secs_f32();
                        let eta = if p > 0.03 { tf!(" · quedan ≈ {} s", format!("{:.0}", elapsed * (1.0 - p) / p)) } else { String::new() };
                        if ui.add(egui::Button::new(RichText::new(tr("Cancelar exportación")).color(DANGER).font(font(15.0, W::Semi))).fill(theme::ELEVATED).corner_radius(16).min_size(egui::vec2(ui.available_width(), 50.0))).clicked() {
                            *cancel = true;
                        }
                        ui.label(RichText::new(tf!("Exportando {}", tr(job.format.label())) + &eta).font(font(12.0, W::Regular)).color(TEXT2));
                        ui.add(egui::ProgressBar::new(p).desired_height(8.0).corner_radius(4).fill(ACCENT));
                        ctx.request_repaint_after(std::time::Duration::from_millis(150));
                    } else {
                        let place = if cfg!(target_os = "android") { tr("se guarda en tu galería") } else { tr("se guarda en tu carpeta de Videos") };
                        ui.label(RichText::new(tf!("Unos {} · {}", size, place)).font(font(12.0, W::Regular)).color(TEXT2));
                        let label = if cfg!(target_os = "android") { tr("Guardar y compartir") } else { tr("Guardar") };
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.0), egui::Sense::click());
                        let p = ui.painter();
                        p.rect_filled(r, 16.0, if resp.is_pointer_button_down_on() { theme::PRIMARY.gamma_multiply(0.85) } else { theme::PRIMARY });
                        let g = p.layout_no_wrap(label.to_string(), font(16.0, W::Bold), theme::ON_ACCENT);
                        let x = r.center().x - (g.size().x + 28.0) / 2.0;
                        crate::icons::paint(p, egui::Rect::from_center_size(egui::pos2(x + 10.0, r.center().y), egui::vec2(20.0, 20.0)), Icon::Export, theme::ON_ACCENT);
                        p.galley(egui::pos2(x + 28.0, r.center().y - g.size().y / 2.0), g, theme::ON_ACCENT);
                        if resp.clicked() {
                            app.export_ui.share_after = cfg!(target_os = "android");
                            *start = true;
                        }
                    }
                });
            });
    });
}

/// A row of the sheet with a hairline on top.
fn sheet_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, Color32::from_rgb(35, 35, 38)));
    ui.horizontal(|ui| {
        ui.set_height(56.0);
        add(ui);
    });
}

/// "Label            value ›" row; returns its response (for a menu).
fn sheet_value_row(ui: &mut egui::Ui, label: &str, value: &str) -> egui::Response {
    use crate::theme::{font, TEXT, TEXT2, W};
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(line.x_range(), line.center().y, egui::Stroke::new(1.0, Color32::from_rgb(35, 35, 38)));
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(r, 10.0, crate::theme::ELEVATED);
    }
    p.text(egui::pos2(r.left(), r.center().y), egui::Align2::LEFT_CENTER, label, font(15.0, W::Medium), TEXT);
    p.text(egui::pos2(r.right(), r.center().y), egui::Align2::RIGHT_CENTER, format!("{value} ›"), font(14.0, W::Regular), TEXT2);
    resp
}
