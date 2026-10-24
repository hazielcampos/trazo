//! Top bar: project menu, undo/redo, playback, view options.

use egui::RichText;

use crate::app::{TrazoApp, Redraw};
use crate::icons::{Icon, IconButton};
use crate::panels::SideTab;

pub fn show(app: &mut TrazoApp, ui: &mut egui::Ui) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            let ctx = ui.ctx().clone();
            let menu = ui.add(IconButton::new(Icon::Menu).text("Proyecto").size(32.0));
            egui::Popup::menu(&menu).show(|ui| {
                ui.set_min_width(220.0);
                if ui.add(IconButton::new(Icon::New).text("Nuevo proyecto…")).clicked() {
                    app.ui.new_project_dialog = Some(Default::default());
                    ui.close();
                }
                if ui.add(IconButton::new(Icon::Open).text("Abrir…")).clicked() {
                    app.ui.open_dialog = true;
                    ui.close();
                }
                if ui.add(IconButton::new(Icon::Save).text("Guardar (Ctrl+S)")).clicked() {
                    app.save_async(true);
                    ui.close();
                }
                if ui.add(IconButton::new(Icon::Export).text("Exportar video, GIF… (Ctrl+E)")).clicked() {
                    app.export_ui.open = true;
                    ui.close();
                }
                ui.separator();
                if ui.add(IconButton::new(Icon::Settings).text("Ajustes de animación…")).clicked() {
                    app.ui.settings_dialog = true;
                    ui.close();
                }
            });

            if ui.add(IconButton::new(Icon::Export).text("Exportar").size(32.0).selected(app.export_ui.is_running())).on_hover_text("Exportar a MP4, GIF, PNG, AVI o WAV").clicked() {
                app.export_ui.open = true;
            }

            let title = if app.doc.is_modified() { format!("{} •", app.doc.project.name) } else { app.doc.project.name.clone() };
            ui.label(RichText::new(title).strong());
            ui.separator();

            let can_undo = app.doc.history.can_undo() || app.canvas.select.is_active();
            let can_redo = app.doc.history.can_redo();
            if ui.add_enabled(can_undo, IconButton::new(Icon::Undo).size(32.0)).on_hover_text("Deshacer (Ctrl+Z · 2 dedos)").clicked() {
                app.undo(&ctx);
            }
            if ui.add_enabled(can_redo, IconButton::new(Icon::Redo).size(32.0)).on_hover_text("Rehacer (Ctrl+Y · 3 dedos)").clicked() {
                app.redo(&ctx);
            }
            ui.separator();

            let play = if app.playback.playing { Icon::Pause } else { Icon::Play };
            if ui.add(IconButton::new(play).size(32.0).selected(app.playback.playing)).on_hover_text("Reproducir / pausar (Enter)").clicked() {
                app.toggle_play(&ctx);
            }
            if ui.add(IconButton::new(Icon::Loop).size(32.0).selected(app.playback.looping)).on_hover_text("Repetir en bucle").clicked() {
                app.playback.looping = !app.playback.looping;
            }
            let mut fps = app.doc.project.canvas.fps;
            let r = ui.add(egui::DragValue::new(&mut fps).range(1..=60).suffix(" fps"));
            if r.changed() {
                app.doc.project.canvas.fps = fps;
                app.doc.mark_settings_changed();
            }
            ui.separator();

            if ui.add(IconButton::new(Icon::Onion).text("Cebolla").size(32.0).selected(app.onion.enabled)).on_hover_text("Papel cebolla: ver fotogramas vecinos (O)").clicked() {
                app.onion.enabled = !app.onion.enabled;
                app.doc.redraw = Redraw::Full;
            }
            if ui.add(IconButton::new(Icon::RotateLeft).size(32.0)).on_hover_text("Girar lienzo a la izquierda (Mayús+R)").clicked() {
                app.canvas.rotate_by(-15.0);
            }
            if ui.add(IconButton::new(Icon::RotateRight).size(32.0)).on_hover_text("Girar lienzo a la derecha (R)").clicked() {
                app.canvas.rotate_by(15.0);
            }
            if ui.add(IconButton::new(Icon::FlipH).size(32.0).selected(app.canvas.is_flipped())).on_hover_text("Voltear vista (M) — no cambia el dibujo").clicked() {
                app.canvas.toggle_flip();
            }
            if ui.add(IconButton::new(Icon::Grid).size(32.0).selected(app.tools.show_grid)).on_hover_text("Cuadrícula").clicked() {
                app.tools.show_grid = !app.tools.show_grid;
            }
            let sym = app.tools.symmetry_vertical || app.tools.symmetry_horizontal;
            let sym_btn = ui.add(IconButton::new(Icon::Symmetry).size(32.0).selected(sym)).on_hover_text("Simetría");
            egui::Popup::menu(&sym_btn).show(|ui| {
                ui.checkbox(&mut app.tools.symmetry_vertical, "Espejo izquierda ↔ derecha");
                ui.checkbox(&mut app.tools.symmetry_horizontal, "Espejo arriba ↔ abajo");
            });
            if app.tools.show_grid {
                ui.add(egui::DragValue::new(&mut app.tools.grid_size).range(4..=512).suffix(" px")).on_hover_text("Tamaño de la cuadrícula");
            }
            if ui.add(IconButton::new(Icon::Fit).size(32.0)).on_hover_text("Ajustar a la pantalla y enderezar").clicked() {
                app.canvas.fit();
            }
            let rot = app.canvas.rotation_deg();
            let view = if rot.abs() > 0.5 { format!("{:.0}% · {:.0}°", app.canvas.zoom() * 100.0, rot) } else { format!("{:.0}%", app.canvas.zoom() * 100.0) };
            ui.label(view);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let open = app.ui.show_side_panel;
                for (tab, icon, label) in [(SideTab::Color, Icon::Palette, "Color"), (SideTab::Brushes, Icon::Brush, "Pinceles"), (SideTab::Layers, Icon::Layers, "Capas")] {
                    if ui.add(IconButton::new(icon).text(label).size(32.0).selected(open && app.ui.side_tab == tab)).clicked() {
                        toggle_tab(app, tab);
                    }
                }
            });
        });
        ui.add_space(2.0);
    });
}

fn toggle_tab(app: &mut TrazoApp, tab: SideTab) {
    if app.ui.show_side_panel && app.ui.side_tab == tab {
        app.ui.show_side_panel = false;
    } else {
        app.ui.show_side_panel = true;
        app.ui.side_tab = tab;
    }
}
