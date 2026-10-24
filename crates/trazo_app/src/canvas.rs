//! The drawing canvas: view transform (zoom, pan, rotate, mirror), input
//! (mouse, pen, touch gestures) and the texture that mirrors the frame.

use crate::i18n::tr;
use std::collections::HashMap;

use trazo_core::brush::BrushMode;
use trazo_core::composite::{flatten_frame, render_region, render_region_scaled, OnionSkin, RenderOptions};
use trazo_core::fill::flood_fill;
use trazo_core::history::{Edit, PixelEditBuilder};
use trazo_core::raster::{tile_rect, TiledImage};
use trazo_core::shapes::shape_points;
use trazo_core::stroke::{Stroke, StrokePoint};
use trazo_core::{IRect, Rgba};
use egui::{Color32, ColorImage, Pos2, Rect, Sense, Stroke as UiStroke, TextureHandle, TextureOptions, Vec2};

use crate::app::{Document, Redraw};
use crate::selection::{SelectKind, SelectParams, SelectTool};
use crate::tools::{Tool, ToolState};

const TEX_OPTIONS: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    ..TextureOptions::LINEAR
};

/// Memory allowed for pre-rendered playback frames.
/// A rendered frame kept for looping playback.
struct CachedFrame {
    step: usize,
    w: usize,
    h: usize,
    /// LZ4 of the premultiplied RGBA pixels.
    data: Vec<u8>,
}

impl CachedFrame {
    fn unpack(&self) -> Option<ColorImage> {
        let px = lz4_flex::decompress_size_prepended(&self.data).ok()?;
        (px.len() == self.w * self.h * 4).then(|| ColorImage::from_rgba_premultiplied([self.w, self.h], &px))
    }
}

/// Compressed bytes of rendered frames kept for looping playback (smaller on phones).
const PLAYBACK_CACHE_BYTES: usize = if cfg!(target_os = "android") { 96 * 1024 * 1024 } else { 384 * 1024 * 1024 };

/// Multi-finger taps recognised on touch screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Undo,
    Redo,
}

/// What the texture currently shows, to detect when a full redraw is needed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shown {
    frame: usize,
    onion: Option<OnionSkin>,
}

/// State of a multi-touch gesture in progress.
struct TouchGesture {
    start: f64,
    max_touches: usize,
    movement: f32,
}

/// Screen ↔ canvas mapping for one UI frame.
#[derive(Clone, Copy)]
pub struct ViewXf {
    center: Pos2,
    half: Vec2,
    zoom: f32,
    cos: f32,
    sin: f32,
    flip: f32,
}

impl ViewXf {
    pub fn to_screen(self, c: Pos2) -> Pos2 {
        let v = (c.to_vec2() - self.half) * self.zoom;
        let v = Vec2::new(v.x * self.flip, v.y);
        self.center + Vec2::new(v.x * self.cos - v.y * self.sin, v.x * self.sin + v.y * self.cos)
    }

    pub fn to_canvas(self, s: Pos2) -> Pos2 {
        let v = s - self.center;
        let v = Vec2::new(v.x * self.cos + v.y * self.sin, -v.x * self.sin + v.y * self.cos);
        let v = Vec2::new(v.x * self.flip, v.y) / self.zoom;
        (v + self.half).to_pos2()
    }
}

pub struct CanvasView {
    texture: Option<TextureHandle>,
    shown: Option<Shown>,
    zoom: f32,
    pan: Vec2,
    rotation: f32,
    flip: bool,
    needs_fit: bool,
    /// Room left around the canvas when it is fitted on screen.
    pub fit_pad: f32,
    stroke: Option<Stroke>,
    /// For speed-based pressure.
    last_sample: Option<(Pos2, f64, f32)>,
    pen_force: Option<f32>,
    touch: Option<TouchGesture>,
    gesture: Option<Gesture>,
    message: Option<String>,
    /// Where the text tool was tapped (canvas pixels), for the app to ask for the text.
    text_at: Option<(f32, f32)>,
    /// Play through the project's camera (when it has one).
    pub camera_view: bool,
    stop_playback: bool,
    scratch: Vec<u8>,
    /// Frames rendered while playing, LZ4-compressed (a whole loop fits).
    playback_cache: HashMap<usize, CachedFrame>,
    playback_cache_bytes: usize,
    cache_version: u64,
    /// The texture holds every `tex_step`-th pixel (2 while playing a canvas
    /// shown at half size or less).
    tex_step: usize,
    pub select: SelectTool,
    /// Shape being dragged: start point and the preview stroke.
    /// (start, preview stroke, last end point)
    shape: Option<(Pos2, Option<Stroke>, Option<Pos2>)>,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            texture: None,
            shown: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            rotation: 0.0,
            flip: false,
            needs_fit: true,
            fit_pad: 40.0,
            stroke: None,
            last_sample: None,
            pen_force: None,
            touch: None,
            gesture: None,
            message: None,
            text_at: None,
            camera_view: true,
            stop_playback: false,
            scratch: Vec::new(),
            playback_cache: HashMap::new(),
            playback_cache_bytes: 0,
            cache_version: u64::MAX,
            tex_step: 1,
            select: SelectTool::default(),
            shape: None,
        }
    }
}

impl CanvasView {
    pub fn reset_view(&mut self) {
        // The clipboard survives switching projects.
        let clipboard = std::mem::take(&mut self.select);
        *self = Self::default();
        self.select = clipboard;
        self.select.forget_active();
    }

    pub fn is_stroking(&self) -> bool {
        self.stroke.is_some() || self.shape.is_some() || self.select.is_busy()
    }

    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    pub fn take_text_request(&mut self) -> Option<(f32, f32)> {
        self.text_at.take()
    }

    pub fn take_gesture(&mut self) -> Option<Gesture> {
        self.gesture.take()
    }

    pub fn wants_stop_playback(&mut self) -> bool {
        std::mem::take(&mut self.stop_playback)
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn rotation_deg(&self) -> f32 {
        self.rotation.to_degrees()
    }

    pub fn is_flipped(&self) -> bool {
        self.flip
    }

    /// Fit the canvas on screen and clear rotation.
    pub fn fit(&mut self) {
        self.needs_fit = true;
        self.rotation = 0.0;
    }

    pub fn rotate_by(&mut self, degrees: f32) {
        self.rotation = (self.rotation + degrees.to_radians()) % std::f32::consts::TAU;
    }

    pub fn toggle_flip(&mut self) {
        self.flip = !self.flip;
    }

    /// Abort the stroke (or shape) in progress and restore the cel.
    pub fn cancel_stroke(&mut self, doc: &mut Document) {
        if let Some((_, Some(st), _)) = self.shape.take() {
            if let Some(cel) = doc.project.cel_at_mut(st.frame(), st.layer()) {
                let r = st.cancel(cel);
                doc.redraw.add(r);
            }
        }
        let Some(stroke) = self.stroke.take() else { return };
        let (owner, layer) = (stroke.frame(), stroke.layer());
        if let Some(cel) = doc.project.cel_at_mut(owner, layer) {
            let r = stroke.cancel(cel);
            doc.redraw.add(r);
        }
    }

    fn xf(&self, rect: Rect, cw: f32, ch: f32) -> ViewXf {
        let (sin, cos) = self.rotation.sin_cos();
        ViewXf {
            center: rect.center() + self.pan,
            half: Vec2::new(cw, ch) * 0.5,
            zoom: self.zoom,
            cos,
            sin,
            flip: if self.flip { -1.0 } else { 1.0 },
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        doc: &mut Document,
        tools: &mut ToolState,
        frame: usize,
        onion: Option<OnionSkin>,
        playing: bool,
    ) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let (cw, ch) = (doc.project.canvas.width as f32, doc.project.canvas.height as f32);

        if self.needs_fit && rect.width() > 10.0 {
            let z = ((rect.width() - self.fit_pad) / cw).min((rect.height() - self.fit_pad) / ch);
            self.zoom = z.clamp(0.05, 16.0);
            self.pan = Vec2::ZERO;
            self.needs_fit = false;
        }

        self.handle_view_gestures(ui, &response, rect, tools, doc, cw, ch);

        let xf = self.xf(rect, cw, ch);
        if !playing {
            self.handle_tool_input(ui, &response, doc, tools, xf);
        } else if response.is_pointer_button_down_on() {
            self.stop_playback = true;
        }

        self.sync_texture(ui.ctx(), doc, frame, onion, playing);

        let painter = ui.painter_at(rect);
        let corners = [Pos2::new(0.0, 0.0), Pos2::new(cw, 0.0), Pos2::new(cw, ch), Pos2::new(0.0, ch)].map(|c| xf.to_screen(c));
        let shadow: Vec<Pos2> = corners.iter().map(|p| *p + Vec2::splat(4.0)).collect();
        painter.add(egui::Shape::convex_polygon(shadow, Color32::from_black_alpha(80), UiStroke::NONE));
        let cam = doc.project.canvas.camera;
        let cam_pose = cam.pose_at(frame, doc.project.frames.len());
        let through_camera = playing && self.camera_view && cam.is_active();
        if through_camera {
            // Outside the canvas the camera sees the background color.
            painter.add(egui::Shape::convex_polygon(corners.to_vec(), {
                let c = doc.project.canvas.background.to_rgba8();
                Color32::from_rgb(c[0], c[1], c[2])
            }, UiStroke::NONE));
        }
        if let Some(tex) = &self.texture {
            let mut mesh = egui::Mesh::with_texture(tex.id());
            // Through the camera the picture is moved, scaled and turned the
            // opposite way and clipped to the canvas (done by the GPU, and
            // what leaves the canvas shows the background, like the export).
            let place = |c: Pos2| -> Pos2 {
                if !through_camera {
                    return xf.to_screen(c);
                }
                let (cx, cy) = (cam_pose.x * cw, cam_pose.y * ch);
                let (s, co) = (-cam_pose.angle).sin_cos();
                let (dx, dy) = (c.x - cx, c.y - cy);
                let o = Pos2::new(cw / 2.0 + cam_pose.zoom * (dx * co - dy * s), ch / 2.0 + cam_pose.zoom * (dx * s + dy * co));
                xf.to_screen(o)
            };
            let uvs = [Pos2::new(0.0, 0.0), Pos2::new(1.0, 0.0), Pos2::new(1.0, 1.0), Pos2::new(0.0, 1.0)];
            let canvas_pts = [Pos2::new(0.0, 0.0), Pos2::new(cw, 0.0), Pos2::new(cw, ch), Pos2::new(0.0, ch)];
            for (c, uv) in canvas_pts.into_iter().zip(uvs) {
                mesh.vertices.push(egui::epaint::Vertex { pos: place(c), uv, color: Color32::WHITE });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            let painter = if through_camera {
                let r = Rect::from_points(&corners);
                painter.with_clip_rect(r.intersect(painter.clip_rect()))
            } else {
                painter.clone()
            };
            painter.add(mesh);
        }

        // The camera's framing at this frame.
        if cam.enabled && !playing && self.camera_view {
            let pts: Vec<Pos2> = cam_pose.corners(doc.project.canvas.width, doc.project.canvas.height).map(|(x, y)| xf.to_screen(Pos2::new(x, y))).to_vec();
            let mut closed = pts.clone();
            closed.push(pts[0]);
            painter.add(egui::Shape::dashed_line(&closed, UiStroke::new(2.0, crate::theme::ACCENT), 10.0, 6.0));
            painter.text(pts[0] + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, crate::i18n::tr("Cámara"), egui::FontId::proportional(12.0), crate::theme::ACCENT);
        }
        if tools.show_grid && !playing {
            let step = tools.grid_size.max(4) as f32;
            let line = UiStroke::new(1.0, Color32::from_rgba_unmultiplied(80, 140, 255, 70));
            let mut x = step;
            while x < cw {
                painter.line_segment([xf.to_screen(Pos2::new(x, 0.0)), xf.to_screen(Pos2::new(x, ch))], line);
                x += step;
            }
            let mut y = step;
            while y < ch {
                painter.line_segment([xf.to_screen(Pos2::new(0.0, y)), xf.to_screen(Pos2::new(cw, y))], line);
                y += step;
            }
        }
        if !playing {
            let axis = UiStroke::new(1.5, Color32::from_rgba_unmultiplied(255, 120, 70, 170));
            if tools.symmetry_vertical {
                painter.line_segment([xf.to_screen(Pos2::new(cw * 0.5, 0.0)), xf.to_screen(Pos2::new(cw * 0.5, ch))], axis);
            }
            if tools.symmetry_horizontal {
                painter.line_segment([xf.to_screen(Pos2::new(0.0, ch * 0.5)), xf.to_screen(Pos2::new(cw, ch * 0.5))], axis);
            }
        }
        self.select.paint(&painter, xf);

        // Brush outline under the pointer.
        if matches!(tools.tool, Tool::Brush | Tool::Eraser | Tool::Shape) && !playing {
            if let Some(hover) = response.hover_pos() {
                let r = (tools.tweaks[tools.active_index()].size * self.zoom * 0.5).max(1.5);
                painter.circle_stroke(hover, r, UiStroke::new(1.5, Color32::from_black_alpha(160)));
                painter.circle_stroke(hover, r + 1.0, UiStroke::new(1.0, Color32::from_white_alpha(160)));
            }
        }
        if response.hovered() {
            let icon = match tools.tool {
                Tool::Hand => egui::CursorIcon::Grab,
                Tool::Eyedropper | Tool::Fill | Tool::Select | Tool::Lasso | Tool::Wand => egui::CursorIcon::Crosshair,
                _ => egui::CursorIcon::None,
            };
            ui.ctx().set_cursor_icon(icon);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_view_gestures(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect, tools: &ToolState, doc: &mut Document, cw: f32, ch: f32) {
        let (multi, scroll, zoom_delta, space, time) =
            ui.input(|i| (i.multi_touch(), i.smooth_scroll_delta(), i.zoom_delta(), i.key_down(egui::Key::Space), i.time));

        // Two-finger pinch / pan / rotate, and multi-finger taps.
        if let Some(mt) = multi {
            if self.stroke.is_some() || self.shape.is_some() {
                self.cancel_stroke(doc);
            }
            self.select.abort_gesture(doc);
            let g = self.touch.get_or_insert(TouchGesture { start: time, max_touches: 0, movement: 0.0 });
            g.max_touches = g.max_touches.max(mt.num_touches);
            g.movement += mt.translation_delta.length() + (mt.zoom_delta - 1.0).abs() * 300.0 + mt.rotation_delta.abs() * 150.0;
            self.zoom_around(mt.center_pos, rect, cw, ch, mt.zoom_delta);
            self.rotate_around(mt.center_pos, rect, cw, ch, mt.rotation_delta);
            self.pan += mt.translation_delta;
            return;
        }
        if let Some(g) = self.touch.take() {
            if time - g.start < 0.35 && g.movement < 25.0 {
                self.gesture = Some(if g.max_touches >= 3 { Gesture::Redo } else { Gesture::Undo });
            }
            // Snap a nearly straight canvas back to 0°.
            if self.rotation.to_degrees().abs() < 4.0 {
                self.rotation = 0.0;
            }
        }

        if response.hovered() {
            // Ctrl+wheel or trackpad pinch zooms; plain wheel / two-finger scroll pans.
            if zoom_delta != 1.0 {
                if let Some(p) = response.hover_pos() {
                    self.zoom_around(p, rect, cw, ch, zoom_delta);
                }
            } else if scroll != Vec2::ZERO {
                self.pan += scroll;
            }
        }

        let pan_drag = response.dragged_by(egui::PointerButton::Middle)
            || ((tools.tool == Tool::Hand || space) && response.dragged_by(egui::PointerButton::Primary));
        if pan_drag {
            self.pan += response.drag_delta();
        }
    }

    /// Change zoom keeping the canvas point under `p` in place.
    fn zoom_around(&mut self, p: Pos2, rect: Rect, cw: f32, ch: f32, factor: f32) {
        if factor == 1.0 {
            return;
        }
        let anchor = self.xf(rect, cw, ch).to_canvas(p);
        self.zoom = (self.zoom * factor).clamp(0.05, 32.0);
        let moved = self.xf(rect, cw, ch).to_screen(anchor);
        self.pan += p - moved;
    }

    fn rotate_around(&mut self, p: Pos2, rect: Rect, cw: f32, ch: f32, radians: f32) {
        if radians == 0.0 {
            return;
        }
        let anchor = self.xf(rect, cw, ch).to_canvas(p);
        self.rotation += radians;
        let moved = self.xf(rect, cw, ch).to_screen(anchor);
        self.pan += p - moved;
    }

    fn handle_tool_input(&mut self, ui: &egui::Ui, response: &egui::Response, doc: &mut Document, tools: &mut ToolState, xf: ViewXf) {
        let space = ui.input(|i| i.key_down(egui::Key::Space));
        if tools.tool == Tool::Hand || space || ui.input(|i| i.multi_touch().is_some()) || self.touch.is_some() {
            return;
        }
        let (pressed, down, released, events, time) = ui.input(|i| {
            (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released(), i.events.clone(), i.time)
        });
        if pressed {
            self.pen_force = None;
        }
        for e in &events {
            if let egui::Event::Touch { force: Some(f), phase, .. } = e {
                if matches!(phase, egui::TouchPhase::Start | egui::TouchPhase::Move) && *f > 0.0 {
                    self.pen_force = Some(*f);
                }
            }
        }

        // Alt = quick eyedropper while painting.
        // (ignored mid-stroke so a stroke can't be left open)
        let alt = ui.input(|i| i.modifiers.alt) && self.stroke.is_none() && self.shape.is_none();
        let tool = if alt && matches!(tools.tool, Tool::Brush | Tool::Fill | Tool::Shape) { Tool::Eyedropper } else { tools.tool };

        match tool {
            Tool::Select | Tool::Lasso | Tool::Wand => {
                let params = SelectParams {
                    kind: match tool {
                        Tool::Lasso => SelectKind::Lasso,
                        Tool::Wand => SelectKind::Wand,
                        _ => SelectKind::Rect,
                    },
                    lasso_mode: tools.lasso_mode,
                    color: tools.color_rgba(),
                    fill: tools.fill,
                    wand_all_layers: tools.fill_all_layers,
                    editable: self.editable_layer_quiet(doc),
                };
                if let Some(msg) = self.select.input(ui, response, doc, xf, params) {
                    self.message = Some(msg);
                }
            }
            Tool::Shape => {
                let shift = ui.input(|i| i.modifiers.shift);
                if pressed && response.hovered() && self.shape.is_none() && self.editable_layer(doc) {
                    if let Some(p) = response.interact_pointer_pos() {
                        self.shape = Some((xf.to_canvas(p), None, None));
                    }
                }
                if let (Some(start), Some(p)) = (self.shape.as_ref().map(|s| s.0), response.interact_pointer_pos()) {
                    if down {
                        self.preview_shape(doc, tools, start, xf.to_canvas(p), shift || tools.shape_constrain);
                    }
                }
                if self.shape.is_some() && (released || !down) {
                    self.finish_shape(doc, tools);
                }
            }
            Tool::Brush | Tool::Eraser => {
                if pressed && response.hovered() && self.stroke.is_none() {
                    self.begin_stroke(doc, tools);
                    self.last_sample = None;
                }
                if self.stroke.is_some() {
                    let mut positions: Vec<Pos2> =
                        events.iter().filter_map(|e| if let egui::Event::PointerMoved(p) = e { Some(*p) } else { None }).collect();
                    if positions.is_empty() && pressed {
                        positions.extend(response.interact_pointer_pos());
                    }
                    for p in positions {
                        let pressure = self.pressure(p, time, tools.simulate_pressure);
                        let c = xf.to_canvas(p);
                        self.add_stroke_point(doc, StrokePoint { x: c.x, y: c.y, pressure });
                    }
                    if released || !down {
                        self.end_stroke(doc, tools);
                    }
                }
            }
            Tool::Fill => {
                if response.clicked() {
                    if let Some(p) = response.interact_pointer_pos() {
                        let c = xf.to_canvas(p);
                        self.fill(doc, tools, c.x.floor() as i32, c.y.floor() as i32);
                    }
                }
            }
            Tool::Eyedropper => {
                if response.clicked() || response.dragged() {
                    if let Some(p) = response.interact_pointer_pos() {
                        let c = xf.to_canvas(p);
                        self.pick_color(doc, tools, c.x.floor() as i32, c.y.floor() as i32);
                    }
                }
            }
            Tool::Text => {
                if response.clicked() && self.editable_layer(doc) {
                    if let Some(p) = response.interact_pointer_pos() {
                        let c = xf.to_canvas(p);
                        self.text_at = Some((c.x, c.y));
                    }
                }
            }
            Tool::Hand => {}
        }
    }

    fn pressure(&mut self, p: Pos2, time: f64, simulate: bool) -> f32 {
        if let Some(f) = self.pen_force {
            return f.clamp(0.05, 1.0);
        }
        if !simulate {
            return 1.0;
        }
        // Faster movement = thinner line, like a real brush.
        let pressure = match self.last_sample {
            Some((lp, lt, prev)) if time > lt => {
                let speed = (p - lp).length() / (time - lt) as f32;
                let target = (1.1 - speed / 2500.0).clamp(0.25, 1.0);
                prev + (target - prev) * 0.3
            }
            Some((_, _, prev)) => prev,
            None => 0.6,
        };
        self.last_sample = Some((p, time, pressure));
        pressure
    }

    fn editable_layer_quiet(&self, doc: &Document) -> bool {
        doc.project.layer(doc.layer).is_some_and(|l| l.props.paintable())
    }

    /// Redraw the shape preview from `a` to `b` with the current brush.
    fn preview_shape(&mut self, doc: &mut Document, tools: &ToolState, a: Pos2, b: Pos2, constrain: bool) {
        // Nothing moved: keep the current preview.
        if self.shape.as_ref().is_some_and(|s| s.2 == Some(b) && s.1.is_some()) {
            return;
        }
        let Some((start, old, _)) = self.shape.take() else { return };
        let owner = doc.project.cel_owner(doc.frame_id(), doc.layer);
        let layer = doc.layer;
        let Some(cel) = doc.project.cel_at_mut(owner, layer) else { return };
        if let Some(st) = old {
            let r = st.cancel(cel);
            doc.redraw.add(r);
        }
        let alpha_lock = doc.project.layer(layer).is_some_and(|l| l.props.alpha_lock);
        let center = (doc.project.canvas.width as f32 * 0.5, doc.project.canvas.height as f32 * 0.5);
        let mut settings = tools.stroke_settings(alpha_lock, center);
        settings.stabilizer = 0.0;
        let mut st = Stroke::new(tools.library[tools.brush].clone(), settings, owner, layer, 99);
        let Some(cel) = doc.project.cel_at_mut(owner, layer) else { return };
        for (x, y) in shape_points(tools.shape, (a.x, a.y), (b.x, b.y), constrain) {
            st.add_point(cel, StrokePoint { x, y, pressure: 1.0 });
        }
        let dirty = st.take_dirty();
        doc.redraw.add(dirty);
        self.shape = Some((start, Some(st), Some(b)));
    }

    fn finish_shape(&mut self, doc: &mut Document, tools: &mut ToolState) {
        let Some((_, Some(st), _)) = self.shape.take() else { return };
        let (owner, layer) = (st.frame(), st.layer());
        let Some(cel) = doc.project.cel_at_mut(owner, layer) else { return };
        let tiles_x = cel.tiles_x();
        if let Some(edit) = st.finish(cel) {
            let area = edit.tiles.iter().fold(IRect::EMPTY, |acc, c| acc.union(&tile_rect(c.index % tiles_x, c.index / tiles_x)));
            doc.history.push_applied(Edit::Pixels(edit));
            doc.invalidate_frame(owner, Some(area));
            tools.push_recent();
        }
    }

    fn editable_layer(&mut self, doc: &Document) -> bool {
        let Some(layer) = doc.project.layer(doc.layer) else { return false };
        if layer.props.video.is_some() {
            self.message = Some(tr("Es una capa de video: dibuja en otra capa").into());
            return false;
        }
        if layer.props.locked {
            self.message = Some(tr("La capa está bloqueada").into());
            return false;
        }
        if !layer.props.visible {
            self.message = Some(tr("La capa está oculta").into());
            return false;
        }
        true
    }

    fn begin_stroke(&mut self, doc: &mut Document, tools: &ToolState) {
        if !self.editable_layer(doc) {
            return;
        }
        let alpha_lock = doc.project.layer(doc.layer).is_some_and(|l| l.props.alpha_lock);
        let seed = doc.history.revision().wrapping_mul(2654435761).wrapping_add(7);
        let owner = doc.project.cel_owner(doc.frame_id(), doc.layer);
        let center = (doc.project.canvas.width as f32 * 0.5, doc.project.canvas.height as f32 * 0.5);
        self.stroke = Some(Stroke::new(tools.active_brush().clone(), tools.stroke_settings(alpha_lock, center), owner, doc.layer, seed));
    }

    fn add_stroke_point(&mut self, doc: &mut Document, pt: StrokePoint) {
        let Some(stroke) = &mut self.stroke else { return };
        let Some(cel) = doc.project.cel_at_mut(stroke.frame(), stroke.layer()) else { return };
        stroke.add_point(cel, pt);
        let dirty = stroke.take_dirty();
        if !dirty.is_empty() {
            doc.redraw.add(dirty);
        }
    }

    fn end_stroke(&mut self, doc: &mut Document, tools: &mut ToolState) {
        let Some(stroke) = self.stroke.take() else { return };
        let (owner, lid) = (stroke.frame(), stroke.layer());
        let Some(cel) = doc.project.cel_at_mut(owner, lid) else { return };
        let tiles_x = cel.tiles_x();
        if let Some(edit) = stroke.finish(cel) {
            // Redraw exactly the tiles the stroke changed (end taper included).
            let area = edit.tiles.iter().fold(IRect::EMPTY, |acc, c| acc.union(&tile_rect(c.index % tiles_x, c.index / tiles_x)));
            doc.history.push_applied(Edit::Pixels(edit));
            doc.invalidate_frame(owner, Some(area));
            if tools.active_brush().mode == BrushMode::Paint {
                tools.push_recent();
            }
        }
        self.pen_force = None;
    }

    fn fill(&mut self, doc: &mut Document, tools: &mut ToolState, x: i32, y: i32) {
        if !self.editable_layer(doc) {
            return;
        }
        let lid = doc.layer;
        let shared = doc.project.layer(lid).is_some_and(|l| l.props.shared);
        if !tools.fill_every_drawing || shared {
            if Self::fill_at(doc, tools, doc.frame, x, y) {
                tools.push_recent();
            }
            return;
        }
        // The same spot on every drawing of the layer, as one undo step.
        let starts: Vec<usize> = trazo_core::timeline::blocks(&doc.project, lid).iter().map(|b| b.start).collect();
        let mark = doc.history.undo_len();
        let filled = starts.into_iter().filter(|&f| Self::fill_at(doc, tools, f, x, y)).count();
        doc.history.group_since(mark);
        if filled > 0 {
            tools.push_recent();
            self.message = Some(tf!("Relleno aplicado en {} dibujos", filled));
        }
    }

    /// Flood fill at (x, y) on the current layer's drawing shown at `frame`.
    /// Returns whether anything changed.
    fn fill_at(doc: &mut Document, tools: &ToolState, frame: usize, x: i32, y: i32) -> bool {
        let (w, h) = (doc.project.canvas.width, doc.project.canvas.height);
        let lid = doc.layer;
        let Some(fid) = doc.project.frames.get(frame).map(|f| f.id) else { return false };
        let owner = doc.project.cel_owner(fid, lid);
        let has_reference = doc.project.layers.iter().any(|l| l.props.reference);
        let reference = if has_reference {
            // Line-art layers marked "reference" define the edges.
            let mut refs = doc.project.clone();
            refs.layers.retain(|l| l.props.reference);
            for l in &mut refs.layers {
                l.props.visible = true;
                l.props.opacity = 1.0;
            }
            flatten_frame(&refs, frame)
        } else if tools.fill_all_layers {
            flatten_frame(&doc.project, frame)
        } else {
            doc.project.visible_cel(frame, lid).cloned().unwrap_or_else(|| TiledImage::new(w, h))
        };
        let alpha_lock = doc.project.layer(lid).is_some_and(|l| l.props.alpha_lock);
        let color = tools.color_rgba();
        let opts = trazo_core::fill::FillOptions { alpha_lock, ..tools.fill };
        let Some(cel) = doc.project.cel_at_mut(owner, lid) else { return false };
        let mut edit = PixelEditBuilder::new();
        let r = flood_fill(cel, &reference, x, y, color, opts, &mut edit);
        match edit.finish(cel, owner, lid) {
            Some(e) => {
                doc.history.push_applied(Edit::Pixels(e));
                doc.invalidate_frame(owner, Some(r));
                true
            }
            None => false,
        }
    }

    fn pick_color(&mut self, doc: &Document, tools: &mut ToolState, x: i32, y: i32) {
        let r = IRect::new(x, y, x + 1, y + 1);
        if r.intersect(&IRect::from_size(doc.project.canvas.width, doc.project.canvas.height)).is_empty() {
            return;
        }
        let px = if tools.pick_layer_only {
            let p = doc.project.visible_cel(doc.frame, doc.layer).map(|c| c.pixel(x, y)).unwrap_or([0; 4]);
            if p[3] == 0 {
                return; // nothing on this layer here: keep the current color
            }
            p
        } else {
            let mut px = Vec::new();
            render_region(&doc.project, doc.frame, r, &RenderOptions { background: true, ..Default::default() }, &mut px);
            [px[0], px[1], px[2], px[3]]
        };
        let c = Rgba::from_premul8(px).to_rgba8();
        tools.set_color(Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 255));
    }

    /// Bring the GPU texture up to date with the document.
    fn sync_texture(&mut self, ctx: &egui::Context, doc: &mut Document, frame: usize, onion: Option<OnionSkin>, playing: bool) {
        let (w, h) = (doc.project.canvas.width, doc.project.canvas.height);
        let shown = Shown { frame, onion };
        let full_rect = IRect::from_size(w, h);
        // While playing, render at the size the canvas is shown on screen
        // (a phone shows a 1080p canvas at about half size: 4× less work).
        let step = if playing && onion.is_none() {
            let screen_px_per_canvas_px = self.zoom * ctx.pixels_per_point();
            ((1.0 / screen_px_per_canvas_px.max(0.01)) + 0.25).floor().clamp(1.0, 4.0) as usize
        } else {
            1
        };
        let (tw, th) = ((w as usize).div_ceil(step), (h as usize).div_ceil(step));
        let size_ok = self.texture.as_ref().is_some_and(|t| t.size() == [tw, th]);
        let redraw = if !size_ok || self.shown != Some(shown) || self.tex_step != step { Redraw::Full } else { doc.redraw };
        doc.redraw = Redraw::None;
        self.shown = Some(shown);
        self.tex_step = step;

        if self.cache_version != doc.content_version {
            self.playback_cache.clear();
            self.playback_cache_bytes = 0;
            self.cache_version = doc.content_version;
        }

        let opts = RenderOptions { background: true, onion, ..Default::default() };
        match redraw {
            Redraw::None => {}
            Redraw::Full => {
                let cached = (playing && onion.is_none()).then(|| self.playback_cache.get(&frame).filter(|c| c.step == step)).flatten();
                let img = match cached.and_then(|c| c.unpack()) {
                    Some(img) => img,
                    None => {
                        let (ow, oh) = render_region_scaled(&doc.project, frame, full_rect, step, &opts, &mut self.scratch);
                        if playing && onion.is_none() {
                            // Keep it (compressed) for the next loop.
                            let data = lz4_flex::compress_prepend_size(&self.scratch);
                            if self.playback_cache_bytes + data.len() <= PLAYBACK_CACHE_BYTES {
                                self.playback_cache_bytes += data.len();
                                if let Some(old) = self.playback_cache.insert(frame, CachedFrame { step, w: ow, h: oh, data }) {
                                    self.playback_cache_bytes -= old.data.len();
                                }
                            }
                        }
                        ColorImage::from_rgba_premultiplied([ow, oh], &self.scratch)
                    }
                };
                match &mut self.texture {
                    Some(t) if size_ok => t.set(img, TEX_OPTIONS),
                    _ => self.texture = Some(ctx.load_texture("canvas", img, TEX_OPTIONS)),
                }
            }
            Redraw::Rect(r) => {
                let r = r.intersect(&full_rect);
                if r.is_empty() {
                    return;
                }
                render_region(&doc.project, frame, r, &opts, &mut self.scratch);
                let img = ColorImage::from_rgba_premultiplied([r.width() as usize, r.height() as usize], &self.scratch);
                if let Some(t) = &mut self.texture {
                    t.set_partial([r.x0 as usize, r.y0 as usize], img, TEX_OPTIONS);
                }
            }
        }
    }
}
