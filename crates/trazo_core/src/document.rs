//! The animation document: canvas settings, layers and frames.
//!
//! Layers are shared by every frame (like FlipaClip): a frame holds one cel
//! per layer, created lazily the first time something is drawn on it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::color::{BlendMode, Rgba};
use crate::raster::TiledImage;

pub type LayerId = u64;
pub type FrameId = u64;

/// Pseudo frame id that owns the single cel of a shared (background) layer.
pub const SHARED_FRAME: FrameId = 0;

fn yes() -> bool {
    true
}

/// Editable properties of a layer (kept together so undo can swap them as one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerProps {
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// Paint only where the layer already has pixels (coloring line art).
    pub alpha_lock: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    /// One drawing shown in every frame (backgrounds, static props).
    #[serde(default)]
    pub shared: bool,
    /// Show this layer in onion-skin ghosts.
    #[serde(default = "yes")]
    pub onion_skin: bool,
    /// Line-art layer the paint bucket uses to find edges.
    #[serde(default)]
    pub reference: bool,
    /// Clipping mask: only visible where the layer below has pixels.
    #[serde(default)]
    pub clip: bool,
    /// Video layer: shows imported video pictures instead of drawings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<crate::video::VideoLayer>,
    /// Folder the layer belongs to (layers of a folder sit next to each other).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub props: LayerProps,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub id: FrameId,
    /// How many timeline ticks this drawing stays on screen ("on twos" = 2).
    pub hold: u32,
    pub cels: BTreeMap<LayerId, TiledImage>,
    /// Layers whose drawing from an earlier frame is still shown here
    /// (per-layer timing, see [`crate::timeline`]).
    pub cont: BTreeSet<LayerId>,
}

impl Frame {
    pub fn new(id: FrameId) -> Self {
        Self { id, hold: 1, cels: BTreeMap::new(), cont: BTreeSet::new() }
    }

    pub fn cel(&self, layer: LayerId) -> Option<&TiledImage> {
        self.cels.get(&layer)
    }

    pub fn cel_mut(&mut self, layer: LayerId, w: u32, h: u32) -> &mut TiledImage {
        self.cels.entry(layer).or_insert_with(|| TiledImage::new(w, h))
    }

    pub fn is_blank(&self) -> bool {
        self.cels.values().all(|c| c.is_empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub background: Rgba,
    /// Framing for the video (off by default).
    #[serde(default)]
    pub camera: crate::camera::Camera,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self { width: 1280, height: 720, fps: 12, background: Rgba::WHITE, camera: Default::default() }
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub canvas: CanvasSettings,
    /// Bottom-to-top order.
    pub layers: Vec<Layer>,
    pub frames: Vec<Frame>,
    /// Cels of shared layers (see [`LayerProps::shared`]).
    pub shared_cels: BTreeMap<LayerId, TiledImage>,
    /// Sound: tracks, clips and imported audio.
    pub audio: crate::audio::AudioTimeline,
    next_id: u64,
}

impl Project {
    /// New project with one layer and one empty frame.
    pub fn new(name: impl Into<String>, canvas: CanvasSettings) -> Self {
        let mut p = Self { name: name.into(), canvas, layers: Vec::new(), frames: Vec::new(), shared_cels: BTreeMap::new(), audio: Default::default(), next_id: 1 };
        let lid = p.alloc_id();
        p.layers.push(Layer { id: lid, props: LayerProps::named("Capa 1") });
        let fid = p.alloc_id();
        p.frames.push(Frame::new(fid));
        p
    }

    /// Used by the loader to rebuild a project with known ids.
    pub(crate) fn from_parts(
        name: String,
        canvas: CanvasSettings,
        layers: Vec<Layer>,
        frames: Vec<Frame>,
        shared_cels: BTreeMap<LayerId, TiledImage>,
        audio: crate::audio::AudioTimeline,
        next_id: u64,
    ) -> Self {
        let audio_ids = audio
            .tracks
            .iter()
            .flat_map(|t| std::iter::once(t.id).chain(t.clips.iter().map(|c| c.id)))
            .chain(audio.assets.keys().copied())
            .chain(audio.markers.iter().map(|m| m.id));
        let max_id = layers.iter().map(|l| l.id).chain(frames.iter().map(|f| f.id)).chain(audio_ids).max().unwrap_or(0);
        Self { name, canvas, layers, frames, shared_cels, audio, next_id: next_id.max(max_id + 1) }
    }

    /// Which frame id owns the cel for `layer` in `frame` (shared layers use [`SHARED_FRAME`]).
    pub fn cel_owner(&self, frame: FrameId, layer: LayerId) -> FrameId {
        if self.layer(layer).is_some_and(|l| l.props.shared) {
            return SHARED_FRAME;
        }
        // A frame that continues a drawing paints on the frame where it starts.
        self.frame_index(frame).and_then(|i| self.drawing_start(i, layer)).map_or(frame, |s| self.frames[s].id)
    }

    /// Index of the frame where the drawing of `layer` shown at `i` starts.
    pub fn drawing_start(&self, i: usize, layer: LayerId) -> Option<usize> {
        let mut j = i.min(self.frames.len().checked_sub(1)?);
        loop {
            let f = &self.frames[j];
            if f.cels.contains_key(&layer) {
                return Some(j);
            }
            if !f.cont.contains(&layer) || j == 0 {
                return None;
            }
            j -= 1;
        }
    }

    /// The cel visible for `layer` in the frame at `frame_index`.
    pub fn visible_cel(&self, frame_index: usize, layer: LayerId) -> Option<&TiledImage> {
        let shared = self.layer(layer).is_some_and(|l| l.props.shared);
        if shared {
            self.shared_cels.get(&layer)
        } else {
            let s = self.drawing_start(frame_index, layer)?;
            self.frames[s].cel(layer)
        }
    }

    /// Start of the `k`-th drawing of `layer` before (`k < 0`) or after
    /// (`k > 0`) the one shown at `i` (for onion skin).
    pub fn neighbor_drawing(&self, i: usize, layer: LayerId, k: i32) -> Option<usize> {
        let n = self.frames.len();
        if k < 0 {
            let mut s = self.drawing_start(i, layer).unwrap_or(i);
            for _ in 0..(-k) {
                let mut j = s.checked_sub(1)?;
                s = loop {
                    if let Some(t) = self.drawing_start(j, layer) {
                        break t;
                    }
                    j = j.checked_sub(1)?;
                };
            }
            Some(s)
        } else {
            let mut s = match self.drawing_start(i, layer) {
                Some(s) => s,
                None => i,
            };
            for _ in 0..k {
                let mut j = s + 1;
                s = loop {
                    if j >= n {
                        return None;
                    }
                    if self.frames[j].cels.contains_key(&layer) {
                        break j;
                    }
                    j += 1;
                };
            }
            Some(s)
        }
    }

    /// Mutable cel by owner id (a frame id or [`SHARED_FRAME`]), created if missing.
    pub fn cel_at_mut(&mut self, owner: FrameId, layer: LayerId) -> Option<&mut TiledImage> {
        let (w, h) = (self.canvas.width, self.canvas.height);
        if owner == SHARED_FRAME {
            Some(self.shared_cels.entry(layer).or_insert_with(|| TiledImage::new(w, h)))
        } else {
            Some(self.frame_by_id_mut(owner)?.cel_mut(layer, w, h))
        }
    }

    /// All cels of a layer, including its shared cel (keyed by [`SHARED_FRAME`]).
    pub fn layer_cels(&self, layer: LayerId) -> Vec<(FrameId, TiledImage)> {
        let mut v: Vec<_> = self.frames.iter().filter_map(|f| f.cel(layer).map(|c| (f.id, c.clone()))).collect();
        if let Some(c) = self.shared_cels.get(&layer) {
            v.push((SHARED_FRAME, c.clone()));
        }
        v
    }

    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn frame_index(&self, id: FrameId) -> Option<usize> {
        self.frames.iter().position(|f| f.id == id)
    }

    pub fn frame_by_id_mut(&mut self, id: FrameId) -> Option<&mut Frame> {
        self.frames.iter_mut().find(|f| f.id == id)
    }

    /// Total length in timeline ticks (sum of holds).
    pub fn total_ticks(&self) -> u32 {
        self.frames.iter().map(|f| f.hold.max(1)).sum()
    }

    /// Which frame is visible at a given tick (wraps around).
    pub fn frame_at_tick(&self, tick: u32) -> usize {
        let total = self.total_ticks().max(1);
        let mut t = tick % total;
        for (i, f) in self.frames.iter().enumerate() {
            let h = f.hold.max(1);
            if t < h {
                return i;
            }
            t -= h;
        }
        0
    }

    /// First tick at which frame `index` appears.
    pub fn tick_of_frame(&self, index: usize) -> u32 {
        self.frames.iter().take(index).map(|f| f.hold.max(1)).sum()
    }

    /// Whether any layer is a video (its pictures depend on frame timing).
    pub fn has_video(&self) -> bool {
        self.layers.iter().any(|l| l.props.video.is_some())
    }

    /// Distinct video pictures used by the layers.
    pub fn video_media(&self) -> Vec<std::sync::Arc<crate::video::VideoMedia>> {
        let mut out: Vec<std::sync::Arc<crate::video::VideoMedia>> = Vec::new();
        for l in &self.layers {
            if let Some(m) = l.props.video.as_ref().and_then(|v| v.data.as_ref()) {
                if !out.iter().any(|o| o.uid == m.uid) {
                    out.push(m.clone());
                }
            }
        }
        out
    }

    pub fn duration_secs(&self) -> f32 {
        self.total_ticks() as f32 / self.canvas.fps.max(1) as f32
    }
}

impl LayerProps {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            locked: false,
            alpha_lock: false,
            opacity: 1.0,
            blend: BlendMode::Normal,
            shared: false,
            onion_skin: true,
            reference: false,
            clip: false,
            video: None,
            folder: None,
        }
    }

    /// Whether drawing tools may change this layer.
    pub fn paintable(&self) -> bool {
        !self.locked && self.visible && self.video.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_map_ticks_to_frames() {
        let mut p = Project::new("t", CanvasSettings::default());
        let id = p.alloc_id();
        p.frames.push(Frame { hold: 2, ..Frame::new(id) });
        let _ = &p;
        let id = p.alloc_id();
        p.frames.push(Frame::new(id));
        // frames: [1 tick][2 ticks][1 tick]
        assert_eq!(p.total_ticks(), 4);
        let seq: Vec<usize> = (0..5).map(|t| p.frame_at_tick(t)).collect();
        assert_eq!(seq, vec![0, 1, 1, 2, 0]);
        assert_eq!(p.tick_of_frame(2), 3);
    }
}
