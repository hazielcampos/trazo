//! Video layers: a clip of pictures that plays under the drawings, for
//! rotoscoping or as a background.
//!
//! The pictures are kept compressed (JPEG, or PNG when they have
//! transparency), about 20 times smaller than raw pixels, and decoded only
//! when shown. A small shared cache keeps the last decoded pictures and a
//! background thread decodes the next one ahead during playback.
//!
//! The clip is placed on the timeline clock (ticks): each drawing shows the
//! picture at the tick where that drawing starts. Pictures are sampled at the
//! project's fps on import, so with drawings "on ones" every picture is seen.
//!
//! A video layer is a reference by default: shown while drawing, left out of
//! exports.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{mpsc, Arc, Mutex, OnceLock, Weak};

use serde::{Deserialize, Serialize};

use crate::composite::premultiply;
use crate::io::{ProjectError, Result};

/// Most pictures one video layer may hold (5 minutes at 12 fps, 2 at 30).
pub const MAX_FRAMES: usize = 3600;
/// Longest side of the stored pictures: enough to trace, light on memory.
pub const MAX_SIDE: u32 = 1280;
/// Most memory the compressed pictures of one import may use.
pub const MAX_BYTES: usize = 400 * 1024 * 1024;
/// Width of the small pictures used for timeline thumbnails.
pub const THUMB_W: u32 = 64;
const JPEG_QUALITY: u8 = 80;
/// Memory for decoded pictures (shared by all video layers), and the fewest
/// pictures kept whatever their size.
const CACHE_BYTES: usize = 64 * 1024 * 1024;
const CACHE_MIN: usize = 3;

/// Compressed picture bytes, shared when a picture repeats.
pub type Bytes = Arc<[u8]>;

/// The pictures of an imported video (never changes once built, so it is
/// shared between undo steps, layer copies and autosaves).
pub struct VideoMedia {
    /// Unique id (random), also the folder name inside the project file.
    pub uid: u64,
    pub name: String,
    /// Size of the stored pictures.
    pub width: u32,
    pub height: u32,
    /// Pictures per second.
    pub fps: f32,
    /// Compressed pictures (JPEG or PNG).
    pub frames: Vec<Bytes>,
    /// Premultiplied RGBA8 thumbnails, `thumb_w`×`thumb_h` each.
    pub thumbs: Vec<Bytes>,
    pub thumb_w: u32,
    pub thumb_h: u32,
}

impl VideoMedia {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn duration_secs(&self) -> f32 {
        self.frames.len() as f32 / self.fps.max(0.001)
    }

    /// Memory used by the compressed pictures and thumbnails (repeats counted once).
    pub fn bytes(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        self.frames.iter().chain(&self.thumbs).filter(|b| seen.insert(b.as_ptr() as usize)).map(|b| b.len()).sum()
    }
}

impl std::fmt::Debug for VideoMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VideoMedia({} «{}» {}×{} {} fotos)", self.uid, self.name, self.width, self.height, self.frames.len())
    }
}

/// Color treatment so drawn lines stand out over busy footage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VideoTint {
    #[default]
    Color,
    Gray,
    Blue,
}

impl VideoTint {
    pub const ALL: [VideoTint; 3] = [VideoTint::Color, VideoTint::Gray, VideoTint::Blue];

    pub fn label(self) -> &'static str {
        match self {
            VideoTint::Color => "Color",
            VideoTint::Gray => "Gris",
            VideoTint::Blue => "Azul",
        }
    }

    /// Apply to a premultiplied pixel.
    #[inline]
    pub fn apply(self, p: [u8; 4]) -> [u8; 4] {
        match self {
            VideoTint::Color => p,
            VideoTint::Gray | VideoTint::Blue => {
                let l = ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8;
                if self == VideoTint::Gray {
                    [l, l, l, p[3]]
                } else {
                    // Dark → blue, light → white, like "non-photo blue" pencil.
                    let (a, l) = (p[3] as u32, l as u32);
                    let mix = |base: u32| ((base * (255 - l) + 255 * l) / 255 * a / 255) as u8;
                    [mix(60), mix(120), mix(230), p[3]]
                }
            }
        }
    }
}

fn yes() -> bool {
    true
}

/// How a video is placed on a layer. Lives in [`crate::LayerProps`], so undo,
/// layer copies and deletion carry it along like any other property.
#[derive(Clone, Serialize, Deserialize)]
pub struct VideoLayer {
    /// [`VideoMedia::uid`] of the pictures.
    pub media: u64,
    /// Timeline tick where the first used picture appears (may be negative).
    pub start: i64,
    /// Used pictures: `trim_start..trim_end`.
    pub trim_start: u32,
    pub trim_end: u32,
    /// Where the pictures are drawn on the canvas (scaled to this size).
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// Only a guide: shown while drawing, left out of exports.
    #[serde(default = "yes")]
    pub reference: bool,
    #[serde(default)]
    pub tint: VideoTint,
    /// The pictures; `None` until loaded (or if they were lost).
    #[serde(skip)]
    pub data: Option<Arc<VideoMedia>>,
}

impl PartialEq for VideoLayer {
    fn eq(&self, o: &Self) -> bool {
        // Media never changes, so its id identifies it.
        (self.media, self.start, self.trim_start, self.trim_end, self.x, self.y, self.w, self.h, self.reference, self.tint)
            == (o.media, o.start, o.trim_start, o.trim_end, o.x, o.y, o.w, o.h, o.reference, o.tint)
    }
}

impl std::fmt::Debug for VideoLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VideoLayer(media {} desde {} [{}..{}])", self.media, self.start, self.trim_start, self.trim_end)
    }
}

impl VideoLayer {
    /// Place `media` fitted and centered on a `cw`×`ch` canvas, starting at `start`.
    pub fn new(media: Arc<VideoMedia>, start: i64, cw: u32, ch: u32) -> Self {
        let f = (cw as f32 / media.width.max(1) as f32).min(ch as f32 / media.height.max(1) as f32);
        let (w, h) = (((media.width as f32 * f).round() as u32).max(1), ((media.height as f32 * f).round() as u32).max(1));
        Self {
            media: media.uid,
            start,
            trim_start: 0,
            trim_end: media.len() as u32,
            x: (cw as i32 - w as i32) / 2,
            y: (ch as i32 - h as i32) / 2,
            w,
            h,
            reference: true,
            tint: VideoTint::Color,
            data: Some(media),
        }
    }

    /// Scale around the center, keeping the aspect ratio of the pictures.
    pub fn set_scale(&mut self, scale: f32) {
        let Some(m) = &self.data else { return };
        let (cx, cy) = (self.x as f32 + self.w as f32 / 2.0, self.y as f32 + self.h as f32 / 2.0);
        let s = scale.clamp(0.05, 20.0);
        self.w = ((m.width as f32 * s).round() as u32).max(1);
        self.h = ((m.height as f32 * s).round() as u32).max(1);
        self.x = (cx - self.w as f32 / 2.0).round() as i32;
        self.y = (cy - self.h as f32 / 2.0).round() as i32;
    }

    /// Current scale relative to the stored pictures.
    pub fn scale(&self) -> f32 {
        self.data.as_ref().map_or(1.0, |m| self.w as f32 / m.width.max(1) as f32)
    }

    /// Picture shown at timeline `tick` (None before the start or after the end).
    pub fn picture_at(&self, tick: u32, project_fps: u32) -> Option<usize> {
        let media = self.data.as_ref()?;
        let local = tick as i64 - self.start;
        if local < 0 {
            return None;
        }
        let secs = local as f64 / project_fps.max(1) as f64;
        let idx = self.trim_start as usize + (secs * media.fps as f64 + 1e-6).floor() as usize;
        (idx < (self.trim_end as usize).min(media.len())).then_some(idx)
    }

    /// Timeline length of the used part, in ticks.
    pub fn len_ticks(&self, project_fps: u32) -> i64 {
        let Some(media) = &self.data else { return 0 };
        let n = self.trim_end.saturating_sub(self.trim_start) as f64;
        (n / media.fps.max(0.001) as f64 * project_fps.max(1) as f64).ceil() as i64
    }

    /// Tick just after the last picture.
    pub fn end_tick(&self, project_fps: u32) -> i64 {
        self.start + self.len_ticks(project_fps)
    }

    /// Keep trims inside the media and the size sane.
    pub fn clamp(&mut self) {
        let n = self.data.as_ref().map_or(self.trim_end, |m| m.len() as u32);
        self.trim_end = self.trim_end.min(n);
        self.trim_start = self.trim_start.min(self.trim_end.saturating_sub(1));
        self.w = self.w.max(1);
        self.h = self.h.max(1);
    }

    /// Canvas pixel → picture pixel (None outside the placed rectangle).
    pub fn map(&self, cx: f32, cy: f32, pw: u32, ph: u32) -> Option<(f32, f32)> {
        let (u, v) = ((cx - self.x as f32) / self.w as f32, (cy - self.y as f32) / self.h as f32);
        (0.0..1.0).contains(&u).then_some(())?;
        (0.0..1.0).contains(&v).then_some(())?;
        Some((u * pw as f32, v * ph as f32))
    }
}

// ---- building (import) ------------------------------------------------------

/// Collects pictures for a new [`VideoMedia`]: scales them to fit the canvas
/// (never enlarges), compresses them and makes thumbnails.
pub struct VideoBuilder {
    max_w: u32,
    max_h: u32,
    fps: f32,
    size: Option<(u32, u32)>,
    frames: Vec<Bytes>,
    thumbs: Vec<Bytes>,
    thumb: (u32, u32),
    bytes: usize,
}

impl VideoBuilder {
    pub fn new(canvas_w: u32, canvas_h: u32, fps: f32) -> Self {
        Self { max_w: canvas_w.min(MAX_SIDE), max_h: canvas_h.min(MAX_SIDE), fps, size: None, frames: Vec::new(), thumbs: Vec::new(), thumb: (1, 1), bytes: 0 }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.frames.len() >= MAX_FRAMES || self.bytes >= MAX_BYTES
    }

    fn full_error(&self) -> ProjectError {
        if self.bytes >= MAX_BYTES {
            ProjectError::Invalid(format!("el video ocupa más de {} MB: importa un tramo más corto", MAX_BYTES >> 20))
        } else {
            ProjectError::Invalid(format!("el video tiene más de {MAX_FRAMES} fotogramas: importa un tramo más corto"))
        }
    }

    /// Compressed bytes so far.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Add a picture (straight RGBA8, any size).
    pub fn push(&mut self, w: u32, h: u32, rgba: Vec<u8>) -> Result<()> {
        if self.is_full() {
            return Err(self.full_error());
        }
        if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
            return Err(ProjectError::Invalid("fotograma de video vacío".into()));
        }
        // Every picture gets the size of the first one.
        let (tw, th) = *self.size.get_or_insert_with(|| {
            let f = (self.max_w as f32 / w as f32).min(self.max_h as f32 / h as f32).min(1.0);
            (((w as f32 * f).round() as u32).max(2), ((h as f32 * f).round() as u32).max(2))
        });
        let img = image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| ProjectError::Invalid("fotograma de video dañado".into()))?;
        let img = if (w, h) == (tw, th) { img } else { image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle) };
        let (thw, thh) = (THUMB_W.min(tw), ((th as f32 * THUMB_W.min(tw) as f32 / tw as f32).round() as u32).max(1));
        self.thumb = (thw, thh);
        let mut small = image::imageops::resize(&img, thw, thh, image::imageops::FilterType::Triangle).into_raw();
        premultiply(&mut small);
        let encoded = encode_picture(tw, th, img.as_raw())?;
        self.bytes += encoded.len() + small.len();
        self.frames.push(encoded.into());
        self.thumbs.push(small.into());
        Ok(())
    }

    /// Repeat the last picture (the source had no new picture for this slot).
    pub fn repeat_last(&mut self) -> Result<()> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(self.full_error());
        }
        // Shares the bytes of the previous picture: costs nothing.
        let (Some(f), Some(t)) = (self.frames.last().cloned(), self.thumbs.last().cloned()) else { return Ok(()) };
        self.frames.push(f);
        self.thumbs.push(t);
        Ok(())
    }

    pub fn finish(self, name: impl Into<String>) -> Result<VideoMedia> {
        let Some((width, height)) = self.size else { return Err(ProjectError::Invalid("el video no tiene imágenes".into())) };
        Ok(VideoMedia {
            uid: new_uid(),
            name: name.into(),
            width,
            height,
            fps: self.fps,
            frames: self.frames,
            thumbs: self.thumbs,
            thumb_w: self.thumb.0,
            thumb_h: self.thumb.1,
        })
    }
}

/// Random id that won't clash with media of other projects.
pub fn new_uid() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    if let Ok(t) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        h.write_u128(t.as_nanos());
    }
    h.finish() >> 11 // fits exactly in a JSON number (f64)
}

/// JPEG for opaque pictures, PNG when there is transparency.
fn encode_picture(w: u32, h: u32, straight_rgba: &[u8]) -> Result<Vec<u8>> {
    let opaque = straight_rgba.chunks_exact(4).all(|p| p[3] == 255);
    if !opaque {
        return crate::io::png_bytes(w, h, straight_rgba);
    }
    let rgb: Vec<u8> = straight_rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode(&rgb, w, h, image::ExtendedColorType::Rgb8)
        .map_err(|e| ProjectError::Png(e.to_string()))?;
    Ok(out)
}

/// Decode one stored picture to premultiplied RGBA8.
fn decode_picture(media: &VideoMedia, idx: usize) -> Option<Vec<u8>> {
    let bytes = media.frames.get(idx)?;
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    if img.dimensions() != (media.width, media.height) {
        return None;
    }
    let mut px = img.into_raw();
    premultiply(&mut px);
    Some(px)
}

// ---- decoded picture cache ----------------------------------------------------

type Entry = (u64, usize, Arc<Vec<u8>>);

fn cache() -> &'static Mutex<VecDeque<Entry>> {
    static CACHE: OnceLock<Mutex<VecDeque<Entry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn cached(uid: u64, idx: usize) -> Option<Arc<Vec<u8>>> {
    let mut c = cache().lock().ok()?;
    let pos = c.iter().position(|(u, i, _)| *u == uid && *i == idx)?;
    let e = c.remove(pos)?;
    let px = e.2.clone();
    c.push_back(e); // most recently used last
    Some(px)
}

fn store(uid: u64, idx: usize, px: Arc<Vec<u8>>) {
    if let Ok(mut c) = cache().lock() {
        if c.iter().any(|(u, i, _)| *u == uid && *i == idx) {
            return;
        }
        c.push_back((uid, idx, px));
        while c.len() > CACHE_MIN && c.iter().map(|e| e.2.len()).sum::<usize>() > CACHE_BYTES {
            c.pop_front();
        }
    }
}

/// Free the decoded pictures (e.g. when another project is opened).
pub fn clear_cache() {
    if let Ok(mut c) = cache().lock() {
        c.clear();
    }
}

/// Premultiplied RGBA8 of picture `idx` (`width`×`height`), decoded on demand.
pub fn picture(media: &Arc<VideoMedia>, idx: usize) -> Option<Arc<Vec<u8>>> {
    if let Some(px) = cached(media.uid, idx) {
        return Some(px);
    }
    let px = Arc::new(decode_picture(media, idx)?);
    store(media.uid, idx, px.clone());
    Some(px)
}

/// Decode picture `idx` in the background so it is ready when shown.
pub fn prefetch(media: &Arc<VideoMedia>, idx: usize) {
    if idx >= media.len() || cached(media.uid, idx).is_some() {
        return;
    }
    static WORKER: OnceLock<Mutex<mpsc::Sender<(Weak<VideoMedia>, usize)>>> = OnceLock::new();
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(Weak<VideoMedia>, usize)>();
        let _ = std::thread::Builder::new().name("video-prefetch".into()).spawn(move || {
            while let Ok(mut job) = rx.recv() {
                // Only the newest request matters (playback moved on).
                while let Ok(newer) = rx.try_recv() {
                    job = newer;
                }
                let Some(media) = job.0.upgrade() else { continue };
                if cached(media.uid, job.1).is_none() {
                    if let Some(px) = decode_picture(&media, job.1) {
                        store(media.uid, job.1, Arc::new(px));
                    }
                }
            }
        });
        Mutex::new(tx)
    });
    if let Ok(tx) = tx.lock() {
        let _ = tx.send((Arc::downgrade(media), idx));
    }
}

// ---- animated GIF -------------------------------------------------------------

/// Decode an animated GIF into pictures sampled at `fps`, pushed into `out`.
/// `keep_going` gets the progress (0..1) and returns false to stop.
pub fn decode_gif(bytes: &[u8], fps: f32, out: &mut VideoBuilder, keep_going: &mut dyn FnMut(f32) -> bool) -> Result<()> {
    use image::AnimationDecoder;
    let bad = |e: &dyn std::fmt::Display| ProjectError::Invalid(format!("GIF no válido: {e}"));
    let dec = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| bad(&e))?;
    {
        use image::ImageDecoder;
        let (w, h) = dec.dimensions();
        if w == 0 || h == 0 || w > crate::io::MAX_IMPORT_SIDE || h > crate::io::MAX_IMPORT_SIDE {
            return Err(bad(&format!("{w}×{h}")));
        }
    }
    // Source frames with their start time; sampled as they arrive.
    let mut t = 0.0f64; // start of the current source frame
    let mut slot = 0usize; // next output picture
    let step = 1.0 / fps.max(1.0) as f64;
    for frame in dec.into_frames() {
        let frame = frame.map_err(|e| bad(&e))?;
        let (num, den) = frame.delay().numer_denom_ms();
        // Browsers show 0–10 ms delays as 100 ms.
        let mut d = num as f64 / den.max(1) as f64 / 1000.0;
        if d <= 0.011 {
            d = 0.1;
        }
        let end = t + d;
        let buf = frame.into_buffer();
        let (w, h) = buf.dimensions();
        let raw = buf.into_raw();
        let mut first = true;
        while (slot as f64) * step < end - 1e-9 {
            if first {
                out.push(w, h, raw.clone())?;
                first = false;
            } else {
                out.repeat_last()?;
            }
            slot += 1;
            if out.is_full() {
                return Ok(());
            }
        }
        t = end;
        if !keep_going((out.len() % 100) as f32 / 100.0) {
            return Err(ProjectError::Invalid("cancelado".into()));
        }
    }
    if out.is_empty() {
        return Err(bad(&"sin imágenes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(n: usize, fps: f32) -> Arc<VideoMedia> {
        let mut b = VideoBuilder::new(64, 64, fps);
        for i in 0..n {
            let px: Vec<u8> = (0..32 * 16).flat_map(|_| [(i * 40) as u8, 10, 200, 255]).collect();
            b.push(32, 16, px).unwrap();
        }
        Arc::new(b.finish("prueba").unwrap())
    }

    #[test]
    fn timing_follows_ticks_and_trims() {
        let m = media(6, 6.0);
        let mut v = VideoLayer::new(m, 2, 64, 64);
        // Fitted and centered: 32×16 → 64×32 at y = 16.
        assert_eq!((v.x, v.y, v.w, v.h), (0, 16, 64, 32));
        // Project at 12 fps: each picture lasts two ticks.
        assert_eq!(v.picture_at(1, 12), None);
        assert_eq!(v.picture_at(2, 12), Some(0));
        assert_eq!(v.picture_at(4, 12), Some(1));
        assert_eq!(v.picture_at(13, 12), Some(5));
        assert_eq!(v.picture_at(14, 12), None);
        assert_eq!(v.end_tick(12), 14);
        v.trim_start = 2;
        v.trim_end = 4;
        assert_eq!(v.picture_at(2, 12), Some(2));
        assert_eq!(v.picture_at(6, 12), None);
    }

    #[test]
    fn tints_keep_alpha_and_brightness() {
        assert_eq!(VideoTint::Gray.apply([255, 0, 0, 255])[3], 255);
        let g = VideoTint::Gray.apply([200, 200, 200, 255]);
        assert!(g[0] == g[1] && g[1] == g[2] && (g[0] as i32 - 200).abs() <= 2);
        let b = VideoTint::Blue.apply([0, 0, 0, 255]);
        assert!(b[2] > b[0], "dark pixels turn blue: {b:?}");
        assert_eq!(VideoTint::Blue.apply([255, 255, 255, 255]), [255, 255, 255, 255]);
    }

    #[test]
    fn big_pictures_are_capped() {
        let mut b = VideoBuilder::new(3840, 2160, 12.0);
        b.push(3840, 2160, vec![9; 3840 * 2160 * 4]).unwrap();
        let m = b.finish("4k").unwrap();
        assert_eq!((m.width, m.height), (1280, 720));
        let mut v = VideoLayer::new(Arc::new(m), 0, 3840, 2160);
        assert_eq!((v.w, v.h), (3840, 2160), "still shown over the whole canvas");
        v.set_scale(1.0);
        assert_eq!((v.w, v.h, v.x, v.y), (1280, 720, 1280, 720), "scaled around the center");
    }

    #[test]
    fn pictures_decode_and_cache() {
        let m = media(3, 12.0);
        assert_eq!((m.width, m.height), (32, 16), "never enlarged");
        let px = picture(&m, 2).unwrap();
        assert_eq!(px.len(), 32 * 16 * 4);
        assert!((px[0] as i32 - 80).abs() < 6 && px[3] == 255, "{:?}", &px[..4]);
        assert!(Arc::ptr_eq(&px, &picture(&m, 2).unwrap()), "second call comes from the cache");
        assert_eq!(m.thumbs[0].len(), (m.thumb_w * m.thumb_h * 4) as usize);
    }

    #[test]
    fn gif_frames_are_resampled_by_delay() {
        use image::codecs::gif::GifEncoder;
        use image::{Delay, Frame, RgbaImage};
        let mut bytes = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut bytes);
            for (i, ms) in [(0u8, 250u32), (1, 500)] {
                let img = RgbaImage::from_pixel(8, 8, image::Rgba([i * 200, 0, 0, 255]));
                enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(ms, 1))).unwrap();
            }
        }
        let mut b = VideoBuilder::new(100, 100, 12.0);
        decode_gif(&bytes, 12.0, &mut b, &mut |_| true).unwrap();
        // 0.75 s at 12 fps = 9 pictures: 3 of the first frame, 6 of the second.
        assert_eq!(b.len(), 9);
        let m = Arc::new(b.finish("gif").unwrap());
        assert_eq!(m.frames[0], m.frames[2]);
        assert!(Arc::ptr_eq(&m.frames[0], &m.frames[2]), "repeats share their bytes");
        assert_ne!(m.frames[2], m.frames[3]);
    }
}
