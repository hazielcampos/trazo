//! Phone layouts (see the "Trazo — Interfaz responsiva" design).
//!
//! Upright: slim top bar, canvas with a floating tool dock and a size rail,
//! compact timeline; layers / brushes / color open as sheets from below.
//! On its side: tool rail on the left, actions on the right, a floating
//! frame strip, and panels as a sheet docked on the right.

use crate::i18n::tr;
use egui::{Align2, Color32, Frame, Margin, Order, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

use trazo_core::shapes::ShapeKind;

use crate::app::{TrazoApp, Redraw};
use crate::icons::{paint, Icon};
use crate::panels::SideTab;
use crate::theme::{self, ACCENT, CONTROL, ELEVATED, SURFACE, TEXT, TEXT2};
use crate::tools::{LassoMode, Tool};
use crate::widgets::{self, tool_button, SheetEdge};

/// Tools always in reach; the rest live under "Más".
const MAIN_TOOLS: [Tool; 4] = [Tool::Brush, Tool::Eraser, Tool::Fill, Tool::Lasso];
const MORE_TOOLS: [Tool; 8] = [Tool::Select, Tool::Wand, Tool::Shape, Tool::Text, Tool::Eyedropper, Tool::Hand, Tool::Zoom, Tool::ZoomOut];

fn bar_frame() -> Frame {
    Frame::new().fill(SURFACE).inner_margin(Margin::symmetric(4, 4))
}

fn floating_frame() -> Frame {
    Frame::new()
        .fill(theme::floating())
        .corner_radius(22)
        .inner_margin(Margin::same(6))
        .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(115) })
}

/// Plain 44 pt icon button.
fn icon(ui: &mut Ui, icon: Icon, enabled: bool, color: Color32, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(theme::TOUCH), if enabled { Sense::click() } else { Sense::hover() });
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 12.0, ELEVATED);
    }
    let c = if enabled { color } else { theme::TEXT3 };
    paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(22.0)), icon, c);
    resp.on_hover_text(tip)
}

/// Opens or closes a panel sheet.
fn toggle_sheet(app: &mut TrazoApp, tab: SideTab) {
    app.ui.sheet = if app.ui.sheet == Some(tab) { None } else { Some(tab) };
}


// ---- upright --------------------------------------------------------------------------

/// Top bar: projects, title (project menu), undo, redo, layers, export.
pub fn top_bar(app: &mut TrazoApp, ui: &mut Ui) {
    egui::Panel::top("phone_top").exact_size(54.0).resizable(false).show_separator_line(false).frame(bar_frame()).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let ctx = ui.ctx().clone();
            if icon(ui, Icon::ArrowLeft, true, TEXT, tr("Inicio: mis animaciones")).clicked() {
                app.go_home(&ctx);
            }
            let title_w = (ui.available_width() - 4.0 * theme::TOUCH).max(80.0);
            let title = title_button(app, ui, title_w);
            egui::Popup::menu(&title).show(|ui| project_menu(app, ui));
            let can_undo = app.doc.history.can_undo() || app.canvas.select.is_active();
            if icon(ui, Icon::Undo, can_undo, TEXT, tr("Deshacer (2 dedos)")).clicked() {
                app.undo(&ctx);
            }
            if icon(ui, Icon::Redo, app.doc.history.can_redo(), TEXT, tr("Rehacer (3 dedos)")).clicked() {
                app.redo(&ctx);
            }
            let layers_on = app.ui.sheet == Some(SideTab::Layers);
            if icon(ui, Icon::Layers, true, if layers_on { ACCENT } else { TEXT }, tr("Capas")).clicked() {
                toggle_sheet(app, SideTab::Layers);
            }
            if icon(ui, Icon::Export, true, ACCENT, tr("Exportar y compartir")).clicked() {
                app.export_ui.open = true;
            }
        });
    });
}

/// Project name with fps and save state underneath; opens the project menu.
fn title_button(app: &TrazoApp, ui: &mut Ui, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, theme::TOUCH), Sense::click());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 12.0, ELEVATED);
    }
    let p = ui.painter_at(rect);
    let x = rect.left() + 8.0;
    let name = app.doc.project.name.clone();
    p.text(Pos2::new(x, rect.top() + 13.0), Align2::LEFT_CENTER, name, egui::FontId::proportional(16.0), TEXT);
    let state = if app.doc.is_modified() { tr("Sin guardar") } else { tr("Guardado") };
    let sub = tf!("{} fps · {}", app.doc.project.canvas.fps, state);
    let r = p.text(Pos2::new(x, rect.top() + 32.0), Align2::LEFT_CENTER, sub, egui::FontId::proportional(12.0), TEXT2);
    paint(&p, Rect::from_center_size(Pos2::new(r.right() + 9.0, r.center().y), Vec2::splat(12.0)), Icon::Down, TEXT2);
    resp.on_hover_text(tr("Menú del proyecto"))
}

/// Project, import and view options (used by both orientations).
pub fn project_menu(app: &mut TrazoApp, ui: &mut Ui) {
    ui.set_min_width(250.0);
    let ctx = ui.ctx().clone();
    let item = |ui: &mut Ui, icon: Icon, text: &str| ui.add(crate::icons::IconButton::new(icon).keep_text().text(text).size(40.0)).clicked();
    if item(ui, Icon::New, tr("Nuevo proyecto…")) {
        app.ui.new_project_dialog = Some(Default::default());
        ui.close();
    }
    if item(ui, Icon::ArrowLeft, tr("Inicio: mis animaciones")) {
        ui.close();
        app.go_home(&ctx);
    }
    if item(ui, Icon::Save, tr("Guardar")) {
        app.save_async(true);
        ui.close();
    }
    if item(ui, Icon::Settings, tr("Ajustes de la animación…")) {
        app.ui.settings_dialog = true;
        ui.close();
    }
    if item(ui, Icon::Camera, tr("Cámara…")) {
        app.ui.camera.open = true;
        ui.close();
    }
    if item(ui, Icon::Clock, tr("Versiones anteriores…")) {
        app.ui.versions_dialog = true;
        ui.close();
    }
    if cfg!(target_os = "android") {
        if item(ui, Icon::Globe, tr("Idioma · Language…")) {
            app.ui.language_dialog = Some(false);
            ui.close();
        }
    } else if item(ui, Icon::Settings, tr("Ajustes de Trazo…")) {
        app.ui.app_settings = true;
        ui.close();
    }
    ui.separator();
    if item(ui, Icon::Music, tr("Sonido y voz…")) {
        app.audio_ui.show = true;
        ui.close();
    }
    if item(ui, Icon::Film, tr("Importar video…")) {
        crate::files::request(app, &ctx, crate::files::PickPurpose::Video);
        ui.close();
    }
    if item(ui, Icon::Image, tr("Importar imagen…")) {
        crate::files::request(app, &ctx, crate::files::PickPurpose::ImageLayer);
        ui.close();
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.add(crate::icons::IconButton::new(Icon::RotateLeft).size(40.0)).on_hover_text(tr("Girar lienzo")).clicked() {
            app.canvas.rotate_by(-15.0);
        }
        if ui.add(crate::icons::IconButton::new(Icon::RotateRight).size(40.0)).on_hover_text(tr("Girar lienzo")).clicked() {
            app.canvas.rotate_by(15.0);
        }
        if ui.add(crate::icons::IconButton::new(Icon::FlipH).size(40.0).selected(app.canvas.is_flipped())).on_hover_text(tr("Voltear vista")).clicked() {
            app.canvas.toggle_flip();
        }
        if ui.add(crate::icons::IconButton::new(Icon::Fit).size(40.0)).on_hover_text(tr("Ajustar a la pantalla")).clicked() {
            app.canvas.fit();
        }
    });
    widgets::switch_row(ui, tr("Cuadrícula"), &mut app.tools.show_grid);
    widgets::switch_row(ui, tr("Simetría izquierda-derecha"), &mut app.tools.symmetry_vertical);
    widgets::switch_row(ui, tr("Simetría arriba-abajo"), &mut app.tools.symmetry_horizontal);
}

/// Floating tool dock over the bottom of the canvas.
pub fn dock(app: &mut TrazoApp, ctx: &egui::Context, canvas: Rect) {
    egui::Area::new(egui::Id::unique("phone_dock"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(Pos2::new(canvas.center().x, canvas.bottom() - 10.0))
        .show(ctx, |ui| {
            floating_frame().show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 6.0);
                tool_options(app, ui);
                ui.horizontal(|ui| {
                    for t in MAIN_TOOLS {
                        tool_slot(app, ui, t, Vec2::new(46.0, 44.0));
                    }
                    more_tools(app, ui, Vec2::new(44.0, 44.0));
                    let (sep, _) = ui.allocate_exact_size(Vec2::new(9.0, 28.0), Sense::hover());
                    ui.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, CONTROL));
                    brush_slot(app, ui);
                    color_slot(app, ui);
                });
            });
        });
}

/// A tool button; a second tap on brush or eraser opens the brush library.
fn tool_slot(app: &mut TrazoApp, ui: &mut Ui, t: Tool, size: Vec2) {
    let selected = app.tools.tool == t;
    if tool_button(ui, t.icon(), selected, size, t.label()).clicked() {
        if selected && matches!(t, Tool::Brush | Tool::Eraser) {
            app.ui.sheet = Some(SideTab::Brushes);
        }
        app.tools.tool = t;
    }
}

/// "Más": the less frequent tools. Shows the active one when it's in here.
pub fn more_tools_pub(app: &mut TrazoApp, ui: &mut Ui, size: Vec2) {
    more_tools(app, ui, size);
}

fn more_tools(app: &mut TrazoApp, ui: &mut Ui, size: Vec2) {
    let active = MORE_TOOLS.contains(&app.tools.tool);
    let ic = if active { app.tools.tool.icon() } else { Icon::More };
    let resp = tool_button(ui, ic, active, size, tr("Más herramientas"));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(220.0);
        for t in MORE_TOOLS {
            if ui.add(crate::icons::IconButton::new(t.icon()).keep_text().text(t.label()).size(40.0).selected(app.tools.tool == t)).clicked() {
                app.tools.tool = t;
                ui.close();
            }
        }
    });
}

/// Current brush: a stroke sample; tap for the library.
fn brush_slot(app: &mut TrazoApp, ui: &mut Ui) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(44.0, 44.0), Sense::click());
    let open = app.ui.sheet == Some(SideTab::Brushes);
    ui.painter().rect_filled(rect, 14.0, if open { theme::accent_soft() } else { ELEVATED });
    let w = (app.tools.active_tweak_mut().size / 6.0).clamp(1.5, 5.0);
    let c = rect.center();
    let pts: Vec<Pos2> = (0..=16).map(|i| {
        let t = i as f32 / 16.0;
        Pos2::new(c.x - 13.0 + 26.0 * t, c.y + (t * std::f32::consts::TAU).sin() * 5.0)
    }).collect();
    ui.painter().add(egui::Shape::line(pts, Stroke::new(w, TEXT)));
    if resp.on_hover_text(tr(&app.tools.active_brush().name).to_string()).clicked() {
        toggle_sheet(app, SideTab::Brushes);
    }
}

/// Current color; tap for the color sheet.
fn color_slot(app: &mut TrazoApp, ui: &mut Ui) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(44.0, 44.0), Sense::click());
    ui.painter().circle_filled(rect.center(), 15.0, app.tools.color);
    ui.painter().circle_stroke(rect.center(), 16.0, Stroke::new(2.0, TEXT));
    if resp.on_hover_text(tr("Color")).clicked() {
        toggle_sheet(app, SideTab::Color);
    }
}

/// Options of the active tool (lasso modes, shape kinds) above the dock.
pub fn tool_options_pub(app: &mut TrazoApp, ui: &mut Ui) {
    tool_options(app, ui);
}

fn tool_options(app: &mut TrazoApp, ui: &mut Ui) {
    match app.tools.tool {
        Tool::Lasso => {
            ui.horizontal(|ui| {
                for (mode, ic, tip) in [(LassoMode::Select, Icon::Lasso, tr("Seleccionar")), (LassoMode::Fill, Icon::Bucket, tr("Rellenar")), (LassoMode::Erase, Icon::Eraser, tr("Borrar"))] {
                    if ui.add(crate::icons::IconButton::new(ic).text(tip).size(36.0).selected(app.tools.lasso_mode == mode)).clicked() {
                        app.tools.lasso_mode = mode;
                    }
                }
            });
        }
        Tool::Shape => {
            ui.horizontal(|ui| {
                for (kind, ic) in [(ShapeKind::Line, Icon::Line), (ShapeKind::Rect, Icon::Rect), (ShapeKind::Ellipse, Icon::Ellipse)] {
                    if ui.add(crate::icons::IconButton::new(ic).size(36.0).selected(app.tools.shape == kind)).on_hover_text(tr(kind.label())).clicked() {
                        app.tools.shape = kind;
                    }
                }
                ui.label(RichText::new(tr("Proporción fija")).size(13.0).color(TEXT2));
                widgets::switch(ui, &mut app.tools.shape_constrain);
            });
        }
        Tool::Fill => {
            ui.set_max_width(300.0);
            ui.checkbox(&mut app.tools.fill_all_layers, tr("Detectar bordes de todas las capas"))
                .on_hover_text(tr("Si alguna capa está marcada como «Líneas para el relleno», se usa esa"));
            ui.checkbox(&mut app.tools.fill_every_drawing, tr("Rellenar en todos los dibujos de la capa"))
                .on_hover_text(tr("Un toque rellena el mismo punto en cada dibujo de esta capa (útil para colorear fondos o personajes quietos)"));
            let mut tol = app.tools.fill.tolerance as f32;
            widgets::hslider(ui, tr("Tolerancia"), &mut tol, 0.0..=255.0, false, |v| format!("{v:.0}"));
            app.tools.fill.tolerance = tol as u8;
            let mut g = app.tools.fill.grow as f32;
            widgets::hslider(ui, tr("Expandir"), &mut g, 0.0..=4.0, false, |v| format!("{v:.0} px"));
            app.tools.fill.grow = g.round() as _;
            let mut gc = app.tools.fill.gap_close as f32;
            widgets::hslider(ui, tr("Cerrar huecos"), &mut gc, 0.0..=8.0, false, |v| format!("{v:.0} px"));
            app.tools.fill.gap_close = gc.round() as _;
        }
        Tool::Zoom | Tool::ZoomOut => {
            let mut i = usize::from(app.tools.tool == Tool::ZoomOut);
            if widgets::segmented(ui, &mut i, &[tr("Acercar"), tr("Alejar")], 240.0) {
                app.tools.tool = if i == 0 { Tool::Zoom } else { Tool::ZoomOut };
            }
        }
        Tool::Eyedropper => {
            ui.checkbox(&mut app.tools.pick_layer_only, tr("Cuentagotas: solo la capa actual"));
        }
        _ => {}
    }
}

/// Whether a tool has options worth a popup.
pub fn has_options(t: Tool) -> bool {
    matches!(t, Tool::Fill | Tool::Lasso | Tool::Shape | Tool::Eyedropper | Tool::Zoom | Tool::ZoomOut)
}

/// Brush size and opacity on the left edge of the canvas (Procreate style).
pub fn size_rail(app: &mut TrazoApp, ctx: &egui::Context, canvas: Rect, top: bool) {
    // Both sliders need ~270 pt plus room for the dock below; on a very
    // short canvas (a tall audio panel) the rail would cover the dock.
    if !top && canvas.height() < 250.0 {
        return;
    }
    let top = top || canvas.height() < 360.0;
    let pos = if top { Pos2::new(canvas.left() + 8.0, canvas.top() + 12.0) } else { Pos2::new(canvas.left() + 8.0, canvas.center().y - 40.0) };
    let pivot = if top { Align2::LEFT_TOP } else { Align2::LEFT_CENTER };
    egui::Area::new(egui::Id::unique("size_rail")).order(Order::Foreground).pivot(pivot).fixed_pos(pos).show(ctx, |ui| {
        Frame::new().fill(theme::floating()).corner_radius(18).inner_margin(Margin::symmetric(2, 8)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.vertical_centered(|ui| {
                let tw = app.tools.active_tweak_mut();
                widgets::vslider(ui, &mut tw.size, 1.0..=300.0, 128.0, true, tr("Tamaño del pincel"));
                ui.label(RichText::new(format!("{:.0}", tw.size)).size(11.0).color(TEXT2));
                if !top {
                    ui.add_space(4.0);
                    widgets::vslider(ui, &mut tw.opacity, 0.02..=1.0, 92.0, false, tr("Opacidad"));
                    ui.label(RichText::new(format!("{:.0}", tw.opacity * 100.0)).size(11.0).color(TEXT2));
                }
            });
        });
    });
}

/// Zoom level; tap to fit the canvas again.
pub fn zoom_pill(app: &mut TrazoApp, ctx: &egui::Context, canvas: Rect) {
    egui::Area::new(egui::Id::unique("zoom_pill")).order(Order::Foreground).pivot(Align2::RIGHT_TOP).fixed_pos(Pos2::new(canvas.right() - 10.0, canvas.top() + 10.0)).show(ctx, |ui| {
        let text = format!("{:.0} %", app.canvas.zoom() * 100.0);
        let r = ui.add(egui::Button::new(RichText::new(text).size(12.0).color(TEXT2)).fill(theme::floating()).corner_radius(14).min_size(Vec2::new(52.0, 30.0)));
        if r.on_hover_text(tr("Ajustar a la pantalla")).clicked() {
            app.canvas.fit();
        }
    });
}

/// Compact timeline: transport row and the frame strip.
pub fn timeline(app: &mut TrazoApp, ui: &mut Ui) {
    egui::Panel::bottom("phone_timeline")
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(SURFACE).inner_margin(Margin { left: 8, right: 8, top: 6, bottom: 8 }).stroke(Stroke::new(1.0, ELEVATED)))
        .show(ui, |ui| {
            crate::panels::timeline::commit_selection_on_click(app, ui);
            tabs_header(app, ui);
            ui.add_space(4.0);
            crate::panels::timeline::tracks(app, ui, true, 3.0);
            crate::panels::timeline::block_actions(app, ui);
        });
}

/// Bottom header shared by the two tabs: play, frame counter, the
/// "Fotogramas | Audio" switch and the options of the open tab.
pub fn tabs_header(app: &mut TrazoApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        play_button(app, ui, 44.0);
        counter(app, ui);
        ui.add_space(6.0);
        let mut tab = usize::from(app.audio_ui.show);
        let w = (ui.available_width() - 2.0 * theme::TOUCH - 8.0).clamp(120.0, 220.0);
        if widgets::segmented(ui, &mut tab, &[tr("Animación"), tr("Audio")], w) {
            app.audio_ui.show = tab == 1;
            ui.ctx().request_repaint();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !app.audio_ui.show {
                let more = icon(ui, Icon::More, true, TEXT, tr("Opciones del fotograma"));
                egui::Popup::menu(&more).show(|ui| frame_menu(app, ui));
                onion_toggle(app, ui);
            }
        });
    });
}

fn play_button(app: &mut TrazoApp, ui: &mut Ui, size: f32) {
    let ctx = ui.ctx().clone();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    ui.painter().circle_filled(rect.center(), size / 2.0, TEXT);
    let ic = if app.playback.playing { Icon::Pause } else { Icon::Play };
    paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(size * 0.45)), ic, Color32::BLACK);
    if resp.on_hover_text(tr("Reproducir / pausar")).clicked() {
        app.toggle_play(&ctx);
    }
}

fn counter(app: &TrazoApp, ui: &mut Ui) {
    let n = app.doc.project.frames.len();
    let cur = app.displayed_frame() + 1;
    let mut job = egui::text::LayoutJob::default();
    job.append(&format!(" {cur}"), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(15.0), color: TEXT, ..Default::default() });
    job.append(&format!(" / {n}"), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(15.0), color: TEXT2, ..Default::default() });
    ui.label(job);
}

fn onion_toggle(app: &mut TrazoApp, ui: &mut Ui) {
    let on = app.onion.enabled;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(theme::TOUCH), Sense::click());
    if on {
        ui.painter().rect_filled(rect, 12.0, theme::accent_soft());
    }
    paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(22.0)), Icon::Onion, if on { ACCENT } else { TEXT });
    if resp.on_hover_text(tr("Papel cebolla")).clicked() {
        app.onion.enabled = !app.onion.enabled;
        app.doc.redraw = Redraw::Full;
    }
}

/// Playback and onion-skin options (drawing operations live in the timeline).
pub fn frame_menu(app: &mut TrazoApp, ui: &mut Ui) {
    ui.set_min_width(260.0);
    ui.horizontal(|ui| {
        ui.label(tr("Velocidad"));
        let mut fps = app.doc.project.canvas.fps;
        if ui.add(egui::DragValue::new(&mut fps).range(1..=60).suffix(" fps")).changed() {
            app.doc.project.canvas.fps = fps;
            app.doc.mark_settings_changed();
        }
    });
    widgets::switch_row(ui, tr("Repetir en bucle"), &mut app.playback.looping);
    widgets::switch_row(ui, tr("Ida y vuelta"), &mut app.playback.ping_pong);
    ui.separator();
    ui.label(RichText::new(tr("PAPEL CEBOLLA")).size(12.0).color(TEXT2));
    let o = &mut app.onion;
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui.add(egui::DragValue::new(&mut o.before).range(0..=5).prefix(tr("antes "))).changed();
        changed |= ui.add(egui::DragValue::new(&mut o.after).range(0..=5).prefix(tr("después "))).changed();
    });
    changed |= ui.add(egui::Slider::new(&mut o.opacity, 0.05..=0.9).text(tr("Opacidad")).show_value(false)).changed();
    changed |= widgets::switch_row(ui, tr("Colores reales"), &mut o.keep_colors);
    if changed {
        app.doc.redraw = Redraw::Full;
    }

}

// ---- on its side ----------------------------------------------------------------------

/// Left rail: projects, tools, brush and color.
pub fn left_rail(app: &mut TrazoApp, ui: &mut Ui) {
    egui::Panel::left("phone_left").exact_size(64.0).resizable(false).show_separator_line(false).frame(bar_frame()).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let menu = icon(ui, Icon::Open, true, TEXT, tr("Proyecto"));
            egui::Popup::menu(&menu).show(|ui| project_menu(app, ui));
            for t in MAIN_TOOLS {
                tool_slot(app, ui, t, Vec2::new(48.0, 42.0));
            }
            more_tools(app, ui, Vec2::new(48.0, 42.0));
            // Brush and color go at the bottom only if they fit without
            // covering the tools (short phones in landscape).
            if ui.available_height() >= 2.0 * theme::TOUCH + 8.0 {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    color_slot(app, ui);
                    brush_slot(app, ui);
                });
            } else {
                color_slot(app, ui);
            }
        });
    });
}

/// Right rail: history, panels, frame menu, export.
pub fn right_rail(app: &mut TrazoApp, ui: &mut Ui) {
    egui::Panel::right("phone_right").exact_size(64.0).resizable(false).show_separator_line(false).frame(bar_frame()).show(ui, |ui| {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let can_undo = app.doc.history.can_undo() || app.canvas.select.is_active();
            if icon(ui, Icon::Undo, can_undo, TEXT, tr("Deshacer")).clicked() {
                app.undo(&ctx);
            }
            if icon(ui, Icon::Redo, app.doc.history.can_redo(), TEXT, tr("Rehacer")).clicked() {
                app.redo(&ctx);
            }
            let layers_on = app.ui.sheet.is_some();
            if icon(ui, Icon::Layers, true, if layers_on { ACCENT } else { TEXT }, tr("Capas, pinceles y color")).clicked() {
                app.ui.sheet = if layers_on { None } else { Some(SideTab::Layers) };
            }
            let audio_on = app.audio_ui.show;
            if icon(ui, Icon::Music, true, if audio_on { ACCENT } else { TEXT }, tr("Audio")).clicked() {
                app.audio_ui.show = !app.audio_ui.show;
                ui.ctx().request_repaint();
            }
            let more = icon(ui, Icon::More, true, TEXT, tr("Opciones del fotograma"));
            egui::Popup::menu(&more).show(|ui| frame_menu(app, ui));
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                if tool_button(ui, Icon::Export, true, Vec2::new(48.0, 48.0), tr("Exportar y compartir")).clicked() {
                    app.export_ui.open = true;
                }
            });
        });
    });
}

/// Floating frame strip along the bottom of the canvas.
pub fn floating_strip(app: &mut TrazoApp, ctx: &egui::Context, canvas: Rect) {
    let w = canvas.width() - 20.0;
    egui::Area::new(egui::Id::unique("phone_strip")).order(Order::Foreground).pivot(Align2::CENTER_BOTTOM).fixed_pos(Pos2::new(canvas.center().x, canvas.bottom() - 8.0)).show(ctx, |ui| {
        Frame::new().fill(Color32::from_rgba_unmultiplied(28, 28, 30, 245)).corner_radius(16).inner_margin(Margin::same(6)).show(ui, |ui| {
            ui.set_width(w - 12.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                play_button(app, ui, 40.0);
                counter(app, ui);
                onion_toggle(app, ui);
                ui.vertical(|ui| crate::panels::timeline::tracks(app, ui, true, 2.0));
            });
        });
    });
}

/// The panel sheet (layers / brushes / color) for the phone layouts.
pub fn panel_sheet(app: &mut TrazoApp, ctx: &egui::Context, edge: SheetEdge) {
    let Some(tab) = app.ui.sheet else { return };
    if tab == SideTab::Color && edge == SheetEdge::Bottom {
        crate::panels::phone_up::color_popover(app, ctx);
        return;
    }
    let touch = app.platform.touch_first;
    let closed = widgets::sheet(
        ctx,
        "panels",
        edge,
        touch,
        |ui| match tab {
            SideTab::Layers => crate::panels::layers::header(ui),
            SideTab::Brushes => crate::panels::brushes::header(ui),
            SideTab::Color => {
                ui.label(RichText::new(tr("Color")).font(crate::theme::font(20.0, crate::theme::W::Semi)).color(TEXT));
            }
        },
        |ui| match tab {
            SideTab::Layers => crate::panels::layers::show(app, ui),
            SideTab::Brushes => crate::panels::brushes::show(app, ui),
            SideTab::Color => crate::panels::toolbar::color_panel(app, ui),
        },
    );
    if closed {
        app.ui.sheet = None;
    }
}
