//! Application state and top-level layout.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use trazo_core::composite::OnionSkin;
use trazo_core::io::{self, FILE_EXTENSION};
use crate::i18n::{tr, tr_err};
use trazo_core::{CanvasSettings, FrameId, History, IRect, LayerId, Project};

use trazo_core::audio::{samples_to_ticks, ticks_to_samples};

use crate::audio_engine::AudioEngine;
use crate::canvas::CanvasView;
use crate::panels;
use crate::platform::Platform;
use crate::tools::{Tool, ToolState};

/// What part of the canvas texture must be re-rendered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Redraw {
    None,
    Rect(IRect),
    Full,
}

impl Redraw {
    pub fn add(&mut self, r: IRect) {
        *self = match *self {
            Redraw::Full => Redraw::Full,
            Redraw::None => Redraw::Rect(r),
            Redraw::Rect(old) => Redraw::Rect(old.union(&r)),
        };
    }
}

/// The open project plus editing position and change tracking.
pub struct Document {
    pub project: Project,
    pub history: History,
    pub path: PathBuf,
    /// Index of the frame being edited.
    pub frame: usize,
    pub layer: LayerId,
    pub saved_revision: u64,
    pub redraw: Redraw,
    /// Frames whose thumbnails are out of date.
    pub stale_frames: HashSet<FrameId>,
    /// Changes outside the undo history (fps, name, background...).
    pub settings_dirty: bool,
    /// Bumped whenever pixels may have changed (invalidates render caches).
    pub content_version: u64,
    /// Identifies this open document, so late save results for a previous one are ignored.
    pub serial: u64,
    /// Bumped whenever audio tracks change (the audio thread gets a new copy).
    pub audio_version: u64,
}

static DOC_SERIAL: AtomicU64 = AtomicU64::new(1);

impl Document {
    pub fn new(project: Project, path: PathBuf) -> Self {
        let layer = project.layers.last().map(|l| l.id).unwrap_or_default();
        let stale_frames = project.frames.iter().map(|f| f.id).collect();
        Self {
            project,
            history: if cfg!(target_os = "android") { History::with_budget(300, 192 * 1024 * 1024) } else { History::default() },
            path,
            frame: 0,
            layer,
            saved_revision: 0,
            redraw: Redraw::Full,
            stale_frames,
            settings_dirty: false,
            content_version: 0,
            serial: DOC_SERIAL.fetch_add(1, Ordering::Relaxed),
            audio_version: 0,
        }
    }

    pub fn is_modified(&self) -> bool {
        self.history.revision() != self.saved_revision || self.settings_dirty
    }

    /// Record a change that is not part of the undo history.
    pub fn mark_settings_changed(&mut self) {
        self.settings_dirty = true;
        self.content_version += 1;
    }

    pub fn frame_id(&self) -> FrameId {
        self.project.frames[self.frame].id
    }

    pub fn layer_index(&self) -> usize {
        self.project.layer_index(self.layer).unwrap_or(0)
    }

    /// Keep the selection valid after structural changes or undo.
    pub fn validate(&mut self) {
        self.frame = self.frame.min(self.project.frames.len().saturating_sub(1));
        if self.project.layer_index(self.layer).is_none() {
            self.layer = self.project.layers.last().map(|l| l.id).unwrap_or_default();
        }
    }

    /// Everything may have changed (undo, structural edits).
    pub fn invalidate_all(&mut self) {
        self.validate();
        self.redraw = Redraw::Full;
        self.content_version += 1;
        self.stale_frames.extend(self.project.frames.iter().map(|f| f.id));
    }

    /// Pixels of `frame` changed (or of every frame, for a shared layer).
    pub fn invalidate_frame(&mut self, frame: FrameId, rect: Option<IRect>) {
        self.content_version += 1;
        if frame == trazo_core::document::SHARED_FRAME {
            self.stale_frames.extend(self.project.frames.iter().map(|f| f.id));
        } else {
            self.stale_frames.insert(frame);
        }
        match rect {
            Some(r) => self.redraw.add(r),
            None => self.redraw = Redraw::Full,
        }
    }

    pub fn set_frame(&mut self, index: usize) {
        let index = index.min(self.project.frames.len() - 1);
        if index != self.frame {
            self.frame = index;
            self.redraw = Redraw::Full;
        }
    }

    /// Frame durations or order changed: only video pictures depend on it.
    pub fn timing_changed(&mut self) {
        if self.project.has_video() {
            self.invalidate_all();
        } else {
            self.content_version += 1;
        }
    }

    pub fn undo(&mut self) -> Option<&'static str> {
        let r = self.history.undo(&mut self.project).map(|e| (e.label(), e.affected_frames()));
        self.after_history_step(r)
    }

    pub fn redo(&mut self) -> Option<&'static str> {
        let r = self.history.redo(&mut self.project).map(|e| (e.label(), e.affected_frames()));
        self.after_history_step(r)
    }

    fn after_history_step(&mut self, r: Option<(&'static str, Option<Vec<FrameId>>)>) -> Option<&'static str> {
        let (label, frames) = r?;
        self.audio_version += 1;
        // With a video layer, timing changes move the pictures of later frames.
        let frames = frames.filter(|_| !self.project.has_video());
        match frames {
            Some(frames) => {
                self.validate();
                self.redraw = Redraw::Full;
                self.content_version += 1;
                for f in frames {
                    self.invalidate_frame(f, None);
                }
            }
            None => self.invalidate_all(),
        }
        Some(label)
    }
}

pub struct Playback {
    pub playing: bool,
    pub looping: bool,
    /// Play forwards then backwards.
    pub ping_pong: bool,
    /// Only play frames `in..=out`.
    pub range: Option<(usize, usize)>,
    /// Frames follow the audio device's position (sound and picture in sync).
    audio_clock: bool,
    /// The audio clock reached the range at least once: when it later reads
    /// before the range, it has looped and is still sounding the end.
    audio_in_range: bool,
    start_time: f64,
    /// Ticks into the range where playback started.
    start_offset: u64,
    pub frame: usize,
}

impl Playback {
    fn new() -> Self {
        Self { playing: false, looping: true, ping_pong: false, range: None, audio_clock: false, audio_in_range: false, start_time: 0.0, start_offset: 0, frame: 0 }
    }

    /// The playable range, clamped to the current number of frames.
    pub fn bounds(&self, frames: usize) -> (usize, usize) {
        let last = frames.saturating_sub(1);
        match self.range {
            Some((a, b)) => (a.min(last), b.min(last).max(a.min(last))),
            None => (0, last),
        }
    }
}

pub struct Toast {
    pub text: String,
    pub until: f64,
}

/// Open dialogs and panel visibility.
pub struct UiState {
    pub show_side_panel: bool,
    pub new_project_dialog: Option<panels::dialogs::NewProject>,
    pub open_dialog: bool,
    pub settings_dialog: bool,
    /// Language picker; `Some(true)` the first time the app opens.
    pub language_dialog: Option<bool>,
    /// The home screen (the project gallery) is showing instead of the editor.
    pub home: bool,
    /// Options for generating in-betweens from the selection.
    pub tween_dialog: Option<panels::dialogs::TweenDialog>,
    /// Earlier versions of the project.
    pub versions_dialog: bool,
    /// The copies listed while the versions dialog is open (read once).
    pub versions_list: Option<Vec<crate::versions::Version>>,
    /// Go back to the text tool once the placed text is applied.
    pub text_return: bool,
    /// Camera panel and preview.
    pub camera: panels::camera::CameraUi,
    /// Writing text to place on the canvas.
    pub text_dialog: Option<panels::dialogs::TextDialog>,
    /// Text settings used last time (text, size, bold, centered).
    pub last_text: (String, Option<f32>, bool, bool),
    pub toast: Option<Toast>,
    pub side_tab: panels::SideTab,
    /// Panel open as a sheet in the phone layouts.
    pub sheet: Option<panels::SideTab>,
    /// The phone's bottom dock is pulled up: one track per layer.
    pub dock_open: bool,
    /// Menu of the drawing that starts at this frame (long press in the strip).
    pub drawing_menu: Option<usize>,
    /// Zoom and gestures of the per-layer timeline.
    pub tracks: panels::timeline::TracksState,
}

pub struct TrazoApp {
    pub doc: Document,
    pub tools: ToolState,
    pub onion: OnionSkin,
    pub canvas: CanvasView,
    pub playback: Playback,
    pub ui: UiState,
    pub thumbs: panels::timeline::ThumbCache,
    pub previews: panels::brushes::BrushPreviews,
    pub platform: Platform,
    /// Copied frame (for paste in the timeline).
    pub frame_clipboard: Option<trazo_core::Frame>,
    /// A drawing copied from the drawing menu, with its length.
    pub drawing_clipboard: Option<(trazo_core::raster::TiledImage, usize)>,
    pub export_ui: panels::export::ExportUi,
    pub video_import: panels::video_import::VideoImportUi,
    /// In-app file browser (Android import).
    pub file_browser: Option<crate::files::FileBrowser>,
    /// Sound device (None if there is no audio output).
    pub audio: Option<AudioEngine>,
    pub audio_ui: panels::audio::AudioUi,
    synced_audio: (u64, u64),
    last_scrub_frame: usize,
    /// Frames picked with Ctrl/Shift-click or long press (indices).
    pub frame_selection: std::collections::BTreeSet<usize>,
    saver: crate::save::SaveWorker,
    /// Projects on the home screen.
    pub home: panels::home::HomeState,
    /// Packs drawings away from the current frame (memory on big projects).
    compactor: crate::compactor::Compactor,
    last_autosave: f64,
}

const AUTOSAVE_SECS: f64 = 45.0;
const TOOLS_KEY: &str = "herramientas";
const EXPORT_KEY: &str = "exportar";

impl TrazoApp {
    pub fn new(cc: &eframe::CreationContext<'_>, platform: Platform) -> Self {
        let wide = cc.egui_ctx.content_rect().width() >= 900.0;
        // Language first: it names the starting project.
        let first_run = crate::i18n::load(&platform.data_dir).is_none();
        if first_run {
            let upgrading = !list_projects(&platform.projects_dir).is_empty();
            crate::i18n::set_lang(if upgrading { crate::i18n::Lang::Es } else { crate::i18n::system_guess() });
        }
        // The app opens on the home screen: no project is loaded until one is
        // chosen (this empty one is never saved unless it gets drawn on).
        let doc = blank_document(&platform.projects_dir);
        let has_audio = !doc.project.audio.tracks.is_empty();
        Self {
            doc,
            tools: {
                let mut t = ToolState::load(&platform.brushes_dir);
                if let Some(saved) = cc.storage.and_then(|s| eframe::get_value::<crate::tools::SavedTools>(s, TOOLS_KEY)) {
                    t.restore(saved);
                }
                t
            },
            onion: OnionSkin::default(),
            canvas: CanvasView::default(),
            playback: Playback::new(),
            ui: UiState {
                show_side_panel: wide && !platform.touch_first,
                new_project_dialog: None,
                open_dialog: false,
                settings_dialog: false,
                language_dialog: first_run.then_some(true),
                home: true,
                tween_dialog: None,
                versions_dialog: false,
                versions_list: None,
                text_return: false,
                camera: Default::default(),
                text_dialog: None,
                last_text: Default::default(),
                toast: None,
                side_tab: panels::SideTab::Layers,
                sheet: None,
                dock_open: false,
                drawing_menu: None,
                tracks: Default::default(),
            },
            thumbs: Default::default(),
            previews: Default::default(),
            frame_clipboard: None,
            drawing_clipboard: None,
            file_browser: None,
            video_import: Default::default(),
            export_ui: {
                let mut e = panels::export::ExportUi::default();
                if let Some(s) = cc.storage.and_then(|s| eframe::get_value::<trazo_core::export::ExportSettings>(s, EXPORT_KEY)) {
                    e.restore(s);
                }
                e
            },
            audio: match AudioEngine::new() {
                Ok(e) => Some(e),
                Err(e) => {
                    log::warn!("audio desactivado: {e}");
                    None
                }
            },
            audio_ui: panels::audio::AudioUi::with_panel(has_audio),
            synced_audio: (u64::MAX, u64::MAX),
            last_scrub_frame: usize::MAX,
            frame_selection: Default::default(),
            platform,
            saver: Default::default(),
            home: Default::default(),
            compactor: Default::default(),
            last_autosave: 0.0,
        }
    }

    pub fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        let now = ctx.input(|i| i.time);
        self.ui.toast = Some(Toast { text: text.into(), until: now + 2.5 });
    }

    // ---- project files -------------------------------------------------

    /// Save in the background (the project clone shares tile data).
    /// `announce` shows a confirmation when it finishes.
    pub fn save_async(&mut self, announce: bool) {
        if !announce && self.saver.is_busy() {
            return; // an autosave is already running
        }
        if announce {
            self.canvas.select.commit(&mut self.doc);
        }
        let d = &mut self.doc;
        d.settings_dirty = false;
        self.saver.save(d.project.clone(), d.path.clone(), d.frame, d.history.revision(), d.serial, announce);
    }

    pub fn save_blocking(&mut self) -> Result<(), String> {
        self.canvas.select.commit(&mut self.doc);
        let d = &mut self.doc;
        let rev = d.history.revision();
        let r = self.saver.save_and_wait(d.project.clone(), d.path.clone(), d.frame, rev, d.serial);
        if r.is_ok() {
            d.saved_revision = rev;
            d.settings_dirty = false;
        }
        r
    }

    fn poll_save(&mut self, ctx: &egui::Context) {
        for done in self.saver.poll() {
            if done.doc_serial != self.doc.serial {
                continue; // result for a project that is no longer open
            }
            match done.result {
                Ok(()) => {
                    self.doc.saved_revision = self.doc.saved_revision.max(done.revision);
                    if done.announce {
                        self.toast(ctx, tr("Guardado"));
                    }
                }
                Err(e) => {
                    self.doc.settings_dirty = true;
                    self.toast(ctx, tf!("No se pudo guardar: {}", tr_err(&e)));
                }
            }
        }
    }

    pub fn open_project(&mut self, ctx: &egui::Context, path: &Path) {
        if self.audio_ui.is_recording() {
            panels::audio::cancel_recording(self);
        }
        self.stop_playback();
        self.canvas.select.commit(&mut self.doc);
        if self.doc.is_modified() {
            if let Err(e) = self.save_blocking() {
                self.toast(ctx, tf!("No se pudo guardar el proyecto actual: {}", tr_err(&e)));
                return;
            }
        }
        match io::load_project(path) {
            Ok(p) => {
                self.doc = Document::new(p, path.to_path_buf());
                self.after_document_change();
                self.toast(ctx, tf!("Abierto: {}", self.doc.project.name));
            }
            Err(e) => self.toast(ctx, tf!("No se pudo abrir: {}", tr_err(&e.to_string()))),
        }
    }

    /// Draw the text from the dialog as a floating selection centred where
    /// the canvas was tapped, ready to move, scale or rotate.
    pub fn place_text(&mut self, ctx: &egui::Context, dlg: &panels::dialogs::TextDialog) {
        let fonts = egui::FontDefinitions::default();
        let Some(font) = fonts.font_data.get("Ubuntu-Light") else { return };
        let align = if dlg.center { trazo_core::text::Align::Center } else { trazo_core::text::Align::Left };
        let Some(img) = trazo_core::text::render((*font.font).as_ref(), &dlg.text, dlg.size, self.tools.color_rgba(), dlg.bold, align) else {
            self.toast(ctx, tr("No hay texto que insertar"));
            return;
        };
        let x = (dlg.at.0 - img.width as f32 / 2.0).round() as i32;
        let y = (dlg.at.1 - img.height as f32 / 2.0).round() as i32;
        let rect = trazo_core::IRect::new(x, y, x + img.width, y + img.height);
        self.canvas.select.place(&mut self.doc, rect, img.pixels);
        // The selection tool shows the handles to move it; once applied,
        // the text tool comes back for the next text.
        self.tools.tool = Tool::Select;
        self.ui.text_return = true;
        self.tools.push_recent();
    }

    /// Open a project picked on the home screen, at the frame it was left on.
    pub fn open_from_home(&mut self, ctx: &egui::Context, path: &Path, frame: usize) {
        self.open_project(ctx, path);
        if self.doc.path == path && path.exists() {
            self.home.leave();
            self.doc.set_frame(frame.min(self.doc.project.frames.len().saturating_sub(1)));
            self.ui.home = false;
            self.ui.sheet = None;
        }
    }

    /// Back to the home screen: save, then let go of the project (memory).
    pub fn go_home(&mut self, ctx: &egui::Context) {
        if self.audio_ui.is_recording() {
            panels::audio::cancel_recording(self);
        }
        self.stop_playback();
        self.canvas.select.commit(&mut self.doc);
        // Saved even when unchanged if it exists, so its picture and the
        // frame to come back to are current.
        if self.doc.is_modified() || self.doc.path.exists() {
            if let Err(e) = self.save_blocking() {
                self.toast(ctx, tf!("No se pudo guardar: {}", tr_err(&e)));
                // Unsaved drawing stays open; with nothing new to lose, go anyway.
                if self.doc.is_modified() {
                    return;
                }
            }
        }
        self.home.refresh();
        self.doc = blank_document(&self.platform.projects_dir);
        self.after_document_change();
        self.ui.home = true;
        self.ui.sheet = None;
        self.export_ui.open = false;
        self.home.refresh();
    }

    /// Go back to an earlier version. What is open now is kept as a version
    /// first, so restoring can itself be undone the same way.
    pub fn restore_version(&mut self, ctx: &egui::Context, version: &Path) {
        if self.audio_ui.is_recording() {
            panels::audio::cancel_recording(self);
        }
        self.stop_playback();
        if let Err(e) = self.save_blocking() {
            self.toast(ctx, tf!("No se pudo guardar el proyecto actual: {}", tr_err(&e)));
            return;
        }
        let path = self.doc.path.clone();
        // Read the chosen copy (and keep its bytes) before anything else: a
        // damaged copy changes nothing, and keeping the current state as a
        // version below may prune the oldest copies.
        let (mut project, bytes) = match io::load_project(version).map_err(|e| e.to_string()).and_then(|p| std::fs::read(version).map(|b| (p, b)).map_err(|e| e.to_string())) {
            Ok(v) => v,
            Err(e) => {
                self.toast(ctx, tf!("No se pudo abrir: {}", tr_err(&e)));
                return;
            }
        };
        if let Err(e) = crate::versions::snapshot(&path, true) {
            self.toast(ctx, tf!("No se pudo guardar el proyecto actual: {}", tr_err(&e.to_string())));
            return;
        }
        let staged = path.with_extension("version.tmp");
        if let Err(e) = std::fs::write(&staged, &bytes).and_then(|_| crate::versions::replace_file(&staged, &path)) {
            let _ = std::fs::remove_file(&staged);
            self.toast(ctx, tf!("No se pudo guardar: {}", tr_err(&e.to_string())));
            return;
        }
        let _ = std::fs::remove_file(&staged);
        // The project keeps its current name.
        project.name = self.doc.project.name.clone();
        self.doc = Document::new(project, path);
        self.after_document_change();
        self.toast(ctx, tr("Versión restaurada; la que tenías abierta quedó guardada como versión"));
    }

    pub fn new_project(&mut self, ctx: &egui::Context, name: String, canvas: CanvasSettings) {
        let p = Project::new(name, canvas);
        self.open_new_project(ctx, p);
    }

    /// Switch to a project that isn't saved yet (blank or from a template).
    pub fn open_new_project(&mut self, ctx: &egui::Context, project: Project) {
        if self.audio_ui.is_recording() {
            panels::audio::cancel_recording(self);
        }
        self.stop_playback();
        self.canvas.select.commit(&mut self.doc);
        if self.doc.is_modified() {
            if let Err(e) = self.save_blocking() {
                self.toast(ctx, tf!("No se pudo guardar el proyecto actual: {}", tr_err(&e)));
                return;
            }
        }
        let path = unique_project_path(&self.platform.projects_dir, &project.name);
        // Start on the drawing layer (the top one).
        let top = project.layers.last().map(|l| l.id);
        self.doc = Document::new(project, path);
        if let Some(l) = top {
            self.doc.layer = l;
        }
        // Save immediately so it shows up in the project list.
        self.doc.settings_dirty = true;
        self.after_document_change();
    }

    /// After the first-run language choice: the starting project, if it is
    /// still the untouched default, takes its name in that language.
    pub fn after_first_language(&mut self) {
        let untouched = !self.doc.path.exists() && !self.doc.history.can_undo() && self.doc.project.frames.len() == 1;
        if untouched {
            let name = tr("Mi animación").to_string();
            let path = unique_project_path(&self.platform.projects_dir, &name);
            self.doc = Document::new(Project::new(name, self.doc.project.canvas.clone()), path);
            self.after_document_change();
        }
    }

    fn after_document_change(&mut self) {
        self.frame_selection.clear();
        self.audio_ui.reset_for_new_project();
        self.last_scrub_frame = usize::MAX;
        if let Some(e) = &self.audio {
            e.stop();
        }
        if !self.doc.project.audio.tracks.is_empty() {
            self.audio_ui.show = true;
        }
        self.playback.range = None;
        self.canvas.reset_view();
        self.thumbs.clear();
        self.playback.playing = false;
        self.export_ui.reset_name();
    }

    // ---- playback -------------------------------------------------------

    pub fn toggle_play(&mut self, ctx: &egui::Context) {
        if self.playback.playing {
            self.stop_playback();
        } else {
            let p = &self.doc.project;
            let (a, b) = self.playback.bounds(p.frames.len());
            // Start from the current frame if it's inside the range (and not the last one).
            let start = if self.doc.frame >= a && self.doc.frame < b { self.doc.frame } else { a };
            let tick = p.tick_of_frame(start) as f64;
            self.play_from_tick(ctx, tick);
        }
    }

    /// Start playback at an exact tick (used by the recording pre-roll too).
    pub fn play_from_tick(&mut self, ctx: &egui::Context, tick: f64) {
        self.canvas.cancel_stroke(&mut self.doc);
        self.canvas.select.commit(&mut self.doc);
        let now = ctx.input(|i| i.time);
        let p = &self.doc.project;
        let (a, _) = self.playback.bounds(p.frames.len());
        let first = p.tick_of_frame(a) as f64;
        self.playback.start_offset = (tick - first).max(0.0) as u64;
        self.playback.start_time = now;
        self.playback.frame = p.frame_at_tick(tick.max(0.0) as u32);
        self.playback.playing = true;
        // Ping-pong plays backwards, which audio can't follow: use the plain clock.
        self.playback.audio_clock = false;
        self.playback.audio_in_range = false;
        if let Some(engine) = &self.audio {
            // Without sound there is nothing to stay in sync with: the plain
            // clock shows every frame (the audio clock lags by the device
            // latency, which used to skip the last frames of each loop).
            if !self.playback.ping_pong && !p.audio.tracks.is_empty() {
                // Loop inside the audio thread so the sound never overshoots the range.
                let (a, b) = self.playback.bounds(p.frames.len());
                let fps = p.canvas.fps;
                let ls = ticks_to_samples(p.tick_of_frame(a) as f64, fps);
                let le = ticks_to_samples((p.tick_of_frame(b) + p.frames[b].hold.max(1)) as f64, fps);
                engine.set_loop(self.playback.looping.then_some((ls, le)));
                engine.play_from(ticks_to_samples(tick, fps));
                self.playback.audio_clock = true;
            }
        }
    }

    pub fn stop_playback(&mut self) {
        if self.playback.playing {
            self.playback.playing = false;
            if let Some(engine) = &self.audio {
                engine.stop();
            }
            let last = self.doc.project.frames.len().saturating_sub(1);
            self.doc.set_frame(self.playback.frame.min(last));
            // Landing on a frame after playback is not a scrub.
            self.last_scrub_frame = self.doc.frame;
            self.doc.redraw = crate::app::Redraw::Full;
        }
    }

    fn advance_playback(&mut self, ctx: &egui::Context) {
        if !self.playback.playing {
            return;
        }
        let p = &self.doc.project;
        let fps = p.canvas.fps.max(1) as f64;
        let now = ctx.input(|i| i.time);
        let (a, b) = self.playback.bounds(p.frames.len());
        let first = p.tick_of_frame(a) as u64;
        let span = (p.tick_of_frame(b) as u64 + p.frames[b].hold.max(1) as u64 - first).max(1);
        if self.playback.audio_clock {
            if let Some(engine) = &self.audio {
                let fps_u = p.canvas.fps;
                // Small epsilon: fps that don't divide 48 kHz would otherwise land a frame early.
                let mut tick = samples_to_ticks(engine.position(), fps_u) + 1e-6;
                if tick >= first as f64 {
                    self.playback.audio_in_range = true;
                } else if self.playback.looping && self.playback.audio_in_range {
                    // The mixer already wrapped to the loop start, but what we
                    // hear (it lags by the output latency) is still the end.
                    tick += span as f64;
                }
                if !engine.is_playing() {
                    // The device stopped (unplugged headphones…): keep animating on the plain clock.
                    self.playback.audio_clock = false;
                    self.playback.start_time = now;
                    self.playback.start_offset = (tick - first as f64).max(0.0) as u64;
                } else {
                    if !self.playback.looping && tick >= (first + span) as f64 {
                        self.playback.frame = b;
                        self.stop_playback();
                        return;
                    }
                    let t = tick.clamp(first as f64, (first + span) as f64 - 1e-3);
                    self.playback.frame = p.frame_at_tick(t as u32);
                    ctx.request_repaint_after(std::time::Duration::from_millis(8));
                    return;
                }
            }
        }
        let elapsed_f = (now - self.playback.start_time) * fps;
        let elapsed = self.playback.start_offset + elapsed_f as u64;
        let (t, finished) = if self.playback.ping_pong && span > 1 {
            let period = 2 * span - 2;
            let u = elapsed % period;
            (if u < span { u } else { period - u }, elapsed >= period)
        } else {
            (elapsed % span, elapsed >= span)
        };
        if !self.playback.looping && finished {
            self.playback.frame = if self.playback.ping_pong { a } else { b };
            self.stop_playback();
            return;
        }
        self.playback.frame = p.frame_at_tick((first + t) as u32);
        let wait = (elapsed_f.floor() + 1.0 - elapsed_f) / fps;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.clamp(0.001, 1.0)));
    }

    /// Frame shown on the canvas right now (always a valid index).
    pub fn displayed_frame(&self) -> usize {
        let last = self.doc.project.frames.len().saturating_sub(1);
        if self.playback.playing {
            self.playback.frame.min(last)
        } else {
            self.doc.frame.min(last)
        }
    }

    // ---- keyboard ---------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        // Nothing reaches the drawing while the language is being chosen.
        if ctx.egui_wants_keyboard_input() || self.ui.home || self.ui.language_dialog.is_some() || self.ui.tween_dialog.is_some() || self.ui.text_dialog.is_some() || self.ui.versions_dialog || self.ui.camera.open {
            return;
        }
        use egui::{Key, KeyboardShortcut, Modifiers};
        let cmd = Modifiers::COMMAND;
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));

        if pressed(cmd | Modifiers::SHIFT, Key::Z) || pressed(cmd, Key::Y) {
            self.redo(ctx);
        }
        if pressed(cmd, Key::Z) {
            self.undo(ctx);
        }
        if pressed(cmd, Key::S) {
            self.save_async(true);
        }
        if pressed(cmd, Key::E) {
            self.export_ui.open = true;
        }
        let (ev_copy, ev_cut, ev_paste) = ctx.input(|i| {
            let has = |f: &dyn Fn(&egui::Event) -> bool| i.events.iter().any(f);
            (
                has(&|e| matches!(e, egui::Event::Copy)),
                has(&|e| matches!(e, egui::Event::Cut)),
                has(&|e| matches!(e, egui::Event::Paste(_))),
            )
        });
        if ev_copy || pressed(cmd, Key::C) {
            if self.canvas.select.copy() {
                ctx.copy_text("[Trazo] dibujo copiado".into());
            } else {
                self.toast(ctx, tr("Selecciona algo primero (S, L o W)"));
            }
        }
        if ev_cut || pressed(cmd, Key::X) {
            if self.canvas.select.is_active() {
                ctx.copy_text("[Trazo] dibujo copiado".into());
            }
            self.canvas.select.cut(&mut self.doc);
        }
        let ev_paste_image = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::PasteImage(_))));
        if ev_paste || ev_paste_image || pressed(cmd, Key::V) {
            self.paste_selection(ctx);
        }
        if pressed(cmd, Key::A) && !self.playback.playing {
            self.tools.tool = Tool::Select;
            if let Some(m) = self.canvas.select.select_all(&mut self.doc) {
                self.toast(ctx, m);
            }
        }
        let selecting = self.canvas.select.is_active();
        if selecting && ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
            self.canvas.select.delete(&mut self.doc);
        }
        if (selecting || self.canvas.select.is_busy()) && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.canvas.select.cancel(&mut self.doc);
        }
        if selecting {
            // Arrow keys nudge the selection (Shift = 10 px) instead of changing frame.
            let step = if ctx.input(|i| i.modifiers.shift) { 10.0 } else { 1.0 };
            let (l, r, u, d) = ctx.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowLeft) || i.consume_key(Modifiers::SHIFT, Key::ArrowLeft),
                    i.consume_key(Modifiers::NONE, Key::ArrowRight) || i.consume_key(Modifiers::SHIFT, Key::ArrowRight),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::SHIFT, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::SHIFT, Key::ArrowDown),
                )
            });
            let (dx, dy) = ((r as i32 - l as i32) as f32 * step, (d as i32 - u as i32) as f32 * step);
            if dx != 0.0 || dy != 0.0 {
                self.canvas.select.nudge(&mut self.doc, dx, dy);
            }
        }
        if selecting && ctx.input(|i| i.key_pressed(Key::Enter)) {
            self.canvas.select.commit(&mut self.doc);
            return;
        }
        let key = |k: Key| ctx.input(|i| i.key_pressed(k) && i.modifiers.is_none());
        if key(Key::B) {
            self.tools.tool = Tool::Brush;
        }
        if key(Key::E) {
            self.tools.tool = Tool::Eraser;
        }
        if key(Key::G) {
            self.tools.tool = Tool::Fill;
        }
        if key(Key::I) {
            self.tools.tool = Tool::Eyedropper;
        }
        if key(Key::H) {
            self.tools.tool = Tool::Hand;
        }
        if key(Key::S) {
            self.tools.tool = Tool::Select;
        }
        if key(Key::L) {
            self.tools.tool = Tool::Lasso;
        }
        if key(Key::U) {
            self.tools.tool = Tool::Shape;
        }
        if key(Key::T) {
            self.tools.tool = Tool::Text;
        }
        if key(Key::W) {
            self.tools.tool = Tool::Wand;
        }
        if key(Key::K) {
            panels::audio::add_marker(self);
        }
        if self.audio_ui.is_recording() && ctx.input(|i| i.key_pressed(Key::Escape)) {
            panels::audio::cancel_recording(self);
        }
        if ctx.input(|i| i.key_pressed(Key::R) && i.modifiers.shift_only()) {
            self.canvas.rotate_by(-15.0);
        } else if key(Key::R) {
            self.canvas.rotate_by(15.0);
        }
        if key(Key::M) {
            self.canvas.toggle_flip();
        }
        if key(Key::O) {
            self.onion.enabled = !self.onion.enabled;
            self.doc.redraw = Redraw::Full;
        }
        if key(Key::Enter) {
            self.toggle_play(ctx);
        }
        if key(Key::Comma) || key(Key::ArrowLeft) {
            self.stop_playback();
            self.doc.set_frame(self.doc.frame.saturating_sub(1));
        }
        if key(Key::Period) || key(Key::ArrowRight) {
            self.stop_playback();
            self.doc.set_frame(self.doc.frame + 1);
        }
        if key(Key::N) {
            panels::timeline::add_frame(self);
        }
        if key(Key::OpenBracket) {
            let t = self.tools.active_tweak_mut();
            t.size = (t.size / 1.15).max(1.0);
        }
        if key(Key::CloseBracket) {
            let t = self.tools.active_tweak_mut();
            t.size = (t.size * 1.15).min(500.0);
        }
    }

    pub fn undo(&mut self, ctx: &egui::Context) {
        if self.audio_ui.is_recording() || self.audio_ui.gesture_in_progress() {
            return;
        }
        self.stop_playback();
        self.canvas.cancel_stroke(&mut self.doc);
        if self.canvas.select.is_active() {
            // Undo while transforming = cancel the transform.
            self.canvas.select.cancel(&mut self.doc);
            self.toast(ctx, tr("Transformación cancelada"));
            return;
        }
        match self.doc.undo() {
            Some(l) => self.toast(ctx, tf!("Deshacer: {}", tr(l))),
            None => self.toast(ctx, tr("Nada que deshacer")),
        }
    }

    pub fn redo(&mut self, ctx: &egui::Context) {
        if self.audio_ui.is_recording() || self.audio_ui.gesture_in_progress() {
            return;
        }
        self.stop_playback();
        self.canvas.cancel_stroke(&mut self.doc);
        self.canvas.select.commit(&mut self.doc);
        match self.doc.redo() {
            Some(l) => self.toast(ctx, tf!("Rehacer: {}", tr(l))),
            None => self.toast(ctx, tr("Nada que rehacer")),
        }
    }

    /// Keep the audio thread's copy of the tracks current, scrub on frame
    /// changes and drive the voice-over recorder.
    fn sync_audio(&mut self, ctx: &egui::Context) {
        let key = (self.doc.serial, self.doc.audio_version);
        if let Some(engine) = &self.audio {
            if self.synced_audio != key {
                engine.set_timeline(self.doc.project.audio.clone());
                self.synced_audio = key;
            }
            // While recording without monitoring, keep the speakers silent.
            let silent = self.audio_ui.is_recording() && !self.audio_ui.monitor;
            engine.set_volume(if silent { 0.0 } else { self.audio_ui.volume });
            // Scrubbing: hear one frame of sound when stepping through frames.
            let f = self.doc.frame;
            if !self.playback.playing && f != self.last_scrub_frame {
                if self.audio_ui.scrub && self.last_scrub_frame != usize::MAX && !self.doc.project.audio.is_empty() {
                    let p = &self.doc.project;
                    let fps = p.canvas.fps;
                    let start = ticks_to_samples(p.tick_of_frame(f) as f64, fps);
                    let len = ticks_to_samples(p.frames[f].hold.max(1) as f64, fps);
                    engine.scrub(start, len);
                }
                self.last_scrub_frame = f;
            }
        }
        panels::audio::update_recording(self, ctx);
    }

    /// Paste copied pixels on the current layer/frame as a movable selection.
    pub fn paste_selection(&mut self, ctx: &egui::Context) {
        if !self.canvas.select.has_clipboard() {
            self.toast(ctx, tr("No hay nada copiado"));
            return;
        }
        self.stop_playback();
        let editable = self.doc.project.layer(self.doc.layer).is_some_and(|l| l.props.paintable());
        if !editable {
            self.toast(ctx, tr("La capa está bloqueada, oculta o es de video"));
            return;
        }
        self.tools.tool = Tool::Select;
        if let Some(m) = self.canvas.select.paste(&mut self.doc) {
            self.toast(ctx, m);
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let Some(t) = &self.ui.toast else { return };
        if now > t.until {
            self.ui.toast = None;
            return;
        }
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -150.0])
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(&t.text).size(15.0));
                });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

impl eframe::App for TrazoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // After a rotation the root area can keep the old (larger) size for a
        // while: lay everything out inside what is really on screen.
        let screen = ctx.content_rect();
        let mut root = ui.new_child(egui::UiBuilder::new().max_rect(screen));
        let ui = &mut root;
        // Test builds: 1 point = 1 pixel, so screenshots match the design's sizes.
        if cfg!(debug_assertions) && std::env::var_os("TRAZO_ESCALA1").is_some() && ctx.pixels_per_point() != 1.0 {
            ctx.set_pixels_per_point(1.0);
        }
        // Test builds: open a project straight away (screenshots of the editor).
        if cfg!(debug_assertions) && self.ui.home && !ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("debug_open")).unwrap_or(false)) {
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("debug_open"), true));
            if let Some(p) = std::env::var_os("TRAZO_ABRIR") {
                self.open_from_home(&ctx, std::path::Path::new(&p), 7);
            }
            match std::env::var("TRAZO_PANEL").as_deref() {
                Ok("pinceles") => self.ui.sheet = Some(panels::SideTab::Brushes),
                Ok("capas") => self.ui.sheet = Some(panels::SideTab::Layers),
                Ok("color") => self.ui.sheet = Some(panels::SideTab::Color),
                Ok("pistas") => self.ui.dock_open = true,
                Ok("menu") => self.ui.drawing_menu = Some(self.doc.frame),
                Ok("exportar") => self.export_ui.open = true,
                _ => {}
            }
        }
        self.poll_save(&ctx);
        self.shortcuts(&ctx);
        self.advance_playback(&ctx);

        // Autosave while idle.
        let now = ctx.input(|i| i.time);
        let idle = !self.canvas.is_stroking() && !self.playback.playing && !self.canvas.select.is_busy();
        let ghosts = if self.onion.enabled { (self.onion.before as usize, self.onion.after as usize) } else { (0, 0) };
        self.compactor.tick(now, &self.doc, idle, ghosts);
        if self.doc.is_modified() && !self.canvas.is_stroking() && now - self.last_autosave > AUTOSAVE_SECS {
            self.last_autosave = now;
            let keep = self.doc.history.audio_assets();
            let freed = self.doc.project.audio.collect_garbage(&keep);
            if freed > 0 {
                log::info!("audio liberado: {} MB", freed / (1024 * 1024));
            }
            self.save_async(false);
        }

        // A floating selection is applied as soon as you leave it behind.
        if self.canvas.select.is_active() && (!self.tools.tool.is_selection() || self.canvas.select.is_stale(&self.doc)) {
            self.canvas.select.commit(&mut self.doc);
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            if let Some(e) = self.tools.flush_user_brushes() {
                self.toast(&ctx, e);
            }
        }

        self.sync_audio(&ctx);

        // Android draws its status bar and gesture bar over the app: keep that
        // space free (egui only reports safe areas on iOS).
        if self.platform.touch_first {
            let bg = egui::Frame::new().fill(crate::theme::BG);
            egui::Panel::top("system_status_bar").exact_size(26.0).resizable(false).show_separator_line(false).frame(bg).show(ui, |_| {});
            egui::Panel::bottom("system_nav_bar").exact_size(22.0).resizable(false).show_separator_line(false).frame(bg).show(ui, |_| {});
        }

        if self.ui.home {
            // Android back: close what is open on the home screen.
            if ctx.input(|i| i.key_pressed(egui::Key::BrowserBack)) {
                self.home.back();
            }
            panels::home::show(self, ui);
            crate::files::show_browser(self, &ctx);
            panels::export::show(self, &ctx);
            panels::dialogs::show(self, &ctx);
            self.draw_toast(&ctx);
            return;
        }

        // Android back in the editor: close the open panel, else go home.
        if ctx.input(|i| i.key_pressed(egui::Key::BrowserBack)) {
            if self.ui.sheet.is_some() {
                self.ui.sheet = None;
            } else {
                self.go_home(&ctx);
                return;
            }
        }

        // One layout per screen shape (see the design canvas).
        let class = crate::widgets::SizeClass::of(&ctx);
        match class {
            crate::widgets::SizeClass::Phone => {
                panels::phone_up::top_bar(self, ui);
                // Sound has its own panel (from the dock's menu).
                if self.audio_ui.show {
                    panels::audio::show(self, ui);
                } else {
                    panels::phone_up::dock(self, ui);
                }
            }
            crate::widgets::SizeClass::PhoneLandscape => {
                panels::phone::left_rail(self, ui);
                panels::phone::right_rail(self, ui);
            }
            crate::widgets::SizeClass::Wide => {
                self.ui.sheet = None;
                panels::top_bar::show(self, ui);
                // Frames and audio are two tabs of the bottom panel.
                if self.audio_ui.show {
                    panels::audio::show(self, ui);
                } else {
                    panels::timeline::show(self, ui);
                }
                panels::toolbar::show(self, ui);
                if self.ui.show_side_panel {
                    panels::side_panel(self, ui);
                }
            }
        }
        let canvas_margin = if class == crate::widgets::SizeClass::Phone {
            if self.ui.dock_open { egui::Margin { left: 24, right: 24, top: 8, bottom: 24 } } else { panels::phone_up::CANVAS_MARGIN }
        } else {
            egui::Margin::ZERO
        };
        let canvas_rect = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(crate::theme::CANVAS_BG).inner_margin(canvas_margin))
            .show(ui, |ui| {
                let frame = self.displayed_frame();
                let onion = (!self.playback.playing).then_some(self.onion);
                self.canvas.camera_view = self.ui.camera.view_on();
                // On the phone the margins already leave room for the floating controls.
                self.canvas.fit_pad = if class == crate::widgets::SizeClass::Phone { 0.0 } else { 40.0 };
                self.canvas.fit_top = class == crate::widgets::SizeClass::Phone;
                self.canvas.show(ui, &mut self.doc, &mut self.tools, frame, onion, self.playback.playing);
                if let Some(g) = self.canvas.take_gesture() {
                    match g {
                        crate::canvas::Gesture::Undo => self.undo(&ctx),
                        crate::canvas::Gesture::Redo => self.redo(&ctx),
                    }
                }
                if let Some(msg) = self.canvas.take_message() {
                    self.toast(&ctx, msg);
                }
                if self.ui.text_return && !self.canvas.select.is_active() {
                    self.ui.text_return = false;
                    if self.tools.tool == Tool::Select {
                        self.tools.tool = Tool::Text;
                    }
                }
                if std::mem::take(&mut self.canvas.select.dropped_recording) {
                    self.toast(&ctx, tr("Grabación cancelada: cambiaste de fotograma, capa o herramienta"));
                }
                if let Some(done) = self.canvas.select.finish_recording(&mut self.doc) {
                    match done {
                        Ok((first, n)) => {
                            self.doc.frame = first.min(self.doc.project.frames.len().saturating_sub(1));
                            self.toast(&ctx, tf!("{} dibujos grabados: pulsa reproducir para verlos", n));
                        }
                        Err(e) if !e.is_empty() => self.toast(&ctx, e),
                        Err(_) => {}
                    }
                }
                if let Some(at) = self.canvas.take_text_request() {
                    let size = (self.doc.project.canvas.height as f32 / 10.0).round().clamp(16.0, 400.0);
                    let last = self.ui.last_text.clone();
                    self.ui.text_dialog = Some(panels::dialogs::TextDialog { at, size: last.1.unwrap_or(size), bold: last.2, center: last.3, text: String::new() });
                }
                if self.canvas.wants_stop_playback() {
                    self.stop_playback();
                }
            })
            .response
            .rect;

        // Floating controls and sheets of the phone layouts (hidden while a
        // dialog is open, so it is always in front).
        let modal = self.ui.new_project_dialog.is_some()
            || self.ui.open_dialog
            || self.ui.settings_dialog
            || self.ui.language_dialog.is_some()
            || self.ui.tween_dialog.is_some()
            || self.ui.versions_dialog
            || self.ui.camera.open
            || self.ui.text_dialog.is_some()
            || self.export_ui.open
            || self.video_import.is_busy()
            || self.file_browser.is_some();
        match if modal { crate::widgets::SizeClass::Wide } else { class } {
            crate::widgets::SizeClass::Phone => {
                if !self.ui.dock_open {
                    panels::phone_up::size_rail(self, &ctx, canvas_rect);
                    panels::phone_up::island(self, &ctx, canvas_rect);
                }
                panels::phone_up::drawing_menu(self, &ctx);
                panels::phone::panel_sheet(self, &ctx, crate::widgets::SheetEdge::Bottom);
            }
            crate::widgets::SizeClass::PhoneLandscape => {
                panels::phone::size_rail(self, &ctx, canvas_rect, true);
                panels::phone::zoom_pill(self, &ctx, canvas_rect);
                let inset = crate::widgets::insets(self.platform.touch_first).1;
                // The audio tab takes the place of the frame strip.
                if panels::audio::show_overlay(self, &ctx, inset, 64.0).is_none() {
                    panels::phone::floating_strip(self, &ctx, canvas_rect);
                }
                panels::phone::panel_sheet(self, &ctx, crate::widgets::SheetEdge::Right);
            }
            crate::widgets::SizeClass::Wide => {}
        }

        crate::files::show_browser(self, &ctx);
        panels::export::show(self, &ctx);
        panels::video_import::show(self, &ctx);
        panels::selection_bar::show(self, &ctx);
        panels::dialogs::show(self, &ctx);
        panels::camera::show(self, &ctx);
        self.thumbs.update(&ctx, &mut self.doc, self.canvas.is_stroking());
        self.draw_toast(&ctx);
    }

    /// Runs even while the window is hidden: keep the microphone ring drained.
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(e) = self.audio.as_mut() {
            e.drain_recording();
        }
    }

    /// eframe calls `save` every 30 s by default; that would block the UI.
    /// Our own background autosave handles that, so `save` only runs on exit.
    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(24 * 3600)
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, TOOLS_KEY, &self.tools.saved());
        eframe::set_value(storage, EXPORT_KEY, &self.export_ui.saved());
        let _ = self.tools.flush_user_brushes();
        if self.doc.is_modified() {
            if let Err(e) = self.save_blocking() {
                log::error!("autoguardado fallido: {e}");
            }
        }
    }
}

/// An empty, unsaved project (what the editor holds while the home screen shows).
fn blank_document(dir: &Path) -> Document {
    let name = tr("Mi animación").to_string();
    let path = unique_project_path(dir, &name);
    Document::new(Project::new(name, CanvasSettings::default()), path)
}

// ---- project folder helpers ----------------------------------------------

pub fn list_projects(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == FILE_EXTENSION))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}


pub fn unique_project_path(dir: &Path, name: &str) -> PathBuf {
    let slug: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let slug = if slug.trim_matches('_').is_empty() { "animacion".to_string() } else { slug };
    let mut path = dir.join(format!("{slug}.{FILE_EXTENSION}"));
    let mut n = 2;
    // A leftover folder of earlier versions would be taken by the new project.
    while path.exists() || crate::versions::dir_for(&path).exists() {
        path = dir.join(format!("{slug}_{n}.{FILE_EXTENSION}"));
        n += 1;
    }
    path
}
