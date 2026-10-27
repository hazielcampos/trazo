//! Home screen: "My animations" (see the design canvas, "Pantalla de inicio").
//!
//! Lists the projects with a picture of each (read from the file, without
//! opening the project), a "continue editing" card for the latest one, the
//! "new animation" button and, the first time, the examples. Each project has
//! a menu: open, rename, duplicate, export, earlier versions, delete.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::SystemTime;

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Ui, Vec2};
use trazo_core::io::ProjectInfo;
use trazo_core::templates::Template;

use crate::app::TrazoApp;
use crate::i18n::tr;
use crate::icons::{paint, Icon};
use crate::theme::{ACCENT, DANGER, ELEVATED, ON_ACCENT, SURFACE, TEXT, TEXT2, TEXT3};
use crate::widgets::SizeClass;

/// Picture made in the background: (project, (width, height, straight RGBA)).
type ThumbResult = (PathBuf, Option<(u32, u32, Vec<u8>)>);

/// One project on the home screen.
pub struct Card {
    pub path: PathBuf,
    modified: SystemTime,
    bytes: u64,
    /// None if the file couldn't be read.
    info: Option<ProjectInfo>,
    name: String,
    /// Lower-case name, for sorting and searching.
    key: String,
    texture: Option<TextureHandle>,
}

impl Card {
    fn texture(&mut self, ctx: &egui::Context) -> Option<&TextureHandle> {
        if self.texture.is_none() {
            // The pixels are only needed once: the texture keeps them.
            let (w, h, px) = self.info.as_mut()?.thumb.take()?;
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            self.texture = Some(ctx.load_texture(format!("home_{}", self.path.display()), img, egui::TextureOptions::LINEAR));
        }
        self.texture.as_ref()
    }

    fn openable(&self) -> bool {
        self.info.as_ref().is_some_and(|i| !i.newer)
    }

    fn background(&self) -> Color32 {
        let bg = self.info.as_ref().map(|i| i.background).unwrap_or(trazo_core::Rgba::WHITE).to_rgba8();
        Color32::from_rgb(bg[0], bg[1], bg[2])
    }
}

/// Work on a project file that takes a moment.
enum Job {
    Rename(PathBuf, String, usize),
    Duplicate(PathBuf, usize),
}

#[derive(Default)]
pub struct HomeState {
    cards: Vec<Card>,
    /// Re-read the folder on the next frame.
    stale: bool,
    loaded: bool,
    search: Option<String>,
    focus_search: bool,
    by_name: bool,
    /// Project whose menu is open.
    menu: Option<PathBuf>,
    /// Rename dialog: (project, text, focus the field).
    rename: Option<(PathBuf, String, bool)>,
    confirm_delete: Option<PathBuf>,
    running: Option<mpsc::Receiver<Result<String, String>>>,
    /// The project a running rename/duplicate works on (can't be opened meanwhile).
    running_path: Option<PathBuf>,
    /// Pictures made in the background for projects saved before they had one.
    thumbs: Option<mpsc::Receiver<ThumbResult>>,
    thumbs_cancel: Option<Arc<AtomicBool>>,
    /// Files whose picture couldn't be made (not tried again this session).
    no_thumb: HashSet<PathBuf>,
}

impl HomeState {
    /// The folder changed (saved, renamed, deleted…): read it again.
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    /// Leaving for the editor: stop background work and let go of the pictures.
    pub fn leave(&mut self) {
        if let Some(c) = self.thumbs_cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        self.thumbs = None;
        self.cards.clear();
        self.loaded = false;
        self.menu = None;
        self.rename = None;
        self.confirm_delete = None;
    }

    /// Close the topmost menu, dialog or search (Android back).
    pub fn back(&mut self) -> bool {
        self.confirm_delete.take().is_some() || self.rename.take().is_some() || self.menu.take().is_some() || self.search.take().is_some()
    }

    fn reload(&mut self, dir: &Path) {
        let mut old: HashMap<PathBuf, Card> = self.cards.drain(..).map(|c| (c.path.clone(), c)).collect();
        for (path, modified) in crate::app::list_projects(dir) {
            let card = match old.remove(&path) {
                Some(c) if c.modified == modified => c,
                _ => {
                    let info = trazo_core::io::read_info(&path).ok();
                    let name = match &info {
                        Some(i) => i.name.clone(),
                        None => path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                    };
                    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    Card { key: name.to_lowercase(), name, bytes, info, path, modified, texture: None }
                }
            };
            self.cards.push(card);
        }
        self.stale = false;
        self.loaded = true;
        self.make_missing_thumbs();
    }

    fn make_missing_thumbs(&mut self) {
        if self.thumbs.is_some() {
            return;
        }
        let missing: Vec<PathBuf> = self
            .cards
            .iter()
            .filter(|c| c.texture.is_none() && !self.no_thumb.contains(&c.path) && c.info.as_ref().is_some_and(|i| i.thumb.is_none() && !i.newer))
            .map(|c| c.path.clone())
            .collect();
        if missing.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let spawned = std::thread::Builder::new().name("miniaturas".into()).spawn(move || {
            for path in missing {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                // Only the first frame's drawings are read, one at a time.
                let thumb = trazo_core::io::render_first_frame(&path).ok();
                if tx.send((path, thumb)).is_err() {
                    return;
                }
            }
        });
        if spawned.is_ok() {
            self.thumbs = Some(rx);
            self.thumbs_cancel = Some(cancel);
        }
    }

    fn poll_thumbs(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.thumbs else { return };
        loop {
            match rx.try_recv() {
                Ok((path, Some(t))) => {
                    if let Some(c) = self.cards.iter_mut().find(|c| c.path == path) {
                        if let Some(i) = c.info.as_mut() {
                            i.thumb = Some(t);
                            c.texture = None;
                        }
                    }
                    ctx.request_repaint();
                }
                Ok((path, None)) => {
                    self.no_thumb.insert(path);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.thumbs = None;
                    self.thumbs_cancel = None;
                    break;
                }
            }
        }
    }

    fn is_running(&self, path: &Path) -> bool {
        self.running_path.as_deref() == Some(path)
    }
}

/// What a tap on the home screen asks for.
enum Action {
    Open(PathBuf, usize),
    New,
    Example(Template),
    Menu(PathBuf),
    Language,
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
    let margin = match class {
        SizeClass::Phone => egui::Margin::symmetric(16, 12),
        SizeClass::PhoneLandscape => egui::Margin::symmetric(28, 10),
        SizeClass::Wide => egui::Margin::symmetric(28, 18),
    };
    egui::CentralPanel::default().frame(egui::Frame::new().fill(crate::theme::BG).inner_margin(margin)).show(ui, |ui| {
        let h = &mut app.home;
        // Order: most recent first (as listed), or by name; filtered by the search.
        let mut order: Vec<usize> = (0..h.cards.len()).collect();
        if h.by_name {
            order.sort_by(|&a, &b| h.cards[a].key.cmp(&h.cards[b].key));
        }
        if let Some(q) = h.search.as_ref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
            order.retain(|&i| h.cards[i].key.contains(&q));
        }
        action = if h.cards.is_empty() {
            first_time(ui, class)
        } else {
            match class {
                SizeClass::Phone => phone(ui, h, &order),
                SizeClass::PhoneLandscape => landscape(ui, h, &order),
                SizeClass::Wide => wide(ui, h, &order),
            }
        };
    });
    match action {
        Some(Action::Open(path, frame)) => {
            if !app.home.is_running(&path) {
                app.open_from_home(&ctx, &path, frame);
            }
        }
        Some(Action::New) => app.ui.new_project_dialog = Some(Default::default()),
        Some(Action::Example(t)) => {
            app.home.leave();
            app.open_new_project(&ctx, t.build(tr(t.name())));
            app.save_async(true);
            app.ui.home = false;
        }
        Some(Action::Menu(path)) => app.home.menu = Some(path),
        Some(Action::Language) => {
            if cfg!(target_os = "android") {
                app.ui.language_dialog = Some(false);
            } else {
                app.ui.app_settings = true;
            }
        }
        None => {}
    }
    project_menu(app, &ctx, class);
    rename_dialog(app, &ctx);
    delete_dialog(app, &ctx);
}

// ---- layouts ------------------------------------------------------------------

/// Title row with search and language buttons; the search field under it.
fn header(ui: &mut Ui, h: &mut HomeState, title: &str, big: bool) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        logo(ui, if big { 30.0 } else { 26.0 });
        ui.label(RichText::new(title).size(if big { 26.0 } else { 22.0 }).strong().color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if round_icon(ui, if cfg!(target_os = "android") { Icon::Globe } else { Icon::Settings }, if cfg!(target_os = "android") { tr("Idioma · Language…") } else { tr("Ajustes de Trazo…") }).clicked() {
                action = Some(Action::Language);
            }
            search_button(ui, h);
        });
    });
    search_field(ui, h);
    action
}

fn search_button(ui: &mut Ui, h: &mut HomeState) {
    let open = h.search.is_some();
    let (icon, tip) = if open { (Icon::Close, tr("Cerrar la búsqueda")) } else { (Icon::Search, tr("Buscar")) };
    if round_icon(ui, icon, tip).clicked() {
        h.search = if open { None } else { Some(String::new()) };
        h.focus_search = !open;
    }
}

fn search_field(ui: &mut Ui, h: &mut HomeState) {
    if let Some(q) = &mut h.search {
        ui.add_space(6.0);
        let r = ui.add(egui::TextEdit::singleline(q).hint_text(tr("Buscar animaciones")).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 10.0)));
        // Focus once, when the search opens (the keyboard can be closed later).
        if std::mem::take(&mut h.focus_search) {
            r.request_focus();
        }
    }
}

fn phone(ui: &mut Ui, h: &mut HomeState, order: &[usize]) -> Option<Action> {
    use crate::theme::{font, W};
    let mut action = None;
    // Title, search and settings.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Mis animaciones")).font(font(26.0, W::Bold)).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::widgets::icon_button(ui, Icon::Sliders, tr("Ajustes e idioma")).clicked() {
                action = Some(Action::Language);
            }
            let open = h.search.is_some();
            if crate::widgets::icon_button(ui, if open { Icon::Close } else { Icon::Search }, if open { tr("Cerrar la búsqueda") } else { tr("Buscar") }).clicked() {
                h.search = if open { None } else { Some(String::new()) };
                h.focus_search = !open;
            }
        });
    });
    search_field(ui, h);
    ui.add_space(6.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if h.search.is_none() {
            if let Some(a) = continue_row(ui, h) {
                action = Some(a);
            }
            ui.add_space(20.0);
        }
        // How many, and the order.
        ui.horizontal(|ui| {
            ui.set_height(40.0);
            ui.add_space(4.0);
            ui.label(RichText::new(tf!("{} animaciones", order.len())).font(font(13.0, W::Regular)).color(TEXT2));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if h.by_name { tr("Por nombre") } else { tr("Más recientes") };
                let g = ui.painter().layout_no_wrap(label.to_string(), font(13.0, W::Medium), Color32::from_rgb(214, 214, 211));
                let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 38.0, 40.0), Sense::click());
                if resp.hovered() || resp.is_pointer_button_down_on() {
                    ui.painter().rect_filled(r, 12.0, ELEVATED);
                }
                ui.painter().galley(Pos2::new(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, TEXT);
                paint(ui.painter(), Rect::from_center_size(Pos2::new(r.right() - 17.0, r.center().y), Vec2::splat(14.0)), Icon::Down, TEXT2);
                egui::Popup::menu(&resp).show(|ui| {
                    ui.set_min_width(200.0);
                    for (by_name, l) in [(false, tr("Más recientes")), (true, tr("Por nombre"))] {
                        if ui.add(egui::Button::new(l).selected(h.by_name == by_name).min_size(Vec2::new(200.0, 40.0))).clicked() {
                            h.by_name = by_name;
                            ui.close();
                        }
                    }
                });
            });
        });
        ui.add_space(10.0);
        if let Some(a) = tall_grid(ui, h, order, 2) {
            action = Some(a);
        }
        ui.add_space(24.0);
    });
    action
}

/// "Seguir con «X»": a quiet row back into the last animation.
fn continue_row(ui: &mut Ui, h: &mut HomeState) -> Option<Action> {
    use crate::theme::{font, W};
    let latest = (0..h.cards.len()).filter(|&i| h.cards[i].openable()).max_by_key(|&i| h.cards[i].modified)?;
    let busy = h.is_running(&h.cards[latest].path);
    let ctx = ui.ctx().clone();
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 64.0), Sense::click());
    let p = ui.painter().clone();
    p.rect_filled(r, 16.0, if resp.hovered() || resp.is_pointer_button_down_on() { ELEVATED } else { SURFACE });
    let card = &mut h.cards[latest];
    let th = Rect::from_min_size(Pos2::new(r.left() + 8.0, r.center().y - 25.0), Vec2::new(40.0, 50.0));
    let bg = card.background();
    thumb(&p, th, card.texture(&ctx), bg);
    let x = th.right() + 12.0;
    let w = (r.right() - 56.0 - x).max(40.0);
    let frame = card.info.as_ref().map_or(0, |i| i.last_frame);
    let name = one_line_pub(ui, &tf!("Seguir con «{}»", card.name), font(15.0, W::Semi), TEXT, w);
    let sub = one_line_pub(ui, &tf!("Donde lo dejaste: dibujo {}", frame + 1), font(12.0, W::Regular), TEXT2, w);
    p.galley(Pos2::new(x, r.center().y - name.size().y), name, TEXT);
    p.galley(Pos2::new(x, r.center().y + 2.0), sub, TEXT2);
    let c = Pos2::new(r.right() - 28.0, r.center().y);
    p.circle_filled(c, 22.0, if busy { ELEVATED } else { crate::theme::PRIMARY });
    paint(&p, Rect::from_center_size(c, Vec2::splat(20.0)), Icon::ArrowRight, if busy { TEXT3 } else { ON_ACCENT });
    (resp.on_hover_text(tr("Abrir")).clicked() && !busy).then(|| Action::Open(card.path.clone(), frame))
}

/// Cards with tall pictures; the first one makes a new animation.
fn tall_grid(ui: &mut Ui, h: &mut HomeState, order: &[usize], cols: usize) -> Option<Action> {
    use crate::theme::{font, W};
    if order.is_empty() && h.search.is_some() {
        return grid(ui, h, order, cols);
    }
    let mut action = None;
    let ctx = ui.ctx().clone();
    let gap = 12.0;
    let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let th = (w * 1.22).round();
    let mut items: Vec<Option<usize>> = Vec::new();
    if h.search.is_none() {
        items.push(None);
    }
    items.extend(order.iter().map(|&i| Some(i)));
    for row in items.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for item in row {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, th + 46.0), Sense::click());
                if !ui.is_rect_visible(r) {
                    continue;
                }
                let p = ui.painter().clone();
                let pic = Rect::from_min_size(r.min, Vec2::new(w, th));
                let pressed = resp.hovered() || resp.is_pointer_button_down_on();
                let Some(i) = *item else {
                    p.rect_filled(pic, 16.0, if pressed { ELEVATED } else { SURFACE });
                    p.rect_stroke(pic, 16.0, Stroke::new(1.5, crate::theme::SEPARATOR), egui::StrokeKind::Inside);
                    let c = Pos2::new(pic.center().x, pic.center().y - 20.0);
                    p.circle_filled(c, 28.0, crate::theme::PRIMARY);
                    paint(&p, Rect::from_center_size(c, Vec2::splat(26.0)), Icon::Plus, ON_ACCENT);
                    let g = p.layout(tr("Eliges forma: vertical, cuadrada o ancha").to_string(), font(13.0, W::Regular), TEXT2, w - 32.0);
                    let gw = g.size().x;
                    p.galley(Pos2::new(pic.center().x - gw / 2.0, c.y + 44.0), g, TEXT2);
                    let name = one_line_pub(ui, tr("Nueva animación"), font(15.0, W::Semi), TEXT, w);
                    p.galley(Pos2::new(r.left(), pic.bottom() + 8.0), name, TEXT);
                    if resp.clicked() {
                        action = Some(Action::New);
                    }
                    continue;
                };
                let busy = h.is_running(&h.cards[i].path);
                let card = &mut h.cards[i];
                let bg = card.background();
                p.rect_filled(pic, 16.0, bg);
                if let Some(t) = card.texture(&ctx) {
                    let s = t.size_vec2();
                    let k = (pic.width() / s.x).max(pic.height() / s.y);
                    let ir = Rect::from_center_size(pic.center(), s * k);
                    p.with_clip_rect(pic).image(t.id(), ir, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                // Round the picture's corners over the page.
                p.with_clip_rect(pic).rect_stroke(pic.expand(8.0), 24.0, Stroke::new(16.0, crate::theme::BG), egui::StrokeKind::Middle);
                if pressed {
                    p.rect_stroke(pic, 16.0, Stroke::new(2.0, crate::theme::TEXT), egui::StrokeKind::Inside);
                }
                if let Some(info) = &card.info {
                    let secs = info.duration_secs().round() as u32;
                    let g = p.layout_no_wrap(format!("{}:{:02}", secs / 60, secs % 60), font(11.0, W::Semi), TEXT);
                    let br = Rect::from_min_size(pic.right_bottom() - Vec2::new(g.size().x + 14.0 + 8.0, 22.0 + 8.0), Vec2::new(g.size().x + 14.0, 22.0));
                    p.rect_filled(br, 7.0, Color32::from_rgba_unmultiplied(14, 14, 15, 204));
                    p.galley(Pos2::new(br.left() + 7.0, br.center().y - g.size().y / 2.0), g, TEXT);
                }
                p.galley(Pos2::new(r.left(), pic.bottom() + 8.0), one_line_pub(ui, &card.name, font(15.0, W::Semi), TEXT, w), TEXT);
                let (detail, color) = match &card.info {
                    _ if busy => (tr("Trabajando…").to_string(), TEXT2),
                    Some(i) if i.newer => (tr("Hecha con una versión más nueva de Trazo").to_string(), DANGER),
                    Some(_) => (crate::versions::age(card.modified), TEXT2),
                    None => (tr("No se puede leer").to_string(), DANGER),
                };
                p.galley(Pos2::new(r.left(), pic.bottom() + 28.0), one_line_pub(ui, &detail, font(12.0, W::Regular), color, w), color);
                let frame = card.info.as_ref().map_or(0, |i| i.last_frame);
                if resp.long_touched() || resp.secondary_clicked() {
                    action = Some(Action::Menu(card.path.clone()));
                } else if resp.clicked() && !busy {
                    action = Some(if card.openable() { Action::Open(card.path.clone(), frame) } else { Action::Menu(card.path.clone()) });
                }
            }
        });
        ui.add_space(18.0);
    }
    action
}

fn landscape(ui: &mut Ui, h: &mut HomeState, order: &[usize]) -> Option<Action> {
    let mut action = None;
    let full = ui.available_rect_before_wrap();
    let left = Rect::from_min_size(full.min, Vec2::new(250.0f32.min(full.width() * 0.34), full.height()));
    let right = Rect::from_min_max(Pos2::new(left.right() + 20.0, full.top()), full.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        egui::ScrollArea::vertical().id_salt("home_left").auto_shrink([false, false]).show(ui, |ui| {
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
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tf!("Todas · {}", order.len())).size(14.0).color(TEXT2));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if round_icon(ui, if cfg!(target_os = "android") { Icon::Globe } else { Icon::Settings }, if cfg!(target_os = "android") { tr("Idioma · Language…") } else { tr("Ajustes de Trazo…") }).clicked() {
                    action = Some(Action::Language);
                }
                search_button(ui, h);
                sort_toggle(ui, h);
            });
        });
        search_field(ui, h);
        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt("home_grid").auto_shrink([false, false]).show(ui, |ui| {
            if let Some(a) = grid(ui, h, order, 3) {
                action = Some(a);
            }
        });
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
        grid_header(ui, h, order.len());
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
            if round_icon(ui, if cfg!(target_os = "android") { Icon::Globe } else { Icon::Settings }, if cfg!(target_os = "android") { tr("Idioma · Language…") } else { tr("Ajustes de Trazo…") }).clicked() {
                action = Some(Action::Language);
            }
        });
    });
    let max_w = if class == SizeClass::Phone { ui.available_width() } else { 460.0 };
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
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let t = i as f32 / 20.0;
                    front.min + Vec2::new(10.0 + 52.0 * t, 40.0 - 22.0 * (t * std::f32::consts::PI * 1.4).sin().abs())
                })
                .collect();
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

/// One line of text cut with "…" to `width`.
/// One line cut with "…" in any font.
pub fn one_line_pub(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping { max_width: width.max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

fn one_line(ui: &Ui, text: &str, size: f32, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), FontId::proportional(size), color);
    job.wrap = TextWrapping { max_width: width.max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

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
    let galley = ui.painter().layout_no_wrap(tr("Nueva animación").to_string(), FontId::proportional(16.0), ON_ACCENT);
    let total = 20.0 + 8.0 + galley.size().x;
    let x0 = r.center().x - total / 2.0;
    paint(ui.painter(), Rect::from_min_size(Pos2::new(x0, r.center().y - 10.0), Vec2::splat(20.0)), Icon::Plus, ON_ACCENT);
    ui.painter().galley(Pos2::new(x0 + 28.0, r.center().y - galley.size().y / 2.0), galley, ON_ACCENT);
    resp
}

fn sort_toggle(ui: &mut Ui, h: &mut HomeState) {
    let label = if h.by_name { tr("Orden: nombre") } else { tr("Orden: recientes") };
    let b = egui::Button::new(RichText::new(label).size(13.0)).corner_radius(10).fill(SURFACE).min_size(Vec2::new(0.0, crate::theme::TOUCH));
    if ui.add(b).on_hover_text(tr("Cambiar el orden")).clicked() {
        h.by_name = !h.by_name;
    }
}

fn grid_header(ui: &mut Ui, h: &mut HomeState, count: usize) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(tf!("Todas · {}", count)).size(15.0).strong().color(TEXT2));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| sort_toggle(ui, h));
    });
}

/// "Continue editing": the most recently saved project.
fn continue_card(ui: &mut Ui, h: &mut HomeState, stacked: bool) -> Option<Action> {
    let latest = (0..h.cards.len()).filter(|&i| h.cards[i].openable()).max_by_key(|&i| h.cards[i].modified)?;
    let busy = h.is_running(&h.cards[latest].path);
    let ctx = ui.ctx().clone();
    let w = ui.available_width();
    let height = if stacked { 168.0 } else if w > 500.0 { 196.0 } else { 98.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, height), Sense::click());
    let p = ui.painter().clone();
    p.rect_filled(r, 18.0, if resp.hovered() || resp.is_pointer_button_down_on() { ELEVATED } else { SURFACE });
    let card = &mut h.cards[latest];
    let big = height > 120.0 && !stacked;
    let (thumb_rect, text_x, text_y) = if stacked {
        let t = Rect::from_min_size(r.min + Vec2::splat(10.0), Vec2::new(w - 20.0, height - 70.0));
        (t, r.left() + 12.0, t.bottom() + 8.0)
    } else {
        let th = height - 24.0;
        let t = Rect::from_min_size(r.min + Vec2::splat(12.0), Vec2::new(th * 16.0 / 9.0, th));
        (t, t.right() + 14.0, r.top() + if big { 30.0 } else { 14.0 })
    };
    let bg = card.background();
    thumb(&p, thumb_rect, card.texture(&ctx), bg);
    let text_w = (r.right() - text_x - 64.0).max(40.0);
    let detail = match &card.info {
        Some(i) => tf!("Fotograma {} · {}", i.last_frame + 1, crate::versions::age(card.modified)),
        None => crate::versions::age(card.modified),
    };
    p.text(Pos2::new(text_x, text_y), Align2::LEFT_TOP, tr("Seguir editando"), FontId::proportional(12.0), TEXT2);
    let name = one_line(ui, &card.name, if big { 22.0 } else { 16.0 }, TEXT, text_w);
    let name_h = name.size().y;
    p.galley(Pos2::new(text_x, text_y + 17.0), name, TEXT);
    p.galley(Pos2::new(text_x, text_y + 21.0 + name_h), one_line(ui, &detail, 12.0, TEXT2, text_w), TEXT2);
    if let (Some(i), true) = (&card.info, big) {
        let more = tf!("{} × {} · {} fps · {} fotogramas · {} s", i.width, i.height, i.fps, i.frames, crate::i18n::num(i.duration_secs(), 1));
        p.galley(Pos2::new(text_x, text_y + 40.0 + name_h), one_line(ui, &more, 12.0, TEXT2, text_w), TEXT2);
    }
    let arrow_c = Pos2::new(r.right() - 34.0, if stacked { r.bottom() - 30.0 } else { r.center().y });
    p.circle_filled(arrow_c, 22.0, ELEVATED);
    paint(&p, Rect::from_center_size(arrow_c, Vec2::splat(20.0)), Icon::Next, if busy { TEXT3 } else { TEXT });
    let frame = card.info.as_ref().map_or(0, |i| i.last_frame);
    (resp.on_hover_text(tr("Abrir")).clicked() && !busy).then(|| Action::Open(card.path.clone(), frame))
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
            p.with_clip_rect(r.shrink(1.0)).add(mesh);
        }
        None => {
            paint(p, Rect::from_center_size(r.center(), Vec2::splat(r.height().min(40.0))), Icon::Film, Color32::from_black_alpha(60));
        }
    }
}

fn grid(ui: &mut Ui, h: &mut HomeState, order: &[usize], cols: usize) -> Option<Action> {
    if order.is_empty() {
        let q = h.search.clone().unwrap_or_default();
        ui.label(RichText::new(tf!("Ninguna animación se llama «{}».", q.trim())).color(TEXT2));
        if ui.add(egui::Button::new(tr("Borrar la búsqueda")).min_size(Vec2::new(0.0, crate::theme::TOUCH))).clicked() {
            h.search = Some(String::new());
            h.focus_search = true;
        }
        return None;
    }
    let mut action = None;
    let ctx = ui.ctx().clone();
    let gap = 12.0;
    let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let th = (w * 0.58).round();
    for row in order.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &i in row {
                let busy = h.is_running(&h.cards[i].path);
                let card = &mut h.cards[i];
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, th + 48.0), Sense::click());
                if !ui.is_rect_visible(r) {
                    continue; // off screen: no texture made for it yet
                }
                let p = ui.painter().clone();
                let tr_ = Rect::from_min_size(r.min, Vec2::new(w, th));
                let bg = card.background();
                thumb(&p, tr_, card.texture(&ctx), bg);
                if resp.hovered() || resp.is_pointer_button_down_on() {
                    p.rect_stroke(tr_, 10.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
                }
                if let Some(info) = &card.info {
                    let secs = info.duration_secs().round() as u32;
                    let g = p.layout_no_wrap(format!("{}:{:02}", secs / 60, secs % 60), FontId::proportional(11.0), TEXT);
                    let br = Rect::from_min_size(tr_.right_bottom() - g.size() - Vec2::new(14.0, 10.0), g.size() + Vec2::new(8.0, 4.0));
                    p.rect_filled(br, 6.0, Color32::from_black_alpha(178));
                    p.galley(br.min + Vec2::new(4.0, 2.0), g, TEXT);
                }
                p.galley(Pos2::new(r.left(), tr_.bottom() + 6.0), one_line(ui, &card.name, 14.0, TEXT, w - 44.0), TEXT);
                let (detail, color) = match &card.info {
                    _ if busy => (tr("Trabajando…").to_string(), TEXT2),
                    Some(i) if i.newer => (tr("Hecha con una versión más nueva de Trazo").to_string(), DANGER),
                    Some(i) => (tf!("{} fotog. · {}", i.frames, crate::versions::age(card.modified)), TEXT2),
                    None => (tr("No se puede leer").to_string(), DANGER),
                };
                p.galley(Pos2::new(r.left(), tr_.bottom() + 26.0), one_line(ui, &detail, 12.0, color, w - 44.0), color);
                // "⋯": the project's menu (a long press or right click does the same).
                let more = Rect::from_min_size(Pos2::new(r.right() - 44.0, tr_.bottom() + 4.0), Vec2::splat(44.0));
                let mr = ui.interact(more, ui.scope_id().with(("home_more", i)), Sense::click());
                paint(&p, Rect::from_center_size(more.center(), Vec2::splat(20.0)), Icon::More, if mr.hovered() { TEXT } else { TEXT2 });
                let frame = card.info.as_ref().map_or(0, |i| i.last_frame);
                if mr.on_hover_text(tr("Opciones")).clicked() || resp.long_touched() || resp.secondary_clicked() {
                    action = Some(Action::Menu(card.path.clone()));
                } else if resp.clicked() && !busy {
                    action = Some(if card.openable() { Action::Open(card.path.clone(), frame) } else { Action::Menu(card.path.clone()) });
                }
            }
        });
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

#[derive(Clone, Copy, PartialEq)]
enum MenuItem {
    Open,
    Rename,
    Duplicate,
    Export,
    Versions,
    Delete,
}

/// The menu's rows (shared by the phone sheet and the dialog).
fn menu_items(ui: &mut Ui, can_open: bool, busy: bool) -> Option<MenuItem> {
    let mut chosen = None;
    let mut item = |ui: &mut Ui, icon: Icon, label: &str, color: Color32, enabled: bool, it: MenuItem| {
        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && (resp.hovered() || resp.is_pointer_button_down_on()) {
            ui.painter().rect_filled(r, 12.0, ELEVATED);
        }
        let c = if enabled { color } else { TEXT3 };
        paint(ui.painter(), Rect::from_min_size(Pos2::new(r.left() + 10.0, r.center().y - 11.0), Vec2::splat(22.0)), icon, c);
        ui.painter().text(Pos2::new(r.left() + 46.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(16.0), c);
        if resp.clicked() {
            chosen = Some(it);
        }
    };
    let ok = can_open && !busy;
    item(ui, Icon::Next, tr("Abrir"), TEXT, ok, MenuItem::Open);
    item(ui, Icon::Text, tr("Renombrar"), TEXT, ok, MenuItem::Rename);
    item(ui, Icon::Duplicate, tr("Duplicar"), TEXT, ok, MenuItem::Duplicate);
    item(ui, Icon::Export, tr("Exportar y compartir (abre la animación)"), TEXT, ok, MenuItem::Export);
    item(ui, Icon::Clock, tr("Versiones anteriores (abre la animación)"), TEXT, ok, MenuItem::Versions);
    ui.separator();
    item(ui, Icon::Trash, tr("Borrar…"), DANGER, !busy, MenuItem::Delete);
    chosen
}

fn project_menu(app: &mut TrazoApp, ctx: &egui::Context, class: SizeClass) {
    let Some(path) = app.home.menu.clone() else { return };
    let Some(card) = app.home.cards.iter().find(|c| c.path == path) else {
        app.home.menu = None;
        return;
    };
    let name = card.name.clone();
    let detail = card.info.as_ref().map(|i| {
        tf!("{} × {} · {} fps · {} fotogramas · {} s", i.width, i.height, i.fps, i.frames, crate::i18n::num(i.duration_secs(), 1))
    });
    let size = tf!("Editado {} · {} MB", crate::versions::age(card.modified), crate::i18n::num(card.bytes as f32 / 1e6, 1));
    let frame = card.info.as_ref().map_or(0, |i| i.last_frame);
    let can_open = card.openable();
    let busy = app.home.running.is_some();
    let mut chosen = None;
    let body = |ui: &mut Ui, chosen: &mut Option<MenuItem>| {
        if let Some(d) = &detail {
            ui.label(RichText::new(d).size(12.0).color(TEXT2));
        }
        ui.label(RichText::new(&size).size(12.0).color(TEXT2));
        ui.add_space(6.0);
        *chosen = menu_items(ui, can_open, busy);
    };
    let closed = if class == SizeClass::Phone {
        // Phone: a sheet from the bottom, in reach of the thumb.
        crate::widgets::sheet(
            ctx,
            "home_menu",
            crate::widgets::SheetEdge::Bottom,
            app.platform.touch_first,
            |ui| {
                ui.label(RichText::new(&name).size(17.0).strong());
            },
            |ui| body(ui, &mut chosen),
        )
    } else {
        let mut open = true;
        crate::widgets::backdrop(ctx);
        crate::widgets::dialog(ctx, &name, 400.0).id(egui::Id::unique("home_project_menu")).min_height(0.0).open(&mut open).show(ctx, |ui| body(ui, &mut chosen));
        !open
    };
    let Some(item) = chosen else {
        if closed {
            app.home.menu = None;
        }
        return;
    };
    app.home.menu = None;
    match item {
        MenuItem::Open => app.open_from_home(ctx, &path, frame),
        MenuItem::Rename => app.home.rename = Some((path, name, true)),
        MenuItem::Duplicate => start(app, ctx, Job::Duplicate(path, frame)),
        // Both open the animation first; only if it opened, so they never
        // act on the empty project behind the home screen.
        MenuItem::Export => {
            app.open_from_home(ctx, &path, frame);
            if !app.ui.home {
                app.export_ui.open = true;
            }
        }
        MenuItem::Versions => {
            app.open_from_home(ctx, &path, frame);
            if !app.ui.home {
                app.ui.versions_dialog = true;
            }
        }
        MenuItem::Delete => app.home.confirm_delete = Some(path),
    }
}

fn rename_dialog(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some((path, mut text, focus)) = app.home.rename.take() else { return };
    let original = app.home.cards.iter().find(|c| c.path == path).map(|c| c.name.clone()).unwrap_or_default();
    let mut open = true;
    let (mut go, mut cancel) = (false, false);
    let valid = !text.trim().is_empty() && text.trim() != original && app.home.running.is_none();
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Renombrar"), 380.0).id(egui::Id::unique("home_rename")).min_height(0.0).open(&mut open).show(ctx, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 10.0)));
        if focus {
            r.request_focus();
        }
        go |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 10.0) / 2.0;
            cancel = ui.add_sized([w, 46.0], egui::Button::new(RichText::new(tr("Cancelar")).strong()).corner_radius(12).fill(ELEVATED)).clicked();
            let save = egui::Button::new(RichText::new(tr("Guardar")).strong().color(ON_ACCENT)).corner_radius(12).fill(ACCENT);
            go |= ui.add_enabled_ui(valid, |ui| ui.add_sized([w, 46.0], save)).inner.clicked();
        });
    });
    // Re-check: Enter in the field also lands here.
    let valid = !text.trim().is_empty() && text.trim() != original && app.home.running.is_none();
    if go && valid {
        let frame = app.home.cards.iter().find(|c| c.path == path).and_then(|c| c.info.as_ref()).map_or(0, |i| i.last_frame);
        start(app, ctx, Job::Rename(path, text.trim().to_string(), frame));
    } else if open && !cancel {
        app.home.rename = Some((path, text, false));
    }
}

fn delete_dialog(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(path) = app.home.confirm_delete.clone() else { return };
    let name = app.home.cards.iter().find(|c| c.path == path).map(|c| c.name.clone()).unwrap_or_default();
    let mut open = true;
    let (mut yes, mut no) = (false, false);
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, &tf!("¿Borrar «{}»?", name), 380.0).id(egui::Id::unique("home_delete")).min_height(0.0).open(&mut open).show(ctx, |ui| {
        ui.label(RichText::new(tr("Se borran la animación y sus versiones anteriores. No se puede deshacer.")).color(TEXT2));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 10.0) / 2.0;
            no = ui.add_sized([w, 46.0], egui::Button::new(RichText::new(tr("Cancelar")).strong()).corner_radius(12).fill(ELEVATED)).clicked();
            yes = ui.add_sized([w, 46.0], egui::Button::new(RichText::new(tr("Borrar")).strong().color(Color32::WHITE)).corner_radius(12).fill(DANGER)).clicked();
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

/// Rename and duplicate rewrite project files: on a thread.
fn start(app: &mut TrazoApp, ctx: &egui::Context, job: Job) {
    if app.home.running.is_some() {
        app.toast(ctx, tr("Espera a que termine lo anterior"));
        return;
    }
    let dir = app.platform.projects_dir.clone();
    let (tx, rx) = mpsc::channel();
    let ctx2 = ctx.clone();
    let (msg, path) = match &job {
        Job::Rename(p, ..) => (tr("Renombrando…"), p.clone()),
        Job::Duplicate(p, _) => (tr("Duplicando…"), p.clone()),
    };
    app.toast(ctx, msg);
    let copy_name = tr("{} (copia)").to_string();
    let spawned = std::thread::Builder::new().name("inicio".into()).spawn(move || {
        let result = match job {
            Job::Rename(path, name, frame) => rename(&dir, &path, &name, frame),
            Job::Duplicate(path, frame) => duplicate(&dir, &path, &copy_name, frame),
        };
        let _ = tx.send(result);
        ctx2.request_repaint();
    });
    if spawned.is_ok() {
        app.home.running = Some(rx);
        app.home.running_path = Some(path);
    }
}

/// Rename: the file moves first (instant), then the name inside it changes;
/// if that fails the file moves back.
fn rename(dir: &Path, path: &Path, name: &str, frame: usize) -> Result<String, String> {
    let new_path = crate::app::unique_project_path(dir, name);
    std::fs::rename(path, &new_path).map_err(|e| e.to_string())?;
    trazo_core::io::forget_cels(path);
    let renamed = trazo_core::io::load_project(&new_path).map_err(|e| e.to_string()).and_then(|mut p| {
        p.name = name.to_string();
        trazo_core::io::save_project_at(&p, &new_path, frame).map_err(|e| e.to_string())
    });
    if let Err(e) = renamed {
        trazo_core::io::forget_cels(&new_path);
        let _ = std::fs::rename(&new_path, path);
        return Err(e);
    }
    // Earlier versions follow the project.
    let (old_v, new_v) = (crate::versions::dir_for(path), crate::versions::dir_for(&new_path));
    if old_v.is_dir() && !new_v.exists() {
        let _ = std::fs::rename(old_v, new_v);
    }
    Ok(tf!("Ahora se llama «{}»", name))
}

fn duplicate(dir: &Path, path: &Path, copy_name: &str, frame: usize) -> Result<String, String> {
    let mut p = trazo_core::io::load_project(path).map_err(|e| e.to_string())?;
    p.name = crate::i18n::fill(copy_name, &[&p.name]);
    let new_path = crate::app::unique_project_path(dir, &p.name);
    trazo_core::io::save_project_at(&p, &new_path, frame).map_err(|e| e.to_string())?;
    Ok(tf!("Copia creada: «{}»", p.name))
}

fn poll_job(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(rx) = &app.home.running else { return };
    let result = match rx.try_recv() {
        Ok(r) => Some(r),
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => None,
    };
    app.home.running = None;
    app.home.running_path = None;
    app.home.refresh();
    match result {
        Some(Ok(msg)) => app.toast(ctx, msg),
        Some(Err(e)) => app.toast(ctx, tf!("No se pudo completar: {}", crate::i18n::tr_err(&e))),
        None => {}
    }
}
