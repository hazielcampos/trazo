//! Modal windows: new project, open project, animation settings.

use trazo_core::{CanvasSettings, Rgba};
use egui::RichText;

use crate::app::{list_projects, TrazoApp, Redraw};

pub struct NewProject {
    pub name: String,
    pub preset: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

const PRESETS: [(&str, u32, u32); 6] = [
    ("HD 16:9 (1280×720)", 1280, 720),
    ("Full HD (1920×1080)", 1920, 1080),
    ("Cuadrado (1080×1080)", 1080, 1080),
    ("Vertical / historias (1080×1920)", 1080, 1920),
    ("Pequeño / GIF (640×480)", 640, 480),
    ("Personalizado", 0, 0),
];

impl Default for NewProject {
    fn default() -> Self {
        Self { name: "Nueva animación".into(), preset: 0, width: 1280, height: 720, fps: 12 }
    }
}

pub fn show(app: &mut TrazoApp, ctx: &egui::Context) {
    new_project(app, ctx);
    open_project(app, ctx);
    settings(app, ctx);
}

fn new_project(app: &mut TrazoApp, ctx: &egui::Context) {
    let Some(dlg) = &mut app.ui.new_project_dialog else { return };
    let mut open = true;
    let mut create = false;
    crate::widgets::backdrop(ctx);
    crate::widgets::dialog(ctx, "Nuevo proyecto", 420.0).open(&mut open).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(RichText::new("Nombre").size(13.0).color(crate::theme::TEXT2));
        ui.add(egui::TextEdit::singleline(&mut dlg.name).desired_width(f32::INFINITY).min_size(egui::vec2(0.0, 40.0)).margin(egui::vec2(12.0, 10.0)));
        ui.add_space(4.0);
        // The list scrolls; "Crear" stays visible at the bottom.
        let list_h = (ui.available_height() - 66.0).max(120.0);
        egui::ScrollArea::vertical().id_salt("new_project_list").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
        ui.label(RichText::new("Tamaño del lienzo").size(13.0).color(crate::theme::TEXT2));
        for (i, (label, w, h)) in PRESETS.iter().enumerate() {
            let (name, detail) = match label.split_once(" (") {
                Some((n, d)) => (n, d.trim_end_matches(')')),
                None => (*label, ""),
            };
            let detail = if *w == 0 { format!("{} × {}", dlg.width, dlg.height) } else { detail.to_string() };
            if crate::widgets::choice_row(ui, dlg.preset == i, name, &detail).clicked() {
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
                ui.label("×");
                ui.add(egui::DragValue::new(&mut dlg.height).range(16..=4096).suffix(" px"));
            });
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Velocidad (fotogramas por segundo)").size(13.0).color(crate::theme::TEXT2));
        let options = [8u32, 12, 15, 24, 30];
        let labels = ["8", "12", "15", "24", "30"];
        let mut idx = options.iter().position(|f| *f == dlg.fps).unwrap_or(1);
        if crate::widgets::segmented(ui, &mut idx, &labels, ui.available_width()) {
            dlg.fps = options[idx];
        }
        });
        ui.add_space(10.0);
        if crate::widgets::primary_button(ui, "Crear").clicked() {
            create = true;
        }
    });
    if create {
        let dlg = app.ui.new_project_dialog.take().expect("dialog open");
        let name = if dlg.name.trim().is_empty() { "Nueva animación".to_string() } else { dlg.name.trim().to_string() };
        let canvas = CanvasSettings { width: dlg.width, height: dlg.height, fps: dlg.fps, background: Rgba::WHITE };
        app.new_project(ctx, name, canvas);
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
    crate::widgets::dialog(ctx, "Mis proyectos", 400.0).open(&mut open).show(ctx, |ui| {
        ui.label(RichText::new(app.platform.projects_dir.display().to_string()).small().weak());
        ui.separator();
        if projects.is_empty() {
            ui.label("Todavía no hay proyectos guardados.");
        }
        egui::ScrollArea::vertical().id_salt("projects_list").max_height(ui.available_height()).show(ui, |ui| {
        for (path, _) in &projects {
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let current = *path == app.doc.path;
            if crate::widgets::choice_row(ui, current, &name, if current { "abierto" } else { "" }).clicked() {
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
    crate::widgets::dialog(ctx, "Ajustes de animación", 420.0).open(&mut open).show(ctx, |ui| {
        egui::ScrollArea::vertical().id_salt("settings_body").show(ui, |ui| {
        let p = &mut app.doc.project;
        ui.horizontal(|ui| {
            ui.label("Nombre");
            if ui.text_edit_singleline(&mut p.name).changed() {
                settings_changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Velocidad");
            if ui.add(egui::DragValue::new(&mut p.canvas.fps).range(1..=60).suffix(" fps")).changed() {
                settings_changed = true;
            }
        });
        ui.label(format!("Lienzo: {} × {} px", p.canvas.width, p.canvas.height));
        ui.horizontal(|ui| {
            ui.label("Color de fondo");
            let c = p.canvas.background.to_rgba8();
            let mut c32 = egui::Color32::from_rgb(c[0], c[1], c[2]);
            if egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::Opaque).changed() {
                p.canvas.background = Rgba::from_rgba8([c32.r(), c32.g(), c32.b(), 255]);
                bg_changed = true;
            }
        });
        ui.separator();
        ui.label(RichText::new("Atajos: B pincel · E borrador · G relleno · I cuentagotas · H mano · espacio+arrastrar mover · rueda zoom · [ ] tamaño · , . fotograma · N nuevo · O cebolla · Enter reproducir · Ctrl+Z / Ctrl+Y").small());
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
