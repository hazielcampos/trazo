//! Phone held upright (board "Teléfono · Editor" of the design canvas).
//!
//! Top: back, the animation's name and export. The canvas leaves room for
//! the size/opacity rail on the left (away from the system back gesture).
//! Above the dock floats the tool island: undo, redo, tools and color. The
//! dock below holds play, the counter, "Capas", "Cebolla" and the strip of
//! drawings of the current layer with "Nuevo" always in reach; pulled up it
//! shows one track per layer.

use egui::{Align2, Color32, Frame, Margin, Order, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use trazo_core::timeline as tl;

use crate::app::{Redraw, TrazoApp};
use crate::i18n::tr;
use crate::icons::{paint, Icon};
use crate::panels::SideTab;
use crate::theme::{self, ACCENT, CONTROL, ELEVATED, SEPARATOR, SURFACE, TEXT, TEXT2, TEXT3, W};
use crate::tools::Tool;
use crate::widgets;

/// Tools in the island; the rest live under "Más".
const MAIN_TOOLS: [Tool; 4] = [Tool::Brush, Tool::Eraser, Tool::Fill, Tool::Lasso];

/// Room the canvas leaves for the floating controls (left rail, island).
pub const CANVAS_MARGIN: Margin = Margin { left: 72, right: 24, top: 6, bottom: 84 };

// ---- top bar --------------------------------------------------------------------------

pub fn top_bar(app: &mut TrazoApp, ui: &mut Ui) {
    let top = crate::widgets::insets(app.platform.touch_first).0 as i8;
    egui::Panel::top("phone_up_top")
        .exact_size(56.0 + top as f32)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::BG).inner_margin(Margin { left: 12, right: 12, top: top + 6, bottom: 6 }))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let ctx = ui.ctx().clone();
                if widgets::icon_button(ui, Icon::ArrowLeft, tr("Inicio: mis animaciones")).clicked() {
                    app.go_home(&ctx);
                }
                let title = title(app, ui);
                egui::Popup::menu(&title).show(|ui| crate::panels::phone::project_menu(app, ui));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button_ex(ui, Icon::Export, tr("Exportar y compartir"), true, Some(ELEVATED)).clicked() {
                        app.export_ui.open = true;
                    }
                });
            });
        });
}

/// Name with a chevron; save state and speed underneath. Opens the menu.
fn title(app: &TrazoApp, ui: &mut Ui) -> egui::Response {
    let name = app.doc.project.name.clone();
    let name_g = ui.painter().layout_no_wrap(name, theme::font(17.0, W::Semi), TEXT);
    let state = if app.doc.is_modified() { tr("Sin guardar") } else { tr("Guardado") };
    let sub = tf!("{} · {} fps", state, app.doc.project.canvas.fps);
    let sub_g = ui.painter().layout_no_wrap(sub, theme::font(12.0, W::Regular), TEXT2);
    let max_w = (ui.available_width() - 60.0).max(80.0);
    let w = (name_g.size().x + 26.0).max(sub_g.size().x + 14.0).min(max_w) + 12.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, theme::TOUCH), Sense::click());
    let p = ui.painter_at(rect);
    if resp.hovered() || resp.is_pointer_button_down_on() {
        p.rect_filled(rect, theme::R_CONTROL, ELEVATED);
    }
    let x = rect.left() + 6.0;
    let nh = name_g.size().y;
    let nw = name_g.size().x;
    p.galley(Pos2::new(x, rect.top() + 2.0), name_g, TEXT);
    paint(&p, Rect::from_center_size(Pos2::new(x + nw + 11.0, rect.top() + 2.0 + nh / 2.0), Vec2::splat(15.0)), Icon::Down, TEXT2);
    let dot = if app.doc.is_modified() { TEXT3 } else { theme::SAVED };
    let sy = rect.bottom() - 10.0;
    p.circle_filled(Pos2::new(x + 3.0, sy), 3.0, dot);
    p.galley(Pos2::new(x + 11.0, sy - sub_g.size().y / 2.0), sub_g, TEXT2);
    resp.on_hover_text(tr("Menú de la animación"))
}

// ---- size and opacity rail ---------------------------------------------------------------

pub fn size_rail(app: &mut TrazoApp, ctx: &egui::Context, area: Rect) {
    let h = 196.0f32.min(area.height() - 24.0);
    if h < 120.0 {
        return;
    }
    let pos = Pos2::new(area.left() + 24.0, area.top() + (area.height() - CANVAS_MARGIN.bottom as f32 - h) / 2.0 + 10.0);
    let mut dragging_size = None;
    egui::Area::new(egui::Id::unique("phone_up_rail")).order(Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        Frame::new().fill(theme::floating()).corner_radius(18).shadow(theme::island_shadow()).inner_margin(Margin::symmetric(0, 10)).show(ui, |ui| {
            ui.set_width(36.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(36.0, 10.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, TEXT);
                let slider_h = (h - 20.0 - 10.0 - 10.0 - 13.0 - 16.0) / 2.0;
                let tw = app.tools.active_tweak_mut();
                let rs = widgets::vslider(ui, &mut tw.size, 1.0..=300.0, slider_h, true, tr("Tamaño del pincel"));
                if rs.dragged() {
                    dragging_size = Some((true, tw.size));
                }
                let (r, _) = ui.allocate_exact_size(Vec2::new(36.0, 5.0), Sense::hover());
                ui.painter().hline((r.center().x - 9.0)..=(r.center().x + 9.0), r.center().y, Stroke::new(1.0, SEPARATOR));
                let ro = widgets::vslider(ui, &mut tw.opacity, 0.02..=1.0, slider_h, false, tr("Opacidad"));
                if ro.dragged() {
                    dragging_size = Some((false, tw.opacity));
                }
                let (r, _) = ui.allocate_exact_size(Vec2::new(36.0, 12.0), Sense::hover());
                checker(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(10.0)));
            });
        });
    });
    // While dragging: the value in a bubble above the island, clear of the stroke.
    if let Some((is_size, v)) = dragging_size {
        let at = Pos2::new(area.center().x, area.bottom() - CANVAS_MARGIN.bottom as f32 - 8.0);
        egui::Area::new(egui::Id::unique("phone_up_bubble")).order(Order::Tooltip).pivot(Align2::CENTER_BOTTOM).fixed_pos(at).interactable(false).show(ctx, |ui| {
            Frame::new().fill(ELEVATED).corner_radius(14).shadow(theme::island_shadow()).inner_margin(Margin { left: 10, right: 14, top: 10, bottom: 10 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                    ui.painter().rect_filled(r, 8.0, theme::PAPER);
                    if is_size {
                        ui.painter().circle_filled(r.center(), (v / 4.0).clamp(1.5, 13.0), Color32::from_rgb(33, 28, 26));
                    } else {
                        ui.painter().circle_filled(r.center(), 9.0, Color32::from_rgba_unmultiplied(33, 28, 26, (v * 255.0) as u8));
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(egui::RichText::new(if is_size { tr("Tamaño") } else { tr("Opacidad") }).font(theme::font(11.0, W::Regular)).color(TEXT2));
                        let txt = if is_size { format!("{:.0} px", v) } else { format!("{:.0} %", v * 100.0) };
                        ui.label(egui::RichText::new(txt).font(theme::font(15.0, W::Semi)).color(TEXT));
                    });
                });
            });
        });
    }
}

fn checker(p: &egui::Painter, r: Rect) {
    p.circle_filled(r.center(), r.width() / 2.0, TEXT3);
    let h = r.width() / 2.0;
    for (dx, dy) in [(0.0, 0.0), (1.0, 1.0)] {
        let q = Rect::from_min_size(Pos2::new(r.left() + dx * h, r.top() + dy * h), Vec2::splat(h));
        p.with_clip_rect(Rect::from_center_size(r.center(), Vec2::splat(r.width() * 0.72))).rect_filled(q, 0.0, TEXT);
    }
}

// ---- tool island -------------------------------------------------------------------------

pub fn island(app: &mut TrazoApp, ctx: &egui::Context, area: Rect) {
    let w = (area.width() - 20.0).min(420.0);
    egui::Area::new(egui::Id::unique("phone_up_island"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(Pos2::new(area.center().x, area.bottom() - 14.0))
        .show(ctx, |ui| {
            Frame::new().fill(SURFACE).corner_radius(theme::R_ISLAND).shadow(theme::island_shadow()).stroke(Stroke::new(1.0, Color32::from_white_alpha(12))).inner_margin(Margin::symmetric(6, 7)).show(ui, |ui| {
                ui.set_width(w - 12.0);
                crate::panels::phone::tool_options_pub(app, ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    let ctx = ui.ctx().clone();
                    let can_undo = app.doc.history.can_undo() || app.canvas.select.is_active();
                    if widgets::icon_button_ex(ui, Icon::Undo, tr("Deshacer (toca con 2 dedos)"), can_undo, None).clicked() {
                        app.undo(&ctx);
                    }
                    if widgets::icon_button_ex(ui, Icon::Redo, tr("Rehacer (toca con 3 dedos)"), app.doc.history.can_redo(), None).clicked() {
                        app.redo(&ctx);
                    }
                    let (sep, _) = ui.allocate_exact_size(Vec2::new(5.0, 28.0), Sense::hover());
                    ui.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, SEPARATOR));
                    let tool_w = ((ui.available_width() - 46.0) / 5.0).clamp(40.0, 46.0);
                    for t in MAIN_TOOLS {
                        tool_slot(app, ui, t, Vec2::new(tool_w, 46.0));
                    }
                    crate::panels::phone::more_tools_pub(app, ui, Vec2::new(tool_w, 46.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| color_slot(app, ui));
                });
            });
        });
}

/// A tool; tapping the active brush or eraser again opens the brushes. The
/// small chevron says so.
fn tool_slot(app: &mut TrazoApp, ui: &mut Ui, t: Tool, size: Vec2) {
    let selected = app.tools.tool == t;
    let resp = widgets::tool_button(ui, t.icon(), selected, size, t.label());
    if selected && matches!(t, Tool::Brush | Tool::Eraser) {
        let r = resp.rect;
        paint(ui.painter(), Rect::from_center_size(Pos2::new(r.right() - 8.0, r.top() + 8.0), Vec2::splat(10.0)), Icon::Up, theme::ON_ACCENT);
    }
    if resp.clicked() {
        if selected && matches!(t, Tool::Brush | Tool::Eraser) {
            app.ui.sheet = Some(SideTab::Brushes);
        }
        app.tools.tool = t;
    }
}

fn color_slot(app: &mut TrazoApp, ui: &mut Ui) {
    let open = app.ui.sheet == Some(SideTab::Color);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(44.0, 46.0), Sense::click());
    let p = ui.painter();
    if open {
        p.rect_filled(rect, 14.0, SEPARATOR);
    }
    let ring_bg = if open { SEPARATOR } else { SURFACE };
    p.circle_filled(rect.center(), 17.0, TEXT);
    p.circle_filled(rect.center(), 15.0, ring_bg);
    p.circle_filled(rect.center(), 13.0, app.tools.color);
    if resp.on_hover_text(tr("Color")).clicked() {
        app.ui.sheet = if open { None } else { Some(SideTab::Color) };
    }
}

// ---- dock ------------------------------------------------------------------------------

pub fn dock(app: &mut TrazoApp, ui: &mut Ui) {
    let bottom = crate::widgets::insets(app.platform.touch_first).1 as i8;
    egui::Panel::bottom("phone_up_dock")
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(theme::SHEET)
                .corner_radius(egui::CornerRadius { nw: theme::R_SHEET, ne: theme::R_SHEET, sw: 0, se: 0 })
                .shadow(egui::Shadow { offset: [0, -8], blur: 30, spread: 0, color: Color32::from_black_alpha(115) })
                .inner_margin(Margin { left: 12, right: 12, top: 0, bottom: bottom + 10 }),
        )
        .show(ui, |ui| {
            crate::panels::timeline::commit_selection_on_click(app, ui);
            grabber(app, ui);
            header(app, ui);
            ui.add_space(8.0);
            if app.ui.dock_open {
                crate::panels::timeline::tracks(app, ui, true, 5.0);
                crate::panels::timeline::block_actions(app, ui);
            } else {
                strip(app, ui);
            }
        });
}

/// Tap or drag up/down: show or hide the tracks.
fn grabber(app: &mut TrazoApp, ui: &mut Ui) {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::click_and_drag());
    ui.painter().rect_filled(Rect::from_center_size(r.center(), Vec2::new(36.0, 5.0)), 3.0, CONTROL);
    let delta = ui.input(|i| i.pointer.delta().y);
    if resp.clicked() || (resp.dragged() && delta < -6.0 && !app.ui.dock_open) || (resp.dragged() && delta > 6.0 && app.ui.dock_open) {
        app.ui.dock_open = !app.ui.dock_open;
    }
    resp.on_hover_text(if app.ui.dock_open { tr("Bajar: ver solo los dibujos") } else { tr("Subir: una pista por capa y el sonido") });
}

fn header(app: &mut TrazoApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        play_button(app, ui);
        counter(app, ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if app.ui.dock_open {
                let more = widgets::icon_button(ui, Icon::More, tr("Velocidad, sonido y papel cebolla"));
                egui::Popup::menu(&more).show(|ui| crate::panels::phone::frame_menu(app, ui));
                if widgets::toggle_icon(ui, Icon::Loop, app.playback.looping, tr("Bucle")).clicked() {
                    app.playback.looping = !app.playback.looping;
                }
                if widgets::toggle_icon(ui, Icon::Onion, app.onion.enabled, tr("Papel cebolla")).clicked() {
                    toggle_onion(app);
                }
            } else {
                let on = app.onion.enabled;
                let r = widgets::pill_button(ui, Icon::Onion, tr("Cebolla"), if on { ACCENT } else { TEXT2 }, on).on_hover_text(tr("Papel cebolla (mantén pulsado para ajustarlo)"));
                if r.clicked() {
                    toggle_onion(app);
                }
                egui::Popup::context_menu(&r).show(|ui| crate::panels::phone::frame_menu(app, ui));
                let name = app.doc.project.layer(app.doc.layer).map(|l| crate::i18n::layer_name(&l.props.name).into_owned()).unwrap_or_default();
                let open = app.ui.sheet == Some(SideTab::Layers);
                if widgets::pill_button(ui, Icon::Layers, &name, if open { ACCENT } else { TEXT }, false).on_hover_text(tr("Capas")).clicked() {
                    app.ui.sheet = if open { None } else { Some(SideTab::Layers) };
                }
            }
        });
    });
}

fn toggle_onion(app: &mut TrazoApp) {
    app.onion.enabled = !app.onion.enabled;
    app.doc.redraw = Redraw::Full;
}

fn play_button(app: &mut TrazoApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() { theme::PRIMARY.gamma_multiply(0.85) } else { theme::PRIMARY };
    ui.painter().circle_filled(rect.center(), 22.0, fill);
    let ic = if app.playback.playing { Icon::Pause } else { Icon::Play };
    paint(ui.painter(), Rect::from_center_size(rect.center() + Vec2::new(if app.playback.playing { 0.0 } else { 1.0 }, 0.0), Vec2::splat(22.0)), ic, theme::ON_ACCENT);
    if resp.on_hover_text(tr("Reproducir / pausar")).clicked() {
        app.toggle_play(&ctx);
    }
}

/// "08 / 24": the frame in the counter face, the total quieter.
fn counter(app: &TrazoApp, ui: &mut Ui) {
    let n = app.doc.project.frames.len();
    let cur = app.displayed_frame() + 1;
    let mut job = egui::text::LayoutJob::default();
    job.append(&format!("{cur:02}"), 4.0, egui::TextFormat { font_id: theme::counter_font(20.0), color: TEXT, ..Default::default() });
    job.append(&format!(" / {n}"), 0.0, egui::TextFormat { font_id: theme::font(13.0, W::Regular), color: TEXT2, valign: egui::Align::Max, ..Default::default() });
    if app.ui.dock_open {
        let fps = app.doc.project.canvas.fps.max(1) as f32;
        job.append(&format!("\n{} s", crate::i18n::num((cur - 1) as f32 / fps, 2)), 4.0, egui::TextFormat { font_id: theme::font(12.0, W::Regular), color: TEXT2, ..Default::default() });
    }
    ui.label(job);
}

// ---- strip of drawings -----------------------------------------------------------------

const CELL: Vec2 = Vec2::new(52.0, 65.0);
const HOLD_STEP: f32 = 58.0;

fn strip(app: &mut TrazoApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let layer = app.doc.layer;
    let Some(l) = app.doc.project.layer(layer).cloned() else { return };
    let height = CELL.y + 22.0;
    if l.props.shared || l.props.video.is_some() {
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        let msg = if l.props.shared { tr("Capa de fondo fijo: se ve en toda la toma") } else { tr("Capa de video: ajústala en Capas") };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, msg, theme::font(13.0, W::Regular), TEXT2);
        return;
    }
    let blocks = tl::blocks(&app.doc.project, layer);
    let shown = app.displayed_frame();
    let current = blocks.iter().position(|b| b.start <= shown && shown < b.end());
    let new_w = CELL.x + 12.0;
    let full = ui.available_width();
    let mut goto = None;
    let mut add = false;
    let mut menu_for: Option<usize> = None;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        egui::ScrollArea::horizontal().id_salt("phone_strip").max_width(full - new_w).auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (i, b) in blocks.iter().enumerate() {
                    let w = CELL.x + (b.len.saturating_sub(1)) as f32 * HOLD_STEP;
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, height), Sense::click());
                    let is_cur = Some(i) == current;
                    // Keep the current drawing in view when it changes.
                    let last_id = egui::Id::unique("phone_strip_last");
                    if is_cur && (app.playback.playing || ui.data(|d| d.get_temp::<usize>(last_id)) != Some(b.start)) {
                        ui.scroll_to_rect(r.expand2(Vec2::new(60.0, 0.0)), Some(egui::Align::Center));
                        ui.ctx().request_repaint();
                        if ui.clip_rect().contains_rect(r) {
                            ui.data_mut(|d| d.insert_temp(last_id, b.start));
                        }
                    }
                    if !ui.is_rect_visible(r) {
                        continue;
                    }
                    let thumb = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), CELL);
                    let cell = Rect::from_min_size(thumb.min, Vec2::new(w, CELL.y));
                    let p = ui.painter();
                    if b.len > 1 {
                        // The rest of the cell: stripes and how long it lasts.
                        p.rect_filled(cell, 8.0, Color32::from_rgb(42, 42, 46));
                        let clip = p.with_clip_rect(Rect::from_min_max(Pos2::new(thumb.right(), cell.top()), cell.max));
                        let mut x = thumb.right();
                        while x < cell.right() {
                            clip.rect_filled(Rect::from_min_size(Pos2::new(x + 6.0, cell.top()), Vec2::new(6.0, cell.height())), 0.0, Color32::from_rgb(50, 50, 54));
                            x += 12.0;
                        }
                        let label = tf!("Dura {}", b.len);
                        let g = p.layout_no_wrap(label, theme::font(12.0, W::Semi), Color32::from_rgb(226, 226, 223));
                        let mid = Pos2::new((thumb.right() + cell.right()) / 2.0, cell.center().y);
                        let pill = Rect::from_center_size(mid, g.size() + Vec2::new(12.0, 6.0));
                        p.rect_filled(pill, 6.0, theme::SHEET);
                        p.galley(pill.min + Vec2::new(6.0, 3.0), g, TEXT);
                    }
                    frame_thumb(app, &ctx, p, thumb, b.start);
                    if is_cur {
                        p.rect_stroke(thumb.expand(2.0), 10.0, Stroke::new(2.0, theme::SHEET), egui::StrokeKind::Inside);
                        p.rect_stroke(thumb.expand(4.0), 12.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
                    }
                    let num = format!("{}", b.start + 1);
                    p.text(Pos2::new(thumb.center().x, r.bottom() - 7.0), Align2::CENTER_CENTER, num, theme::font(11.0, if is_cur { W::Bold } else { W::Regular }), if is_cur { ACCENT } else { TEXT2 });
                    if resp.clicked() {
                        goto = Some(b.start);
                    }
                    if resp.long_touched() || resp.secondary_clicked() {
                        goto = Some(b.start);
                        menu_for = Some(b.start);
                    }
                }
            });
        });
        // "Nuevo": always in reach, after the strip.
        let (r, resp) = ui.allocate_exact_size(Vec2::new(new_w, height), Sense::click());
        let cell = Rect::from_min_size(Pos2::new(r.right() - CELL.x, r.top() + 4.0), CELL);
        let p = ui.painter();
        p.rect_filled(cell, 10.0, if resp.hovered() { SEPARATOR } else { ELEVATED });
        dashed_rect(p, cell, Stroke::new(1.5, TEXT3));
        paint(p, Rect::from_center_size(cell.center(), Vec2::splat(24.0)), Icon::Plus, TEXT);
        p.text(Pos2::new(cell.center().x, r.bottom() - 7.0), Align2::CENTER_CENTER, tr("Nuevo"), theme::font(11.0, W::Semi), Color32::from_rgb(214, 214, 211));
        if resp.on_hover_text(tr("Nuevo dibujo después del actual (N)")).clicked() {
            add = true;
        }
    });
    if let Some(f) = goto {
        app.stop_playback();
        app.canvas.select.commit(&mut app.doc);
        app.doc.set_frame(f);
    }
    if add {
        crate::panels::timeline::add_frame(app);
    }
    if let Some(start) = menu_for {
        app.ui.drawing_menu = Some(start);
    }
}

pub fn dashed_rect(p: &egui::Painter, r: Rect, st: Stroke) {
    let rr = 10.0;
    let mut pts = Vec::new();
    let corner = |pts: &mut Vec<Pos2>, c: Pos2, a0: f32| {
        for i in 0..=5 {
            let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / 5.0;
            pts.push(c + Vec2::new(a.cos(), a.sin()) * rr);
        }
    };
    let r = r.shrink(0.75);
    corner(&mut pts, Pos2::new(r.right() - rr, r.top() + rr), -std::f32::consts::FRAC_PI_2);
    corner(&mut pts, Pos2::new(r.right() - rr, r.bottom() - rr), 0.0);
    corner(&mut pts, Pos2::new(r.left() + rr, r.bottom() - rr), std::f32::consts::FRAC_PI_2);
    corner(&mut pts, Pos2::new(r.left() + rr, r.top() + rr), std::f32::consts::PI);
    if let Some(f) = pts.first().copied() {
        pts.push(f);
    }
    p.extend(Shape::dashed_line(&pts, st, 4.0, 3.0));
}

/// The whole frame, small: every visible layer's drawing on the paper.
pub fn frame_thumb(app: &mut TrazoApp, ctx: &egui::Context, p: &egui::Painter, rect: Rect, frame: usize) {
    let bg = app.doc.project.canvas.background.to_rgba8();
    p.rect_filled(rect, 8.0, Color32::from_rgb(bg[0], bg[1], bg[2]));
    let layers: Vec<_> = app.doc.project.layers.iter().filter(|l| l.props.visible && !l.props.guide && l.props.video.is_none()).map(|l| (l.id, l.props.opacity, l.props.shared)).collect();
    let clip = p.with_clip_rect(rect.shrink(0.5).intersect(p.clip_rect()));
    for (id, opacity, shared) in layers {
        let tex = if shared {
            shared_thumb(app, ctx, id)
        } else {
            let Some(b) = tl::block_at(&app.doc.project, id, frame) else { continue };
            app.thumbs.get(ctx, &app.doc, b.start, id)
        };
        if let Some(t) = tex {
            let s = t.size_vec2();
            let k = (rect.width() / s.x).min(rect.height() / s.y);
            let ir = Rect::from_center_size(rect.center(), s * k);
            clip.image(t.id(), ir, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE.gamma_multiply(opacity));
        }
    }
}

/// Thumbnail of a background layer (one drawing for the whole shot).
fn shared_thumb(app: &TrazoApp, ctx: &egui::Context, layer: trazo_core::LayerId) -> Option<egui::TextureHandle> {
    let cel = app.doc.project.shared_cels.get(&layer)?;
    let fp = cel.fingerprint();
    let id = egui::Id::unique(("shared_thumb", layer));
    if let Some((have, t)) = ctx.data(|d| d.get_temp::<(u64, egui::TextureHandle)>(id)) {
        if have == fp {
            return Some(t);
        }
    }
    let (w, h, data) = trazo_core::composite::cel_thumbnail_alpha(cel, 96);
    let t = ctx.load_texture(format!("shared{layer}"), egui::ColorImage::from_rgba_premultiplied([w, h], &data), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, (fp, t.clone())));
    Some(t)
}
