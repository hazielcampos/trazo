//! Left tool strip: tools, size/opacity sliders, current color.

use crate::i18n::tr;
use egui::{Color32, RichText, Vec2};

use trazo_core::shapes::ShapeKind;

use crate::app::TrazoApp;
use crate::icons::{Icon, IconButton};
use crate::panels::SideTab;
use crate::tools::{LassoMode, Tool, PALETTE};

pub fn show(app: &mut TrazoApp, ui: &mut egui::Ui) {
    egui::Panel::left("toolbar").resizable(false).exact_size(64.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                for tool in Tool::ALL {
                    let selected = app.tools.tool == tool;
                    if ui.add(IconButton::new(tool.icon()).size(44.0).selected(selected)).on_hover_text(tool.label()).clicked() {
                        if selected && matches!(tool, Tool::Brush | Tool::Eraser) {
                            // Second tap opens the brush library, like FlipaClip.
                            app.ui.show_side_panel = true;
                            app.ui.side_tab = SideTab::Brushes;
                        }
                        app.tools.tool = tool;
                    }
                }
                if app.tools.tool == Tool::Lasso {
                    ui.separator();
                    for (mode, icon, tip) in [
                        (LassoMode::Select, Icon::Lasso, tr("Lazo: seleccionar")),
                        (LassoMode::Fill, Icon::Bucket, tr("Lazo: rellenar con el color")),
                        (LassoMode::Erase, Icon::Eraser, tr("Lazo: borrar")),
                    ] {
                        if ui.add(IconButton::new(icon).size(40.0).selected(app.tools.lasso_mode == mode)).on_hover_text(tip).clicked() {
                            app.tools.lasso_mode = mode;
                        }
                    }
                }
                if app.tools.tool == Tool::Shape {
                    ui.separator();
                    for (kind, icon) in [(ShapeKind::Line, Icon::Line), (ShapeKind::Rect, Icon::Rect), (ShapeKind::Ellipse, Icon::Ellipse)] {
                        if ui.add(IconButton::new(icon).size(40.0).selected(app.tools.shape == kind)).on_hover_text(tr(kind.label())).clicked() {
                            app.tools.shape = kind;
                        }
                    }
                    ui.checkbox(&mut app.tools.shape_constrain, "").on_hover_text(tr("Proporción fija: cuadrado, círculo, líneas a 15° (o mantén Mayús)"));
                }
                ui.separator();

                // Current color: tap to open the color tab.
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(46.0), egui::Sense::click());
                ui.painter().circle_filled(rect.center(), 21.0, app.tools.color);
                ui.painter().circle_stroke(rect.center(), 22.0, egui::Stroke::new(2.0, Color32::WHITE));
                if resp.on_hover_text(tr("Color")).clicked() {
                    app.ui.show_side_panel = true;
                    app.ui.side_tab = SideTab::Color;
                }
                ui.separator();

                let brush_name = tr(&app.tools.active_brush().name).to_string();
                ui.label(RichText::new(brush_name).small());
                let tweak = app.tools.active_tweak_mut();
                ui.label(RichText::new(tr("Tamaño")).small());
                ui.add(egui::Slider::new(&mut tweak.size, 1.0..=300.0).logarithmic(true).vertical().show_value(false));
                ui.label(RichText::new(format!("{:.0}px", tweak.size)).small());
                ui.label(RichText::new(tr("Opacidad")).small());
                ui.add(egui::Slider::new(&mut tweak.opacity, 0.02..=1.0).vertical().show_value(false));
                ui.label(RichText::new(format!("{:.0}%", tweak.opacity * 100.0)).small());
            });
        });
    });
}

/// Color tab: HSV picker, palette and recent colors.
pub fn color_panel(app: &mut TrazoApp, ui: &mut egui::Ui) {
    use crate::theme::{self, ELEVATED, SEPARATOR, TEXT, TEXT2, W};
    let ctx = ui.ctx().clone();
    // New and previous color, the color's name, code and eyedropper.
    let prev_id = egui::Id::unique("color_prev");
    let prev = ui.data(|d| d.get_temp::<Color32>(prev_id)).unwrap_or(app.tools.color);
    ui.data_mut(|d| d.insert_temp(prev_id, prev));
    let hex_id = egui::Id::unique("color_hex_open");
    let mut hex_open = ui.data(|d| d.get_temp::<bool>(hex_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (r, resp) = ui.allocate_exact_size(Vec2::new(72.0, 40.0), egui::Sense::click());
        let half = egui::Rect::from_min_max(r.min, egui::pos2(r.center().x, r.bottom()));
        let p = ui.painter();
        p.rect_filled(r, 12.0, prev);
        p.rect_filled(half, egui::CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 }, app.tools.color);
        p.rect_stroke(r, 12.0, egui::Stroke::new(1.0, Color32::from_white_alpha(30)), egui::StrokeKind::Inside);
        if resp.on_hover_text(tr("Toca para volver al color anterior")).clicked() {
            app.tools.set_color(prev);
        }
        ui.label(egui::RichText::new(color_name(app.tools.color)).font(theme::font(15.0, W::Semi)).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::widgets::icon_button_ex(ui, Icon::Eyedropper, tr("Tomar un color del dibujo"), true, Some(ELEVATED)).clicked() {
                app.tools.tool = crate::tools::Tool::Eyedropper;
                app.ui.sheet = None;
            }
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(theme::TOUCH), egui::Sense::click());
            ui.painter().rect_filled(r, theme::R_CONTROL, if hex_open || resp.hovered() { SEPARATOR } else { ELEVATED });
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "#", theme::font(16.0, W::Bold), TEXT2);
            if resp.on_hover_text(tr("Escribir un código de color")).clicked() {
                hex_open = !hex_open;
            }
        });
    });
    if hex_open {
        ui.horizontal(|ui| {
            let id = egui::Id::unique("hex_edit");
            let current = format!("#{:02X}{:02X}{:02X}", app.tools.color.r(), app.tools.color.g(), app.tools.color.b());
            let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.clone());
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0).font(theme::font(14.0, W::Medium)).margin(egui::vec2(10.0, 8.0)));
            if r.changed() {
                if let Some(c) = parse_hex(&text) {
                    app.tools.set_color(c);
                }
                ui.data_mut(|d| d.insert_temp(id, text));
            } else if !r.has_focus() {
                ui.data_mut(|d| d.remove::<String>(id));
            }
        });
    }
    ui.data_mut(|d| d.insert_temp(hex_id, hex_open));
    let mut c = app.tools.color;
    if crate::widgets::hsv_picker(ui, &mut c) {
        app.tools.set_color(c);
    }

    // A palette: mine, the basic one or the recent colors.
    let which_id = egui::Id::unique("palette_which");
    let mut which: u8 = ui.data(|d| d.get_temp(which_id)).unwrap_or(if app.tools.user_palette.is_empty() { 1 } else { 0 });
    ui.horizontal(|ui| {
        let title = match which {
            0 => tf!("Colores de «{}»", app.doc.project.name),
            1 => tr("Paleta básica").to_string(),
            _ => tr("Recientes").to_string(),
        };
        ui.label(egui::RichText::new(title).font(theme::font(13.0, W::Regular)).color(TEXT2));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = ui.add(egui::Button::new(egui::RichText::new(tr("Otras paletas")).font(theme::font(13.0, W::Semi)).color(TEXT).underline()).frame(false).min_size(Vec2::new(0.0, 36.0)));
            egui::Popup::menu(&r).show(|ui| {
                ui.set_min_width(200.0);
                for (k, label) in [(0u8, tr("Mi paleta")), (1, tr("Paleta básica")), (2, tr("Recientes"))] {
                    if ui.add(egui::Button::new(label).selected(which == k).min_size(Vec2::new(200.0, 40.0))).clicked() {
                        which = k;
                        ui.close();
                    }
                }
            });
        });
    });
    ui.data_mut(|d| d.insert_temp(which_id, which));
    let colors: Vec<Color32> = match which {
        0 => app.tools.user_palette.clone(),
        1 => PALETTE.to_vec(),
        _ => app.tools.recent_colors.clone(),
    };
    let add = swatches(ui, &colors, &mut app.tools.color, which == 0);
    if add && !app.tools.user_palette.contains(&app.tools.color) {
        app.tools.user_palette.push(app.tools.color);
        which = 0;
        ui.data_mut(|d| d.insert_temp(which_id, which));
    }
    if which == 0 && colors.is_empty() {
        ui.label(egui::RichText::new(tr("Toca + para guardar el color actual")).font(theme::font(12.0, W::Regular)).color(TEXT2));
    }
    let _ = ctx;
}

/// A name for a color, in words people use.
fn color_name(c: Color32) -> String {
    let h = egui::ecolor::Hsva::from(c);
    let (hue, s, v) = (h.h * 360.0, h.s, h.v);
    let base = if v < 0.12 {
        return tr("Negro").to_string();
    } else if s < 0.1 {
        if v > 0.9 { tr("Blanco") } else if v > 0.55 { tr("Gris claro") } else { tr("Gris oscuro") }
    } else {
        match hue {
            h if !(15.0..345.0).contains(&h) => tr("Rojo"),
            h if h < 40.0 => tr("Naranja"),
            h if h < 65.0 => tr("Amarillo"),
            h if h < 160.0 => tr("Verde"),
            h if h < 200.0 => tr("Turquesa"),
            h if h < 255.0 => tr("Azul"),
            h if h < 290.0 => tr("Violeta"),
            _ => tr("Rosa"),
        }
    };
    let shade = if s >= 0.1 && v < 0.45 { tr(" oscuro") } else if s >= 0.1 && s < 0.4 && v > 0.7 { tr(" claro") } else { "" };
    format!("{base}{shade}")
}

/// One row of round 44 pt swatches that fits the width, ending with "+"
/// (save the current color); returns true when "+" was tapped.
fn swatches(ui: &mut egui::Ui, colors: &[Color32], current: &mut Color32, _mine: bool) -> bool {
    let w = ui.available_width();
    let k = (((w + 5.0) / 49.0).floor() as usize).max(2);
    let shown = colors.len().min(k - 1);
    let n = shown + 1;
    let gap = if n > 1 { ((w - n as f32 * 44.0) / (n as f32 - 1.0)).min(14.0) } else { 0.0 };
    let mut add = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for &c in &colors[..shown] {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::click());
            let p = ui.painter();
            if *current == c {
                p.circle_filled(rect.center(), 22.0, crate::theme::TEXT);
                p.circle_filled(rect.center(), 20.0, crate::theme::SURFACE);
                p.circle_filled(rect.center(), 18.0, c);
            } else {
                p.circle_filled(rect.center(), 22.0, c);
                p.circle_stroke(rect.center(), 21.25, egui::Stroke::new(1.5, Color32::from_white_alpha(56)));
            }
            if resp.clicked() {
                *current = c;
            }
        }
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::click());
        ui.painter().circle_stroke(rect.center(), 21.25, egui::Stroke::new(1.5, Color32::from_rgb(94, 94, 100)));
        crate::icons::paint(ui.painter(), egui::Rect::from_center_size(rect.center(), Vec2::splat(18.0)), Icon::Plus, Color32::from_rgb(214, 214, 211));
        add = resp.on_hover_text(tr("Guardar este color")).clicked();
    });
    add
}

/// "#RRGGBB" or "RRGGBB" (also 3-digit "#RGB").
fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    let v = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    match h.len() {
        6 => Some(Color32::from_rgb(v(0, 2)?, v(2, 2)?, v(4, 2)?)),
        3 => Some(Color32::from_rgb(v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17)),
        _ => None,
    }
}
