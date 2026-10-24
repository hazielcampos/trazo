//! Flattening layers (and onion skins) into displayable pixels.

use serde::{Deserialize, Serialize};

use crate::color::{blend_px, pack, BlendMode, Rgba};
use crate::document::{LayerId, Project};
use crate::geom::IRect;
use crate::raster::{TiledImage, TILE};

/// Onion skin ("papel cebolla"): faint copies of neighbouring frames.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OnionSkin {
    pub enabled: bool,
    pub before: u32,
    pub after: u32,
    /// Opacity of the closest ghost; farther ones fade out.
    pub opacity: f32,
    pub before_tint: Rgba,
    pub after_tint: Rgba,
    /// Draw ghosts in their own colors instead of tinting them.
    pub keep_colors: bool,
}

impl Default for OnionSkin {
    fn default() -> Self {
        Self {
            enabled: true,
            before: 1,
            after: 1,
            opacity: 0.35,
            before_tint: Rgba::new(0.9, 0.15, 0.2, 1.0),
            after_tint: Rgba::new(0.1, 0.65, 0.3, 1.0),
            keep_colors: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    pub background: bool,
    pub onion: Option<OnionSkin>,
    /// Leave video layers out (e.g. the paint bucket's "all layers" reference).
    pub hide_video: bool,
    /// Leave out video layers marked as reference only (exports).
    pub hide_reference: bool,
}

/// Composite one region of a frame into premultiplied RGBA8 (`rect` sized).
pub fn render_region(p: &Project, frame_index: usize, rect: IRect, opts: &RenderOptions, out: &mut Vec<u8>) {
    render_region_scaled(p, frame_index, rect, 1, opts, out);
}

/// Like [`render_region`] but only every `step`-th pixel in each direction
/// (a smaller picture for a canvas shown smaller than its size, e.g. while
/// playing on a phone). Returns the size of `out` in pixels.
pub fn render_region_scaled(p: &Project, frame_index: usize, rect: IRect, step: usize, opts: &RenderOptions, out: &mut Vec<u8>) -> (usize, usize) {
    let rect = rect.intersect(&IRect::from_size(p.canvas.width, p.canvas.height));
    let s = step.max(1);
    let (ow, oh) = ((rect.width().max(0) as usize).div_ceil(s), (rect.height().max(0) as usize).div_ceil(s));
    out.clear();
    out.resize(ow * oh * 4, 0);
    if ow == 0 || oh == 0 {
        return (ow, oh);
    }
    // Decode video pictures once here, not once per thread.
    if !opts.hide_video {
        warm_video(p, frame_index);
    }
    let cap = if cfg!(target_os = "android") { 4 } else { 8 };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(cap);
    // Work items: output rows covering 64 canvas rows (one row of tiles, so
    // no tile is decoded twice). Threads take the next item when done, so a
    // slow core doesn't hold everyone back.
    let rows = (TILE / s).max(1);
    let items = oh.div_ceil(rows);
    let render_item = |k: usize, dst: &mut [u8]| {
        let j0 = k * rows;
        let j1 = (j0 + rows).min(oh);
        let y0 = rect.y0 + (j0 * s) as i32;
        let y1 = (rect.y0 + (j1 * s) as i32).min(rect.y1);
        render_band(p, frame_index, IRect::new(rect.x0, y0, rect.x1, y1), s, ow, j1 - j0, opts, dst);
    };
    if threads <= 1 || ow * oh < 150_000 || items < 2 {
        for (k, dst) in out.chunks_mut(rows * ow * 4).enumerate() {
            render_item(k, dst);
        }
        return (ow, oh);
    }
    let chunks: Vec<std::sync::Mutex<&mut [u8]>> = out.chunks_mut(rows * ow * 4).map(std::sync::Mutex::new).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..threads.min(items) {
            sc.spawn(|| loop {
                let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(c) = chunks.get(k) else { break };
                let mut dst = c.lock().unwrap_or_else(|e| e.into_inner());
                render_item(k, &mut dst);
            });
        }
    });
    (ow, oh)
}

/// Decode (into the video cache) the pictures video layers show in this frame.
fn warm_video(p: &Project, frame_index: usize) {
    let tick = p.tick_of_frame(frame_index);
    for l in &p.layers {
        if let Some(v) = &l.props.video {
            if let (Some(media), Some(idx)) = (&v.data, v.picture_at(tick, p.canvas.fps)) {
                let _ = crate::video::picture(media, idx);
            }
        }
    }
}

/// Buffers reused by the rendering threads (no big allocation per frame).
static BAND_BUFFERS: std::sync::Mutex<Vec<Vec<[f32; 4]>>> = std::sync::Mutex::new(Vec::new());

/// Render `rect` sampled every `step` px into `out` (`ow`×`oh` pixels, RGBA8).
#[allow(clippy::too_many_arguments)]
fn render_band(p: &Project, frame_index: usize, rect: IRect, step: usize, ow: usize, oh: usize, opts: &RenderOptions, out: &mut [u8]) {
    let mut acc = BAND_BUFFERS.lock().unwrap_or_else(|e| e.into_inner()).pop().unwrap_or_default();
    acc.clear();
    acc.resize(ow * oh, [0.0; 4]);
    render_into(p, frame_index, Grid { rect, step, w: ow }, opts, &mut acc);
    for (o, px) in out.chunks_exact_mut(4).zip(acc.iter()) {
        o.copy_from_slice(&pack(*px));
    }
    let mut pool = BAND_BUFFERS.lock().unwrap_or_else(|e| e.into_inner());
    if pool.len() < 8 {
        pool.push(acc);
    }
}

/// The pixels being rendered: `rect` of the canvas, every `step` px; the
/// accumulator holds `w` samples per row.
#[derive(Clone, Copy)]
struct Grid {
    rect: IRect,
    step: usize,
    w: usize,
}

impl Grid {
    /// Sampled coordinates inside `[a, b)` along one axis starting at `origin`.
    fn samples(&self, origin: i32, a: i32, b: i32) -> std::iter::StepBy<std::ops::Range<i32>> {
        let s = self.step as i32;
        let first = origin + (a - origin + s - 1).div_euclid(s) * s;
        (first..b.max(first)).step_by(self.step)
    }

    fn index(&self, x: i32, y: i32) -> usize {
        ((y - self.rect.y0) as usize / self.step) * self.w + (x - self.rect.x0) as usize / self.step
    }
}

fn render_into(p: &Project, frame_index: usize, grid: Grid, opts: &RenderOptions, acc: &mut [[f32; 4]]) {

    if opts.background {
        let bg = p.canvas.background;
        let px = [bg.r * bg.a, bg.g * bg.a, bg.b * bg.a, bg.a];
        acc.fill(px);
    }

    if let Some(onion) = opts.onion.filter(|o| o.enabled) {
        // Each layer shows its own neighbouring drawings (layers have their
        // own timing), not simply the neighbouring frames.
        let before = (1..=onion.before).rev().map(|k| (-(k as i32), k, onion.before, onion.before_tint));
        let after = (1..=onion.after).map(|k| (k as i32, k, onion.after, onion.after_tint));
        for (step, k, n, tint) in before.chain(after) {
            let fade = 1.0 - (k - 1) as f32 / n.max(1) as f32;
            let tint = (!onion.keep_colors).then_some(tint);
            let cel_of = |l: LayerId| p.neighbor_drawing(frame_index, l, step).and_then(|s| p.frames[s].cel(l));
            composite_frame(p, &cel_of, grid, acc, onion.opacity * fade.max(0.3), tint, None);
        }
    }

    let video = (!opts.hide_video).then(|| VideoPass {
        tick: p.tick_of_frame(frame_index),
        next_tick: p.tick_of_frame(frame_index + 1),
        hide_reference: opts.hide_reference,
    });
    let cel_of = |l: LayerId| p.visible_cel(frame_index, l);
    composite_frame(p, &cel_of, grid, acc, 1.0, None, video);
}

/// Which pictures video layers show in this render.
#[derive(Clone, Copy)]
struct VideoPass {
    tick: u32,
    /// Where the next drawing starts (its picture is decoded ahead).
    next_tick: u32,
    hide_reference: bool,
}

/// Composite all visible layers of `frame` into `acc` (covering `rect`).
/// Video layers are drawn only with a `video` pass (never in onion ghosts).
fn composite_frame<'a>(p: &'a Project, cel_of: &dyn Fn(LayerId) -> Option<&'a TiledImage>, grid: Grid, acc: &mut [[f32; 4]], opacity: f32, tint: Option<Rgba>, video: Option<VideoPass>) {
    let rect = grid.rect;
    // Cel that clipping layers are masked by (the nearest non-clipped layer below).
    let mut base: Option<&TiledImage> = None;
    let mut base_visible = false;
    for layer in &p.layers {
        // Guides help while drawing but aren't part of the result.
        if layer.props.guide && video.is_some_and(|pass| pass.hide_reference) {
            continue;
        }
        if let Some(v) = &layer.props.video {
            if !layer.props.clip {
                base = None;
                base_visible = false;
            }
            let shown = layer.props.visible && layer.props.opacity > 0.0;
            if let Some(pass) = video.filter(|pass| shown && !(pass.hide_reference && v.reference)) {
                draw_video(p, v, pass, grid, acc, layer.props.opacity * opacity, layer.props.blend);
            }
            continue;
        }
        let own_cel = if layer.props.shared { p.shared_cels.get(&layer.id) } else { cel_of(layer.id) };
        if !layer.props.clip {
            base = own_cel;
            base_visible = layer.props.visible && layer.props.opacity > 0.0;
        }
        if !layer.props.visible || layer.props.opacity <= 0.0 {
            continue;
        }
        let clip_to = if layer.props.clip {
            if !base_visible {
                continue;
            }
            match base {
                Some(b) => Some(b),
                None => continue, // nothing below to clip to
            }
        } else {
            None
        };
        let ghost = opacity < 1.0 || tint.is_some();
        // Shared layers look the same in every frame: ghosting them only adds noise.
        if ghost && (layer.props.shared || !layer.props.onion_skin) {
            continue;
        }
        let Some(cel) = own_cel else { continue };
        let op = layer.props.opacity * opacity;
        // Ghosts always use normal blending so they stay readable.
        let mode = if tint.is_some() || opacity < 1.0 { BlendMode::Normal } else { layer.props.blend };
        let fast = mode == BlendMode::Normal && op >= 1.0 && clip_to.is_none();
        for (tx, ty) in cel.tiles_in_rect(&rect).collect::<Vec<_>>() {
            let Some(tile) = cel.tile_ref(cel.tile_index(tx, ty)) else { continue };
            // Read without opening: playing many frames keeps them packed.
            let tile = tile.px_cold();
            // The clip base has the same tile grid: its tile, read once.
            let base_px = clip_to.map(|b| b.tile_ref(b.tile_index(tx, ty)).as_ref().map(|t| t.px_cold()));
            if matches!(base_px, Some(None)) {
                continue; // nothing below here to clip to
            }
            let trect = IRect::new((tx * TILE) as i32, (ty * TILE) as i32, ((tx + 1) * TILE) as i32, ((ty + 1) * TILE) as i32);
            let r = trect.intersect(&rect);
            for y in grid.samples(rect.y0, r.y0, r.y1) {
                let trow = (y as usize % TILE) * TILE;
                for x in grid.samples(rect.x0, r.x0, r.x1) {
                    let o = (trow + x as usize % TILE) * 4;
                    let mut src = [tile[o], tile[o + 1], tile[o + 2], tile[o + 3]];
                    if src[3] == 0 {
                        continue;
                    }
                    if let Some(Some(b)) = &base_px {
                        let ba = b[o + 3] as u32;
                        if ba == 0 {
                            continue;
                        }
                        src = src.map(|c| ((c as u32 * ba + 127) / 255) as u8);
                    }
                    if let Some(t) = tint {
                        let a = src[3] as f32;
                        src = [(t.r * a) as u8, (t.g * a) as u8, (t.b * a) as u8, src[3]];
                    }
                    let ai = grid.index(x, y);
                    if fast && src[3] == 255 {
                        // Opaque pixel, normal blend, full opacity: plain copy.
                        acc[ai] = [src[0] as f32 / 255.0, src[1] as f32 / 255.0, src[2] as f32 / 255.0, 1.0];
                    } else {
                        acc[ai] = blend_px(acc[ai], src, op, mode);
                    }
                }
            }
        }
    }
}

/// Draw the picture a video layer shows in this render.
fn draw_video(p: &Project, v: &crate::video::VideoLayer, pass: VideoPass, grid: Grid, acc: &mut [[f32; 4]], op: f32, mode: BlendMode) {
    let rect = grid.rect;
    let Some(media) = &v.data else { return };
    let fps = p.canvas.fps;
    let Some(idx) = v.picture_at(pass.tick, fps) else { return };
    let Some(px) = crate::video::picture(media, idx) else { return };
    if let Some(next) = v.picture_at(pass.next_tick, fps).filter(|n| *n != idx) {
        crate::video::prefetch(media, next);
    }
    let dst = IRect::new(v.x, v.y, v.x + v.w as i32, v.y + v.h as i32).intersect(&rect);
    if dst.width() <= 0 || dst.height() <= 0 {
        return;
    }
    let (pw, ph) = (media.width as usize, media.height as usize);
    let same = (pw, ph) == (v.w as usize, v.h as usize);
    let (sx, sy) = (pw as f32 / v.w as f32, ph as f32 / v.h as f32);
    // Horizontal sample positions are the same for every row: compute once.
    let xs: Vec<i32> = grid.samples(rect.x0, dst.x0, dst.x1).collect();
    let cols: Vec<(usize, usize, f32)> = xs
        .iter()
        .map(|&x| {
            let f = (((x - v.x) as f32 + 0.5) * sx - 0.5).clamp(0.0, (pw - 1) as f32);
            let x0 = f as usize;
            (x0, (x0 + 1).min(pw - 1), f - x0 as f32)
        })
        .collect();
    let tint = v.tint;
    for y in grid.samples(rect.y0, dst.y0, dst.y1) {
        let Some(&first_x) = xs.first() else { break };
        let arow = grid.index(first_x, y);
        let fy = (((y - v.y) as f32 + 0.5) * sy - 0.5).clamp(0.0, (ph - 1) as f32);
        let (y0, ty) = (fy as usize, fy - (fy as usize) as f32);
        let y1 = (y0 + 1).min(ph - 1);
        let (r0, r1) = (&px[y0 * pw * 4..(y0 + 1) * pw * 4], &px[y1 * pw * 4..(y1 + 1) * pw * 4]);
        let srow = if same { (y - v.y) as usize * pw * 4 } else { 0 };
        for (k, &(x0, x1, tx)) in cols.iter().enumerate() {
            let src = if same {
                let o = srow + (xs[k] - v.x) as usize * 4;
                [px[o], px[o + 1], px[o + 2], px[o + 3]]
            } else {
                let mut s = [0u8; 4];
                for (c, o) in s.iter_mut().enumerate() {
                    let top = r0[x0 * 4 + c] as f32 * (1.0 - tx) + r0[x1 * 4 + c] as f32 * tx;
                    let bot = r1[x0 * 4 + c] as f32 * (1.0 - tx) + r1[x1 * 4 + c] as f32 * tx;
                    *o = (top * (1.0 - ty) + bot * ty + 0.5) as u8;
                }
                s
            };
            if src[3] == 0 {
                continue;
            }
            let src = tint.apply(src);
            let a = &mut acc[arow + k];
            if mode == BlendMode::Normal {
                // Premultiplied "over" with layer opacity, without the general blend.
                let k = op / 255.0;
                let inv = 1.0 - src[3] as f32 * k;
                *a = [src[0] as f32 * k + a[0] * inv, src[1] as f32 * k + a[1] * inv, src[2] as f32 * k + a[2] * inv, src[3] as f32 * k + a[3] * inv];
            } else {
                *a = blend_px(*a, src, op, mode);
            }
        }
    }
}

/// Full-canvas premultiplied RGBA8 for one frame, as exported (reference
/// videos left out).
pub fn render_frame(p: &Project, frame_index: usize, background: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let opts = RenderOptions { background, hide_reference: true, ..Default::default() };
    render_region(p, frame_index, IRect::from_size(p.canvas.width, p.canvas.height), &opts, &mut out);
    out
}

/// All visible drawing layers of a frame merged into one image (no
/// background, no video). Used as the reference for "fill using all layers".
pub fn flatten_frame(p: &Project, frame_index: usize) -> TiledImage {
    let (w, h) = (p.canvas.width, p.canvas.height);
    let mut data = Vec::new();
    let opts = RenderOptions { hide_video: true, ..Default::default() };
    render_region(p, frame_index, IRect::from_size(w, h), &opts, &mut data);
    let mut img = TiledImage::new(w, h);
    img.write_region(0, 0, w as usize, h as usize, &data);
    img
}

/// Small preview of a frame (premultiplied RGBA8), at most `max_w` wide.
/// Samples the layers directly instead of rendering the full canvas.
pub fn render_thumbnail(p: &Project, frame_index: usize, max_w: u32) -> (usize, usize, Vec<u8>) {
    let (cw, ch) = (p.canvas.width.max(1) as f32, p.canvas.height.max(1) as f32);
    let step = (cw / max_w.max(1) as f32).max(1.0);
    let (tw, th) = (((cw / step) as usize).max(1), ((ch / step) as usize).max(1));
    let bg = p.canvas.background;
    let bgp = [bg.r * bg.a, bg.g * bg.a, bg.b * bg.a, bg.a];
    // (source, opacity, blend, clip base)
    let mut layers = Vec::new();
    let mut base: Option<&TiledImage> = None;
    let tick = p.tick_of_frame(frame_index);
    for l in &p.layers {
        if let Some(v) = &l.props.video {
            if !l.props.clip {
                base = None;
            }
            let thumb = v.data.as_ref().and_then(|m| Some((m, v.picture_at(tick, p.canvas.fps)?)));
            if let (true, Some((m, idx))) = (l.props.visible && l.props.opacity > 0.0, thumb) {
                layers.push((ThumbSrc::Video(v, m, idx), l.props.opacity, l.props.blend, None));
            }
            continue;
        }
        let cel = p.visible_cel(frame_index, l.id);
        if !l.props.clip {
            base = if l.props.visible { cel } else { None };
        }
        if !l.props.visible || l.props.opacity <= 0.0 {
            continue;
        }
        let clip = if l.props.clip { if base.is_none() { continue } else { base } } else { None };
        if let Some(c) = cel {
            layers.push((ThumbSrc::Cel(c), l.props.opacity, l.props.blend, clip));
        }
    }
    let mut out = Vec::with_capacity(tw * th * 4);
    // 2×2 samples per output pixel for some anti-aliasing.
    let offs = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];
    for y in 0..th {
        for x in 0..tw {
            let mut sum = [0.0f32; 4];
            for (ox, oy) in offs {
                let (sx, sy) = (((x as f32 + ox) * step) as i32, ((y as f32 + oy) * step) as i32);
                let mut acc = bgp;
                for (src, op, mode, clip) in &layers {
                    let mut s = src.pixel(sx, sy);
                    if let Some(b) = clip {
                        let ba = b.pixel(sx, sy)[3] as u32;
                        s = s.map(|c| ((c as u32 * ba + 127) / 255) as u8);
                    }
                    if s[3] != 0 {
                        acc = blend_px(acc, s, *op, *mode);
                    }
                }
                for i in 0..4 {
                    sum[i] += acc[i] * 0.25;
                }
            }
            out.extend_from_slice(&pack(sum));
        }
    }
    (tw, th, out)
}

enum ThumbSrc<'a> {
    Cel(&'a TiledImage),
    Video(&'a crate::video::VideoLayer, &'a std::sync::Arc<crate::video::VideoMedia>, usize),
}

impl ThumbSrc<'_> {
    fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        match self {
            ThumbSrc::Cel(c) => c.pixel(x, y),
            ThumbSrc::Video(v, m, idx) => {
                let layer_tint = v.tint;
                let (tw, th) = (m.thumb_w as usize, m.thumb_h as usize);
                let Some(t) = m.thumbs.get(*idx) else { return [0; 4] };
                match v.map(x as f32 + 0.5, y as f32 + 0.5, tw as u32, th as u32) {
                    Some((u, v)) => {
                        let o = ((v as usize).min(th - 1) * tw + (u as usize).min(tw - 1)) * 4;
                        layer_tint.apply([t[o], t[o + 1], t[o + 2], t[o + 3]])
                    }
                    None => [0; 4],
                }
            }
        }
    }
}

/// Convert premultiplied RGBA8 to straight alpha in place (for file export).
pub fn unpremultiply(data: &mut [u8]) {
    for p in data.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// Convert straight RGBA8 to premultiplied in place.
pub fn premultiply(data: &mut [u8]) {
    for p in data.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CanvasSettings;

    /// A big canvas (threaded path) with a clipping layer over a base layer.
    fn big_project() -> Project {
        let (w, h) = (700u32, 500u32);
        let mut p = Project::new("t", CanvasSettings { width: w, height: h, ..Default::default() });
        let base = p.layers[0].id;
        for y in 50..450 {
            for x in 100..600 {
                p.frames[0].cel_mut(base, w, h).set_pixel(x, y, [0, 0, 200, 255]);
            }
        }
        let top = p.alloc_id();
        let mut props = crate::document::LayerProps::named("clip");
        props.clip = true;
        p.layers.push(crate::document::Layer { id: top, props });
        for y in 0..h as i32 {
            p.frames[0].cel_mut(top, w, h).set_pixel((y * 3) % w as i32, y, [255, 0, 0, 255]);
        }
        p
    }

    #[test]
    fn threaded_packed_and_scaled_renders_agree() {
        let p = big_project();
        let full = render_frame(&p, 0, true);
        // Same picture from small pieces (single-threaded path).
        let mut pieces = vec![0u8; full.len()];
        for y0 in (0..500).step_by(37) {
            let r = IRect::new(0, y0, 700, (y0 + 37).min(500));
            let mut out = Vec::new();
            render_region(&p, 0, r, &RenderOptions { background: true, hide_reference: true, ..Default::default() }, &mut out);
            pieces[y0 as usize * 700 * 4..][..out.len()].copy_from_slice(&out);
        }
        assert!(full == pieces, "threaded render differs from piecewise render");
        // Packing changes nothing, and rendering keeps tiles packed.
        for c in p.frames[0].cels.values() {
            c.pack_all();
        }
        assert!(render_frame(&p, 0, true) == full, "packed render differs");
        assert!(p.frames[0].cels.values().all(|c| c.tiles().all(|t| t.is_packed())), "rendering opened tiles");
        // Every second pixel of the full picture.
        let mut half = Vec::new();
        let (w2, h2) = render_region_scaled(&p, 0, IRect::new(0, 0, 700, 500), 2, &RenderOptions { background: true, hide_reference: true, ..Default::default() }, &mut half);
        assert_eq!((w2, h2), (350, 250));
        for (j, i) in [(0usize, 0usize), (40, 100), (100, 77), (249, 349)] {
            let a = &half[(j * w2 + i) * 4..][..4];
            let b = &full[((j * 2) * 700 + i * 2) * 4..][..4];
            assert_eq!(a, b, "sample ({i}, {j})");
        }
    }

    #[test]
    fn background_and_layers() {
        let mut p = Project::new("t", CanvasSettings { width: 70, height: 70, ..Default::default() });
        let lid = p.layers[0].id;
        p.frames[0].cel_mut(lid, 70, 70).set_pixel(65, 65, [255, 0, 0, 255]);
        let img = render_frame(&p, 0, true);
        let at = |x: usize, y: usize| &img[(y * 70 + x) * 4..][..4];
        assert_eq!(at(0, 0), &[255, 255, 255, 255]);
        assert_eq!(at(65, 65), &[255, 0, 0, 255]);
        p.layers[0].props.visible = false;
        let img = render_frame(&p, 0, false);
        assert_eq!(&img[(65 * 70 + 65) * 4..][..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn reference_video_is_drawn_but_not_exported() {
        use crate::document::{Layer, LayerProps};
        use crate::video::{VideoBuilder, VideoLayer};
        let mut p = Project::new("t", CanvasSettings { width: 8, height: 8, ..Default::default() });
        let mut b = VideoBuilder::new(8, 8, 12.0);
        b.push(8, 8, [0u8, 0, 255, 255].repeat(64)).unwrap();
        let mut props = LayerProps::named("v");
        props.video = Some(VideoLayer::new(std::sync::Arc::new(b.finish("v").unwrap()), 0, 8, 8));
        p.layers.insert(0, Layer { id: 99, props });
        let mut on_canvas = Vec::new();
        render_region(&p, 0, IRect::from_size(8, 8), &RenderOptions::default(), &mut on_canvas);
        assert!(on_canvas[2] > 200 && on_canvas[0] < 40, "shown while drawing: {:?}", &on_canvas[..4]);
        assert_eq!(&render_frame(&p, 0, false)[..4], &[0, 0, 0, 0], "left out of exports");
        p.layers[0].props.video.as_mut().unwrap().reference = false;
        assert!(render_frame(&p, 0, false)[2] > 200, "exported when not a reference");
    }

    #[test]
    fn premultiply_roundtrip() {
        let mut d = vec![200, 100, 50, 128];
        premultiply(&mut d);
        unpremultiply(&mut d);
        assert!((d[0] as i32 - 200).abs() <= 2 && (d[2] as i32 - 50).abs() <= 2);
    }

    #[test]
    fn clipping_layer_only_shows_over_base() {
        use crate::document::{Layer, LayerProps};
        let mut p = Project::new("t", CanvasSettings { width: 64, height: 64, ..Default::default() });
        let base = p.layers[0].id;
        p.frames[0].cel_mut(base, 64, 64).set_pixel(5, 5, [0, 0, 0, 255]);
        let id = p.alloc_id();
        let mut props = LayerProps::named("color");
        props.clip = true;
        p.layers.push(Layer { id, props });
        let c = p.frames[0].cel_mut(id, 64, 64);
        c.set_pixel(5, 5, [255, 0, 0, 255]);
        c.set_pixel(6, 6, [255, 0, 0, 255]);
        let img = render_frame(&p, 0, false);
        assert_eq!(&img[(5 * 64 + 5) * 4..][..4], &[255, 0, 0, 255]);
        assert_eq!(&img[(6 * 64 + 6) * 4..][..4], &[0, 0, 0, 0], "clipped away outside the base");
    }
}

/// Small preview of one drawing (premultiplied RGBA8 on white), `max_w` wide.
pub fn cel_thumbnail(cel: &TiledImage, max_w: u32) -> (usize, usize, Vec<u8>) {
    let (cw, ch) = (cel.width().max(1) as f32, cel.height().max(1) as f32);
    let step = (cw / max_w.max(1) as f32).max(1.0);
    let (tw, th) = (((cw / step) as usize).max(1), ((ch / step) as usize).max(1));
    let mut out = Vec::with_capacity(tw * th * 4);
    let offs = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];
    let mut cache = std::collections::HashMap::new();
    for y in 0..th {
        for x in 0..tw {
            let mut sum = [0.0f32; 4];
            for (ox, oy) in offs {
                let s = cel.pixel_cached(&mut cache, ((x as f32 + ox) * step) as i32, ((y as f32 + oy) * step) as i32);
                let acc = blend_px([1.0, 1.0, 1.0, 1.0], s, 1.0, BlendMode::Normal);
                for i in 0..4 {
                    sum[i] += acc[i] * 0.25;
                }
            }
            out.extend_from_slice(&pack(sum));
        }
    }
    (tw, th, out)
}
