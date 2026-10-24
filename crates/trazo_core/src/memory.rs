//! Keeping memory low on big projects: which tiles to pack, and how much
//! memory the pixels take.
//!
//! The app calls [`cold_tiles`] now and then (cheap: it only clones `Arc`s)
//! and packs the result on a background thread with [`pack`]. Frames near
//! the one being drawn stay open so drawing is never slowed down.

use std::collections::HashSet;
use std::sync::Arc;

use crate::document::Project;
use crate::history::{Edit, History};
use crate::raster::{Tile, TiledImage};

/// Memory used by pixels.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub open_tiles: usize,
    pub packed_tiles: usize,
    pub bytes: usize,
}

impl Usage {
    pub fn megabytes(&self) -> f32 {
        self.bytes as f32 / (1024.0 * 1024.0)
    }
}

/// Every distinct tile of the project and its undo history.
fn each_tile(p: &Project, h: Option<&History>, mut f: impl FnMut(&Arc<Tile>)) {
    let mut seen: HashSet<*const Tile> = HashSet::new();
    let mut visit = |img: &TiledImage, f: &mut dyn FnMut(&Arc<Tile>)| {
        for t in img.tiles() {
            if seen.insert(Arc::as_ptr(t)) {
                f(t);
            }
        }
    };
    for fr in &p.frames {
        for c in fr.cels.values() {
            visit(c, &mut f);
        }
    }
    for c in p.shared_cels.values() {
        visit(c, &mut f);
    }
    if let Some(h) = h {
        h.for_each_image(&mut |img| visit(img, &mut f));
        h.for_each_tile(&mut |t| {
            if seen.insert(Arc::as_ptr(t)) {
                f(t);
            }
        });
    }
}

/// How much memory the pixels of the project (and its history) take.
pub fn usage(p: &Project, h: Option<&History>) -> Usage {
    let mut u = Usage::default();
    each_tile(p, h, |t| {
        if t.is_packed() {
            u.packed_tiles += 1;
        } else {
            u.open_tiles += 1;
        }
        u.bytes += t.heap_bytes();
    });
    u
}

/// Open tiles that can be packed: everything except the frames in `keep`
/// (indices), their onion-skin drawings (`ghosts` before/after) and fixed
/// backgrounds, which are on screen all the time.
pub fn cold_tiles(p: &Project, h: Option<&History>, keep: std::ops::RangeInclusive<usize>, ghosts: (usize, usize)) -> Vec<Arc<Tile>> {
    let mut hot: HashSet<*const Tile> = HashSet::new();
    for i in keep {
        if let Some(f) = p.frames.get(i) {
            // The drawings shown in those frames (they may start earlier),
            // and the onion-skin drawings around them.
            for l in &p.layers {
                if let Some(c) = p.visible_cel(i, l.id) {
                    hot.extend(c.tiles().map(Arc::as_ptr));
                }
                for k in (1..=ghosts.0 as i32).map(|k| -k).chain(1..=ghosts.1 as i32) {
                    if let Some(c) = p.neighbor_drawing(i, l.id, k).and_then(|s| p.frames[s].cel(l.id)) {
                        hot.extend(c.tiles().map(Arc::as_ptr));
                    }
                }
            }
            hot.extend(f.cels.values().flat_map(|c| c.tiles().map(Arc::as_ptr)));
        }
    }
    for c in p.shared_cels.values() {
        hot.extend(c.tiles().map(Arc::as_ptr));
    }
    let mut out = Vec::new();
    each_tile(p, h, |t| {
        if !hot.contains(&Arc::as_ptr(t)) && !t.is_packed() {
            out.push(t.clone());
        }
    });
    out
}

/// Packed tiles that are about to be used: the drawings shown in the frames
/// of `keep` and, with onion skin, their neighbouring drawings (`ghosts`
/// before and after). Opening them ahead keeps drawing on them fast.
pub fn hot_packed_tiles(p: &Project, keep: std::ops::RangeInclusive<usize>, ghosts: (usize, usize)) -> Vec<Arc<Tile>> {
    let mut seen: HashSet<*const Tile> = HashSet::new();
    let mut out = Vec::new();
    let mut add = |img: &TiledImage| {
        for t in img.tiles() {
            if t.is_packed() && seen.insert(Arc::as_ptr(t)) {
                out.push(t.clone());
            }
        }
    };
    for i in keep {
        if i >= p.frames.len() {
            break;
        }
        for l in &p.layers {
            if let Some(c) = p.visible_cel(i, l.id) {
                add(c);
            }
            for k in (1..=ghosts.0 as i32).map(|k| -k).chain(1..=ghosts.1 as i32) {
                if let Some(c) = p.neighbor_drawing(i, l.id, k).and_then(|s| p.frames[s].cel(l.id)) {
                    add(c);
                }
            }
        }
    }
    out
}

/// Open packed tiles (off the UI thread), so drawing on them doesn't decode.
pub fn open(tiles: &[Arc<Tile>]) {
    for t in tiles {
        drop(t.px());
    }
}

/// Pack tiles (run it off the UI thread). Returns the bytes saved.
pub fn pack(tiles: &[Arc<Tile>]) -> usize {
    tiles.iter().map(|t| t.pack()).sum()
}

impl History {
    /// Images kept by undo steps (layers and frames that were removed…).
    pub fn for_each_image(&self, f: &mut dyn FnMut(&TiledImage)) {
        fn walk(e: &Edit, f: &mut dyn FnMut(&TiledImage)) {
            match e {
                Edit::InsertFrame { frame, .. } | Edit::RemoveFrame { frame, .. } => frame.cels.values().for_each(&mut *f),
                Edit::InsertLayer { cels, .. } | Edit::RemoveLayer { cels, .. } => cels.iter().for_each(|(_, c)| f(c)),
                Edit::SharedCel { before, after, .. } => before.iter().chain(after.iter()).for_each(&mut *f),
                Edit::Timeline { before, after } => before.iter().chain(after.iter()).flat_map(|fr| fr.cels.values()).for_each(&mut *f),
                Edit::Batch(v) => v.iter().for_each(|e| walk(e, f)),
                _ => {}
            }
        }
        for e in self.steps() {
            walk(e, f);
        }
    }

    /// Single tiles kept by undo steps (the pixels before and after each stroke).
    pub fn for_each_tile(&self, f: &mut dyn FnMut(&Arc<Tile>)) {
        fn walk(e: &Edit, f: &mut dyn FnMut(&Arc<Tile>)) {
            match e {
                Edit::Pixels(p) => p.tiles.iter().flat_map(|c| c.before.iter().chain(c.after.iter())).for_each(&mut *f),
                Edit::Batch(v) => v.iter().for_each(|e| walk(e, f)),
                _ => {}
            }
        }
        for e in self.steps() {
            walk(e, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CanvasSettings;

    #[test]
    fn far_frames_pack_and_near_ones_stay_open() {
        let mut p = Project::new("m", CanvasSettings { width: 256, height: 256, ..Default::default() });
        let layer = p.layers[0].id;
        let h = History::new(10);
        for i in 0..10 {
            if i > 0 {
                let id = p.alloc_id();
                p.frames.push(crate::Frame::new(id));
            }
            let mut c = TiledImage::new(256, 256);
            for x in 0..256 {
                c.set_pixel(x, 30 + i, [0, 0, 0, 255]);
            }
            p.frames[i as usize].cels.insert(layer, c);
        }
        let before = usage(&p, Some(&h));
        let cold = cold_tiles(&p, Some(&h), 4..=5, (0, 0));
        assert_eq!(cold.len(), 8 * 4, "8 far frames × 4 tiles");
        let saved = pack(&cold);
        let after = usage(&p, Some(&h));
        assert!(saved > 0 && after.bytes < before.bytes / 3, "{before:?} → {after:?}");
        assert_eq!(after.open_tiles, 2 * 4);
        // Rendering a packed frame still works.
        let px = crate::composite::render_frame(&p, 0, false);
        assert!(px.chunks_exact(4).any(|q| q[3] == 255));
    }
}
