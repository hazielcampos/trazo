//! Floating action bar for the selection tools.
//!
//! On narrow screens the less used actions (flip, rotate, duplicate, copy,
//! cut, delete, paste) go into a "⋯" menu so the bar always fits.

use crate::app::TrazoApp;
use crate::i18n::tr;
use crate::icons::{Icon, IconButton};

pub fn show(app: &mut TrazoApp, ctx: &egui::Context) {
    let active = app.canvas.select.is_active();
    if !(app.tools.tool.is_selection() || active) || app.playback.playing {
        return;
    }
    let narrow = ctx.content_rect().width() < 640.0;
    egui::Area::new(egui::Id::unique("selection_bar"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -175.0])
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Recording: only the state and a way out.
                    if let Some(n) = app.canvas.select.recording() {
                        let text = if n == 0 { tr("Arrastra la selección para grabar").to_string() } else { tf!("● Grabando… {} dibujos", n) };
                        ui.label(egui::RichText::new(text).color(egui::Color32::from_rgb(240, 80, 80)).strong());
                        if ui.add(IconButton::new(Icon::Close).keep_text().text(tr("Parar"))).on_hover_text(tr("Dejar de grabar sin crear dibujos")).clicked() {
                            app.canvas.select.cancel_recording();
                        }
                        return;
                    }
                    if !active {
                        if !narrow {
                            ui.label(tr("Arrastra sobre el dibujo para seleccionar"));
                        }
                        if ui.add(IconButton::new(Icon::SelectRect).keep_text().text(tr("Todo"))).on_hover_text(tr("Seleccionar todo lo de la capa (Ctrl+A)")).clicked() {
                            if let Some(m) = app.canvas.select.select_all(&mut app.doc) {
                                app.toast(ctx, m);
                            }
                        }
                        paste_button(app, ui, ctx);
                        return;
                    }
                    if narrow {
                        let more = ui.add(IconButton::new(Icon::More)).on_hover_text(tr("Más acciones"));
                        egui::Popup::menu(&more).show(|ui| {
                            ui.set_min_width(220.0);
                            edit_actions(app, ui, ctx, true);
                        });
                    } else {
                        edit_actions(app, ui, ctx, false);
                    }
                    ui.separator();
                    if ui.add(IconButton::new(Icon::Record).keep_text().text(tr("Grabar movimiento"))).on_hover_text(tr("Grabar movimiento: arrastra la selección y cada momento se vuelve un dibujo")).clicked() {
                        if let Err(e) = app.canvas.select.arm_recording(&app.doc) {
                            app.toast(ctx, e);
                        }
                    }
                    let can = app.canvas.select.can_tween(&app.doc);
                    let tip = if can { tr("Generar los dibujos intermedios hasta esta posición") } else { tr("Mueve, escala o gira la selección para marcar la posición final") };
                    if ui.add_enabled(can, IconButton::new(Icon::Tween).keep_text().text(tr("Intermedios"))).on_hover_text(tip).on_disabled_hover_text(tip).clicked() {
                        app.ui.tween_dialog = Some(Default::default());
                    }
                    ui.separator();
                    if ui.add(IconButton::new(Icon::Check).keep_text().text(tr("Aplicar")).selected(true)).on_hover_text(tr("Fijar (Enter)")).clicked() {
                        app.canvas.select.commit(&mut app.doc);
                    }
                    if ui.add(IconButton::new(Icon::Close)).on_hover_text(tr("Cancelar (Esc)")).clicked() {
                        app.canvas.select.cancel(&mut app.doc);
                    }
                });
            });
        });
}

/// Flip, rotate, duplicate, copy, cut, delete and paste. `menu` lays them
/// out as a list with labels (the "⋯" menu on phones).
fn edit_actions(app: &mut TrazoApp, ui: &mut egui::Ui, ctx: &egui::Context, menu: bool) {
    fn btn(icon: Icon, label: &str, menu: bool) -> IconButton<'_> {
        let b = IconButton::new(icon);
        if menu { b.keep_text().text(label).size(40.0) } else { b }
    }
    let b = |icon: Icon, label: &'static str| btn(icon, label, menu);
    let sel = &mut app.canvas.select;
    let doc = &mut app.doc;
    if ui.add(b(Icon::FlipH, tr("Voltear horizontal"))).on_hover_text(tr("Voltear horizontal")).clicked() {
        sel.flip(doc, true);
    }
    if ui.add(b(Icon::FlipV, tr("Voltear vertical"))).on_hover_text(tr("Voltear vertical")).clicked() {
        sel.flip(doc, false);
    }
    if ui.add(b(Icon::RotateRight, tr("Girar 90°"))).on_hover_text(tr("Girar 90°")).clicked() {
        sel.rotate90(doc);
    }
    if !menu {
        ui.separator();
    }
    if ui.add(b(Icon::Duplicate, tr("Duplicar selección"))).on_hover_text(tr("Duplicar selección")).clicked() {
        sel.duplicate(doc);
    }
    if ui.add(b(Icon::Copy, tr("Copiar"))).on_hover_text(tr("Copiar (Ctrl+C) — pégalo en otro fotograma")).clicked() {
        sel.copy();
        app.toast(ctx, tr("Copiado: cambia de fotograma o capa y pulsa Pegar"));
    }
    let sel = &mut app.canvas.select;
    if ui.add(b(Icon::Cut, tr("Cortar"))).on_hover_text(tr("Cortar (Ctrl+X)")).clicked() {
        sel.cut(&mut app.doc);
    }
    if ui.add(b(Icon::Trash, tr("Borrar contenido"))).on_hover_text(tr("Borrar contenido (Supr)")).clicked() {
        sel.delete(&mut app.doc);
    }
    paste_button(app, ui, ctx);
}

fn paste_button(app: &mut TrazoApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let has_clip = app.canvas.select.has_clipboard();
    if ui.add_enabled(has_clip, IconButton::new(Icon::Paste).text(tr("Pegar"))).on_hover_text(tr("Pegar en la capa y fotograma actuales (Ctrl+V)")).clicked() {
        app.paste_selection(ctx);
    }
}
