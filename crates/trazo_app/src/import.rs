//! Importing pictures: as a movable image on a new layer, as a fixed
//! reference picture, or as an image sequence for rotoscoping.

use crate::i18n::tr;
use trazo_core::composite::premultiply;
use trazo_core::document::SHARED_FRAME;
use trazo_core::io::{decode_image, fit_image};
use trazo_core::{Edit, Frame, IRect, Layer, LayerProps, TiledImage};

use crate::app::TrazoApp;
use crate::tools::Tool;

pub const IMAGE_EXTS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];

/// Decode a file and scale it to fit the canvas. Returns premultiplied pixels
/// and the centered rectangle where they go.
fn load_fitted(path: &std::path::Path, cw: u32, ch: u32) -> Result<(IRect, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let (w, h, rgba) = decode_image(&bytes).map_err(|e| e.to_string())?;
    let (w, h, mut rgba) = fit_image(w, h, rgba, cw, ch);
    if w == 0 {
        return Err(tr("imagen no válida").into());
    }
    premultiply(&mut rgba);
    let x = (cw as i32 - w as i32) / 2;
    let y = (ch as i32 - h as i32) / 2;
    Ok((IRect::new(x, y, x + w as i32, y + h as i32), rgba))
}

fn file_name(path: &std::path::Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("Imagen").into())
}

fn new_layer(app: &mut TrazoApp, name: String, props: impl FnOnce(&mut LayerProps), cels: Vec<(u64, TiledImage)>) -> u64 {
    let d = &mut app.doc;
    let id = d.project.alloc_id();
    let mut p = LayerProps::named(name);
    props(&mut p);
    let index = d.layer_index() + 1;
    d.history.execute(&mut d.project, Edit::InsertLayer { index, layer: Layer { id, props: p }, cels });
    d.layer = id;
    d.invalidate_all();
    id
}

/// Picture on a new layer, left floating so it can be moved and scaled.
pub fn image_as_layer(app: &mut TrazoApp, ctx: &egui::Context, files: Vec<std::path::PathBuf>) {
    let Some(path) = files.first() else { return };
    let (cw, ch) = (app.doc.project.canvas.width, app.doc.project.canvas.height);
    match load_fitted(path, cw, ch) {
        Ok((rect, px)) => {
            app.stop_playback();
            app.canvas.select.commit(&mut app.doc);
            new_layer(app, file_name(path), |_| {}, vec![]);
            app.tools.tool = Tool::Select;
            app.canvas.select.place(&mut app.doc, rect, px);
            app.toast(ctx, tr("Imagen importada: muévela o escálala y pulsa Aplicar"));
        }
        Err(e) => app.toast(ctx, tf!("No se pudo importar: {}", crate::i18n::tr_err(&e.to_string()))),
    }
}

/// Fixed reference picture shown in every frame (semi-transparent, locked).
pub fn reference_image(app: &mut TrazoApp, ctx: &egui::Context, files: Vec<std::path::PathBuf>) {
    let Some(path) = files.first() else { return };
    let (cw, ch) = (app.doc.project.canvas.width, app.doc.project.canvas.height);
    match load_fitted(path, cw, ch) {
        Ok((rect, px)) => {
            app.stop_playback();
            app.canvas.select.commit(&mut app.doc);
            let mut img = TiledImage::new(cw, ch);
            img.write_region(rect.x0, rect.y0, rect.width() as usize, rect.height() as usize, &px);
            new_layer(
                app,
                format!("Referencia: {}", file_name(path)),
                |p| {
                    p.shared = true;
                    p.locked = true;
                    p.opacity = 0.5;
                    p.onion_skin = false;
                },
                vec![(SHARED_FRAME, img)],
            );
            app.toast(ctx, tr("Referencia añadida (capa bloqueada al 50 %)"));
        }
        Err(e) => app.toast(ctx, tf!("No se pudo importar: {}", crate::i18n::tr_err(&e.to_string()))),
    }
}

/// One picture per frame on a new layer, starting at the current frame.
/// Frames are added when the sequence is longer than the animation.
pub fn image_sequence(app: &mut TrazoApp, ctx: &egui::Context, mut files: Vec<std::path::PathBuf>) {
    if files.is_empty() {
        return;
    }
    files.sort();
    app.stop_playback();
    app.canvas.select.commit(&mut app.doc);
    let (cw, ch) = (app.doc.project.canvas.width, app.doc.project.canvas.height);
    let mut images = Vec::new();
    for f in &files {
        match load_fitted(f, cw, ch) {
            Ok((rect, px)) => {
                let mut img = TiledImage::new(cw, ch);
                img.write_region(rect.x0, rect.y0, rect.width() as usize, rect.height() as usize, &px);
                images.push(img);
            }
            Err(e) => {
                app.toast(ctx, format!("{}: {}", file_name(f), crate::i18n::tr_err(&e.to_string())));
                return;
            }
        }
    }
    let d = &mut app.doc;
    let mark = d.history.undo_len();
    let layer_id = d.project.alloc_id();
    let start = d.frame;
    let mut existing = Vec::new();
    for (i, img) in images.into_iter().enumerate() {
        let index = start + i;
        if index < d.project.frames.len() {
            existing.push((d.project.frames[index].id, img));
        } else {
            let mut f = Frame::new(d.project.alloc_id());
            f.cels.insert(layer_id, img);
            d.history.execute(&mut d.project, Edit::InsertFrame { index, frame: f });
        }
    }
    let mut props = LayerProps::named("Rotoscopia");
    props.opacity = 0.6;
    props.locked = true;
    let index = d.layer_index() + 1;
    d.history.execute(&mut d.project, Edit::InsertLayer { index, layer: Layer { id: layer_id, props }, cels: existing });
    d.history.group_since(mark);
    d.invalidate_all();
    app.toast(ctx, tf!("{} imágenes importadas. Dibuja encima en una capa nueva.", files.len()));
}
