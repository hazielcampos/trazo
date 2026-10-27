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

const CARD: Vec2 = Vec2::new(150.0, 44.0);
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
    /// Brushes picked this session, newest first ("Recientes").
    recent: Vec<usize>,
    /// Category chip; None = recent.
    category: Option<BrushCategory>,
    /// "Ajustar" open: every setting of the brush.
    tuning: bool,
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
        let px = brush_preview(b, w, h, Rgba::new(0.92, 0.92, 0.9, 1.0), size);
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

/// Header of the brushes sheet: title, import and "new" (a copy to edit).
pub fn header(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.label(RichText::new(tr("Pinceles")).font(crate::theme::font(20.0, crate::theme::W::Semi)).color(crate::theme::TEXT));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if crate::widgets::icon_button(ui, Icon::Plus, tr("Nuevo pincel (copia del actual)")).clicked() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("brush_new"), true));
        }
        if crate::widgets::icon_button(ui, Icon::Import, tr("Importar pincel")).clicked() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("brush_import"), true));
        }
    });
}

pub fn show(app: &mut TrazoApp, ui: &mut egui::Ui) {
    use crate::theme::{self, SEPARATOR, TEXT, TEXT2, W};
    let ctx = ui.ctx().clone();
    // Header buttons.
    if ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::unique("brush_import"))).is_some() {
        crate::files::request(app, &ctx, crate::files::PickPurpose::Brushes);
    }
    if ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::unique("brush_new"))).is_some() {
        copy_brush(app, &ctx);
    }
    let mut budget = PREVIEWS_PER_FRAME;
    let mut pending = false;

    // Category chips: recent first.
    egui::ScrollArea::horizontal().id_salt("brush_chips").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space(12.0);
            let mut chip = |ui: &mut egui::Ui, label: &str, on: bool| -> bool {
                let (bg, fg) = if on { (theme::PRIMARY, theme::ON_ACCENT) } else { (crate::theme::ELEVATED, Color32::from_rgb(214, 214, 211)) };
                let g = ui.painter().layout_no_wrap(label.to_string(), theme::font(13.0, if on { W::Semi } else { W::Medium }), fg);
                let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 28.0, 36.0), egui::Sense::click());
                ui.painter().rect_filled(r, 18.0, if resp.hovered() && !on { SEPARATOR } else { bg });
                ui.painter().galley(egui::pos2(r.left() + 14.0, r.center().y - g.size().y / 2.0), g, fg);
                resp.clicked()
            };
            if chip(ui, tr("Recientes"), app.previews.category.is_none()) {
                app.previews.category = None;
            }
            for cat in BrushCategory::ALL {
                if (0..app.tools.library.len()).any(|i| app.tools.library[i].category == cat) && chip(ui, tr(cat.label()), app.previews.category == Some(cat)) {
                    app.previews.category = Some(cat);
                }
            }
        });
    });
    ui.add_space(6.0);

    let active = app.tools.active_index();
    let members: Vec<usize> = match app.previews.category {
        Some(cat) => (0..app.tools.library.len()).filter(|&i| app.tools.library[i].category == cat).collect(),
        None => {
            let mut v: Vec<usize> = app.previews.recent.iter().copied().filter(|&i| i < app.tools.library.len()).collect();
            if !v.contains(&active) {
                v.insert(0, active);
            }
            // Fill up with the first brushes so the list is never short.
            for i in 0..app.tools.library.len() {
                if v.len() >= 6 {
                    break;
                }
                if !v.contains(&i) {
                    v.push(i);
                }
            }
            v
        }
    };
    let bottom_h = if app.previews.tuning { 0.0 } else { 150.0 };
    egui::ScrollArea::vertical().id_salt("brush_list").max_height((ui.available_height() - bottom_h).max(120.0)).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for i in members {
            let tex = app.previews.card(&ctx, &app.tools, i, &mut budget);
            pending |= tex.is_none();
            let b = &app.tools.library[i];
            let sub = format!("{} · {}", tr(b.category.label()), trait_of(b));
            if brush_row(ui, tex, tr(&b.name), &sub, i == active).clicked() {
                app.tools.select_brush(i);
                app.previews.recent.retain(|&x| x != i);
                app.previews.recent.insert(0, i);
                app.previews.recent.truncate(8);
            }
        }
        if app.previews.tuning {
            ui.add_space(8.0);
            tuning(app, ui, &ctx);
        }
    });

    // Size and opacity, always at hand; "Ajustar" opens every setting.
    if !app.previews.tuning {
        ui.add_space(6.0);
        // Its own surface, edge to edge, down to the bottom of the sheet.
        let top = ui.cursor().top();
        let full = egui::Rect::from_min_max(egui::pos2(ui.max_rect().left() - 8.0, top), egui::pos2(ui.max_rect().right() + 8.0, ui.clip_rect().bottom().min(ui.ctx().content_rect().bottom())));
        let lp = ui.ctx().layer_painter(ui.layer_id()).with_clip_rect(ui.clip_rect().expand2(egui::vec2(8.0, 0.0)));
        lp.rect_filled(full, 0.0, crate::theme::SURFACE);
        lp.hline(full.x_range(), top, egui::Stroke::new(1.0, Color32::from_white_alpha(15)));
        ui.add_space(9.5);
        let i = app.tools.active_index();
        let name = tr(&app.tools.active_brush().name).to_string();
        let mut tune = false;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 0)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let tw = &mut app.tools.tweaks[i];
            crate::widgets::hslider(ui, tr("Tamaño"), &mut tw.size, 1.0..=300.0, true, |v| format!("{:.0} px", v));
            crate::widgets::hslider(ui, tr("Opacidad"), &mut tw.opacity, 0.02..=1.0, false, |v| format!("{:.0} %", v * 100.0));
            ui.add_space(6.0);
            tune = crate::widgets::secondary_button_icon(ui, Some(Icon::Sliders), &tf!("Ajustar «{}»", name), 44.0).clicked();
        });
        if tune {
            app.previews.tuning = true;
        }
    }
    if pending {
        ctx.request_repaint();
    }
    let _ = TEXT2;
}

/// Everything about the brush: big preview, stroke settings, the brush engine.
fn tuning(app: &mut TrazoApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        if crate::widgets::icon_button(ui, Icon::ArrowLeft, tr("Volver a la lista")).clicked() {
            app.previews.tuning = false;
        }
        ui.label(RichText::new(tf!("Ajustar «{}»", tr(&app.tools.active_brush().name))).font(crate::theme::font(16.0, crate::theme::W::Semi)));
    });
    let big = app.previews.big(ctx, &app.tools);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, w * 150.0 / 560.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 10.0, PAPER);
    ui.painter().image(big, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    let i = app.tools.active_index();
    let tw = &mut app.tools.tweaks[i];
    crate::widgets::hslider(ui, tr("Tamaño"), &mut tw.size, 1.0..=300.0, true, |v| format!("{:.0} px", v));
    crate::widgets::hslider(ui, tr("Opacidad"), &mut tw.opacity, 0.02..=1.0, false, |v| format!("{:.0} %", v * 100.0));
    ui.add_space(4.0);
    crate::widgets::stabilizer_control(ui, &mut app.tools.stabilizer);
    ui.add_space(4.0);
    crate::widgets::hslider(ui, tr("Presión"), &mut app.tools.pressure_gamma, 0.3..=3.0, true, |v| crate::i18n::num(v, 1));
    crate::widgets::switch_row(ui, tr("Simular presión con la velocidad"), &mut app.tools.simulate_pressure);
    brush_actions(app, ui, ctx);
    advanced(app, ui);
}

/// What the brush is like, in a few words.
fn trait_of(b: &Brush) -> &'static str {
    if b.blend == BlendMode::Multiply || b.accumulate == Accumulate::Buildup {
        tr("se acumula")
    } else if b.grain > 0.3 {
        tr("con grano")
    } else if b.pressure_size {
        tr("cambia con la presión")
    } else {
        tr("trazo uniforme")
    }
}

fn copy_brush(app: &mut TrazoApp, ctx: &egui::Context) {
    let i = app.tools.active_index();
    let mut b = app.tools.library[i].clone();
    b.name = tf!("{} (mío)", tr(&b.name));
    b.size = app.tools.tweaks[i].size;
    b.opacity = app.tools.tweaks[i].opacity;
    match app.tools.add_user_brush(b, &app.platform.brushes_dir) {
        Ok(n) => {
            app.tools.select_brush(n);
            app.previews.tuning = true;
            app.toast(ctx, tr("Pincel creado en «Mis pinceles»"));
        }
        Err(e) => app.toast(ctx, tf!("No se pudo crear: {}", crate::i18n::tr_err(&e.to_string()))),
    }
}

fn brush_row(ui: &mut egui::Ui, tex: Option<egui::TextureId>, name: &str, sub: &str, selected: bool) -> egui::Response {
    use crate::theme::{self, TEXT, TEXT2, W};
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 68.0), egui::Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 16.0, Color32::from_rgb(35, 35, 38));
    } else if resp.hovered() {
        p.rect_filled(rect, 16.0, Color32::from_rgb(30, 30, 33));
    }
    let tile = egui::Rect::from_min_size(egui::pos2(rect.left() + 12.0, rect.center().y - CARD.y / 2.0), CARD);
    p.rect_filled(tile, 10.0, Color32::from_rgb(31, 31, 34));
    if let Some(t) = tex {
        p.image(t, tile, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    let x = tile.right() + 12.0;
    let w = (rect.right() - x - 44.0).max(40.0);
    let ng = crate::panels::home::one_line_pub(ui, name, theme::font(15.0, W::Semi), TEXT, w);
    let sg = crate::panels::home::one_line_pub(ui, sub, theme::font(12.0, W::Regular), TEXT2, w);
    p.galley(egui::pos2(x, rect.center().y - ng.size().y + 1.0), ng, TEXT);
    p.galley(egui::pos2(x, rect.center().y + 3.0), sg, TEXT2);
    if selected {
        let c = egui::pos2(rect.right() - 24.0, rect.center().y);
        p.circle_filled(c, 12.0, theme::PRIMARY);
        crate::icons::paint(p, egui::Rect::from_center_size(c, Vec2::splat(14.0)), Icon::Check, theme::ON_ACCENT);
    }
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
