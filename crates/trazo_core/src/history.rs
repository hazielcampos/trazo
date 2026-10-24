//! Undo/redo.
//!
//! Every change to a [`Project`] is described by an [`Edit`] that knows how to
//! apply itself forwards and backwards. Pixel edits store whole tiles before and
//! after the change; tiles are `Arc`s, so unchanged data is shared, not copied.

use std::collections::{BTreeMap, VecDeque};

use crate::document::{Frame, FrameId, Layer, LayerId, LayerProps, Project, SHARED_FRAME};
use crate::raster::{TileRef, TiledImage, TILE_BYTES};

#[derive(Clone)]
pub struct TileChange {
    pub index: usize,
    pub before: TileRef,
    pub after: TileRef,
}

#[derive(Clone)]
pub struct PixelEdit {
    pub frame: FrameId,
    pub layer: LayerId,
    pub tiles: Vec<TileChange>,
}

/// Records the original tiles of a cel while it is being modified.
#[derive(Default)]
pub struct PixelEditBuilder {
    before: BTreeMap<usize, TileRef>,
}

impl PixelEditBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember tile `idx` as it is now (only the first call per tile counts).
    pub fn touch(&mut self, img: &TiledImage, idx: usize) {
        self.before.entry(idx).or_insert_with(|| img.tile_ref(idx).clone());
    }

    /// The original version of a tile (before this edit started).
    pub fn original(&self, idx: usize) -> Option<&TileRef> {
        self.before.get(&idx)
    }

    pub fn is_empty(&self) -> bool {
        self.before.is_empty()
    }

    /// Indices of every tile touched so far.
    pub fn touched(&self) -> impl Iterator<Item = usize> + '_ {
        self.before.keys().copied()
    }

    /// Put the original tiles back but keep recording (used to re-render).
    pub fn restore(&self, img: &mut TiledImage) {
        for (idx, t) in &self.before {
            img.set_tile_ref(*idx, t.clone());
        }
    }

    /// Finish: drop tiles that became transparent and build the undo record.
    pub fn finish(self, img: &mut TiledImage, frame: FrameId, layer: LayerId) -> Option<PixelEdit> {
        let mut tiles = Vec::new();
        for (index, before) in self.before {
            if img.tile_ref(index).as_ref().is_some_and(|t| t.is_transparent()) {
                img.set_tile_ref(index, None);
            }
            let after = img.tile_ref(index).clone();
            let same = match (&before, &after) {
                (None, None) => true,
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                _ => false,
            };
            if !same {
                tiles.push(TileChange { index, before, after });
            }
        }
        (!tiles.is_empty()).then_some(PixelEdit { frame, layer, tiles })
    }

    /// Abort: put every original tile back.
    pub fn revert(self, img: &mut TiledImage) {
        for (idx, t) in self.before {
            img.set_tile_ref(idx, t);
        }
    }
}

/// A reversible change to the project.
#[derive(Clone)]
pub enum Edit {
    Pixels(PixelEdit),
    InsertFrame { index: usize, frame: Frame },
    RemoveFrame { index: usize, frame: Frame },
    MoveFrame { from: usize, to: usize },
    FrameHold { frame: FrameId, before: u32, after: u32 },
    InsertLayer { index: usize, layer: Layer, cels: Vec<(FrameId, TiledImage)> },
    RemoveLayer { index: usize, layer: Layer, cels: Vec<(FrameId, TiledImage)> },
    MoveLayer { from: usize, to: usize },
    LayerProps { layer: LayerId, before: LayerProps, after: LayerProps },
    /// Replace the shared cel of a layer.
    SharedCel { layer: LayerId, before: Option<TiledImage>, after: Option<TiledImage> },
    /// Any change to audio tracks or clips (the list is small; audio data is shared).
    Audio { before: Vec<crate::audio::AudioTrack>, after: Vec<crate::audio::AudioTrack> },
    /// Timeline markers (dialogue, phonemes).
    Markers { before: Vec<crate::audio::Marker>, after: Vec<crate::audio::Marker> },
    /// Whole frame list before/after a per-layer timing change (pixels are
    /// shared tiles, so this is cheap).
    Timeline { before: Vec<Frame>, after: Vec<Frame> },
    /// Several edits that undo as one step.
    Batch(Vec<Edit>),
}

impl Edit {
    /// Short human-readable label (shown in the UI as "Deshacer: …").
    pub fn label(&self) -> &'static str {
        match self {
            Edit::Pixels(_) => "trazo",
            Edit::InsertFrame { .. } => "añadir fotograma",
            Edit::RemoveFrame { .. } => "borrar fotograma",
            Edit::MoveFrame { .. } => "mover fotograma",
            Edit::FrameHold { .. } => "duración de fotograma",
            Edit::InsertLayer { .. } => "añadir capa",
            Edit::RemoveLayer { .. } => "borrar capa",
            Edit::MoveLayer { .. } => "mover capa",
            Edit::LayerProps { .. } => "propiedades de capa",
            Edit::SharedCel { .. } => "capa de fondo",
            Edit::Audio { .. } => "audio",
            Edit::Markers { .. } => "marcadores",
            Edit::Timeline { .. } => "línea de tiempo",
            Edit::Batch(_) => "varios cambios",
        }
    }

    /// Rough memory held by this edit (tiles may be shared, so this is an upper bound).
    pub fn approx_bytes(&self) -> usize {
        let cels = |c: &Vec<(FrameId, TiledImage)>| c.iter().map(|(_, i)| i.allocated_tiles() * TILE_BYTES).sum::<usize>();
        64 + match self {
            Edit::Pixels(e) => e.tiles.iter().map(|t| (t.before.is_some() as usize + t.after.is_some() as usize) * TILE_BYTES).sum(),
            Edit::InsertFrame { frame, .. } | Edit::RemoveFrame { frame, .. } => {
                frame.cels.values().map(|c| c.allocated_tiles() * TILE_BYTES).sum()
            }
            Edit::InsertLayer { cels: c, .. } | Edit::RemoveLayer { cels: c, .. } => cels(c),
            Edit::Batch(v) => v.iter().map(Edit::approx_bytes).sum(),
            // Two copies of the frame list: each cel keeps its own list of
            // tile pointers (the pixels themselves are shared).
            Edit::Timeline { before, after } => [before, after]
                .iter()
                .flat_map(|v| v.iter())
                .map(|f| 96 + f.cont.len() * 16 + f.cels.values().map(|c| 64 + c.tile_count() * 8).sum::<usize>())
                .sum(),
            _ => 0,
        }
    }

    /// Frames whose pixels this edit changes; `None` means "possibly all".
    pub fn affected_frames(&self) -> Option<Vec<FrameId>> {
        match self {
            Edit::Pixels(e) => Some(vec![e.frame]),
            Edit::InsertFrame { frame, .. } | Edit::RemoveFrame { frame, .. } => Some(vec![frame.id]),
            Edit::FrameHold { .. } | Edit::MoveFrame { .. } | Edit::Audio { .. } | Edit::Markers { .. } => Some(vec![]),
            Edit::Batch(v) => {
                let mut out = Vec::new();
                for e in v {
                    out.extend(e.affected_frames()?);
                }
                Some(out)
            }
            _ => None,
        }
    }

    pub fn redo(&self, p: &mut Project) {
        self.apply(p, true)
    }

    pub fn undo(&self, p: &mut Project) {
        self.apply(p, false)
    }

    fn apply(&self, p: &mut Project, forward: bool) {
        match self {
            Edit::Pixels(e) => {
                if let Some(cel) = p.cel_at_mut(e.frame, e.layer) {
                    for t in &e.tiles {
                        cel.set_tile_ref(t.index, if forward { t.after.clone() } else { t.before.clone() });
                    }
                }
            }
            Edit::InsertFrame { index, frame } | Edit::RemoveFrame { index, frame } => {
                let insert = matches!(self, Edit::InsertFrame { .. }) == forward;
                if insert {
                    p.frames.insert((*index).min(p.frames.len()), frame.clone());
                } else if *index < p.frames.len() {
                    p.frames.remove(*index);
                }
            }
            Edit::MoveFrame { from, to } => {
                let (a, b) = if forward { (*from, *to) } else { (*to, *from) };
                if a < p.frames.len() && b < p.frames.len() {
                    let f = p.frames.remove(a);
                    p.frames.insert(b, f);
                }
            }
            Edit::FrameHold { frame, before, after } => {
                if let Some(f) = p.frame_by_id_mut(*frame) {
                    f.hold = if forward { *after } else { *before };
                }
            }
            Edit::InsertLayer { index, layer, cels } | Edit::RemoveLayer { index, layer, cels } => {
                let insert = matches!(self, Edit::InsertLayer { .. }) == forward;
                if insert {
                    p.layers.insert((*index).min(p.layers.len()), layer.clone());
                    for (fid, img) in cels {
                        if *fid == SHARED_FRAME {
                            p.shared_cels.insert(layer.id, img.clone());
                        } else if let Some(f) = p.frame_by_id_mut(*fid) {
                            f.cels.insert(layer.id, img.clone());
                        }
                    }
                } else {
                    if let Some(i) = p.layer_index(layer.id) {
                        p.layers.remove(i);
                    }
                    for f in &mut p.frames {
                        f.cels.remove(&layer.id);
                        f.cont.remove(&layer.id);
                    }
                    p.shared_cels.remove(&layer.id);
                }
            }
            Edit::MoveLayer { from, to } => {
                let (a, b) = if forward { (*from, *to) } else { (*to, *from) };
                if a < p.layers.len() && b < p.layers.len() {
                    let l = p.layers.remove(a);
                    p.layers.insert(b, l);
                }
            }
            Edit::LayerProps { layer, before, after } => {
                if let Some(i) = p.layer_index(*layer) {
                    p.layers[i].props = if forward { after.clone() } else { before.clone() };
                }
            }
            Edit::Audio { before, after } => {
                p.audio.tracks = if forward { after.clone() } else { before.clone() };
            }
            Edit::Markers { before, after } => {
                p.audio.markers = if forward { after.clone() } else { before.clone() };
            }
            Edit::SharedCel { layer, before, after } => {
                match if forward { after } else { before } {
                    Some(c) => p.shared_cels.insert(*layer, c.clone()),
                    None => p.shared_cels.remove(layer),
                };
            }
            Edit::Timeline { before, after } => {
                p.frames = if forward { after.clone() } else { before.clone() };
            }
            Edit::Batch(edits) => {
                if forward {
                    edits.iter().for_each(|e| e.apply(p, true));
                } else {
                    edits.iter().rev().for_each(|e| e.apply(p, false));
                }
            }
        }
    }
}

/// Linear undo history limited by step count and by memory.
pub struct History {
    undo: VecDeque<(Edit, usize)>,
    redo: Vec<(Edit, usize)>,
    limit: usize,
    byte_budget: usize,
    bytes: usize,
    /// Bumped on every change; lets the UI know when to save or redraw.
    revision: u64,
    /// Logical number of undo steps, including ones dropped by the limits
    /// (so a grouping mark stays valid when old steps fall off).
    depth: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new(300)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self::with_budget(limit, 512 * 1024 * 1024)
    }

    pub fn with_budget(limit: usize, byte_budget: usize) -> Self {
        Self { undo: VecDeque::new(), redo: Vec::new(), limit: limit.max(1), byte_budget, bytes: 0, revision: 0, depth: 0 }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }

    /// Apply an edit and record it.
    pub fn execute(&mut self, p: &mut Project, edit: Edit) {
        edit.redo(p);
        self.push_applied(edit);
    }

    /// Record an edit whose effect is already in the project (e.g. a live stroke).
    pub fn push_applied(&mut self, edit: Edit) {
        for (_, b) in self.redo.drain(..) {
            self.bytes -= b;
        }
        let b = edit.approx_bytes();
        self.bytes += b;
        self.undo.push_back((edit, b));
        self.depth += 1;
        // Keep at least the newest step even if it is huge.
        while self.undo.len() > self.limit || (self.bytes > self.byte_budget && self.undo.len() > 1) {
            if let Some((_, old)) = self.undo.pop_front() {
                self.bytes -= old;
            }
        }
        self.revision += 1;
    }

    /// Audio assets referenced by any undo or redo step.
    pub fn audio_assets(&self) -> std::collections::BTreeSet<crate::audio::AssetId> {
        fn walk(e: &Edit, out: &mut std::collections::BTreeSet<crate::audio::AssetId>) {
            match e {
                Edit::Audio { before, after } => {
                    for t in before.iter().chain(after) {
                        out.extend(t.clips.iter().map(|c| c.asset));
                    }
                }
                Edit::Batch(v) => v.iter().for_each(|e| walk(e, out)),
                _ => {}
            }
        }
        let mut out = std::collections::BTreeSet::new();
        for (e, _) in self.undo.iter().chain(self.redo.iter()) {
            walk(e, &mut out);
        }
        out
    }

    /// Mark to pass to [`History::group_since`] after several operations.
    pub fn undo_len(&self) -> usize {
        self.depth as usize
    }

    /// Merge every step recorded after `mark` (a previous `undo_len`) into
    /// one, so a multi-frame action undoes in a single step.
    pub fn group_since(&mut self, mark: usize) {
        let n = (self.depth.saturating_sub(mark as u64) as usize).min(self.undo.len());
        if n <= 1 {
            return;
        }
        let tail: Vec<(Edit, usize)> = self.undo.drain(self.undo.len() - n..).collect();
        let bytes = tail.iter().map(|(_, b)| b).sum();
        self.undo.push_back((Edit::Batch(tail.into_iter().map(|(e, _)| e).collect()), bytes));
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, p: &mut Project) -> Option<&Edit> {
        let e = self.undo.pop_back()?;
        self.depth = self.depth.saturating_sub(1);
        e.0.undo(p);
        self.redo.push(e);
        self.revision += 1;
        self.redo.last().map(|e| &e.0)
    }

    pub fn redo(&mut self, p: &mut Project) -> Option<&Edit> {
        let e = self.redo.pop()?;
        e.0.redo(p);
        self.undo.push_back(e);
        self.depth += 1;
        self.revision += 1;
        self.undo.back().map(|e| &e.0)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
        self.depth = 0;
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CanvasSettings;

    #[test]
    fn pixel_undo_redo() {
        let mut p = Project::new("t", CanvasSettings { width: 128, height: 128, ..Default::default() });
        let (fid, lid) = (p.frames[0].id, p.layers[0].id);
        let mut h = History::default();

        let cel = p.frames[0].cel_mut(lid, 128, 128);
        let mut b = PixelEditBuilder::new();
        b.touch(cel, 0);
        cel.set_pixel(3, 3, [255, 0, 0, 255]);
        let e = b.finish(cel, fid, lid).expect("changed");
        h.push_applied(Edit::Pixels(e));

        h.undo(&mut p);
        assert_eq!(p.frames[0].cel(lid).unwrap().pixel(3, 3), [0; 4]);
        h.redo(&mut p);
        assert_eq!(p.frames[0].cel(lid).unwrap().pixel(3, 3), [255, 0, 0, 255]);
    }

    #[test]
    fn structural_undo() {
        let mut p = Project::new("t", CanvasSettings::default());
        let mut h = History::default();
        let id = p.alloc_id();
        h.execute(&mut p, Edit::InsertFrame { index: 1, frame: Frame::new(id) });
        assert_eq!(p.frames.len(), 2);
        let lid = p.alloc_id();
        let layer = Layer { id: lid, props: LayerProps::named("B") };
        h.execute(&mut p, Edit::InsertLayer { index: 1, layer, cels: vec![] });
        h.execute(&mut p, Edit::MoveLayer { from: 1, to: 0 });
        assert_eq!(p.layers[0].id, lid);
        h.undo(&mut p);
        h.undo(&mut p);
        h.undo(&mut p);
        assert_eq!(p.frames.len(), 1);
        assert_eq!(p.layers.len(), 1);
        h.redo(&mut p);
        assert_eq!(p.frames.len(), 2);
    }

    #[test]
    fn limit_drops_oldest() {
        let mut p = Project::new("t", CanvasSettings::default());
        let mut h = History::new(2);
        for _ in 0..5 {
            let id = p.alloc_id();
            h.execute(&mut p, Edit::InsertFrame { index: 0, frame: Frame::new(id) });
        }
        assert!(h.undo(&mut p).is_some());
        assert!(h.undo(&mut p).is_some());
        assert!(h.undo(&mut p).is_none());
    }

    #[test]
    fn grouping_works_with_a_full_history() {
        let mut p = Project::new("t", crate::CanvasSettings::default());
        let mut h = History::new(3);
        for _ in 0..3 {
            let id = p.alloc_id();
            h.execute(&mut p, Edit::InsertFrame { index: 0, frame: crate::Frame::new(id) });
        }
        let mark = h.undo_len();
        for _ in 0..2 {
            let id = p.alloc_id();
            h.execute(&mut p, Edit::InsertFrame { index: 0, frame: crate::Frame::new(id) });
        }
        h.group_since(mark);
        assert_eq!(p.frames.len(), 6);
        h.undo(&mut p);
        assert_eq!(p.frames.len(), 4, "both new frames undone in one step");
    }

    #[test]
    fn grouping_undoes_together() {
        let mut p = Project::new("t", CanvasSettings::default());
        let mut h = History::default();
        let mark = h.undo_len();
        for _ in 0..3 {
            let id = p.alloc_id();
            h.execute(&mut p, Edit::InsertFrame { index: 0, frame: Frame::new(id) });
        }
        h.group_since(mark);
        assert_eq!(p.frames.len(), 4);
        h.undo(&mut p);
        assert_eq!(p.frames.len(), 1);
    }
}
