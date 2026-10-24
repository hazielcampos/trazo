//! Per-layer timing ("una pista por capa").
//!
//! Frames are columns of the timeline, one tick each. In every column a layer
//! either starts a drawing (it has a cel there), continues the drawing that
//! started earlier (its id is in [`Frame::cont`]), or shows nothing. So each
//! layer has its own row of drawings, each lasting as many frames as needed,
//! independently of the other layers.
//!
//! Every change here is one [`Edit::Timeline`] (a snapshot of the frame list;
//! cel pixels are shared tiles, so snapshots are cheap) and undoes in one step.

use crate::document::{Frame, LayerId, Project};
use crate::history::{Edit, History};
use crate::raster::TiledImage;

/// One drawing on a layer's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// Frame index where it starts.
    pub start: usize,
    /// How many frames it lasts.
    pub len: usize,
}

impl Block {
    pub fn end(&self) -> usize {
        self.start + self.len
    }

    pub fn contains(&self, frame: usize) -> bool {
        frame >= self.start && frame < self.end()
    }
}

/// A stretch of a layer's row: a drawing, or nothing (a gap).
#[derive(Clone)]
struct Span {
    cel: Option<TiledImage>,
    len: usize,
}

/// The drawings of a layer, in order.
pub fn blocks(p: &Project, layer: LayerId) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (i, f) in p.frames.iter().enumerate() {
        if f.cels.contains_key(&layer) {
            out.push(Block { start: i, len: 1 });
        } else if f.cont.contains(&layer) {
            match out.last_mut() {
                Some(b) if b.end() == i => b.len += 1,
                _ => {}
            }
        }
    }
    out
}

/// The drawing of `layer` shown at `frame`, if any.
pub fn block_at(p: &Project, layer: LayerId, frame: usize) -> Option<Block> {
    blocks(p, layer).into_iter().find(|b| b.contains(frame))
}

/// Read a layer's row as spans (drawings and gaps).
fn spans(p: &Project, layer: LayerId) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut in_drawing = false;
    for f in &p.frames {
        if let Some(c) = f.cels.get(&layer) {
            out.push(Span { cel: Some(c.clone()), len: 1 });
            in_drawing = true;
        } else if f.cont.contains(&layer) && in_drawing {
            out.last_mut().expect("drawing open").len += 1;
        } else {
            in_drawing = false;
            match out.last_mut() {
                Some(s) if s.cel.is_none() => s.len += 1,
                _ => out.push(Span { cel: None, len: 1 }),
            }
        }
    }
    out
}

/// Where a layer's content ends (frames after it show nothing on that layer).
fn content_end(p: &Project, layer: LayerId) -> usize {
    let mut end = 0;
    let mut pos = 0;
    for s in spans(p, layer) {
        pos += s.len;
        if s.cel.is_some() {
            end = pos;
        }
    }
    end
}

/// Write `spans` as the row of `layer`, growing or shrinking the frame list,
/// and record it as one undo step.
fn apply_spans(p: &mut Project, h: &mut History, layer: LayerId, new: Vec<Span>) {
    let before = p.frames.clone();
    let old_end = content_end(p, layer);
    let new_end: usize = {
        let mut end = 0;
        let mut pos = 0;
        for s in &new {
            pos += s.len;
            if s.cel.is_some() {
                end = pos;
            }
        }
        end
    };
    // Length: enough for every layer's content; keep frames the user left
    // empty at the end unless this layer was the one reaching that far.
    let others = p.layers.iter().filter(|l| l.id != layer).map(|l| {
        if let Some(v) = &l.props.video {
            v.end_tick(p.canvas.fps).max(0) as usize
        } else {
            content_end(p, l.id)
        }
    });
    let other_end = others.max().unwrap_or(0);
    let keep_tail = if old_end < p.frames.len() { p.frames.len() } else { 0 };
    let count = new_end.max(other_end).max(keep_tail).max(1);

    let mut frames = before.clone();
    while frames.len() < count {
        let id = p.alloc_id();
        frames.push(Frame::new(id));
    }
    frames.truncate(count);
    for f in &mut frames {
        f.cels.remove(&layer);
        f.cont.remove(&layer);
    }
    let mut pos = 0;
    for s in new {
        if let Some(cel) = s.cel {
            if pos < frames.len() {
                frames[pos].cels.insert(layer, cel);
            }
            for i in pos + 1..(pos + s.len).min(frames.len()) {
                frames[i].cont.insert(layer);
            }
        }
        pos += s.len;
    }
    h.execute(p, Edit::Timeline { before, after: frames });
}

/// Index in `spans` of the drawing starting at `start`.
fn span_index(spans: &[Span], start: usize) -> Option<usize> {
    let mut pos = 0;
    for (i, s) in spans.iter().enumerate() {
        if pos == start && s.cel.is_some() {
            return Some(i);
        }
        pos += s.len;
    }
    None
}

/// Make the drawing starting at `start` last `len` frames. Later drawings of
/// the same layer move along; other layers don't change.
pub fn set_len(p: &mut Project, h: &mut History, layer: LayerId, start: usize, len: usize) -> bool {
    let mut sp = spans(p, layer);
    let Some(i) = span_index(&sp, start) else { return false };
    let len = len.clamp(1, 100_000);
    if sp[i].len == len {
        return false;
    }
    sp[i].len = len;
    apply_spans(p, h, layer, sp);
    true
}

/// Insert a new drawing (`cel`, or an empty one) of `len` frames on `layer`
/// right after the drawing that covers `frame`; in a gap it starts at `frame`.
/// Returns the frame where it starts.
pub fn insert_drawing(p: &mut Project, h: &mut History, layer: LayerId, frame: usize, len: usize, cel: Option<TiledImage>) -> usize {
    let (w, ht) = (p.canvas.width, p.canvas.height);
    let cel = cel.unwrap_or_else(|| TiledImage::new(w, ht));
    let mut sp = spans(p, layer);
    let mut pos = 0;
    let mut at = sp.len();
    let mut start = pos;
    for (i, s) in sp.iter().enumerate() {
        if frame < pos + s.len {
            if s.cel.is_some() {
                at = i + 1;
                start = pos + s.len;
            } else {
                // Split the gap: drawing at `frame`, the rest of the gap after it.
                let (before, after) = (frame - pos, s.len - (frame - pos));
                let mut parts = Vec::new();
                if before > 0 {
                    parts.push(Span { cel: None, len: before });
                }
                parts.push(Span { cel: Some(cel), len: len.max(1) });
                if after > len.max(1) {
                    parts.push(Span { cel: None, len: after - len.max(1) });
                }
                sp.splice(i..=i, parts);
                apply_spans(p, h, layer, sp);
                return frame;
            }
            break;
        }
        pos += s.len;
        start = pos;
    }
    if at == sp.len() {
        // Past the end of the row: fill the gap up to `frame`.
        if frame > start {
            sp.push(Span { cel: None, len: frame - start });
            start = frame;
        }
        at = sp.len();
    }
    sp.insert(at, Span { cel: Some(cel), len: len.max(1) });
    apply_spans(p, h, layer, sp);
    start
}

/// Insert `cels` as new drawings of `len` frames each, right after the
/// drawing starting at `start`, in one undo step. Later drawings of the same
/// layer move along. Returns where the first new drawing starts.
pub fn insert_drawings_after(p: &mut Project, h: &mut History, layer: LayerId, start: usize, len: usize, cels: Vec<TiledImage>) -> Option<usize> {
    let mut sp = spans(p, layer);
    let i = span_index(&sp, start)?;
    if cels.is_empty() {
        return None;
    }
    let first = start + sp[i].len;
    let new = cels.into_iter().map(|c| Span { cel: Some(c), len: len.max(1) });
    sp.splice(i + 1..i + 1, new);
    apply_spans(p, h, layer, sp);
    Some(first)
}

/// Whether `layer` has drawings after the one starting at `start`.
pub fn has_drawings_after(p: &Project, layer: LayerId, start: usize) -> bool {
    blocks(p, layer).iter().any(|b| b.start > start)
}

/// Add a drawing at the end of a layer's row. Returns where it starts.
pub fn append_drawing(p: &mut Project, h: &mut History, layer: LayerId) -> usize {
    let end = content_end(p, layer);
    let (w, ht) = (p.canvas.width, p.canvas.height);
    let mut sp = spans(p, layer);
    // Drop the trailing gap, then add the drawing.
    let mut total: usize = sp.iter().map(|s| s.len).sum();
    while sp.last().is_some_and(|s| s.cel.is_none()) {
        total -= sp.pop().expect("non-empty").len;
    }
    debug_assert_eq!(total, end);
    sp.push(Span { cel: Some(TiledImage::new(w, ht)), len: 1 });
    apply_spans(p, h, layer, sp);
    end
}

/// Copy of the drawing starting at `start`, placed right after it.
pub fn duplicate(p: &mut Project, h: &mut History, layer: LayerId, start: usize) -> Option<usize> {
    let mut sp = spans(p, layer);
    let i = span_index(&sp, start)?;
    let copy = sp[i].clone();
    let at = start + copy.len;
    sp.insert(i + 1, copy);
    apply_spans(p, h, layer, sp);
    Some(at)
}

/// Remove the drawing starting at `start`; later drawings move back.
pub fn delete(p: &mut Project, h: &mut History, layer: LayerId, start: usize) -> bool {
    let mut sp = spans(p, layer);
    let Some(i) = span_index(&sp, start) else { return false };
    sp.remove(i);
    apply_spans(p, h, layer, sp);
    true
}

/// Swap the drawing starting at `start` with its neighbour (`-1` earlier, `1` later).
/// Returns where it starts afterwards.
pub fn shift(p: &mut Project, h: &mut History, layer: LayerId, start: usize, dir: i32) -> Option<usize> {
    let mut sp = spans(p, layer);
    let i = span_index(&sp, start)?;
    let j = if dir < 0 { i.checked_sub(1)? } else { i + 1 };
    if j >= sp.len() {
        return None;
    }
    sp.swap(i, j);
    let new_start: usize = sp[..j].iter().map(|s| s.len).sum();
    apply_spans(p, h, layer, sp);
    Some(new_start)
}

/// Old projects stored a duration per frame for all layers at once. Turn
/// each extra tick into its own frame where every layer continues.
pub fn normalize_holds(p: &mut Project) {
    if p.frames.iter().all(|f| f.hold <= 1) {
        return;
    }
    let old = std::mem::take(&mut p.frames);
    for mut f in old {
        let hold = f.hold.max(1);
        f.hold = 1;
        let showing: Vec<LayerId> = p.layers.iter().map(|l| l.id).filter(|id| f.cels.contains_key(id) || f.cont.contains(id)).collect();
        p.frames.push(f);
        for _ in 1..hold {
            let mut extra = Frame::new(p.alloc_id());
            extra.cont = showing.iter().copied().collect();
            p.frames.push(extra);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{CanvasSettings, Layer, LayerProps};

    fn project() -> (Project, LayerId, LayerId) {
        let mut p = Project::new("t", CanvasSettings { width: 64, height: 64, ..Default::default() });
        let a = p.layers[0].id;
        let b = p.alloc_id();
        p.layers.push(Layer { id: b, props: LayerProps::named("B") });
        for _ in 0..3 {
            let id = p.alloc_id();
            p.frames.push(Frame::new(id));
        }
        for i in 0..4 {
            p.frames[i].cel_mut(a, 64, 64).set_pixel(i as i32, 0, [255, 0, 0, 255]);
            p.frames[i].cel_mut(b, 64, 64).set_pixel(0, i as i32, [0, 0, 255, 255]);
        }
        (p, a, b)
    }

    #[test]
    fn stretching_moves_only_that_layer() {
        let (mut p, a, b) = project();
        let mut h = History::default();
        assert!(set_len(&mut p, &mut h, a, 1, 3));
        // A: drawing 0 [0], drawing 1 [1..4], drawing 2 [4], drawing 3 [5]
        assert_eq!(blocks(&p, a).iter().map(|b| (b.start, b.len)).collect::<Vec<_>>(), vec![(0, 1), (1, 3), (4, 1), (5, 1)]);
        assert_eq!(p.frames.len(), 6, "frames added at the end");
        // B is untouched: still one drawing per frame for its first 4 frames.
        assert_eq!(blocks(&p, b).len(), 4);
        assert_eq!(p.visible_cel(3, a).unwrap().pixel(1, 0)[3], 255, "frame 3 still shows drawing 1");
        assert_eq!(p.visible_cel(5, b).map(|_| ()), None, "B shows nothing after its end");
        h.undo(&mut p);
        assert_eq!(p.frames.len(), 4);
        assert_eq!(blocks(&p, a).len(), 4);
    }

    #[test]
    fn shrinking_the_longest_layer_shortens_the_animation() {
        let (mut p, a, b) = project();
        let mut h = History::default();
        delete(&mut p, &mut h, b, 3);
        assert_eq!(p.frames.len(), 4, "A still needs 4 frames");
        set_len(&mut p, &mut h, a, 0, 1);
        delete(&mut p, &mut h, a, 3);
        assert_eq!(p.frames.len(), 3);
    }

    #[test]
    fn insert_append_duplicate_shift() {
        let (mut p, a, _) = project();
        let mut h = History::default();
        let at = insert_drawing(&mut p, &mut h, a, 1, 2, None);
        assert_eq!(at, 2);
        assert_eq!(block_at(&p, a, 3), Some(Block { start: 2, len: 2 }));
        assert_eq!(append_drawing(&mut p, &mut h, a), 6);
        assert_eq!(duplicate(&mut p, &mut h, a, 0), Some(1));
        assert_eq!(p.visible_cel(1, a).unwrap().pixel(0, 0)[3], 255);
        assert_eq!(shift(&mut p, &mut h, a, 0, 1), Some(1));
        assert_eq!(shift(&mut p, &mut h, a, 0, -1), None);
    }

    #[test]
    fn old_holds_become_continued_frames() {
        let (mut p, a, b) = project();
        p.frames[1].hold = 3;
        normalize_holds(&mut p);
        assert_eq!(p.frames.len(), 6);
        assert!(p.frames.iter().all(|f| f.hold == 1));
        assert_eq!(block_at(&p, a, 3), Some(Block { start: 1, len: 3 }));
        assert_eq!(block_at(&p, b, 2), Some(Block { start: 1, len: 3 }));
    }

    #[test]
    fn painting_a_continued_frame_edits_the_drawing() {
        let (mut p, a, _) = project();
        let mut h = History::default();
        set_len(&mut p, &mut h, a, 0, 3);
        let f2 = p.frames[2].id;
        assert_eq!(p.cel_owner(f2, a), p.frames[0].id);
    }

    #[test]
    fn insert_several_after_in_one_step() {
        let mut p = Project::new("t", crate::CanvasSettings::default());
        let mut h = History::new(100);
        let layer = p.layers[0].id;
        assert_eq!(insert_drawing(&mut p, &mut h, layer, 0, 2, None), 0);
        let next = insert_drawing(&mut p, &mut h, layer, 0, 1, None);
        assert_eq!(next, 2);
        let undo_before = h.undo_len();
        let (w, ht) = (p.canvas.width, p.canvas.height);
        let first = insert_drawings_after(&mut p, &mut h, layer, 0, 2, vec![TiledImage::new(w, ht), TiledImage::new(w, ht)]);
        assert_eq!(first, Some(2));
        assert_eq!(h.undo_len(), undo_before + 1, "one undo step");
        let b = blocks(&p, layer);
        assert_eq!(b.iter().map(|b| (b.start, b.len)).collect::<Vec<_>>(), vec![(0, 2), (2, 2), (4, 2), (6, 1)]);
        assert!(has_drawings_after(&p, layer, 0));
        assert!(!has_drawings_after(&p, layer, 6));
    }

}
