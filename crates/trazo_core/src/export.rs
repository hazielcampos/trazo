//! Exporting animations: GIF, PNG (sequence or single frame), AVI (MJPEG +
//! PCM), WAV, plus the shared planning used by every format (including the
//! platform MP4 encoders in the app).
//!
//! Everything here is deterministic and offline. Frames come from the
//! project, audio is mixed block by block with [`AudioTimeline::mix`], and
//! timing uses the same tick ↔ sample conversion as playback, so exports stay
//! in sync.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::audio::{ticks_to_samples, CHANNELS, SAMPLE_RATE};
use crate::color::Rgba;
use crate::composite::{render_frame, unpremultiply};
use crate::document::Project;
use crate::io::{ProjectError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// H.264 + AAC (done by the platform encoder in the app).
    Mp4,
    Gif,
    PngSequence,
    /// The current frame as one PNG image.
    Png,
    Avi,
    Wav,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Mp4 => "Video MP4",
            ExportFormat::Gif => "GIF animado",
            ExportFormat::PngSequence => "Secuencia PNG (zip)",
            ExportFormat::Png => "Fotograma actual (PNG)",
            ExportFormat::Avi => "Video AVI (MJPEG)",
            ExportFormat::Wav => "Solo audio (WAV)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Mp4 => "mp4",
            ExportFormat::Gif => "gif",
            ExportFormat::PngSequence => "zip",
            ExportFormat::Png => "png",
            ExportFormat::Avi => "avi",
            ExportFormat::Wav => "wav",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            ExportFormat::Mp4 => "video/mp4",
            ExportFormat::Gif => "image/gif",
            ExportFormat::PngSequence => "application/zip",
            ExportFormat::Png => "image/png",
            ExportFormat::Avi => "video/x-msvideo",
            ExportFormat::Wav => "audio/wav",
        }
    }

    pub fn has_video(self) -> bool {
        self != ExportFormat::Wav
    }

    /// Formats that can keep a transparent background.
    pub fn supports_alpha(self) -> bool {
        matches!(self, ExportFormat::Gif | ExportFormat::PngSequence | ExportFormat::Png)
    }

    /// Formats where "repetitions" and "output fps" make sense.
    pub fn is_timed_video(self) -> bool {
        matches!(self, ExportFormat::Mp4 | ExportFormat::Avi | ExportFormat::PngSequence)
    }
}

/// Output frame shape (for social networks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aspect {
    Canvas,
    /// 9:16 — TikTok, Reels, Shorts, stories.
    Vertical,
    /// 4:5 — Instagram feed.
    Portrait,
    Square,
    /// 16:9 — YouTube.
    Wide,
}

impl Aspect {
    pub const ALL: [Aspect; 5] = [Aspect::Canvas, Aspect::Vertical, Aspect::Portrait, Aspect::Square, Aspect::Wide];

    pub fn label(self) -> &'static str {
        match self {
            Aspect::Canvas => "Como el lienzo",
            Aspect::Vertical => "9:16 vertical",
            Aspect::Portrait => "4:5",
            Aspect::Square => "1:1 cuadrado",
            Aspect::Wide => "16:9",
        }
    }

    fn ratio(self, cw: u32, ch: u32) -> f32 {
        match self {
            Aspect::Canvas => cw as f32 / ch.max(1) as f32,
            Aspect::Vertical => 9.0 / 16.0,
            Aspect::Portrait => 4.0 / 5.0,
            Aspect::Square => 1.0,
            Aspect::Wide => 16.0 / 9.0,
        }
    }
}

/// How the canvas goes into a frame of another shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fit {
    /// Whole drawing visible, bars filled with `pad_color`.
    Pad,
    /// Fill the frame, cutting the edges.
    Crop,
}

/// Output size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OutSize {
    /// Fraction of the canvas size (0.1..=1).
    Percent(f32),
    /// Fixed short side in pixels (1080p, 720p, 480p…).
    Short(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub format: ExportFormat,
    pub size: OutSize,
    pub aspect: Aspect,
    pub fit: Fit,
    /// Color of the bars (and of transparent areas in formats without alpha).
    pub pad_color: Rgba,
    /// Output frame rate (None = the project's fps).
    pub out_fps: Option<u32>,
    /// Frames `a..=b` only (None = everything).
    pub range: Option<(usize, usize)>,
    /// Paint the canvas background (off = transparent where possible).
    pub background: bool,
    /// How many times the animation is repeated in the file.
    pub loops: u32,
    pub include_audio: bool,
    /// 1..=100, used by lossy formats.
    pub quality: u8,
    /// Frame exported by [`ExportFormat::Png`].
    pub frame: usize,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: ExportFormat::Mp4,
            size: OutSize::Percent(1.0),
            aspect: Aspect::Canvas,
            fit: Fit::Pad,
            pad_color: Rgba::WHITE,
            out_fps: None,
            range: None,
            background: true,
            loops: 1,
            include_audio: true,
            quality: 85,
            frame: 0,
        }
    }
}

/// Where the (scaled) canvas lands inside the output frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub scale: f32,
    /// Top-left of the scaled canvas in output pixels (negative = cropped).
    pub x: i32,
    pub y: i32,
    pub content_w: u32,
    pub content_h: u32,
}

/// What to render: which drawing is shown on each output frame.
#[derive(Debug, Clone)]
pub struct VideoPlan {
    pub width: u32,
    pub height: u32,
    /// Output frames per second.
    pub fps: u32,
    /// (frame index, number of output frames it stays on screen), loops included.
    pub spans: Vec<(usize, u32)>,
    /// Same spans in project ticks (for GIF, which keeps exact timing).
    pub tick_spans: Vec<(usize, u32)>,
    pub project_fps: u32,
    /// Audio comes from this sample range of the timeline (one loop).
    pub audio_start: i64,
    pub audio_end: i64,
    pub layout: Layout,
    pub pad: Rgba,
    pub has_audio: bool,
}

impl VideoPlan {
    pub fn total_frames(&self) -> u64 {
        self.spans.iter().map(|s| s.1 as u64).sum()
    }

    pub fn duration_secs(&self) -> f64 {
        self.total_frames() as f64 / self.fps.max(1) as f64
    }

    /// Audio sample frames matching the video length exactly.
    pub fn audio_frames(&self) -> u64 {
        (self.total_frames() as f64 * SAMPLE_RATE as f64 / self.fps.max(1) as f64).round() as u64
    }

    /// Audio sample frame where output frame `k` starts.
    pub fn sample_of_frame(&self, k: u64) -> u64 {
        (k as f64 * SAMPLE_RATE as f64 / self.fps.max(1) as f64).round() as u64
    }
}

fn round_to(v: f32, m: u32) -> u32 {
    (((v / m as f32).round() as u32).max(1)) * m
}

pub fn plan(p: &Project, s: &ExportSettings) -> VideoPlan {
    let last = p.frames.len().saturating_sub(1);
    let (a, b) = if s.format == ExportFormat::Png {
        let f = s.frame.min(last);
        (f, f)
    } else {
        s.range.map(|(a, b)| (a.min(last), b.min(last).max(a.min(last)))).unwrap_or((0, last))
    };
    let (cw, ch) = (p.canvas.width.max(1), p.canvas.height.max(1));
    let ratio = s.aspect.ratio(cw, ch);
    // Output box from the size setting and the aspect ratio.
    let short = match s.size {
        OutSize::Percent(f) => cw.min(ch) as f32 * f.clamp(0.05, 1.0),
        OutSize::Short(px) => px.clamp(16, 4320) as f32,
    };
    let (mut w, mut h) = if ratio >= 1.0 { (short * ratio, short) } else { (short, short / ratio) };
    if s.aspect == Aspect::Canvas {
        if let OutSize::Percent(f) = s.size {
            let f = f.clamp(0.05, 1.0);
            (w, h) = (cw as f32 * f, ch as f32 * f);
        }
    }
    // Video encoders want even sizes; H.264 encoders accept multiples of 8
    // (1080 wide is standard), the encoder pads internally to 16.
    let m = if s.format == ExportFormat::Mp4 { 8 } else { 2 };
    let (width, height) = if s.format.supports_alpha() && s.format != ExportFormat::Gif {
        ((w.round() as u32).max(1), (h.round() as u32).max(1))
    } else {
        (round_to(w, m), round_to(h, m))
    };
    let fit_scale = (width as f32 / cw as f32).min(height as f32 / ch as f32);
    let fill_scale = (width as f32 / cw as f32).max(height as f32 / ch as f32);
    let scale = if s.fit == Fit::Pad { fit_scale } else { fill_scale };
    let (content_w, content_h) = (((cw as f32 * scale).round() as u32).max(1), ((ch as f32 * scale).round() as u32).max(1));
    let layout = Layout { scale, x: (width as i32 - content_w as i32) / 2, y: (height as i32 - content_h as i32) / 2, content_w, content_h };

    let project_fps = p.canvas.fps.max(1);
    let fps = if s.format.is_timed_video() { s.out_fps.unwrap_or(project_fps).clamp(1, 120) } else { project_fps };
    let loops = if s.format.is_timed_video() { s.loops.clamp(1, 100) } else { 1 };
    let one: Vec<(usize, u32)> = (a..=b).map(|i| (i, p.frames[i].hold.max(1))).collect();
    let mut tick_spans = Vec::new();
    for _ in 0..loops {
        tick_spans.extend_from_slice(&one);
    }
    // Convert tick spans to output frames by exact time (no drift).
    let mut spans = Vec::with_capacity(tick_spans.len());
    let (mut ticks_done, mut out_done) = (0u64, 0u64);
    for &(f, t) in &tick_spans {
        ticks_done += t as u64;
        let end = (ticks_done as f64 * fps as f64 / project_fps as f64).round() as u64;
        let n = end.saturating_sub(out_done) as u32;
        out_done = end;
        if n > 0 {
            spans.push((f, n));
        }
    }
    if spans.is_empty() {
        spans.push((a, 1));
    }
    let audio_start = ticks_to_samples(p.tick_of_frame(a) as f64, project_fps);
    let audio_end = ticks_to_samples((p.tick_of_frame(b) + p.frames[b].hold.max(1)) as f64, project_fps);
    let pad = if s.fit == Fit::Pad || !s.format.supports_alpha() { s.pad_color } else { Rgba::TRANSPARENT };
    VideoPlan { width, height, fps, spans, tick_spans, project_fps, audio_start, audio_end, layout, pad, has_audio: !p.audio.is_empty() }
}

/// Straight (non-premultiplied) RGBA8 of one frame at the plan's size.
/// Scaling happens in premultiplied space so transparent edges don't get halos.
pub fn render_rgba(p: &Project, frame: usize, plan: &VideoPlan, background: bool) -> Vec<u8> {
    let (cw, ch) = (p.canvas.width, p.canvas.height);
    let full = render_frame(p, frame, background);
    // Through the camera, when there is one.
    let cam = &p.canvas.camera;
    let full = if cam.is_active() {
        let fill = if background { p.canvas.background.premul8(1.0).map(|v| v as u8) } else { [0; 4] };
        crate::camera::view(&full, cw, ch, &cam.pose_at(frame, p.frames.len()), fill)
    } else {
        full
    };
    let l = plan.layout;
    let scaled = if l.content_w == cw && l.content_h == ch {
        full
    } else {
        match image::RgbaImage::from_raw(cw, ch, full) {
            Some(img) => {
                // Whole-number enlargements stay crisp (pixel art).
                let filter = if l.scale >= 1.0 && (l.scale - l.scale.round()).abs() < 1e-3 {
                    image::imageops::FilterType::Nearest
                } else {
                    image::imageops::FilterType::Triangle
                };
                image::imageops::resize(&img, l.content_w, l.content_h, filter).into_raw()
            }
            None => vec![0; (l.content_w * l.content_h * 4) as usize],
        }
    };
    let pad = plan.pad.premul8(1.0).map(|v| v as u8);
    let (w, h) = (plan.width as i32, plan.height as i32);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..w * h {
        out.extend_from_slice(&pad);
    }
    // Paste the scaled canvas (premultiplied "over" the pad color).
    for y in 0..l.content_h as i32 {
        let oy = y + l.y;
        if oy < 0 || oy >= h {
            continue;
        }
        for x in 0..l.content_w as i32 {
            let ox = x + l.x;
            if ox < 0 || ox >= w {
                continue;
            }
            let si = ((y * l.content_w as i32 + x) * 4) as usize;
            let di = ((oy * w + ox) * 4) as usize;
            let sa = scaled[si + 3] as u32;
            for c in 0..4 {
                out[di + c] = (scaled[si + c] as u32 + out[di + c] as u32 * (255 - sa) / 255).min(255) as u8;
            }
        }
    }
    unpremultiply(&mut out);
    out
}

/// Renders frames once per drawing (holds and loops reuse them), with a
/// memory budget.
pub struct FrameCache<'a> {
    project: &'a Project,
    plan: &'a VideoPlan,
    background: bool,
    map: HashMap<usize, Arc<Vec<u8>>>,
    bytes: usize,
    budget: usize,
}

impl<'a> FrameCache<'a> {
    pub fn new(project: &'a Project, plan: &'a VideoPlan, background: bool) -> Self {
        Self { project, plan, background, map: HashMap::new(), bytes: 0, budget: 256 * 1024 * 1024 }
    }

    pub fn get(&mut self, frame: usize) -> Arc<Vec<u8>> {
        if let Some(v) = self.map.get(&frame) {
            return v.clone();
        }
        let v = Arc::new(render_rgba(self.project, frame, self.plan, self.background));
        if self.bytes + v.len() > self.budget {
            self.map.clear();
            self.bytes = 0;
        }
        self.bytes += v.len();
        self.map.insert(frame, v.clone());
        v
    }
}

/// Mixed audio for output sample frames `from..from+n` (stereo f32), looping
/// the selected range so long exports never hold all the audio in memory.
pub fn audio_block(p: &Project, plan: &VideoPlan, from: u64, n: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; n * CHANNELS];
    let loop_len = (plan.audio_end - plan.audio_start).max(1) as u64;
    let mut done = 0usize;
    while done < n {
        let pos = (from + done as u64) % loop_len;
        let len = ((loop_len - pos) as usize).min(n - done);
        p.audio.mix(plan.audio_start + pos as i64, &mut out[done * CHANNELS..(done + len) * CHANNELS]);
        done += len;
    }
    out
}

/// Progress callback: receives 0..=1, returns false to cancel.
pub type Progress<'a> = &'a mut dyn FnMut(f32) -> bool;

/// Message used for cancelled exports (the UI shows it as a neutral notice).
pub const CANCELLED: &str = "exportación cancelada";

fn cancelled() -> ProjectError {
    ProjectError::Invalid(CANCELLED.into())
}

/// Write the export to `path` (MP4 is handled by the platform layer).
pub fn export(p: &Project, s: &ExportSettings, path: &Path, progress: Progress) -> Result<()> {
    match s.format {
        ExportFormat::Gif => write_gif(p, s, path, progress),
        ExportFormat::PngSequence => write_png_sequence(p, s, path, progress),
        ExportFormat::Png => {
            let plan = plan(p, s);
            let rgba = render_rgba(p, plan.spans[0].0, &plan, s.background);
            std::fs::write(path, crate::io::png_bytes(plan.width, plan.height, &rgba)?)?;
            progress(1.0);
            Ok(())
        }
        ExportFormat::Avi => write_avi(p, s, path, progress),
        ExportFormat::Wav => write_wav(p, s, path, progress),
        ExportFormat::Mp4 => Err(ProjectError::Invalid("MP4 se exporta con el codificador del sistema".into())),
    }
}

// ---- WAV -----------------------------------------------------------------------------

fn write_wav(p: &Project, s: &ExportSettings, path: &Path, progress: Progress) -> Result<()> {
    let plan = plan(p, s);
    let spec = hound::WavSpec { channels: 2, sample_rate: SAMPLE_RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let err = |e: hound::Error| ProjectError::Invalid(format!("WAV: {e}"));
    let mut w = hound::WavWriter::create(path, spec).map_err(err)?;
    let total = plan.audio_frames();
    let mut done = 0u64;
    while done < total {
        let n = (total - done).min(SAMPLE_RATE as u64) as usize;
        for v in audio_block(p, &plan, done, n) {
            w.write_sample((v.clamp(-1.0, 1.0) * 32767.0).round() as i16).map_err(err)?;
        }
        done += n as u64;
        if !progress(done as f32 / total.max(1) as f32) {
            return Err(cancelled());
        }
    }
    w.finalize().map_err(err)?;
    Ok(())
}

// ---- GIF ---------------------------------------------------------------------------

fn write_gif(p: &Project, s: &ExportSettings, path: &Path, progress: Progress) -> Result<()> {
    let plan = plan(p, s);
    if plan.width > u16::MAX as u32 || plan.height > u16::MAX as u32 {
        return Err(ProjectError::Invalid("demasiado grande para GIF".into()));
    }
    let file = BufWriter::new(File::create(path)?);
    let gif_err = |e: gif::EncodingError| ProjectError::Invalid(format!("GIF: {e}"));
    let mut enc = gif::Encoder::new(file, plan.width as u16, plan.height as u16, &[]).map_err(gif_err)?;
    // GIFs loop by themselves, so repetitions are never written twice.
    enc.set_repeat(gif::Repeat::Infinite).map_err(gif_err)?;
    let speed = (31 - (s.quality.clamp(1, 100) as i32 * 30 / 100)).clamp(1, 30);
    let mut quantized: HashMap<usize, gif::Frame<'static>> = HashMap::new();
    let mut cache = FrameCache::new(p, &plan, s.background);
    // GIF delays are 1/100 s with a 2 cs minimum. Time is tracked exactly and
    // every frame gets the delay that keeps its end where it should be.
    let (mut ticks_done, mut cs_done) = (0u64, 0u64);
    let n = plan.tick_spans.len();
    for (k, &(frame, ticks)) in plan.tick_spans.iter().enumerate() {
        ticks_done += ticks as u64;
        let end_cs = (ticks_done as f64 * 100.0 / plan.project_fps as f64).round() as u64;
        let delay = end_cs.saturating_sub(cs_done).max(2);
        cs_done += delay;
        let mut f = match quantized.get(&frame) {
            Some(f) => f.clone(),
            None => {
                let mut rgba = (*cache.get(frame)).clone();
                let f = gif::Frame::from_rgba_speed(plan.width as u16, plan.height as u16, &mut rgba, speed);
                quantized.insert(frame, f.clone());
                f
            }
        };
        f.delay = delay.min(u16::MAX as u64) as u16;
        f.dispose = gif::DisposalMethod::Background;
        enc.write_frame(&f).map_err(gif_err)?;
        if !progress((k + 1) as f32 / n as f32) {
            return Err(cancelled());
        }
    }
    Ok(())
}

// ---- PNG sequence ------------------------------------------------------------------

fn write_png_sequence(p: &Project, s: &ExportSettings, path: &Path, progress: Progress) -> Result<()> {
    let plan = plan(p, s);
    let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(path)?));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    // One image per output frame, so the sequence plays at the chosen fps in any editor.
    let total = plan.total_frames().max(1);
    let mut k = 0u64;
    let mut pngs: HashMap<usize, Arc<Vec<u8>>> = HashMap::new();
    let mut cache = FrameCache::new(p, &plan, s.background);
    for &(frame, n) in &plan.spans {
        let png = match pngs.get(&frame) {
            Some(b) => b.clone(),
            None => {
                let b = Arc::new(crate::io::png_bytes(plan.width, plan.height, &cache.get(frame))?);
                pngs.insert(frame, b.clone());
                b
            }
        };
        for _ in 0..n {
            zip.start_file(format!("{:05}.png", k + 1), stored)?;
            zip.write_all(&png)?;
            k += 1;
            if !progress(k as f32 / total as f32) {
                return Err(cancelled());
            }
        }
    }
    zip.finish()?;
    Ok(())
}

// ---- AVI (MJPEG + PCM) ----------------------------------------------------------------

/// Plain AVI (no OpenDML) is limited to about 2 GB.
const AVI_LIMIT: u64 = 2_000_000_000;

fn jpeg(rgba: &[u8], w: u32, h: u32, quality: u8, pad: Rgba) -> Result<Vec<u8>> {
    // JPEG has no alpha: composite over the pad color.
    let bg = pad.to_rgba8();
    let rgb: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as u32;
            [0, 1, 2].map(|i| ((p[i] as u32 * a + bg[i] as u32 * (255 - a)) / 255) as u8)
        })
        .collect();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    enc.encode(&rgb, w, h, image::ExtendedColorType::Rgb8).map_err(|e| ProjectError::Invalid(format!("JPEG: {e}")))?;
    Ok(out)
}

struct Riff<W: Write + Seek> {
    w: W,
    stack: Vec<u64>,
}

impl<W: Write + Seek> Riff<W> {
    fn begin(&mut self, kind: &[u8; 4], name: &[u8; 4]) -> Result<()> {
        self.w.write_all(kind)?;
        let pos = self.w.stream_position()?;
        self.w.write_all(&0u32.to_le_bytes())?;
        self.w.write_all(name)?;
        self.stack.push(pos);
        Ok(())
    }

    fn end(&mut self) -> Result<()> {
        let pos = self.stack.pop().expect("balanced lists");
        let here = self.w.stream_position()?;
        self.w.seek(SeekFrom::Start(pos))?;
        self.w.write_all(&((here - pos - 4) as u32).to_le_bytes())?;
        self.w.seek(SeekFrom::Start(here))?;
        Ok(())
    }

    /// Returns the chunk's offset (for the index).
    fn chunk(&mut self, id: &[u8; 4], data: &[u8]) -> Result<u64> {
        let at = self.w.stream_position()?;
        if at + data.len() as u64 > AVI_LIMIT {
            return Err(ProjectError::Invalid("el AVI pasaría de 2 GB: reduce el tamaño o la calidad, o usa MP4".into()));
        }
        self.w.write_all(id)?;
        self.w.write_all(&(data.len() as u32).to_le_bytes())?;
        self.w.write_all(data)?;
        if data.len() % 2 == 1 {
            self.w.write_all(&[0])?;
        }
        Ok(at)
    }
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn write_avi(p: &Project, s: &ExportSettings, path: &Path, progress: Progress) -> Result<()> {
    let plan = plan(p, s);
    let with_audio = s.include_audio && plan.has_audio;
    let frames = plan.total_frames() as u32;
    let (w, h, fps) = (plan.width, plan.height, plan.fps);
    let mut r = Riff { w: BufWriter::new(File::create(path)?), stack: Vec::new() };
    r.begin(b"RIFF", b"AVI ")?;
    r.begin(b"LIST", b"hdrl")?;
    let streams = if with_audio { 2 } else { 1 };
    let mut avih = Vec::new();
    // dwFlags: AVIF_HASINDEX | AVIF_ISINTERLEAVED
    for v in [1_000_000 / fps, 0, 0, 0x110, frames, 0, streams, 0, w, h, 0, 0, 0, 0] {
        avih.extend_from_slice(&le32(v));
    }
    r.chunk(b"avih", &avih)?;
    // Video stream.
    r.begin(b"LIST", b"strl")?;
    let mut strh = Vec::new();
    strh.extend_from_slice(b"vidsMJPG");
    for v in [0u32, 0, 0, 1, fps, 0, frames, 0, u32::MAX, 0] {
        strh.extend_from_slice(&le32(v));
    }
    strh.extend_from_slice(&[0u8; 8]); // rcFrame
    r.chunk(b"strh", &strh)?;
    let mut strf = Vec::new();
    for v in [40u32, w, h] {
        strf.extend_from_slice(&le32(v));
    }
    strf.extend_from_slice(&1u16.to_le_bytes());
    strf.extend_from_slice(&24u16.to_le_bytes());
    strf.extend_from_slice(b"MJPG");
    for v in [w * h * 3, 0, 0, 0, 0] {
        strf.extend_from_slice(&le32(v));
    }
    r.chunk(b"strf", &strf)?;
    r.end()?;
    // Audio stream (16-bit stereo PCM). dwScale = block align, dwRate = bytes/s.
    if with_audio {
        r.begin(b"LIST", b"strl")?;
        let mut strh = Vec::new();
        strh.extend_from_slice(b"auds\0\0\0\0");
        for v in [0u32, 0, 0, 4, SAMPLE_RATE * 4, 0, plan.audio_frames() as u32, 0, u32::MAX, 4] {
            strh.extend_from_slice(&le32(v));
        }
        strh.extend_from_slice(&[0u8; 8]);
        r.chunk(b"strh", &strh)?;
        let mut strf = Vec::new();
        strf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        strf.extend_from_slice(&2u16.to_le_bytes());
        strf.extend_from_slice(&le32(SAMPLE_RATE));
        strf.extend_from_slice(&le32(SAMPLE_RATE * 4));
        strf.extend_from_slice(&4u16.to_le_bytes());
        strf.extend_from_slice(&16u16.to_le_bytes());
        strf.extend_from_slice(&0u16.to_le_bytes());
        r.chunk(b"strf", &strf)?;
        r.end()?;
    }
    r.end()?; // hdrl

    r.begin(b"LIST", b"movi")?;
    let movi_start = r.stack.last().copied().expect("in movi") + 4;
    let mut index: Vec<([u8; 4], u32, u32)> = Vec::new();
    let mut jpegs: HashMap<usize, Arc<Vec<u8>>> = HashMap::new();
    let mut cache = FrameCache::new(p, &plan, true);
    let mut k = 0u64;
    let mut audio_pos = 0u64;
    for &(frame, n) in &plan.spans {
        let data = match jpegs.get(&frame) {
            Some(d) => d.clone(),
            None => {
                let d = Arc::new(jpeg(&cache.get(frame), w, h, s.quality, plan.pad)?);
                if jpegs.len() > 512 {
                    jpegs.clear();
                }
                jpegs.insert(frame, d.clone());
                d
            }
        };
        for _ in 0..n {
            let at = r.chunk(b"00dc", &data)?;
            index.push((*b"00dc", (at - movi_start) as u32, data.len() as u32));
            k += 1;
            if with_audio {
                // Audio for this frame, interleaved right after it.
                let until = plan.sample_of_frame(k);
                let pcm: Vec<u8> = audio_block(p, &plan, audio_pos, (until - audio_pos) as usize)
                    .iter()
                    .flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
                    .collect();
                audio_pos = until;
                if !pcm.is_empty() {
                    let at = r.chunk(b"01wb", &pcm)?;
                    index.push((*b"01wb", (at - movi_start) as u32, pcm.len() as u32));
                }
            }
            if !progress(k as f32 / frames.max(1) as f32) {
                return Err(cancelled());
            }
        }
    }
    r.end()?; // movi
    let mut idx = Vec::with_capacity(index.len() * 16);
    for (id, off, len) in index {
        idx.extend_from_slice(&id);
        idx.extend_from_slice(&le32(0x10)); // AVIIF_KEYFRAME
        idx.extend_from_slice(&le32(off));
        idx.extend_from_slice(&le32(len));
    }
    r.chunk(b"idx1", &idx)?;
    r.end()?; // RIFF
    r.w.flush()?;
    Ok(())
}

/// Rough size of the result in bytes (shown before exporting).
pub fn estimate_bytes(p: &Project, s: &ExportSettings) -> u64 {
    let pl = plan(p, s);
    let px = (pl.width * pl.height) as f64;
    let frames = pl.total_frames() as f64;
    let audio = if s.include_audio && pl.has_audio { pl.duration_secs() * 24_000.0 } else { 0.0 };
    let q = s.quality as f64 / 100.0;
    let b = match s.format {
        ExportFormat::Mp4 => mp4_bitrate(pl.width, pl.height, pl.fps, s.quality) as f64 / 8.0 * pl.duration_secs() + audio,
        ExportFormat::Gif => px * pl.tick_spans.len() as f64 * 0.25,
        ExportFormat::PngSequence => px * 0.3 * frames,
        ExportFormat::Png => px * 0.5,
        ExportFormat::Avi => px * (0.08 + 0.25 * q) * frames + if s.include_audio && pl.has_audio { pl.duration_secs() * 192_000.0 } else { 0.0 },
        ExportFormat::Wav => pl.duration_secs() * 192_000.0,
    };
    b as u64
}

/// H.264 bitrate used by the MP4 encoders (line art needs more than photos).
pub fn mp4_bitrate(w: u32, h: u32, fps: u32, quality: u8) -> u32 {
    let q = quality.clamp(10, 100) as f64 / 100.0;
    let bpp = 0.08 + 0.17 * q; // bits per pixel per frame
    ((w * h) as f64 * fps.clamp(12, 60) as f64 * bpp).clamp(1_000_000.0, 40_000_000.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioAsset, AudioClip, AudioTrack};
    use crate::document::{CanvasSettings, Frame};

    fn project() -> Project {
        let mut p = Project::new("x", CanvasSettings { width: 101, height: 64, fps: 12, ..Default::default() });
        let lid = p.layers[0].id;
        p.frames[0].cel_mut(lid, 101, 64).set_pixel(10, 10, [255, 0, 0, 255]);
        let id = p.alloc_id();
        p.frames.push(Frame { hold: 2, ..Frame::new(id) });
        let aid = p.alloc_id();
        p.audio.assets.insert(aid, std::sync::Arc::new(AudioAsset::new("t", vec![0.5; 48_000 * 2])));
        let mut t = AudioTrack::new(p.alloc_id(), "A");
        t.clips.push(AudioClip { id: p.alloc_id(), asset: aid, start: 0, offset: 0, len: 48_000, gain: 1.0, fade_in: 0, fade_out: 0, muted: false });
        p.audio.tracks.push(t);
        p
    }

    #[test]
    fn plan_respects_holds_loops_and_even_size() {
        let p = project();
        let s = ExportSettings { loops: 2, ..Default::default() };
        let pl = plan(&p, &s);
        assert_eq!((pl.width % 8, pl.height % 8), (0, 0), "MP4 sizes are multiples of 8");
        assert_eq!(pl.total_frames(), 6);
        assert_eq!(pl.audio_frames(), 24_000);
        let a = audio_block(&p, &pl, 0, 24_000);
        assert_eq!(a.len(), 48_000);
        assert_eq!(a[1000], 0.5);
    }

    #[test]
    fn output_fps_and_social_formats() {
        let p = project();
        let s = ExportSettings { out_fps: Some(30), aspect: Aspect::Vertical, size: OutSize::Short(720), format: ExportFormat::Avi, ..Default::default() };
        let pl = plan(&p, &s);
        assert_eq!((pl.width, pl.height), (720, 1280));
        // 3 ticks at 12 fps = 0.25 s = 7.5 → 8 frames at 30 fps (1 + 2 ticks → 2.5 + 5 rounded).
        assert_eq!(pl.total_frames(), 8);
        assert!(pl.layout.y > 0 && pl.layout.x == 0, "letterboxed vertically");
        let crop = plan(&p, &ExportSettings { fit: Fit::Crop, ..s });
        assert!(crop.layout.x < 0, "cropped at the sides");
        let rgba = render_rgba(&p, 0, &pl, true);
        assert_eq!(rgba.len(), (720 * 1280 * 4) as usize);
        assert_eq!(&rgba[..4], &[255, 255, 255, 255], "bars use the pad color");
    }

    #[test]
    fn gif_timing_survives_high_fps() {
        let mut p = project();
        p.canvas.fps = 60;
        p.frames[1].hold = 60; // one second
        let pl = plan(&p, &ExportSettings { format: ExportFormat::Gif, ..Default::default() });
        assert_eq!(pl.tick_spans.len(), 2);
        let dir = std::env::temp_dir().join(format!("trazo_gif_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.gif");
        export(&p, &ExportSettings { format: ExportFormat::Gif, ..Default::default() }, &path, &mut |_| true).unwrap();
        let mut dec = gif::DecodeOptions::new();
        dec.set_color_output(gif::ColorOutput::Indexed);
        let mut d = dec.read_info(std::fs::File::open(&path).unwrap()).unwrap();
        let mut delays = Vec::new();
        while let Some(f) = d.read_next_frame().unwrap() {
            delays.push(f.delay);
        }
        assert_eq!(delays, vec![2, 100], "the 1 s hold keeps 1 s");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_every_format() {
        let p = project();
        let dir = std::env::temp_dir().join(format!("trazo_export_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in [ExportFormat::Gif, ExportFormat::PngSequence, ExportFormat::Png, ExportFormat::Avi, ExportFormat::Wav] {
            let path = dir.join(format!("out.{}", f.extension()));
            let s = ExportSettings { format: f, size: OutSize::Percent(0.5), ..Default::default() };
            let mut last = 0.0;
            export(&p, &s, &path, &mut |v| {
                last = v;
                true
            })
            .unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.len() > 40, "{f:?} too small");
            assert!(last > 0.99, "{f:?} progress ended at {last}");
            match f {
                ExportFormat::Gif => assert_eq!(&bytes[..3], b"GIF"),
                ExportFormat::Png => assert_eq!(&bytes[1..4], b"PNG"),
                ExportFormat::Avi => {
                    assert_eq!(&bytes[..4], b"RIFF");
                    assert_eq!(&bytes[8..12], b"AVI ");
                    let riff_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
                    assert_eq!(riff_len + 8, bytes.len(), "RIFF size matches file");
                }
                ExportFormat::PngSequence => assert_eq!(&bytes[..2], b"PK"),
                _ => {}
            }
            assert!(estimate_bytes(&p, &s) > 0);
        }
        let path = dir.join("cancel.gif");
        let s = ExportSettings { format: ExportFormat::Gif, ..Default::default() };
        assert!(export(&p, &s, &path, &mut |_| false).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
