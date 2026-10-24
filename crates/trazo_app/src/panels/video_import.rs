//! "Importar video": a video (Android) or animated GIF becomes a video layer
//! under the current layer, for rotoscoping. Reading and decoding run in a
//! background thread; the dialog shows the range, audio and memory options.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};

use trazo_core::audio::{ticks_to_samples, SAMPLE_RATE};
use trazo_core::video::{VideoBuilder, VideoLayer, VideoMedia, MAX_BYTES, MAX_FRAMES, MAX_SIDE, THUMB_W};
use trazo_core::ops;
use egui::RichText;

use crate::app::TrazoApp;

#[cfg(target_os = "android")]
pub const VIDEO_EXTS: [&str; 7] = ["mp4", "m4v", "mov", "3gp", "webm", "mkv", "gif"];
#[cfg(not(target_os = "android"))]
pub const VIDEO_EXTS: [&str; 1] = ["gif"];

/// What we know about the file before importing.
#[derive(Clone)]
struct SourceInfo {
    gif: bool,
    duration: f64,
    width: u32,
    height: u32,
    has_audio: bool,
}

struct Options {
    path: PathBuf,
    info: SourceInfo,
    from: f64,
    to: f64,
    audio: bool,
    /// Empty drawing frames to create up to the end of the video: hold of
    /// each (1 = "a unos", 2 = "a doses"…), 0 = none.
    frames_hold: u32,
}

struct Output {
    media: VideoMedia,
    /// Audio of the imported range (None when not asked or not present).
    audio: Option<Result<Vec<f32>, String>>,
}

struct Job {
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Output, String>>,
    name: String,
    start_tick: u32,
    frames_hold: u32,
    /// Document the import belongs to (discarded if another is opened).
    doc_serial: u64,
}

#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    Probing(PathBuf, mpsc::Receiver<Result<SourceInfo, String>>),
    Ready(Options),
    Running(Job),
}

#[derive(Default)]
pub struct VideoImportUi {
    stage: Stage,
}

impl VideoImportUi {
    pub fn is_busy(&self) -> bool {
        !matches!(self.stage, Stage::Idle)
    }
}

fn name_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Video".into())
}

fn is_gif(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("gif"))
}

/// Start reading the chosen file (the dialog opens when its details are known).
pub fn open(app: &mut TrazoApp, ctx: &egui::Context, files: Vec<PathBuf>) {
    let Some(path) = files.into_iter().next() else { return };
    if app.video_import.is_busy() {
        app.toast(ctx, "Ya se está importando un video");
        return;
    }
    app.stop_playback();
    let (tx, rx) = mpsc::channel();
    let p = path.clone();
    let ctx2 = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(probe(&p));
        ctx2.request_repaint();
    });
    app.video_import.stage = Stage::Probing(path, rx);
}

fn probe(path: &Path) -> Result<SourceInfo, String> {
    if is_gif(path) {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let (w, h, duration) = gif_info(&bytes)?;
        return Ok(SourceInfo { gif: true, duration, width: w, height: h, has_audio: false });
    }
    #[cfg(target_os = "android")]
    {
        let i = crate::android_video::probe(path)?;
        let (width, height) = i.upright();
        Ok(SourceInfo { gif: false, duration: i.duration, width, height, has_audio: i.has_audio })
    }
    #[cfg(not(target_os = "android"))]
    Err("En el ordenador por ahora solo se pueden importar GIF animados".into())
}

/// Size and length of an animated GIF (reads the frame delays only).
fn gif_info(bytes: &[u8]) -> Result<(u32, u32, f64), String> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::Indexed);
    let mut dec = opts.read_info(std::io::Cursor::new(bytes)).map_err(|e| format!("GIF no válido: {e}"))?;
    let (w, h) = (dec.width() as u32, dec.height() as u32);
    let mut secs = 0.0;
    while let Some(f) = dec.next_frame_info().map_err(|e| format!("GIF no válido: {e}"))? {
        // Browsers show 0–10 ms delays as 100 ms (same rule as the decoder).
        secs += if f.delay <= 1 { 0.1 } else { f.delay as f64 / 100.0 };
    }
    Ok((w, h, secs.max(0.01)))
}

/// Pictures to store and rough memory they need (JPEG ≈ 2.8 bits per pixel
/// for camera footage, plus the thumbnails).
fn estimate_bytes(o: &Options, cw: u32, ch: u32, fps: u32) -> (usize, u64) {
    let (mw, mh) = (cw.min(MAX_SIDE) as f64, ch.min(MAX_SIDE) as f64);
    let f = (mw / o.info.width.max(1) as f64).min(mh / o.info.height.max(1) as f64).min(1.0);
    let (w, h) = (o.info.width as f64 * f, o.info.height as f64 * f);
    let thumb = THUMB_W as f64 * (THUMB_W as f64 * h / w.max(1.0)) * 4.0;
    let n = ((o.to - o.from).max(0.0) * fps as f64).ceil() as usize;
    (n, (n as f64 * (w * h * 0.35 + thumb)) as u64)
}

fn start(app: &mut TrazoApp, ctx: &egui::Context, o: Options) {
    let p = &app.doc.project;
    let (cw, ch, fps) = (p.canvas.width, p.canvas.height, p.canvas.fps.max(1));
    let start_tick = p.tick_of_frame(app.doc.frame);
    let progress = Arc::new(AtomicU32::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (pr, ca, ctx2) = (progress.clone(), cancel.clone(), ctx.clone());
    let name = name_of(&o.path);
    let job_name = name.clone();
    let (frames_hold, doc_serial) = (o.frames_hold, app.doc.serial);
    let (path, info, from, to, want_audio) = (o.path, o.info, o.from, o.to, o.audio);
    std::thread::spawn(move || {
        let run = || -> Result<Output, String> {
            let mut b = VideoBuilder::new(cw, ch, fps as f32);
            let mut tick = |f: f32| {
                pr.store(f.to_bits(), Ordering::Relaxed);
                ctx2.request_repaint_after(std::time::Duration::from_millis(100));
                !ca.load(Ordering::Relaxed)
            };
            if info.gif {
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                trazo_core::video::decode_gif(&bytes, fps as f32, &mut b, &mut tick).map_err(|e| e.to_string())?;
            } else {
                #[cfg(target_os = "android")]
                crate::android_video::decode(&path, fps as f32, from, to, &mut b, &mut tick)?;
                #[cfg(not(target_os = "android"))]
                return Err("formato no compatible en el ordenador".into());
            }
            let cancelled = || if ca.load(Ordering::Relaxed) { Err(trazo_core::export::CANCELLED.to_string()) } else { Ok(()) };
            cancelled()?;
            let audio = (want_audio && info.has_audio).then(|| {
                trazo_core::audio::decode_audio_file(&path).map_err(|e| e.to_string()).map(|s| {
                    // Keep only the imported range (stereo samples).
                    let a = ((from * SAMPLE_RATE as f64) as usize * 2).min(s.len());
                    let z = ((to * SAMPLE_RATE as f64) as usize * 2).clamp(a, s.len());
                    s[a..z].to_vec()
                })
            });
            cancelled()?;
            Ok(Output { media: b.finish(name).map_err(|e| e.to_string())?, audio })
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| Err("error inesperado al leer el video".into()));
        let _ = tx.send(result);
        ctx2.request_repaint();
    });
    app.video_import.stage = Stage::Running(Job { progress, cancel, rx, name: job_name, start_tick, frames_hold, doc_serial });
}

/// Put the finished video in the project as one undo step.
fn finish(app: &mut TrazoApp, ctx: &egui::Context, job: &Job, out: Output) {
    let d = &mut app.doc;
    let (cw, ch, fps) = (d.project.canvas.width, d.project.canvas.height, d.project.canvas.fps.max(1));
    let mark = d.history.undo_len();
    let media = Arc::new(out.media);
    let n = media.len();
    let video = VideoLayer::new(media, job.start_tick as i64, cw, ch);
    let end = video.end_tick(fps).clamp(0, 200_000) as u32;
    // Under the current layer, so the drawing layer stays on top and
    // selected; below a clipping group's base so the group keeps working.
    let mut index = d.layer_index();
    while index > 0 && d.project.layers[index].props.clip {
        index -= 1;
    }
    ops::add_video_layer(&mut d.project, &mut d.history, index, format!("Video: {}", job.name), video, 0.6);
    let added = if job.frames_hold > 0 { ops::extend_to_ticks(&mut d.project, &mut d.history, end, job.frames_hold) } else { 0 };
    let mut note = String::new();
    match out.audio {
        Some(Ok(samples)) if !samples.is_empty() => {
            let at = ticks_to_samples(job.start_tick as f64, fps);
            crate::panels::audio::add_clip(app, &job.name, samples, at, None, "Audio del video", false);
            note = " con su audio".into();
        }
        Some(Err(e)) => note = format!(" (sin audio: {e})"),
        _ => {}
    }
    let d = &mut app.doc;
    d.history.group_since(mark);
    if d.project.layer(d.layer).is_some_and(|l| l.props.video.is_some()) {
        d.layer = d.project.layers.iter().rev().find(|l| l.props.video.is_none()).map_or(d.layer, |l| l.id);
    }
    d.invalidate_all();
    let extra = if added > 0 { format!(", +{added} fotogramas") } else { String::new() };
    app.toast(ctx, format!("Video importado: {n} imágenes{note}{extra}. Dibuja encima."));
}

pub fn show(app: &mut TrazoApp, ctx: &egui::Context) {
    // Background results.
    match std::mem::take(&mut app.video_import.stage) {
        Stage::Probing(path, rx) => match rx.try_recv() {
            Ok(Ok(info)) => {
                let total = app.doc.project.duration_secs() as f64;
                let to = info.duration.min(MAX_FRAMES as f64 / app.doc.project.canvas.fps.max(1) as f64);
                let frames_hold = if total + 0.01 < to { 1 } else { 0 };
                app.video_import.stage = Stage::Ready(Options { path, from: 0.0, to, audio: info.has_audio, frames_hold, info });
            }
            Ok(Err(e)) => app.toast(ctx, format!("No se pudo abrir el video: {e}")),
            Err(mpsc::TryRecvError::Empty) => app.video_import.stage = Stage::Probing(path, rx),
            Err(mpsc::TryRecvError::Disconnected) => app.toast(ctx, "No se pudo abrir el video"),
        },
        Stage::Running(job) => match job.rx.try_recv() {
            Ok(Ok(_)) if job.cancel.load(Ordering::Relaxed) => app.toast(ctx, "Importación cancelada"),
            Ok(Ok(_)) if job.doc_serial != app.doc.serial => app.toast(ctx, "Se abrió otro proyecto: el video no se importó"),
            Ok(Ok(out)) => finish(app, ctx, &job, out),
            Ok(Err(e)) if e == trazo_core::export::CANCELLED => app.toast(ctx, "Importación cancelada"),
            Ok(Err(e)) => app.toast(ctx, format!("No se pudo importar el video: {e}")),
            Err(mpsc::TryRecvError::Empty) => app.video_import.stage = Stage::Running(job),
            Err(mpsc::TryRecvError::Disconnected) => app.toast(ctx, "La importación se interrumpió"),
        },
        other => app.video_import.stage = other,
    }

    let p = &app.doc.project;
    let (cw, ch, fps) = (p.canvas.width, p.canvas.height, p.canvas.fps.max(1));
    let mut close = false;
    let mut go = false;
    let mut open = true;
    let stage = &mut app.video_import.stage;
    if matches!(stage, Stage::Idle) {
        return;
    }
    crate::widgets::backdrop(ctx);
    egui::Window::new("Importar video")
        .order(egui::Order::Foreground)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(crate::widgets::dialog_width(ctx, 460.0))
        .max_width(crate::widgets::dialog_width(ctx, 460.0))
        .max_height(ctx.content_rect().height() - 80.0)
        .vscroll(true)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| match stage {
            Stage::Idle => {}
            Stage::Probing(path, _) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Leyendo «{}»…", name_of(path)));
                });
            }
            Stage::Ready(o) => {
                let i = &o.info;
                ui.label(RichText::new(name_of(&o.path)).strong());
                let sound = if i.gif { "GIF animado" } else if i.has_audio { "con audio" } else { "sin audio" };
                ui.label(format!("{}×{} · {:.1} s · {sound}", i.width, i.height, i.duration));
                ui.add_space(6.0);
                if !i.gif {
                    let max = i.duration.max(0.1);
                    ui.horizontal(|ui| {
                        ui.label("Desde");
                        ui.add(egui::DragValue::new(&mut o.from).range(0.0..=max).speed(0.05).suffix(" s").fixed_decimals(1));
                        ui.label("hasta");
                        ui.add(egui::DragValue::new(&mut o.to).range(0.0..=max).speed(0.05).suffix(" s").fixed_decimals(1));
                    });
                    if o.to < o.from + 0.1 {
                        o.to = (o.from + 0.1).min(max);
                    }
                    if i.has_audio {
                        ui.checkbox(&mut o.audio, "Importar también el audio");
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Fotogramas para dibujar:").on_hover_text("Fotogramas vacíos hasta el final del video, para calcar uno cada 1, 2 o 3 imágenes");
                    for (label, hold) in [("no crear", 0), ("a unos", 1), ("a doses", 2), ("a treses", 3)] {
                        ui.selectable_value(&mut o.frames_hold, hold, label);
                    }
                });
                let (n, bytes) = estimate_bytes(o, cw, ch, fps);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("{n} imágenes a {fps} fps (las del proyecto) · ≈ {:.0} MB", bytes as f64 / 1e6 + 0.5))
                        .small()
                        .weak(),
                );
                let too_long = n > MAX_FRAMES || bytes as usize > MAX_BYTES;
                if too_long {
                    ui.colored_label(ui.visuals().warn_fg_color, format!("Demasiado largo: el máximo son {MAX_FRAMES} imágenes y {} MB. Acorta el rango.", MAX_BYTES >> 20));
                }
                ui.label(RichText::new("Se añade como capa de referencia bajo la capa actual, al 60 %: se ve al dibujar y no se exporta.").small().weak());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!too_long, egui::Button::new("Importar")).clicked() {
                        go = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        close = true;
                    }
                });
            }
            Stage::Running(job) => {
                let f = f32::from_bits(job.progress.load(Ordering::Relaxed));
                ui.label(format!("Importando «{}»…", job.name));
                ui.add(egui::ProgressBar::new(f).show_percentage());
                if ui.button("Cancelar").clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            }
        });
    if !open {
        match &app.video_import.stage {
            Stage::Running(job) => job.cancel.store(true, Ordering::Relaxed),
            _ => close = true,
        }
    }
    if close {
        app.video_import.stage = Stage::Idle;
    }
    if go {
        if let Stage::Ready(o) = std::mem::take(&mut app.video_import.stage) {
            start(app, ctx, o);
        }
    }
}
