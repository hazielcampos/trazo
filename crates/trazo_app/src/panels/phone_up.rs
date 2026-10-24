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
pub const CANVAS_MARGIN: Margin = Margin { left: 72, right: 24, top: 16, bottom: 84 };

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
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(36.0, 8.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, TEXT);
                let slider_h = (h - 20.0 - 8.0 - 5.0 - 10.0 - 24.0) / 2.0;
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
                let (r, _) = ui.allocate_exact_size(Vec2::new(36.0, 10.0), Sense::hover());
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
    let clip = p.with_clip_rect(r);
    let path: Vec<Pos2> = (0..32).map(|i| r.center() + Vec2::angled(i as f32 / 32.0 * std::f32::consts::TAU) * (r.width() / 2.0)).collect();
    clip.add(Shape::convex_polygon(path, TEXT3, Stroke::NONE));
    let q = r.width() / 4.0;
    for y in 0..4 {
        for x in 0..4 {
            if (x + y) % 2 == 0 {
                let c = Rect::from_min_size(Pos2::new(r.left() + x as f32 * q, r.top() + y as f32 * q), Vec2::splat(q));
                if c.center().distance(r.center()) < r.width() / 2.0 {
                    clip.rect_filled(c, 0.0, TEXT);
                }
            }
        }
    }
}

// ---- tool island -------------------------------------------------------------------------

pub fn island(app: &mut TrazoApp, ctx: &egui::Context, area: Rect) {
    let w = (area.width() - 20.0).min(420.0);
    egui::Area::new(egui::Id::unique("phone_up_island"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(Pos2::new(area.center().x, area.bottom() - 16.0))
        .show(ctx, |ui| {
            Frame::new().fill(SURFACE).corner_radius(theme::R_ISLAND).shadow(theme::island_shadow()).stroke(Stroke::new(1.0, Color32::from_white_alpha(12))).inner_margin(Margin::symmetric(6, 7)).show(ui, |ui| {
                ui.set_width(w - 12.0);
                crate::panels::phone::tool_options_pub(app, ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    let ctx = ui.ctx().clone();
                    let can_undo = app.doc.history.can_undo() || app.canvas.select.is_active();
                    if narrow_button(ui, Icon::Undo, tr("Deshacer (toca con 2 dedos)"), can_undo).clicked() {
                        app.undo(&ctx);
                    }
                    if narrow_button(ui, Icon::Redo, tr("Rehacer (toca con 3 dedos)"), app.doc.history.can_redo()).clicked() {
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
    if selected && (matches!(t, Tool::Brush | Tool::Eraser) || crate::panels::phone::has_options(t)) {
        let r = resp.rect;
        paint(ui.painter(), Rect::from_center_size(Pos2::new(r.right() - 8.0, r.top() + 8.0), Vec2::splat(10.0)), Icon::Up, theme::ON_ACCENT);
    }
    if resp.clicked() {
        if selected && matches!(t, Tool::Brush | Tool::Eraser) {
            app.ui.sheet = Some(SideTab::Brushes);
        }
        app.tools.tool = t;
    }
    if selected && crate::panels::phone::has_options(t) {
        egui::Popup::menu(&resp).align(egui::RectAlign::TOP).show(|ui| {
            ui.set_min_width(260.0);
            crate::panels::phone::tool_options_pub(app, ui);
        });
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
                .inner_margin(Margin { left: 0, right: 0, top: 0, bottom: bottom + 10 }),
        )
        .show(ui, |ui| {
            crate::panels::timeline::commit_selection_on_click(app, ui);
            grabber(app, ui);
            header(app, ui);
            ui.add_space(4.0);
            if app.ui.dock_open {
                tracks(app, ui);
                ui.add_space(12.0);
                action_bar(app, ui);
            } else {
                strip(app, ui);
            }
        });
}

/// Tap or drag up/down: show or hide the tracks.
fn grabber(app: &mut TrazoApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 19.0), Sense::click_and_drag());
    ui.painter().rect_filled(Rect::from_center_size(Pos2::new(r.center().x, r.top() + 10.5), Vec2::new(36.0, 5.0)), 3.0, CONTROL);
    let delta = ui.input(|i| i.pointer.delta().y);
    if resp.clicked() || (resp.dragged() && delta < -6.0 && !app.ui.dock_open) || (resp.dragged() && delta > 6.0 && app.ui.dock_open) {
        app.ui.dock_open = !app.ui.dock_open;
    }
    resp.on_hover_text(if app.ui.dock_open { tr("Bajar: ver solo los dibujos") } else { tr("Subir: una pista por capa y el sonido") });
}

fn header(app: &mut TrazoApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_space(6.0);
        play_button(app, ui);
        counter(app, ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            if app.ui.dock_open {
                if widgets::toggle_icon(ui, Icon::Loop, app.playback.looping, tr("Bucle")).clicked() {
                    app.playback.looping = !app.playback.looping;
                }
                let r = widgets::toggle_icon(ui, Icon::Onion, app.onion.enabled, tr("Papel cebolla (mantén pulsado para ajustarlo)"));
                if r.clicked() {
                    toggle_onion(app);
                }
                egui::Popup::context_menu(&r).show(|ui| crate::panels::phone::frame_menu(app, ui));
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
        job.append(&tf!("\n{} s de {} s", crate::i18n::num((cur - 1) as f32 / fps, 2), crate::i18n::num(n as f32 / fps, 2)), 4.0, egui::TextFormat { font_id: theme::font(12.0, W::Regular), color: TEXT2, ..Default::default() });
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
        let strip_left = ui.cursor().min.x;
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
                    if app.ui.drawing_menu == Some(b.start) {
                        ui.data_mut(|d| d.insert_temp(egui::Id::unique("dmenu_cell"), (b.start, thumb)));
                    }
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
        // Fade into the dock before "Nuevo".
        let fade = Rect::from_min_size(Pos2::new(strip_left + full - new_w - 24.0, ui.min_rect().top()), Vec2::new(24.0, height));
        let mut mesh = egui::Mesh::default();
        let (a, b) = (Color32::from_rgba_unmultiplied(22, 22, 24, 0), theme::SHEET);
        mesh.colored_vertex(fade.left_top(), a);
        mesh.colored_vertex(fade.right_top(), b);
        mesh.colored_vertex(fade.left_bottom(), a);
        mesh.colored_vertex(fade.right_bottom(), b);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        ui.painter().add(Shape::mesh(mesh));
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

/// 40 × 46 icon button of the island (undo, redo).
fn narrow_button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(40.0, 46.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && (resp.hovered() || resp.is_pointer_button_down_on()) {
        ui.painter().rect_filled(rect, 12.0, SEPARATOR);
    }
    paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(22.0)), icon, if enabled { TEXT } else { TEXT3 });
    resp.on_hover_text(tip)
}

/// The color picker floating over the canvas, above the island (closes on a
/// tap outside it).
pub fn color_popover(app: &mut TrazoApp, ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let w = (screen.width() - 24.0).min(420.0);
    let mut inside = false;
    let mut dim_clicked = false;
    egui::Area::new(egui::Id::unique("phone_color_dim")).order(Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(140));
        dim_clicked = resp.clicked();
    });
    let h_id = egui::Id::unique("phone_color_h");
    let h = ctx.data(|d| d.get_temp::<f32>(h_id)).unwrap_or(430.0);
    let bottom = screen.bottom() - 253.0;
    let resp = egui::Area::new(egui::Id::unique("phone_color")).order(Order::Foreground).pivot(Align2::CENTER_TOP)
        .fixed_pos(Pos2::new(screen.center().x, (bottom - h).max(screen.top() + 8.0)))
        .constrain(false)
        .show(ctx, |ui| {
            Frame::new().fill(SURFACE).corner_radius(theme::R_ISLAND).shadow(egui::Shadow { offset: [0, 20], blur: 50, spread: 0, color: Color32::from_black_alpha(166) }).stroke(Stroke::new(1.0, Color32::from_white_alpha(18))).inner_margin(Margin::same(14)).show(ui, |ui| {
                ui.set_width(w - 28.0);
                ui.spacing_mut().item_spacing.y = 12.0;
                crate::panels::toolbar::color_panel(app, ui);
            });
            inside = ui.ui_contains_pointer();
        });
    let r = resp.response.rect;
    if (r.height() - h).abs() > 0.5 {
        ctx.data_mut(|d| d.insert_temp(h_id, r.height()));
        ctx.request_repaint();
    }
    let clicked_outside = dim_clicked && !inside;
    if clicked_outside {
        app.ui.sheet = None;
    }
}

// ---- drawing menu ------------------------------------------------------------------------

/// Long press on a drawing of the strip: everything you can do with it.
pub fn drawing_menu(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(frame) = app.ui.drawing_menu else { return };
    let layer = app.doc.layer;
    let Some(b) = tl::block_at(&app.doc.project, layer, frame) else {
        app.ui.drawing_menu = None;
        return;
    };
    let index = tl::blocks(&app.doc.project, layer).iter().position(|x| *x == b).unwrap_or(0) + 1;
    let screen = ctx.content_rect();
    // Dim everything behind; a tap there closes the menu.
    let mut close = false;
    // The pressed drawing stays lit through the dim, with a lime ring.
    let hole = ctx.data(|d| d.get_temp::<(usize, Rect)>(egui::Id::unique("dmenu_cell"))).filter(|(f, _)| *f == b.start).map(|(_, r)| r);
    egui::Area::new(egui::Id::unique("dmenu_dim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
        let dim = Color32::from_black_alpha(140);
        match hole {
            Some(h) => {
                let p = ui.painter();
                p.rect_filled(Rect::from_min_max(r.min, Pos2::new(r.right(), h.top())), 0.0, dim);
                p.rect_filled(Rect::from_min_max(Pos2::new(r.left(), h.bottom()), r.max), 0.0, dim);
                p.rect_filled(Rect::from_min_max(Pos2::new(r.left(), h.top()), Pos2::new(h.left(), h.bottom())), 0.0, dim);
                p.rect_filled(Rect::from_min_max(Pos2::new(h.right(), h.top()), Pos2::new(r.right(), h.bottom())), 0.0, dim);
                p.rect_stroke(h.expand(1.0), 9.0, Stroke::new(3.0, ACCENT), egui::StrokeKind::Outside);
            }
            None => {
                ui.painter().rect_filled(r, 0.0, dim);
            }
        }
        close |= resp.clicked();
    });
    let h_id = egui::Id::unique("dmenu_h");
    let h = ctx.data(|d| d.get_temp::<f32>(h_id)).unwrap_or(400.0);
    let bottom = screen.bottom() - 248.0;
    let w = (screen.width() - 36.0).min(420.0);
    let mut act: Option<&'static str> = None;
    let mut new_len = None;
    let resp = egui::Area::new(egui::Id::unique("dmenu")).order(Order::Tooltip).pivot(Align2::CENTER_TOP)
        .fixed_pos(Pos2::new(screen.center().x, (bottom - h).max(screen.top() + 8.0))).constrain(false)
        .show(ctx, |ui| {
            Frame::new().fill(ELEVATED).corner_radius(theme::R_ISLAND).inner_margin(Margin::same(8))
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(15)))
                .shadow(egui::Shadow { offset: [0, 24], blur: 60, spread: 0, color: Color32::from_black_alpha(178) })
                .show(ui, |ui| {
                    ui.set_width(w - 16.0);
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                    // Quick actions.
                    ui.horizontal(|ui| {
                        let qw = (ui.available_width() - 6.0) / 4.0;
                        let has_clip = app.drawing_clipboard.is_some();
                        for (ic, label, key, on) in [(Icon::Copy, tr("Copiar"), "copy", true), (Icon::Paste, tr("Pegar"), "paste", has_clip), (Icon::Duplicate, tr("Duplicar"), "dup", true), (Icon::New, tr("Nuevo"), "new", true)] {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(qw, 64.0), if on { Sense::click() } else { Sense::hover() });
                            let col = if on { TEXT } else { TEXT3 };
                            if on && (resp.hovered() || resp.is_pointer_button_down_on()) {
                                ui.painter().rect_filled(r, 14.0, SEPARATOR);
                            }
                            paint(ui.painter(), Rect::from_center_size(Pos2::new(r.center().x, r.top() + 21.0), Vec2::splat(22.0)), ic, col);
                            ui.painter().text(Pos2::new(r.center().x, r.top() + 46.0), Align2::CENTER_CENTER, label, theme::font(11.0, W::Medium), col);
                            if resp.clicked() {
                                act = Some(key);
                            }
                        }
                    });
                    ui.add_space(6.0);
                    hairline(ui, 0.0);
                    // Duration stepper.
                    ui.horizontal(|ui| {
                        ui.set_height(56.0);
                        ui.add_space(14.0);
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                        paint(ui.painter(), r, Icon::Duration, TEXT);
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(tr("Duración")).font(theme::font(15.0, W::Regular)).color(TEXT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(6.0);
                            let (r, _) = ui.allocate_exact_size(Vec2::new(116.0, 40.0), Sense::hover());
                            ui.painter().rect_filled(r, 20.0, Color32::from_rgb(50, 50, 54));
                            let minus = Rect::from_min_size(r.min, Vec2::new(44.0, 40.0));
                            let plus = Rect::from_min_size(Pos2::new(r.right() - 44.0, r.top()), Vec2::new(44.0, 40.0));
                            let rm = ui.interact(minus, ui.id().with("dm_minus"), Sense::click());
                            let rp = ui.interact(plus, ui.id().with("dm_plus"), Sense::click());
                            let p = ui.painter();
                            p.text(minus.center(), Align2::CENTER_CENTER, "−", theme::font(20.0, W::Regular), if b.len > 1 { TEXT } else { TEXT3 });
                            p.text(plus.center(), Align2::CENTER_CENTER, "+", theme::font(20.0, W::Regular), TEXT);
                            p.text(r.center(), Align2::CENTER_CENTER, b.len.to_string(), theme::font(15.0, W::Semi), TEXT);
                            if rm.on_hover_text(tr("Un fotograma menos")).clicked() && b.len > 1 {
                                new_len = Some(b.len - 1);
                            }
                            if rp.on_hover_text(tr("Que dure un fotograma más")).clicked() {
                                new_len = Some(b.len + 1);
                            }
                        });
                    });
                    if menu_row(ui, Icon::InsertBefore, tr("Insertar dibujo antes"), None, TEXT) {
                        act = Some("before");
                    }
                    let sub = tf!("Trazo dibuja los pasos entre el {} y el {}", index, index + 1);
                    if menu_row(ui, Icon::Tween, tr("Dibujos intermedios"), Some(&sub), TEXT) {
                        act = Some("tween");
                    }
                    if menu_row(ui, Icon::Motion, tr("Grabar movimiento"), Some(tr("Arrastra algo y se dibuja en cada fotograma")), TEXT) {
                        act = Some("record");
                    }
                    hairline(ui, 10.0);
                    if menu_row(ui, Icon::Trash, tr("Borrar dibujo"), None, theme::DANGER) {
                        act = Some("delete");
                    }
                });
        });
    let r = resp.response.rect;
    if (r.height() - h).abs() > 0.5 {
        ctx.data_mut(|d| d.insert_temp(h_id, r.height()));
        ctx.request_repaint();
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if act.is_some() || new_len.is_some() {
        app.stop_playback();
    }
    if let Some(len) = new_len {
        let d = &mut app.doc;
        if tl::set_len(&mut d.project, &mut d.history, layer, b.start, len) {
            d.invalidate_all();
        }
    }
    if let Some(a) = act {
        do_action(app, ctx, layer, b, a);
        close = true;
    }
    if close {
        app.ui.drawing_menu = None;
    }
}

fn hairline(ui: &mut Ui, inset: f32) {
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.left() + inset..=r.right() - inset, r.center().y, Stroke::new(1.0, Color32::from_white_alpha(18)));
    ui.add_space(4.0);
}

/// A menu item: icon, label and an optional second line.
fn menu_row(ui: &mut Ui, icon: Icon, label: &str, sub: Option<&str>, color: Color32) -> bool {
    let h = 48.0;
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let p = ui.painter();
    if resp.hovered() || resp.is_pointer_button_down_on() {
        p.rect_filled(r, 12.0, SEPARATOR);
    }
    paint(p, Rect::from_center_size(Pos2::new(r.left() + 25.0, r.center().y), Vec2::splat(22.0)), icon, color);
    let x = r.left() + 50.0;
    match sub {
        Some(s) => {
            p.text(Pos2::new(x, r.center().y - 8.0), Align2::LEFT_CENTER, label, theme::font(15.0, W::Regular), color);
            p.text(Pos2::new(x, r.center().y + 10.0), Align2::LEFT_CENTER, s, theme::font(12.0, W::Regular), TEXT2);
        }
        None => {
            p.text(Pos2::new(x, r.center().y), Align2::LEFT_CENTER, label, theme::font(15.0, W::Regular), color);
        }
    }
    resp.clicked()
}

/// What the drawing menu and the tracks' action bar do with a drawing.
fn do_action(app: &mut TrazoApp, ctx: &egui::Context, layer: trazo_core::LayerId, b: tl::Block, act: &str) {
    app.stop_playback();
    let d = &mut app.doc;
    match act {
        "copy" => {
            if let Some(cel) = d.project.frames.get(b.start).and_then(|f| f.cels.get(&layer)) {
                app.drawing_clipboard = Some((cel.clone(), b.len));
                app.toast(ctx, tr("Dibujo copiado"));
            }
        }
        "paste" => {
            if let Some((cel, len)) = app.drawing_clipboard.clone() {
                let at = tl::insert_drawing(&mut d.project, &mut d.history, layer, b.start, len, Some(cel));
                d.invalidate_all();
                d.set_frame(at);
            }
        }
        "dup" => {
            if let Some(at) = tl::duplicate(&mut d.project, &mut d.history, layer, b.start) {
                d.invalidate_all();
                d.set_frame(at);
            }
        }
        "new" => {
            d.set_frame(b.start);
            crate::panels::timeline::add_frame(app);
        }
        "before" => {
            let at = tl::insert_drawing(&mut d.project, &mut d.history, layer, b.start, 1, None);
            let at = tl::shift(&mut d.project, &mut d.history, layer, at, -1).unwrap_or(at);
            d.invalidate_all();
            d.set_frame(at);
        }
        "tween" | "record" => {
            d.set_frame(b.start);
            let err = app.canvas.select.select_all(&mut app.doc);
            if let Some(e) = err {
                app.toast(ctx, e);
            } else if act == "record" {
                match app.canvas.select.arm_recording(&app.doc) {
                    Ok(()) => app.toast(ctx, tr("Arrastra el dibujo: cada momento se vuelve un dibujo")),
                    Err(e) => app.toast(ctx, e),
                }
            } else {
                app.toast(ctx, tr("Mueve el dibujo a su posición final y toca «Intermedios»"));
            }
        }
        "delete" => {
            if tl::delete(&mut d.project, &mut d.history, layer, b.start) {
                d.invalidate_all();
                app.toast(ctx, tr("Dibujo borrado · Deshacer lo recupera"));
            }
        }
        _ => {}
    }
}

// ---- expanded tracks -------------------------------------------------------------------

const LABEL_W: f32 = 96.0;
const ROW_H: f32 = 52.0;
const PPF: f32 = 48.0;
const RULER_H: f32 = 24.0;

#[derive(Clone, Copy, Default)]
struct TrackView {
    x: f32,
    y: f32,
    follow: usize,
    stretch: Option<(trazo_core::LayerId, usize, usize)>,
}

enum Row {
    Layer(usize),
    Audio,
}

/// One row per layer (and one for the sound), drawings as colored cells.
fn tracks(app: &mut TrazoApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let vid = egui::Id::unique("phone_tracks");
    let mut view: TrackView = ctx.data(|d| d.get_temp(vid)).unwrap_or(TrackView { follow: usize::MAX, ..Default::default() });
    let n_layers = app.doc.project.layers.len();
    let mut rows: Vec<Row> = (0..n_layers).rev().map(Row::Layer).collect();
    if !app.doc.project.audio.tracks.is_empty() {
        rows.push(Row::Audio);
    }
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W + 12.0);
        ui.label(egui::RichText::new(tr("Pellizca: acercar · flecha blanca: alargar")).font(theme::font(11.0, W::Regular)).color(TEXT2));
    });
    let visible_rows = rows.len().min(5) as f32;
    let (area, bg) = ui.allocate_exact_size(Vec2::new(ui.available_width(), RULER_H + visible_rows * ROW_H), Sense::click_and_drag());
    let lanes = Rect::from_min_max(Pos2::new(area.left() + LABEL_W, area.top()), area.max);
    let n_frames = app.doc.project.frames.len();
    let shown = app.displayed_frame();
    let fps = app.doc.project.canvas.fps.max(1) as f32;
    let max_x = ((n_frames as f32 + 2.0) * PPF - lanes.width()).max(0.0);
    let max_y = ((rows.len() as f32 - visible_rows) * ROW_H).max(0.0);
    // Keep the playhead in view when it moves (playing, or picked elsewhere).
    if view.follow != shown {
        let px = shown as f32 * PPF;
        if px < view.x + PPF || px > view.x + lanes.width() - 2.0 * PPF || view.follow == usize::MAX {
            view.x = (px - lanes.width() * 0.4).clamp(0.0, max_x);
        }
        view.follow = shown;
    }
    if bg.dragged() && view.stretch.is_none() {
        let d = bg.drag_delta();
        view.x = (view.x - d.x).clamp(0.0, max_x);
        view.y = (view.y - d.y).clamp(0.0, max_y);
    }
    if ui.rect_contains_pointer(area) {
        let sd = ui.input(|i| i.smooth_scroll_delta);
        view.x = (view.x - sd.x - if sd.x == 0.0 { sd.y } else { 0.0 }).clamp(0.0, max_x);
    }
    let x_of = |f: f32| lanes.left() + 2.0 + f * PPF - view.x;
    let p = ui.painter().with_clip_rect(area);
    let lp = ui.painter().with_clip_rect(lanes);
    let first = (view.x / PPF).floor().max(0.0) as usize;
    let last = (((view.x + lanes.width()) / PPF).ceil() as usize + 1).min(n_frames);

    // Ruler.
    for f in first..last {
        if f == shown {
            continue;
        }
        lp.text(Pos2::new(x_of(f as f32) + 22.0, area.top() + 12.0), Align2::CENTER_CENTER, (f + 1).to_string(), theme::font(11.0, W::Regular), TEXT2);
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let long_press = bg.long_touched() || bg.secondary_clicked();
    let mut pick: Option<(Option<trazo_core::LayerId>, usize)> = None;
    let mut menu = None;
    if bg.clicked() || long_press {
        if let Some(pt) = pointer {
            let f = (((pt.x - lanes.left() + view.x) / PPF).floor().max(0.0) as usize).min(n_frames.saturating_sub(1));
            if pt.y < area.top() + RULER_H {
                pick = Some((None, f));
            } else {
                let row = ((pt.y - area.top() - RULER_H + view.y) / ROW_H).floor() as usize;
                match rows.get(row) {
                    Some(Row::Layer(li)) => {
                        let id = app.doc.project.layers[*li].id;
                        if pt.x < lanes.left() {
                            pick = Some((Some(id), shown));
                        } else {
                            pick = Some((Some(id), f));
                            if long_press {
                                menu = Some((id, f));
                            }
                        }
                    }
                    Some(Row::Audio) => app.audio_ui.show = true,
                    None => {}
                }
            }
        }
    }
    let row_clip = Rect::from_min_max(Pos2::new(area.left(), area.top() + RULER_H), area.max);
    let rp = ui.painter().with_clip_rect(row_clip);
    let lrp = ui.painter().with_clip_rect(Rect::from_min_max(Pos2::new(lanes.left(), area.top() + RULER_H), area.max));
    let mut handle: Option<(Rect, trazo_core::LayerId, tl::Block)> = None;
    for (ri, row) in rows.iter().enumerate() {
        let top = area.top() + RULER_H + ri as f32 * ROW_H - view.y;
        let full = Rect::from_min_size(Pos2::new(area.left(), top), Vec2::new(area.width(), ROW_H));
        if full.bottom() < row_clip.top() || full.top() > row_clip.bottom() {
            continue;
        }
        match row {
            Row::Layer(li) => {
                let layer = &app.doc.project.layers[*li];
                let (dot, cell) = theme::TRACKS[(n_layers - 1 - li) % theme::TRACKS.len()];
                let active = layer.id == app.doc.layer;
                if active {
                    rp.rect_filled(full, 0.0, Color32::from_rgb(29, 29, 32));
                }
                let dim = layer.props.shared || !layer.props.visible;
                rp.circle_filled(Pos2::new(full.left() + 16.0, full.center().y), 4.0, dot);
                let name = crate::i18n::layer_name(&layer.props.name).into_owned();
                let g = crate::panels::home::one_line_pub(ui, &name, theme::font(13.0, W::Semi), if dim { TEXT2 } else { Color32::from_rgb(226, 226, 223) }, LABEL_W - 32.0);
                rp.galley(Pos2::new(full.left() + 27.0, full.center().y - g.size().y / 2.0), g, TEXT);
                let cell_r = |s: usize, len: usize| Rect::from_min_size(Pos2::new(x_of(s as f32), top + 4.0), Vec2::new(len as f32 * PPF - 4.0, 44.0));
                if layer.props.shared || layer.props.video.is_some() {
                    let r = Rect::from_min_max(Pos2::new(x_of(0.0), top + 4.0), Pos2::new(lanes.right() + 20.0, top + 48.0));
                    lrp.rect_filled(r, 9.0, cell);
                    let msg = if layer.props.shared { tr("Fondo · toda la toma") } else { tr("Video para calcar") };
                    lrp.text(Pos2::new(r.left().max(lanes.left()) + 10.0, r.center().y), Align2::LEFT_CENTER, msg, theme::font(12.0, W::Regular), lerp(dot, TEXT, 0.4));
                    continue;
                }
                for b in tl::blocks(&app.doc.project, layer.id) {
                    if b.end() <= first || b.start >= last {
                        continue;
                    }
                    let r = cell_r(b.start, b.len);
                    let sel = active && b.contains(shown);
                    if sel {
                        lrp.rect_filled(r, 9.0, dot);
                        lrp.rect_stroke(r, 9.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Outside);
                        let idx = tl::blocks(&app.doc.project, layer.id).iter().position(|x| *x == b).unwrap_or(0) + 1;
                        lrp.text(r.left_top() + Vec2::new(6.0, 3.0), Align2::LEFT_TOP, idx.to_string(), theme::font(11.0, W::Bold), Color32::from_rgb(14, 34, 54));
                        handle = Some((Rect::from_min_size(Pos2::new(r.right() - 16.0, top + 12.0), Vec2::new(24.0, 28.0)), layer.id, b));
                    } else {
                        lrp.rect_filled(r, 9.0, cell);
                    }
                    if b.len > 1 && r.width() > 80.0 {
                        let col = if sel { Color32::from_rgb(14, 34, 54) } else { lerp(dot, TEXT, 0.7) };
                        lrp.text(Pos2::new(r.right() - 8.0 - if sel { 16.0 } else { 0.0 }, r.center().y), Align2::RIGHT_CENTER, tf!("Dura {}", b.len), theme::font(11.0, W::Semi), col);
                    }
                }
            }
            Row::Audio => {
                let (dot, cell) = theme::AUDIO_TRACK;
                paint(&rp, Rect::from_center_size(Pos2::new(full.left() + 19.0, full.center().y), Vec2::splat(15.0)), Icon::Music, TEXT2);
                rp.text(Pos2::new(full.left() + 34.0, full.center().y), Align2::LEFT_CENTER, tr("Audio"), theme::font(13.0, W::Semi), Color32::from_rgb(226, 226, 223));
                for t in &app.doc.project.audio.tracks {
                    for c in &t.clips {
                        let s = trazo_core::audio::samples_to_ticks(c.start, fps as u32) as f32;
                        let e = trazo_core::audio::samples_to_ticks(c.end(), fps as u32) as f32;
                        let r = Rect::from_min_max(Pos2::new(x_of(s), top + 4.0), Pos2::new(x_of(e) - 4.0, top + 48.0));
                        lrp.rect_filled(r, 9.0, cell);
                        lrp.hline(r.left() + 6.0..=r.right() - 6.0, r.center().y, Stroke::new(1.0, dot.gamma_multiply(0.6)));
                    }
                }
            }
        }
    }
    // Stretch handle of the selected drawing.
    if let Some((hr, lid, b)) = handle {
        let resp = ui.interact(hr.expand(6.0), vid.with("stretch"), Sense::drag());
        if resp.drag_started() {
            view.stretch = Some((lid, b.start, b.len));
        }
        if let (Some((l, start, _)), Some(pt)) = (view.stretch, resp.interact_pointer_pos()) {
            if resp.dragged() {
                let len = (((pt.x - x_of(start as f32) + 8.0) / PPF).round().max(1.0)) as usize;
                if len != tl::block_at(&app.doc.project, l, start).map_or(0, |b| b.len) {
                    let d = &mut app.doc;
                    if tl::set_len(&mut d.project, &mut d.history, l, start, len) {
                        d.invalidate_all();
                    }
                }
            }
        }
        if resp.drag_stopped() {
            view.stretch = None;
        }
        if lanes.contains(hr.center()) && hr.center().y > row_clip.top() {
            lrp.rect_filled(hr.translate(Vec2::new(0.0, 2.0)), 7.0, Color32::from_black_alpha(120));
            lrp.rect_filled(hr, 7.0, TEXT);
            paint(&lrp, Rect::from_center_size(hr.center(), Vec2::splat(14.0)), Icon::Next, theme::ON_ACCENT);
        }
        resp.on_hover_text(tr("Arrastra para que dure más"));
    }
    // Playhead.
    let px = x_of(shown as f32) + 22.0;
    if px >= lanes.left() && px <= lanes.right() {
        p.vline(px, area.top() + 20.0..=area.bottom(), Stroke::new(2.0, ACCENT));
        let pill = Rect::from_center_size(Pos2::new(px, area.top() + 11.0), Vec2::new(28.0, 22.0));
        p.rect_filled(pill, 7.0, ACCENT);
        p.text(pill.center(), Align2::CENTER_CENTER, (shown + 1).to_string(), theme::font(12.0, W::Bold), theme::ON_ACCENT);
    }
    if let Some((layer, f)) = pick {
        app.stop_playback();
        if let Some(l) = layer {
            app.doc.layer = l;
        }
        app.doc.set_frame(f);
    }
    if let Some((_, f)) = menu {
        app.ui.drawing_menu = Some(f);
    }
    if view.stretch.is_some() || bg.dragged() {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(vid, view));
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// What to do with the selected drawing: the drawing menu's main actions.
fn action_bar(app: &mut TrazoApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let layer = app.doc.layer;
    let block = crate::panels::timeline::selected_block(app);
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 66.0), Sense::hover());
    let bar = r.shrink2(Vec2::new(10.0, 0.0));
    let p = ui.painter();
    p.rect_filled(bar, 20.0, Color32::from_rgb(31, 31, 34));
    p.rect_stroke(bar, 20.0, Stroke::new(1.0, Color32::from_white_alpha(13)), egui::StrokeKind::Inside);
    let Some(b) = block else {
        let msg = match app.doc.project.layer(layer) {
            Some(l) if l.props.shared => tr("Capa de fondo fijo: se ve en toda la toma"),
            Some(l) if l.props.video.is_some() => tr("Capa de video: ajústala en Capas"),
            _ => tr("Nada aquí: toca «Nuevo» o dibuja para crear un dibujo"),
        };
        p.text(bar.center(), Align2::CENTER_CENTER, msg, theme::font(12.0, W::Regular), TEXT2);
        return;
    };
    let inner = bar.shrink2(Vec2::new(4.0, 0.0));
    let unit = inner.width() / 4.5;
    let slot = |i: f32, w: f32| Rect::from_min_size(Pos2::new(inner.left() + i * unit, inner.top() + 4.0), Vec2::new(w * unit, 58.0));
    let mut act: Option<&'static str> = None;
    let mut new_len = None;
    let action = |ui: &mut Ui, r: Rect, icon: Icon, label: &str, id: &str| -> egui::Response {
        let resp = ui.interact(r, ui.id().with(("bar", id)), Sense::click());
        if resp.hovered() || resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(r, 14.0, SEPARATOR);
        }
        paint(ui.painter(), Rect::from_center_size(Pos2::new(r.center().x, r.top() + 20.0), Vec2::splat(22.0)), icon, TEXT);
        ui.painter().text(Pos2::new(r.center().x, r.top() + 43.0), Align2::CENTER_CENTER, label, theme::font(12.0, W::Medium), TEXT);
        resp
    };
    if action(ui, slot(0.0, 1.0), Icon::Duplicate, tr("Duplicar"), "dup").clicked() {
        act = Some("dup");
    }
    // Duration stepper.
    let sr = slot(1.0, 1.5);
    let pill = Rect::from_center_size(Pos2::new(sr.center().x, sr.top() + 18.0), Vec2::new(108.0, 36.0));
    let p = ui.painter();
    p.rect_filled(pill, 18.0, Color32::from_rgb(42, 42, 46));
    let minus = Rect::from_min_size(pill.min, Vec2::new(44.0, 36.0));
    let plus = Rect::from_min_size(Pos2::new(pill.right() - 44.0, pill.top()), Vec2::new(44.0, 36.0));
    p.text(minus.center(), Align2::CENTER_CENTER, "−", theme::font(20.0, W::Regular), if b.len > 1 { TEXT } else { TEXT3 });
    p.text(plus.center(), Align2::CENTER_CENTER, "+", theme::font(20.0, W::Regular), TEXT);
    p.text(pill.center(), Align2::CENTER_CENTER, b.len.to_string(), theme::font(14.0, W::Semi), TEXT);
    p.text(Pos2::new(sr.center().x, sr.top() + 49.0), Align2::CENTER_CENTER, tr("Duración"), theme::font(12.0, W::Medium), TEXT);
    if ui.interact(minus, ui.id().with("bar_minus"), Sense::click()).on_hover_text(tr("Dura menos")).clicked() && b.len > 1 {
        new_len = Some(b.len - 1);
    }
    if ui.interact(plus, ui.id().with("bar_plus"), Sense::click()).on_hover_text(tr("Dura más")).clicked() {
        new_len = Some(b.len + 1);
    }
    if action(ui, slot(2.5, 1.0), Icon::Tween, tr("Intermedios"), "tween").clicked() {
        act = Some("tween");
    }
    let more = action(ui, slot(3.5, 1.0), Icon::More, tr("Más"), "more");
    egui::Popup::menu(&more).align(egui::RectAlign::TOP_END).show(|ui| {
        ui.set_min_width(250.0);
        for (ic, label, key, color) in [
            (Icon::New, tr("Nuevo dibujo después"), "new", TEXT),
            (Icon::InsertBefore, tr("Insertar dibujo antes"), "before", TEXT),
            (Icon::Copy, tr("Copiar"), "copy", TEXT),
            (Icon::Paste, tr("Pegar"), "paste", TEXT),
            (Icon::Motion, tr("Grabar movimiento"), "record", TEXT),
            (Icon::Trash, tr("Borrar dibujo"), "delete", theme::DANGER),
        ] {
            if key == "paste" && app.drawing_clipboard.is_none() {
                continue;
            }
            if menu_row(ui, ic, label, None, color) {
                act = Some(key);
                ui.close();
            }
        }
        hairline(ui, 6.0);
        crate::panels::phone::frame_menu(app, ui);
    });
    if let Some(len) = new_len {
        app.stop_playback();
        let d = &mut app.doc;
        if tl::set_len(&mut d.project, &mut d.history, layer, b.start, len) {
            d.invalidate_all();
        }
    }
    if let Some(a) = act {
        do_action(app, &ctx, layer, b, a);
    }
}
