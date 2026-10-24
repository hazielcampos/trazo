//! High-level, undoable editing operations.
//!
//! The UI calls these instead of mutating the project directly, so every user
//! action automatically gets undo/redo.

use crate::color::{blend_px, pack};
use crate::document::{Frame, FrameId, Layer, LayerId, LayerProps, Project};
use crate::history::{Edit, History, PixelEditBuilder};
use crate::raster::{TiledImage, TILE};

/// Insert an empty frame at `index`. Returns the new index.
pub fn insert_blank_frame(p: &mut Project, h: &mut History, index: usize) -> usize {
    let index = index.min(p.frames.len());
    let id = p.alloc_id();
    h.execute(p, Edit::InsertFrame { index, frame: Frame::new(id) });
    index
}

/// Copy frame `index` right after itself. Returns the new index.
pub fn duplicate_frame(p: &mut Project, h: &mut History, index: usize) -> Option<usize> {
    let src = p.frames.get(index)?;
    let mut frame = src.clone(); // tiles are shared until edited
    frame.id = p.alloc_id();
    h.execute(p, Edit::InsertFrame { index: index + 1, frame });
    Some(index + 1)
}

/// Delete a frame; the last remaining frame is cleared instead.
pub fn delete_frame(p: &mut Project, h: &mut History, index: usize) {
    if index >= p.frames.len() {
        return;
    }
    if p.frames.len() == 1 {
        clear_frame(p, h, index);
        return;
    }
    let frame = p.frames[index].clone();
    h.execute(p, Edit::RemoveFrame { index, frame });
}

pub fn move_frame(p: &mut Project, h: &mut History, from: usize, to: usize) {
    if from != to && from < p.frames.len() && to < p.frames.len() {
        h.execute(p, Edit::MoveFrame { from, to });
    }
}

pub fn set_frame_hold(p: &mut Project, h: &mut History, index: usize, hold: u32) {
    let Some(f) = p.frames.get(index) else { return };
    let hold = hold.clamp(1, 240);
    if f.hold != hold {
        h.execute(p, Edit::FrameHold { frame: f.id, before: f.hold, after: hold });
    }
}

/// Erase every layer of one frame.
pub fn clear_frame(p: &mut Project, h: &mut History, index: usize) {
    let Some(f) = p.frames.get(index) else { return };
    let (fid, layers): (FrameId, Vec<LayerId>) = (f.id, f.cels.keys().copied().collect());
    let edits: Vec<Edit> = layers.into_iter().filter_map(|lid| clear_cel_edit(p, fid, lid)).collect();
    if !edits.is_empty() {
        h.push_applied(Edit::Batch(edits));
    }
}

/// Erase one layer in one frame.
pub fn clear_cel(p: &mut Project, h: &mut History, frame: FrameId, layer: LayerId) -> bool {
    match clear_cel_edit(p, frame, layer) {
        Some(e) => {
            h.push_applied(e);
            true
        }
        None => false,
    }
}

fn clear_cel_edit(p: &mut Project, frame: FrameId, layer: LayerId) -> Option<Edit> {
    let frame = p.cel_owner(frame, layer);
    let cel = p.cel_at_mut(frame, layer)?;
    let mut b = PixelEditBuilder::new();
    for i in 0..cel.tile_count() {
        if cel.tile_ref(i).is_some() {
            b.touch(cel, i);
            cel.set_tile_ref(i, None);
        }
    }
    b.finish(cel, frame, layer).map(Edit::Pixels)
}

/// Add an empty layer named `name` at `index` (0 = bottom). Returns the new layer id.
pub fn add_layer(p: &mut Project, h: &mut History, index: usize, name: String) -> LayerId {
    let id = p.alloc_id();
    let index = index.min(p.layers.len());
    h.execute(p, Edit::InsertLayer { index, layer: Layer { id, props: LayerProps::named(name) }, cels: vec![] });
    id
}

pub fn duplicate_layer(p: &mut Project, h: &mut History, layer: LayerId) -> Option<LayerId> {
    let i = p.layer_index(layer)?;
    let id = p.alloc_id();
    let mut props = p.layers[i].props.clone();
    props.name = format!("{} copia", props.name);
    let cels = p.layer_cels(layer);
    let before = p.frames.clone();
    h.execute(p, Edit::InsertLayer { index: i + 1, layer: Layer { id, props }, cels });
    // Same timing as the original.
    let mut after = p.frames.clone();
    for f in &mut after {
        if f.cont.contains(&layer) {
            f.cont.insert(id);
        }
    }
    let mark = h.undo_len();
    h.execute(p, Edit::Timeline { before, after });
    h.group_since(mark - 1);
    Some(id)
}

/// Delete a layer (the last layer can't be deleted).
pub fn delete_layer(p: &mut Project, h: &mut History, layer: LayerId) -> bool {
    if p.layers.len() <= 1 {
        return false;
    }
    let Some(index) = p.layer_index(layer) else { return false };
    let cels = p.layer_cels(layer);
    let l = p.layers[index].clone();
    // The frame snapshot brings its timing back on undo.
    let before = p.frames.clone();
    let remove = Edit::RemoveLayer { index, layer: l, cels };
    remove.redo(p);
    let after = p.frames.clone();
    h.push_applied(Edit::Batch(vec![Edit::Timeline { before, after }, remove]));
    true
}

pub fn move_layer(p: &mut Project, h: &mut History, from: usize, to: usize) {
    if from != to && from < p.layers.len() && to < p.layers.len() {
        h.execute(p, Edit::MoveLayer { from, to });
    }
}

pub fn set_layer_props(p: &mut Project, h: &mut History, layer: LayerId, props: LayerProps) {
    let Some(i) = p.layer_index(layer) else { return };
    let before = p.layers[i].props.clone();
    if before != props {
        h.execute(p, Edit::LayerProps { layer, before, after: props });
    }
}

/// Add a video layer at `index` (0 = bottom) with `opacity`. Returns its id.
pub fn add_video_layer(p: &mut Project, h: &mut History, index: usize, name: String, video: crate::video::VideoLayer, opacity: f32) -> LayerId {
    let id = p.alloc_id();
    let mut props = LayerProps::named(name);
    props.video = Some(video);
    props.onion_skin = false;
    props.opacity = opacity.clamp(0.0, 1.0);
    let index = index.min(p.layers.len());
    h.execute(p, Edit::InsertLayer { index, layer: Layer { id, props }, cels: vec![] });
    id
}

/// Lengthen the animation to at least `ticks` frames. With a `layer`, the new
/// time gets empty drawings on it, one every `hold` frames ("a doses" = 2),
/// ready to draw on. One undo step. Returns how many drawings were added.
pub fn extend_to_ticks(p: &mut Project, h: &mut History, ticks: u32, hold: u32, layer: Option<LayerId>) -> usize {
    let hold = hold.clamp(1, 12) as usize;
    let ticks = (ticks as usize).min(100_000);
    if ticks <= p.frames.len() {
        return 0;
    }
    let before = p.frames.clone();
    let (w, ht) = (p.canvas.width, p.canvas.height);
    let start = p.frames.len();
    let mut after = before.clone();
    let mut added = 0;
    for i in start..ticks {
        let mut f = crate::Frame::new(p.alloc_id());
        if let Some(l) = layer {
            if (i - start).is_multiple_of(hold) {
                f.cels.insert(l, TiledImage::new(w, ht));
                added += 1;
            } else {
                f.cont.insert(l);
            }
        }
        after.push(f);
    }
    h.execute(p, Edit::Timeline { before, after });
    added.max(1)
}

/// Replace the audio track list as one undo step (no-op if unchanged).
pub fn set_audio_tracks(p: &mut Project, h: &mut History, tracks: Vec<crate::audio::AudioTrack>) {
    if p.audio.tracks != tracks {
        let before = p.audio.tracks.clone();
        h.execute(p, Edit::Audio { before, after: tracks });
    }
}

/// Replace the marker list as one undo step (kept sorted by time).
pub fn set_markers(p: &mut Project, h: &mut History, mut markers: Vec<crate::audio::Marker>) {
    markers.sort_by_key(|m| m.at);
    if p.audio.markers != markers {
        let before = p.audio.markers.clone();
        h.execute(p, Edit::Markers { before, after: markers });
    }
}

/// Turn a layer into a shared background (one drawing in every frame) or back.
/// When turning it on, the drawing of `from_frame` becomes the shared one.
pub fn set_layer_shared(p: &mut Project, h: &mut History, layer: LayerId, shared: bool, from_frame: usize) {
    let Some(i) = p.layer_index(layer) else { return };
    let before = p.layers[i].props.clone();
    if before.shared == shared {
        return;
    }
    let mut after = before.clone();
    after.shared = shared;
    let mut edits = vec![Edit::LayerProps { layer, before, after }];
    if shared && !p.shared_cels.contains_key(&layer) {
        let cel = p.frames.get(from_frame).and_then(|f| f.cel(layer)).cloned();
        edits.insert(0, Edit::SharedCel { layer, before: None, after: cel });
    }
    h.execute(p, Edit::Batch(edits));
}

/// Merge a layer into the one below it, in every frame. Returns the lower id.
pub fn merge_down(p: &mut Project, h: &mut History, layer: LayerId) -> Option<LayerId> {
    let i = p.layer_index(layer)?;
    if i == 0 {
        return None;
    }
    let lower = p.layers[i - 1].id;
    let upper = p.layers[i].clone();
    if upper.props.shared || p.layers[i - 1].props.shared {
        return None; // merging a background into animated frames is ambiguous
    }
    if upper.props.video.is_some() || p.layers[i - 1].props.video.is_some() {
        return None; // video pictures aren't drawings
    }
    if p.frames.iter().any(|f| f.cont.contains(&layer) || f.cont.contains(&lower)) {
        return None; // different timings can't be merged frame by frame
    }
    let (w, hgt) = (p.canvas.width, p.canvas.height);
    let mut edits = Vec::new();
    for f in &mut p.frames {
        let Some(src) = f.cels.get(&layer).cloned() else { continue };
        if src.is_empty() {
            continue;
        }
        let fid = f.id;
        let dst = f.cel_mut(lower, w, hgt);
        let mut b = PixelEditBuilder::new();
        merge_into(dst, &src, upper.props.opacity, upper.props.blend, &mut b);
        if let Some(e) = b.finish(dst, fid, lower) {
            edits.push(Edit::Pixels(e));
        }
    }
    // Pixel edits are already applied; removing the layer goes through redo.
    let cels = p.layer_cels(layer);
    let remove = Edit::RemoveLayer { index: i, layer: upper, cels };
    remove.redo(p);
    edits.push(remove);
    h.push_applied(Edit::Batch(edits));
    Some(lower)
}

fn merge_into(dst: &mut TiledImage, src: &TiledImage, opacity: f32, mode: crate::color::BlendMode, b: &mut PixelEditBuilder) {
    for idx in 0..src.tile_count() {
        let Some(st) = src.tile_ref(idx).clone() else { continue };
        b.touch(dst, idx);
        let dt = dst.tile_mut(idx);
        for i in 0..TILE * TILE {
            let o = i * 4;
            let s = [st.px[o], st.px[o + 1], st.px[o + 2], st.px[o + 3]];
            if s[3] == 0 {
                continue;
            }
            let d = [dt.px[o], dt.px[o + 1], dt.px[o + 2], dt.px[o + 3]].map(|v| v as f32 / 255.0);
            dt.px[o..o + 4].copy_from_slice(&pack(blend_px(d, s, opacity, mode)));
        }
    }
}

#[cfg(test)]
mod video_tests {
    use super::*;
    use crate::document::CanvasSettings;

    #[test]
    fn extending_is_one_undo_step_with_holds() {
        let mut p = Project::new("t", CanvasSettings::default());
        let mut h = History::default();
        let l = p.layers[0].id;
        assert_eq!(extend_to_ticks(&mut p, &mut h, 9, 2, Some(l)), 4);
        assert_eq!((p.frames.len(), p.total_ticks()), (9, 9));
        assert_eq!(crate::timeline::block_at(&p, l, 2), Some(crate::timeline::Block { start: 1, len: 2 }));
        h.undo(&mut p);
        assert_eq!(p.frames.len(), 1, "a single undo removes them all");
        assert_eq!(extend_to_ticks(&mut p, &mut h, 1, 1, None), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{CanvasSettings, SHARED_FRAME};

    fn proj() -> (Project, History) {
        (Project::new("t", CanvasSettings { width: 64, height: 64, ..Default::default() }), History::default())
    }

    #[test]
    fn frames_lifecycle() {
        let (mut p, mut h) = proj();
        let lid = p.layers[0].id;
        p.frames[0].cel_mut(lid, 64, 64).set_pixel(1, 1, [0, 0, 0, 255]);
        duplicate_frame(&mut p, &mut h, 0);
        assert_eq!(p.frames.len(), 2);
        assert_eq!(p.frames[1].cel(lid).unwrap().pixel(1, 1)[3], 255);
        delete_frame(&mut p, &mut h, 1);
        delete_frame(&mut p, &mut h, 0); // last frame: cleared, not removed
        assert_eq!(p.frames.len(), 1);
        assert!(p.frames[0].is_blank());
        h.undo(&mut p);
        assert_eq!(p.frames[0].cel(lid).unwrap().pixel(1, 1)[3], 255);
    }

    #[test]
    fn merge_down_combines_and_undoes() {
        let (mut p, mut h) = proj();
        let bottom = p.layers[0].id;
        let top = add_layer(&mut p, &mut h, 1, "Capa 2".into());
        p.frames[0].cel_mut(top, 64, 64).set_pixel(5, 5, [255, 0, 0, 255]);
        assert_eq!(merge_down(&mut p, &mut h, top), Some(bottom));
        assert_eq!(p.layers.len(), 1);
        assert_eq!(p.frames[0].cel(bottom).unwrap().pixel(5, 5), [255, 0, 0, 255]);
        h.undo(&mut p);
        assert_eq!(p.layers.len(), 2);
        assert_eq!(p.frames[0].cel(bottom).unwrap().pixel(5, 5), [0; 4]);
        assert_eq!(p.frames[0].cel(top).unwrap().pixel(5, 5), [255, 0, 0, 255]);
    }

    #[test]
    fn cannot_delete_last_layer() {
        let (mut p, mut h) = proj();
        let id = p.layers[0].id;
        assert!(!delete_layer(&mut p, &mut h, id));
    }

    #[test]
    fn shared_layer_shows_in_every_frame() {
        let (mut p, mut h) = proj();
        let lid = p.layers[0].id;
        p.frames[0].cel_mut(lid, 64, 64).set_pixel(2, 2, [0, 0, 255, 255]);
        insert_blank_frame(&mut p, &mut h, 1);
        set_layer_shared(&mut p, &mut h, lid, true, 0);
        assert_eq!(p.cel_owner(p.frames[1].id, lid), SHARED_FRAME);
        assert_eq!(p.visible_cel(1, lid).unwrap().pixel(2, 2), [0, 0, 255, 255]);
        let dup = duplicate_layer(&mut p, &mut h, lid).unwrap();
        assert!(p.shared_cels.contains_key(&dup));
        h.undo(&mut p);
        h.undo(&mut p);
        assert!(!p.layers[0].props.shared);
        assert!(p.shared_cels.is_empty());
    }
}
