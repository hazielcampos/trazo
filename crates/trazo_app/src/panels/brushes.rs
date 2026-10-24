//! Brush library with live previews, brush settings and user brushes
//! (create, import, export, delete).

use crate::i18n::tr;
use std::collections::HashMap;

use trazo_core::brush::{Accumulate, Brush, BrushCategory, DabShape};
use trazo_core::brush_io::{self, BRUSH_EXTENSION};
use trazo_core::preview::brush_preview;
use trazo_core::{BlendMode, Rgba};
use egui::{Color32, ColorImage, RichText, TextureHandle, TextureOptions, Vec2};

use crate::app::TrazoApp;
use crate::icons::{Icon, IconButton};

const CARD: Vec2 = Vec2::new(124.0, 46.0);
/// Previews rendered per UI frame (keeps scrolling smooth).
const PREVIEWS_PER_FRAME: usize = 6;
const PAPER: Color32 = Color32::from_rgb(244, 241, 234);

/// Cached preview textures. Each one remembers the brush it was drawn from,
/// so only previews of brushes that actually changed are redrawn.
#[derive(Default)]
pub struct BrushPreviews {
    cards: HashMap<usize, (Brush, TextureHandle)>,
    big: Option<(BigKey, TextureHandle)>,
    big_time: f64,
    confirm_delete: Option<usize>,
}

#[derive(Clone, PartialEq)]
struct BigKey {
    brush: Brush,
    size: u32,
    opacity: u32,
    color: Color32,
}

impl BrushPreviews {
    fn card(&mut self, ctx: &egui::Context, app_tools: &crate::tools::ToolState, i: usize, budget: &mut usize) -> Option<egui::TextureId> {
        let current = &app_tools.library[i];
        if let Some((b, t)) = self.cards.get(&i) {
            if b == current || *budget == 0 {
                return Some(t.id());
            }
        }
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let b = &app_tools.library[i];
        let scale = 2.0; // render at 2× for sharp previews on high-DPI screens
        let (w, h) = ((CARD.x * scale) as u32, (CARD.y * scale) as u32);
        let size = (b.size * 1.4).clamp(4.0, h as f32 * 0.4);
        let px = brush_preview(b, w, h, Rgba::new(0.12, 0.12, 0.14, 1.0), size);
        let img = ColorImage::from_rgba_premultiplied([w as usize, h as usize], &px);
        let tex = ctx.load_texture(format!("brush_card_{i}"), img, TextureOptions::LINEAR);
        let id = tex.id();
        self.cards.insert(i, (b.clone(), tex));
        Some(id)
    }

    fn big(&mut self, ctx: &egui::Context, tools: &crate::tools::ToolState) -> egui::TextureId {
        let i = tools.active_index();
        let now = ctx.input(|inp| inp.time);
        let key = BigKey {
            brush: tools.library[i].clone(),
            size: tools.tweaks[i].size as u32,
            opacity: (tools.tweaks[i].opacity * 100.0) as u32,
            color: tools.color,
        };
        if let Some((k, t)) = &self.big {
            // While a slider is being dragged, redraw at most ~8 times a second.
            if *k == key || now - self.big_time < 0.12 {
                if *k != key {
                    ctx.request_repaint_after(std::time::Duration::from_millis(130));
                }
                return t.id();
            }
        }
        self.big_time = now;
        let (w, h) = (560, 150);
        let b = &tools.library[i];
        let mut b = b.clone();
        b.opacity = tools.tweaks[i].opacity;
        let px = brush_preview(&b, w, h, tools.color_rgba(), tools.tweaks[i].size);
        let img = ColorImage::from_rgba_premultiplied([w as usize, h as usize], &px);
        let tex = ctx.load_texture("brush_big", img, TextureOptions::LINEAR);
        let id = tex.id();
        self.big = Some((key, tex));
        id
    }

    pub fn clear(&mut self) {
        self.cards.clear();
        self.big = None;
    }
}

pub fn show(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut budget = PREVIEWS_PER_FRAME;
    let mut pending = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        // Large preview of the brush in use, with the real size and color.
        let big = app.previews.big(&ctx, &app.tools);
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, w * 150.0 / 560.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 8.0, PAPER);
        ui.painter().image(big, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        ui.label(RichText::new(tr(&app.tools.active_brush().name)).strong());

        brush_actions(app, ui, &ctx);
        ui.separator();

        let active = app.tools.active_index();
        for cat in BrushCategory::ALL {
            let members: Vec<usize> = (0..app.tools.library.len()).filter(|&i| app.tools.library[i].category == cat).collect();
            if members.is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(RichText::new(tr(cat.label())).strong()).default_open(true).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                    for i in members {
                        let tex = app.previews.card(&ctx, &app.tools, i, &mut budget);
                        pending |= tex.is_none();
                        if brush_card(ui, tex, tr(&app.tools.library[i].name), i == active).clicked() {
                            app.tools.select_brush(i);
                        }
                    }
                });
            });
        }

        ui.separator();
        ui.label(RichText::new(tr("Ajustes del trazo")).strong());
        ui.add(egui::Slider::new(&mut app.tools.stabilizer, 0.0..=0.95).text(tr("Estabilizador"))).on_hover_text(tr("Suaviza el temblor de la mano"));
        ui.add(egui::Slider::new(&mut app.tools.pressure_gamma, 0.3..=3.0).logarithmic(true).text(tr("Curva de presión")))
            .on_hover_text(tr("Menos de 1: el lápiz se siente más suave. Más de 1: hay que apretar más."));
        ui.checkbox(&mut app.tools.simulate_pressure, tr("Simular presión con la velocidad (ratón / dedo)"));
        advanced(app, ui);
    });
    if pending {
        ctx.request_repaint();
    }
}

fn brush_card(ui: &mut egui::Ui, tex: Option<egui::TextureId>, name: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(CARD + Vec2::new(0.0, 18.0), egui::Sense::click());
    let img_rect = egui::Rect::from_min_size(rect.min, CARD);
    let p = ui.painter();
    p.rect_filled(img_rect, 6.0, PAPER);
    if let Some(t) = tex {
        p.image(t, img_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    let stroke = if selected {
        egui::Stroke::new(3.0, crate::theme::ACCENT)
    } else if resp.hovered() {
        egui::Stroke::new(1.5, Color32::from_gray(200))
    } else {
        egui::Stroke::new(1.0, Color32::from_gray(70))
    };
    p.rect_stroke(img_rect, 6.0, stroke, egui::StrokeKind::Outside);
    let font = egui::FontId::proportional(12.0);
    let color = if selected { crate::theme::ACCENT } else { ui.visuals().text_color() };
    let label = if name.chars().count() > 18 { format!("{}…", name.chars().take(17).collect::<String>()) } else { name.to_string() };
    p.text(egui::pos2(img_rect.center().x, img_rect.max.y + 9.0), egui::Align2::CENTER_CENTER, label, font, color);
    resp.on_hover_text(name)
}

fn brush_actions(app: &mut TrazoApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let i = app.tools.active_index();
    let is_user = app.tools.is_user_brush(i);
    ui.horizontal_wrapped(|ui| {
        if ui.add(IconButton::new(Icon::Duplicate).text(tr("Crear copia"))).on_hover_text(tr("Copia este pincel en «Mis pinceles» para modificarlo")).clicked() {
            let mut b = app.tools.library[i].clone();
            b.name = tf!("{} (mío)", tr(&b.name));
            b.size = app.tools.tweaks[i].size;
            b.opacity = app.tools.tweaks[i].opacity;
            match app.tools.add_user_brush(b, &app.platform.brushes_dir) {
                Ok(n) => {
                    app.tools.select_brush(n);
                    app.toast(ctx, tr("Pincel creado en «Mis pinceles»"));
                }
                Err(e) => app.toast(ctx, tf!("No se pudo crear: {}", crate::i18n::tr_err(&e.to_string()))),
            }
        }
        if ui.add(IconButton::new(Icon::Import).text(tr("Importar"))).on_hover_text(tr("Pinceles .animbrush o imágenes PNG como punta")).clicked() {
            crate::files::request(app, ctx, crate::files::PickPurpose::Brushes);
        }
        if ui.add(IconButton::new(Icon::Export).text(tr("Exportar"))).on_hover_text(tr("Guardar este pincel en un archivo .animbrush para compartirlo")).clicked() {
            let b = app.tools.library[i].clone();
            let name = format!("{}.{BRUSH_EXTENSION}", b.name);
            if let Some(path) = app.platform.save_file(tr("Exportar pincel"), &name, BRUSH_EXTENSION) {
                match brush_io::save_brush(&b, &path) {
                    Ok(()) => app.toast(ctx, tf!("Exportado: {}", path.display())),
                    Err(e) => app.toast(ctx, tf!("No se pudo exportar: {}", crate::i18n::tr_err(&e.to_string()))),
                }
            }
        }
        if is_user {
            let confirming = app.previews.confirm_delete == Some(i);
            let label = if confirming { tr("¿Seguro? Pulsa otra vez") } else { tr("Borrar") };
            if ui.add(IconButton::new(Icon::Trash).keep_text().text(label).selected(confirming)).clicked() {
                if confirming {
                    match app.tools.delete_user_brush(i) {
                        Ok(()) => app.toast(ctx, tr("Pincel borrado")),
                        Err(e) => app.toast(ctx, tf!("No se pudo borrar: {}", crate::i18n::tr_err(&e.to_string()))),
                    }
                    app.previews.clear();
                    app.previews.confirm_delete = None;
                } else {
                    app.previews.confirm_delete = Some(i);
                }
            }
        } else if ui.add(IconButton::new(Icon::RotateLeft).text(tr("Restablecer"))).on_hover_text(tr("Volver a los valores de fábrica")).clicked() {
            let name = app.tools.library[i].name.clone();
            if let Some(orig) = trazo_core::brush::builtin_brushes().into_iter().find(|x| x.name == name) {
                app.tools.tweaks[i] = crate::tools::BrushTweak { size: orig.size, opacity: orig.opacity };
                app.tools.library[i] = orig;
                app.tools.brush_changed(i);
            }
        }
    });
    if app.previews.confirm_delete.is_some_and(|d| d != i) {
        app.previews.confirm_delete = None;
    }
}

pub fn import_brush_files(app: &mut TrazoApp, ctx: &egui::Context, files: Vec<std::path::PathBuf>) {
    if files.is_empty() {
        return;
    }
    let (mut ok, mut last, mut errors) = (0, None, Vec::new());
    for path in files {
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("Pincel").into());
        let is_png = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"));
        let brush = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| if is_png { brush_io::brush_from_png(&name, &bytes) } else { brush_io::load_brush_bytes(&bytes) }.map_err(|e| e.to_string()));
        match brush.and_then(|b| app.tools.add_user_brush(b, &app.platform.brushes_dir)) {
            Ok(i) => {
                ok += 1;
                last = Some(i);
                app.platform.imported(&path);
            }
            Err(e) => errors.push(format!("{name}: {}", crate::i18n::tr_err(&e.to_string()))),
        }
    }
    if let Some(i) = last {
        app.tools.select_brush(i);
    }
    let msg = match (ok, errors.first()) {
        (n, None) => tf!("{} pincel(es) importado(s)", n),
        (n, Some(e)) => tf!("{} importado(s); error en {}", n, e),
    };
    app.toast(ctx, msg);
}

/// Every engine parameter of the active brush. Built-in brushes can be
/// tweaked too (reset brings them back); user brushes are saved to disk.
fn advanced(app: &mut TrazoApp, ui: &mut egui::Ui) {
    let i = app.tools.active_index();
    let is_user = app.tools.is_user_brush(i);
    let mut changed = false;
    let mut rename: Option<String> = None;
    if is_user {
        let id = egui::Id::unique(("brush_name", i));
        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| tr(&app.tools.library[i].name).to_string());
        ui.horizontal(|ui| {
            ui.label(tr("Nombre del pincel"));
            let r = ui.text_edit_singleline(&mut text);
            if r.lost_focus() {
                rename = Some(text.clone());
                ui.data_mut(|d| d.remove::<String>(id));
            } else if r.changed() {
                ui.data_mut(|d| d.insert_temp(id, text.clone()));
            }
        });
    }
    if let Some(n) = rename {
        app.tools.rename_brush(i, &n);
    }
    let b = &mut app.tools.library[i];
    egui::CollapsingHeader::new(RichText::new(tf!("Editar pincel: {}", tr(&b.name))).strong()).id_salt("brush_advanced").show(ui, |ui| {
        ui.label(RichText::new(tr("Forma")).strong());
        let has_tip = b.tip.is_some();
        egui::ComboBox::from_label(tr("Punta"))
            .selected_text(shape_label(b.shape))
            .show_ui(ui, |ui| {
                for s in [DabShape::Round, DabShape::Pixel, DabShape::Spray, DabShape::Image] {
                    if s == DabShape::Image && !has_tip {
                        continue;
                    }
                    changed |= ui.selectable_value(&mut b.shape, s, shape_label(s)).changed();
                }
            });
        changed |= slider(ui, &mut b.hardness, 0.0..=1.0, tr("Dureza"));
        changed |= slider(ui, &mut b.roundness, 0.05..=1.0, tr("Redondez"));
        changed |= slider(ui, &mut b.angle_deg, -90.0..=90.0, tr("Ángulo"));
        changed |= ui.checkbox(&mut b.follow_direction, tr("La punta sigue la dirección del trazo")).changed();
        changed |= slider(ui, &mut b.rotation_jitter, 0.0..=180.0, tr("Rotación aleatoria (°)"));
        if b.shape == DabShape::Spray {
            let mut d = b.density as f32;
            changed |= slider(ui, &mut d, 1.0..=200.0, tr("Densidad"));
            b.density = d as u32;
        }

        ui.label(RichText::new(tr("Trazo")).strong());
        changed |= slider(ui, &mut b.flow, 0.01..=1.0, tr("Flujo"));
        changed |= slider(ui, &mut b.spacing, 0.02..=2.0, tr("Espaciado"));
        egui::ComboBox::from_label(tr("Mezcla dentro del trazo"))
            .selected_text(if b.accumulate == Accumulate::Max { tr("Uniforme (rotulador)") } else { tr("Acumula (aerógrafo)") })
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut b.accumulate, Accumulate::Max, tr("Uniforme (rotulador)")).changed();
                changed |= ui.selectable_value(&mut b.accumulate, Accumulate::Buildup, tr("Acumula (aerógrafo)")).changed();
            });
        egui::ComboBox::from_label(tr("Fusión"))
            .selected_text(tr(b.blend.label()))
            .show_ui(ui, |ui| {
                for m in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen, BlendMode::Add, BlendMode::Overlay] {
                    changed |= ui.selectable_value(&mut b.blend, m, tr(m.label())).changed();
                }
            });
        changed |= slider(ui, &mut b.taper_start, 0.0..=80.0, tr("Afinar inicio (px)"));
        changed |= slider(ui, &mut b.taper_end, 0.0..=80.0, tr("Afinar final (px)"));

        ui.label(RichText::new(tr("Presión")).strong());
        changed |= ui.checkbox(&mut b.pressure_size, tr("Presión → tamaño")).changed();
        changed |= ui.checkbox(&mut b.pressure_opacity, tr("Presión → opacidad")).changed();
        changed |= slider(ui, &mut b.min_size, 0.0..=1.0, tr("Tamaño mínimo"));

        ui.label(RichText::new(tr("Textura y variación")).strong());
        changed |= slider(ui, &mut b.grain, 0.0..=1.0, tr("Textura de papel"));
        changed |= slider(ui, &mut b.grain_scale, 0.5..=16.0, tr("Tamaño del grano"));
        changed |= slider(ui, &mut b.jitter, 0.0..=0.6, tr("Dispersión"));
        changed |= slider(ui, &mut b.size_jitter, 0.0..=1.0, tr("Variación de tamaño"));
        if !is_user {
            ui.label(RichText::new(tr("Es un pincel de serie: los cambios no se guardan. Usa «Crear copia» para conservarlos.")).small().weak());
        }
    });
    if changed {
        app.tools.brush_changed(i);
    }
}

fn shape_label(s: DabShape) -> &'static str {
    match s {
        DabShape::Round => tr("Redonda"),
        DabShape::Pixel => tr("Píxel"),
        DabShape::Spray => tr("Spray"),
        DabShape::Image => tr("Imagen importada"),
    }
}

fn slider(ui: &mut egui::Ui, v: &mut f32, r: std::ops::RangeInclusive<f32>, label: &str) -> bool {
    ui.add(egui::Slider::new(v, r).text(label)).changed()
}
