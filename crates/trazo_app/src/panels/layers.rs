//! Layer list and layer properties.

use crate::i18n::tr;
use trazo_core::{ops, BlendMode, Edit, LayerProps};
use egui::{RichText, Vec2};

use crate::app::TrazoApp;
use crate::icons::{Icon, IconButton};

pub fn show(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let mut import_pending: Option<u8> = None;
    show_inner(app, ui, &mut import_pending);
    let ctx = ui.ctx().clone();
    match import_pending {
        Some(0) => crate::files::request(app, &ctx, crate::files::PickPurpose::ImageLayer),
        Some(1) => crate::files::request(app, &ctx, crate::files::PickPurpose::ImageReference),
        Some(2) => crate::files::request(app, &ctx, crate::files::PickPurpose::ImageSequence),
        Some(3) => crate::files::request(app, &ctx, crate::files::PickPurpose::Video),
        _ => {}
    }
}

fn show_inner(app: &mut TrazoApp, ui: &mut egui::Ui, import_pending: &mut Option<u8>) {
    let doc = &mut app.doc;
    ui.horizontal(|ui| {
        if ui.add(IconButton::new(Icon::Plus)).on_hover_text(tr("Nueva capa")).clicked() {
            let idx = doc.layer_index() + 1;
            let name = crate::i18n::default_layer_name(doc.project.layers.len() + 1);
            doc.layer = ops::add_layer(&mut doc.project, &mut doc.history, idx, name);
            doc.invalidate_all();
        }
        if ui.add(IconButton::new(Icon::Duplicate)).on_hover_text(tr("Duplicar capa")).clicked() {
            if let Some(id) = ops::duplicate_layer(&mut doc.project, &mut doc.history, doc.layer) {
                doc.layer = id;
                doc.invalidate_all();
            }
        }
        if ui.add(IconButton::new(Icon::Merge)).on_hover_text(tr("Combinar con la de abajo")).clicked() {
            if let Some(id) = ops::merge_down(&mut doc.project, &mut doc.history, doc.layer) {
                doc.layer = id;
                doc.invalidate_all();
            }
        }
        if ui.add(IconButton::new(Icon::Trash)).on_hover_text(tr("Borrar capa")).clicked() && ops::delete_layer(&mut doc.project, &mut doc.history, doc.layer) {
            doc.invalidate_all();
        }
        if ui.add(IconButton::new(Icon::Clear)).on_hover_text(tr("Limpiar esta capa en este fotograma")).clicked() {
            let (fid, lid) = (doc.frame_id(), doc.layer);
            let drawing = doc.project.layer(lid).is_some_and(|l| l.props.video.is_none());
            if drawing && ops::clear_cel(&mut doc.project, &mut doc.history, fid, lid) {
                doc.invalidate_frame(fid, None);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        let ctx = ui.ctx().clone();
        if ui.add(IconButton::new(Icon::Image).keep_text().text(tr("Imagen"))).on_hover_text(tr("Importar una imagen en una capa nueva")).clicked() {
            *import_pending = Some(0);
        }
        if ui.add(IconButton::new(Icon::Background).keep_text().text(tr("Referencia"))).on_hover_text(tr("Imagen fija y semitransparente para calcar")).clicked() {
            *import_pending = Some(1);
        }
        if ui.add(IconButton::new(Icon::Film).keep_text().text(tr("Rotoscopia"))).on_hover_text(tr("Varias imágenes: una por fotograma")).clicked() {
            *import_pending = Some(2);
        }
        if ui.add(IconButton::new(Icon::Film).keep_text().text(tr("Video"))).on_hover_text(tr("Un video o GIF en una capa, para calcar encima")).clicked() {
            *import_pending = Some(3);
        }
        let _ = ctx;
    });
    ui.separator();

    // Top layer first, like every paint app.
    let n = doc.project.layers.len();
    let mut action: Option<LayerAction> = None;
    egui::ScrollArea::vertical().max_height((ui.available_height() * 0.4).max(110.0)).show(ui, |ui| {
        for i in (0..n).rev() {
            let layer = &doc.project.layers[i];
            let selected = layer.id == doc.layer;
            let frame = egui::Frame::group(ui.style()).fill(if selected {
                crate::theme::ACCENT.linear_multiply(0.25)
            } else {
                ui.visuals().faint_bg_color
            });
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let eye = if layer.props.visible { Icon::Eye } else { Icon::EyeOff };
                    if ui.add(IconButton::new(eye)).on_hover_text(tr("Mostrar / ocultar")).clicked() {
                        action = Some(LayerAction::ToggleVisible(i));
                    }
                    let lock = if layer.props.locked { Icon::Lock } else { Icon::Unlock };
                    if ui.add(IconButton::new(lock)).on_hover_text(tr("Bloquear")).clicked() {
                        action = Some(LayerAction::ToggleLock(i));
                    }
                    if layer.props.shared || layer.props.video.is_some() {
                        let icon = if layer.props.video.is_some() { Icon::Film } else { Icon::Background };
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::hover());
                        crate::icons::paint(ui.painter(), r, icon, ui.visuals().weak_text_color());
                    }
                    let name = RichText::new(crate::i18n::layer_name(&layer.props.name));
                    let name = if selected { name.strong() } else { name };
                    let name_w = (ui.available_width() - 2.0 * 44.0).max(40.0);
                    let r = ui.scope(|ui| {
                        ui.set_max_width(name_w);
                        ui.add(egui::Label::new(name).truncate().sense(egui::Sense::click()))
                    });
                    if r.inner.clicked() {
                        action = Some(LayerAction::Select(i));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(i > 0, IconButton::new(Icon::Down)).on_hover_text(tr("Bajar capa")).clicked() {
                            action = Some(LayerAction::Move(i, i - 1));
                        }
                        if ui.add_enabled(i + 1 < n, IconButton::new(Icon::Up)).on_hover_text(tr("Subir capa")).clicked() {
                            action = Some(LayerAction::Move(i, i + 1));
                        }
                    });
                });
            });
        }
    });

    if let Some(a) = action {
        match a {
            LayerAction::Select(i) => {
                commit_pending(ui, doc);
                doc.layer = doc.project.layers[i].id;
            }
            LayerAction::ToggleVisible(i) | LayerAction::ToggleLock(i) => {
                let l = doc.project.layers[i].clone();
                let mut props = l.props.clone();
                if matches!(a, LayerAction::ToggleVisible(_)) {
                    props.visible = !props.visible;
                } else {
                    props.locked = !props.locked;
                }
                ops::set_layer_props(&mut doc.project, &mut doc.history, l.id, props);
                doc.invalidate_all();
            }
            LayerAction::Move(from, to) => {
                ops::move_layer(&mut doc.project, &mut doc.history, from, to);
                doc.invalidate_all();
            }
        }
    }

    // Properties of the selected layer (scrolls when they don't fit).
    egui::ScrollArea::vertical().id_salt("layer_props").auto_shrink([false, false]).show(ui, |ui| layer_properties(ui, doc));
}

fn layer_properties(ui: &mut egui::Ui, doc: &mut crate::app::Document) {
    // Properties of the selected layer. Changes preview live and become one
    // undo step when the gesture ends (slider released, text field left…).
    ui.separator();
    let i = doc.layer_index();
    let id = doc.project.layers[i].id;
    let mut props = doc.project.layers[i].props.clone();
    let (mut changed, mut commit) = (false, false);
    ui.horizontal(|ui| {
        ui.label(tr("Nombre"));
        // Default names are shown (and edited) in the current language.
        let shown = crate::i18n::layer_name(&props.name).into_owned();
        let mut text = shown.clone();
        let r = ui.text_edit_singleline(&mut text);
        if text != shown {
            props.name = text;
        }
        changed |= r.changed();
        commit |= r.lost_focus();
    });
    let r = ui.add(egui::Slider::new(&mut props.opacity, 0.0..=1.0).text(tr("Opacidad")));
    changed |= r.changed();
    commit |= r.drag_stopped() || (r.changed() && !r.dragged());
    egui::ComboBox::from_label(tr("Fusión")).selected_text(tr(props.blend.label())).show_ui(ui, |ui| {
        for m in BlendMode::ALL {
            if ui.selectable_value(&mut props.blend, m, tr(m.label())).changed() {
                changed = true;
                commit = true;
            }
        }
    });
    if props.video.is_some() {
        let tick = doc.project.tick_of_frame(doc.frame) as i64;
        let fps = doc.project.canvas.fps.max(1);
        let (cw, ch) = (doc.project.canvas.width, doc.project.canvas.height);
        let mut extend_to = None;
        if let Some(v) = props.video.as_mut() {
            let (c, k, e) = video_props(ui, v, tick, fps, cw, ch);
            changed |= c;
            commit |= k;
            extend_to = e;
        }
        if let Some((end, hold)) = extend_to {
            let target = doc.project.layers.iter().rev().find(|l| l.props.video.is_none() && !l.props.shared).map(|l| l.id);
            ops::extend_to_ticks(&mut doc.project, &mut doc.history, end, hold, target);
            doc.invalidate_all();
        }
    } else {
        drawing_props(ui, &mut props, &mut changed, &mut commit);
        let mut shared = props.shared;
        if crate::widgets::switch_row(ui, tr("Fondo fijo en todos los fotogramas"), &mut shared) {
            ops::set_layer_shared(&mut doc.project, &mut doc.history, id, shared, doc.frame);
            doc.invalidate_all();
            return;
        }
    }
    finish_props(ui, doc, i, id, props, changed, commit);
}

/// Options that only make sense for drawing layers.
fn drawing_props(ui: &mut egui::Ui, props: &mut LayerProps, changed: &mut bool, commit: &mut bool) {
    if crate::widgets::switch_row(ui, tr("Colorear dentro (bloquear transparencia)"), &mut props.alpha_lock) {
        *changed = true;
        *commit = true;
    }
    if crate::widgets::switch_row(ui, tr("Mostrar en papel cebolla"), &mut props.onion_skin) {
        *changed = true;
        *commit = true;
    }
    if crate::widgets::switch_row(ui, tr("Recortar a la capa de abajo"), &mut props.clip) {
        *changed = true;
        *commit = true;
    }
    if crate::widgets::switch_row(ui, tr("Líneas para el relleno"), &mut props.reference) {
        *changed = true;
        *commit = true;
    }
}

fn finish_props(ui: &mut egui::Ui, doc: &mut crate::app::Document, i: usize, id: trazo_core::LayerId, props: LayerProps, changed: bool, commit: bool) {
    let key = egui::Id::unique(("layer_props_start", id));
    ui.data_mut(|d| d.insert_temp(egui::Id::unique("layer_props_pending"), id));
    if changed {
        let old = doc.project.layers[i].props.clone();
        ui.data_mut(|d| {
            if d.get_temp::<LayerProps>(key).is_none() {
                d.insert_temp(key, old);
            }
        });
        doc.project.layers[i].props = props.clone();
        doc.invalidate_all();
    }
    if commit {
        let start = ui.data_mut(|d| {
            let v = d.get_temp::<LayerProps>(key);
            d.remove::<LayerProps>(key);
            v
        });
        if let Some(before) = start {
            if before != props {
                doc.history.push_applied(Edit::LayerProps { layer: id, before, after: props });
            }
        }
    }
}

/// Position, trims, look and size of a video layer. Returns (changed,
/// commit, extend the animation to this tick with frames of this hold).
fn video_props(ui: &mut egui::Ui, v: &mut trazo_core::video::VideoLayer, tick: i64, fps: u32, cw: u32, ch: u32) -> (bool, bool, Option<(u32, u32)>) {
    use trazo_core::video::VideoTint;
    let (mut changed, mut commit, mut extend) = (false, false, None);
    let Some(media) = v.data.clone() else {
        ui.colored_label(ui.visuals().warn_fg_color, tr("No se encontraron las imágenes de este video (se conservan en el archivo)"));
        return (false, false, None);
    };
    // (changed, commit) for a drag value: commit when the gesture ends.
    let drag = |r: &egui::Response| (r.changed(), r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()));
    ui.label(RichText::new(tf!("{} imágenes · {}×{} · {} MB", media.len(), media.width, media.height, crate::i18n::num((media.bytes() as f64 / 1e6) as f32, 1))).small().weak());
    if crate::widgets::switch_row(ui, tr("Solo referencia (no se exporta)"), &mut v.reference) {
        changed = true;
        commit = true;
    }
    ui.horizontal(|ui| {
        ui.label(tr("Tinte"));
        for t in VideoTint::ALL {
            if ui.selectable_label(v.tint == t, tr(t.label())).on_hover_text(tr("Gris o azul para que tus líneas resalten sobre el video")).clicked() && v.tint != t {
                v.tint = t;
                changed = true;
                commit = true;
            }
        }
    });
    let fps_f = fps.max(1) as f64;
    ui.horizontal(|ui| {
        ui.label(tr("Empieza en"));
        let mut secs = v.start as f64 / fps_f;
        let r = ui.add(egui::DragValue::new(&mut secs).range(-3600.0..=3600.0).speed(1.0 / fps_f).suffix(" s").fixed_decimals(2));
        v.start = (secs * fps_f).round() as i64;
        let (c, k) = drag(&r);
        changed |= c;
        commit |= k;
        if ui.button(tr("Aquí")).on_hover_text(tr("Que empiece en el fotograma actual")).clicked() {
            v.start = tick;
            changed = true;
            commit = true;
        }
    });
    let mfps = media.fps.max(0.1) as f64;
    let n = media.len() as u32;
    ui.horizontal(|ui| {
        ui.label(tr("Recortar: desde"));
        let mut a = v.trim_start as f64 / mfps;
        let r = ui.add(egui::DragValue::new(&mut a).range(0.0..=n as f64 / mfps).speed(0.05).suffix(" s").fixed_decimals(1));
        v.trim_start = ((a * mfps).round() as u32).min(n.saturating_sub(1));
        let (c, k) = drag(&r);
        changed |= c;
        commit |= k;
        ui.label(tr("hasta"));
        let mut b = v.trim_end as f64 / mfps;
        let r = ui.add(egui::DragValue::new(&mut b).range(0.0..=n as f64 / mfps).speed(0.05).suffix(" s").fixed_decimals(1));
        v.trim_end = ((b * mfps).round() as u32).clamp(v.trim_start + 1, n);
        let (c, k) = drag(&r);
        changed |= c;
        commit |= k;
    });
    ui.horizontal(|ui| {
        ui.label(tr("Escala"));
        let mut s = v.scale() * 100.0;
        let r = ui.add(egui::DragValue::new(&mut s).range(5.0..=2000.0).speed(1.0).suffix(" %").fixed_decimals(0));
        if r.changed() {
            v.set_scale(s / 100.0);
        }
        let (c, k) = drag(&r);
        changed |= c;
        commit |= k;
        ui.label(tr("X"));
        let r = ui.add(egui::DragValue::new(&mut v.x).range(-(cw as i32) * 4..=cw as i32 * 4).speed(2.0));
        let (c, k) = drag(&r);
        changed |= c;
        commit |= k;
        ui.label(tr("Y"));
        let r = ui.add(egui::DragValue::new(&mut v.y).range(-(ch as i32) * 4..=ch as i32 * 4).speed(2.0));
        let (c, k) = drag(&r);
        changed |= c;
        commit |= k;
    });
    ui.horizontal_wrapped(|ui| {
        let (mw, mh) = (media.width as f32, media.height as f32);
        let mut place = |v: &mut trazo_core::video::VideoLayer, f: f32| {
            v.set_scale(f);
            v.x = (cw as i32 - v.w as i32) / 2;
            v.y = (ch as i32 - v.h as i32) / 2;
            changed = true;
            commit = true;
        };
        if ui.button(tr("Ajustar al lienzo")).on_hover_text(tr("Todo el video visible, centrado")).clicked() {
            place(v, (cw as f32 / mw).min(ch as f32 / mh));
        }
        if ui.button(tr("Llenar el lienzo")).on_hover_text(tr("Sin bandas, recortando los bordes")).clicked() {
            place(v, (cw as f32 / mw).max(ch as f32 / mh));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("Crear fotogramas hasta el final:")).on_hover_text(tr("Añade fotogramas vacíos para dibujar, uno cada 1, 2 o 3 imágenes del video"));
        for (label, hold) in [(tr("a unos"), 1), (tr("a doses"), 2), (tr("a treses"), 3)] {
            if ui.button(label).clicked() {
                extend = Some((v.end_tick(fps).clamp(0, 200_000) as u32, hold));
            }
        }
    });
    v.clamp();
    (changed, commit, extend)
}

/// Record an unfinished edit (e.g. a rename still being typed) of the
/// previously shown layer as one undo step.
fn commit_pending(ui: &egui::Ui, doc: &mut crate::app::Document) {
    let Some(id) = ui.data(|d| d.get_temp::<trazo_core::LayerId>(egui::Id::unique("layer_props_pending"))) else { return };
    let key = egui::Id::unique(("layer_props_start", id));
    let start = ui.data_mut(|d| {
        let v = d.get_temp::<LayerProps>(key);
        d.remove::<LayerProps>(key);
        v
    });
    if let (Some(before), Some(i)) = (start, doc.project.layer_index(id)) {
        let after = doc.project.layers[i].props.clone();
        if before != after {
            doc.history.push_applied(Edit::LayerProps { layer: id, before, after });
        }
    }
}

#[derive(Clone, Copy)]
enum LayerAction {
    Select(usize),
    ToggleVisible(usize),
    ToggleLock(usize),
    Move(usize, usize),
}
