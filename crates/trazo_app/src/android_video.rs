//! Video import on Android: MP4 / 3GP / WebM / MKV decoded with the device's
//! own decoders (MediaExtractor + MediaCodec through the NDK). The decoder
//! writes YUV 4:2:0 into its own output buffers ("flexible" format, which
//! decoders deliver as NV12 or I420); every read is bounds-checked, so an
//! unexpected layout gives an error instead of a crash. If the device's
//! decoder can't be used, Android's software decoder is tried.

#![cfg(target_os = "android")]

use std::os::fd::AsRawFd;
use std::path::Path;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use trazo_core::video::VideoBuilder;
use ndk::media::media_codec::{DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection};
use ndk::media::media_format::MediaFormat;
use ndk_sys as ffi;

const FLAG_CODEC_CONFIG: u32 = 2;
const FLAG_END_OF_STREAM: u32 = 4;
const COLOR_YUV420_PLANAR: i32 = 19; // I420
const COLOR_YUV420_PACKED_PLANAR: i32 = 20;
const COLOR_YUV420_SEMIPLANAR: i32 = 21; // NV12
const COLOR_YUV420_PACKED_SEMIPLANAR: i32 = 39;
const COLOR_YUV420_FLEXIBLE: i32 = 0x7F42_0888;
const TIMEOUT: Duration = Duration::from_millis(5);
/// Longest time without any decoded picture before giving up.
const STALL_LIMIT: Duration = Duration::from_secs(10);

/// What the import dialog shows before decoding.
#[derive(Clone, Debug)]
pub struct VideoInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    /// Clockwise rotation to show it upright (phone videos).
    pub rotation: i32,
    pub has_audio: bool,
}

impl VideoInfo {
    /// Size once rotated upright.
    pub fn upright(&self) -> (u32, u32) {
        if self.rotation % 180 != 0 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

struct Extractor {
    ptr: *mut ffi::AMediaExtractor,
    _file: std::fs::File,
}

impl Drop for Extractor {
    fn drop(&mut self) {
        unsafe { ffi::AMediaExtractor_delete(self.ptr) };
    }
}

impl Extractor {
    fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let ptr = unsafe { ffi::AMediaExtractor_new() };
        if ptr.is_null() {
            return Err("no se pudo abrir el lector de video".into());
        }
        let ex = Self { ptr, _file: file };
        let st = unsafe { ffi::AMediaExtractor_setDataSourceFd(ptr, ex._file.as_raw_fd(), 0, len as i64) };
        if st.0 != 0 {
            return Err("formato de video no compatible".into());
        }
        Ok(ex)
    }

    fn format(&self, track: usize) -> Option<MediaFormat> {
        let f = unsafe { ffi::AMediaExtractor_getTrackFormat(self.ptr, track) };
        NonNull::new(f).map(|p| unsafe { MediaFormat::from_ptr(p) })
    }

    /// (video track and its format, whether there is an audio track)
    fn tracks(&self) -> (Option<(usize, MediaFormat)>, bool) {
        let n = unsafe { ffi::AMediaExtractor_getTrackCount(self.ptr) };
        let (mut video, mut audio) = (None, false);
        for i in 0..n {
            let Some(mut f) = self.format(i) else { continue };
            let mime = f.str("mime").unwrap_or_default().to_string();
            if mime.starts_with("video/") && video.is_none() {
                video = Some((i, f));
            } else if mime.starts_with("audio/") {
                audio = true;
            }
        }
        (video, audio)
    }
}

pub fn probe(path: &Path) -> Result<VideoInfo, String> {
    let ex = Extractor::open(path)?;
    let (video, has_audio) = ex.tracks();
    let (_, f) = video.ok_or("el archivo no tiene video")?;
    let width = f.i32("width").unwrap_or(0).max(0) as u32;
    let height = f.i32("height").unwrap_or(0).max(0) as u32;
    if width == 0 || height == 0 {
        return Err("tamaño de video desconocido".into());
    }
    let duration = f.i64("durationUs").unwrap_or(0).max(0) as f64 / 1e6;
    let rotation = f.i32("rotation-degrees").unwrap_or(0).rem_euclid(360);
    Ok(VideoInfo { duration, width, height, rotation, has_audio })
}

/// Copy of a decoded picture (the decoder's buffer must be returned quickly).
struct Yuv {
    w: usize,
    h: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    y_row: usize,
    c_row: usize,
    c_pix: usize,
}

/// Where the picture is inside a decoder output buffer.
#[derive(Clone, Copy, Debug)]
struct OutLayout {
    /// Visible area (crop).
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    /// Bytes per luma row and luma rows before the chroma starts.
    stride: usize,
    slice: usize,
    /// I420 (separate U and V planes) instead of NV12 (interleaved UV).
    planar: bool,
    /// The decoder didn't say how tall the luma plane is.
    slice_guessed: bool,
}

impl OutLayout {
    fn new(w: u32, h: u32) -> Self {
        Self { x0: 0, y0: 0, w: w as usize, h: h as usize, stride: w as usize, slice: h as usize, planar: false, slice_guessed: true }
    }

    /// Update from the decoder's output format. Errors on layouts we can't read
    /// (vendor tiled formats), so the software decoder is tried instead.
    fn update(&mut self, f: &MediaFormat) -> Result<(), String> {
        let (w, h) = (f.i32("width").unwrap_or(self.w as i32).max(1) as usize, f.i32("height").unwrap_or(self.h as i32).max(1) as usize);
        let (l, t) = (f.i32("crop-left"), f.i32("crop-top"));
        let (r, b) = (f.i32("crop-right"), f.i32("crop-bottom"));
        if let (Some(l), Some(t), Some(r), Some(b)) = (l, t, r, b) {
            self.x0 = l.max(0) as usize;
            self.y0 = t.max(0) as usize;
            self.w = (r - l + 1).max(2) as usize;
            self.h = (b - t + 1).max(2) as usize;
        } else {
            self.w = w;
            self.h = h;
        }
        self.stride = f.i32("stride").filter(|s| *s as usize >= self.x0 + self.w).map_or(w.max(self.x0 + self.w), |s| s as usize);
        match f.i32("slice-height").filter(|s| *s as usize >= self.y0 + self.h) {
            Some(s) => {
                self.slice = s as usize;
                self.slice_guessed = false;
            }
            None => {
                self.slice = h.max(self.y0 + self.h);
                self.slice_guessed = true;
            }
        }
        let color = f.i32("color-format").unwrap_or(COLOR_YUV420_FLEXIBLE);
        self.planar = match color {
            COLOR_YUV420_PLANAR | COLOR_YUV420_PACKED_PLANAR => true,
            COLOR_YUV420_SEMIPLANAR | COLOR_YUV420_PACKED_SEMIPLANAR | COLOR_YUV420_FLEXIBLE => false,
            other => return Err(format!("formato de color del decodificador no compatible ({other:#x})")),
        };
        log::info!("salida del decodificador: {self:?} color={color:#x}");
        Ok(())
    }
}

impl Yuv {
    /// Copy the visible picture out of a decoder output buffer. Every range
    /// is checked: a buffer smaller than its declared layout gives None.
    fn from_buffer(data: &[u8], l: &OutLayout) -> Option<Self> {
        let (w, h, stride) = (l.w, l.h, l.stride);
        if w < 2 || h < 2 || stride < w {
            return None;
        }
        // Some decoders don't report the luma height but pad it to 16 or 32 rows.
        let mut slice = l.slice;
        if l.slice_guessed {
            for align in [16, 32] {
                let aligned = (l.slice + align - 1) / align * align;
                if aligned != l.slice && data.len() >= stride * aligned * 3 / 2 && data.len() < stride * (aligned + align) * 3 / 2 {
                    slice = aligned;
                }
            }
        }
        // A declared luma height that doesn't fit the buffer is wrong (some
        // decoders report it before the real format): derive it from the size.
        if stride * slice * 3 / 2 > data.len() {
            let fit = data.len() * 2 / 3 / stride;
            if fit >= l.y0 + h {
                slice = fit;
            }
        }
        let ystart = l.y0 * stride + l.x0;
        let y_len = (h - 1) * stride + w;
        let y = data.get(ystart..ystart.checked_add(y_len)?)?.to_vec();
        let luma = stride.checked_mul(slice)?;
        let (u, v, c_row, c_pix) = if l.planar {
            let c_row = stride / 2;
            let c_len = ((h - 1) / 2) * c_row + (w - 1) / 2 + 1;
            let first = (l.y0 / 2) * c_row + l.x0 / 2;
            let u0 = luma + first;
            let v0 = luma + c_row * (slice / 2) + first;
            (data.get(u0..u0 + c_len)?.to_vec(), data.get(v0..v0 + c_len)?.to_vec(), c_row, 1)
        } else {
            let c_len = ((h - 1) / 2) * stride + ((w - 1) / 2) * 2 + 1;
            let u0 = luma + (l.y0 / 2) * stride + (l.x0 / 2) * 2;
            (data.get(u0..u0 + c_len)?.to_vec(), data.get(u0 + 1..u0 + 1 + c_len)?.to_vec(), stride, 2)
        };
        Some(Self { w, h, y, u, v, y_row: stride, c_row, c_pix })
    }

    /// Straight RGBA8 with the video's color matrix and range.
    fn to_rgba(&self, c: ColorSpace) -> Vec<u8> {
        // Coefficients for limited range; full range scales them back.
        let (rv, gu, gv, bu) = if c.bt709 { (1.793, 0.213, 0.533, 2.112) } else { (1.596, 0.392, 0.813, 2.017) };
        let (ys, yo, cs) = if c.full { (1.0, 0.0, 224.0 / 255.0) } else { (1.164, 16.0, 1.0) };
        let (rv, gu, gv, bu) = (rv * cs, gu * cs, gv * cs, bu * cs);
        let mut out = vec![255u8; self.w * self.h * 4];
        for row in 0..self.h {
            let yr = &self.y[row * self.y_row..];
            let cr = (row / 2) * self.c_row;
            let o = &mut out[row * self.w * 4..(row + 1) * self.w * 4];
            for x in 0..self.w {
                let yy = ys * (yr[x] as f32 - yo);
                let ci = cr + (x / 2) * self.c_pix;
                let (u, v) = (self.u[ci] as f32 - 128.0, self.v[ci] as f32 - 128.0);
                let px = &mut o[x * 4..x * 4 + 3];
                px[0] = (yy + rv * v).clamp(0.0, 255.0) as u8;
                px[1] = (yy - gu * u - gv * v).clamp(0.0, 255.0) as u8;
                px[2] = (yy + bu * u).clamp(0.0, 255.0) as u8;
            }
        }
        out
    }
}

/// Color matrix (BT.709 or BT.601) and range (full 0–255 or limited 16–235).
#[derive(Clone, Copy)]
struct ColorSpace {
    bt709: bool,
    full: bool,
}

impl ColorSpace {
    /// From the track format; guesses BT.709 for HD when not stated.
    fn of(format: &MediaFormat, height: u32) -> Self {
        let bt709 = match format.i32("color-standard") {
            Some(1) => true,
            Some(2) | Some(4) => false,
            _ => height >= 720,
        };
        Self { bt709, full: format.i32("color-range") == Some(1) }
    }
}

/// Rotate straight RGBA8 clockwise by 90, 180 or 270 degrees.
fn rotate(w: usize, h: usize, px: Vec<u8>, degrees: i32) -> (usize, usize, Vec<u8>) {
    let get = |x: usize, y: usize| -> [u8; 4] { px[(y * w + x) * 4..][..4].try_into().unwrap_or([0; 4]) };
    match degrees {
        90 | 270 => {
            let mut out = vec![0u8; w * h * 4];
            for y in 0..w {
                for x in 0..h {
                    let s = if degrees == 90 { get(y, h - 1 - x) } else { get(w - 1 - y, x) };
                    out[(y * h + x) * 4..][..4].copy_from_slice(&s);
                }
            }
            (h, w, out)
        }
        180 => {
            let mut out = px.clone();
            for (i, p) in out.chunks_exact_mut(4).enumerate() {
                let j = w * h - 1 - i;
                p.copy_from_slice(&px[j * 4..j * 4 + 4]);
            }
            (w, h, out)
        }
        _ => (w, h, px),
    }
}

/// Sampling state: picks, for each output slot (`from + n / fps`), the last
/// decoded picture that started at or before it.
struct Sampler<'a> {
    out: &'a mut VideoBuilder,
    from: f64,
    to: f64,
    step: f64,
    slot: usize,
    prev: Option<Yuv>,
    prev_pushed: bool,
    rotation: i32,
    color: ColorSpace,
}

impl Sampler<'_> {
    fn slot_time(&self) -> f64 {
        self.from + self.slot as f64 * self.step
    }

    fn done(&self) -> bool {
        self.slot_time() >= self.to - 1e-6 || self.out.is_full()
    }

    fn emit(&mut self, pic: &Yuv, fresh: bool) -> Result<(), String> {
        if fresh {
            let (w, h, px) = rotate(pic.w, pic.h, pic.to_rgba(self.color), self.rotation);
            self.out.push(w as u32, h as u32, px).map_err(|e| e.to_string())
        } else {
            self.out.repeat_last().map_err(|e| e.to_string())
        }
    }

    /// A new picture starting at `t` seconds. Slots before `t` show the
    /// previous picture (or this one, when it is the first).
    fn picture(&mut self, t: f64, pic: Yuv) -> Result<(), String> {
        let mut pic_pushed = false;
        while !self.done() && self.slot_time() < t - 1e-6 {
            match self.prev.take() {
                Some(prev) => {
                    let fresh = !self.prev_pushed;
                    let r = self.emit(&prev, fresh);
                    self.prev = Some(prev);
                    r?;
                    self.prev_pushed = true;
                }
                None => {
                    self.emit(&pic, !pic_pushed)?;
                    pic_pushed = true;
                }
            }
            self.slot += 1;
        }
        self.prev = Some(pic);
        self.prev_pushed = pic_pushed;
        Ok(())
    }

    /// End of the video: the last picture fills the remaining slots.
    fn finish(&mut self, end: f64) -> Result<(), String> {
        let end = end.min(self.to);
        while !self.done() && self.slot_time() < end - 1e-6 {
            let Some(prev) = self.prev.take() else { break };
            let fresh = !self.prev_pushed;
            self.emit(&prev, fresh)?;
            self.prev = Some(prev);
            self.prev_pushed = true;
            self.slot += 1;
        }
        Ok(())
    }
}

/// Android's own software decoder for a format (used when the device's
/// decoder refuses our input, as happens on emulators).
fn software_decoder(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "video/avc" => "c2.android.avc.decoder",
        "video/hevc" => "c2.android.hevc.decoder",
        "video/x-vnd.on2.vp8" => "c2.android.vp8.decoder",
        "video/x-vnd.on2.vp9" => "c2.android.vp9.decoder",
        "video/av01" => "c2.android.av1.decoder",
        "video/mp4v-es" => "c2.android.mpeg4.decoder",
        "video/3gpp" => "c2.android.h263.decoder",
        _ => return None,
    })
}

/// Decode `from..to` seconds of the video at `fps` pictures per second into
/// `out`. `progress` gets 0..=1 and returns false to cancel.
pub fn decode(path: &Path, fps: f32, from: f64, to: f64, out: &mut VideoBuilder, progress: &mut dyn FnMut(f32) -> bool) -> Result<(), String> {
    match decode_with(path, fps, from, to, out, progress, false) {
        // Nothing decoded with the device's decoder: try the software one.
        Err(e) if out.is_empty() && e != trazo_core::export::CANCELLED => {
            log::warn!("decodificador del sistema falló ({e}); se prueba el de software");
            decode_with(path, fps, from, to, out, progress, true).map_err(|e2| format!("{e2} (antes: {e})"))
        }
        r => r,
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_with(path: &Path, fps: f32, from: f64, to: f64, out: &mut VideoBuilder, progress: &mut dyn FnMut(f32) -> bool, software: bool) -> Result<(), String> {
    let vinfo = probe(path)?;
    let ex = Extractor::open(path)?;
    let (video, _) = ex.tracks();
    let (track, mut format) = video.ok_or("el archivo no tiene video")?;
    let mime = format.str("mime").unwrap_or_default().to_string();
    let color = ColorSpace::of(&format, vinfo.height);
    // A clean format for the decoder: the codec-specific data (csd-*) is sent
    // as the first input buffers instead, which works on every device (some,
    // like the emulator, fail to pass it through the format). We rotate the
    // pictures ourselves, so no rotation either.
    let mut dec_format = MediaFormat::new();
    dec_format.set_str("mime", &mime);
    dec_format.set_i32("width", vinfo.width as i32);
    dec_format.set_i32("height", vinfo.height as i32);
    if let Some(n) = format.i32("max-input-size") {
        dec_format.set_i32("max-input-size", n);
    }
    let mut csd: std::collections::VecDeque<Vec<u8>> = ["csd-0", "csd-1", "csd-2"].iter().filter_map(|k| format.buffer(k).map(<[u8]>::to_vec)).collect();
    if unsafe { ffi::AMediaExtractor_selectTrack(ex.ptr, track) }.0 != 0 {
        return Err("no se pudo leer la pista de video".into());
    }
    if from > 0.0 && unsafe { ffi::AMediaExtractor_seekTo(ex.ptr, (from * 1e6) as i64, ffi::SeekMode::AMEDIAEXTRACTOR_SEEK_PREVIOUS_SYNC) }.0 != 0 {
        log::warn!("no se pudo saltar a {from} s: se lee desde el principio");
    }
    // Ask for YUV 4:2:0 in the output buffers (no surface).
    dec_format.set_i32("color-format", COLOR_YUV420_FLEXIBLE);
    let codec = if software {
        software_decoder(&mime).and_then(MediaCodec::from_codec_name)
    } else {
        MediaCodec::from_decoder_type(&mime)
    }
    .ok_or_else(|| format!("este dispositivo no puede leer {mime}"))?;
    codec.configure(&dec_format, None, MediaCodecDirection::Decoder).map_err(|e| format!("decodificador: {e}"))?;
    codec.start().map_err(|e| e.to_string())?;

    let to = to.min(if vinfo.duration > 0.0 { vinfo.duration } else { f64::MAX });
    let mut s = Sampler { out, from, to, step: 1.0 / fps.max(1.0) as f64, slot: 0, prev: None, prev_pushed: false, rotation: vinfo.rotation, color };
    let mut input_done = false;
    let mut last_t = from;
    let mut last_activity = Instant::now();
    let mut layout = OutLayout::new(vinfo.width, vinfo.height);
    let mut bad_pictures = 0;
    let result = (|| -> Result<(), String> {
        loop {
            if !input_done {
                if let DequeuedInputBufferResult::Buffer(mut ib) = codec.dequeue_input_buffer(TIMEOUT).map_err(|e| e.to_string())? {
                    let buf = ib.buffer_mut();
                    if let Some(data) = csd.pop_front() {
                        let n = data.len().min(buf.len());
                        for (d, v) in buf.iter_mut().zip(&data[..n]) {
                            d.write(*v);
                        }
                        codec.queue_input_buffer(ib, 0, n, 0, FLAG_CODEC_CONFIG).map_err(|e| e.to_string())?;
                        continue;
                    }
                    let n = unsafe { ffi::AMediaExtractor_readSampleData(ex.ptr, buf.as_mut_ptr().cast(), buf.len()) };
                    let pts = unsafe { ffi::AMediaExtractor_getSampleTime(ex.ptr) };
                    if n < 0 && unsafe { ffi::AMediaExtractor_getSampleSize(ex.ptr) } > buf.len() as isize {
                        return Err("una imagen del video es demasiado grande para el decodificador".into());
                    }
                    // Past the wanted range (plus a margin for reordered frames): stop reading.
                    if n < 0 || pts as f64 / 1e6 > to + 1.0 {
                        codec.queue_input_buffer(ib, 0, 0, 0, FLAG_END_OF_STREAM).map_err(|e| e.to_string())?;
                        input_done = true;
                    } else {
                        codec.queue_input_buffer(ib, 0, n as usize, pts.max(0) as u64, 0).map_err(|e| e.to_string())?;
                        unsafe { ffi::AMediaExtractor_advance(ex.ptr) };
                    }
                }
            }
            match codec.dequeue_output_buffer(TIMEOUT).map_err(|e| e.to_string())? {
                DequeuedOutputBufferInfoResult::OutputFormatChanged => layout.update(&codec.output_format())?,
                DequeuedOutputBufferInfoResult::Buffer(b) => {
                    let info = *b.info();
                    let eos = info.flags() & FLAG_END_OF_STREAM != 0;
                    let t = info.presentation_time_us() as f64 / 1e6;
                    if info.size() > 0 {
                        let start = info.offset().max(0) as usize;
                        let data = b.buffer().get(start..start + info.size() as usize).unwrap_or(&[]);
                        match Yuv::from_buffer(data, &layout) {
                            Some(pic) => {
                                if t <= to + 1e-6 {
                                    s.picture(t, pic)?;
                                }
                                last_t = last_t.max(t);
                                last_activity = Instant::now();
                            }
                            None => {
                                bad_pictures += 1;
                                log::warn!("imagen del decodificador ilegible ({} bytes, {layout:?})", data.len());
                            }
                        }
                    }
                    codec.release_output_buffer(b, false).map_err(|e| e.to_string())?;
                    if bad_pictures >= 3 && s.out.is_empty() {
                        return Err("el decodificador entrega imágenes en un formato que no se puede leer".into());
                    }
                    if eos || s.done() {
                        let end = if vinfo.duration > 0.0 { vinfo.duration } else { last_t + s.step };
                        s.finish(end)?;
                        return Ok(());
                    }
                    let frac = ((last_t - from) / (to - from).max(0.001)).clamp(0.0, 1.0) as f32;
                    if !progress(frac) {
                        return Err(trazo_core::export::CANCELLED.into());
                    }
                }
                DequeuedOutputBufferInfoResult::TryAgainLater => {
                    if last_activity.elapsed() > STALL_LIMIT {
                        return Err("el decodificador dejó de responder".into());
                    }
                    if !progress(((last_t - from) / (to - from).max(0.001)).clamp(0.0, 1.0) as f32) {
                        return Err(trazo_core::export::CANCELLED.into());
                    }
                }
                _ => {}
            }
        }
    })();
    let _ = codec.stop();
    result?;
    if s.out.is_empty() {
        return Err("no se pudo leer ninguna imagen del video".into());
    }
    Ok(())
}
