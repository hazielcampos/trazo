//! Floating action bar for the selection tools.

use crate::app::TrazoApp;
use crate::icons::{Icon, IconButton};

pub fn show(app: &mut TrazoApp, ctx: &egui::Context) {
    let active = app.canvas.select.is_active();
    if !(app.tools.tool.is_selection() || active) || app.playback.playing {
        return;
    }
    egui::Area::new(egui::Id::unique("selection_bar"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -175.0])
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let doc = &mut app.doc;
                    let sel = &mut app.canvas.select;
                    if !active {
                        ui.label("Arrastra sobre el dibujo para seleccionar");
                        if ui.add(IconButton::new(Icon::SelectRect).text("Todo")).on_hover_text("Seleccionar todo lo de la capa (Ctrl+A)").clicked() {
                            if let Some(m) = sel.select_all(doc) {
                                app.toast(ctx, m);
                            }
                        }
                    } else {
                        if ui.add(IconButton::new(Icon::FlipH)).on_hover_text("Voltear horizontal").clicked() {
                            sel.flip(doc, true);
                        }
                        if ui.add(IconButton::new(Icon::FlipV)).on_hover_text("Voltear vertical").clicked() {
                            sel.flip(doc, false);
                        }
                        if ui.add(IconButton::new(Icon::RotateRight)).on_hover_text("Girar 90°").clicked() {
                            sel.rotate90(doc);
                        }
                        ui.separator();
                        if ui.add(IconButton::new(Icon::Duplicate)).on_hover_text("Duplicar selección").clicked() {
                            sel.duplicate(doc);
                        }
                        if ui.add(IconButton::new(Icon::Copy)).on_hover_text("Copiar (Ctrl+C) — pégalo en otro fotograma").clicked() {
                            sel.copy();
                            app.toast(ctx, "Copiado: cambia de fotograma o capa y pulsa Pegar");
                        }
                        let sel = &mut app.canvas.select;
                        if ui.add(IconButton::new(Icon::Cut)).on_hover_text("Cortar (Ctrl+X)").clicked() {
                            sel.cut(&mut app.doc);
                        }
                        if ui.add(IconButton::new(Icon::Trash)).on_hover_text("Borrar contenido (Supr)").clicked() {
                            sel.delete(&mut app.doc);
                        }
                    }
                    let has_clip = app.canvas.select.has_clipboard();
                    if ui.add_enabled(has_clip, IconButton::new(Icon::Paste).text("Pegar")).on_hover_text("Pegar en la capa y fotograma actuales (Ctrl+V)").clicked() {
                        app.paste_selection(ctx);
                    }
                    if app.canvas.select.is_active() {
                        ui.separator();
                        if ui.add(IconButton::new(Icon::Check).keep_text().text("Aplicar").selected(true)).on_hover_text("Fijar (Enter)").clicked() {
                            app.canvas.select.commit(&mut app.doc);
                        }
                        if ui.add(IconButton::new(Icon::Close)).on_hover_text("Cancelar (Esc)").clicked() {
                            app.canvas.select.cancel(&mut app.doc);
                        }
                    }
                });
            });
        });
}
