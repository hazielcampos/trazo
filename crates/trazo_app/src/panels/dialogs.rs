//! Modal windows: new project, open project, animation settings, language.

use trazo_core::{CanvasSettings, Rgba};
use egui::RichText;

use crate::app::{list_projects, TrazoApp, Redraw};
use crate::i18n::{self, tr, Lang};

pub struct NewProject {
    pub name: String,
    /// Start from an example instead of a blank canvas.
    pub template: Option<trazo_core::templates::Template>,
    pub preset: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

const PRESETS: [(&str, u32, u32); 6] = [
    ("HD 16:9", 1280, 720),
    ("Full HD", 1920, 1080),
    ("Cuadrado", 1080, 1080),
    ("Vertical / historias", 1080, 1920),
    ("Pequeño / GIF", 640, 480),
    ("Personalizado", 0, 0),
];

impl Default for NewProject {
    fn default() -> Self {
        Self { name: tr("Nueva animación").into(), template: None, preset: 0, width: 1280, height: 720, fps: 12 }
    }
}

pub fn show(app: &mut TrazoApp, ctx: &egui::Context) {
    new_project(app, ctx);
    open_project(app, ctx);
    settings(app, ctx);
    language(app, ctx);
    tween(app, ctx);
    versions(app, ctx);
    text(app, ctx);
}

/// Text being written; placed as a selection that can still be moved.
pub struct TextDialog {
    /// Where the canvas was tapped (canvas pixels).
    pub at: (f32, f32),
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub center: bool,
}

fn text(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(dlg) = &mut app.ui.text_dialog else { return };
    let mut open = true;
    let mut go = false;
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Texto"), 420.0).min_height(0.0).open(&mut open).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        let r = ui.add(
            egui::TextEdit::multiline(&mut dlg.text)
                .hint_text(tr("Escribe aquí…"))
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .margin(egui::vec2(12.0, 10.0)),
        );
        if r.gained_focus() || ui.memory(|m| m.focused().is_none()) {
            r.request_focus();
        }
        ui.horizontal(|ui| {
            ui.label(tr("Tamaño"));
            ui.add(egui::Slider::new(&mut dlg.size, 8.0..=400.0).suffix(" px").logarithmic(true));
        });
        crate::widgets::switch_row(ui, tr("Negrita"), &mut dlg.bold);
        crate::widgets::switch_row(ui, tr("Centrar las líneas"), &mut dlg.center);
        ui.label(RichText::new(tr("Usa el color actual. Después podrás moverlo, escalarlo o girarlo antes de pulsar Aplicar.")).size(12.0).color(crate::theme::TEXT2));
        ui.add_space(6.0);
        if crate::widgets::primary_button(ui, tr("Insertar")).clicked() {
            go = true;
        }
    });
    if go {
        let dlg = app.ui.text_dialog.take().expect("dialog open");
        app.ui.last_text = (String::new(), Some(dlg.size), dlg.bold, dlg.center);
        app.place_text(ctx, &dlg);
    } else if !open {
        app.ui.text_dialog = None;
    }
}

/// Earlier versions of the open project.
fn versions(app: &mut TrazoApp, ctx: &egui::Context) {
    if !app.ui.versions_dialog {
        return;
    }
    let mut open = true;
    let mut chosen = None;
    let path = app.doc.path.clone();
    let list = app.ui.versions_list.get_or_insert_with(|| crate::versions::list(&path));
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Versiones anteriores"), 400.0).open(&mut open).show(ctx, |ui| {
        ui.label(RichText::new(tr("Trazo guarda una copia de tu proyecto cada 10 minutos mientras trabajas (las 20 más recientes). Restaurar no borra nada: lo que tienes ahora también queda guardado.")).size(13.0).color(crate::theme::TEXT2));
        ui.add_space(4.0);
        if list.is_empty() {
            ui.label(tr("Todavía no hay versiones de este proyecto."));
        }
        egui::ScrollArea::vertical().id_salt("versions_list").max_height(ui.available_height()).show(ui, |ui| {
            for v in list.iter() {
                let detail = tf!("{} MB · Restaurar", crate::i18n::num(v.bytes as f32 / 1e6, 1));
                if crate::widgets::choice_row(ui, false, &crate::versions::age(v.when), &detail).on_hover_text(tr("Restaurar esta versión")).clicked() {
                    chosen = Some(v.path.clone());
                }
            }
        });
    });
    if let Some(p) = chosen {
        app.ui.versions_dialog = false;
        app.ui.versions_list = None;
        app.restore_version(ctx, &p);
    } else if !open {
        app.ui.versions_dialog = false;
        app.ui.versions_list = None;
    }
}

/// How the in-betweens are made.
pub struct TweenDialog {
    pub steps: usize,
    pub ease: trazo_core::transform::Ease,
    /// Also add a drawing with the final pose (off when it is already drawn).
    pub with_end: bool,
}

impl Default for TweenDialog {
    fn default() -> Self {
        Self { steps: 3, ease: trazo_core::transform::Ease::InOut, with_end: true }
    }
}

pub fn ease_label(e: trazo_core::transform::Ease) -> &'static str {
    use trazo_core::transform::Ease;
    match e {
        Ease::Linear => tr("Lineal"),
        Ease::In => tr("Acelera"),
        Ease::Out => tr("Frena"),
        Ease::InOut => tr("Suave"),
    }
}

/// "Generate in-betweens": the drawing is the first pose, where the selection
/// was moved to is the last one.
fn tween(app: &mut TrazoApp, ctx: &egui::Context) {
    use trazo_core::transform::Ease;
    if app.ui.tween_dialog.is_none() {
        return;
    }
    // The selection was applied or dropped meanwhile: nothing to tween.
    let Some((layer, block)) = app.canvas.select.tween_source(&app.doc).filter(|_| app.canvas.select.can_tween(&app.doc)) else {
        app.ui.tween_dialog = None;
        return;
    };
    let later = trazo_core::timeline::has_drawings_after(&app.doc.project, layer, block.start);
    let Some(dlg) = &mut app.ui.tween_dialog else { return };
    let mut open = true;
    let mut go = false;
    let fps = app.doc.project.canvas.fps.max(1) as f32;
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Generar intermedios"), 400.0).min_height(0.0).open(&mut open).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(RichText::new(tr("Este dibujo es el inicio; donde dejaste la selección es el final. Los dibujos nuevos van después de este, en la misma capa.")).size(13.0).color(crate::theme::TEXT2));
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Dibujos intermedios")).size(13.0).color(crate::theme::TEXT2));
        ui.horizontal(|ui| {
            if ui.add_enabled(dlg.steps > 1, egui::Button::new("−").min_size(egui::vec2(44.0, 40.0))).clicked() {
                dlg.steps -= 1;
            }
            ui.add(egui::DragValue::new(&mut dlg.steps).range(1..=48).speed(0.1));
            if ui.add_enabled(dlg.steps < 48, egui::Button::new("+").min_size(egui::vec2(44.0, 40.0))).clicked() {
                dlg.steps += 1;
            }
        });
        ui.label(RichText::new(tr("Espaciado")).size(13.0).color(crate::theme::TEXT2));
        let labels: Vec<&str> = Ease::ALL.iter().map(|e| ease_label(*e)).collect();
        let mut idx = Ease::ALL.iter().position(|e| *e == dlg.ease).unwrap_or(0);
        if crate::widgets::segmented(ui, &mut idx, &labels, ui.available_width()) {
            dlg.ease = Ease::ALL[idx];
        }
        ui.label(RichText::new(match dlg.ease {
            Ease::Linear => tr("Velocidad constante"),
            Ease::In => tr("Arranca despacio y acelera"),
            Ease::Out => tr("Va rápido y frena al llegar"),
            Ease::InOut => tr("Arranca y llega despacio"),
        }).size(12.0).color(crate::theme::TEXT2));
        // Timing chart: a tick per pose, spaced as they will move.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::hover());
        let p = ui.painter();
        let (x0, x1, y) = (rect.left() + 10.0, rect.right() - 10.0, rect.center().y);
        p.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], egui::Stroke::new(1.0, crate::theme::SEPARATOR));
        p.circle_filled(egui::pos2(x0, y), 6.0, crate::theme::TEXT);
        for (i, t) in dlg.ease.spacing(dlg.steps).iter().enumerate() {
            let last = i == dlg.steps;
            let pos = egui::pos2(x0 + (x1 - x0) * t, y);
            if last && !dlg.with_end {
                p.circle_stroke(pos, 6.0, egui::Stroke::new(1.5, crate::theme::TEXT2));
            } else {
                p.circle_filled(pos, if last { 6.0 } else { 4.0 }, if last { crate::theme::TEXT } else { crate::theme::ACCENT });
            }
        }
        crate::widgets::switch_row(ui, tr("Añadir también el dibujo final"), &mut dlg.with_end);
        let count = dlg.steps + dlg.with_end as usize;
        let total = count * block.len;
        ui.label(RichText::new(tf!("{} dibujos nuevos · +{} fotogramas · {} s", count, total, crate::i18n::num(total as f32 / fps, 2))).size(12.0).color(crate::theme::TEXT2));
        if later {
            ui.label(RichText::new(tf!("Los dibujos siguientes de esta capa se correrán {} fotogramas; las demás capas no cambian.", total)).size(12.0).color(ui.visuals().warn_fg_color));
        }
        ui.add_space(6.0);
        if crate::widgets::primary_button(ui, tr("Generar")).clicked() {
            go = true;
        }
    });
    if go {
        let dlg = app.ui.tween_dialog.take().expect("dialog open");
        app.stop_playback();
        match app.canvas.select.tween(&mut app.doc, dlg.steps, dlg.ease, dlg.with_end) {
            Ok((first, n)) => {
                // Land on the first new drawing so it can be checked right away.
                app.doc.frame = first.min(app.doc.project.frames.len().saturating_sub(1));
                app.toast(ctx, tf!("{} dibujos creados: pulsa reproducir para verlos", n));
            }
            Err(e) => app.toast(ctx, e),
        }
    } else if !open {
        app.ui.tween_dialog = None;
    }
}

fn new_project(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(dlg) = &mut app.ui.new_project_dialog else { return };
    let mut open = true;
    let mut create = false;
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Nuevo proyecto"), 420.0).open(&mut open).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(RichText::new(tr("Nombre")).size(13.0).color(crate::theme::TEXT2));
        ui.add(egui::TextEdit::singleline(&mut dlg.name).desired_width(f32::INFINITY).min_size(egui::vec2(0.0, 40.0)).margin(egui::vec2(12.0, 10.0)));
        ui.add_space(4.0);
        // The list scrolls; "Crear" stays visible at the bottom.
        let list_h = (ui.available_height() - 66.0).max(120.0);
        egui::ScrollArea::vertical().id_salt("new_project_list").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
        ui.label(RichText::new(tr("Empezar con")).size(13.0).color(crate::theme::TEXT2));
        if crate::widgets::choice_row(ui, dlg.template.is_none(), tr("Lienzo en blanco"), "").clicked() {
            if dlg.template.is_some() {
                dlg.name = tr("Nueva animación").into();
            }
            dlg.template = None;
        }
        for t in trazo_core::templates::Template::ALL {
            if crate::widgets::choice_row(ui, dlg.template == Some(t), tr(t.name()), tr(t.hint())).clicked() {
                dlg.template = Some(t);
                dlg.name = tr(t.name()).into();
            }
        }
        if dlg.template.is_some() {
            ui.label(RichText::new(tr("El ejemplo va en la capa «Guía»: dibuja encima en la capa «Dibujo». Oculta o borra la guía antes de exportar.")).size(12.0).color(crate::theme::TEXT2));
        } else {
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Tamaño del lienzo")).size(13.0).color(crate::theme::TEXT2));
        for (i, (name, w, h)) in PRESETS.iter().enumerate() {
            let detail = if *w == 0 { format!("{} × {}", dlg.width, dlg.height) } else { format!("{w}×{h}") };
            if crate::widgets::choice_row(ui, dlg.preset == i, tr(name), &detail).clicked() {
                dlg.preset = i;
                if *w > 0 {
                    dlg.width = *w;
                    dlg.height = *h;
                }
            }
        }
        if PRESETS[dlg.preset].1 == 0 {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut dlg.width).range(16..=4096).suffix(" px"));
                ui.label(tr("×"));
                ui.add(egui::DragValue::new(&mut dlg.height).range(16..=4096).suffix(" px"));
            });
        }
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Velocidad (fotogramas por segundo)")).size(13.0).color(crate::theme::TEXT2));
        let options = [8u32, 12, 15, 24, 30];
        let labels = ["8", "12", "15", "24", "30"];
        let mut idx = options.iter().position(|f| *f == dlg.fps).unwrap_or(1);
        if crate::widgets::segmented(ui, &mut idx, &labels, ui.available_width()) {
            dlg.fps = options[idx];
        }
        }
        });
        ui.add_space(10.0);
        if crate::widgets::primary_button(ui, tr("Crear")).clicked() {
            create = true;
        }
    });
    if create {
        let dlg = app.ui.new_project_dialog.take().expect("dialog open");
        let name = if dlg.name.trim().is_empty() { tr("Nueva animación").to_string() } else { dlg.name.trim().to_string() };
        app.ui.home = false;
        match dlg.template {
            Some(t) => app.open_new_project(ctx, t.build(&name)),
            None => {
                let canvas = CanvasSettings { width: dlg.width, height: dlg.height, fps: dlg.fps, background: Rgba::WHITE, ..Default::default() };
                app.new_project(ctx, name, canvas);
            }
        }
        app.save_async(true);
    } else if !open {
        app.ui.new_project_dialog = None;
    }
}

fn open_project(app: &mut TrazoApp, ctx: &egui::Context) {
    if !app.ui.open_dialog {
        return;
    }
    let mut open = true;
    let mut chosen = None;
    let projects = list_projects(&app.platform.projects_dir);
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Mis proyectos"), 400.0).open(&mut open).show(ctx, |ui| {
        ui.label(RichText::new(app.platform.projects_dir.display().to_string()).small().weak());
        ui.separator();
        if projects.is_empty() {
            ui.label(tr("Todavía no hay proyectos guardados."));
        }
        egui::ScrollArea::vertical().id_salt("projects_list").max_height(ui.available_height()).show(ui, |ui| {
        for (path, _) in &projects {
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let current = *path == app.doc.path;
            if crate::widgets::choice_row(ui, current, &name, if current { tr("abierto") } else { "" }).clicked() {
                chosen = Some(path.clone());
            }
        }
        });
    });
    if let Some(p) = chosen {
        app.ui.open_dialog = false;
        if p != app.doc.path {
            app.open_project(ctx, &p);
        }
    } else if !open {
        app.ui.open_dialog = false;
    }
}

fn settings(app: &mut TrazoApp, ctx: &egui::Context) {
    if !app.ui.settings_dialog {
        return;
    }
    let mut open = true;
    let mut bg_changed = false;
    let mut settings_changed = false;
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, tr("Ajustes de animación"), 420.0).open(&mut open).show(ctx, |ui| {
        egui::ScrollArea::vertical().id_salt("settings_body").show(ui, |ui| {
        let p = &mut app.doc.project;
        ui.horizontal(|ui| {
            ui.label(tr("Nombre"));
            if ui.text_edit_singleline(&mut p.name).changed() {
                settings_changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr("Velocidad"));
            if ui.add(egui::DragValue::new(&mut p.canvas.fps).range(1..=60).suffix(" fps")).changed() {
                settings_changed = true;
            }
        });
        ui.label(tf!("Lienzo: {} × {} px", p.canvas.width, p.canvas.height));
        // Memory the drawings take (measured at most once a second).
        let now = ui.input(|i| i.time);
        let mem_id = egui::Id::unique("drawing_memory");
        let cached: Option<(f64, f32, usize)> = ui.data(|d| d.get_temp(mem_id));
        let (mb, frames) = match cached {
            Some((t, mb, n)) if now - t < 1.0 => (mb, n),
            _ => {
                let u = trazo_core::memory::usage(p, None);
                ui.data_mut(|d| d.insert_temp(mem_id, (now, u.megabytes(), p.frames.len())));
                (u.megabytes(), p.frames.len())
            }
        };
        ui.label(RichText::new(tf!("{} fotogramas · los dibujos ocupan {} MB de memoria", frames, crate::i18n::num(mb, 1))).small().weak());
        ui.horizontal(|ui| {
            ui.label(tr("Color de fondo"));
            let c = p.canvas.background.to_rgba8();
            let mut c32 = egui::Color32::from_rgb(c[0], c[1], c[2]);
            if egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::Opaque).changed() {
                p.canvas.background = Rgba::from_rgba8([c32.r(), c32.g(), c32.b(), 255]);
                bg_changed = true;
            }
        });
        ui.separator();
        ui.label(RichText::new(tr("Atajos: B pincel · E borrador · G relleno · I cuentagotas · H mano · espacio+arrastrar mover · rueda zoom · [ ] tamaño · , . fotograma · N nuevo · O cebolla · Enter reproducir · Ctrl+Z / Ctrl+Y")).small());
        });
    });
    if settings_changed || bg_changed {
        app.doc.mark_settings_changed();
    }
    if bg_changed {
        app.doc.invalidate_all();
        app.doc.redraw = Redraw::Full;
    }
    if !open {
        app.ui.settings_dialog = false;
    }
}

/// Shown in both languages on purpose, so it is not translated.
const BOTH_LANGUAGES_PROMPT: &str = "Elige tu idioma · Choose your language";

/// Language picker. The first time the app opens it can't be dismissed
/// without choosing; later it is opened from the project menu.
fn language(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(first_run) = app.ui.language_dialog else { return };
    let mut open = true;
    let mut done = false;
    crate::widgets::backdrop(ctx);
    let title = if first_run { "Trazo" } else { tr("Idioma") };
    // A fixed id: the title changes with the language being picked.
    let mut win = crate::widgets::dialog(ctx, title, 380.0).id(egui::Id::unique("language_dialog")).min_height(0.0);
    if !first_run {
        win = win.open(&mut open);
    }
    win.show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        if first_run {
            // Both languages, so it reads right before choosing.
            ui.label(RichText::new(BOTH_LANGUAGES_PROMPT).size(15.0).color(crate::theme::TEXT2));
            ui.add_space(4.0);
        }
        for l in Lang::ALL {
            if crate::widgets::choice_row(ui, i18n::lang() == l, l.native_name(), "").clicked() {
                i18n::set_lang(l);
                if !first_run {
                    i18n::save(&app.platform.data_dir, l);
                }
            }
        }
        if first_run {
            ui.add_space(10.0);
            if crate::widgets::primary_button(ui, tr("Continuar")).clicked() {
                done = true;
            }
        }
    });
    if done {
        i18n::save(&app.platform.data_dir, i18n::lang());
        app.after_first_language();
        app.ui.language_dialog = None;
    } else if !open {
        app.ui.language_dialog = None;
    }
}
