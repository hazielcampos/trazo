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
    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut c = app.tools.color;
        if crate::widgets::hsv_picker(ui, &mut c) {
            app.tools.set_color(c);
        }
        ui.horizontal(|ui| {
            ui.label(tr("Hex"));
            let id = egui::Id::unique("hex_edit");
            let current = format!("#{:02X}{:02X}{:02X}", app.tools.color.r(), app.tools.color.g(), app.tools.color.b());
            let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.clone());
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0));
            if r.changed() {
                if let Some(c) = parse_hex(&text) {
                    app.tools.set_color(c);
                }
                ui.data_mut(|d| d.insert_temp(id, text));
            } else if !r.has_focus() {
                ui.data_mut(|d| d.remove::<String>(id));
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(tr("Mi paleta"));
            if ui.add(IconButton::new(Icon::Plus).size(24.0)).on_hover_text(tr("Guardar el color actual")).clicked() && !app.tools.user_palette.contains(&app.tools.color) {
                app.tools.user_palette.push(app.tools.color);
            }
            if ui.add_enabled(!app.tools.user_palette.is_empty(), IconButton::new(Icon::Minus).size(24.0)).on_hover_text(tr("Quitar el color actual de mi paleta")).clicked() {
                let c = app.tools.color;
                app.tools.user_palette.retain(|x| *x != c);
            }
        });
        if app.tools.user_palette.is_empty() {
            ui.label(egui::RichText::new(tr("Pulsa + para guardar colores aquí")).small().weak());
        } else {
            let mine = app.tools.user_palette.clone();
            swatches(ui, &mine, &mut app.tools.color);
        }
        ui.add_space(6.0);
        ui.label(tr("Paleta básica"));
        swatches(ui, &PALETTE, &mut app.tools.color);
        if !app.tools.recent_colors.is_empty() {
            ui.add_space(6.0);
            ui.label(tr("Recientes"));
            let recent = app.tools.recent_colors.clone();
            swatches(ui, &recent, &mut app.tools.color);
        }
        ui.add_space(6.0);
        ui.separator();
        ui.checkbox(&mut app.tools.pick_layer_only, tr("Cuentagotas: solo la capa actual"));
        ui.separator();
        ui.label(tr("Relleno y varita mágica"));
        ui.checkbox(&mut app.tools.fill_all_layers, tr("Detectar bordes de todas las capas"))
            .on_hover_text(tr("Si alguna capa está marcada como «Líneas para el relleno», se usa esa"));
        ui.checkbox(&mut app.tools.fill_every_drawing, tr("Rellenar en todos los dibujos de la capa"))
            .on_hover_text(tr("Un toque rellena el mismo punto en cada dibujo de esta capa (útil para colorear fondos o personajes quietos)"));
        let mut tol = app.tools.fill.tolerance as i32;
        if ui.add(egui::Slider::new(&mut tol, 0..=255).text(tr("Tolerancia"))).changed() {
            app.tools.fill.tolerance = tol as u8;
        }
        ui.add(egui::Slider::new(&mut app.tools.fill.grow, 0..=4).text(tr("Expandir (px)")));
        ui.add(egui::Slider::new(&mut app.tools.fill.gap_close, 0..=8).text(tr("Cerrar huecos (px)")))
            .on_hover_text(tr("Rellena aunque la línea tenga pequeñas aberturas"));
    });
}

fn swatches(ui: &mut egui::Ui, colors: &[Color32], current: &mut Color32) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(8.0);
        for &c in colors {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), egui::Sense::click());
            ui.painter().circle_filled(rect.center(), 17.0, c);
            ui.painter().circle_stroke(rect.center(), 17.0, egui::Stroke::new(1.0, crate::theme::SEPARATOR));
            if *current == c {
                ui.painter().circle_stroke(rect.center(), 20.0, egui::Stroke::new(2.5, crate::theme::ACCENT));
            }
            if resp.clicked() {
                *current = c;
            }
        }
    });
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
