//! Home screen: "My animations" (see the design canvas, "Pantalla de inicio").
//!
//! Lists the projects with a picture of each (read from the file, without
//! opening the project), a "continue editing" card for the latest one, the
//! "new animation" button and, the first time, the examples. Each project has
//! a menu: open, rename, duplicate, export, earlier versions, delete.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Ui, Vec2};
use trazo_core::io::ProjectInfo;
use trazo_core::templates::Template;

use crate::app::TrazoApp;
use crate::i18n::tr;
use crate::icons::{paint, Icon};
use crate::theme::{ACCENT, DANGER, ELEVATED, ON_ACCENT, SURFACE, TEXT, TEXT2, TEXT3};
use crate::widgets::SizeClass;

/// One project on the home screen.
pub struct Card {
    pub path: PathBuf,
    modified: SystemTime,
    bytes: u64,
    /// None if the file couldn't be read.
    info: Option<ProjectInfo>,
    texture: Option<TextureHandle>,
}

impl Card {
    fn name(&self) -> String {
        match &self.info {
            Some(i) => i.name.clone(),
            None => self.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        }
    }

    fn texture(&mut self, ctx: &egui::Context) -> Option<&TextureHandle> {
        if self.texture.is_none() {
            let (w, h, px) = self.info.as_ref()?.thumb.as_ref()?;
            let img = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], px);
            self.texture = Some(ctx.load_texture(format!("home_{}", self.path.display()), img, egui::TextureOptions::LINEAR));
        }
        self.texture.as_ref()
    }
}

/// Work on a project file that takes a moment (it is loaded and saved again).
enum Job {
    Rename(PathBuf, String),
    Duplicate(PathBuf),
}

#[derive(Default)]
pub struct HomeState {
    cards: Vec<Card>,
    /// Re-read the folder on the next frame.
    stale: bool,
    loaded: bool,
    search: Option<String>,
    by_name: bool,
    /// Project whose menu is open.
    menu: Option<PathBuf>,
    rename: Option<(PathBuf, String)>,
    confirm_delete: Option<PathBuf>,
    running: Option<mpsc::Receiver<Result<String, String>>>,
    /// Pictures made in the background for projects saved before they had
    /// one (the files aren't touched).
    thumbs: Option<mpsc::Receiver<(PathBuf, (u32, u32, Vec<u8>))>>,
}

impl HomeState {
    /// The folder changed (saved, renamed, deleted…): read it again.
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    fn reload(&mut self, dir: &Path) {
        let mut old: HashMap<PathBuf, Card> = self.cards.drain(..).map(|c| (c.path.clone(), c)).collect();
        for (path, modified) in crate::app::list_projects(dir) {
            let card = match old.remove(&path) {
                Some(c) if c.modified == modified => c,
                _ => Card {
                    bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                    info: trazo_core::io::read_info(&path).ok(),
                    path,
                    modified,
                    texture: None,
                },
            };
            self.cards.push(card);
        }
        self.stale = false;
        self.loaded = true;
        self.make_missing_thumbs();
    }

    fn make_missing_thumbs(&mut self) {
        let missing: Vec<PathBuf> = self.cards.iter().filter(|c| c.info.as_ref().is_some_and(|i| i.thumb.is_none())).map(|c| c.path.clone()).collect();
        if missing.is_empty() || self.thumbs.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("miniaturas".into()).spawn(move || {
            for path in missing {
                if let Ok(p) = trazo_core::io::load_project(&path) {
                    if tx.send((path, trazo_core::io::thumbnail(&p))).is_err() {
                        return;
                    }
                }
            }
        });
        if spawned.is_ok() {
            self.thumbs = Some(rx);
        }
    }

    fn poll_thumbs(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.thumbs else { return };
        loop {
            match rx.try_recv() {
                Ok((path, thumb)) => {
                    if let Some(c) = self.cards.iter_mut().find(|c| c.path == path) {
                        if let Some(i) = c.info.as_mut() {
                            i.thumb = Some(thumb);
                            c.texture = None;
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.thumbs = None;
                    break;
                }
            }
        }
    }

    /// Something modal is open (keeps the editor shortcuts and the canvas quiet).
    pub fn is_busy(&self) -> bool {
        self.menu.is_some() || self.rename.is_some() || self.confirm_delete.is_some()
    }
}

/// What a tap on the home screen asks for.
enum Action {
    Open(PathBuf, usize),
    New,
    Example(Template),
    Menu(PathBuf),
}

pub fn show(app: &mut TrazoApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if !app.home.loaded || app.home.stale {
        let dir = app.platform.projects_dir.clone();
        app.home.reload(&dir);
    }
    poll_job(app, &ctx);
    app.home.poll_thumbs(&ctx);
    let class = SizeClass::of(&ctx);
    let mut action = None;
    egui::CentralPanel::default().frame(egui::Frame::new().fill(crate::theme::BG)).show(ui, |ui| {
        let h = &mut app.home;
        // Order: most recent first, or by name; filtered by the search.
        let mut order: Vec<usize> = (0..h.cards.len()).collect();
        if h.by_name {
            order.sort_by_key(|&i| h.cards[i].name().to_lowercase());
        }
        if let Some(q) = h.search.as_ref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
            order.retain(|&i| h.cards[i].name().to_lowercase().contains(&q));
        }
        if h.cards.is_empty() {
            action = first_time(ui, class);
            return;
        }
        match class {
            SizeClass::Phone => action = phone(ui, h, &order),
            SizeClass::PhoneLandscape => action = landscape(ui, h, &order),
            SizeClass::Wide => action = wide(ui, h, &order),
        }
    });
    // The language button lives in layouts that can't reach the app.
    if ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::unique("home_language"))).is_some() {
        app.ui.language_dialog = Some(false);
    }
    match action {
        Some(Action::Open(path, frame)) => app.open_from_home(&ctx, &path, frame),
        Some(Action::New) => app.ui.new_project_dialog = Some(Default::default()),
        Some(Action::Example(t)) => {
            app.open_new_project(&ctx, t.build(tr(t.name())));
            app.save_async(true);
            app.ui.home = false;
        }
        Some(Action::Menu(path)) => app.home.menu = Some(path),
        None => {}
    }
    project_menu(app, &ctx);
    rename_dialog(app, &ctx);
    delete_dialog(app, &ctx);
}

// ---- layouts ------------------------------------------------------------------

fn header(ui: &mut Ui, h: &mut HomeState, title: &str, big: bool) -> Option<Action> {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        logo(ui, if big { 30.0 } else { 26.0 });
        ui.label(RichText::new(title).size(if big { 26.0 } else { 20.0 }).strong().color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if round_icon(ui, Icon::Globe, tr("Idioma · Language…")).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("home_language"), true));
            }
            if round_icon(ui, Icon::Search, tr("Buscar")).clicked() {
                h.search = if h.search.is_some() { None } else { Some(String::new()) };
            }
        });
    });
    if let Some(q) = &mut h.search {
        ui.add_space(6.0);
        let r = ui.add(egui::TextEdit::singleline(q).hint_text(tr("Buscar animaciones")).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 10.0)));
        if r.gained_focus() || q.is_empty() {
            r.request_focus();
        }
    }
    None
}

fn phone(ui: &mut Ui, h: &mut HomeState, order: &[usize]) -> Option<Action> {
    let mut action = header(ui, h, tr("Mis animaciones"), true);
    ui.add_space(10.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 12.0;
        if h.search.is_none() {
            if let Some(a) = continue_card(ui, h, false) {
                action = Some(a);
            }
            if new_button(ui, 52.0).clicked() {
                action = Some(Action::New);
            }
        }
        if let Some(a) = grid_header(ui, h, order.len()) {
            action = Some(a);
        }
        if let Some(a) = grid(ui, h, order, 2) {
            action = Some(a);
        }
        ui.add_space(24.0);
    });
    action
}

fn landscape(ui: &mut Ui, h: &mut HomeState, order: &[usize]) -> Option<Action> {
    let mut action = None;
    let full = ui.available_rect_before_wrap();
    let left = Rect::from_min_size(full.min, Vec2::new(250.0f32.min(full.width() * 0.34), full.height()));
    let right = Rect::from_min_max(Pos2::new(left.right() + 16.0, full.top()), full.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        ui.horizontal(|ui| {
            logo(ui, 26.0);
            ui.label(RichText::new(tr("Mis animaciones")).size(20.0).strong().color(TEXT));
        });
        ui.add_space(10.0);
        if let Some(a) = continue_card(ui, h, true) {
            action = Some(a);
        }
        ui.add_space(10.0);
        if new_button(ui, 46.0).clicked() {
            action = Some(Action::New);
        }
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        let mut sub = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(tf!("Todas · {}", order.len())).size(14.0).color(TEXT2));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if round_icon(ui, Icon::Globe, tr("Idioma · Language…")).clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("home_language"), true));
                }
                if round_icon(ui, Icon::Search, tr("Buscar")).clicked() {
                    h.search = if h.search.is_some() { None } else { Some(String::new()) };
                }
                sort_toggle(ui, h);
            });
        });
        if let Some(q) = &mut h.search {
            let r = ui.add(egui::TextEdit::singleline(q).hint_text(tr("Buscar animaciones")).desired_width(f32::INFINITY));
            if q.is_empty() {
                r.request_focus();
            }
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            sub = grid(ui, h, order, 3);
        });
        if sub.is_some() {
            action = sub;
        }
    });
    action
}

fn wide(ui: &mut Ui, h: &mut HomeState, order: &[usize]) -> Option<Action> {
    let mut action = header(ui, h, "Trazo", false);
    ui.add_space(14.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 16.0;
        if h.search.is_none() {
            // Continue, new and examples in one row.
            let w = ui.available_width();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                ui.allocate_ui(Vec2::new((w - 16.0) * 0.58, 200.0), |ui| {
                    if let Some(a) = continue_card(ui, h, false) {
                        action = Some(a);
                    }
                });
                ui.allocate_ui(Vec2::new((w - 16.0) * 0.42, 200.0), |ui| {
                    ui.vertical(|ui| {
                        if new_button(ui, 56.0).clicked() {
                            action = Some(Action::New);
                        }
                        ui.add_space(10.0);
                        if let Some(a) = examples_row(ui) {
                            action = Some(a);
                        }
                    });
                });
            });
        }
        if let Some(a) = grid_header(ui, h, order.len()) {
            action = Some(a);
        }
        let cols = ((ui.available_width() / 230.0).floor() as usize).clamp(3, 6);
        if let Some(a) = grid(ui, h, order, cols) {
            action = Some(a);
        }
    });
    action
}

/// No projects yet: welcome, new, examples.
fn first_time(ui: &mut Ui, class: SizeClass) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        logo(ui, 30.0);
        ui.label(RichText::new("Trazo").size(26.0).strong().color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if round_icon(ui, Icon::Globe, tr("Idioma · Language…")).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("home_language"), true));
            }
        });
    });
    let max_w = if class == SizeClass::Wide { 460.0 } else { ui.available_width() };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.set_max_width(max_w);
            ui.add_space(24.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(132.0, 88.0), Sense::hover());
            let p = ui.painter();
            p.rect(Rect::from_min_size(r.min + Vec2::new(6.0, 10.0), Vec2::new(72.0, 54.0)), 8.0, SURFACE, Stroke::new(2.0, Color32::from_gray(58)), egui::StrokeKind::Inside);
            p.rect(Rect::from_min_size(r.min + Vec2::new(30.0, 22.0), Vec2::new(72.0, 54.0)), 8.0, ELEVATED, Stroke::new(2.0, Color32::from_gray(72)), egui::StrokeKind::Inside);
            let front = Rect::from_min_size(r.min + Vec2::new(54.0, 30.0), Vec2::new(72.0, 54.0));
            p.rect_filled(front, 8.0, Color32::WHITE);
            let pts: Vec<Pos2> = (0..=20).map(|i| {
                let t = i as f32 / 20.0;
                front.min + Vec2::new(10.0 + 52.0 * t, 40.0 - 22.0 * (t * std::f32::consts::PI * 1.4).sin().abs())
            }).collect();
            p.add(egui::Shape::line(pts, Stroke::new(3.0, ACCENT)));
            ui.add_space(8.0);
            ui.label(RichText::new(tr("Tu primera animación")).size(22.0).strong().color(TEXT));
            ui.label(RichText::new(tr("Dibuja un fotograma, añade el siguiente y dale a reproducir. Aquí aparecerán todas tus animaciones.")).size(15.0).color(TEXT2));
            ui.add_space(14.0);
            if new_button(ui, 52.0).clicked() {
                action = Some(Action::New);
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.label(RichText::new(tr("O aprende con un ejemplo")).size(15.0).color(TEXT2));
            });
            for t in Template::ALL {
                if example_row(ui, t).clicked() {
                    action = Some(Action::Example(t));
                }
            }
        });
    });
    action
}

// ---- pieces ---------------------------------------------------------------------

fn logo(ui: &mut Ui, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().rect_filled(r, size * 0.3, ACCENT);
    let pts: Vec<Pos2> = [(4.0, 17.0), (8.0, 14.5), (11.0, 8.5), (13.0, 8.0), (16.0, 11.0), (18.0, 14.0), (20.0, 12.0)]
        .iter()
        .map(|(x, y)| r.min + Vec2::new(x / 24.0 * size, y / 24.0 * size))
        .collect();
    ui.painter().add(egui::Shape::line(pts, Stroke::new(size * 0.1, ON_ACCENT)));
}

fn round_icon(ui: &mut Ui, icon: Icon, tip: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(crate::theme::TOUCH), Sense::click());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().circle_filled(r.center(), 22.0, ELEVATED);
    }
    paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(22.0)), icon, TEXT);
    resp.on_hover_text(tip)
}

fn new_button(ui: &mut Ui, height: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let fill = if resp.is_pointer_button_down_on() { ACCENT.gamma_multiply(0.8) } else { ACCENT };
    ui.painter().rect_filled(r, 14.0, fill);
    let label = tr("Nueva animación");
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(16.0), ON_ACCENT);
    let total = 20.0 + 8.0 + galley.size().x;
    let x0 = r.center().x - total / 2.0;
    paint(ui.painter(), Rect::from_min_size(Pos2::new(x0, r.center().y - 10.0), Vec2::splat(20.0)), Icon::Plus, ON_ACCENT);
    ui.painter().galley(Pos2::new(x0 + 28.0, r.center().y - galley.size().y / 2.0), galley, ON_ACCENT);
    resp
}

fn sort_toggle(ui: &mut Ui, h: &mut HomeState) {
    let label = if h.by_name { tr("Nombre") } else { tr("Recientes") };
    if ui.add(egui::Button::new(RichText::new(label).size(13.0)).corner_radius(10).fill(SURFACE)).on_hover_text(tr("Cambiar el orden")).clicked() {
        h.by_name = !h.by_name;
    }
}

fn grid_header(ui: &mut Ui, h: &mut HomeState, count: usize) -> Option<Action> {
    ui.horizontal(|ui| {
        ui.label(RichText::new(tf!("Todas · {}", count)).size(15.0).strong().color(TEXT2));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| sort_toggle(ui, h));
    });
    None
}

/// "Continue editing": the most recently saved project.
fn continue_card(ui: &mut Ui, h: &mut HomeState, stacked: bool) -> Option<Action> {
    let latest = (0..h.cards.len()).max_by_key(|&i| h.cards[i].modified)?;
    let ctx = ui.ctx().clone();
    let w = ui.available_width();
    let height = if stacked { 168.0 } else if w > 500.0 { 196.0 } else { 98.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, height), Sense::click());
    let p = ui.painter().clone();
    p.rect_filled(r, 18.0, if resp.hovered() { ELEVATED } else { SURFACE });
    let card = &mut h.cards[latest];
    let name = card.name();
    let detail = match &card.info {
        Some(i) => tf!("Fotograma {} · {}", i.last_frame + 1, crate::versions::age(card.modified)),
        None => crate::versions::age(card.modified),
    };
    let (thumb_rect, text_x, text_y) = if stacked {
        let t = Rect::from_min_size(r.min + Vec2::splat(10.0), Vec2::new(w - 20.0, height - 70.0));
        (t, r.left() + 12.0, t.bottom() + 8.0)
    } else {
        let th = height - 24.0;
        let t = Rect::from_min_size(r.min + Vec2::splat(12.0), Vec2::new(th * 16.0 / 9.0, th));
        (t, t.right() + 14.0, r.top() + if height > 120.0 { 30.0 } else { 16.0 })
    };
    let bg = card.info.as_ref().map(|i| i.background).unwrap_or(trazo_core::Rgba::WHITE).to_rgba8();
    thumb(&p, thumb_rect, card.texture(&ctx), Color32::from_rgb(bg[0], bg[1], bg[2]));
    p.text(Pos2::new(text_x, text_y), Align2::LEFT_TOP, tr("Seguir editando"), FontId::proportional(12.0), TEXT2);
    let name_w = r.right() - text_x - 60.0;
    let galley = p.layout(name, FontId::proportional(if height > 120.0 && !stacked { 22.0 } else { 16.0 }), TEXT, name_w.max(40.0));
    let name_h = galley.size().y;
    p.galley(Pos2::new(text_x, text_y + 17.0), galley, TEXT);
    p.text(Pos2::new(text_x, text_y + 22.0 + name_h), Align2::LEFT_TOP, detail, FontId::proportional(12.0), TEXT2);
    if let (Some(i), false) = (&card.info, stacked) {
        if height > 120.0 {
            p.text(
                Pos2::new(text_x, text_y + 40.0 + name_h),
                Align2::LEFT_TOP,
                tf!("{} × {} · {} fps · {} fotogramas · {} s", i.width, i.height, i.fps, i.frames, crate::i18n::num(i.duration_secs(), 1)),
                FontId::proportional(12.0),
                TEXT2,
            );
        }
    }
    let arrow = Rect::from_center_size(Pos2::new(r.right() - 34.0, if stacked { r.bottom() - 30.0 } else { r.center().y }), Vec2::splat(44.0));
    p.circle_filled(arrow.center(), 22.0, ELEVATED);
    paint(&p, Rect::from_center_size(arrow.center(), Vec2::splat(20.0)), Icon::Next, TEXT);
    let frame = card.info.as_ref().map_or(0, |i| i.last_frame);
    resp.on_hover_text(tr("Abrir")).clicked().then(|| Action::Open(card.path.clone(), frame))
}

/// Project picture; the canvas color with a soft mark when there is none.
fn thumb(p: &egui::Painter, r: Rect, tex: Option<&TextureHandle>, bg: Color32) {
    p.rect_filled(r, 10.0, bg);
    match tex {
        Some(t) => {
            // Fit the picture's aspect inside the box.
            let s = t.size_vec2();
            let k = (r.width() / s.x).min(r.height() / s.y);
            let ir = Rect::from_center_size(r.center(), s * k);
            let mut mesh = egui::Mesh::with_texture(t.id());
            mesh.add_rect_with_uv(ir, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            p.with_clip_rect(r).add(mesh);
        }
        None => {
            paint(p, Rect::from_center_size(r.center(), Vec2::splat(r.height().min(40.0))), Icon::Film, Color32::from_black_alpha(60));
        }
    }
}

fn grid(ui: &mut Ui, h: &mut HomeState, order: &[usize], cols: usize) -> Option<Action> {
    let mut action = None;
    let ctx = ui.ctx().clone();
    let gap = 12.0;
    let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let th = (w * 0.58).round();
    for row in order.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &i in row {
                let card = &mut h.cards[i];
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, th + 44.0), Sense::click());
                let p = ui.painter().clone();
                let tr_ = Rect::from_min_size(r.min, Vec2::new(w, th));
                let bg = card.info.as_ref().map(|i| i.background).unwrap_or(trazo_core::Rgba::WHITE).to_rgba8();
                thumb(&p, tr_, card.texture(&ctx), Color32::from_rgb(bg[0], bg[1], bg[2]));
                if resp.hovered() {
                    p.rect_stroke(tr_, 10.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
                }
                if let Some(info) = &card.info {
                    let secs = info.duration_secs().round() as u32;
                    let badge = format!("{}:{:02}", secs / 60, secs % 60);
                    let g = p.layout_no_wrap(badge, FontId::proportional(11.0), TEXT);
                    let br = Rect::from_min_size(tr_.right_bottom() - g.size() - Vec2::new(14.0, 10.0), g.size() + Vec2::new(8.0, 4.0));
                    p.rect_filled(br, 6.0, Color32::from_black_alpha(178));
                    p.galley(br.min + Vec2::new(4.0, 2.0), g, TEXT);
                }
                let name = p.layout(card.name(), FontId::proportional(14.0), TEXT, w - 36.0);
                p.galley(Pos2::new(r.left(), tr_.bottom() + 6.0), name, TEXT);
                let detail = match &card.info {
                    Some(i) => tf!("{} fotog. · {}", i.frames, crate::versions::age(card.modified)),
                    None => tr("No se puede leer").to_string(),
                };
                p.text(Pos2::new(r.left(), tr_.bottom() + 26.0), Align2::LEFT_TOP, detail, FontId::proportional(12.0), if card.info.is_some() { TEXT2 } else { DANGER });
                // "⋯": the project's menu (a long press does the same).
                let more = Rect::from_min_size(Pos2::new(r.right() - 32.0, tr_.bottom() + 6.0), Vec2::splat(32.0));
                let mr = ui.interact(more, ui.scope_id().with(("home_more", i)), Sense::click());
                paint(&p, Rect::from_center_size(more.center(), Vec2::splat(18.0)), Icon::More, if mr.hovered() { TEXT } else { TEXT2 });
                let frame = card.info.as_ref().map_or(0, |i| i.last_frame);
                if mr.on_hover_text(tr("Opciones")).clicked() || resp.long_touched() || resp.secondary_clicked() {
                    action = Some(Action::Menu(card.path.clone()));
                } else if resp.clicked() {
                    action = Some(Action::Open(card.path.clone(), frame));
                }
            }
        });
    }
    if order.is_empty() {
        ui.label(RichText::new(tr("Ninguna animación coincide con la búsqueda.")).color(TEXT2));
    }
    action
}

fn example_row(ui: &mut Ui, t: Template) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 76.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 16.0, if resp.hovered() { ELEVATED } else { SURFACE });
    let thumb = Rect::from_min_size(r.min + Vec2::splat(10.0), Vec2::new(96.0, 56.0));
    p.rect_filled(thumb, 10.0, Color32::WHITE);
    paint(p, Rect::from_center_size(thumb.center(), Vec2::splat(32.0)), Icon::Tween, Color32::from_rgb(70, 120, 235));
    p.text(Pos2::new(thumb.right() + 12.0, r.top() + 22.0), Align2::LEFT_TOP, tr(t.name()), FontId::proportional(15.0), TEXT);
    p.text(Pos2::new(thumb.right() + 12.0, r.top() + 42.0), Align2::LEFT_TOP, tr(t.hint()), FontId::proportional(13.0), TEXT2);
    resp
}

fn examples_row(ui: &mut Ui) -> Option<Action> {
    let mut action = None;
    egui::Frame::new().fill(SURFACE).corner_radius(16).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("Aprende con un ejemplo")).size(13.0).color(TEXT2));
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 16.0) / 3.0;
            for t in Template::ALL {
                if ui.add_sized([w, 64.0], egui::Button::new(RichText::new(tr(t.name())).size(13.0).strong()).corner_radius(12).fill(ELEVATED)).on_hover_text(tr(t.hint())).clicked() {
                    action = Some(Action::Example(t));
                }
            }
        });
    });
    action
}

// ---- menu and dialogs -------------------------------------------------------------

fn project_menu(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(path) = app.home.menu.clone() else { return };
    let Some(card) = app.home.cards.iter_mut().find(|c| c.path == path) else {
        app.home.menu = None;
        return;
    };
    let name = card.name();
    let detail = card.info.as_ref().map(|i| {
        tf!("{} × {} · {} fps · {} fotogramas · {} s", i.width, i.height, i.fps, i.frames, crate::i18n::num(i.duration_secs(), 1))
    });
    let size = tf!("Editado {} · {} MB", crate::versions::age(card.modified), crate::i18n::num(card.bytes as f32 / 1e6, 1));
    let frame = card.info.as_ref().map_or(0, |i| i.last_frame);
    let busy = app.home.running.is_some();
    let mut close = false;
    let mut chosen: Option<&str> = None;
    crate::widgets::backdrop(ctx);
    let mut open = true;
    crate::widgets::dialog(ctx, &name, 400.0).id(egui::Id::unique("home_project_menu")).min_height(0.0).open(&mut open).show(ctx, |ui| {
        if let Some(d) = &detail {
            ui.label(RichText::new(d).size(12.0).color(TEXT2));
        }
        ui.label(RichText::new(&size).size(12.0).color(TEXT2));
        ui.add_space(6.0);
        let item = |ui: &mut Ui, icon: Icon, label: &str, color: Color32, enabled: bool| -> bool {
            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), if enabled { Sense::click() } else { Sense::hover() });
            if resp.hovered() && enabled {
                ui.painter().rect_filled(r, 12.0, ELEVATED);
            }
            let c = if enabled { color } else { TEXT3 };
            paint(ui.painter(), Rect::from_min_size(Pos2::new(r.left() + 10.0, r.center().y - 11.0), Vec2::splat(22.0)), icon, c);
            ui.painter().text(Pos2::new(r.left() + 46.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(16.0), c);
            resp.clicked()
        };
        if item(ui, Icon::Play, tr("Abrir"), TEXT, true) {
            chosen = Some("open");
        }
        if item(ui, Icon::Text, tr("Renombrar"), TEXT, !busy) {
            chosen = Some("rename");
        }
        if item(ui, Icon::Duplicate, tr("Duplicar"), TEXT, !busy) {
            chosen = Some("duplicate");
        }
        if item(ui, Icon::Export, tr("Exportar y compartir"), TEXT, true) {
            chosen = Some("export");
        }
        if item(ui, Icon::Clock, tr("Versiones anteriores"), TEXT, true) {
            chosen = Some("versions");
        }
        ui.separator();
        if item(ui, Icon::Trash, tr("Borrar…"), DANGER, !busy) {
            chosen = Some("delete");
        }
    });
    if !open {
        close = true;
    }
    match chosen {
        Some("open") => {
            app.home.menu = None;
            app.open_from_home(ctx, &path, frame);
        }
        Some("rename") => {
            app.home.menu = None;
            app.home.rename = Some((path, name));
        }
        Some("duplicate") => {
            app.home.menu = None;
            start(app, ctx, Job::Duplicate(path));
        }
        Some("export") => {
            app.home.menu = None;
            app.open_from_home(ctx, &path, frame);
            app.export_ui.open = true;
        }
        Some("versions") => {
            app.home.menu = None;
            app.open_from_home(ctx, &path, frame);
            app.ui.versions_dialog = true;
        }
        Some("delete") => {
            app.home.menu = None;
            app.home.confirm_delete = Some(path);
        }
        _ if close => app.home.menu = None,
        _ => {}
    }
}

fn rename_dialog(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some((path, mut text)) = app.home.rename.take() else { return };
    let mut open = true;
    let mut go = false;
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Renombrar"), 380.0).id(egui::Id::unique("home_rename")).min_height(0.0).open(&mut open).show(ctx, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 10.0)));
        if !r.has_focus() && !r.lost_focus() {
            r.request_focus();
        }
        go |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(8.0);
        go |= crate::widgets::primary_button(ui, tr("Guardar")).clicked();
    });
    if go && !text.trim().is_empty() {
        start(app, ctx, Job::Rename(path, text.trim().to_string()));
    } else if open {
        app.home.rename = Some((path, text));
    }
}

fn delete_dialog(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(path) = app.home.confirm_delete.clone() else { return };
    let name = app.home.cards.iter().find(|c| c.path == path).map(|c| c.name()).unwrap_or_default();
    let mut open = true;
    let (mut yes, mut no) = (false, false);
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, &tf!("¿Borrar «{}»?", name), 380.0).id(egui::Id::unique("home_delete")).min_height(0.0).open(&mut open).show(ctx, |ui| {
        ui.label(RichText::new(tr("Se borran la animación y sus versiones anteriores. No se puede deshacer.")).color(TEXT2));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 10.0) / 2.0;
            no = ui.add_sized([w, 46.0], egui::Button::new(RichText::new(tr("Cancelar")).strong()).corner_radius(12).fill(ELEVATED)).clicked();
            yes = ui.add_sized([w, 46.0], egui::Button::new(RichText::new(tr("Borrar")).strong().color(Color32::WHITE)).corner_radius(12).fill(Color32::from_rgb(229, 72, 77))).clicked();
        });
    });
    if yes {
        app.home.confirm_delete = None;
        match std::fs::remove_file(&path) {
            Ok(()) => {
                let _ = std::fs::remove_dir_all(crate::versions::dir_for(&path));
                trazo_core::io::forget_cels(&path);
                app.toast(ctx, tf!("«{}» borrada", name));
            }
            Err(e) => app.toast(ctx, tf!("No se pudo borrar: {}", crate::i18n::tr_err(&e.to_string()))),
        }
        app.home.refresh();
    } else if no || !open {
        app.home.confirm_delete = None;
    }
}

/// Rename and duplicate load the project and save it again: on a thread.
fn start(app: &mut TrazoApp, ctx: &egui::Context, job: Job) {
    if app.home.running.is_some() {
        return;
    }
    let dir = app.platform.projects_dir.clone();
    let (tx, rx) = mpsc::channel();
    let ctx2 = ctx.clone();
    let msg = match &job {
        Job::Rename(..) => tr("Renombrando…"),
        Job::Duplicate(_) => tr("Duplicando…"),
    };
    app.toast(ctx, msg);
    let dup_suffix = tr("{} (copia)").to_string();
    let spawned = std::thread::Builder::new().name("inicio".into()).spawn(move || {
        let result = (|| -> Result<String, String> {
            match job {
                Job::Rename(path, name) => {
                    let mut p = trazo_core::io::load_project(&path).map_err(|e| e.to_string())?;
                    p.name = name.clone();
                    let new_path = crate::app::unique_project_path(&dir, &name);
                    trazo_core::io::save_project(&p, &new_path).map_err(|e| e.to_string())?;
                    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
                    trazo_core::io::forget_cels(&path);
                    // Earlier versions follow the project.
                    let (old_v, new_v) = (crate::versions::dir_for(&path), crate::versions::dir_for(&new_path));
                    if old_v.is_dir() {
                        let _ = std::fs::rename(old_v, new_v);
                    }
                    Ok(tf!("Ahora se llama «{}»", name))
                }
                Job::Duplicate(path) => {
                    let mut p = trazo_core::io::load_project(&path).map_err(|e| e.to_string())?;
                    p.name = crate::i18n::fill(&dup_suffix, &[&p.name]);
                    let new_path = crate::app::unique_project_path(&dir, &p.name);
                    trazo_core::io::save_project(&p, &new_path).map_err(|e| e.to_string())?;
                    Ok(tf!("Copia creada: «{}»", p.name))
                }
            }
        })();
        let _ = tx.send(result);
        ctx2.request_repaint();
    });
    if spawned.is_ok() {
        app.home.running = Some(rx);
    }
}

fn poll_job(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(rx) = &app.home.running else { return };
    match rx.try_recv() {
        Ok(result) => {
            app.home.running = None;
            app.home.refresh();
            match result {
                Ok(msg) => app.toast(ctx, msg),
                Err(e) => app.toast(ctx, tf!("No se pudo completar: {}", crate::i18n::tr_err(&e))),
            }
        }
        Err(mpsc::TryRecvError::Empty) => {}
        Err(mpsc::TryRecvError::Disconnected) => app.home.running = None,
    }
}
