//! Selection tools (rectangle, lasso) and the free-transform handles.

use crate::i18n::tr;
use trazo_core::fill::{flood_region, paint_mask, FillOptions};
use trazo_core::history::{Edit, PixelEditBuilder};
use trazo_core::transform::{Ease, Mask, Transform, Xform};
use trazo_core::{IRect, Rgba};
use egui::{Color32, Pos2, Shape, Stroke, Vec2};

use crate::app::Document;
use crate::canvas::ViewXf;
use crate::tools::LassoMode;

/// What the selection tool should do with the pointer.
#[derive(Clone, Copy)]
pub struct SelectParams {
    pub kind: SelectKind,
    pub lasso_mode: LassoMode,
    pub color: Rgba,
    pub fill: FillOptions,
    pub wand_all_layers: bool,
    pub editable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectKind {
    Rect,
    Lasso,
    Wand,
}

/// Handle hit radius in screen pixels (finger friendly).
const HANDLE_HIT: f32 = 18.0;
/// Rotation handle distance above the top edge, in screen pixels.
const ROTATE_OFFSET: f32 = 34.0;

enum Drawing {
    Rect(Pos2, Pos2),
    Lasso(Vec<(f32, f32)>),
}

#[derive(Clone, Copy)]
enum DragMode {
    Move,
    Scale,
    Rotate,
}

struct Drag {
    mode: DragMode,
    start: Pos2,
    start_xf: Xform,
}

/// Most drawings one recording can make.
pub const MAX_RECORDED: usize = 240;

/// "Record motion": while the selection is dragged, its pose is sampled at
/// the animation's speed; each sample becomes a drawing.
struct MotionRec {
    /// Seconds between samples (one drawing's exposure).
    interval: f64,
    started: Option<f64>,
    poses: Vec<Xform>,
    done: bool,
}

#[derive(Default)]
pub struct SelectTool {
    rec: Option<MotionRec>,
    /// A recording in progress was dropped (frame, layer or tool changed).
    pub dropped_recording: bool,
    drawing: Option<Drawing>,
    active: Option<Transform>,
    drag: Option<Drag>,
    /// Copied pixels: source rect + premultiplied RGBA.
    clipboard: Option<(IRect, Vec<u8>)>,
}

impl SelectTool {
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn has_clipboard(&self) -> bool {
        self.clipboard.is_some()
    }

    /// Drop the floating selection without touching any document (used when
    /// the project is replaced).
    pub fn forget_active(&mut self) {
        self.active = None;
        self.drag = None;
        self.drawing = None;
    }

    pub fn is_busy(&self) -> bool {
        self.drawing.is_some() || self.drag.is_some()
    }

    /// True if the floating selection belongs to another frame/layer than the current one.
    pub fn is_stale(&self, doc: &Document) -> bool {
        self.active.as_ref().is_some_and(|t| t.layer() != doc.layer || t.owner() != doc.project.cel_owner(doc.frame_id(), doc.layer))
    }

    /// Handle pointer input for the selection tools.
    pub fn input(&mut self, ui: &egui::Ui, response: &egui::Response, doc: &mut Document, xf: ViewXf, params: SelectParams) -> Option<String> {
        let editable = params.editable;
        let lasso = params.kind == SelectKind::Lasso;
        let (pressed, down, released) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released()));
        let pos = response.interact_pointer_pos().or(response.hover_pos());

        if pressed && response.hovered() {
            let Some(s) = pos else { return None };
            let c = xf.to_canvas(s);
            // While recording, a drag anywhere moves the selection (easier with a finger).
            let armed = self.rec.as_ref().is_some_and(|r| r.started.is_none()) && self.active.is_some();
            if let Some(mode) = self.hit(s, xf).or(armed.then_some(DragMode::Move)) {
                if let Some(r) = self.rec.as_mut().filter(|r| r.started.is_none()) {
                    r.started = Some(ui.input(|i| i.time));
                }
                let start_xf = self.active.as_ref().map(|t| t.xform()).unwrap_or_default();
                self.drag = Some(Drag { mode, start: c, start_xf });
                return None;
            }
            self.commit(doc);
            if !editable {
                return Some(tr("La capa está bloqueada u oculta").into());
            }
            if params.kind == SelectKind::Wand {
                return self.wand(doc, c.x.floor() as i32, c.y.floor() as i32, params);
            }
            self.drawing = Some(if lasso { Drawing::Lasso(vec![(c.x, c.y)]) } else { Drawing::Rect(c, c) });
            return None;
        }

        if down {
            if let Some(s) = pos {
                let c = xf.to_canvas(s);
                if let Some(d) = &self.drawing {
                    match d {
                        Drawing::Rect(a, _) => self.drawing = Some(Drawing::Rect(*a, c)),
                        Drawing::Lasso(pts) => {
                            let last = *pts.last().expect("lasso starts with a point");
                            if (last.0 - c.x).abs() + (last.1 - c.y).abs() > 1.5 {
                                let mut pts = std::mem::take(match &mut self.drawing {
                                    Some(Drawing::Lasso(p)) => p,
                                    _ => unreachable!(),
                                });
                                pts.push((c.x, c.y));
                                self.drawing = Some(Drawing::Lasso(pts));
                            }
                        }
                    }
                }
                if let Some(drag) = &self.drag {
                    let new_xf = self.drag_xform(drag, c);
                    self.set_xform(doc, new_xf);
                }
            }
            self.sample_motion(ui);
        }

        if released || (!down && (self.drawing.is_some() || self.drag.is_some())) {
            if self.drag.is_some() {
                // Take the samples that were due, then stop.
                self.sample_motion(ui);
                if let Some(r) = self.rec.as_mut().filter(|r| r.started.is_some()) {
                    r.done = true;
                }
            }
            self.drag = None;
            if let Some(d) = self.drawing.take() {
                let bounds = IRect::from_size(doc.project.canvas.width, doc.project.canvas.height);
                let mask = match d {
                    Drawing::Rect(a, b) => {
                        let r = IRect::new(a.x.min(b.x).floor() as i32, a.y.min(b.y).floor() as i32, a.x.max(b.x).ceil() as i32, a.y.max(b.y).ceil() as i32);
                        if r.width() < 2 || r.height() < 2 {
                            return None; // a tap just deselects
                        }
                        Mask::from_rect(r, bounds)
                    }
                    Drawing::Lasso(pts) => Mask::from_polygon(&pts, bounds),
                };
                if let Some(mask) = mask {
                    if lasso && params.lasso_mode != LassoMode::Select {
                        return self.paint_lasso(doc, &mask, params);
                    }
                    return self.lift(doc, mask);
                }
            }
        }
        None
    }

    /// Magic wand: select the area of similar color under `(x, y)`.
    fn wand(&mut self, doc: &mut Document, x: i32, y: i32, params: SelectParams) -> Option<String> {
        let (w, h) = (doc.project.canvas.width, doc.project.canvas.height);
        let reference = if params.wand_all_layers {
            trazo_core::composite::flatten_frame(&doc.project, doc.frame)
        } else {
            doc.project.visible_cel(doc.frame, doc.layer).cloned().unwrap_or_else(|| trazo_core::TiledImage::new(w, h))
        };
        let opts = FillOptions { grow: 0, gap_close: 0, ..params.fill };
        let (bits, rect) = flood_region(&reference, x, y, opts)?;
        let mask = Mask::from_bits(&bits, w as i32, rect)?;
        self.lift(doc, mask)
    }

    /// Lasso fill / lasso erase: paint the closed loop directly (one undo step).
    fn paint_lasso(&mut self, doc: &mut Document, mask: &Mask, params: SelectParams) -> Option<String> {
        let (layer, owner) = (doc.layer, doc.project.cel_owner(doc.frame_id(), doc.layer));
        let alpha_lock = doc.project.layer(layer).is_some_and(|l| l.props.alpha_lock);
        let cel = doc.project.cel_at_mut(owner, layer)?;
        let mut edit = PixelEditBuilder::new();
        let r = mask.rect;
        match params.lasso_mode {
            LassoMode::Fill => paint_mask(cel, &|x, y| mask.at(x, y) > 0, r, params.color, alpha_lock, &mut edit),
            LassoMode::Erase => {
                for (tx, ty) in cel.tiles_in_rect(&r).collect::<Vec<_>>() {
                    let idx = cel.tile_index(tx, ty);
                    edit.touch(cel, idx);
                }
                for y in r.y0..r.y1 {
                    for x in r.x0..r.x1 {
                        if mask.at(x, y) > 0 {
                            cel.set_pixel(x, y, [0; 4]);
                        }
                    }
                }
            }
            LassoMode::Select => {}
        }
        if let Some(e) = edit.finish(cel, owner, layer) {
            doc.history.push_applied(Edit::Pixels(e));
            doc.invalidate_frame(owner, Some(r));
        }
        None
    }

    fn lift(&mut self, doc: &mut Document, mask: Mask) -> Option<String> {
        let (layer, owner) = (doc.layer, doc.project.cel_owner(doc.frame_id(), doc.layer));
        let cel = doc.project.cel_at_mut(owner, layer)?;
        match Transform::lift(cel, owner, layer, mask) {
            Some(t) => {
                self.active = Some(t);
                None
            }
            None => Some(tr("No hay nada dibujado en la selección").into()),
        }
    }

    fn drag_xform(&self, drag: &Drag, c: Pos2) -> Xform {
        let Some(t) = &self.active else { return drag.start_xf };
        let r = t.source_rect();
        let s = drag.start_xf;
        let center = Pos2::new((r.x0 + r.x1) as f32 * 0.5 + s.dx, (r.y0 + r.y1) as f32 * 0.5 + s.dy);
        match drag.mode {
            DragMode::Move => Xform { dx: s.dx + c.x - drag.start.x, dy: s.dy + c.y - drag.start.y, ..s },
            DragMode::Scale => {
                let d0 = (drag.start - center).length().max(1.0);
                let f = ((c - center).length() / d0).clamp(0.02, 50.0);
                Xform { sx: s.sx * f, sy: s.sy * f, ..s }
            }
            DragMode::Rotate => {
                let a0 = (drag.start - center).angle();
                let a1 = (c - center).angle();
                // atan2 jumps at ±180°: take the short way so a small turn
                // isn't stored as almost a full one (in-betweens would spin).
                let d = (a1 - a0 + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                Xform { angle: s.angle + d, ..s }
            }
        }
    }

    fn set_xform(&mut self, doc: &mut Document, xf: Xform) {
        let Some(t) = &mut self.active else { return };
        let (owner, layer) = (t.owner(), t.layer());
        if let Some(cel) = doc.project.cel_at_mut(owner, layer) {
            let area = t.set_xform(cel, xf);
            doc.redraw.add(area);
        }
    }

    /// Which part of the transform box is under screen point `s`.
    fn hit(&self, s: Pos2, xf: ViewXf) -> Option<DragMode> {
        let t = self.active.as_ref()?;
        let corners = t.corners().map(|(x, y)| xf.to_screen(Pos2::new(x, y)));
        if (s - rotate_handle(&corners)).length() < HANDLE_HIT {
            return Some(DragMode::Rotate);
        }
        let inside = point_in_quad(s, &corners);
        // Small boxes: handles would cover everything, so moving wins inside
        // unless the pointer is right on a corner.
        let small = (corners[2] - corners[0]).length() < HANDLE_HIT * 4.0;
        let corner_r = if small && inside { HANDLE_HIT * 0.4 } else { HANDLE_HIT };
        if corners.iter().any(|c| (s - *c).length() < corner_r) {
            return Some(DragMode::Scale);
        }
        inside.then_some(DragMode::Move)
    }

    /// A second finger touched the screen: abandon the gesture in progress.
    pub fn abort_gesture(&mut self, doc: &mut Document) {
        self.drawing = None;
        if let Some(d) = self.drag.take() {
            let start = d.start_xf;
            self.set_xform(doc, start);
        }
    }

    /// Move the floating selection by whole pixels (arrow keys).
    pub fn nudge(&mut self, doc: &mut Document, dx: f32, dy: f32) {
        let Some(t) = &self.active else { return };
        let x = t.xform();
        self.set_xform(doc, Xform { dx: x.dx + dx, dy: x.dy + dy, ..x });
    }

    /// Draw the marching outline and handles.
    pub fn paint(&self, painter: &egui::Painter, xf: ViewXf) {
        let dash = |pts: Vec<Pos2>| {
            painter.extend(Shape::dashed_line(&pts, Stroke::new(3.0, Color32::from_black_alpha(140)), 6.0, 4.0));
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.5, Color32::WHITE), 6.0, 4.0));
        };
        match &self.drawing {
            Some(Drawing::Rect(a, b)) => {
                let pts = [Pos2::new(a.x, a.y), Pos2::new(b.x, a.y), Pos2::new(b.x, b.y), Pos2::new(a.x, b.y), Pos2::new(a.x, a.y)];
                dash(pts.iter().map(|p| xf.to_screen(*p)).collect());
            }
            Some(Drawing::Lasso(pts)) => {
                let mut v: Vec<Pos2> = pts.iter().map(|&(x, y)| xf.to_screen(Pos2::new(x, y))).collect();
                if let Some(f) = v.first().copied() {
                    v.push(f);
                }
                dash(v);
            }
            None => {}
        }
        if let Some(t) = &self.active {
            let c = t.corners().map(|(x, y)| xf.to_screen(Pos2::new(x, y)));
            dash(vec![c[0], c[1], c[2], c[3], c[0]]);
            let accent = crate::theme::ACCENT;
            let rot = rotate_handle(&c);
            let top_mid = c[0] + (c[1] - c[0]) * 0.5;
            painter.line_segment([top_mid, rot], Stroke::new(1.5, accent));
            painter.circle_filled(rot, 8.0, accent);
            painter.circle_stroke(rot, 8.0, Stroke::new(1.5, Color32::WHITE));
            for p in c {
                painter.rect_filled(egui::Rect::from_center_size(p, Vec2::splat(13.0)), 2.0, Color32::WHITE);
                painter.rect_stroke(egui::Rect::from_center_size(p, Vec2::splat(13.0)), 2.0, Stroke::new(1.5, accent), egui::StrokeKind::Middle);
            }
        }
    }

    // ---- actions ----------------------------------------------------------

    /// Apply the floating selection to the layer (one undo step).
    pub fn commit(&mut self, doc: &mut Document) {
        if self.rec.take().is_some_and(|r| r.started.is_some()) {
            self.dropped_recording = true;
        }
        self.drag = None;
        self.drawing = None;
        let Some(t) = self.active.take() else { return };
        let (owner, layer) = (t.owner(), t.layer());
        let Some(cel) = doc.project.cel_at_mut(owner, layer) else { return };
        if t.is_untouched() {
            // Nothing changed: restore exactly, without an undo step.
            let area = t.cancel(cel);
            doc.redraw.add(area);
            return;
        }
        let area = t.source_rect();
        if let Some(edit) = t.commit(cel) {
            doc.history.push_applied(Edit::Pixels(edit));
        }
        doc.invalidate_frame(owner, Some(area));
        doc.redraw = crate::app::Redraw::Full;
    }

    /// Start "record motion": the next drag of the selection is recorded.
    pub fn arm_recording(&mut self, doc: &Document) -> Result<(), String> {
        let Some((_, block)) = self.tween_source(doc) else {
            return Err(tr("Aquí no se puede grabar movimiento (capa de fondo fija, bloqueada o de video)").into());
        };
        let fps = doc.project.canvas.fps.max(1) as f64;
        self.rec = Some(MotionRec { interval: block.len as f64 / fps, started: None, poses: Vec::new(), done: false });
        Ok(())
    }

    pub fn cancel_recording(&mut self) {
        self.rec = None;
    }

    /// `None` when not recording; otherwise how many drawings so far (0 while waiting for the drag).
    pub fn recording(&self) -> Option<usize> {
        self.rec.as_ref().map(|r| r.poses.len())
    }

    /// Sample the pose at the animation's speed while dragging.
    fn sample_motion(&mut self, ui: &egui::Ui) {
        let Some(xf) = self.active.as_ref().map(|t| t.xform()) else { return };
        let Some(r) = self.rec.as_mut() else { return };
        let Some(t0) = r.started else { return };
        if r.done {
            return;
        }
        let due = (((ui.input(|i| i.time) - t0) / r.interval).floor() as usize).min(MAX_RECORDED);
        while r.poses.len() < due {
            r.poses.push(xf);
        }
        if r.poses.len() >= MAX_RECORDED {
            r.done = true;
        }
        // Keep sampling while the finger rests.
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(r.interval / 2.0));
    }

    /// A finished recording turned into drawings after the current one, in
    /// one undo step. Returns (first new frame, drawings added); `None` while
    /// still recording.
    pub fn finish_recording(&mut self, doc: &mut Document) -> Option<Result<(usize, usize), String>> {
        if !self.rec.as_ref().is_some_and(|r| r.done) {
            return None;
        }
        let rec = self.rec.take()?;
        let mut poses = rec.poses;
        // Drop the still tail (the finger rested before letting go).
        while poses.len() > 1 && poses[poses.len() - 1] == poses[poses.len() - 2] {
            poses.pop();
        }
        if poses.is_empty() || poses.iter().all(|p| p.is_identity()) {
            return Some(Err(tr("Grabación muy corta: arrastra la selección mientras graba").into()));
        }
        let Some((layer, block)) = self.tween_source(doc) else { return Some(Err(String::new())) };
        let owner = self.active.as_ref()?.owner();
        // Check the drawing exists before taking the floating pixels.
        doc.project.cel_at_mut(owner, layer)?;
        let t = self.active.take()?;
        self.drag = None;
        let cel = doc.project.cel_at_mut(owner, layer)?;
        let (cels, stamp) = t.render_poses(cel, &poses);
        let added = cels.len();
        let mark = doc.history.undo_len();
        if let Some(e) = stamp {
            doc.history.push_applied(Edit::Pixels(e));
        }
        let first = trazo_core::timeline::insert_drawings_after(&mut doc.project, &mut doc.history, layer, block.start, block.len, cels);
        doc.history.group_since(mark);
        doc.invalidate_all();
        doc.redraw = crate::app::Redraw::Full;
        Some(Ok((first.unwrap_or(block.end()), added)))
    }

    /// Where in-betweens of the floating selection would go: its layer and
    /// the drawing it belongs to. None on fixed background layers (they have
    /// no timing), on layers that can't be painted, or if the content was erased.
    pub fn tween_source(&self, doc: &Document) -> Option<(trazo_core::LayerId, trazo_core::timeline::Block)> {
        let t = self.active.as_ref()?;
        let (owner, layer) = (t.owner(), t.layer());
        if owner == trazo_core::document::SHARED_FRAME || t.is_cleared() || !doc.project.layer(layer)?.props.paintable() {
            return None;
        }
        let start = doc.project.frame_index(owner)?;
        Some((layer, trazo_core::timeline::block_at(&doc.project, layer, start)?))
    }

    /// The selection was moved, scaled or turned somewhere it can make in-betweens.
    pub fn can_tween(&self, doc: &Document) -> bool {
        self.active.as_ref().is_some_and(|t| !t.xform().is_identity()) && self.tween_source(doc).is_some()
    }

    /// In-betweens from where the selection started to where it is now: the
    /// drawing keeps the first pose and `steps` new drawings (plus the final
    /// pose if `with_end`) follow it on the same layer, each as long as the
    /// first one, in one undo step. Returns where the first new drawing starts
    /// and how many were added.
    pub fn tween(&mut self, doc: &mut Document, steps: usize, ease: Ease, with_end: bool) -> Result<(usize, usize), String> {
        let Some((layer, block)) = self.tween_source(doc).filter(|_| self.can_tween(doc)) else {
            return Err(tr("Mueve, escala o gira la selección para marcar la posición final").into());
        };
        let Some(t) = self.active.take() else { return Err(String::new()) };
        self.drag = None;
        self.drawing = None;
        let owner = t.owner();
        let Some(cel) = doc.project.cel_at_mut(owner, layer) else {
            // Can't happen (checked above); keep the selection rather than lose it.
            self.active = Some(t);
            return Err(tr("La capa está vacía en este fotograma").into());
        };
        let mut progress = ease.spacing(steps);
        if !with_end {
            progress.pop();
        }
        let (cels, stamp) = t.tween(cel, &progress);
        let added = cels.len();
        let mark = doc.history.undo_len();
        if let Some(e) = stamp {
            doc.history.push_applied(Edit::Pixels(e));
        }
        let first = trazo_core::timeline::insert_drawings_after(&mut doc.project, &mut doc.history, layer, block.start, block.len, cels);
        doc.history.group_since(mark);
        doc.invalidate_all();
        doc.redraw = crate::app::Redraw::Full;
        Ok((first.unwrap_or(block.end()), added))
    }

    /// Undo the transform and put the pixels back where they were.
    pub fn cancel(&mut self, doc: &mut Document) {
        self.rec = None;
        self.drawing = None;
        if self.drag.is_some() {
            self.abort_gesture(doc);
        }
        let Some(t) = self.active.take() else { return };
        let (owner, layer) = (t.owner(), t.layer());
        if let Some(cel) = doc.project.cel_at_mut(owner, layer) {
            let area = t.cancel(cel);
            doc.redraw.add(area);
        }
    }

    pub fn flip(&mut self, doc: &mut Document, horizontal: bool) {
        let Some(t) = &self.active else { return };
        let mut x = t.xform();
        if horizontal {
            x.sx = -x.sx;
        } else {
            x.sy = -x.sy;
        }
        self.set_xform(doc, x);
    }

    pub fn rotate90(&mut self, doc: &mut Document) {
        let Some(t) = &self.active else { return };
        let x = t.xform();
        self.set_xform(doc, Xform { angle: x.angle + std::f32::consts::FRAC_PI_2, ..x });
    }

    pub fn copy(&mut self) -> bool {
        let Some(t) = &self.active else { return false };
        // The untransformed pixels are copied; pasting places them at their source spot.
        let (rect, px) = t.pixels();
        self.clipboard = Some((rect, px.to_vec()));
        true
    }

    pub fn delete(&mut self, doc: &mut Document) {
        let Some(t) = &mut self.active else { return };
        let (owner, layer) = (t.owner(), t.layer());
        if let Some(cel) = doc.project.cel_at_mut(owner, layer) {
            let area = t.clear_pixels(cel);
            doc.redraw.add(area);
        }
        self.commit(doc);
    }

    pub fn cut(&mut self, doc: &mut Document) {
        if self.copy() {
            self.delete(doc);
        }
    }

    /// Put pixels on the current layer as a floating selection (image import).
    pub fn place(&mut self, doc: &mut Document, rect: IRect, px: Vec<u8>) {
        self.commit(doc);
        let (layer, owner) = (doc.layer, doc.project.cel_owner(doc.frame_id(), doc.layer));
        let Some(cel) = doc.project.cel_at_mut(owner, layer) else { return };
        self.active = Some(Transform::paste(cel, owner, layer, rect, px));
        doc.redraw.add(rect);
    }

    /// Paste the clipboard on the current layer and frame as a new floating selection.
    pub fn paste(&mut self, doc: &mut Document) -> Option<String> {
        self.commit(doc);
        let (rect, px) = self.clipboard.clone()?;
        let (layer, owner) = (doc.layer, doc.project.cel_owner(doc.frame_id(), doc.layer));
        let cel = doc.project.cel_at_mut(owner, layer)?;
        let t = Transform::paste(cel, owner, layer, rect, px);
        doc.redraw.add(rect);
        self.active = Some(t);
        None
    }

    /// Copy the selection and paste it right away (to move a copy).
    pub fn duplicate(&mut self, doc: &mut Document) {
        if self.copy() {
            self.paste(doc);
            if let Some(t) = &self.active {
                let x = t.xform();
                self.set_xform(doc, Xform { dx: x.dx + 20.0, dy: x.dy + 20.0, ..x });
            }
        }
    }

    /// Select everything drawn on the current layer.
    pub fn select_all(&mut self, doc: &mut Document) -> Option<String> {
        self.commit(doc);
        if !doc.project.layer(doc.layer).is_some_and(|l| l.props.paintable()) {
            return Some(tr("La capa está bloqueada, oculta o es de video").into());
        }
        let bounds = IRect::from_size(doc.project.canvas.width, doc.project.canvas.height);
        let content = doc.project.visible_cel(doc.frame, doc.layer).and_then(|c| c.content_bounds());
        let Some(r) = content else { return Some(tr("La capa está vacía en este fotograma").into()) };
        let mask = Mask::from_rect(r, bounds)?;
        self.lift(doc, mask)
    }
}

fn rotate_handle(c: &[Pos2; 4]) -> Pos2 {
    let top_mid = c[0] + (c[1] - c[0]) * 0.5;
    let center = c[0] + (c[2] - c[0]) * 0.5;
    let up = (top_mid - center).normalized();
    let up = if up.is_finite() { up } else { Vec2::new(0.0, -1.0) };
    top_mid + up * ROTATE_OFFSET
}

fn point_in_quad(p: Pos2, q: &[Pos2; 4]) -> bool {
    // Works for convex quads in either winding.
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let cross = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if cross != 0.0 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    true
}
