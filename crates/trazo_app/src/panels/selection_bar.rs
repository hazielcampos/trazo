//! Floating action bar for the selection tools.

use crate::i18n::tr;
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
                        ui.label(tr("Arrastra sobre el dibujo para seleccionar"));
                        if ui.add(IconButton::new(Icon::SelectRect).text(tr("Todo"))).on_hover_text(tr("Seleccionar todo lo de la capa (Ctrl+A)")).clicked() {
                            if let Some(m) = sel.select_all(doc) {
                                app.toast(ctx, m);
                            }
                        }
                    } else {
                        if ui.add(IconButton::new(Icon::FlipH)).on_hover_text(tr("Voltear horizontal")).clicked() {
                            sel.flip(doc, true);
                        }
                        if ui.add(IconButton::new(Icon::FlipV)).on_hover_text(tr("Voltear vertical")).clicked() {
                            sel.flip(doc, false);
                        }
                        if ui.add(IconButton::new(Icon::RotateRight)).on_hover_text(tr("Girar 90°")).clicked() {
                            sel.rotate90(doc);
                        }
                        ui.separator();
                        if ui.add(IconButton::new(Icon::Duplicate)).on_hover_text(tr("Duplicar selección")).clicked() {
                            sel.duplicate(doc);
                        }
                        if ui.add(IconButton::new(Icon::Copy)).on_hover_text(tr("Copiar (Ctrl+C) — pégalo en otro fotograma")).clicked() {
                            sel.copy();
                            app.toast(ctx, tr("Copiado: cambia de fotograma o capa y pulsa Pegar"));
                        }
                        let sel = &mut app.canvas.select;
                        if ui.add(IconButton::new(Icon::Cut)).on_hover_text(tr("Cortar (Ctrl+X)")).clicked() {
                            sel.cut(&mut app.doc);
                        }
                        if ui.add(IconButton::new(Icon::Trash)).on_hover_text(tr("Borrar contenido (Supr)")).clicked() {
                            sel.delete(&mut app.doc);
                        }
                    }
                    let has_clip = app.canvas.select.has_clipboard();
                    if ui.add_enabled(has_clip, IconButton::new(Icon::Paste).text(tr("Pegar"))).on_hover_text(tr("Pegar en la capa y fotograma actuales (Ctrl+V)")).clicked() {
                        app.paste_selection(ctx);
                    }
                    if app.canvas.select.is_active() {
                        ui.separator();
                        let can = app.canvas.select.can_tween(&app.doc);
                        let tip = if can { tr("Generar los dibujos intermedios hasta esta posición") } else { tr("Mueve, escala o gira la selección para marcar la posición final") };
                        if ui.add_enabled(can, IconButton::new(Icon::Tween).keep_text().text(tr("Intermedios"))).on_hover_text(tip).on_disabled_hover_text(tip).clicked() {
                            app.ui.tween_dialog = Some(Default::default());
                        }
                        if ui.add(IconButton::new(Icon::Check).keep_text().text(tr("Aplicar")).selected(true)).on_hover_text(tr("Fijar (Enter)")).clicked() {
                            app.canvas.select.commit(&mut app.doc);
                        }
                        if ui.add(IconButton::new(Icon::Close)).on_hover_text(tr("Cancelar (Esc)")).clicked() {
                            app.canvas.select.cancel(&mut app.doc);
                        }
                    }
                });
            });
        });
}
