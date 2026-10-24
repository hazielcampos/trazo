//! Bottom timeline: frame thumbnails, frame operations and onion skin settings.

use std::collections::HashMap;

use trazo_core::composite::render_thumbnail;
use trazo_core::{ops, FrameId};
use egui::{Color32, ColorImage, Rect, RichText, TextureHandle, TextureOptions, Vec2};

use crate::app::{TrazoApp, Document, Redraw};
use crate::icons::{Icon, IconButton};

const THUMB_W: u32 = 96;
/// Thumbnails regenerated per UI frame (keeps the UI responsive).
const THUMBS_PER_FRAME: usize = 2;

#[derive(Default)]
pub struct ThumbCache {
    map: HashMap<FrameId, TextureHandle>,
}

impl ThumbCache {
    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn update(&mut self, ctx: &egui::Context, doc: &mut Document, busy: bool) {
        self.map.retain(|id, _| doc.project.frame_index(*id).is_some());
        doc.stale_frames.retain(|id| doc.project.frame_index(*id).is_some());
        if busy || doc.stale_frames.is_empty() {
            return;
        }
        // Current frame first, then the rest.
        let current = doc.frame_id();
        let mut todo: Vec<FrameId> = Vec::new();
        if doc.stale_frames.contains(&current) {
            todo.push(current);
        }
        todo.extend(doc.stale_frames.iter().copied().filter(|id| *id != current).take(THUMBS_PER_FRAME));
        todo.truncate(THUMBS_PER_FRAME);
        for id in todo {
            doc.stale_frames.remove(&id);
            let Some(idx) = doc.project.frame_index(id) else { continue };
            let (w, h, data) = render_thumbnail(&doc.project, idx, THUMB_W);
            let img = ColorImage::from_rgba_premultiplied([w, h], &data);
            match self.map.get_mut(&id) {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => {
                    let t = ctx.load_texture(format!("thumb{id}"), img, TextureOptions::LINEAR);
                    self.map.insert(id, t);
                }
            }
        }
        if !doc.stale_frames.is_empty() {
            ctx.request_repaint();
        }
    }
}

pub fn add_frame(app: &mut TrazoApp) {
    app.stop_playback();
    let d = &mut app.doc;
    let idx = ops::insert_blank_frame(&mut d.project, &mut d.history, d.frame + 1);
    d.invalidate_all();
    d.set_frame(idx);
}

pub fn show(app: &mut TrazoApp, ui: &mut egui::Ui) {
    egui::Panel::bottom("timeline").resizable(false).show(ui, |ui| {
        commit_selection_on_click(app, ui);
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| controls(app, ui));
        ui.add_space(2.0);
        frames_strip(app, ui, THUMB_W as f32, 90.0);
        ui.add_space(4.0);
    });
}

fn controls(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let mut tab = usize::from(app.audio_ui.show);
    if crate::widgets::segmented(ui, &mut tab, &["Fotogramas", "Audio"], 220.0) {
        app.audio_ui.show = tab == 1;
        ui.ctx().request_repaint();
    }
    ui.separator();
    let n = app.doc.project.frames.len();
    let cur = app.displayed_frame();
    ui.label(RichText::new(format!("Fotograma {} / {}", cur + 1, n)).strong());
    ui.separator();

    let mut goto = None;
    if ui.add(IconButton::new(Icon::First)).on_hover_text("Primero").clicked() {
        goto = Some(Nav::First);
    }
    if ui.add(IconButton::new(Icon::Prev)).on_hover_text("Anterior (,)").clicked() {
        goto = Some(Nav::Prev);
    }
    if ui.add(IconButton::new(Icon::Next)).on_hover_text("Siguiente (.)").clicked() {
        goto = Some(Nav::Next);
    }
    if ui.add(IconButton::new(Icon::Last)).on_hover_text("Último").clicked() {
        goto = Some(Nav::Last);
    }
    if let Some(g) = goto {
        let shown = app.displayed_frame();
        app.stop_playback();
        let g = match g {
            Nav::First => 0,
            Nav::Prev => shown.saturating_sub(1),
            Nav::Next => shown + 1,
            Nav::Last => n - 1,
        };
        app.doc.set_frame(g);
    }
    ui.separator();

    if ui.add(IconButton::new(Icon::Plus).text("Nuevo")).on_hover_text("Fotograma vacío después del actual (N)").clicked() {
        add_frame(app);
    }
    if ui.add(IconButton::new(Icon::Plus).text("Antes")).on_hover_text("Fotograma vacío antes del actual").clicked() {
        insert_before(app);
    }
    let multi = app.frame_selection.len() > 1;
    let dup_tip = if multi { "Duplicar los fotogramas seleccionados" } else { "Duplicar fotograma" };
    if ui.add(IconButton::new(Icon::Duplicate)).on_hover_text(dup_tip).clicked() {
        duplicate_frames(app);
    }
    if ui.add(IconButton::new(Icon::Copy)).on_hover_text("Copiar fotograma").clicked() {
        copy_frame(app, ui.ctx());
    }
    if ui.add_enabled(app.frame_clipboard.is_some(), IconButton::new(Icon::Paste)).on_hover_text("Pegar fotograma después del actual").clicked() {
        paste_frame(app, ui.ctx());
    }
    let del_tip = if multi { "Borrar los fotogramas seleccionados" } else { "Borrar fotograma" };
    if ui.add(IconButton::new(Icon::Trash)).on_hover_text(del_tip).clicked() {
        delete_frames(app);
    }
    if ui.add_enabled(app.doc.frame > 0, IconButton::new(Icon::ArrowLeft)).on_hover_text("Mover a la izquierda").clicked() {
        move_frame_by(app, -1);
    }
    if ui.add_enabled(app.doc.frame + 1 < n, IconButton::new(Icon::ArrowRight)).on_hover_text("Mover a la derecha").clicked() {
        move_frame_by(app, 1);
    }
    // Hold ("exposición"): one undo step per click.
    let hold = app.doc.project.frames[app.doc.frame].hold;
    ui.label("Duración").on_hover_text("Cuántos tiempos dura este dibujo (2 = «a doses»)");
    if ui.add_enabled(hold > 1, IconButton::new(Icon::Minus).size(26.0)).clicked() {
        change_hold(app, -1);
    }
    ui.label(format!("×{hold}"));
    if ui.add_enabled(hold < 48, IconButton::new(Icon::Plus).size(26.0)).clicked() {
        change_hold(app, 1);
    }
    ui.separator();

    // Playback range and ping-pong.
    let cur = app.doc.frame;
    if ui.add(IconButton::new(Icon::RangeIn)).on_hover_text("Inicio del rango de reproducción = fotograma actual").clicked() {
        let (_, b) = app.playback.range.unwrap_or((0, n - 1));
        app.playback.range = Some((cur, b.max(cur)));
    }
    if ui.add(IconButton::new(Icon::RangeOut)).on_hover_text("Fin del rango de reproducción = fotograma actual").clicked() {
        let (a, _) = app.playback.range.unwrap_or((0, n - 1));
        app.playback.range = Some((a.min(cur), cur));
    }
    if app.playback.range.is_some() && ui.add(IconButton::new(Icon::Close)).on_hover_text("Quitar el rango (reproducir todo)").clicked() {
        app.playback.range = None;
    }
    if ui.add(IconButton::new(Icon::PingPong).selected(app.playback.ping_pong)).on_hover_text("Ping-pong: ida y vuelta").clicked() {
        app.playback.ping_pong = !app.playback.ping_pong;
    }
    ui.separator();

    let o = &mut app.onion;
    let mut changed = false;
    let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(22.0), egui::Sense::hover());
    crate::icons::paint(ui.painter(), r, Icon::Onion, ui.visuals().text_color());
    changed |= ui.add(egui::DragValue::new(&mut o.before).range(0..=5).prefix("antes ")).on_hover_text("Fotogramas anteriores").changed();
    changed |= ui.add(egui::DragValue::new(&mut o.after).range(0..=5).prefix("después ")).on_hover_text("Fotogramas siguientes").changed();
    changed |= ui.add(egui::Slider::new(&mut o.opacity, 0.05..=0.9).show_value(false)).on_hover_text("Opacidad del papel cebolla").changed();
    changed |= ui.checkbox(&mut o.keep_colors, "Colores reales").changed();
    if changed {
        app.doc.redraw = Redraw::Full;
    }
    ui.label(format!("{:.1} s", app.doc.project.duration_secs()));
}

/// Thumbnails of every frame (`thumb_w` wide): tap to go, long-press to
/// pick several, drag to reorder, "+" to add at the end.
pub fn frames_strip(app: &mut TrazoApp, ui: &mut egui::Ui, thumb_w: f32, max_h: f32) {
    let current = app.displayed_frame();
    let (cw, ch) = (app.doc.project.canvas.width as f32, app.doc.project.canvas.height as f32);
    let thumb_h = (thumb_w * ch / cw).clamp(30.0, max_h);
    let n = app.doc.project.frames.len();
    let range = app.playback.range.map(|_| app.playback.bounds(n));
    let mods = ui.input(|i| i.modifiers);
    let mut action: Option<StripAction> = None;
    let drag_id = egui::Id::unique("frame_drag");
    let dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    let mut rects: Vec<egui::Rect> = Vec::with_capacity(n);
    egui::ScrollArea::horizontal().id_salt("frames").show(ui, |ui| {
        ui.horizontal(|ui| {
            for (i, f) in app.doc.project.frames.iter().enumerate() {
                let size = Vec2::new(thumb_w, thumb_h);
                let (rect, resp) = ui.allocate_exact_size(size + Vec2::new(0.0, 16.0), egui::Sense::click_and_drag());
                rects.push(rect);
                let img_rect = egui::Rect::from_min_size(rect.min, size);
                let p = ui.painter();
                let dim = dragging == Some(i);
                p.rect_filled(img_rect, 8.0, if dim { Color32::from_gray(150) } else { Color32::WHITE });
                if let Some(t) = app.thumbs.map.get(&f.id) {
                    let tint = if dim { Color32::from_white_alpha(90) } else { Color32::WHITE };
                    p.image(t.id(), img_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
                }
                // Frames outside the playback range are greyed out.
                if range.is_some_and(|(a, b)| i < a || i > b) {
                    p.rect_filled(img_rect, 4.0, Color32::from_black_alpha(120));
                }
                let picked = app.frame_selection.contains(&i);
                if picked {
                    p.rect_filled(img_rect, 4.0, crate::theme::ACCENT.gamma_multiply(0.25));
                }
                let stroke = if i == current {
                    egui::Stroke::new(2.5, crate::theme::ACCENT)
                } else if picked {
                    egui::Stroke::new(2.0, crate::theme::ACCENT.gamma_multiply(0.7))
                } else {
                    egui::Stroke::NONE
                };
                p.rect_stroke(img_rect.expand(if i == current { 2.0 } else { 0.0 }), 9.0, stroke, egui::StrokeKind::Outside);
                let label = if f.hold > 1 { format!("{}  ×{}", i + 1, f.hold) } else { format!("{}", i + 1) };
                p.text(egui::pos2(img_rect.center().x, img_rect.max.y + 8.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), ui.visuals().text_color());
                if i == current && app.playback.playing {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.drag_started() {
                    ui.data_mut(|d| d.insert_temp(drag_id, i));
                }
                if resp.long_touched() {
                    action = Some(StripAction::Toggle(i));
                } else if resp.clicked() {
                    action = Some(if mods.command {
                        StripAction::Toggle(i)
                    } else if mods.shift {
                        StripAction::Extend(i)
                    } else {
                        StripAction::Select(i)
                    });
                }
            }
            // "+" slot at the end, like FlipaClip.
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(44.0, thumb_h), egui::Sense::click());
            ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(1.5, crate::theme::TEXT3), egui::StrokeKind::Inside);
            crate::icons::paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(20.0)), Icon::Plus, crate::theme::TEXT);
            if resp.on_hover_text("Añadir fotograma al final").clicked() {
                action = Some(StripAction::Append);
            }

            // Drag to reorder: show where the frame will land.
            if let Some(from) = dragging {
                let pointer = ui.input(|i| i.pointer.interact_pos());
                let to = pointer.map(|p| rects.iter().filter(|r| r.center().x < p.x).count()).unwrap_or(from);
                if let Some(r) = rects.get(to).or(rects.last()) {
                    let x = if to < rects.len() { r.min.x - 3.0 } else { r.max.x + 3.0 };
                    ui.painter().line_segment([egui::pos2(x, r.min.y), egui::pos2(x, r.min.y + thumb_h)], egui::Stroke::new(3.0, crate::theme::ACCENT));
                }
                if ui.input(|i| !i.pointer.any_down()) {
                    ui.data_mut(|d| d.remove::<usize>(drag_id));
                    // Inserting after itself is a no-op.
                    let target = if to > from { to - 1 } else { to };
                    if target != from {
                        action = Some(StripAction::Move(from, target));
                    }
                }
            }
        });
    });
    match action {
        Some(StripAction::Append) => {
            app.stop_playback();
            let d = &mut app.doc;
            let idx = ops::insert_blank_frame(&mut d.project, &mut d.history, n);
            d.invalidate_all();
            d.set_frame(idx);
            app.frame_selection.clear();
        }
        Some(StripAction::Select(i)) => {
            app.stop_playback();
            app.doc.set_frame(i);
            app.frame_selection.clear();
        }
        Some(StripAction::Toggle(i)) => {
            if app.frame_selection.is_empty() {
                app.frame_selection.insert(app.doc.frame);
            }
            if !app.frame_selection.remove(&i) {
                app.frame_selection.insert(i);
            }
        }
        Some(StripAction::Extend(i)) => {
            let a = app.doc.frame;
            app.frame_selection = (a.min(i)..=a.max(i)).collect();
        }
        Some(StripAction::Move(from, to)) => {
            app.stop_playback();
            let d = &mut app.doc;
            ops::move_frame(&mut d.project, &mut d.history, from, to);
            d.invalidate_all();
            d.set_frame(to);
            app.frame_selection.clear();
        }
        None => {}
    }
}

enum StripAction {
    Append,
    Select(usize),
    Toggle(usize),
    Extend(usize),
    Move(usize, usize),
}

#[derive(Clone, Copy)]
enum Nav {
    First,
    Prev,
    Next,
    Last,
}

/// Clicking a panel applies any floating selection first, so frame and layer
/// operations always see the finished drawing.
pub fn commit_selection_on_click(app: &mut TrazoApp, ui: &egui::Ui) {
    if app.canvas.select.is_active() && ui.ui_contains_pointer() && ui.input(|i| i.pointer.any_pressed()) {
        app.canvas.select.commit(&mut app.doc);
    }
}

// ---- frame operations (shared by the tablet bar and the phone menu) ----------

/// Empty frame before the current one.
pub fn insert_before(app: &mut TrazoApp) {
    app.stop_playback();
    let d = &mut app.doc;
    let idx = ops::insert_blank_frame(&mut d.project, &mut d.history, d.frame);
    d.invalidate_all();
    d.frame = idx;
    d.redraw = Redraw::Full;
}

/// Duplicate the current frame, or every picked frame.
pub fn duplicate_frames(app: &mut TrazoApp) {
    app.stop_playback();
    let multi = app.frame_selection.len() > 1;
    let sel: Vec<usize> = if multi { app.frame_selection.iter().copied().collect() } else { vec![app.doc.frame] };
    let d = &mut app.doc;
    let mark = d.history.undo_len();
    let after = *sel.last().expect("non-empty");
    for (k, idx) in sel.iter().enumerate() {
        let mut f = d.project.frames[*idx].clone();
        f.id = d.project.alloc_id();
        d.history.execute(&mut d.project, trazo_core::Edit::InsertFrame { index: after + 1 + k, frame: f });
    }
    d.history.group_since(mark);
    d.invalidate_all();
    d.set_frame(after + 1);
    app.frame_selection.clear();
}

pub fn copy_frame(app: &mut TrazoApp, ctx: &egui::Context) {
    app.frame_clipboard = Some(app.doc.project.frames[app.doc.frame].clone());
    app.toast(ctx, "Fotograma copiado");
}

/// Paste the copied frame after the current one.
pub fn paste_frame(app: &mut TrazoApp, ctx: &egui::Context) {
    app.stop_playback();
    let (cw, ch) = (app.doc.project.canvas.width, app.doc.project.canvas.height);
    if app.frame_clipboard.as_ref().is_some_and(|f| f.cels.values().any(|c| c.width() != cw || c.height() != ch)) {
        app.toast(ctx, "Ese fotograma es de un lienzo de otro tamaño");
    } else if let Some(mut f) = app.frame_clipboard.clone() {
        let d = &mut app.doc;
        // Drop cels of layers that no longer exist.
        f.cels.retain(|lid, _| d.project.layer_index(*lid).is_some());
        f.id = d.project.alloc_id();
        let index = d.frame + 1;
        d.history.execute(&mut d.project, trazo_core::Edit::InsertFrame { index, frame: f });
        d.invalidate_all();
        d.set_frame(index);
    }
}

/// Delete the current frame, or every picked frame.
pub fn delete_frames(app: &mut TrazoApp) {
    app.stop_playback();
    let multi = app.frame_selection.len() > 1;
    let sel: Vec<usize> = if multi { app.frame_selection.iter().copied().collect() } else { vec![app.doc.frame] };
    let d = &mut app.doc;
    let mark = d.history.undo_len();
    for idx in sel.iter().rev() {
        ops::delete_frame(&mut d.project, &mut d.history, *idx);
    }
    d.history.group_since(mark);
    d.invalidate_all();
    app.frame_selection.clear();
}

/// Move the current frame one place left (-1) or right (+1).
pub fn move_frame_by(app: &mut TrazoApp, delta: i32) {
    let d = &mut app.doc;
    let n = d.project.frames.len();
    let to = d.frame as i64 + delta as i64;
    if to < 0 || to >= n as i64 {
        return;
    }
    ops::move_frame(&mut d.project, &mut d.history, d.frame, to as usize);
    d.frame = to as usize;
    d.invalidate_all();
}

/// Lengthen or shorten how long the current drawing stays on screen.
pub fn change_hold(app: &mut TrazoApp, delta: i32) {
    let d = &mut app.doc;
    let hold = d.project.frames[d.frame].hold as i32;
    let new = (hold + delta).clamp(1, 48) as u32;
    if new as i32 != hold {
        ops::set_frame_hold(&mut d.project, &mut d.history, d.frame, new);
        d.timing_changed();
    }
}
