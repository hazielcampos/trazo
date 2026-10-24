//! Audio panel: tracks with waveforms, clip editing (move, trim, split,
//! volume, fades), lip-sync markers, mixing controls and voice-over recording.

use std::sync::mpsc;
use std::sync::Arc;

use trazo_core::audio::{
    normalized, render_mix, samples_to_ticks, sound_starts, ticks_to_samples, AudioAsset, AudioClip, AudioTrack, ClipId, Marker, Phoneme,
    TrackId, PEAK_BLOCK, SAMPLE_RATE,
};
use trazo_core::{ops, Edit};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::app::TrazoApp;
use crate::icons::{Icon, IconButton};

const HEADER_W: f32 = 176.0;
const LANE_H: f32 = 58.0;
const RULER_H: f32 = 24.0;
const MARKER_H: f32 = 22.0;
/// Clip edges within this many pixels trim instead of move.
const EDGE: f32 = 9.0;
/// Fade handle size (top corners of a selected clip).
const FADE_HANDLE: f32 = 10.0;
pub const AUDIO_EXTS: [&str; 9] = ["wav", "mp3", "ogg", "flac", "m4a", "aac", "aif", "aiff", "opus"];
const COUNTDOWN: f64 = 3.0;

#[derive(Clone, Copy, PartialEq)]
enum DragMode {
    Move,
    TrimStart,
    TrimEnd,
    FadeIn,
    FadeOut,
}

struct ClipDrag {
    clip: ClipId,
    mode: DragMode,
    start_x: f32,
    orig: AudioClip,
    orig_track: usize,
    before: Vec<AudioTrack>,
}

enum LaneDrag {
    Clip(ClipDrag),
    /// Dragging the playhead: hear the sound under it.
    Scrub,
    /// Shift-drag on the ruler: playback loop range (start sample).
    Range(i64),
    Marker { id: u64, before: Vec<Marker>, start_x: f32, orig_at: i64 },
}

pub enum RecordState {
    Idle,
    /// Android: the system permission dialog is showing.
    WaitingPermission { since: f64, track: Option<TrackId> },
    Countdown { until: f64, track: Option<TrackId> },
    /// `start`: where the take goes. `capturing` becomes true once the
    /// pre-roll has played up to `start`.
    Recording { start: i64, track: Option<TrackId>, prev_loop: bool, capturing: bool },
}

pub struct AudioUi {
    pub show: bool,
    /// Zoom: pixels per second.
    pub pps: f32,
    pub selected: Option<ClipId>,
    pub selected_marker: Option<u64>,
    /// Hear a frame's sound when changing frames.
    pub scrub: bool,
    pub volume: f32,
    /// Play the other tracks while recording (use headphones).
    pub monitor: bool,
    /// Seconds played before the recording point.
    pub preroll: f32,
    /// Shifts new takes in time to compensate microphone latency (ms).
    pub record_offset_ms: f32,
    pub record: RecordState,
    /// Height of the panel (dragged by the user; None = automatic).
    pub height: Option<f32>,
    /// Track that receives recordings and imports (None = create one).
    pub target_track: Option<TrackId>,
    drag: Option<LaneDrag>,
    /// Tracks before a slider edit started (one undo step per gesture).
    slider_before: Option<Vec<AudioTrack>>,
    renaming: Option<(TrackId, String)>,
    /// Imports decoding in the background.
    imports: Vec<mpsc::Receiver<ImportResult>>,
}

struct ImportResult {
    name: String,
    path: std::path::PathBuf,
    start: i64,
    track: Option<TrackId>,
    result: Result<Vec<f32>, String>,
}

impl Default for AudioUi {
    fn default() -> Self {
        Self {
            show: false,
            pps: 120.0,
            selected: None,
            selected_marker: None,
            scrub: true,
            volume: 1.0,
            monitor: true,
            preroll: 1.0,
            record_offset_ms: 0.0,
            record: RecordState::Idle,
            height: None,
            target_track: None,
            drag: None,
            slider_before: None,
            renaming: None,
            imports: Vec::new(),
        }
    }
}

impl AudioUi {
    pub fn with_panel(show: bool) -> Self {
        Self { show, ..Default::default() }
    }

    pub fn is_recording(&self) -> bool {
        !matches!(self.record, RecordState::Idle)
    }

    pub fn is_importing(&self) -> bool {
        !self.imports.is_empty()
    }

    /// A drag or slider gesture is in progress (undo must wait for it).
    pub fn gesture_in_progress(&self) -> bool {
        self.drag.is_some() || self.slider_before.is_some()
    }

    /// Forget per-project state (selection, gestures) when the project changes.
    pub fn reset_for_new_project(&mut self) {
        self.selected = None;
        self.selected_marker = None;
        self.target_track = None;
        self.drag = None;
        self.slider_before = None;
        self.renaming = None;
        self.imports.clear();
    }
}

// ---- editing helpers --------------------------------------------------------

/// Apply `f` to a copy of the tracks and record it as one undo step.
fn edit_tracks(app: &mut TrazoApp, f: impl FnOnce(&mut Vec<AudioTrack>, &mut trazo_core::Project)) {
    let mut tracks = app.doc.project.audio.tracks.clone();
    f(&mut tracks, &mut app.doc.project);
    ops::set_audio_tracks(&mut app.doc.project, &mut app.doc.history, tracks);
    app.doc.audio_version += 1;
}

fn edit_markers(app: &mut TrazoApp, f: impl FnOnce(&mut Vec<Marker>)) {
    let mut m = app.doc.project.audio.markers.clone();
    f(&mut m);
    ops::set_markers(&mut app.doc.project, &mut app.doc.history, m);
}

/// Add decoded audio as a new clip at `start` on `track` (or a new track).
/// With `replace_takes`, clips it overlaps on that track are muted (kept as
/// alternative takes).
pub fn add_clip(app: &mut TrazoApp, name: &str, samples: Vec<f32>, start: i64, track: Option<TrackId>, track_name: &str, replace_takes: bool) -> ClipId {
    let p = &mut app.doc.project;
    let asset_id = p.alloc_id();
    let asset = AudioAsset::new(name, samples);
    let len = asset.frames().max(1);
    p.audio.assets.insert(asset_id, Arc::new(asset));
    let clip_id = p.alloc_id();
    let new_track_id = p.alloc_id();
    let mut chosen = None;
    edit_tracks(app, |tracks, _| {
        let idx = track.and_then(|t| tracks.iter().position(|x| x.id == t)).unwrap_or_else(|| {
            tracks.push(AudioTrack::new(new_track_id, track_name));
            tracks.len() - 1
        });
        chosen = Some(tracks[idx].id);
        let end = start + len as i64;
        if replace_takes {
            for c in &mut tracks[idx].clips {
                if c.start < end && c.end() > start {
                    c.muted = true;
                }
            }
        }
        tracks[idx].clips.push(AudioClip { id: clip_id, asset: asset_id, start, offset: 0, len, gain: 1.0, fade_in: 0, fade_out: 0, muted: false });
    });
    app.audio_ui.target_track = chosen;
    app.audio_ui.selected = Some(clip_id);
    app.audio_ui.show = true;
    clip_id
}

fn current_sample(app: &TrazoApp) -> i64 {
    let p = &app.doc.project;
    ticks_to_samples(p.tick_of_frame(app.doc.frame) as f64, p.canvas.fps)
}

/// Decode in a background thread so long MP3s don't freeze the app.
pub fn import_audio_files(app: &mut TrazoApp, ctx: &egui::Context, files: Vec<std::path::PathBuf>) {
    if files.is_empty() {
        return;
    }
    app.stop_playback();
    let start = current_sample(app);
    let track = app.audio_ui.target_track;
    let (tx, rx) = mpsc::channel();
    let ctx2 = ctx.clone();
    std::thread::spawn(move || {
        let mut at = start;
        for path in files {
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Audio".into());
            let result = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| trazo_core::audio::decode_audio(b).map_err(|e| e.to_string()));
            let len = result.as_ref().map(|s| (s.len() / 2) as i64).unwrap_or(0);
            if tx.send(ImportResult { name, path, start: at, track, result }).is_err() {
                return;
            }
            at += len; // several files go one after another
            ctx2.request_repaint();
        }
    });
    app.audio_ui.imports.push(rx);
    app.toast(ctx, "Importando audio…");
}

fn poll_imports(app: &mut TrazoApp, ctx: &egui::Context) {
    let mut done = Vec::new();
    let mut keep = Vec::new();
    for rx in std::mem::take(&mut app.audio_ui.imports) {
        let mut alive = true;
        loop {
            match rx.try_recv() {
                Ok(r) => done.push(r),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    alive = false;
                    break;
                }
            }
        }
        if alive {
            keep.push(rx);
        }
    }
    app.audio_ui.imports = keep;
    for r in done {
        match r.result {
            Ok(samples) => {
                let track = r.track.filter(|t| app.doc.project.audio.track_index(*t).is_some());
                add_clip(app, &r.name, samples, r.start, track, "Música y efectos", false);
                app.platform.imported(&r.path);
                app.toast(ctx, format!("Audio importado: {}", r.name));
            }
            Err(e) => {
                // Don't retry a broken file forever (Android import folder).
                app.platform.imported(&r.path);
                app.toast(ctx, format!("No se pudo importar {}: {e}", r.name));
            }
        }
    }
}

// ---- voice-over -----------------------------------------------------------------

fn start_recording(app: &mut TrazoApp, ctx: &egui::Context) {
    if app.audio.is_none() {
        app.toast(ctx, "No hay dispositivo de audio");
        return;
    }
    #[cfg(target_os = "android")]
    if !crate::android::has_permission(crate::android::RECORD_AUDIO) {
        // Ask, then start automatically once it is granted (see update_recording).
        if let Err(e) = crate::android::request_permissions(&[crate::android::RECORD_AUDIO]) {
            app.toast(ctx, format!("No se pudo pedir el permiso de micrófono: {e}"));
            return;
        }
        let since = ctx.input(|i| i.time);
        let track = app.audio_ui.target_track;
        app.audio_ui.record = RecordState::WaitingPermission { since, track };
        return;
    }
    let Some(engine) = app.audio.as_mut() else { return };
    // Open the microphone now so it is warm when recording starts.
    if let Err(e) = engine.arm_recording() {
        app.toast(ctx, format!("No se pudo abrir el micrófono: {e}"));
        return;
    }
    app.stop_playback();
    app.canvas.select.commit(&mut app.doc);
    let now = ctx.input(|i| i.time);
    let track = app.audio_ui.target_track.filter(|t| app.doc.project.audio.track_index(*t).is_some());
    app.audio_ui.record = RecordState::Countdown { until: now + COUNTDOWN, track };
}

/// Stop without keeping the take (Esc, project change, app exit).
pub fn cancel_recording(app: &mut TrazoApp) {
    if let RecordState::Recording { prev_loop, .. } = app.audio_ui.record {
        app.playback.looping = prev_loop;
    }
    app.audio_ui.record = RecordState::Idle;
    if let Some(e) = app.audio.as_mut() {
        e.cancel_recording();
    }
    app.stop_playback();
}

fn stop_recording(app: &mut TrazoApp, ctx: &egui::Context) {
    let state = std::mem::replace(&mut app.audio_ui.record, RecordState::Idle);
    let RecordState::Recording { start, track, prev_loop, capturing } = state else {
        cancel_recording(app);
        return;
    };
    app.stop_playback();
    app.playback.looping = prev_loop;
    let overflow = app.audio.as_ref().is_some_and(|e| e.recording_overflowed());
    let take = app.audio.as_mut().and_then(|e| e.stop_recording());
    match take {
        Some(samples) if capturing && samples.len() > 2 * 2400 => {
            let n = app.doc.project.audio.tracks.iter().map(|t| t.clips.len()).sum::<usize>() + 1;
            let shift = (app.audio_ui.record_offset_ms / 1000.0 * SAMPLE_RATE as f32) as i64;
            add_clip(app, &format!("Toma {n}"), samples, start - shift, track, "Voz en off", true);
            let msg = if overflow { "Grabación añadida (se perdió un trozo: el equipo iba lento)" } else { "Grabación añadida (Ctrl+Z para descartarla)" };
            app.toast(ctx, msg);
        }
        _ => app.toast(ctx, "Grabación demasiado corta: no se guardó"),
    }
}

/// Called every frame: imports, countdown, pre-roll, then capture.
pub fn update_recording(app: &mut TrazoApp, ctx: &egui::Context) {
    poll_imports(app, ctx);
    if app.audio_ui.is_importing() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let now = ctx.input(|i| i.time);
    match app.audio_ui.record {
        RecordState::WaitingPermission { since, .. } => {
            #[cfg(target_os = "android")]
            if crate::android::has_permission(crate::android::RECORD_AUDIO) {
                app.audio_ui.record = RecordState::Idle;
                start_recording(app, ctx);
                return;
            }
            if now - since > 120.0 {
                app.audio_ui.record = RecordState::Idle;
                app.toast(ctx, "Sin permiso de micrófono no se puede grabar");
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        RecordState::Countdown { until, track } => {
            if now >= until {
                // The take goes exactly where playback reaches after the pre-roll.
                let start = current_sample(app);
                let prev_loop = app.playback.looping;
                app.audio_ui.record = RecordState::Recording { start, track, prev_loop, capturing: false };
                app.playback.looping = false;
                let fps = app.doc.project.canvas.fps;
                let from = (start - (app.audio_ui.preroll * SAMPLE_RATE as f32) as i64).max(0);
                app.play_from_tick(ctx, samples_to_ticks(from, fps));
            }
            ctx.request_repaint();
        }
        RecordState::Recording { start, ref mut capturing, .. } => {
            if !*capturing {
                let reached = app.audio.as_ref().map(|e| e.position() >= start || !e.is_playing()).unwrap_or(true);
                if reached {
                    *capturing = true;
                    if let Some(e) = app.audio.as_mut() {
                        e.begin_capture();
                    }
                }
            }
            if let Some(e) = app.audio.as_mut() {
                e.drain_recording();
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        RecordState::Idle => {}
    }
}

/// Save the whole mix (animation length) as a WAV file.
fn export_mix(app: &mut TrazoApp, ctx: &egui::Context) {
    let p = &app.doc.project;
    let end = ticks_to_samples(p.total_ticks() as f64, p.canvas.fps).max(p.audio.end());
    let name = format!("{} (audio).wav", p.name);
    let Some(path) = app.platform.save_file("Exportar audio", &name, "wav") else { return };
    let samples = render_mix(&p.audio, 0, end);
    match trazo_core::audio::encode_wav(&samples).map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
        Ok(()) => app.toast(ctx, format!("Audio exportado: {}", path.display())),
        Err(e) => app.toast(ctx, format!("No se pudo exportar: {e}")),
    }
}

// ---- UI ---------------------------------------------------------------------------

/// Size limits and current height of the panel for this screen.
fn heights(app: &TrazoApp, screen_h: f32) -> (f32, f32) {
    let (lo, hi) = (110.0, (screen_h * 0.7).max(160.0));
    let auto = if screen_h < 520.0 { screen_h * 0.45 } else { 230.0f32.min(screen_h * 0.4) };
    (app.audio_ui.height.unwrap_or(auto).clamp(lo, hi), lo)
}

fn body(app: &mut TrazoApp, ui: &mut egui::Ui, height: f32, lo: f32) {
    grab_bar(app, ui, height, lo);
    crate::panels::timeline::commit_selection_on_click(app, ui);
    crate::panels::phone::tabs_header(app, ui);
    // Never taller than the panel: the rest scrolls.
    egui::ScrollArea::vertical().id_salt("audio_body").max_height(ui.available_height()).auto_shrink([false, true]).show(ui, |ui| {
        ui.add_space(3.0);
        controls(app, ui);
        ui.separator();
        tracks(app, ui);
    });
}

/// Audio as a bottom panel (the canvas gets the rest of the screen).
pub fn show(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let (height, lo) = heights(app, ui.ctx().content_rect().height());
    egui::Panel::bottom("audio_panel_drag")
        .resizable(false)
        .exact_size(height)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(crate::theme::SURFACE).inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 4 }))
        .show(ui, |ui| body(app, ui, height, lo));
}

/// Audio floating over the bottom of the screen (phone on its side).
/// Returns the top edge, so other floating controls can sit above it.
pub fn show_overlay(app: &mut TrazoApp, ctx: &egui::Context, bottom_inset: f32, side: f32) -> Option<f32> {
    if !app.audio_ui.show {
        return None;
    }
    let screen = ctx.content_rect();
    let (height, lo) = heights(app, screen.height());
    let rect = Rect::from_min_max(Pos2::new(screen.left() + side, screen.bottom() - bottom_inset - height), Pos2::new(screen.right() - side, screen.bottom() - bottom_inset));
    egui::Area::new(egui::Id::unique("audio_overlay")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        egui::Frame::new()
            .fill(crate::theme::SURFACE)
            .corner_radius(egui::CornerRadius { nw: crate::theme::R_SHEET, ne: crate::theme::R_SHEET, sw: 0, se: 0 })
            .shadow(egui::Shadow { offset: [0, -6], blur: 24, spread: 0, color: Color32::from_black_alpha(120) })
            .inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 4 })
            .show(ui, |ui| {
                ui.set_width(rect.width() - 16.0);
                ui.set_height(rect.height() - 4.0);
                ui.set_max_height(rect.height() - 4.0);
                body(app, ui, height, lo);
            });
    });
    Some(rect.top())
}

/// Phone-sized screen (labels give way to icons).
fn compact(ui: &egui::Ui) -> bool {
    let r = ui.ctx().content_rect();
    r.width() < 600.0 || r.height() < 520.0
}

/// Handle on top of the panel: drag to resize, drag down to hide; × closes.
fn grab_bar(app: &mut TrazoApp, ui: &mut egui::Ui, height: f32, min: f32) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click_and_drag());
    let p = ui.painter();
    p.rect_filled(Rect::from_center_size(rect.center(), Vec2::new(40.0, 5.0)), 3.0, crate::theme::CONTROL);
    let start_id = ui.scope_id().with("audio_drag_start");
    if resp.drag_started() {
        ui.data_mut(|d| d.insert_temp(start_id, height));
    }
    if resp.dragged() {
        // Measured from where the gesture began, so a long pull down hides it.
        let start = ui.data(|d| d.get_temp::<f32>(start_id)).unwrap_or(height);
        let moved = match (ui.input(|i| i.pointer.press_origin()), resp.interact_pointer_pos()) {
            (Some(a), Some(b)) => b.y - a.y,
            _ => resp.drag_delta().y,
        };
        let new = start - moved;
        ui.ctx().request_repaint();
        if new < min - 50.0 {
            app.audio_ui.show = false;
            app.audio_ui.height = None;
        } else {
            app.audio_ui.height = Some(new);
        }
    }
    if resp.double_clicked() {
        app.audio_ui.height = None;
    }
    let close = Rect::from_center_size(Pos2::new(rect.right() - 22.0, rect.center().y), Vec2::splat(28.0));
    let cr = ui.interact(close, ui.scope_id().with("audio_close"), Sense::click());
    crate::icons::paint(ui.painter(), close.shrink(6.0), Icon::Close, crate::theme::TEXT2);
    if cr.on_hover_text("Ocultar audio").clicked() {
        ui.ctx().request_repaint();
        app.audio_ui.show = false;
    }
    resp.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text("Arrastra para cambiar la altura; hacia abajo para ocultar");
}

fn controls(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let small = compact(ui);
    ui.horizontal_wrapped(|ui| {
        // The tab already says "Audio".
        if app.audio.is_none() {
            ui.label(RichText::new("(sin dispositivo de sonido)").weak());
        }

        match app.audio_ui.record {
            RecordState::WaitingPermission { .. } => {
                ui.label(RichText::new("Concede el permiso de micrófono en el aviso de Android…").strong());
                if ui.add(IconButton::new(Icon::Close).keep_text().text("Cancelar")).clicked() {
                    app.audio_ui.record = RecordState::Idle;
                }
            }
            RecordState::Idle => {
                if ui.add(IconButton::new(Icon::Mic).keep_text().text("Grabar voz")).on_hover_text("Graba desde el micrófono mientras se reproduce la animación (cuenta atrás y pre-roll)").clicked() {
                    start_recording(app, &ctx);
                }
            }
            RecordState::Countdown { until, .. } => {
                let left = (until - ctx.input(|i| i.time)).ceil().max(1.0);
                ui.label(RichText::new(format!("Grabando en {left:.0}…")).strong().color(Color32::from_rgb(240, 80, 80)));
                level_meter(ui, app.audio.as_ref().map(|e| e.input_level()).unwrap_or(0.0));
                if ui.add(IconButton::new(Icon::Close).keep_text().text("Cancelar")).clicked() {
                    cancel_recording(app);
                }
            }
            RecordState::Recording { capturing, .. } => {
                if ui.add(IconButton::new(Icon::Stop).keep_text().text("Detener").selected(true)).clicked() {
                    stop_recording(app, &ctx);
                }
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                let red = if capturing { Color32::from_rgb(230, 50, 50) } else { Color32::from_rgb(120, 60, 60) };
                ui.painter().circle_filled(rect.center(), 6.0, red);
                if capturing {
                    let secs = app.audio.as_ref().map(|e| e.recorded_seconds()).unwrap_or(0.0);
                    ui.label(format!("{secs:.1} s"));
                } else {
                    ui.label("pre-roll…");
                }
                level_meter(ui, app.audio.as_ref().map(|e| e.input_level()).unwrap_or(0.0));
            }
        }
        if !small {
            ui.checkbox(&mut app.audio_ui.monitor, "Oír pistas al grabar").on_hover_text("Usa auriculares para que el micrófono no recoja el sonido");
            ui.separator();
        }

        if ui.add(IconButton::new(Icon::Import).text("Importar")).on_hover_text("WAV, MP3, OGG, FLAC, M4A…: se coloca en el fotograma actual").clicked() {
            crate::files::request(app, &ctx, crate::files::PickPurpose::Audio);
        }
        if app.audio_ui.is_importing() {
            ui.spinner();
        }
        if ui.add(IconButton::new(Icon::Plus).text("Pista")).on_hover_text("Nueva pista de audio").clicked() {
            let id = app.doc.project.alloc_id();
            let n = app.doc.project.audio.tracks.len() + 1;
            edit_tracks(app, |t, _| t.push(AudioTrack::new(id, format!("Pista {n}"))));
            app.audio_ui.target_track = Some(id);
        }
        if ui.add(IconButton::new(Icon::RangeIn).text("Marcador")).on_hover_text("Marcador en el fotograma actual (K): diálogo, golpes, fonemas").clicked() {
            add_marker(app);
        }
        let scrub_icon = if app.audio_ui.scrub { Icon::Speaker } else { Icon::SpeakerOff };
        if ui.add(IconButton::new(scrub_icon).selected(app.audio_ui.scrub)).on_hover_text("Oír el sonido de cada fotograma al recorrerlos (lip-sync)").clicked() {
            app.audio_ui.scrub = !app.audio_ui.scrub;
        }
        ui.add(egui::Slider::new(&mut app.audio_ui.volume, 0.0..=2.0).show_value(false)).on_hover_text("Volumen de escucha");
        if ui.add(IconButton::new(Icon::Export).text("WAV")).on_hover_text("Exportar la mezcla de audio a un archivo WAV").clicked() {
            export_mix(app, &ctx);
        }
        let sync = ui.add(IconButton::new(Icon::Settings)).on_hover_text("Sincronía y grabación");
        egui::Popup::menu(&sync).show(|ui| {
            ui.set_min_width(300.0);
            if small {
                crate::widgets::switch_row(ui, "Oír pistas al grabar", &mut app.audio_ui.monitor);
            }
            if let Some(e) = app.audio.as_mut() {
                ui.label(format!("Latencia de salida detectada: {:.0} ms (ya compensada)", e.latency_ms()));
                ui.add(egui::Slider::new(&mut e.av_offset_ms, -300.0..=300.0).suffix(" ms").text("Ajuste imagen/sonido"))
                    .on_hover_text("Si el sonido llega tarde respecto al dibujo, sube este valor");
            }
            ui.add(egui::Slider::new(&mut app.audio_ui.record_offset_ms, -300.0..=300.0).suffix(" ms").text("Desfase de grabación"))
                .on_hover_text("Si tus tomas quedan tarde, sube este valor");
            ui.add(egui::Slider::new(&mut app.audio_ui.preroll, 0.0..=4.0).suffix(" s").text("Pre-roll"))
                .on_hover_text("Segundos que suenan antes del punto de grabación");
            ui.label(RichText::new("Consejo: Mayús+arrastrar en la regla marca el bucle; arrastrar sobre las pistas hace scrub.").small().weak());
        });
        if !small {
            ui.separator();
            ui.label("Zoom");
        }
        ui.add(egui::Slider::new(&mut app.audio_ui.pps, 20.0..=1200.0).logarithmic(true).show_value(false)).on_hover_text("También con Ctrl+rueda sobre las pistas");
    });

    if let Some(id) = app.audio_ui.selected_marker {
        if app.doc.project.audio.markers.iter().any(|m| m.id == id) {
            marker_controls(app, ui, id);
        } else {
            app.audio_ui.selected_marker = None;
        }
    }
    if let Some(id) = app.audio_ui.selected {
        if app.doc.project.audio.find_clip(id).is_none() {
            app.audio_ui.selected = None;
        } else {
            clip_controls(app, ui, id);
        }
    }
}

pub fn add_marker(app: &mut TrazoApp) {
    let at = current_sample(app);
    let id = app.doc.project.alloc_id();
    let n = app.doc.project.audio.markers.len() + 1;
    edit_markers(app, |m| m.push(Marker { id, at, text: format!("M{n}"), phoneme: None }));
    app.audio_ui.selected_marker = Some(id);
    app.audio_ui.show = true;
}

fn marker_controls(app: &mut TrazoApp, ui: &mut egui::Ui, id: u64) {
    let Some(m) = app.doc.project.audio.markers.iter().find(|m| m.id == id).cloned() else { return };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Marcador").strong());
        let key = egui::Id::unique(("marker_text", id));
        let mut text = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_else(|| m.text.clone());
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0).hint_text("diálogo o nota"));
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(key, text.clone()));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(key));
            if text != m.text {
                edit_markers(app, |ms| {
                    if let Some(x) = ms.iter_mut().find(|x| x.id == id) {
                        x.text = text.clone();
                    }
                });
            }
        }
        ui.label("Fonema:");
        for ph in std::iter::once(None).chain(Phoneme::ALL.iter().copied().map(Some)) {
            let label = ph.map(|p| p.label()).unwrap_or("—");
            if ui.selectable_label(m.phoneme == ph, label).clicked() && m.phoneme != ph {
                edit_markers(app, |ms| {
                    if let Some(x) = ms.iter_mut().find(|x| x.id == id) {
                        x.phoneme = ph;
                    }
                });
            }
        }
        if ui.add(IconButton::new(Icon::Trash)).on_hover_text("Borrar marcador").clicked() {
            edit_markers(app, |ms| ms.retain(|x| x.id != id));
            app.audio_ui.selected_marker = None;
        }
        if ui.add(IconButton::new(Icon::Close)).on_hover_text("Cerrar").clicked() {
            app.audio_ui.selected_marker = None;
        }
    });
}

fn level_meter(ui: &mut egui::Ui, level: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(90.0, 10.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, Color32::from_gray(50));
    let w = rect.width() * level.clamp(0.0, 1.0).sqrt();
    let c = if level > 0.9 { Color32::from_rgb(240, 70, 60) } else if level > 0.5 { Color32::from_rgb(240, 200, 60) } else { Color32::from_rgb(90, 200, 110) };
    ui.painter().rect_filled(Rect::from_min_size(rect.min, Vec2::new(w, rect.height())), 3.0, c);
}

fn clip_controls(app: &mut TrazoApp, ui: &mut egui::Ui, id: ClipId) {
    let Some((ti, clip)) = app.doc.project.audio.find_clip(id).map(|(t, c)| (t, c.clone())) else { return };
    ui.horizontal_wrapped(|ui| {
        let asset = app.doc.project.audio.assets.get(&clip.asset).cloned();
        let name = asset.as_ref().map(|a| a.name.clone()).unwrap_or_default();
        ui.label(RichText::new(format!("Clip: {name}")).strong());
        let secs = |s: u64| s as f32 / SAMPLE_RATE as f32;
        let mut gain = clip.gain * 100.0;
        let mut fin = secs(clip.fade_in);
        let mut fout = secs(clip.fade_out);
        let max_fade = secs(clip.len).min(10.0);
        let r1 = ui.add(egui::Slider::new(&mut gain, 0.0..=200.0).suffix(" %").text("Volumen"));
        let r2 = ui.add(egui::Slider::new(&mut fin, 0.0..=max_fade).suffix(" s").text("Entrada"));
        let r3 = ui.add(egui::Slider::new(&mut fout, 0.0..=max_fade).suffix(" s").text("Salida"));
        let rs = [&r1, &r2, &r3];
        let changed = rs.iter().any(|r| r.changed());
        let stopped = rs.iter().any(|r| r.drag_stopped());
        let dragging = rs.iter().any(|r| r.dragged());
        if changed {
            if app.audio_ui.slider_before.is_none() {
                app.audio_ui.slider_before = Some(app.doc.project.audio.tracks.clone());
            }
            let frames = asset.as_ref().map(|a| a.frames()).unwrap_or(clip.offset + clip.len);
            if let Some(c) = app.doc.project.audio.tracks[ti].clip_mut(id) {
                c.gain = gain / 100.0;
                c.fade_in = (fin * SAMPLE_RATE as f32) as u64;
                c.fade_out = (fout * SAMPLE_RATE as f32) as u64;
                c.clamp_to(frames);
            }
            app.doc.audio_version += 1;
        }
        if stopped || (changed && !dragging) {
            commit_slider(app);
        }

        ui.separator();
        let playhead = current_sample(app);
        if ui.add_enabled(playhead > clip.start && playhead < clip.end(), IconButton::new(Icon::Cut).text("Dividir")).on_hover_text("Cortar el clip en el fotograma actual").clicked() {
            let new_id = app.doc.project.alloc_id();
            edit_tracks(app, |t, _| {
                if let Some(pos) = t[ti].clips.iter().position(|c| c.id == id) {
                    if let Some((a, b)) = t[ti].clips[pos].split(playhead, new_id) {
                        t[ti].clips[pos] = a;
                        t[ti].clips.insert(pos + 1, b);
                    }
                }
            });
        }
        if let Some(asset) = &asset {
            if ui.button("Separar en silencios").on_hover_text("Corta el clip donde empieza cada frase").clicked() {
                let cuts: Vec<i64> = sound_starts(&asset.peaks, 0.02, SAMPLE_RATE as u64 / 4)
                    .into_iter()
                    .filter(|&f| f > clip.offset && f < clip.offset + clip.len)
                    .map(|f| clip.start + (f - clip.offset) as i64)
                    .collect();
                if cuts.is_empty() {
                    app.toast(ui.ctx(), "No se encontraron silencios");
                } else {
                    let ids: Vec<u64> = cuts.iter().map(|_| app.doc.project.alloc_id()).collect();
                    edit_tracks(app, |t, _| {
                        let mut current = id;
                        for (at, nid) in cuts.iter().zip(&ids) {
                            if let Some(pos) = t[ti].clips.iter().position(|c| c.id == current) {
                                if let Some((a, b)) = t[ti].clips[pos].split(*at, *nid) {
                                    t[ti].clips[pos] = a;
                                    t[ti].clips.insert(pos + 1, b);
                                    current = *nid;
                                }
                            }
                        }
                    });
                }
            }
            if ui.button("Normalizar").on_hover_text("Sube el volumen hasta el máximo sin distorsionar").clicked() {
                match normalized(&asset.samples, 0.95) {
                    Some(samples) => {
                        let new_asset = app.doc.project.alloc_id();
                        app.doc.project.audio.assets.insert(new_asset, Arc::new(AudioAsset::new(asset.name.clone(), samples)));
                        edit_tracks(app, |t, _| {
                            if let Some(c) = t[ti].clip_mut(id) {
                                c.asset = new_asset;
                            }
                        });
                    }
                    None => app.toast(ui.ctx(), "El clip está en silencio"),
                }
            }
        }
        if ui.add(IconButton::new(Icon::Duplicate)).on_hover_text("Duplicar el clip a continuación").clicked() {
            let new_id = app.doc.project.alloc_id();
            edit_tracks(app, |t, _| t[ti].clips.push(AudioClip { id: new_id, start: clip.end(), ..clip.clone() }));
            app.audio_ui.selected = Some(new_id);
        }
        let mute_icon = if clip.muted { Icon::SpeakerOff } else { Icon::Speaker };
        if ui.add(IconButton::new(mute_icon).selected(clip.muted)).on_hover_text("Silenciar este clip (las tomas sustituidas quedan silenciadas)").clicked() {
            edit_tracks(app, |t, _| {
                if let Some(c) = t[ti].clip_mut(id) {
                    c.muted = !c.muted;
                }
            });
        }
        if ui.add(IconButton::new(Icon::Trash)).on_hover_text("Borrar el clip (Supr)").clicked() {
            edit_tracks(app, |t, _| t[ti].clips.retain(|c| c.id != id));
            app.audio_ui.selected = None;
        }
        if ui.add(IconButton::new(Icon::Close)).on_hover_text("Deseleccionar").clicked() {
            app.audio_ui.selected = None;
        }
    });
}

fn commit_slider(app: &mut TrazoApp) {
    if let Some(before) = app.audio_ui.slider_before.take() {
        let after = app.doc.project.audio.tracks.clone();
        if before != after {
            app.doc.history.push_applied(Edit::Audio { before, after });
        }
    }
}

fn tracks(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let n = app.doc.project.audio.tracks.len();
    if n == 0 && app.doc.project.audio.markers.is_empty() {
        ui.label(RichText::new("Importa música o graba tu voz para empezar. Al reproducir, la animación sigue al sonido.").weak());
        return;
    }
    let fps = app.doc.project.canvas.fps.max(1);
    let anim_end = ticks_to_samples(app.doc.project.total_ticks() as f64, fps);
    let content_end = anim_end.max(app.doc.project.audio.end()) + SAMPLE_RATE as i64 * 2;
    let pps = app.audio_ui.pps;
    let width = content_end as f32 / SAMPLE_RATE as f32 * pps;
    let height = RULER_H + MARKER_H + n as f32 * LANE_H;
    let mut delete_track = None;

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(HEADER_W);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.allocate_ui_with_layout(Vec2::new(HEADER_W, RULER_H + MARKER_H), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_min_height(RULER_H + MARKER_H);
                ui.add_space(RULER_H);
                ui.label(RichText::new("Marcadores (K)").small().weak());
            });
            for i in 0..n {
                // Fixed-height rows so headers line up with their lanes.
                ui.allocate_ui_with_layout(Vec2::new(HEADER_W, LANE_H), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_height(LANE_H);
                    if track_header(app, ui, i) {
                        delete_track = Some(i);
                    }
                });
            }
        });
        let scroll = egui::ScrollArea::horizontal().id_salt("audio_lanes").show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(width.max(ui.available_width()), height), Sense::click_and_drag());
            lanes(app, ui, rect, &resp, anim_end);
        });
        // Ctrl + wheel zooms the audio lanes.
        let lanes_rect = scroll.inner_rect;
        let (zoom, hover) = ui.input(|i| (i.zoom_delta(), i.pointer.hover_pos()));
        if zoom != 1.0 && hover.is_some_and(|p| lanes_rect.contains(p)) {
            app.audio_ui.pps = (app.audio_ui.pps * zoom).clamp(20.0, 1200.0);
        }
    });
    // Deleting inside the header loop would invalidate the indices.
    if let Some(i) = delete_track {
        edit_tracks(app, |tr, _| {
            if i < tr.len() {
                tr.remove(i);
            }
        });
    }
}

/// Track name, mute, solo, volume. Returns true if "delete" was pressed.
fn track_header(app: &mut TrazoApp, ui: &mut egui::Ui, i: usize) -> bool {
    let mut delete = false;
    let t = app.doc.project.audio.tracks[i].clone();
    let target = app.audio_ui.target_track == Some(t.id);
    let frame = egui::Frame::new()
        .fill(if target { crate::theme::ACCENT.gamma_multiply(0.18) } else { ui.visuals().faint_bg_color })
        .inner_margin(4.0)
        .corner_radius(4.0);
    frame.show(ui, |ui| {
        ui.set_height(LANE_H - 10.0);
        ui.set_width(HEADER_W - 8.0);
        ui.vertical(|ui| {
            ui.horizontal(|ui| match &mut app.audio_ui.renaming {
                Some((id, text)) if *id == t.id => {
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(110.0));
                    if r.lost_focus() {
                        let name = text.trim().to_string();
                        app.audio_ui.renaming = None;
                        if !name.is_empty() {
                            edit_tracks(app, |tr, _| tr[i].name = name);
                        }
                    } else {
                        r.request_focus();
                    }
                }
                _ => {
                    let r = ui.add(egui::Label::new(RichText::new(&t.name).strong()).sense(Sense::click())).on_hover_text("Clic: pista de destino para grabar e importar · doble clic: renombrar");
                    if r.double_clicked() {
                        app.audio_ui.renaming = Some((t.id, t.name.clone()));
                    } else if r.clicked() {
                        app.audio_ui.target_track = Some(t.id);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let mute_icon = if t.muted { Icon::SpeakerOff } else { Icon::Speaker };
                if ui.add(IconButton::new(mute_icon).size(24.0).selected(t.muted)).on_hover_text("Silenciar pista").clicked() {
                    edit_tracks(app, |tr, _| tr[i].muted = !tr[i].muted);
                }
                if ui.add(egui::Button::new(RichText::new("S").strong()).selected(t.solo).min_size(Vec2::splat(24.0))).on_hover_text("Solo: escuchar solo esta pista").clicked() {
                    edit_tracks(app, |tr, _| tr[i].solo = !tr[i].solo);
                }
                let mut vol = (t.gain * 100.0).round() as i32;
                let r = ui.add(egui::DragValue::new(&mut vol).range(0..=200).suffix("%")).on_hover_text("Volumen de la pista");
                if r.changed() {
                    if app.audio_ui.slider_before.is_none() {
                        app.audio_ui.slider_before = Some(app.doc.project.audio.tracks.clone());
                    }
                    app.doc.project.audio.tracks[i].gain = vol as f32 / 100.0;
                    app.doc.audio_version += 1;
                }
                if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged()) {
                    commit_slider(app);
                }
                if ui.add(IconButton::new(Icon::Trash).size(24.0)).on_hover_text("Borrar pista").clicked() {
                    delete = true;
                }
            });
        });
    });
    delete
}

fn lanes(app: &mut TrazoApp, ui: &mut egui::Ui, rect: Rect, resp: &egui::Response, anim_end: i64) {
    let pps = app.audio_ui.pps;
    let fps = app.doc.project.canvas.fps.max(1);
    let x_of = |s: i64| rect.min.x + s as f32 / SAMPLE_RATE as f32 * pps;
    let s_of = |x: f32| (((x - rect.min.x) / pps) * SAMPLE_RATE as f32) as i64;
    let painter = ui.painter_at(rect);
    let visible = ui.clip_rect().intersect(rect);
    let lanes_top = rect.min.y + RULER_H + MARKER_H;

    // ---- ruler: one cell per drawing (holds included), numbered ----
    let ruler = Rect::from_min_size(rect.min, Vec2::new(rect.width(), RULER_H));
    painter.rect_filled(ruler, 0.0, Color32::from_gray(38));
    if let Some((a, b)) = app.playback.range.map(|_| app.playback.bounds(app.doc.project.frames.len())) {
        let p = &app.doc.project;
        let x0 = x_of(ticks_to_samples(p.tick_of_frame(a) as f64, fps));
        let x1 = x_of(ticks_to_samples((p.tick_of_frame(b) + p.frames[b].hold.max(1)) as f64, fps));
        painter.rect_filled(Rect::from_min_max(Pos2::new(x0, rect.min.y), Pos2::new(x1, rect.max.y)), 0.0, Color32::from_rgba_unmultiplied(255, 120, 70, 22));
    }
    {
        let p = &app.doc.project;
        let shown = app.displayed_frame();
        let mut tick = 0u32;
        let mut last_label_x = f32::MIN;
        for (i, f) in p.frames.iter().enumerate() {
            let hold = f.hold.max(1);
            let x0 = x_of(ticks_to_samples(tick as f64, fps));
            let x1 = x_of(ticks_to_samples((tick + hold) as f64, fps));
            tick += hold;
            if x1 < visible.min.x || x0 > visible.max.x {
                continue;
            }
            if i == shown {
                painter.rect_filled(Rect::from_min_max(Pos2::new(x0, ruler.min.y), Pos2::new(x1, rect.max.y)), 0.0, Color32::from_rgba_unmultiplied(255, 120, 70, 28));
            }
            painter.line_segment([Pos2::new(x0, ruler.min.y + 6.0), Pos2::new(x0, ruler.max.y)], Stroke::new(1.0, Color32::from_gray(130)));
            painter.line_segment([Pos2::new(x0, lanes_top), Pos2::new(x0, rect.max.y)], Stroke::new(1.0, Color32::from_white_alpha(9)));
            if x0 - last_label_x >= 14.0 {
                painter.text(Pos2::new(x0 + 2.0, ruler.min.y + 1.0), egui::Align2::LEFT_TOP, format!("{}", i + 1), egui::FontId::proportional(10.0), Color32::from_gray(200));
                last_label_x = x0;
            }
        }
    }
    let first_sec = (s_of(visible.min.x).max(0) / SAMPLE_RATE as i64) as u64;
    let last_sec = (s_of(visible.max.x) / SAMPLE_RATE as i64).max(0) as u64 + 1;
    for sec in first_sec..=last_sec {
        let x = x_of(sec as i64 * SAMPLE_RATE as i64);
        painter.text(Pos2::new(x + 2.0, ruler.max.y - 1.0), egui::Align2::LEFT_BOTTOM, format!("{sec}s"), egui::FontId::proportional(9.0), Color32::from_gray(140));
    }
    let end_x = x_of(anim_end);
    if end_x < rect.max.x {
        painter.rect_filled(Rect::from_min_max(Pos2::new(end_x, rect.min.y), rect.max), 0.0, Color32::from_black_alpha(70));
        painter.line_segment([Pos2::new(end_x, rect.min.y), Pos2::new(end_x, rect.max.y)], Stroke::new(1.0, Color32::from_gray(140)));
    }

    // ---- markers ----
    let marker_band = Rect::from_min_size(Pos2::new(rect.min.x, rect.min.y + RULER_H), Vec2::new(rect.width(), MARKER_H));
    painter.rect_filled(marker_band, 0.0, Color32::from_gray(31));
    let markers = app.doc.project.audio.markers.clone();
    for m in &markers {
        let x = x_of(m.at);
        let sel = app.audio_ui.selected_marker == Some(m.id);
        let col = if sel { crate::theme::ACCENT } else { Color32::from_rgb(250, 200, 80) };
        painter.line_segment([Pos2::new(x, marker_band.min.y), Pos2::new(x, rect.max.y)], Stroke::new(1.0, col.gamma_multiply(0.5)));
        painter.add(egui::Shape::convex_polygon(
            vec![Pos2::new(x, marker_band.min.y + 2.0), Pos2::new(x + 8.0, marker_band.min.y + 6.0), Pos2::new(x, marker_band.min.y + 10.0)],
            col,
            Stroke::NONE,
        ));
        let label = match m.phoneme {
            Some(p) if m.text.is_empty() => p.label().to_string(),
            Some(p) => format!("{} · {}", p.label(), m.text),
            None => m.text.clone(),
        };
        painter.text(Pos2::new(x + 10.0, marker_band.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(11.0), col);
    }

    // ---- clips ----
    let tracks = app.doc.project.audio.tracks.clone();
    let lane_rect = |i: usize| Rect::from_min_size(Pos2::new(rect.min.x, lanes_top + i as f32 * LANE_H), Vec2::new(rect.width(), LANE_H));
    let any_solo = tracks.iter().any(|t| t.solo);
    for (i, track) in tracks.iter().enumerate() {
        let lr = lane_rect(i);
        painter.line_segment([Pos2::new(lr.min.x, lr.max.y), Pos2::new(lr.max.x, lr.max.y)], Stroke::new(1.0, Color32::from_gray(55)));
        let audible = !track.muted && (!any_solo || track.solo);
        let hue = if audible { Color32::from_rgb(70, 150, 220) } else { Color32::from_gray(95) };
        for clip in &track.clips {
            let cr = Rect::from_min_max(Pos2::new(x_of(clip.start), lr.min.y + 4.0), Pos2::new(x_of(clip.end()), lr.max.y - 4.0));
            if !cr.intersects(visible) {
                continue;
            }
            let selected = app.audio_ui.selected == Some(clip.id);
            let fill = if clip.muted { Color32::from_gray(70) } else { hue.gamma_multiply(0.55) };
            painter.rect_filled(cr, 5.0, fill);
            if let Some(asset) = app.doc.project.audio.assets.get(&clip.asset) {
                let wave_col = if audible && !clip.muted { Color32::from_rgb(200, 230, 255) } else { Color32::from_gray(150) };
                draw_wave(&painter, cr.intersect(visible), cr, clip, asset, pps, wave_col);
                painter.text(cr.min + Vec2::new(6.0, 3.0), egui::Align2::LEFT_TOP, &asset.name, egui::FontId::proportional(11.0), Color32::WHITE);
            }
            let fade_col = Color32::from_rgba_unmultiplied(255, 255, 255, 140);
            let fx_in = x_of(clip.start + clip.fade_in as i64);
            let fx_out = x_of(clip.end() - clip.fade_out as i64);
            if clip.fade_in > 0 {
                painter.line_segment([Pos2::new(cr.min.x, cr.max.y), Pos2::new(fx_in, cr.min.y)], Stroke::new(1.5, fade_col));
            }
            if clip.fade_out > 0 {
                painter.line_segment([Pos2::new(fx_out, cr.min.y), Pos2::new(cr.max.x, cr.max.y)], Stroke::new(1.5, fade_col));
            }
            if selected {
                for x in [fx_in, fx_out] {
                    painter.rect_filled(Rect::from_center_size(Pos2::new(x, cr.min.y + 1.0), Vec2::splat(FADE_HANDLE)), 2.0, Color32::WHITE);
                }
            }
            let border = if selected { Stroke::new(2.5, crate::theme::ACCENT) } else { Stroke::new(1.0, hue) };
            painter.rect_stroke(cr, 5.0, border, egui::StrokeKind::Inside);
        }
    }

    // ---- playhead ----
    let head = if app.playback.playing && app.audio.as_ref().is_some_and(|e| e.is_playing()) {
        app.audio.as_ref().map(|e| e.position()).unwrap_or(0)
    } else {
        let p = &app.doc.project;
        ticks_to_samples(p.tick_of_frame(app.displayed_frame()) as f64, fps)
    };
    let hx = x_of(head);
    painter.line_segment([Pos2::new(hx, rect.min.y), Pos2::new(hx, rect.max.y)], Stroke::new(2.0, crate::theme::ACCENT));
    if app.playback.playing && (hx < visible.min.x || hx > visible.max.x - 40.0) {
        // Follow the playhead page by page.
        ui.scroll_to_rect(Rect::from_min_size(Pos2::new(hx - 40.0, rect.min.y), Vec2::new(visible.width(), 1.0)), Some(egui::Align::Min));
    }

    // ---- interaction ----
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
    let selected = app.audio_ui.selected;
    let hit_clip = |p: Pos2| -> Option<(usize, ClipId, DragMode)> {
        if p.y < lanes_top {
            return None;
        }
        let ti = ((p.y - lanes_top) / LANE_H) as usize;
        let track = tracks.get(ti)?;
        track.clips.iter().rev().find_map(|c| {
            let (x0, x1) = (x_of(c.start), x_of(c.end()));
            if p.x < x0 - 2.0 || p.x > x1 + 2.0 {
                return None;
            }
            let top = lane_rect(ti).min.y + 4.0;
            if selected == Some(c.id) && (p.y - top).abs() < FADE_HANDLE {
                if (p.x - x_of(c.start + c.fade_in as i64)).abs() < FADE_HANDLE {
                    return Some((ti, c.id, DragMode::FadeIn));
                }
                if (p.x - x_of(c.end() - c.fade_out as i64)).abs() < FADE_HANDLE {
                    return Some((ti, c.id, DragMode::FadeOut));
                }
            }
            let wide = x1 - x0 > EDGE * 3.0;
            let mode = if wide && p.x < x0 + EDGE {
                DragMode::TrimStart
            } else if wide && p.x > x1 - EDGE {
                DragMode::TrimEnd
            } else {
                DragMode::Move
            };
            Some((ti, c.id, mode))
        })
    };
    let hit_marker = |p: Pos2| -> Option<&Marker> {
        if !marker_band.contains(p) {
            return None;
        }
        markers.iter().rev().find(|m| p.x >= x_of(m.at) - 4.0 && p.x <= x_of(m.at) + 60.0)
    };
    if let Some(p) = resp.hover_pos() {
        if let Some((_, _, m)) = hit_clip(p) {
            let icon = if m == DragMode::Move { egui::CursorIcon::Grab } else { egui::CursorIcon::ResizeHorizontal };
            ui.ctx().set_cursor_icon(icon);
        }
    }

    let shift = ui.input(|i| i.modifiers.shift);
    if resp.drag_started() && !app.audio_ui.is_recording() {
        if let Some(p) = pointer {
            app.stop_playback();
            if let Some(m) = hit_marker(p) {
                app.audio_ui.selected_marker = Some(m.id);
                app.audio_ui.drag = Some(LaneDrag::Marker { id: m.id, before: markers.clone(), start_x: p.x, orig_at: m.at });
            } else if let Some((ti, id, mode)) = hit_clip(p) {
                let orig = tracks[ti].clip(id).cloned().expect("clip exists");
                app.audio_ui.selected = Some(id);
                app.audio_ui.drag = Some(LaneDrag::Clip(ClipDrag { clip: id, mode, start_x: p.x, orig, orig_track: ti, before: tracks.clone() }));
            } else if shift && p.y < rect.min.y + RULER_H {
                app.audio_ui.drag = Some(LaneDrag::Range(s_of(p.x).max(0)));
            } else {
                app.audio_ui.drag = Some(LaneDrag::Scrub);
            }
        }
    }

    if let Some(p) = pointer {
        if resp.dragged() {
            let alt = ui.input(|i| i.modifiers.alt);
            let snap = |s: i64| if alt { s } else { ticks_to_samples(samples_to_ticks(s, fps).round(), fps) };
            match &app.audio_ui.drag {
                Some(LaneDrag::Scrub) => {
                    let total = app.doc.project.total_ticks().saturating_sub(1) as f64;
                    let tick = samples_to_ticks(s_of(p.x).max(0), fps).min(total);
                    let f = app.doc.project.frame_at_tick((tick + 1e-6) as u32);
                    app.doc.set_frame(f);
                }
                Some(LaneDrag::Range(from)) => {
                    let to = s_of(p.x).max(0);
                    let (a, b) = ((*from).min(to), (*from).max(to));
                    let pr = &app.doc.project;
                    let last = pr.total_ticks().saturating_sub(1);
                    let fa = pr.frame_at_tick((samples_to_ticks(a, fps) as u32).min(last));
                    let fb = pr.frame_at_tick((samples_to_ticks(b, fps) as u32).min(last));
                    app.playback.range = Some((fa, fb.max(fa)));
                }
                Some(LaneDrag::Marker { id, start_x, orig_at, .. }) => {
                    let d = ((p.x - start_x) / pps * SAMPLE_RATE as f32) as i64;
                    let at = snap((orig_at + d).max(0));
                    let id = *id;
                    if let Some(m) = app.doc.project.audio.markers.iter_mut().find(|m| m.id == id) {
                        m.at = at;
                    }
                }
                Some(LaneDrag::Clip(drag)) => {
                    let d = ((p.x - drag.start_x) / pps * SAMPLE_RATE as f32) as i64;
                    let o = drag.orig.clone();
                    let asset_frames = app.doc.project.audio.assets.get(&o.asset).map(|a| a.frames()).unwrap_or(o.offset + o.len);
                    let mut c = o.clone();
                    let mut target_track = drag.orig_track;
                    match drag.mode {
                        DragMode::Move => {
                            // Never lose a clip off the left edge.
                            c.start = snap(o.start + d).max(0);
                            let ti = ((p.y - lanes_top) / LANE_H).floor();
                            if ti >= 0.0 && (ti as usize) < tracks.len() {
                                target_track = ti as usize;
                            }
                        }
                        DragMode::TrimStart => {
                            let new_start = snap(o.start + d).clamp(o.start - o.offset as i64, o.end() - 1);
                            let delta = new_start - o.start;
                            c.start = new_start;
                            c.offset = (o.offset as i64 + delta) as u64;
                            c.len = (o.len as i64 - delta) as u64;
                        }
                        DragMode::TrimEnd => {
                            let new_end = snap(o.end() + d).clamp(o.start + 1, o.start + (asset_frames - o.offset) as i64);
                            c.len = (new_end - o.start) as u64;
                        }
                        DragMode::FadeIn => c.fade_in = (o.fade_in as i64 + d).max(0) as u64,
                        DragMode::FadeOut => c.fade_out = (o.fade_out as i64 - d).max(0) as u64,
                    }
                    c.clamp_to(asset_frames);
                    let id = drag.clip;
                    let mut tr = drag.before.clone();
                    for t in &mut tr {
                        t.clips.retain(|x| x.id != id);
                    }
                    if target_track < tr.len() {
                        tr[target_track].clips.push(c);
                        app.doc.project.audio.tracks = tr;
                        app.doc.audio_version += 1;
                    }
                }
                None => {}
            }
        }
    }
    if resp.drag_stopped() {
        finish_drag(app);
    }
    if resp.clicked() {
        if let Some(p) = pointer {
            if let Some(m) = hit_marker(p) {
                app.audio_ui.selected_marker = Some(m.id);
            } else if let Some((ti, id, _)) = hit_clip(p) {
                app.audio_ui.selected = Some(id);
                app.audio_ui.target_track = Some(tracks[ti].id);
            } else {
                // Click on the ruler or empty space: jump there.
                app.audio_ui.selected = None;
                let tick = samples_to_ticks(s_of(p.x).max(0), fps);
                app.stop_playback();
                let last = app.doc.project.total_ticks().saturating_sub(1) as f64;
                let f = app.doc.project.frame_at_tick((tick.min(last) + 1e-6) as u32);
                app.doc.set_frame(f);
            }
        }
    }
    // Delete / Backspace removes the selected clip.
    if app.audio_ui.selected.is_some() && resp.hovered() && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
        let id = app.audio_ui.selected.take().expect("checked");
        edit_tracks(app, |t, _| t.iter_mut().for_each(|tr| tr.clips.retain(|c| c.id != id)));
    }
}

/// End a lane drag, recording one undo step for clips or markers.
fn finish_drag(app: &mut TrazoApp) {
    match app.audio_ui.drag.take() {
        Some(LaneDrag::Clip(drag)) => {
            let after = app.doc.project.audio.tracks.clone();
            if after != drag.before {
                app.doc.history.push_applied(Edit::Audio { before: drag.before, after });
            }
        }
        Some(LaneDrag::Marker { before, .. }) => {
            let mut after = app.doc.project.audio.markers.clone();
            after.sort_by_key(|m| m.at);
            app.doc.project.audio.markers = after.clone();
            if after != before {
                app.doc.history.push_applied(Edit::Markers { before, after });
            }
        }
        _ => {}
    }
}

/// Waveform from the precomputed peaks, one vertical line per pixel,
/// scaled by the clip's volume and fades.
fn draw_wave(painter: &egui::Painter, visible: Rect, cr: Rect, clip: &AudioClip, asset: &AudioAsset, pps: f32, color: Color32) {
    if visible.width() <= 0.0 {
        return;
    }
    let mid = cr.center().y;
    let half = cr.height() * 0.42;
    let frames_per_px = SAMPLE_RATE as f32 / pps;
    let peaks = &asset.peaks;
    let mut x = visible.min.x.floor();
    while x < visible.max.x {
        let local = (x - cr.min.x) * frames_per_px;
        let f0 = clip.offset as f32 + local;
        let f1 = f0 + frames_per_px;
        let b0 = f0.max(0.0) as usize / PEAK_BLOCK;
        let b1 = (f1.max(0.0) as usize / PEAK_BLOCK).max(b0 + 1);
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for b in b0..b1.min(peaks.min.len()) {
            lo = lo.min(peaks.min[b]);
            hi = hi.max(peaks.max[b]);
        }
        let g = clip.envelope(local.max(0.0) as u64).min(2.0);
        if hi > lo {
            painter.line_segment([Pos2::new(x, mid - hi * g * half), Pos2::new(x, mid - lo * g * half)], Stroke::new(1.0, color));
        }
        x += 1.0;
    }
}
