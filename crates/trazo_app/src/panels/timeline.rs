//! Timeline with one track per layer (see the design canvas): every layer has
//! its own row of drawings, each lasting as many frames as you stretch it.

use crate::i18n::tr;
use std::collections::HashMap;

use egui::{Align2, Color32, ColorImage, FontId, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use trazo_core::timeline::{self as tl, Block};
use trazo_core::{FrameId, LayerId};

use crate::app::{Document, Redraw, TrazoApp};
use crate::icons::{paint, Icon, IconButton};
use crate::theme::{ACCENT, CONTROL, DANGER, ELEVATED, SEPARATOR, TEXT, TEXT2, TEXT3};

/// Width of a drawing thumbnail.
const THUMB_W: u32 = 48;
/// Thumbnails made per UI frame (keeps the UI responsive).
const THUMBS_PER_FRAME: usize = 3;

/// Thumbnails of drawings, remade when their pixels change.
#[derive(Default)]
pub struct ThumbCache {
    map: HashMap<(FrameId, LayerId), (u64, TextureHandle)>,
    budget: usize,
}

impl ThumbCache {
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Once per UI frame: refill the budget and drop thumbnails of frames that are gone.
    pub fn update(&mut self, _ctx: &egui::Context, doc: &mut Document, busy: bool) {
        self.budget = if busy { 0 } else { THUMBS_PER_FRAME };
        doc.stale_frames.clear();
        if self.map.len() > 4 * doc.project.frames.len().max(1) * doc.project.layers.len().max(1) {
            let alive: std::collections::HashSet<FrameId> = doc.project.frames.iter().map(|f| f.id).collect();
            self.map.retain(|(f, _), _| alive.contains(f));
        }
    }

    /// Thumbnail of the drawing of `layer` that starts in frame `index`.
    fn get(&mut self, ctx: &egui::Context, doc: &Document, index: usize, layer: LayerId) -> Option<TextureHandle> {
        let f = doc.project.frames.get(index)?;
        let cel = f.cel(layer)?;
        let fp = cel.fingerprint();
        let key = (f.id, layer);
        if let Some((have, t)) = self.map.get(&key) {
            if *have == fp || self.budget == 0 {
                return Some(t.clone());
            }
        }
        if self.budget == 0 {
            ctx.request_repaint();
            return None;
        }
        self.budget -= 1;
        let (w, h, data) = trazo_core::composite::cel_thumbnail(cel, THUMB_W);
        let img = ColorImage::from_rgba_premultiplied([w, h], &data);
        let t = match self.map.remove(&key) {
            Some((_, mut t)) => {
                t.set(img, TextureOptions::LINEAR);
                t
            }
            None => ctx.load_texture(format!("cel{}_{}", f.id, layer), img, TextureOptions::LINEAR),
        };
        self.map.insert(key, (fp, t.clone()));
        Some(t)
    }
}

/// View state of the tracks (zoom and the edge being dragged).
pub struct TracksState {
    /// Width of one frame in points.
    pub ppf: f32,
    stretch: Option<Stretch>,
}

impl Default for TracksState {
    fn default() -> Self {
        Self { ppf: 26.0, stretch: None }
    }
}

#[derive(Clone, Copy)]
struct Stretch {
    layer: LayerId,
    start: usize,
    len: usize,
}

/// The drawing selected in the timeline: the current layer's drawing at the playhead.
pub fn selected_block(app: &TrazoApp) -> Option<Block> {
    let d = &app.doc;
    let l = d.project.layer(d.layer)?;
    if l.props.shared || l.props.video.is_some() {
        return None;
    }
    tl::block_at(&d.project, d.layer, d.frame)
}

/// New drawing on the current layer, right after the one at the playhead (N).
pub fn add_frame(app: &mut TrazoApp) {
    app.stop_playback();
    let d = &mut app.doc;
    if d.project.layer(d.layer).is_some_and(|l| l.props.shared || l.props.video.is_some()) {
        return;
    }
    let at = tl::insert_drawing(&mut d.project, &mut d.history, d.layer, d.frame, 1, None);
    d.invalidate_all();
    d.set_frame(at);
}

/// Wide layout: controls on top, tracks below, actions of the selected drawing.
pub fn show(app: &mut TrazoApp, ui: &mut egui::Ui) {
    egui::Panel::bottom("timeline").resizable(false).show(ui, |ui| {
        commit_selection_on_click(app, ui);
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| controls(app, ui));
        ui.add_space(2.0);
        tracks(app, ui, false, 4.5);
        block_actions(app, ui);
        ui.add_space(4.0);
    });
}

fn controls(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let mut tab = usize::from(app.audio_ui.show);
    if crate::widgets::segmented(ui, &mut tab, &[tr("Animación"), tr("Audio")], 220.0) {
        app.audio_ui.show = tab == 1;
        ui.ctx().request_repaint();
    }
    ui.separator();
    let n = app.doc.project.frames.len();
    let cur = app.displayed_frame();
    let fps = app.doc.project.canvas.fps.max(1) as f32;
    ui.label(RichText::new(tf!("Fotograma {} / {}", cur + 1, n)).strong());
    ui.label(RichText::new(tf!("{} s de {} s", crate::i18n::num(cur as f32 / fps, 2), crate::i18n::num(n as f32 / fps, 1))).color(TEXT2));
    ui.separator();
    let mut goto = None;
    if ui.add(IconButton::new(Icon::First)).on_hover_text(tr("Primero")).clicked() {
        goto = Some(0);
    }
    if ui.add(IconButton::new(Icon::Prev)).on_hover_text(tr("Anterior (,)")).clicked() {
        goto = Some(cur.saturating_sub(1));
    }
    if ui.add(IconButton::new(Icon::Next)).on_hover_text(tr("Siguiente (.)")).clicked() {
        goto = Some(cur + 1);
    }
    if ui.add(IconButton::new(Icon::Last)).on_hover_text(tr("Último")).clicked() {
        goto = Some(n - 1);
    }
    if let Some(g) = goto {
        app.stop_playback();
        app.doc.set_frame(g);
    }
    ui.separator();
    // Playback range and ping-pong.
    let cur = app.doc.frame;
    if ui.add(IconButton::new(Icon::RangeIn)).on_hover_text(tr("Inicio del rango de reproducción = fotograma actual")).clicked() {
        let (_, b) = app.playback.range.unwrap_or((0, n - 1));
        app.playback.range = Some((cur, b.max(cur)));
    }
    if ui.add(IconButton::new(Icon::RangeOut)).on_hover_text(tr("Fin del rango de reproducción = fotograma actual")).clicked() {
        let (a, _) = app.playback.range.unwrap_or((0, n - 1));
        app.playback.range = Some((a.min(cur), cur));
    }
    if app.playback.range.is_some() && ui.add(IconButton::new(Icon::Close)).on_hover_text(tr("Quitar el rango (reproducir todo)")).clicked() {
        app.playback.range = None;
    }
    if ui.add(IconButton::new(Icon::PingPong).selected(app.playback.ping_pong)).on_hover_text(tr("Ping-pong: ida y vuelta")).clicked() {
        app.playback.ping_pong = !app.playback.ping_pong;
    }
    ui.separator();
    let o = &mut app.onion;
    let mut changed = false;
    let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
    paint(ui.painter(), r, Icon::Onion, ui.visuals().text_color());
    changed |= ui.add(egui::DragValue::new(&mut o.before).range(0..=5).prefix(tr("antes "))).on_hover_text(tr("Dibujos anteriores")).changed();
    changed |= ui.add(egui::DragValue::new(&mut o.after).range(0..=5).prefix(tr("después "))).on_hover_text(tr("Dibujos siguientes")).changed();
    changed |= ui.add(egui::Slider::new(&mut o.opacity, 0.05..=0.9).show_value(false)).on_hover_text(tr("Opacidad del papel cebolla")).changed();
    changed |= ui.checkbox(&mut o.keep_colors, tr("Colores reales")).changed();
    if changed {
        app.doc.redraw = Redraw::Full;
    }
    ui.separator();
    ui.label(tr("Zoom"));
    ui.add(egui::Slider::new(&mut app.ui.tracks.ppf, 6.0..=48.0).show_value(false)).on_hover_text(tr("Ancho de cada fotograma (también Ctrl+rueda o pellizcar)"));
}

/// One row per layer (top layer first), time running to the right.
/// `rows` is how many rows are visible before the list scrolls.
pub fn tracks(app: &mut TrazoApp, ui: &mut egui::Ui, compact: bool, rows: f32) {
    let ctx = ui.ctx().clone();
    let head_w = if compact { 84.0 } else { 160.0 };
    let row_h = if compact { 44.0 } else { 46.0 };
    let ruler_h = 22.0;
    let n_layers = app.doc.project.layers.len();
    let visible = (n_layers as f32).min(rows);
    let height = ruler_h + visible * row_h + 4.0;

    // Pinch / Ctrl+wheel zoom over the tracks.
    let area = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), height));
    if ui.rect_contains_pointer(area) {
        let z = ui.input(|i| i.zoom_delta());
        if z != 1.0 {
            app.ui.tracks.ppf = (app.ui.tracks.ppf * z).clamp(6.0, 48.0);
        }
    }
    let ppf = app.ui.tracks.ppf;

    egui::ScrollArea::vertical().id_salt("tracks_v").max_height(height).auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            // Track names.
            let (hr, _) = ui.allocate_exact_size(Vec2::new(head_w, ruler_h + n_layers as f32 * row_h), Sense::hover());
            let mut picked_layer = None;
            for (row, li) in (0..n_layers).rev().enumerate() {
                let layer = &app.doc.project.layers[li];
                let r = Rect::from_min_size(Pos2::new(hr.left(), hr.top() + ruler_h + row as f32 * row_h), Vec2::new(head_w, row_h));
                let active = layer.id == app.doc.layer;
                let resp = ui.interact(r, ui.scope_id().with(("track_head", layer.id)), Sense::click());
                let p = ui.painter();
                if active {
                    p.rect_filled(r.shrink2(Vec2::new(0.0, 1.0)), 8.0, crate::theme::accent_soft());
                }
                let dot = if layer.props.video.is_some() || layer.props.shared { Color32::from_rgb(74, 111, 165) } else if active { ACCENT } else { CONTROL };
                p.circle_filled(Pos2::new(r.left() + 10.0, r.center().y), 4.0, dot);
                let name = crate::i18n::layer_name(&layer.props.name).into_owned();
                let font = FontId::proportional(if compact { 12.0 } else { 13.0 });
                let galley = p.layout(name, font, if layer.props.visible { TEXT } else { TEXT3 }, head_w - 24.0);
                p.galley(Pos2::new(r.left() + 20.0, r.center().y - galley.size().y / 2.0), galley, TEXT);
                if resp.clicked() {
                    picked_layer = Some(layer.id);
                }
            }
            if let Some(l) = picked_layer {
                app.doc.layer = l;
            }

            // Lanes, scrolling in time.
            let n_frames = app.doc.project.frames.len();
            let content_w = (n_frames as f32 + 4.0) * ppf + 60.0;
            egui::ScrollArea::horizontal().id_salt("tracks_h").auto_shrink([false, true]).show(ui, |ui| {
                lanes(app, ui, &ctx, content_w, ruler_h, row_h, ppf, compact);
            });
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn lanes(app: &mut TrazoApp, ui: &mut egui::Ui, ctx: &egui::Context, content_w: f32, ruler_h: f32, row_h: f32, ppf: f32, compact: bool) {
    let n_layers = app.doc.project.layers.len();
    let (rect, bg) = ui.allocate_exact_size(Vec2::new(content_w, ruler_h + n_layers as f32 * row_h), Sense::click_and_drag());
    let x_of = |f: f32| rect.left() + f * ppf;
    let frame_at = |x: f32| (((x - rect.left()) / ppf).floor().max(0.0)) as usize;
    let n_frames = app.doc.project.frames.len();
    let fps = app.doc.project.canvas.fps.max(1) as usize;
    let shown = app.displayed_frame();

    // Ruler: frame numbers, seconds in bold.
    {
        let p = ui.painter();
        let ruler = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ruler_h));
        p.hline(ruler.x_range(), ruler.bottom(), Stroke::new(1.0, SEPARATOR));
        let label_every = [1usize, 2, 4, 5, 10, 20, 50].into_iter().find(|k| *k as f32 * ppf >= 34.0).unwrap_or(100);
        for f in 0..=n_frames {
            let x = x_of(f as f32);
            let sec = f % fps == 0 && f > 0;
            let tick_h = if sec { 10.0 } else if f % label_every == 0 { 6.0 } else { 3.0 };
            if ppf >= 6.0 || sec {
                p.vline(x, (ruler.bottom() - tick_h)..=ruler.bottom(), Stroke::new(1.0, if sec { TEXT2 } else { TEXT3 }));
            }
            if sec {
                p.text(Pos2::new(x + 3.0, ruler.top() + 7.0), Align2::LEFT_CENTER, format!("{} s", f / fps), FontId::proportional(10.0), TEXT);
            } else if f % label_every == 0 && f < n_frames {
                p.text(Pos2::new(x + 3.0, ruler.top() + 7.0), Align2::LEFT_CENTER, format!("{}", f + 1), FontId::proportional(10.0), TEXT3);
            }
        }
        // End of the animation.
        let end = x_of(n_frames as f32);
        p.vline(end, rect.top()..=rect.bottom(), Stroke::new(1.0, SEPARATOR));
    }

    let mut set_frame: Option<(usize, Option<LayerId>)> = None;
    let mut commit_stretch: Option<Stretch> = None;
    let mut append: Option<LayerId> = None;
    let selected = selected_block(app);
    let stretch = app.ui.tracks.stretch;

    for (row, li) in (0..n_layers).rev().enumerate() {
        let layer = app.doc.project.layers[li].clone();
        let top = rect.top() + ruler_h + row as f32 * row_h;
        let lane = Rect::from_min_size(Pos2::new(rect.left(), top), Vec2::new(rect.width(), row_h));
        let active = layer.id == app.doc.layer;
        if active {
            ui.painter().rect_filled(lane, 0.0, Color32::from_rgba_unmultiplied(140, 123, 255, 14));
        }
        let pad = 3.0;
        let bar = |a: f32, b: f32| Rect::from_min_max(Pos2::new(x_of(a) + 1.0, top + pad), Pos2::new(x_of(b) - 1.0, top + row_h - pad));

        if let Some(v) = &layer.props.video {
            let a = v.start.max(0) as f32;
            let b = (v.end_tick(app.doc.project.canvas.fps).max(0) as f32).max(a + 1.0);
            let r = bar(a, b);
            ui.painter().rect_filled(r, 6.0, Color32::from_rgb(45, 75, 115));
            ui.painter().text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, tr("Video"), FontId::proportional(11.0), Color32::from_rgb(181, 233, 255));
            continue;
        }
        if layer.props.shared {
            let r = bar(0.0, n_frames as f32);
            ui.painter().rect_filled(r, 6.0, Color32::from_rgb(46, 59, 82));
            ui.painter().text(Pos2::new(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, tr("Fondo fijo · todo el plano"), FontId::proportional(11.0), Color32::from_rgb(201, 216, 240));
            continue;
        }

        let blocks = tl::blocks(&app.doc.project, layer.id);
        let mut offset = 0isize; // how far later drawings move while stretching
        for b in &blocks {
            let is_sel = active && selected == Some(*b);
            let mut len = b.len;
            if let Some(s) = stretch.filter(|s| s.layer == layer.id && s.start == b.start) {
                offset = s.len as isize - b.len as isize;
                len = s.len;
            }
            let start = if stretch.is_some_and(|s| s.layer == layer.id && b.start > s.start) { (b.start as isize + offset).max(0) as usize } else { b.start };
            let r = bar(start as f32, (start + len) as f32);
            let p = ui.painter();
            p.rect_filled(r, 6.0, if is_sel { ACCENT } else { ELEVATED });
            if is_sel {
                p.rect_stroke(r, 6.0, Stroke::new(1.5, TEXT), egui::StrokeKind::Outside);
            }
            // Thumbnail and length.
            let mut x = r.left() + 3.0;
            if r.width() >= 18.0 {
                if let Some(t) = app.thumbs.get(ctx, &app.doc, b.start, layer.id) {
                    let s = t.size_vec2();
                    let th = r.height() - 8.0;
                    let tw = (th * s.x / s.y).min(r.width() - 6.0);
                    let tr = Rect::from_min_size(Pos2::new(x, r.center().y - th / 2.0), Vec2::new(tw, th));
                    ui.painter().image(t.id(), tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    x = tr.right() + 4.0;
                }
            }
            if len > 1 && r.right() - x > 18.0 {
                let txt = if r.right() - x > 64.0 && !compact { format!("×{} · {} s", len, crate::i18n::num(len as f32 / fps as f32, 2)) } else { format!("×{len}") };
                ui.painter().text(Pos2::new(x, r.center().y), Align2::LEFT_CENTER, txt, FontId::proportional(10.5), if is_sel { Color32::from_rgb(28, 28, 30) } else { TEXT2 });
            }
            // Stretch handle on the selected drawing.
            if is_sel && !layer.props.locked {
                let hx = r.right();
                let handle = Rect::from_center_size(Pos2::new(hx, r.center().y), Vec2::new(30.0, row_h));
                let hr = ui.interact(handle, ui.scope_id().with(("stretch", layer.id, b.start)), Sense::drag());
                ui.painter().rect_filled(Rect::from_center_size(Pos2::new(hx, r.center().y), Vec2::new(7.0, r.height() - 6.0)), 3.5, TEXT);
                if hr.drag_started() {
                    app.ui.tracks.stretch = Some(Stretch { layer: layer.id, start: b.start, len: b.len });
                }
                if hr.dragged() {
                    if let Some(pos) = hr.interact_pointer_pos() {
                        let new_len = (((pos.x - x_of(b.start as f32)) / ppf).round() as isize).max(1) as usize;
                        app.ui.tracks.stretch = Some(Stretch { layer: layer.id, start: b.start, len: new_len });
                        // Bubble with frames and seconds.
                        let bubble = format!("×{} · {} s", new_len, crate::i18n::num(new_len as f32 / fps as f32, 2));
                        let at = Pos2::new(x_of((b.start + new_len) as f32), top - 4.0);
                        let galley = ui.painter().layout_no_wrap(bubble, FontId::proportional(13.0), Color32::from_rgb(28, 28, 30));
                        let br = Rect::from_center_size(Pos2::new(at.x, at.y - galley.size().y / 2.0 - 4.0), galley.size() + Vec2::new(16.0, 8.0));
                        let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("stretch_bubble")));
                        fg.rect_filled(br, 8.0, ACCENT);
                        fg.galley(br.min + Vec2::new(8.0, 4.0), galley, Color32::from_rgb(28, 28, 30));
                    }
                    ui.ctx().request_repaint();
                }
                if hr.drag_stopped() {
                    commit_stretch = app.ui.tracks.stretch.take();
                }
                hr.on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text(tr("Arrastra para que dure más o menos"));
            }
        }

        // "+" after the last drawing.
        if !layer.props.locked {
            let end = blocks.last().map(|b| b.end()).unwrap_or(0) as f32;
            let end = if stretch.is_some_and(|s| s.layer == layer.id) { end + offset as f32 } else { end };
            let r = Rect::from_min_size(Pos2::new(x_of(end) + 4.0, top + pad + 2.0), Vec2::new(row_h - 2.0 * pad - 4.0, row_h - 2.0 * pad - 4.0));
            let resp = ui.interact(r, ui.scope_id().with(("append", layer.id)), Sense::click());
            let p = ui.painter();
            p.rect_stroke(r, 6.0, Stroke::new(1.2, if resp.hovered() { TEXT2 } else { TEXT3 }), egui::StrokeKind::Inside);
            paint(p, Rect::from_center_size(r.center(), Vec2::splat(14.0)), Icon::Plus, TEXT2);
            if resp.on_hover_text(tr("Nuevo dibujo al final de esta capa")).clicked() {
                append = Some(layer.id);
            }
        }

        // Tap on the lane: go to that frame on that layer.
        if bg.clicked() || bg.dragged() {
            if let Some(pos) = bg.interact_pointer_pos() {
                if lane.contains(pos) {
                    set_frame = Some((frame_at(pos.x).min(n_frames.saturating_sub(1)), Some(layer.id)));
                }
            }
        }
    }

    // Tap or drag on the ruler: scrub.
    if bg.clicked() || bg.dragged() {
        if let Some(pos) = bg.interact_pointer_pos() {
            if pos.y < rect.top() + ruler_h {
                set_frame = Some((frame_at(pos.x).min(n_frames.saturating_sub(1)), None));
            }
        }
    }

    // Playhead.
    let px = x_of(shown as f32 + 0.5);
    let p = ui.painter();
    p.vline(px, rect.top()..=rect.bottom(), Stroke::new(2.0, ACCENT));
    p.rect_filled(Rect::from_center_size(Pos2::new(px, rect.top() + 5.0), Vec2::new(12.0, 10.0)), 3.0, ACCENT);
    if app.playback.playing {
        ui.scroll_to_rect(Rect::from_center_size(Pos2::new(px, rect.center().y), Vec2::new(ppf * 6.0, 4.0)), None);
    }

    if let Some((f, layer)) = set_frame {
        app.stop_playback();
        app.canvas.select.commit(&mut app.doc);
        if let Some(l) = layer {
            app.doc.layer = l;
        }
        app.doc.set_frame(f);
    }
    if let Some(s) = commit_stretch {
        let d = &mut app.doc;
        if tl::set_len(&mut d.project, &mut d.history, s.layer, s.start, s.len) {
            d.invalidate_all();
        }
        d.set_frame(s.start);
    }
    if let Some(l) = append {
        app.stop_playback();
        let d = &mut app.doc;
        d.layer = l;
        let at = tl::append_drawing(&mut d.project, &mut d.history, l);
        d.invalidate_all();
        d.set_frame(at);
    }
}

/// Actions on the selected drawing: duration, new, duplicate, move, delete.
pub fn block_actions(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let Some(b) = selected_block(app) else {
        let msg = match app.doc.project.layer(app.doc.layer) {
            Some(l) if l.props.shared => tr("Capa de fondo fijo: se ve en todo el plano"),
            Some(l) if l.props.video.is_some() => tr("Capa de video: ajústala en Capas"),
            _ => tr("Nada en este fotograma: toca «+» o dibuja para crear un dibujo"),
        };
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            ui.label(RichText::new(msg).size(12.0).color(TEXT2));
        });
        return;
    };
    let fps = app.doc.project.canvas.fps.max(1) as f32;
    let layer = app.doc.layer;
    let index = tl::blocks(&app.doc.project, layer).iter().position(|x| *x == b).unwrap_or(0) + 1;
    let name = app.doc.project.layer(layer).map(|l| crate::i18n::layer_name(&l.props.name).into_owned()).unwrap_or_default();
    let mut action: Option<u8> = None;
    let mut new_len = None;
    let narrow = ui.available_width() < 520.0;
    ui.horizontal(|ui| {
        ui.set_min_height(40.0);
        if !narrow {
            ui.label(RichText::new(tf!("Dibujo {} · {}", index, name)).size(12.0).color(TEXT2));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let small = |ui: &mut egui::Ui, icon: Icon, tip: &str, color: Color32| -> bool {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(40.0, 36.0), Sense::click());
                ui.painter().rect_filled(r, 10.0, if resp.hovered() { SEPARATOR } else { ELEVATED });
                paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(18.0)), icon, color);
                resp.on_hover_text(tip).clicked()
            };
            if small(ui, Icon::Trash, tr("Borrar este dibujo"), DANGER) {
                action = Some(0);
            }
            if small(ui, Icon::Duplicate, tr("Duplicar"), TEXT) {
                action = Some(1);
            }
            if !narrow && small(ui, Icon::ArrowRight, tr("Mover después del siguiente"), TEXT) {
                action = Some(3);
            }
            if !narrow && small(ui, Icon::ArrowLeft, tr("Mover antes del anterior"), TEXT) {
                action = Some(4);
            }
            if small(ui, Icon::Plus, tr("Nuevo dibujo después (N)"), TEXT) {
                action = Some(2);
            }
            ui.add_space(6.0);
            // Duration stepper.
            egui::Frame::new().fill(ELEVATED).corner_radius(10).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if ui.add(egui::Button::new(RichText::new("+").size(17.0)).frame(false).min_size(Vec2::new(34.0, 34.0))).on_hover_text(tr("Que dure un fotograma más")).clicked() {
                        new_len = Some(b.len + 1);
                    }
                    ui.add_sized([86.0, 34.0], egui::Label::new(RichText::new(format!("×{} · {} s", b.len, crate::i18n::num(b.len as f32 / fps, 2))).size(12.5).strong()));
                    if ui.add_enabled(b.len > 1, egui::Button::new(RichText::new("−").size(17.0)).frame(false).min_size(Vec2::new(34.0, 34.0))).on_hover_text(tr("Un fotograma menos")).clicked() {
                        new_len = Some(b.len - 1);
                    }
                });
            });
        });
    });
    if action.is_some_and(|a| a != 2) {
        app.stop_playback();
    }
    let d = &mut app.doc;
    if let Some(len) = new_len {
        if tl::set_len(&mut d.project, &mut d.history, layer, b.start, len) {
            d.invalidate_all();
        }
    }
    match action {
        Some(0) => {
            if tl::delete(&mut d.project, &mut d.history, layer, b.start) {
                d.invalidate_all();
            }
        }
        Some(1) => {
            if let Some(at) = tl::duplicate(&mut d.project, &mut d.history, layer, b.start) {
                d.invalidate_all();
                d.set_frame(at);
            }
        }
        Some(2) => add_frame(app),
        Some(3) | Some(4) => {
            let dir = if action == Some(3) { 1 } else { -1 };
            if let Some(at) = tl::shift(&mut d.project, &mut d.history, layer, b.start, dir) {
                d.invalidate_all();
                d.set_frame(at);
            }
        }
        _ => {}
    }
}

/// Clicking a panel applies any floating selection first, so frame and layer
/// operations always see the finished drawing.
pub fn commit_selection_on_click(app: &mut TrazoApp, ui: &egui::Ui) {
    if app.canvas.select.is_active() && ui.ui_contains_pointer() && ui.input(|i| i.pointer.any_pressed()) {
        app.canvas.select.commit(&mut app.doc);
    }
}
