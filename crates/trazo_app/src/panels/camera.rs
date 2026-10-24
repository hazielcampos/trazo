//! Camera panel: a start and an end framing, dragged on a small preview.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use trazo_core::camera::CamPose;

use crate::app::TrazoApp;
use crate::i18n::tr;

#[derive(Default)]
pub struct CameraUi {
    pub open: bool,
    /// Which framing the controls change.
    pub editing_end: bool,
    /// Look through the camera while playing.
    pub hide_view: bool,
    preview: Option<(u64, usize, egui::TextureHandle)>,
}

impl CameraUi {
    /// Show the camera's view while playing.
    pub fn view_on(&self) -> bool {
        !self.hide_view
    }
}

pub fn show(app: &mut TrazoApp, ctx: &egui::Context) {
    if !app.ui.camera.open {
        return;
    }
    let mut open = true;
    let mut changed = false;
    let (w, h) = (app.doc.project.canvas.width, app.doc.project.canvas.height);
    let tex = preview_texture(app, ctx);
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Cámara"), 460.0).min_height(0.0).open(&mut open).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        let ui_state = &mut app.ui.camera;
        let cam = &mut app.doc.project.canvas.camera;
        changed |= crate::widgets::switch_row(ui, tr("Usar cámara"), &mut cam.enabled);
        ui.label(
            RichText::new(tr("La cámara va del encuadre inicial al final durante toda la animación. Arrastra el recuadro en la vista previa; se aplica al reproducir y al exportar."))
                .size(12.0)
                .color(crate::theme::TEXT2),
        );
        ui.add_enabled_ui(cam.enabled, |ui| {
            let mut idx = ui_state.editing_end as usize;
            if crate::widgets::segmented(ui, &mut idx, &[tr("Inicio"), tr("Final")], ui.available_width()) {
                ui_state.editing_end = idx == 1;
            }
            // Preview: the frame with both framings on top.
            let avail = ui.available_width();
            let scale = (avail / w as f32).min(260.0 / h as f32);
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(w as f32 * scale, h as f32 * scale), Sense::click_and_drag());
            let p = ui.painter_at(rect.expand(2.0));
            p.rect_filled(rect, 2.0, Color32::from_gray(40));
            if let Some(t) = &tex {
                p.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            let to_screen = |(x, y): (f32, f32)| Pos2::new(rect.left() + x * scale, rect.top() + y * scale);
            for (pose, is_end) in [(cam.start, false), (cam.end, true)] {
                let pts: Vec<Pos2> = pose.corners(w, h).map(to_screen).to_vec();
                let active = is_end == ui_state.editing_end;
                let color = if is_end { crate::theme::ACCENT } else { Color32::WHITE };
                p.add(egui::Shape::closed_line(pts.clone(), Stroke::new(if active { 3.0 } else { 1.5 }, color.gamma_multiply(if active { 1.0 } else { 0.6 }))));
                p.text(pts[0] + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, if is_end { tr("Final") } else { tr("Inicio") }, egui::FontId::proportional(11.0), color);
            }
            let pose = if ui_state.editing_end { &mut cam.end } else { &mut cam.start };
            if resp.dragged() {
                let d = resp.drag_delta() / scale;
                pose.x = (pose.x + d.x / w as f32).clamp(-0.5, 1.5);
                pose.y = (pose.y + d.y / h as f32).clamp(-0.5, 1.5);
                changed = true;
            }
            ui.horizontal(|ui| {
                ui.label(tr("Zoom"));
                changed |= ui.add(egui::Slider::new(&mut pose.zoom, 0.5..=6.0).logarithmic(true).suffix("×")).changed();
            });
            ui.horizontal(|ui| {
                ui.label(tr("Giro"));
                let mut deg = pose.angle.to_degrees();
                if ui.add(egui::Slider::new(&mut deg, -180.0..=180.0).suffix("°")).changed() {
                    pose.angle = deg.to_radians();
                    changed = true;
                }
            });
            let (mut reset, mut copy) = (false, false);
            ui.horizontal(|ui| {
                reset = ui.button(tr("Centrar")).clicked();
                copy = ui.button(if ui_state.editing_end { tr("Igual que el inicio") } else { tr("Igual que el final") }).clicked();
            });
            let other = if ui_state.editing_end { cam.start } else { cam.end };
            let pose = if ui_state.editing_end { &mut cam.end } else { &mut cam.start };
            if reset {
                *pose = CamPose::default();
                changed = true;
            }
            if copy {
                *pose = other;
                changed = true;
            }
            ui.label(RichText::new(tr("Espaciado")).size(13.0).color(crate::theme::TEXT2));
            use trazo_core::transform::Ease;
            let labels: Vec<&str> = Ease::ALL.iter().map(|e| crate::panels::dialogs::ease_label(*e)).collect();
            let mut idx = Ease::ALL.iter().position(|e| *e == cam.ease).unwrap_or(0);
            if crate::widgets::segmented(ui, &mut idx, &labels, ui.available_width()) {
                cam.ease = Ease::ALL[idx];
                changed = true;
            }
            let mut view = !ui_state.hide_view;
            if crate::widgets::switch_row(ui, tr("Ver a través de la cámara al reproducir"), &mut view) {
                ui_state.hide_view = !view;
            }
        });
    });
    if changed {
        app.doc.mark_settings_changed();
    }
    if !open {
        app.ui.camera.open = false;
    }
}

/// Small picture of the current frame for the camera panel (re-rendered only
/// when the drawing or the frame changes).
fn preview_texture(app: &mut TrazoApp, ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let key = (app.doc.content_version, app.doc.frame);
    if let Some((v, f, t)) = &app.ui.camera.preview {
        if (*v, *f) == key {
            return Some(t.clone());
        }
    }
    let p = &app.doc.project;
    let (w, h) = (p.canvas.width, p.canvas.height);
    let step = ((w as f32 / 480.0).max(h as f32 / 270.0).ceil() as u32).max(1);
    let mut px = Vec::new();
    let opts = trazo_core::composite::RenderOptions { background: true, ..Default::default() };
    trazo_core::composite::render_region(p, app.doc.frame.min(p.frames.len().saturating_sub(1)), trazo_core::IRect::new(0, 0, w as i32, h as i32), &opts, &mut px);
    let (sw, sh) = (w / step, h / step);
    let mut small = Vec::with_capacity((sw * sh * 4) as usize);
    for y in 0..sh {
        for x in 0..sw {
            let o = (((y * step) * w + x * step) * 4) as usize;
            small.extend_from_slice(&px[o..o + 4]);
        }
    }
    let img = egui::ColorImage::from_rgba_premultiplied([sw as usize, sh as usize], &small);
    let t = ctx.load_texture("camera_preview", img, egui::TextureOptions::LINEAR);
    app.ui.camera.preview = Some((key.0, key.1, t.clone()));
    Some(t)
}
