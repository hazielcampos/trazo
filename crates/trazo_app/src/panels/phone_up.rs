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
                    if widgets::icon_button(ui, Icon::Focus, tr("Modo enfoque: solo herramientas y lienzo")).clicked() {
                        toggle_focus(app);
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
    let bubble = Pos2::new(area.center().x, area.bottom() - CANVAS_MARGIN.bottom as f32 - 8.0);
    size_rail_at(app, ctx, pos, h, bubble);
}

/// The size / opacity rail at `pos`, `h` tall; the value bubble shows at `bubble`.
pub fn size_rail_at(app: &mut TrazoApp, ctx: &egui::Context, pos: Pos2, h: f32, bubble: Pos2) {
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
        let at = bubble;
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
                tracks(app, ui, 0.0);
                ui.add_space(12.0);
                action_bar(app, ui);
                ui.add_space(20.0);
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
    /// Width of the lanes last time (a new layout follows the playhead again).
    w: f32,
    /// Last sound clip brought into view, and for how many rows.
    sel: Option<u64>,
    sel_rows: f32,
}

enum Row {
    Layer(usize),
    /// A sound track (index in the project's audio tracks).
    Audio(usize),
    /// The last row: add sound or record a voice.
    AddSound,
}

/// A clip being moved or trimmed in the tracks.
#[derive(Clone)]
struct ClipDrag {
    clip: trazo_core::audio::ClipId,
    track: usize,
    mode: u8, // 0 move, 1 trim start, 2 trim end
    orig: trazo_core::audio::AudioClip,
    before: Vec<trazo_core::audio::AudioTrack>,
}

/// One row per layer (and one for the sound), drawings as colored cells.
fn tracks(app: &mut TrazoApp, ui: &mut Ui, reserve: f32) {
    tracks_styled(app, ui, reserve, TrackStyle::PHONE);
}

#[derive(Clone, Copy)]
pub(crate) struct TrackStyle {
    pub label_w: f32,
    pub row_h: f32,
    pub ppf: f32,
    pub ruler_h: f32,
    pub max_rows: usize,
    pub hint: bool,
    pub eye: bool,
    /// PC: lock next to the eye, and every drawing numbered.
    pub pc: bool,
}

impl TrackStyle {
    pub const PHONE: TrackStyle = TrackStyle { label_w: LABEL_W, row_h: ROW_H, ppf: PPF, ruler_h: RULER_H, max_rows: 6, hint: true, eye: false, pc: false };
    pub const TABLET: TrackStyle = TrackStyle { label_w: 150.0, row_h: 48.0, ppf: 48.0, ruler_h: 22.0, max_rows: 6, hint: false, eye: true, pc: false };
}

/// Rows of the timeline for this project (layers, then sound).
pub(crate) fn track_rows(app: &TrazoApp) -> usize {
    app.doc.project.layers.len() + app.doc.project.audio.tracks.len() + 1
}

pub(crate) fn tracks_styled(app: &mut TrazoApp, ui: &mut Ui, reserve: f32, st: TrackStyle) {
    let ctx = ui.ctx().clone();
    let vid = egui::Id::unique("phone_tracks");
    let mut view: TrackView = ctx.data(|d| d.get_temp(vid)).unwrap_or(TrackView { follow: usize::MAX, ..Default::default() });
    let n_layers = app.doc.project.layers.len();
    let mut rows: Vec<Row> = (0..n_layers).rev().map(Row::Layer).collect();
    // Sound tracks, then a row to add more (or the first).
    rows.extend((0..app.doc.project.audio.tracks.len()).map(Row::Audio));
    rows.push(Row::AddSound);
    ui.spacing_mut().item_spacing.y = 0.0;
    if st.hint {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(st.label_w + 12.0);
            ui.label(egui::RichText::new(tr("Pellizca: acercar · flecha blanca: alargar")).font(theme::font(11.0, W::Regular)).color(TEXT2));
        });
        ui.add_space(4.0);
    }
    let fit_rows = if st.hint { ((ui.available_height() - reserve - st.ruler_h) / st.row_h).floor().max(1.0) as usize } else { usize::MAX };
    let visible_rows = rows.len().min(st.max_rows).min(fit_rows) as f32;
    let (area, bg) = ui.allocate_exact_size(Vec2::new(ui.available_width(), st.ruler_h + visible_rows * st.row_h), Sense::click_and_drag());
    let lanes = Rect::from_min_max(Pos2::new(area.left() + st.label_w, area.top()), area.max);
    let n_frames = app.doc.project.frames.len();
    let shown = app.displayed_frame();
    let fps = app.doc.project.canvas.fps.max(1) as f32;
    let max_x = ((n_frames as f32 + 2.0) * st.ppf - lanes.width()).max(0.0);
    let max_y = ((rows.len() as f32 - visible_rows) * st.row_h).max(0.0);
    // A newly selected sound clip (just imported or recorded) comes into view.
    if app.audio_ui.selected != view.sel || (app.audio_ui.selected.is_some() && view.sel_rows != visible_rows) {
        view.sel = app.audio_ui.selected;
        view.sel_rows = visible_rows;
        if let Some(id) = view.sel {
            if let Some((ti, c)) = app.doc.project.audio.find_clip(id) {
                if let Some(ri) = rows.iter().position(|r| matches!(r, Row::Audio(t) if *t == ti)) {
                    let y = ri as f32 * st.row_h;
                    if y < view.y || y + st.row_h > view.y + visible_rows * st.row_h {
                        view.y = (y - (visible_rows - 1.0).max(0.0) * st.row_h * 0.5).clamp(0.0, max_y);
                    }
                }
                let x = trazo_core::audio::samples_to_ticks(c.start, fps as u32) as f32 * st.ppf;
                if x > view.x + lanes.width() || x + 40.0 < view.x {
                    view.x = (x - 20.0).clamp(0.0, max_x);
                }
            }
        }
    }
    // Keep the playhead in view when it moves (playing, or picked elsewhere).
    if (view.w - lanes.width()).abs() > 1.0 {
        view.w = lanes.width();
        view.follow = usize::MAX;
    }
    if view.follow != shown {
        let px = shown as f32 * st.ppf;
        if px < view.x + st.ppf || px > view.x + lanes.width() - 2.0 * st.ppf || view.follow == usize::MAX {
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
    let x_of = |f: f32| lanes.left() + 2.0 + f * st.ppf - view.x;
    let p = ui.painter().with_clip_rect(area);
    let lp = ui.painter().with_clip_rect(lanes);
    let first = (view.x / st.ppf).floor().max(0.0) as usize;
    let last = (((view.x + lanes.width()) / st.ppf).ceil() as usize + 1).min(n_frames);

    // Ruler: the playback range as a band.
    if let Some((a, b)) = app.playback.range {
        let band = Rect::from_min_max(Pos2::new(x_of(a as f32), area.top() + 2.0), Pos2::new(x_of(b as f32 + 1.0) - 4.0, area.top() + st.ruler_h - 2.0));
        lp.rect_filled(band, 4.0, Color32::from_white_alpha(22));
    }
    for f in first..last {
        if f == shown || (!st.hint && f != 0 && (f + 1) % 5 != 0) {
            continue;
        }
        lp.text(Pos2::new(x_of(f as f32) + 22.0, area.top() + 12.0), Align2::CENTER_CENTER, (f + 1).to_string(), theme::font(11.0, W::Regular), TEXT2);
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let long_press = bg.long_touched() || bg.secondary_clicked();
    let mut pick: Option<(Option<trazo_core::LayerId>, usize)> = None;
    let mut menu = None;
    let mut eye_toggle: Option<usize> = None;
    let mut lock_toggle: Option<usize> = None;
    let mut keep_clip = false;
    let mut sound_menu: Option<Pos2> = None;
    let drag_id = vid.with("clip_drag");
    if bg.clicked() || long_press {
        if let Some(pt) = pointer {
            let f = (((pt.x - lanes.left() + view.x) / st.ppf).floor().max(0.0) as usize).min(n_frames.saturating_sub(1));
            if pt.y < area.top() + st.ruler_h {
                pick = Some((None, f));
            } else {
                let row = ((pt.y - area.top() - st.ruler_h + view.y) / st.row_h).floor() as usize;
                match rows.get(row) {
                    Some(Row::Layer(li)) => {
                        let id = app.doc.project.layers[*li].id;
                        if pt.x < lanes.left() {
                            if st.pc && (lanes.left() - 59.0..lanes.left() - 29.0).contains(&pt.x) && !long_press {
                                lock_toggle = Some(*li);
                            } else if st.pc && (lanes.left() - 89.0..lanes.left() - 59.0).contains(&pt.x) && !long_press {
                                eye_toggle = Some(*li);
                            } else if !st.pc && st.eye && pt.x > lanes.left() - 40.0 && !long_press {
                                eye_toggle = Some(*li);
                            }
                            pick = Some((Some(id), shown));
                        } else {
                            pick = Some((Some(id), f));
                            if long_press {
                                menu = Some((id, f));
                            }
                        }
                    }
                    Some(Row::Audio(ti)) => {
                        // A clip under the finger is selected; the playhead goes there too.
                        let tick = (pt.x - lanes.left() + view.x - 2.0) / st.ppf;
                        let at = trazo_core::audio::ticks_to_samples(tick as f64, fps as u32);
                        let hit = app.doc.project.audio.tracks.get(*ti).and_then(|t| t.clips.iter().rev().find(|c| at >= c.start && at < c.end()).map(|c| c.id));
                        app.audio_ui.selected = hit;
                        if !long_press {
                            pick = Some((None, f));
                            keep_clip = hit.is_some();
                        }
                    }
                    Some(Row::AddSound) => {
                        sound_menu = Some(pt);
                    }
                    None => {}
                }
            }
        }
    }
    let row_clip = Rect::from_min_max(Pos2::new(area.left(), area.top() + st.ruler_h), area.max);
    let rp = ui.painter().with_clip_rect(row_clip);
    let lrp = ui.painter().with_clip_rect(Rect::from_min_max(Pos2::new(lanes.left(), area.top() + st.ruler_h), area.max));
    let mut handle: Option<(Rect, trazo_core::LayerId, tl::Block)> = None;
    for (ri, row) in rows.iter().enumerate() {
        let top = area.top() + st.ruler_h + ri as f32 * st.row_h - view.y;
        let full = Rect::from_min_size(Pos2::new(area.left(), top), Vec2::new(area.width(), st.row_h));
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
                let g = crate::panels::home::one_line_pub(ui, &name, theme::font(13.0, W::Semi), if dim { TEXT2 } else { Color32::from_rgb(226, 226, 223) }, st.label_w - if st.pc { 110.0 } else if st.eye { 70.0 } else { 32.0 });
                rp.galley(Pos2::new(full.left() + 27.0, full.center().y - g.size().y / 2.0), g, TEXT);
                if st.eye {
                    let ic = if layer.props.visible { Icon::Eye } else { Icon::EyeOff };
                    let ex = if st.pc { lanes.left() - 74.0 } else { lanes.left() - 20.0 };
                    paint(&rp, Rect::from_center_size(Pos2::new(ex, full.center().y), Vec2::splat(if st.pc { 16.0 } else { 18.0 })), ic, if layer.props.visible { TEXT2 } else { TEXT3 });
                }
                if st.pc {
                    let ic = if layer.props.locked { Icon::Lock } else { Icon::Unlock };
                    paint(&rp, Rect::from_center_size(Pos2::new(lanes.left() - 44.0, full.center().y), Vec2::splat(15.0)), ic, if layer.props.locked { TEXT } else { TEXT3 });
                }
                let cell_r = |s: usize, len: usize| Rect::from_min_size(Pos2::new(x_of(s as f32), top + 4.0), Vec2::new(len as f32 * st.ppf - 4.0, st.row_h - 8.0));
                if layer.props.shared || layer.props.video.is_some() {
                    let r = Rect::from_min_max(Pos2::new(x_of(0.0), top + 4.0), Pos2::new(lanes.right() + 20.0, top + st.row_h - 4.0));
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
                        handle = Some((Rect::from_min_size(Pos2::new(r.right() - 16.0, top + (st.row_h - 28.0) / 2.0), Vec2::new(24.0, 28.0)), layer.id, b));
                    } else {
                        lrp.rect_filled(r, if st.pc { 6.0 } else { 9.0 }, cell);
                        if st.pc && r.width() > 16.0 {
                            let idx = tl::blocks(&app.doc.project, layer.id).iter().position(|x| *x == b).unwrap_or(0) + 1;
                            lrp.text(r.left_top() + Vec2::new(6.0, 3.0), Align2::LEFT_TOP, idx.to_string(), theme::font(10.0, W::Medium), lerp(dot, TEXT, 0.6));
                        }
                    }
                    if b.len > 1 && r.width() > 80.0 {
                        let col = if sel { Color32::from_rgb(14, 34, 54) } else { lerp(dot, TEXT, 0.7) };
                        lrp.text(Pos2::new(r.right() - 8.0 - if sel { 16.0 } else { 0.0 }, r.center().y), Align2::RIGHT_CENTER, tf!("Dura {}", b.len), theme::font(11.0, W::Semi), col);
                    }
                }
            }
            Row::Audio(ti) => {
                let (dot, cell) = theme::AUDIO_TRACK;
                let Some(track) = app.doc.project.audio.tracks.get(*ti) else { continue };
                let has_sel = app.audio_ui.selected.is_some_and(|id| track.clip(id).is_some());
                if has_sel {
                    rp.rect_filled(full, 0.0, Color32::from_rgb(29, 29, 32));
                }
                paint(&rp, Rect::from_center_size(Pos2::new(full.left() + 19.0, full.center().y), Vec2::splat(15.0)), Icon::Music, TEXT2);
                let g = crate::panels::home::one_line_pub(ui, &track.name, theme::font(13.0, W::Semi), Color32::from_rgb(226, 226, 223), st.label_w - 44.0);
                rp.galley(Pos2::new(full.left() + 34.0, full.center().y - g.size().y / 2.0), g, TEXT);
                let pps = st.ppf * fps;
                for c in &track.clips {
                    let s = trazo_core::audio::samples_to_ticks(c.start, fps as u32) as f32;
                    let e = trazo_core::audio::samples_to_ticks(c.end(), fps as u32) as f32;
                    let r = Rect::from_min_max(Pos2::new(x_of(s), top + 4.0), Pos2::new(x_of(e) - 2.0, top + st.row_h - 4.0));
                    if r.right() < lanes.left() || r.left() > lanes.right() {
                        continue;
                    }
                    let sel = app.audio_ui.selected == Some(c.id);
                    let fill = if c.muted { cell.gamma_multiply(0.45) } else if sel { lerp(cell, dot, 0.35) } else { cell };
                    lrp.rect_filled(r, if st.pc { 6.0 } else { 9.0 }, fill);
                    if let Some(asset) = app.doc.project.audio.assets.get(&c.asset) {
                        let visible = r.intersect(lrp.clip_rect());
                        let wave = if c.muted { dot.gamma_multiply(0.35) } else if sel { TEXT } else { dot };
                        crate::panels::audio::draw_wave(&lrp, visible, r, c, asset, pps, wave);
                        if r.width() > 60.0 && st.row_h >= 40.0 {
                            let name = crate::panels::home::one_line_pub(ui, &asset.name, theme::font(10.0, W::Semi), lerp(dot, TEXT, 0.5), r.width() - 12.0);
                            lrp.galley(Pos2::new(r.left().max(lanes.left()) + 6.0, r.top() + 2.0), name, TEXT);
                        }
                    }
                    if sel {
                        lrp.rect_stroke(r, if st.pc { 6.0 } else { 9.0 }, Stroke::new(2.0, ACCENT), egui::StrokeKind::Outside);
                        // Trim handles.
                        for x in [r.left() + 3.0, r.right() - 3.0] {
                            lrp.rect_filled(Rect::from_center_size(Pos2::new(x, r.center().y), Vec2::new(4.0, (r.height() - 14.0).max(8.0))), 2.0, TEXT);
                        }
                        // Drag: move, or trim from the edges.
                        let resp = ui.interact(r.intersect(lanes), vid.with(("clip", c.id)), Sense::drag());
                        if resp.drag_started() {
                            if let Some(pt) = resp.interact_pointer_pos() {
                                let edge = (14.0f32).min(r.width() / 3.0);
                                let mode = if pt.x < r.left() + edge { 1 } else if pt.x > r.right() - edge { 2 } else { 0 };
                                let d = ClipDrag { clip: c.id, track: *ti, mode, orig: c.clone(), before: app.doc.project.audio.tracks.clone() };
                                ctx.data_mut(|m| m.insert_temp(drag_id, d));
                            }
                        }
                    }
                }
            }
            Row::AddSound => {
                let (dot, cell) = theme::AUDIO_TRACK;
                let r = Rect::from_min_max(Pos2::new(area.left() + 8.0, top + 6.0), Pos2::new((lanes.left() + 260.0).min(area.right() - 8.0), top + st.row_h - 6.0));
                rp.rect_filled(r, 9.0, cell.gamma_multiply(0.45));
                dashed_rect(&rp, r, Stroke::new(1.5, dot.gamma_multiply(0.6)));
                paint(&rp, Rect::from_center_size(Pos2::new(r.left() + 18.0, r.center().y), Vec2::splat(16.0)), Icon::Plus, dot);
                rp.text(Pos2::new(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, tr("Añadir sonido o grabar tu voz"), theme::font(12.0, W::Semi), dot);
            }
        }
    }
    // A clip being moved or trimmed.
    if let Some(d) = ctx.data(|m| m.get_temp::<ClipDrag>(drag_id)) {
        let (down, delta_x) = ui.input(|i| (i.pointer.any_down(), i.pointer.press_origin().zip(i.pointer.latest_pos()).map(|(a, b)| b.x - a.x)));
        if let Some(dx) = delta_x.filter(|_| down) {
            let ds = (dx / (st.ppf * fps) * trazo_core::audio::SAMPLE_RATE as f32) as i64;
            let frames = app.doc.project.audio.assets.get(&d.orig.asset).map(|a| a.frames()).unwrap_or(d.orig.offset + d.orig.len);
            let min_len = (trazo_core::audio::SAMPLE_RATE / 20) as i64;
            if let Some(c) = app.doc.project.audio.tracks.get_mut(d.track).and_then(|t| t.clip_mut(d.clip)) {
                let o = &d.orig;
                match d.mode {
                    0 => c.start = o.start + ds,
                    1 => {
                        let ds = ds.clamp(-(o.offset as i64), o.len as i64 - min_len);
                        c.start = o.start + ds;
                        c.offset = (o.offset as i64 + ds) as u64;
                        c.len = (o.len as i64 - ds) as u64;
                    }
                    _ => {
                        let max = frames as i64 - o.offset as i64;
                        c.len = (o.len as i64 + ds).clamp(min_len, max.max(min_len)) as u64;
                    }
                }
                app.doc.audio_version += 1;
            }
            ctx.request_repaint();
        } else {
            ctx.data_mut(|m| m.remove::<ClipDrag>(drag_id));
            crate::panels::audio::push_tracks_edit(app, d.before);
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
                let len = (((pt.x - x_of(start as f32) + 8.0) / st.ppf).round().max(1.0)) as usize;
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
        if !keep_clip {
            app.audio_ui.selected = None;
        }
    }
    // "+ Añadir sonido…": import a file or record a voice.
    let menu_id = vid.with("sound_menu");
    if let Some(pt) = sound_menu {
        ctx.data_mut(|m| m.insert_temp(menu_id, pt));
    }
    if let Some(at) = ctx.data(|m| m.get_temp::<Pos2>(menu_id)) {
        let mut close = false;
        let screen = ctx.content_rect();
        let resp = egui::Area::new(menu_id).order(Order::Tooltip).pivot(Align2::LEFT_BOTTOM).fixed_pos(Pos2::new(at.x.min(screen.right() - 280.0), at.y - 8.0)).constrain(true).show(&ctx, |ui| {
            Frame::new().fill(ELEVATED).corner_radius(16).inner_margin(Margin::same(6)).stroke(Stroke::new(1.0, Color32::from_white_alpha(15))).shadow(theme::island_shadow()).show(ui, |ui| {
                ui.set_width(260.0);
                if menu_row(ui, Icon::Import, tr("Importar sonido o música"), Some(tr("MP3, WAV, M4A… va al fotograma actual")), TEXT) {
                    crate::panels::audio::set_target_track(app, None);
                    crate::files::request(app, &ctx, crate::files::PickPurpose::Audio);
                    close = true;
                }
                if menu_row(ui, Icon::Mic, tr("Grabar tu voz"), Some(tr("Cuenta atrás y suena la animación")), TEXT) {
                    crate::panels::audio::set_target_track(app, None);
                    crate::panels::audio::begin_recording(app, &ctx);
                    close = true;
                }
            });
        });
        let clicked_out = ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() && sound_menu.is_none();
        if close || clicked_out {
            ctx.data_mut(|m| m.remove::<Pos2>(menu_id));
        }
    }
    if let Some(li) = lock_toggle {
        if let Some(l) = app.doc.project.layers.get_mut(li) {
            l.props.locked = !l.props.locked;
            app.doc.mark_settings_changed();
        }
    }
    if let Some(li) = eye_toggle {
        if let Some(l) = app.doc.project.layers.get_mut(li) {
            l.props.visible = !l.props.visible;
            app.doc.mark_settings_changed();
            app.doc.redraw = Redraw::Full;
        }
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
    if record_bar(app, ui) || clip_bar(app, ui) {
        return;
    }
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

// ---- first time: gestures ----------------------------------------------------------------

fn gestures_seen_path(app: &TrazoApp) -> std::path::PathBuf {
    app.platform.data_dir.join("gestos_vistos.txt")
}

/// "Cuatro gestos y listo": shown once, the first time the editor opens on a
/// touch screen. `Some(true)` = every gesture listed.
pub fn gestures_card(app: &mut TrazoApp, ctx: &egui::Context) {
    let checked = egui::Id::unique("gestures_checked");
    // Test captures skip it (unless asked for).
    let capturing = cfg!(debug_assertions) && std::env::var_os("TRAZO_ESCALA1").is_some();
    if app.ui.gestures_card.is_none() && app.platform.touch_first && !capturing && !ctx.data(|d| d.get_temp::<bool>(checked).unwrap_or(false)) {
        ctx.data_mut(|d| d.insert_temp(checked, true));
        if !gestures_seen_path(app).exists() {
            app.ui.gestures_card = Some(false);
        }
    }
    let Some(all) = app.ui.gestures_card else { return };
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("gestures_dim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(158));
    });
    let bottom = crate::widgets::insets(app.platform.touch_first).1;
    let w = (screen.width() - 24.0).min(440.0);
    let h_id = egui::Id::unique("gestures_h");
    let h = ctx.data(|d| d.get_temp::<f32>(h_id)).unwrap_or(520.0);
    let mut close = false;
    let mut more = false;
    let resp = egui::Area::new(egui::Id::unique("gestures_card")).order(Order::Tooltip).pivot(Align2::CENTER_TOP)
        .fixed_pos(Pos2::new(screen.center().x, (screen.bottom() - 28.0 - bottom - h).max(screen.top() + 8.0))).constrain(false)
        .show(ctx, |ui| {
            Frame::new().fill(SURFACE).corner_radius(24).inner_margin(Margin { left: 18, right: 18, top: 22, bottom: 18 })
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(18)))
                .shadow(egui::Shadow { offset: [0, 24], blur: 60, spread: 0, color: Color32::from_black_alpha(178) })
                .show(ui, |ui| {
                    ui.set_width(w - 36.0);
                    ui.spacing_mut().item_spacing.y = 16.0;
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(egui::RichText::new(tr("Antes de empezar")).font(theme::font(13.0, W::Regular)).color(TEXT2));
                        ui.label(egui::RichText::new(if all { tr("Todos los gestos") } else { tr("Cuatro gestos y listo") }).font(theme::font(24.0, W::Bold)).color(TEXT));
                    });
                    let max_h = screen.height() - 300.0;
                    egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 16.0;
                        gesture_row(ui, 0, tr("Toca con dos dedos"), tr("Deshace lo último. Con tres, lo rehace."));
                        gesture_row(ui, 1, tr("Pellizca"), tr("Acerca, aleja y gira el dibujo."));
                        gesture_row(ui, 2, tr("Mantén pulsado un dibujo"), tr("Abre su menú: copiar, duplicar…"));
                        gesture_row(ui, 3, tr("Sube el panel de abajo"), tr("Ves cada capa y el sonido por separado."));
                        if all {
                            gesture_row(ui, 4, tr("Arrastra con dos dedos"), tr("Mueve el lienzo."));
                            gesture_row(ui, 5, tr("Toca otra vez la herramienta"), tr("Abre sus ajustes: pinceles, relleno, formas…"));
                        }
                    });
                    Frame::new().fill(Color32::from_rgb(35, 35, 38)).corner_radius(12).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(egui::RichText::new(tr("Toca el nombre de la animación para girar, voltear o recolocar el lienzo.")).font(theme::font(13.0, W::Regular)).color(TEXT2));
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let full = ui.available_width() - 10.0;
                        let (a, b) = (full / 2.4, full * 1.4 / 2.4);
                        if !all {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(a, 50.0), Sense::click());
                            ui.painter().rect_filled(r, 14.0, if resp.hovered() { SEPARATOR } else { ELEVATED });
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, tr("Ver todos"), theme::font(15.0, W::Semi), TEXT);
                            more = resp.clicked();
                        }
                        let bw = if all { full + 10.0 } else { b };
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(bw, 50.0), Sense::click());
                        ui.painter().rect_filled(r, 14.0, theme::PRIMARY);
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, tr("Empezar a dibujar"), theme::font(15.0, W::Bold), theme::ON_ACCENT);
                        close = resp.clicked();
                    });
                });
        });
    let r = resp.response.rect;
    if (r.height() - h).abs() > 0.5 {
        ctx.data_mut(|d| d.insert_temp(h_id, r.height()));
        ctx.request_repaint();
    }
    if more {
        app.ui.gestures_card = Some(true);
    }
    if close {
        app.ui.gestures_card = None;
        let _ = std::fs::write(gestures_seen_path(app), "1");
    }
}

/// A gesture: a small drawing of the fingers, what to do and what it does.
fn gesture_row(ui: &mut Ui, kind: u8, title: &str, what: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let (r, _) = ui.allocate_exact_size(Vec2::splat(60.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 16.0, ELEVATED);
        let o = r.min;
        let at = |x: f32, y: f32| o + Vec2::new(x, y);
        let mark = Stroke::new(1.6, TEXT.gamma_multiply(0.55));
        match kind {
            0 => {
                for x in [23.0, 37.0] {
                    p.circle_stroke(at(x, 30.0), 11.0, mark);
                    p.circle_filled(at(x, 30.0), 6.0, TEXT);
                }
            }
            1 => {
                p.circle_filled(at(21.0, 37.0), 5.5, TEXT);
                p.circle_filled(at(39.0, 23.0), 5.5, TEXT);
                for pts in [[(15.0, 43.0), (10.0, 48.0)], [(45.0, 17.0), (50.0, 12.0)]] {
                    p.line_segment([at(pts[0].0, pts[0].1), at(pts[1].0, pts[1].1)], mark);
                }
                p.add(Shape::line(vec![at(10.0, 42.0), at(10.0, 48.0), at(16.0, 48.0)], mark));
                p.add(Shape::line(vec![at(50.0, 18.0), at(50.0, 12.0), at(44.0, 12.0)], mark));
            }
            2 => {
                p.rect_stroke(Rect::from_min_size(at(17.0, 14.0), Vec2::new(26.0, 32.0)), 5.0, mark, egui::StrokeKind::Middle);
                p.circle_stroke(at(30.0, 30.0), 11.0, mark);
                p.circle_filled(at(30.0, 30.0), 6.0, TEXT);
            }
            3 => {
                p.rect_filled(Rect::from_min_size(at(10.0, 38.0), Vec2::new(40.0, 14.0)), 4.0, CONTROL);
                p.circle_filled(at(30.0, 38.0), 5.5, TEXT);
                p.line_segment([at(30.0, 28.0), at(30.0, 12.0)], mark);
                p.add(Shape::line(vec![at(25.0, 17.0), at(30.0, 12.0), at(35.0, 17.0)], mark));
            }
            4 => {
                for x in [24.0, 36.0] {
                    p.circle_filled(at(x, 34.0), 5.5, TEXT);
                }
                p.line_segment([at(18.0, 22.0), at(42.0, 22.0)], mark);
                p.add(Shape::line(vec![at(37.0, 17.0), at(42.0, 22.0), at(37.0, 27.0)], mark));
            }
            _ => {
                p.rect_filled(Rect::from_min_size(at(16.0, 16.0), Vec2::new(28.0, 28.0)), 8.0, ACCENT);
                p.circle_filled(at(36.0, 38.0), 5.5, TEXT);
                p.circle_stroke(at(36.0, 38.0), 11.0, mark);
            }
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(10.0);
            ui.label(egui::RichText::new(title).font(theme::font(16.0, W::Semi)).color(TEXT));
            ui.label(egui::RichText::new(what).font(theme::font(13.0, W::Regular)).color(TEXT2));
        });
    });
}

// ---- phone on its side (TelHorizontal, HorLinea, HorCapas…) ------------------------------

/// Room for the notch / rounded corners on each side.
const SIDE: f32 = 44.0;
const RAIL_W: f32 = 56.0;
const ISLAND_H: f32 = 58.0;

fn card_width(app: &TrazoApp, screen: Rect) -> Option<f32> {
    if app.ui.focus {
        None
    } else if app.ui.dock_open {
        Some((screen.width() - 2.0 * SIDE - RAIL_W - 28.0 - 184.0).max(360.0))
    } else if app.ui.sheet.is_some() {
        Some(340.0f32.min(screen.width() * 0.45))
    } else {
        None
    }
}

/// Room kept free around the canvas (left, right, bottom): the rails, the
/// card and the timeline island.
pub fn landscape_sides(app: &TrazoApp, screen: Rect) -> (f32, f32, f32) {
    if app.ui.focus {
        return (SIDE + RAIL_W + 12.0 + 48.0, SIDE, 14.0);
    }
    let left = SIDE + RAIL_W + 12.0 + if card_width(app, screen).is_none() { 48.0 } else { 0.0 };
    let right = match card_width(app, screen) {
        Some(w) => SIDE + w + 16.0,
        None => SIDE + RAIL_W + 12.0,
    };
    let bottom = if app.ui.dock_open { 14.0 } else { 24.0 + ISLAND_H + 12.0 };
    (left, right, bottom)
}

/// Empty side panels so the canvas fits between the floating controls.
pub fn landscape_panels(app: &TrazoApp, ui: &mut Ui) {
    let (l, r, b) = landscape_sides(app, ui.ctx().content_rect());
    let frame = Frame::new().fill(theme::CANVAS_BG);
    egui::Panel::bottom("hor_bottom_room").exact_size(b).resizable(false).show_separator_line(false).frame(frame).show(ui, |_| {});
    egui::Panel::left("hor_left_room").exact_size(l).resizable(false).show_separator_line(false).frame(frame).show(ui, |_| {});
    egui::Panel::right("hor_right_room").exact_size(r).resizable(false).show_separator_line(false).frame(frame).show(ui, |_| {});
}

pub fn landscape(app: &mut TrazoApp, ctx: &egui::Context, screen: Rect) {
    let card = card_width(app, screen);
    // Left: back and tools.
    egui::Area::new(egui::Id::unique("hor_left")).order(Order::Foreground).fixed_pos(Pos2::new(screen.left() + SIDE, screen.top() + 14.0)).show(ctx, |ui| {
        rail_frame().show(ui, |ui| {
            ui.set_width(RAIL_W - 12.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.vertical_centered(|ui| {
                if widgets::icon_button(ui, Icon::ArrowLeft, tr("Mis animaciones")).clicked() {
                    app.go_home(ui.ctx());
                }
                rail_sep(ui);
                for t in MAIN_TOOLS {
                    tool_slot(app, ui, t, Vec2::splat(44.0));
                }
                crate::panels::phone::more_tools_pub(app, ui, Vec2::splat(44.0));
            });
        });
    });
    if card.is_none() {
        let h = 196.0f32.min(screen.height() - 120.0);
        if h >= 120.0 {
            let bubble = Pos2::new(screen.center().x, screen.bottom() - 24.0 - ISLAND_H - 12.0);
            size_rail_at(app, ctx, Pos2::new(screen.left() + SIDE + RAIL_W + 12.0, screen.top() + 40.0), h, bubble);
        }
        // Right: export, history, panels.
        if !app.ui.focus {
        egui::Area::new(egui::Id::unique("hor_right")).order(Order::Foreground).pivot(Align2::RIGHT_TOP).fixed_pos(Pos2::new(screen.right() - SIDE, screen.top() + 14.0)).show(ctx, |ui| {
            rail_frame().show(ui, |ui| {
                ui.set_width(RAIL_W - 12.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.vertical_centered(|ui| {
                    let ctx = ui.ctx().clone();
                    if widgets::icon_button_ex(ui, Icon::Export, tr("Exportar y compartir"), true, Some(ELEVATED)).clicked() {
                        app.export_ui.open = true;
                    }
                    let can_undo = app.doc.history.can_undo() || app.canvas.select.is_active();
                    if widgets::icon_button_ex(ui, Icon::Undo, tr("Deshacer (toca con 2 dedos)"), can_undo, None).clicked() {
                        app.undo(&ctx);
                    }
                    if widgets::icon_button_ex(ui, Icon::Redo, tr("Rehacer (toca con 3 dedos)"), app.doc.history.can_redo(), None).clicked() {
                        app.redo(&ctx);
                    }
                    rail_sep(ui);
                    if widgets::icon_button(ui, Icon::Layers, tr("Capas")).clicked() {
                        app.ui.sheet = Some(SideTab::Layers);
                    }
                    if widgets::icon_button(ui, Icon::Brush, tr("Pinceles")).clicked() {
                        app.ui.sheet = Some(SideTab::Brushes);
                    }
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
                    let p = ui.painter();
                    p.circle_filled(rect.center(), 18.0, TEXT);
                    p.circle_filled(rect.center(), 16.0, SURFACE);
                    p.circle_filled(rect.center(), 14.0, app.tools.color);
                    if resp.on_hover_text(tr("Color")).clicked() {
                        app.ui.sheet = Some(SideTab::Color);
                    }
                    rail_sep(ui);
                    if widgets::icon_button(ui, Icon::Focus, tr("Modo enfoque: solo herramientas y lienzo")).clicked() {
                        toggle_focus(app);
                    }
                });
            });
        });
        }
    }
    // Bottom: the compact timeline (not while the tracks are open).
    if !app.ui.dock_open && !app.ui.focus {
        let left = screen.left() + SIDE + RAIL_W + 12.0;
        let right = match card {
            Some(w) => screen.right() - SIDE - w - 16.0,
            None => screen.right() - SIDE - RAIL_W - 12.0,
        };
        hor_island(app, ctx, Rect::from_min_max(Pos2::new(left, screen.bottom() - 24.0 - ISLAND_H), Pos2::new(right, screen.bottom() - 24.0)), card.is_some());
    }
    // Right card: a panel or the tracks.
    if let Some(w) = card {
        let r = Rect::from_min_max(Pos2::new(screen.right() - SIDE - w, screen.top() + 14.0), Pos2::new(screen.right() - SIDE, screen.bottom() - 14.0));
        egui::Area::new(egui::Id::unique("hor_card")).order(Order::Foreground).fixed_pos(r.min).show(ctx, |ui| {
            let pad = if app.ui.dock_open { 0 } else { 8 };
            Frame::new().fill(SURFACE).corner_radius(24).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).shadow(theme::island_shadow()).inner_margin(Margin::same(pad)).show(ui, |ui| {
                ui.set_width(r.width() - 2.0 * pad as f32);
                ui.set_height(r.height() - 2.0 * pad as f32);
                if app.ui.dock_open {
                    hor_tracks(app, ui);
                } else if let Some(tab) = app.ui.sheet {
                    ui.horizontal(|ui| {
                        ui.set_min_height(48.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::icon_button(ui, Icon::Close, tr("Cerrar")).clicked() {
                                app.ui.sheet = None;
                            }
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.add_space(8.0);
                                match tab {
                                    SideTab::Layers => crate::panels::layers::header_hor(ui),
                                    SideTab::Brushes => crate::panels::brushes::header(ui),
                                    SideTab::Color => {
                                        ui.label(egui::RichText::new(tr("Color")).font(theme::font(20.0, W::Semi)).color(TEXT));
                                    }
                                }
                            });
                        });
                    });
                    egui::ScrollArea::vertical().id_salt("hor_card_body").auto_shrink([false, false]).show(ui, |ui| match tab {
                        SideTab::Layers => {
                            crate::panels::layers::set_row_style(ui.ctx(), crate::panels::layers::RowStyle::LANDSCAPE);
                            crate::panels::layers::show(app, ui)
                        }
                        SideTab::Brushes => crate::panels::brushes::show(app, ui),
                        SideTab::Color => {
                            egui::Frame::new().inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 12.0;
                                crate::panels::toolbar::color_panel(app, ui);
                            });
                        }
                    });
                }
            });
        });
    }
}

fn rail_frame() -> Frame {
    Frame::new().fill(SURFACE).corner_radius(22).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).shadow(theme::island_shadow()).inner_margin(Margin::symmetric(6, 6))
}

fn rail_sep(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(44.0, 7.0), Sense::hover());
    ui.painter().hline(r.center().x - 13.0..=r.center().x + 13.0, r.center().y, Stroke::new(1.0, SEPARATOR));
}

/// Play, counter, a few drawings, onion, layer, new and "open the tracks".
fn hor_island(app: &mut TrazoApp, ctx: &egui::Context, r: Rect, short: bool) {
    egui::Area::new(egui::Id::unique("hor_island")).order(Order::Foreground).fixed_pos(r.min).show(ctx, |ui| {
        Frame::new().fill(SURFACE).corner_radius(22).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).shadow(theme::island_shadow()).inner_margin(Margin::symmetric(7, 7)).show(ui, |ui| {
            ui.set_width(r.width() - 14.0);
            ui.set_height(ISLAND_H - 14.0);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                play_button(app, ui);
                let n = app.doc.project.frames.len();
                let cur = app.displayed_frame() + 1;
                let mut job = egui::text::LayoutJob::default();
                job.append(&format!("{cur:02}"), 0.0, egui::TextFormat { font_id: theme::counter_font(15.0), color: TEXT, ..Default::default() });
                job.append(&format!(" /{n}"), 0.0, egui::TextFormat { font_id: theme::font(12.0, W::Regular), color: TEXT2, valign: egui::Align::Max, ..Default::default() });
                ui.add_sized([58.0, 44.0], egui::Label::new(job));
                // Right side first so the drawings get what is left.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !short && widgets::icon_button(ui, Icon::Up, tr("Subir: una pista por capa y el sonido")).clicked() {
                        app.ui.dock_open = true;
                        app.ui.sheet = None;
                    }
                    // New drawing (dashed).
                    let g = ui.painter().layout_no_wrap(tr("Nuevo").to_string(), theme::font(13.0, W::Semi), TEXT);
                    let (nr, nresp) = ui.allocate_exact_size(Vec2::new(g.size().x + 40.0, 44.0), Sense::click());
                    ui.painter().rect_filled(nr, 12.0, ELEVATED);
                    dashed_rect(ui.painter(), nr.shrink(0.75), Stroke::new(1.5, TEXT3));
                    paint(ui.painter(), Rect::from_center_size(Pos2::new(nr.left() + 16.0, nr.center().y), Vec2::splat(20.0)), Icon::Plus, TEXT);
                    ui.painter().galley(Pos2::new(nr.left() + 30.0, nr.center().y - g.size().y / 2.0), g, TEXT);
                    if nresp.on_hover_text(tr("Nuevo dibujo después del actual (N)")).clicked() {
                        crate::panels::timeline::add_frame(app);
                    }
                    if !short {
                        let name = app.doc.project.layer(app.doc.layer).map(|l| crate::i18n::layer_name(&l.props.name).into_owned()).unwrap_or_default();
                        if widgets::pill_button(ui, Icon::Layers, &name, TEXT, false).on_hover_text(tr("Capas")).clicked() {
                            app.ui.sheet = Some(SideTab::Layers);
                        }
                        let on = app.onion.enabled;
                        let rr = widgets::pill_button(ui, Icon::Onion, tr("Cebolla"), if on { ACCENT } else { TEXT2 }, on).on_hover_text(tr("Papel cebolla (mantén pulsado para ajustarlo)"));
                        if rr.clicked() {
                            toggle_onion(app);
                        }
                        egui::Popup::context_menu(&rr).show(|ui| crate::panels::phone::frame_menu(app, ui));
                    }
                    // The drawings around the current one.
                    let room = ui.available_width() - 8.0;
                    if room > 40.0 {
                        ui.allocate_ui_with_layout(Vec2::new(room, 44.0), egui::Layout::left_to_right(egui::Align::Center), |ui| mini_strip(app, ui, room));
                    }
                });
            });
        });
    });
}

/// Small thumbnails of the current layer's drawings, the current one ringed.
fn mini_strip(app: &mut TrazoApp, ui: &mut Ui, room: f32) {
    let ctx = ui.ctx().clone();
    let layer = app.doc.layer;
    let blocks = tl::blocks(&app.doc.project, layer);
    let shown = app.displayed_frame();
    let cur = blocks.iter().position(|b| b.contains(shown));
    let cell = Vec2::new(34.0, 42.0);
    let fit = ((room + 4.0) / (cell.x + 4.0)).floor().max(1.0) as usize;
    let c = cur.unwrap_or(0);
    let first = c.saturating_sub(fit.saturating_sub(1));
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut goto = None;
    for (i, b) in blocks.iter().enumerate().skip(first).take(fit) {
        let (r, resp) = ui.allocate_exact_size(cell, Sense::click());
        let p = ui.painter();
        frame_thumb(app, &ctx, p, r, b.start);
        if Some(i) == cur {
            p.rect_stroke(r, 8.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Outside);
        } else {
            p.rect_filled(r, 8.0, Color32::from_black_alpha(70));
        }
        if resp.clicked() {
            goto = Some(b.start);
        }
        if resp.long_touched() || resp.secondary_clicked() {
            goto = Some(b.start);
            app.ui.drawing_menu = Some(b.start);
        }
    }
    if let Some(f) = goto {
        app.stop_playback();
        app.doc.set_frame(f);
    }
}

/// The tracks in the right card: header, tracks, actions.
fn hor_tracks(app: &mut TrazoApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::Frame::new().inner_margin(Margin { left: 8, right: 8, top: 8, bottom: 0 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            play_button(app, ui);
            let n = app.doc.project.frames.len();
            let cur = app.displayed_frame() + 1;
            let fps = app.doc.project.canvas.fps.max(1) as f32;
            let mut job = egui::text::LayoutJob::default();
            job.append(&format!("{cur:02}"), 4.0, egui::TextFormat { font_id: theme::counter_font(20.0), color: TEXT, ..Default::default() });
            job.append(&tf!(" / {} · {} s", n, crate::i18n::num((cur - 1) as f32 / fps, 2)), 0.0, egui::TextFormat { font_id: theme::font(13.0, W::Regular), color: TEXT2, valign: egui::Align::Max, ..Default::default() });
            ui.label(job);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, Icon::Down, tr("Bajar: ver solo los dibujos")).clicked() {
                    app.ui.dock_open = false;
                }
                if widgets::toggle_icon(ui, Icon::Loop, app.playback.looping, tr("Bucle")).clicked() {
                    app.playback.looping = !app.playback.looping;
                }
                let r = widgets::toggle_icon(ui, Icon::Onion, app.onion.enabled, tr("Papel cebolla (mantén pulsado para ajustarlo)"));
                if r.clicked() {
                    toggle_onion(app);
                }
                egui::Popup::context_menu(&r).show(|ui| crate::panels::phone::frame_menu(app, ui));
            });
        });
    });
    let st = TrackStyle { label_w: 86.0, row_h: 44.0, ppf: 48.0, ruler_h: 20.0, max_rows: 6, hint: false, eye: false, pc: false };
    let rows = ((ui.available_height() - 60.0 - st.ruler_h) / st.row_h).floor().max(1.0) as usize;
    tracks_styled(app, ui, 0.0, TrackStyle { max_rows: rows, ..st });
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
        ui.add_space(8.0);
        egui::Frame::new().inner_margin(Margin::symmetric(8, 0)).show(ui, |ui| action_bar_inline(app, ui));
    });
}

// ---- tablet (Tablet board) -------------------------------------------------------------------

const T_TOOLS: [Tool; 6] = [Tool::Brush, Tool::Eraser, Tool::Fill, Tool::Eyedropper, Tool::Lasso, Tool::Shape];

/// The room the floating islands keep free around the canvas, and the
/// timeline island at the bottom.
pub fn tablet_panels(app: &mut TrazoApp, ui: &mut Ui) {
    let room = Frame::new().fill(theme::CANVAS_BG);
    let n_rows = track_rows(app);
    // Expanded, the timeline still leaves the canvas at least half the screen.
    let fit = ((ui.ctx().content_rect().height() * 0.5 - 200.0) / 48.0).floor().max(2.0) as usize;
    let shown_rows = if app.ui.dock_open { n_rows.min(6).min(fit) } else { n_rows.min(2) };
    let more_row = if n_rows > 2 { 36.0 } else { 8.0 };
    if !app.ui.focus {
    egui::Panel::bottom("tab_timeline").resizable(false).show_separator_line(false)
        .frame(Frame::new().fill(theme::CANVAS_BG).inner_margin(Margin { left: 20, right: 20, top: 12, bottom: 0 }))
        .show(ui, |ui| {
            Frame::new().fill(SURFACE).corner_radius(22).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).shadow(theme::island_shadow()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                crate::panels::timeline::commit_selection_on_click(app, ui);
                // Grabber: drag up for every track.
                let (g, gr) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 12.0), Sense::click_and_drag());
                ui.painter().rect_filled(Rect::from_center_size(Pos2::new(g.center().x, g.top() + 9.5), Vec2::new(36.0, 5.0)), 3.0, CONTROL);
                let dy = ui.input(|i| i.pointer.delta().y);
                if gr.clicked() || (gr.dragged() && ((dy < -4.0 && !app.ui.dock_open) || (dy > 4.0 && app.ui.dock_open))) {
                    app.ui.dock_open = !app.ui.dock_open;
                }
                tablet_transport(app, ui);
                let st = TrackStyle { max_rows: shown_rows.max(1), ..TrackStyle::TABLET };
                tracks_styled(app, ui, 0.0, st);
                if app.audio_ui.is_recording() || app.audio_ui.selected.is_some() {
                    ui.add_space(6.0);
                    if !record_bar(app, ui) {
                        clip_bar(app, ui);
                    }
                    ui.add_space(6.0);
                }
                if n_rows > 2 {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), more_row), Sense::click());
                    let label = if app.ui.dock_open { tr("Menos pistas").to_string() } else { tf!("{} pistas más", n_rows - 2) };
                    ui.painter().text(Pos2::new(r.left() + 20.0, r.center().y), Align2::LEFT_CENTER, label, theme::font(13.0, W::Regular), TEXT2);
                    paint(ui.painter(), Rect::from_center_size(Pos2::new(r.left() + 150.0 - 20.0, r.center().y), Vec2::splat(16.0)), if app.ui.dock_open { Icon::Down } else { Icon::Up }, TEXT2);
                    if resp.clicked() {
                        app.ui.dock_open = !app.ui.dock_open;
                    }
                } else {
                    ui.add_space(more_row);
                }
            });
        });
    }
    let focus = app.ui.focus;
    egui::Panel::top("tab_top_room").exact_size(if focus { 16.0 } else { 78.0 }).resizable(false).show_separator_line(false).frame(room).show(ui, |_| {});
    egui::Panel::left("tab_left_room").exact_size(146.0).resizable(false).show_separator_line(false).frame(room).show(ui, |_| {});
    egui::Panel::right("tab_right_room").exact_size(if focus { 20.0 } else { 356.0 }).resizable(false).show_separator_line(false).frame(room).show(ui, |_| {});
    if focus {
        egui::Panel::bottom("tab_bottom_room").exact_size(16.0).resizable(false).show_separator_line(false).frame(room).show(ui, |_| {});
    }
}

fn tablet_transport(app: &mut TrazoApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.set_height(48.0);
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(10.0);
        let layer = app.doc.layer;
        let blocks = tl::blocks(&app.doc.project, layer);
        let shown = app.displayed_frame();
        let prev = blocks.iter().rev().find(|b| b.start < shown && !b.contains(shown)).map(|b| b.start);
        let next = blocks.iter().find(|b| b.start > shown).map(|b| b.start);
        if widgets::icon_button_ex(ui, Icon::ArrowLeft, tr("Dibujo anterior"), prev.is_some(), None).clicked() {
            if let Some(f) = prev {
                app.stop_playback();
                app.doc.set_frame(f);
            }
        }
        play_button(app, ui);
        if widgets::icon_button_ex(ui, Icon::Next, tr("Dibujo siguiente"), next.is_some(), None).clicked() {
            if let Some(f) = next {
                app.stop_playback();
                app.doc.set_frame(f);
            }
        }
        let n = app.doc.project.frames.len();
        let cur = shown + 1;
        let fps = app.doc.project.canvas.fps.max(1) as f32;
        let mut job = egui::text::LayoutJob::default();
        job.append(&format!("{cur:02}"), 4.0, egui::TextFormat { font_id: theme::counter_font(18.0), color: TEXT, ..Default::default() });
        job.append(&tf!(" / {} · {} s", n, crate::i18n::num((cur - 1) as f32 / fps, 2)), 0.0, egui::TextFormat { font_id: theme::font(13.0, W::Regular), color: TEXT2, valign: egui::Align::Max, ..Default::default() });
        ui.label(job);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            let g = ui.painter().layout_no_wrap(tr("Nuevo").to_string(), theme::font(14.0, W::Semi), TEXT);
            let (nr, nresp) = ui.allocate_exact_size(Vec2::new(g.size().x + 44.0, 44.0), Sense::click());
            ui.painter().rect_filled(nr, 12.0, ELEVATED);
            dashed_rect(ui.painter(), nr.shrink(0.75), Stroke::new(1.5, TEXT3));
            paint(ui.painter(), Rect::from_center_size(Pos2::new(nr.left() + 20.0, nr.center().y), Vec2::splat(20.0)), Icon::Plus, TEXT);
            ui.painter().galley(Pos2::new(nr.left() + 34.0, nr.center().y - g.size().y / 2.0), g, TEXT);
            if nresp.on_hover_text(tr("Nuevo dibujo después del actual (N)")).clicked() {
                crate::panels::timeline::add_frame(app);
            }
            if widgets::icon_button(ui, Icon::Mic, tr("Grabar voz")).clicked() {
                app.ui.dock_open = true;
                crate::panels::audio::set_target_track(app, None);
                crate::panels::audio::begin_recording(app, ui.ctx());
            }
            if widgets::toggle_icon(ui, Icon::Loop, app.playback.looping, tr("Bucle")).clicked() {
                app.playback.looping = !app.playback.looping;
            }
            let (cr, cresp) = ui.allocate_exact_size(Vec2::new(24.0, 44.0), Sense::click());
            paint(ui.painter(), Rect::from_center_size(cr.center(), Vec2::splat(16.0)), Icon::Down, TEXT2);
            egui::Popup::menu(&cresp.on_hover_text(tr("Ajustes del papel cebolla"))).show(|ui| crate::panels::phone::frame_menu(app, ui));
            let on = app.onion.enabled;
            if widgets::pill_button(ui, Icon::Onion, tr("Cebolla"), if on { ACCENT } else { TEXT2 }, on).clicked() {
                toggle_onion(app);
            }
        });
    });
}

/// Floating islands of the tablet: title, actions, tools, size, view, inspector.
pub fn tablet(app: &mut TrazoApp, ctx: &egui::Context, canvas: Rect) {
    let screen = ctx.content_rect();
    let focus = app.ui.focus;
    // Title.
    if !focus {
    egui::Area::new(egui::Id::unique("tab_title")).order(Order::Foreground).fixed_pos(screen.min + Vec2::new(20.0, 20.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if widgets::icon_button_ex(ui, Icon::ArrowLeft, tr("Mis animaciones"), true, Some(SURFACE)).clicked() {
                app.go_home(ui.ctx());
            }
            ui.set_min_width(360.0);
            let t = title(app, ui);
            egui::Popup::menu(&t).show(|ui| crate::panels::phone::project_menu(app, ui));
        });
    });
    }
    // Focus, camera and export.
    if !focus {
    egui::Area::new(egui::Id::unique("tab_actions")).order(Order::Foreground).pivot(Align2::RIGHT_TOP).fixed_pos(Pos2::new(screen.right() - 20.0, screen.top() + 24.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if widgets::icon_button_ex(ui, Icon::Focus, tr("Modo enfoque: solo herramientas y lienzo"), true, Some(SURFACE)).clicked() {
                toggle_focus(app);
            }
            for (ic, label, key) in [(Icon::Camera, tr("Cámara"), 0), (Icon::Export, tr("Exportar"), 1)] {
                let g = ui.painter().layout_no_wrap(label.to_string(), theme::font(14.0, W::Semi), TEXT);
                let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 56.0, 44.0), Sense::click());
                ui.painter().rect_filled(r, 12.0, if resp.hovered() { ELEVATED } else { SURFACE });
                paint(ui.painter(), Rect::from_center_size(Pos2::new(r.left() + 24.0, r.center().y), Vec2::splat(20.0)), ic, TEXT);
                ui.painter().galley(Pos2::new(r.left() + 42.0, r.center().y - g.size().y / 2.0), g, TEXT);
                if resp.clicked() {
                    if key == 0 {
                        app.ui.camera.open = true;
                    } else {
                        app.export_ui.open = true;
                    }
                }
            }
        });
    });
    }
    // Tools, history first (left hand).
    let rail_top = if focus { 20.0 } else { 86.0 };
    let rail_h = 468.0f32.min(canvas.bottom() - screen.top() - rail_top - 8.0);
    egui::Area::new(egui::Id::unique("tab_tools")).order(Order::Foreground).fixed_pos(screen.min + Vec2::new(20.0, rail_top)).show(ctx, |ui| {
        Frame::new().fill(SURFACE).corner_radius(20).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).shadow(theme::island_shadow()).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(44.0);
            ui.set_height(rail_h - 12.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.vertical_centered(|ui| {
                let ctx = ui.ctx().clone();
                let can_undo = app.doc.history.can_undo() || app.canvas.select.is_active();
                if widgets::icon_button_ex(ui, Icon::Undo, tr("Deshacer (toca con 2 dedos)"), can_undo, None).clicked() {
                    app.undo(&ctx);
                }
                if widgets::icon_button_ex(ui, Icon::Redo, tr("Rehacer (toca con 3 dedos)"), app.doc.history.can_redo(), None).clicked() {
                    app.redo(&ctx);
                }
                let (r, _) = ui.allocate_exact_size(Vec2::new(44.0, 9.0), Sense::hover());
                ui.painter().hline(r.center().x - 14.0..=r.center().x + 14.0, r.center().y, Stroke::new(1.0, SEPARATOR));
                // As many tools as fit, then "more" (text, selection, hand, magnifier…).
                let fits = ((ui.available_height() - 46.0) / 42.0).floor().max(2.0) as usize;
                for t in &T_TOOLS[..T_TOOLS.len().min(fits - 1)] {
                    tool_slot(app, ui, *t, Vec2::new(44.0, 42.0));
                }
                crate::panels::phone::more_tools_pub(app, ui, Vec2::new(44.0, 42.0));
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
                    let p = ui.painter();
                    p.circle_filled(rect.center(), 18.0, TEXT);
                    p.circle_filled(rect.center(), 16.0, SURFACE);
                    p.circle_filled(rect.center(), 14.0, app.tools.color);
                    if resp.on_hover_text(tr("Color")).clicked() {
                        if app.ui.focus {
                            app.ui.focus_pop = Some(SideTab::Color);
                        } else {
                            app.ui.inspector = SideTab::Color;
                        }
                    }
                });
            });
        });
    });
    // Size and opacity.
    let h = 290.0f32.min(rail_h - 64.0);
    if h > 120.0 {
        size_rail_at(app, ctx, screen.min + Vec2::new(94.0, 150.0), h, Pos2::new(canvas.center().x, canvas.bottom() - 12.0));
    }
    if focus {
        return;
    }
    // View: fit, flip, zoom.
    egui::Area::new(egui::Id::unique("tab_view")).order(Order::Foreground).pivot(Align2::RIGHT_BOTTOM).fixed_pos(Pos2::new(canvas.right() - 12.0, canvas.bottom() - 12.0)).show(ctx, |ui| {
        Frame::new().fill(SURFACE).corner_radius(14).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).shadow(theme::island_shadow()).inner_margin(Margin::symmetric(2, 0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if widgets::icon_button(ui, Icon::Fit, tr("Ajustar a la pantalla")).clicked() {
                    app.canvas.fit();
                }
                if widgets::icon_button_ex(ui, Icon::FlipH, tr("Voltear vista"), true, app.canvas.is_flipped().then_some(ELEVATED)).clicked() {
                    app.canvas.toggle_flip();
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("{:.0} %", app.canvas.zoom() * 100.0)).font(theme::font(13.0, W::Regular)).color(Color32::from_rgb(214, 214, 211)));
                ui.add_space(10.0);
            });
        });
    });
    // Inspector: layers, brush, color.
    let insp_h = 452.0f32.min(canvas.bottom() - screen.top() - 92.0);
    egui::Area::new(egui::Id::unique("tab_inspector")).order(Order::Foreground).pivot(Align2::RIGHT_TOP).fixed_pos(Pos2::new(screen.right() - 20.0, screen.top() + 92.0)).show(ctx, |ui| {
        Frame::new().fill(SURFACE).corner_radius(20).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).shadow(theme::island_shadow()).inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(300.0);
            ui.set_height(insp_h - 20.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            let mut i = match app.ui.inspector {
                SideTab::Layers => 0,
                SideTab::Brushes => 1,
                SideTab::Color => 2,
            };
            if widgets::segmented(ui, &mut i, &[tr("Capas"), tr("Pincel"), tr("Color")], ui.available_width()) {
                app.ui.inspector = [SideTab::Layers, SideTab::Brushes, SideTab::Color][i];
            }
            match app.ui.inspector {
                SideTab::Layers => {
                    let room = ui.available_height() - 50.0;
                    egui::ScrollArea::vertical().id_salt("tab_layers").max_height(room).auto_shrink([false, false]).show(ui, |ui| {
                        crate::panels::layers::set_row_style(ui.ctx(), crate::panels::layers::RowStyle::TABLET);
                        crate::panels::layers::show(app, ui)
                    });
                    if widgets::secondary_button_icon(ui, Some(Icon::Plus), tr("Añadir capa"), 44.0).clicked() {
                        crate::panels::layers::toggle_add(ui.ctx());
                    }
                }
                SideTab::Brushes => {
                    crate::panels::brushes::show(app, ui);
                }
                SideTab::Color => {
                    egui::ScrollArea::vertical().id_salt("tab_color").auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 12.0;
                        crate::panels::toolbar::color_panel(app, ui);
                    });
                }
            }
        });
    });
}

// ---- PC (PC board) ------------------------------------------------------------------------------

const PC_TOOLS: [Option<Tool>; 14] = [
    Some(Tool::Brush),
    Some(Tool::Eraser),
    Some(Tool::Fill),
    Some(Tool::Eyedropper),
    None,
    Some(Tool::Lasso),
    Some(Tool::Select),
    Some(Tool::Wand),
    Some(Tool::Shape),
    Some(Tool::Text),
    None,
    Some(Tool::Hand),
    Some(Tool::Zoom),
    Some(Tool::ZoomOut),
];

fn kbd(ui: &mut Ui, text: &str) {
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::font(10.0, W::Semi), TEXT2);
    let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x + 10.0, 18.0), Sense::hover());
    ui.painter().rect_filled(r, 4.0, ELEVATED);
    ui.painter().galley(Pos2::new(r.left() + 5.0, r.center().y - g.size().y / 2.0), g, TEXT2);
}

/// Panels of the desktop layout: top bar, tools, inspector, timeline, status.
pub fn pc_panels(app: &mut TrazoApp, ui: &mut Ui) {
    let bar = Frame::new().fill(theme::PANEL);
    let focus = app.ui.focus;
    // Top bar.
    if !focus {
    egui::Panel::top("pc_top").exact_size(44.0).resizable(false).show_separator_line(false).frame(bar.inner_margin(Margin::symmetric(10, 0))).show(ui, |ui| {
        let r = ui.max_rect();
        ui.painter().hline(r.x_range(), r.bottom() + 0.5, Stroke::new(1.0, Color32::from_white_alpha(10)));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let ctx = ui.ctx().clone();
            // Logo: home.
            let (lr, lresp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
            ui.painter().rect_filled(lr, 8.0, theme::PRIMARY);
            let pts: Vec<Pos2> = [(7.0, 19.0), (11.0, 13.0), (14.5, 17.0), (19.0, 9.0), (23.0, 15.0)].iter().map(|(x, y)| lr.min + Vec2::new(*x, *y)).collect();
            ui.painter().add(Shape::line(pts, Stroke::new(2.0, theme::ON_ACCENT)));
            if lresp.on_hover_text(tr("Mis animaciones")).clicked() {
                app.go_home(&ctx);
            }
            if ui.add(egui::Button::new(egui::RichText::new(tr("Mis animaciones")).font(theme::font(13.0, W::Regular)).color(TEXT2)).frame(false)).clicked() {
                app.go_home(&ctx);
            }
            ui.label(egui::RichText::new("/").font(theme::font(13.0, W::Regular)).color(TEXT3));
            let name = egui::RichText::new(app.doc.project.name.clone()).font(theme::font(13.0, W::Semi)).color(TEXT);
            let nr = ui.add(egui::Button::new(name).frame(false));
            paint(ui.painter(), Rect::from_center_size(Pos2::new(nr.rect.right() + 4.0, nr.rect.center().y), Vec2::splat(13.0)), Icon::Down, TEXT2);
            ui.add_space(10.0);
            egui::Popup::menu(&nr).show(|ui| crate::panels::phone::project_menu(app, ui));
            let saved = !app.doc.is_modified();
            let (dr, _) = ui.allocate_exact_size(Vec2::new(8.0, 20.0), Sense::hover());
            ui.painter().circle_filled(dr.center(), 3.0, if saved { theme::SAVED } else { TEXT3 });
            ui.label(egui::RichText::new(if saved { tr("Guardado") } else { tr("Sin guardar") }).font(theme::font(12.0, W::Regular)).color(TEXT2));
            // Right: camera, export.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (ic, label, key) in [(Icon::Export, tr("Exportar"), 1), (Icon::Camera, tr("Cámara"), 0)] {
                    let g = ui.painter().layout_no_wrap(label.to_string(), theme::font(13.0, W::Semi), TEXT);
                    let extra = if key == 1 { 50.0 } else { 0.0 };
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 44.0 + extra, 32.0), Sense::click());
                    ui.painter().rect_filled(r, 9.0, if resp.hovered() { ELEVATED } else if key == 1 { SURFACE } else { theme::BG });
                    paint(ui.painter(), Rect::from_center_size(Pos2::new(r.left() + 18.0, r.center().y), Vec2::splat(17.0)), ic, TEXT);
                    ui.painter().galley(Pos2::new(r.left() + 34.0, r.center().y - g.size().y / 2.0), g, TEXT);
                    if key == 1 {
                        let kg = ui.painter().layout_no_wrap("Ctrl E".to_string(), theme::font(10.0, W::Semi), TEXT2);
                        let kr = Rect::from_min_size(Pos2::new(r.right() - kg.size().x - 18.0, r.center().y - 9.0), Vec2::new(kg.size().x + 10.0, 18.0));
                        ui.painter().rect_filled(kr, 4.0, ELEVATED);
                        ui.painter().galley(Pos2::new(kr.left() + 5.0, kr.center().y - kg.size().y / 2.0), kg, TEXT2);
                    }
                    let tip = if key == 1 { tr("Exportar a MP4, GIF, PNG, AVI o WAV") } else { tr("Cámara: encuadre y movimiento") };
                    if resp.on_hover_text(tip).clicked() {
                        if key == 1 {
                            app.export_ui.open = true;
                        } else {
                            app.ui.camera.open = true;
                        }
                    }
                }
                if widgets::icon_button(ui, Icon::Focus, tr("Modo enfoque: solo herramientas y lienzo (Tab)")).clicked() {
                    toggle_focus(app);
                }
                // Undo / redo in the middle of the bar.
                let full = ui.ctx().content_rect();
                let pill = Rect::from_center_size(Pos2::new(full.center().x, r.center().y), Vec2::new(68.0, 32.0));
                ui.painter().rect_filled(pill, 9.0, SURFACE);
                let ur = Rect::from_min_size(pill.min, Vec2::new(34.0, 32.0));
                let rr = Rect::from_min_size(pill.min + Vec2::new(34.0, 0.0), Vec2::new(34.0, 32.0));
                let can_undo = app.doc.history.can_undo() || app.canvas.select.is_active();
                let can_redo = app.doc.history.can_redo();
                let u = ui.interact(ur, ui.id().with("pc_undo"), Sense::click());
                let rd = ui.interact(rr, ui.id().with("pc_redo"), Sense::click());
                paint(ui.painter(), Rect::from_center_size(ur.center(), Vec2::splat(18.0)), Icon::Undo, if can_undo { TEXT } else { TEXT3 });
                paint(ui.painter(), Rect::from_center_size(rr.center(), Vec2::splat(18.0)), Icon::Redo, if can_redo { TEXT } else { TEXT3 });
                if u.on_hover_text(tr("Deshacer (Ctrl+Z)")).clicked() && can_undo {
                    app.undo(&ctx);
                }
                if rd.on_hover_text(tr("Rehacer (Ctrl+Y)")).clicked() && can_redo {
                    app.redo(&ctx);
                }
            });
        });
    });
    }
    // Status bar.
    if !focus {
    egui::Panel::bottom("pc_status").exact_size(24.0).resizable(false).show_separator_line(false).frame(bar.inner_margin(Margin::symmetric(12, 0))).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            kbd(ui, "1");
            ui.label(egui::RichText::new("–").font(theme::font(11.0, W::Regular)).color(TEXT3));
            kbd(ui, "9");
            ui.label(egui::RichText::new(tr("fija la duración")).font(theme::font(11.0, W::Regular)).color(TEXT2));
            ui.add_space(12.0);
            ui.label(egui::RichText::new(tr("Arrastra la flecha blanca de un dibujo para alargarlo")).font(theme::font(11.0, W::Regular)).color(TEXT2));
            ui.add_space(12.0);
            kbd(ui, "Alt");
            ui.label(egui::RichText::new(tr("+ clic: tomar color")).font(theme::font(11.0, W::Regular)).color(TEXT2));
            ui.add_space(12.0);
            kbd(ui, tr("Espacio"));
            ui.label(egui::RichText::new(tr("+ arrastrar: mover")).font(theme::font(11.0, W::Regular)).color(TEXT2));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let c = &app.doc.project.canvas;
                ui.label(egui::RichText::new(format!("{} × {}", c.width, c.height)).font(theme::font(11.0, W::Regular)).color(TEXT2));
            });
        });
    });
    }
    // Timeline.
    if !focus {
    egui::Panel::bottom("pc_timeline").default_size(292.0).min_size(240.0).max_size(520.0).resizable(true).show_separator_line(false)
        .frame(Frame::new().fill(theme::PANEL))
        .show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, Color32::from_white_alpha(12)));
            crate::panels::timeline::commit_selection_on_click(app, ui);
            ui.spacing_mut().item_spacing.y = 0.0;
            pc_transport(app, ui);
            let st = TrackStyle { label_w: 200.0, row_h: 30.0, ppf: app.ui.tracks.ppf.clamp(12.0, 64.0), ruler_h: 26.0, max_rows: 12, hint: false, eye: true, pc: true };
            let bar = if app.audio_ui.is_recording() || app.audio_ui.selected.is_some() { 64.0 } else { 0.0 };
            let rows = ((ui.available_height() - st.ruler_h - bar) / st.row_h).floor().max(1.0) as usize;
            tracks_styled(app, ui, 0.0, TrackStyle { max_rows: rows, ..st });
            if bar > 0.0 {
                ui.add_space(4.0);
                ui.allocate_ui(Vec2::new(ui.available_width().min(760.0), 56.0), |ui| {
                    if !record_bar(app, ui) {
                        clip_bar(app, ui);
                    }
                });
            }
        });
    }
    // Tools.
    egui::Panel::left("pc_tools").exact_size(52.0).resizable(false).show_separator_line(false).frame(bar.inner_margin(Margin::symmetric(4, 8))).show(ui, |ui| {
        let r = ui.max_rect();
        ui.painter().vline(r.right() + 4.0, r.y_range(), Stroke::new(1.0, Color32::from_white_alpha(10)));
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.vertical_centered(|ui| {
            for t in PC_TOOLS {
                match t {
                    Some(t) => {
                        let sel = app.tools.tool == t;
                        let resp = widgets::tool_button(ui, t.icon(), sel, Vec2::splat(40.0), t.label());
                        if resp.clicked() {
                            app.tools.tool = t;
                        }
                        if sel && crate::panels::phone::has_options(t) {
                            egui::Popup::context_menu(&resp).show(|ui| {
                                ui.set_min_width(260.0);
                                crate::panels::phone::tool_options_pub(app, ui);
                            });
                        }
                    }
                    None => {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(40.0, 9.0), Sense::hover());
                        ui.painter().hline(r.center().x - 12.0..=r.center().x + 12.0, r.center().y, Stroke::new(1.0, SEPARATOR));
                    }
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
                let bg = app.doc.project.canvas.background.to_rgba8();
                let p = ui.painter();
                p.rect_filled(Rect::from_min_size(r.min + Vec2::new(14.0, 14.0), Vec2::splat(20.0)), 5.0, Color32::from_rgb(bg[0], bg[1], bg[2]));
                p.rect_filled(Rect::from_min_size(r.min + Vec2::new(4.0, 4.0), Vec2::splat(22.0)), 6.0, app.tools.color);
                p.rect_stroke(Rect::from_min_size(r.min + Vec2::new(4.0, 4.0), Vec2::splat(22.0)), 6.0, Stroke::new(1.0, Color32::from_white_alpha(40)), egui::StrokeKind::Inside);
                if resp.on_hover_text(tr("Color")).clicked() {
                    if app.ui.focus {
                        app.ui.focus_pop = Some(SideTab::Color);
                    } else {
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("pc_color_open"), true));
                    }
                }
            });
        });
    });
    // Inspector.
    if !focus {
    egui::Panel::right("pc_inspector").exact_size(286.0).resizable(false).show_separator_line(false).frame(bar.inner_margin(Margin::symmetric(10, 6))).show(ui, |ui| {
        let r = ui.max_rect();
        ui.painter().vline(r.left() - 10.0, r.y_range(), Stroke::new(1.0, Color32::from_white_alpha(10)));
        egui::ScrollArea::vertical().id_salt("pc_inspector").auto_shrink([false, false]).show(ui, |ui| pc_inspector(app, ui));
    });
    }
}

fn section(ui: &mut Ui, id: &str, title: &str, default_open: bool, right: impl FnOnce(&mut Ui)) -> bool {
    let key = egui::Id::unique(("pc_section", id));
    let mut open = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(default_open);
    if id == "color" && ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::unique("pc_color_open"))).is_some() {
        open = true;
    }
    ui.horizontal(|ui| {
        ui.set_height(36.0);
        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width() * 0.55, 36.0), Sense::click());
        paint(ui.painter(), Rect::from_center_size(Pos2::new(r.left() + 7.0, r.center().y), Vec2::splat(13.0)), if open { Icon::Down } else { Icon::Next }, TEXT2);
        ui.painter().text(Pos2::new(r.left() + 20.0, r.center().y), Align2::LEFT_CENTER, title, theme::font(13.0, W::Semi), TEXT);
        if resp.clicked() {
            open = !open;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.data_mut(|d| d.insert_temp(key, open));
    open
}

/// A value you drag left/right (or scroll) to change, label on the left.
fn field(ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, speed: f32, fmt: impl Fn(f32) -> String, w: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click_and_drag());
    if resp.dragged() {
        *value = (*value + resp.drag_delta().x * speed).clamp(*range.start(), *range.end());
    }
    if resp.hovered() {
        let sd = ui.input(|i| i.smooth_scroll_delta.y);
        if sd != 0.0 {
            *value = (*value + sd.signum() * speed * 4.0).clamp(*range.start(), *range.end());
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let p = ui.painter();
    p.rect_filled(r, 7.0, if resp.dragged() { ELEVATED } else { SURFACE });
    p.text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, label, theme::font(12.0, W::Regular), TEXT2);
    p.text(Pos2::new(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, fmt(*value), theme::font(12.0, W::Medium), TEXT);
    resp
}

fn pc_inspector(app: &mut TrazoApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
    let full_w = ui.available_width();
    let half = (full_w - 4.0) / 2.0;
    // Brush.
    let brush_name = tr(&app.tools.active_brush().name).to_string();
    let mut list = false;
    if section(ui, "brush", tr("Pincel"), true, |ui| {
        list = ui.add(egui::Button::new(egui::RichText::new(&brush_name).font(theme::font(12.0, W::Regular)).color(TEXT2)).frame(false)).on_hover_text(tr("Elegir pincel")).clicked();
    }) {
        let i = app.tools.active_index();
        ui.horizontal(|ui| {
            let tw = &mut app.tools.tweaks[i];
            field(ui, tr("Tamaño"), &mut tw.size, 1.0..=300.0, 0.5, |v| format!("{v:.0} px"), half);
            let mut o = tw.opacity * 100.0;
            if field(ui, tr("Opacidad"), &mut o, 2.0..=100.0, 0.5, |v| format!("{v:.0} %"), half).changed() || o != tw.opacity * 100.0 {
                tw.opacity = o / 100.0;
            }
        });
        ui.horizontal(|ui| {
            let mut s = app.tools.stabilizer * 100.0;
            field(ui, tr("Estabilizador"), &mut s, 0.0..=95.0, 0.5, |v| (if v < 12.0 { tr("Nada") } else if v < 37.0 { tr("Suave") } else if v < 65.0 { tr("Medio") } else { tr("Fuerte") }).to_string(), half)
                .on_hover_text(tr("Arrastra a los lados. Más fuerte = líneas más limpias, pero el trazo va detrás del lápiz"));
            app.tools.stabilizer = s / 100.0;
            field(ui, tr("Presión"), &mut app.tools.pressure_gamma, 0.3..=3.0, 0.01, |v| (if v < 0.8 { tr("Suave") } else if v > 1.25 { tr("Firme") } else { tr("Normal") }).to_string(), half);
        });
    }
    let id = egui::Id::unique("pc_brush_list");
    if list {
        ui.data_mut(|d| d.insert_temp(id, !d.get_temp::<bool>(id).unwrap_or(false)));
    }
    if ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        egui::Frame::new().fill(SURFACE).corner_radius(10).inner_margin(Margin::same(6)).show(ui, |ui| {
            ui.set_max_height(360.0);
            crate::panels::brushes::show(app, ui);
        });
    }
    hair(ui);
    // Color.
    let swatch: Vec<Color32> = if app.tools.user_palette.is_empty() { app.tools.recent_colors.iter().take(5).copied().collect() } else { app.tools.user_palette.iter().take(5).copied().collect() };
    let current = app.tools.color;
    let mut pick = None;
    if section(ui, "color", tr("Color"), false, |ui| {
        for c in swatch.iter().rev() {
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
            ui.painter().rect_filled(r, 3.0, *c);
            ui.painter().rect_stroke(r, 3.0, Stroke::new(if *c == current { 1.5 } else { 1.0 }, if *c == current { TEXT } else { Color32::from_white_alpha(40) }), egui::StrokeKind::Inside);
            if resp.clicked() {
                pick = Some(*c);
            }
        }
    }) {
        ui.spacing_mut().item_spacing.y = 10.0;
        crate::panels::toolbar::color_panel(app, ui);
        ui.spacing_mut().item_spacing.y = 4.0;
    }
    if let Some(c) = pick {
        app.tools.set_color(c);
    }
    hair(ui);
    // Current layer.
    let li = app.doc.layer_index();
    let lname = crate::i18n::layer_name(&app.doc.project.layers[li].props.name).into_owned();
    if section(ui, "layer", &tf!("Capa «{}»", lname), true, |_| {}) {
        let doc = &mut app.doc;
        let i = doc.layer_index();
        let id = doc.project.layers[i].id;
        let mut props = doc.project.layers[i].props.clone();
        let (mut changed, mut commit) = (false, false);
        ui.horizontal(|ui| {
            let mut o = props.opacity * 100.0;
            let r = field(ui, tr("Opacidad"), &mut o, 0.0..=100.0, 0.5, |v| format!("{v:.0} %"), half);
            if (o / 100.0 - props.opacity).abs() > 1e-4 {
                props.opacity = o / 100.0;
                changed = true;
            }
            commit |= r.drag_stopped();
            egui::ComboBox::from_id_salt("pc_blend").width(half - 10.0).selected_text(tr(props.blend.label())).show_ui(ui, |ui| {
                for m in trazo_core::BlendMode::ALL {
                    if ui.selectable_value(&mut props.blend, m, tr(m.label())).changed() {
                        changed = true;
                        commit = true;
                    }
                }
            });
        });
        if widgets::switch_row(ui, tr("Pintar solo sobre lo dibujado"), &mut props.alpha_lock) {
            changed = true;
            commit = true;
        }
        if widgets::switch_row(ui, tr("Recortar a la capa de abajo"), &mut props.clip) {
            changed = true;
            commit = true;
        }
        crate::panels::layers::finish_props(ui, doc, i, id, props, changed, commit);
    }
    hair(ui);
    // Layers.
    let mut add = false;
    let mut folder = false;
    if section(ui, "layers", tr("Capas"), true, |ui| {
        folder = widgets::icon_button(ui, Icon::Open, tr("Nueva carpeta")).clicked();
        add = widgets::icon_button(ui, Icon::Plus, tr("Nueva capa")).clicked();
    }) {
        let n = app.doc.project.layers.len();
        let mut select = None;
        for li in (0..n).rev() {
            let layer = app.doc.project.layers[li].clone();
            let sel = layer.id == app.doc.layer;
            let (r, resp) = ui.allocate_exact_size(Vec2::new(full_w, 40.0), Sense::click());
            let p = ui.painter().clone();
            if sel {
                p.rect_filled(r, 8.0, SURFACE);
            } else if resp.hovered() {
                p.rect_filled(r, 8.0, Color32::from_rgb(24, 24, 26));
            }
            let th = Rect::from_min_size(Pos2::new(r.left() + 6.0, r.center().y - 16.0), Vec2::new(26.0, 32.0));
            crate::panels::layers::layer_thumb(&p, th, &layer, &app.doc);
            if sel {
                p.rect_stroke(th, 5.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Outside);
            }
            let name = crate::i18n::layer_name(&layer.props.name).into_owned();
            let sub = if layer.props.shared { tr("fondo").to_string() } else if layer.props.guide { tr("no sale en el video").to_string() } else if layer.props.blend != trazo_core::BlendMode::Normal { tr(layer.props.blend.label()).to_string() } else { String::new() };
            let dim = !layer.props.visible;
            let mut job = egui::text::LayoutJob::default();
            job.append(&name, 0.0, egui::TextFormat { font_id: theme::font(13.0, if sel { W::Semi } else { W::Medium }), color: if dim { TEXT2 } else { TEXT }, ..Default::default() });
            if !sub.is_empty() {
                job.append(&format!(" {sub}"), 0.0, egui::TextFormat { font_id: theme::font(11.0, W::Regular), color: TEXT2, ..Default::default() });
            }
            job.wrap = egui::text::TextWrapping { max_width: r.width() - 90.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
            let g = ui.painter().layout_job(job);
            p.galley(Pos2::new(th.right() + 10.0, r.center().y - g.size().y / 2.0), g, TEXT);
            p.text(Pos2::new(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, format!("{:.0} %", layer.props.opacity * 100.0), theme::font(11.0, W::Regular), TEXT2);
            if resp.clicked() || resp.secondary_clicked() {
                select = Some(layer.id);
            }
            egui::Popup::context_menu(&resp).width(300.0).show(|ui| crate::panels::layers::layer_menu(ui, &mut app.doc, layer.id));
        }
        if let Some(id) = select {
            app.doc.layer = id;
        }
    }
    if add {
        crate::panels::layers::add_drawing_layer(&mut app.doc);
    }
    if folder {
        crate::panels::layers::add_folder(&mut app.doc);
    }
}

fn hair(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, Color32::from_white_alpha(10)));
}

fn pc_transport(app: &mut TrazoApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.set_height(48.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(14.0);
        let ctx = ui.ctx().clone();
        let layer = app.doc.layer;
        let blocks = tl::blocks(&app.doc.project, layer);
        let shown = app.displayed_frame();
        let n = app.doc.project.frames.len();
        let prev = blocks.iter().rev().find(|b| b.start < shown && !b.contains(shown)).map(|b| b.start);
        let next = blocks.iter().find(|b| b.start > shown).map(|b| b.start);
        let mut go = None;
        if widgets::icon_button_ex(ui, Icon::First, tr("Al principio (Inicio)"), shown > 0, None).clicked() {
            go = Some(0);
        }
        if widgets::icon_button_ex(ui, Icon::ArrowLeft, tr("Dibujo anterior (,)"), prev.is_some(), None).clicked() {
            go = prev;
        }
        let (pr, presp) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
        ui.painter().circle_filled(pr.center(), 16.0, theme::PRIMARY);
        paint(ui.painter(), Rect::from_center_size(pr.center() + Vec2::new(if app.playback.playing { 0.0 } else { 1.0 }, 0.0), Vec2::splat(18.0)), if app.playback.playing { Icon::Pause } else { Icon::Play }, theme::ON_ACCENT);
        if presp.on_hover_text(tr("Reproducir / pausar (Enter)")).clicked() {
            app.toggle_play(&ctx);
        }
        if widgets::icon_button_ex(ui, Icon::Next, tr("Dibujo siguiente (.)"), next.is_some(), None).clicked() {
            go = next;
        }
        if widgets::icon_button_ex(ui, Icon::Last, tr("Al final (Fin)"), shown + 1 < n, None).clicked() {
            go = Some(n.saturating_sub(1));
        }
        if let Some(f) = go {
            app.stop_playback();
            app.doc.set_frame(f);
        }
        ui.add_space(14.0);
        let fps = app.doc.project.canvas.fps.max(1);
        let mut job = egui::text::LayoutJob::default();
        job.append(&format!("{:02}", shown + 1), 0.0, egui::TextFormat { font_id: theme::counter_font(15.0), color: TEXT, ..Default::default() });
        job.append(&tf!(" / {} · {} s · {} fps", n, crate::i18n::num(shown as f32 / fps as f32, 2), fps), 0.0, egui::TextFormat { font_id: theme::font(12.0, W::Regular), color: TEXT2, valign: egui::Align::Max, ..Default::default() });
        ui.label(job);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(14.0);
            // Zoom: thin rail, small solid knob, magnifier.
            let (zr, zresp) = ui.allocate_exact_size(Vec2::new(110.0, 28.0), Sense::click_and_drag());
            let t = (app.ui.tracks.ppf.clamp(12.0, 64.0) - 12.0) / 52.0;
            if let Some(pt) = zresp.interact_pointer_pos() {
                if zresp.dragged() || zresp.clicked() {
                    app.ui.tracks.ppf = 12.0 + 52.0 * ((pt.x - zr.left() - 6.0) / (zr.width() - 12.0)).clamp(0.0, 1.0);
                }
            }
            let rail = Rect::from_center_size(zr.center(), Vec2::new(zr.width() - 12.0, 3.0));
            ui.painter().rect_filled(rail, 2.0, SEPARATOR);
            let kx = rail.left() + t * rail.width();
            ui.painter().rect_filled(Rect::from_min_max(rail.min, Pos2::new(kx, rail.bottom())), 2.0, TEXT2);
            ui.painter().circle_filled(Pos2::new(kx, zr.center().y), 6.0, TEXT);
            zresp.on_hover_text(tr("Acercar la línea de tiempo"));
            let (mr, _) = ui.allocate_exact_size(Vec2::new(18.0, 28.0), Sense::hover());
            paint(ui.painter(), Rect::from_center_size(mr.center(), Vec2::splat(14.0)), Icon::Search, TEXT2);
            ui.add_space(8.0);
            let (sr, _) = ui.allocate_exact_size(Vec2::new(9.0, 24.0), Sense::hover());
            ui.painter().vline(sr.center().x, sr.y_range(), Stroke::new(1.0, SEPARATOR));
            if widgets::icon_button(ui, Icon::Mic, tr("Grabar voz")).clicked() {
                crate::panels::audio::set_target_track(app, None);
                crate::panels::audio::begin_recording(app, ui.ctx());
            }
            if widgets::toggle_icon(ui, Icon::Loop, app.playback.looping, tr("Repetir en bucle")).clicked() {
                app.playback.looping = !app.playback.looping;
            }
            let (cr, cresp) = ui.allocate_exact_size(Vec2::new(20.0, 36.0), Sense::click());
            paint(ui.painter(), Rect::from_center_size(cr.center(), Vec2::splat(14.0)), Icon::Down, TEXT2);
            egui::Popup::menu(&cresp.on_hover_text(tr("Velocidad y papel cebolla"))).show(|ui| crate::panels::phone::frame_menu(app, ui));
            if widgets::toggle_icon(ui, Icon::Onion, app.onion.enabled, tr("Papel cebolla (O)")).clicked() {
                toggle_onion(app);
            }
        });
    });
}

/// View controls over the canvas: rotate, flip, fit, zoom.
pub fn pc_view(app: &mut TrazoApp, ctx: &egui::Context, canvas: Rect) {
    egui::Area::new(egui::Id::unique("pc_view")).order(Order::Foreground).pivot(Align2::RIGHT_BOTTOM).fixed_pos(Pos2::new(canvas.right() - 12.0, canvas.bottom() - 12.0)).show(ctx, |ui| {
        Frame::new().fill(SURFACE).corner_radius(12).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).inner_margin(Margin::symmetric(4, 0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if widgets::icon_button(ui, Icon::RotateRight, tr("Girar lienzo")).clicked() {
                    app.canvas.rotate_by(15.0);
                }
                if widgets::icon_button_ex(ui, Icon::FlipH, tr("Voltear vista"), true, app.canvas.is_flipped().then_some(ELEVATED)).clicked() {
                    app.canvas.toggle_flip();
                }
                if widgets::icon_button(ui, Icon::Fit, tr("Ajustar a la pantalla")).clicked() {
                    app.canvas.fit();
                }
                let z = ui.add(egui::Button::new(egui::RichText::new(format!("{:.0} %", app.canvas.zoom() * 100.0)).font(theme::font(12.0, W::Medium)).color(TEXT)).frame(false).min_size(Vec2::new(52.0, 44.0)));
                let (cr, _) = ui.allocate_exact_size(Vec2::new(16.0, 44.0), Sense::hover());
                paint(ui.painter(), Rect::from_center_size(cr.center(), Vec2::splat(12.0)), Icon::Down, TEXT2);
                ui.add_space(6.0);
                egui::Popup::menu(&z).show(|ui| {
                    for (label, v) in [("25 %", 0.25), ("50 %", 0.5), ("100 %", 1.0), ("200 %", 2.0)] {
                        if ui.add(egui::Button::new(label).min_size(Vec2::new(120.0, 32.0))).clicked() {
                            app.canvas.set_zoom(v);
                            ui.close();
                        }
                    }
                    if ui.add(egui::Button::new(tr("Ajustar")).min_size(Vec2::new(120.0, 32.0))).clicked() {
                        app.canvas.fit();
                        ui.close();
                    }
                });
            });
        });
    });
}

/// Landscape action bar: icon and label on one line, 52 tall.
fn action_bar_inline(app: &mut TrazoApp, ui: &mut Ui) {
    if record_bar(app, ui) || clip_bar(app, ui) {
        return;
    }
    let ctx = ui.ctx().clone();
    let layer = app.doc.layer;
    let Some(b) = crate::panels::timeline::selected_block(app) else {
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::hover());
        ui.painter().rect_filled(r, 14.0, Color32::from_rgb(31, 31, 34));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, tr("Nada aquí: toca «Nuevo» o dibuja para crear un dibujo"), theme::font(12.0, W::Regular), TEXT2);
        return;
    };
    let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::hover());
    ui.painter().rect_filled(bar, 14.0, Color32::from_rgb(31, 31, 34));
    let inner = bar.shrink2(Vec2::new(6.0, 0.0));
    let unit = inner.width() / 4.0;
    let slot = |i: f32, w: f32| Rect::from_min_size(Pos2::new(inner.left() + i * unit, inner.top() + 6.0), Vec2::new(w * unit, 40.0));
    let mut act = None;
    let mut new_len = None;
    let item = |ui: &mut Ui, r: Rect, icon: Icon, label: Option<&str>, id: &str| -> egui::Response {
        let resp = ui.interact(r, ui.id().with(("bar_i", id)), Sense::click());
        if resp.hovered() || resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(r, 12.0, SEPARATOR);
        }
        match label {
            Some(l) => {
                let g = ui.painter().layout_no_wrap(l.to_string(), theme::font(12.0, W::Semi), TEXT);
                let x = r.center().x - (g.size().x + 26.0) / 2.0;
                paint(ui.painter(), Rect::from_center_size(Pos2::new(x + 10.0, r.center().y), Vec2::splat(20.0)), icon, TEXT);
                ui.painter().galley(Pos2::new(x + 26.0, r.center().y - g.size().y / 2.0), g, TEXT);
            }
            None => paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(20.0)), icon, TEXT),
        }
        resp
    };
    if item(ui, slot(0.0, 1.0), Icon::Duplicate, Some(tr("Duplicar")), "dup").clicked() {
        act = Some("dup");
    }
    let sr = slot(1.0, 1.5).shrink2(Vec2::new(4.0, 0.0));
    let p = ui.painter();
    p.rect_filled(sr, 20.0, Color32::from_rgb(42, 42, 46));
    let minus = Rect::from_min_size(sr.min, Vec2::new(40.0, 40.0));
    let plus = Rect::from_min_size(Pos2::new(sr.right() - 40.0, sr.top()), Vec2::new(40.0, 40.0));
    p.text(minus.center(), Align2::CENTER_CENTER, "−", theme::font(18.0, W::Regular), if b.len > 1 { TEXT } else { TEXT3 });
    p.text(plus.center(), Align2::CENTER_CENTER, "+", theme::font(18.0, W::Regular), TEXT);
    p.text(sr.center(), Align2::CENTER_CENTER, tf!("Duración {}", b.len), theme::font(12.0, W::Semi), TEXT);
    if ui.interact(minus, ui.id().with("bi_minus"), Sense::click()).on_hover_text(tr("Dura menos")).clicked() && b.len > 1 {
        new_len = Some(b.len - 1);
    }
    if ui.interact(plus, ui.id().with("bi_plus"), Sense::click()).on_hover_text(tr("Dura más")).clicked() {
        new_len = Some(b.len + 1);
    }
    if item(ui, slot(2.5, 1.0), Icon::Tween, Some(tr("Intermedios")), "tween").clicked() {
        act = Some("tween");
    }
    let more = item(ui, slot(3.5, 0.5), Icon::More, None, "more").on_hover_text(tr("Más"));
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

// ---- sound: clip actions and recording, in the timeline itself ------------------------------

/// While recording (or about to): countdown, time, level and stop.
pub(crate) fn record_bar(app: &mut TrazoApp, ui: &mut Ui) -> bool {
    use crate::panels::audio::RecordState;
    if !app.audio_ui.is_recording() {
        return false;
    }
    let ctx = ui.ctx().clone();
    let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::hover());
    let bar = bar.shrink2(Vec2::new(10.0, 0.0));
    ui.painter().rect_filled(bar, 18.0, Color32::from_rgb(31, 31, 34));
    let now = ctx.input(|i| i.time);
    let (label, show_level, recording) = match app.audio_ui.record {
        RecordState::WaitingPermission { .. } => (tr("Acepta el permiso del micrófono…").to_string(), false, false),
        RecordState::Countdown { until, .. } => (tf!("Grabando en {}…", format!("{:.0}", (until - now).ceil().max(1.0))), true, false),
        RecordState::Recording { capturing, .. } => {
            if capturing {
                (tf!("Grabando · {} s", crate::i18n::num(crate::panels::audio::recorded_seconds(app), 1)), true, true)
            } else {
                (tr("Preparando…").to_string(), true, false)
            }
        }
        RecordState::Idle => return false,
    };
    let p = ui.painter();
    let dot = Pos2::new(bar.left() + 22.0, bar.center().y);
    let pulse = if recording { 0.6 + 0.4 * ((now * 4.0).sin() as f32 * 0.5 + 0.5) } else { 0.5 };
    p.circle_filled(dot, 7.0, theme::RECORDING.gamma_multiply(pulse));
    p.text(Pos2::new(bar.left() + 38.0, bar.center().y), Align2::LEFT_CENTER, label, theme::font(14.0, W::Semi), TEXT);
    // Stop (or cancel) on the right.
    let stop = Rect::from_center_size(Pos2::new(bar.right() - 52.0, bar.center().y), Vec2::new(88.0, 40.0));
    let sresp = ui.interact(stop, ui.id().with("rec_stop"), Sense::click());
    ui.painter().rect_filled(stop, 20.0, if recording { theme::PRIMARY } else { ELEVATED });
    ui.painter().text(stop.center(), Align2::CENTER_CENTER, if recording { tr("Detener") } else { tr("Cancelar") }, theme::font(14.0, W::Bold), if recording { theme::ON_ACCENT } else { TEXT });
    if show_level {
        let lv = crate::panels::audio::input_level(app).clamp(0.0, 1.0).sqrt();
        let meter = Rect::from_min_max(Pos2::new(stop.left() - 100.0, bar.center().y - 3.0), Pos2::new(stop.left() - 14.0, bar.center().y + 3.0));
        ui.painter().rect_filled(meter, 3.0, SEPARATOR);
        ui.painter().rect_filled(Rect::from_min_size(meter.min, Vec2::new(meter.width() * lv, meter.height())), 3.0, if lv > 0.95 { theme::DANGER } else { theme::SAVED });
    }
    if sresp.clicked() {
        if recording {
            crate::panels::audio::finish_recording(app, &ctx);
        } else {
            crate::panels::audio::cancel_recording(app);
        }
    }
    ctx.request_repaint();
    true
}

/// The selected sound clip: volume, split, mute, delete and more.
pub(crate) fn clip_bar(app: &mut TrazoApp, ui: &mut Ui) -> bool {
    let Some(id) = app.audio_ui.selected else { return false };
    let Some((_, clip)) = app.doc.project.audio.find_clip(id).map(|(t, c)| (t, c.clone())) else {
        app.audio_ui.selected = None;
        return false;
    };
    let ctx = ui.ctx().clone();
    let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::hover());
    let bar = bar.shrink2(Vec2::new(10.0, 0.0));
    ui.painter().rect_filled(bar, 18.0, Color32::from_rgb(31, 31, 34));
    let inner = bar.shrink2(Vec2::new(6.0, 0.0));
    let unit = inner.width() / 5.0;
    let slot = |i: f32, w: f32| Rect::from_min_size(Pos2::new(inner.left() + i * unit, inner.top() + 6.0), Vec2::new(w * unit, 44.0));
    let item = |ui: &mut Ui, r: Rect, icon: Icon, label: &str, on: bool, id: &str| -> egui::Response {
        let resp = ui.interact(r, ui.id().with(("clip_bar", id)), if on { Sense::click() } else { Sense::hover() });
        if on && (resp.hovered() || resp.is_pointer_button_down_on()) {
            ui.painter().rect_filled(r, 12.0, SEPARATOR);
        }
        let col = if on { TEXT } else { TEXT3 };
        paint(ui.painter(), Rect::from_center_size(Pos2::new(r.center().x, r.top() + 15.0), Vec2::splat(19.0)), icon, col);
        ui.painter().text(Pos2::new(r.center().x, r.top() + 35.0), Align2::CENTER_CENTER, label, theme::font(11.0, W::Medium), col);
        resp
    };
    // Volume stepper.
    let sr = slot(0.0, 1.6).shrink2(Vec2::new(2.0, 2.0));
    let p = ui.painter();
    p.rect_filled(sr, 20.0, Color32::from_rgb(42, 42, 46));
    let minus = Rect::from_min_size(sr.min, Vec2::new(36.0, sr.height()));
    let plus = Rect::from_min_size(Pos2::new(sr.right() - 36.0, sr.top()), Vec2::new(36.0, sr.height()));
    p.text(minus.center(), Align2::CENTER_CENTER, "−", theme::font(18.0, W::Regular), TEXT);
    p.text(plus.center(), Align2::CENTER_CENTER, "+", theme::font(18.0, W::Regular), TEXT);
    p.text(Pos2::new(sr.center().x, sr.center().y - 7.0), Align2::CENTER_CENTER, format!("{:.0} %", clip.gain * 100.0), theme::font(13.0, W::Semi), TEXT);
    p.text(Pos2::new(sr.center().x, sr.center().y + 9.0), Align2::CENTER_CENTER, tr("Volumen"), theme::font(10.0, W::Regular), TEXT2);
    let step = |g: f32, up: bool| -> f32 { (((g * 10.0).round() + if up { 1.0 } else { -1.0 }) / 10.0).clamp(0.0, 2.0) };
    if ui.interact(minus, ui.id().with("clip_vm"), Sense::click()).on_hover_text(tr("Más bajo")).clicked() {
        crate::panels::audio::clip_set_gain(app, id, step(clip.gain, false));
    }
    if ui.interact(plus, ui.id().with("clip_vp"), Sense::click()).on_hover_text(tr("Más alto")).clicked() {
        crate::panels::audio::clip_set_gain(app, id, step(clip.gain, true));
    }
    let at = {
        let p = &app.doc.project;
        trazo_core::audio::ticks_to_samples(p.tick_of_frame(app.doc.frame) as f64, p.canvas.fps)
    };
    let can_split = at > clip.start && at < clip.end();
    if item(ui, slot(1.6, 0.85), Icon::Cut, tr("Cortar"), can_split, "split").on_hover_text(tr("Cortar en el fotograma actual")).clicked() {
        crate::panels::audio::clip_split_at_playhead(app, id);
    }
    let mute_icon = if clip.muted { Icon::SpeakerOff } else { Icon::Speaker };
    if item(ui, slot(2.45, 0.85), mute_icon, if clip.muted { tr("Activar") } else { tr("Silenciar") }, true, "mute").clicked() {
        crate::panels::audio::clip_toggle_mute(app, id);
    }
    if item(ui, slot(3.3, 0.85), Icon::Trash, tr("Borrar"), true, "del").clicked() {
        crate::panels::audio::clip_delete(app, id);
    }
    let more = item(ui, slot(4.15, 0.85), Icon::More, tr("Más"), true, "more");
    egui::Popup::menu(&more).align(egui::RectAlign::TOP_END).show(|ui| {
        ui.set_min_width(260.0);
        if menu_row(ui, Icon::Duplicate, tr("Duplicar a continuación"), None, TEXT) {
            crate::panels::audio::clip_duplicate(app, id);
            ui.close();
        }
        let fin = clip.fade_in as f32 / trazo_core::audio::SAMPLE_RATE as f32;
        let fout = clip.fade_out as f32 / trazo_core::audio::SAMPLE_RATE as f32;
        let next = |s: f32| if s < 0.25 { 0.5 } else if s < 0.75 { 1.0 } else if s < 1.5 { 2.0 } else { 0.0 };
        let fade_label = |s: f32| if s <= 0.0 { tr("no").to_string() } else { format!("{} s", crate::i18n::num(s, 1)) };
        if menu_row(ui, Icon::Up, tr("Entrada suave"), Some(&fade_label(fin)), TEXT) {
            crate::panels::audio::clip_set_fades(app, id, Some(next(fin)), None);
        }
        if menu_row(ui, Icon::Down, tr("Salida suave"), Some(&fade_label(fout)), TEXT) {
            crate::panels::audio::clip_set_fades(app, id, None, Some(next(fout)));
        }
        if menu_row(ui, Icon::Speaker, tr("Normalizar volumen"), Some(tr("Lo más alto posible sin distorsión")), TEXT) {
            crate::panels::audio::clip_normalize(app, &ctx, id);
            ui.close();
        }
        if menu_row(ui, Icon::Cut, tr("Separar en frases"), Some(tr("Corta donde hay silencios")), TEXT) {
            crate::panels::audio::clip_split_silences(app, &ctx, id);
            ui.close();
        }
        if menu_row(ui, Icon::Mic, tr("Grabar otra toma aquí"), Some(tr("En esta misma pista")), TEXT) {
            let track = app.doc.project.audio.find_clip(id).map(|(t, _)| app.doc.project.audio.tracks[t].id);
            crate::panels::audio::set_target_track(app, track);
            crate::panels::audio::begin_recording(app, &ctx);
            ui.close();
        }
    });
    true
}

// ---- focus mode ------------------------------------------------------------------------

/// Hide the timeline and the panels: only the tools and the canvas.
pub fn toggle_focus(app: &mut TrazoApp) {
    app.ui.focus = !app.ui.focus;
    if app.ui.focus {
        app.ui.dock_open = false;
        app.ui.sheet = None;
        app.ui.drawing_menu = None;
    }
}

/// While in focus mode: brush, color and leave, in the top corner. Brush and
/// color open floating menus, so the interface doesn't have to come back.
pub fn focus_exit(app: &mut TrazoApp, ctx: &egui::Context) {
    if !app.ui.focus {
        app.ui.focus_pop = None;
        return;
    }
    let screen = ctx.content_rect();
    let top = crate::widgets::insets(app.platform.touch_first).0;
    let side = if crate::widgets::SizeClass::of(ctx) == crate::widgets::SizeClass::PhoneLandscape { SIDE } else { 16.0 };
    let anchor = Pos2::new(screen.right() - side, screen.top() + top + 14.0);
    let bar = egui::Area::new(egui::Id::unique("focus_exit")).order(Order::Foreground).pivot(Align2::RIGHT_TOP).fixed_pos(anchor).show(ctx, |ui| {
        Frame::new().fill(SURFACE).corner_radius(14).stroke(Stroke::new(1.0, Color32::from_white_alpha(13))).shadow(theme::island_shadow()).inner_margin(Margin::same(2)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut button = |ui: &mut Ui, on: bool, tip: &str, draw: &dyn Fn(&egui::Painter, Rect)| -> bool {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
                    if on || resp.hovered() || resp.is_pointer_button_down_on() {
                        ui.painter().rect_filled(r, 12.0, if on { SEPARATOR } else { ELEVATED });
                    }
                    draw(ui.painter(), r);
                    resp.on_hover_text(tip).clicked()
                };
                let brush_on = app.ui.focus_pop == Some(SideTab::Brushes);
                if button(ui, brush_on, tr("Pinceles"), &|p, r| paint(p, Rect::from_center_size(r.center(), Vec2::splat(22.0)), Icon::Brush, TEXT)) {
                    app.ui.focus_pop = if brush_on { None } else { Some(SideTab::Brushes) };
                }
                let color = app.tools.color;
                let color_on = app.ui.focus_pop == Some(SideTab::Color);
                if button(ui, color_on, tr("Color"), &|p, r| {
                    p.circle_filled(r.center(), 13.0, TEXT);
                    p.circle_filled(r.center(), 11.0, SURFACE);
                    p.circle_filled(r.center(), 9.5, color);
                }) {
                    app.ui.focus_pop = if color_on { None } else { Some(SideTab::Color) };
                }
                let (sep, _) = ui.allocate_exact_size(Vec2::new(5.0, 28.0), Sense::hover());
                ui.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, SEPARATOR));
                if button(ui, false, tr("Salir del modo enfoque (Tab)"), &|p, r| paint(p, Rect::from_center_size(r.center(), Vec2::splat(22.0)), Icon::Focus, ACCENT)) {
                    toggle_focus(app);
                }
            });
        });
    });
    // The floating menu under the buttons.
    let Some(tab) = app.ui.focus_pop else { return };
    let w = 340.0f32.min(screen.width() - 24.0);
    let max_h = (screen.bottom() - bar.response.rect.bottom() - 24.0).max(200.0);
    let menu = egui::Area::new(egui::Id::unique("focus_pop")).order(Order::Tooltip).pivot(Align2::RIGHT_TOP)
        .fixed_pos(Pos2::new(anchor.x, bar.response.rect.bottom() + 8.0)).constrain(true)
        .show(ctx, |ui| {
            Frame::new().fill(SURFACE).corner_radius(theme::R_ISLAND).stroke(Stroke::new(1.0, Color32::from_white_alpha(18)))
                .shadow(egui::Shadow { offset: [0, 20], blur: 50, spread: 0, color: Color32::from_black_alpha(166) })
                .inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_width(w - 24.0);
                    let title = if tab == SideTab::Color { tr("Color") } else { tr("Pinceles") };
                    ui.label(egui::RichText::new(title).font(theme::font(17.0, W::Semi)).color(TEXT));
                    ui.add_space(6.0);
                    match tab {
                        SideTab::Color => {
                            egui::ScrollArea::vertical().id_salt("focus_pop_body").max_height(max_h - 60.0).auto_shrink([false, true]).show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 12.0;
                                crate::panels::toolbar::color_panel(app, ui);
                            });
                        }
                        // The brush list scrolls by itself; give it a fixed height.
                        _ => {
                            let h = (max_h - 60.0).min(560.0);
                            ui.allocate_ui(Vec2::new(ui.available_width(), h), |ui| {
                                ui.set_min_height(h);
                                ui.set_clip_rect(ui.max_rect());
                                crate::panels::brushes::show(app, ui);
                            });
                        }
                    }
                });
        });
    // A tap outside closes it (Escape too).
    let outside = ctx.input(|i| i.pointer.any_pressed())
        && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| !menu.response.rect.contains(p) && !bar.response.rect.contains(p));
    if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.ui.focus_pop = None;
    }
}
