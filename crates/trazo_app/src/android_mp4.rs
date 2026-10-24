//! MP4 export on Android: H.264 video + AAC audio with the device's own
//! encoders (MediaCodec, usually hardware) and MediaMuxer, called from Rust
//! through the NDK. Frames and audio come from `trazo_core::export`.

#![cfg(target_os = "android")]

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use trazo_core::audio::SAMPLE_RATE;
use trazo_core::export::{audio_block, mp4_bitrate, plan, ExportSettings, FrameCache, CANCELLED};
use trazo_core::Project;
use ndk::media::media_codec::{DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection};
use ndk::media::media_format::MediaFormat;
use ndk_sys as ffi;

const COLOR_YUV420_PLANAR: i32 = 19; // I420
const COLOR_YUV420_SEMIPLANAR: i32 = 21; // NV12
const COLOR_YUV420_FLEXIBLE: i32 = 0x7F42_0888;
const FLAG_CODEC_CONFIG: u32 = 2;
const FLAG_END_OF_STREAM: u32 = 4;
const AAC_FRAME: usize = 1024;
const TIMEOUT: Duration = Duration::from_millis(10);
/// Longest wait for the encoder to accept or return a buffer.
const STALL_LIMIT: Duration = Duration::from_secs(15);

struct Muxer {
    ptr: *mut ffi::AMediaMuxer,
    started: bool,
    video_track: Option<usize>,
    audio_track: Option<usize>,
    wants_audio: bool,
    /// Samples produced before the muxer could start (audio?, data, info).
    pending: Vec<(bool, Vec<u8>, ffi::AMediaCodecBufferInfo)>,
}

impl Muxer {
    fn ready(&self) -> bool {
        self.video_track.is_some() && (!self.wants_audio || self.audio_track.is_some())
    }

    fn add_track(&mut self, format: &MediaFormat, audio: bool) -> Result<(), String> {
        let idx = unsafe { ffi::AMediaMuxer_addTrack(self.ptr, format.as_ptr()) };
        if idx < 0 {
            return Err("el multiplexor rechazó la pista".into());
        }
        if audio {
            self.audio_track = Some(idx as usize);
        } else {
            self.video_track = Some(idx as usize);
        }
        if self.ready() && !self.started {
            let st = unsafe { ffi::AMediaMuxer_start(self.ptr) };
            if st.0 != 0 {
                return Err(format!("no se pudo iniciar el MP4 ({})", st.0));
            }
            self.started = true;
            for (is_audio, data, info) in std::mem::take(&mut self.pending) {
                self.write(is_audio, &data, info)?;
            }
        }
        Ok(())
    }

    fn write(&mut self, audio: bool, data: &[u8], info: ffi::AMediaCodecBufferInfo) -> Result<(), String> {
        if !self.started {
            self.pending.push((audio, data.to_vec(), info));
            return Ok(());
        }
        let track = if audio { self.audio_track } else { self.video_track }.ok_or("pista inexistente")?;
        let info = ffi::AMediaCodecBufferInfo { offset: 0, ..info };
        let st = unsafe { ffi::AMediaMuxer_writeSampleData(self.ptr, track, data.as_ptr(), &info) };
        if st.0 != 0 {
            return Err(format!("error escribiendo el MP4 ({}): ¿queda espacio?", st.0));
        }
        Ok(())
    }

    /// Write the index and close; reports errors (e.g. disk full).
    fn finish(mut self) -> Result<(), String> {
        let st = unsafe { ffi::AMediaMuxer_stop(self.ptr) };
        self.started = false;
        if st.0 != 0 {
            return Err(format!("no se pudo cerrar el MP4 ({}): ¿queda espacio?", st.0));
        }
        Ok(())
    }
}

impl Drop for Muxer {
    fn drop(&mut self) {
        unsafe {
            if self.started {
                ffi::AMediaMuxer_stop(self.ptr);
            }
            ffi::AMediaMuxer_delete(self.ptr);
        }
    }
}

/// Move every available encoded buffer of `codec` into the muxer.
/// Returns true once the end-of-stream buffer has been seen.
fn drain(codec: &MediaCodec, mux: &mut Muxer, audio: bool, timeout: Duration) -> Result<bool, String> {
    loop {
        match codec.dequeue_output_buffer(timeout).map_err(|e| e.to_string())? {
            DequeuedOutputBufferInfoResult::TryAgainLater => return Ok(false),
            DequeuedOutputBufferInfoResult::OutputFormatChanged => {
                let fmt = codec.output_format();
                mux.add_track(&fmt, audio)?;
            }
            DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
            DequeuedOutputBufferInfoResult::Buffer(buf) => {
                let info = *buf.info();
                let flags = info.flags();
                if flags & FLAG_CODEC_CONFIG == 0 && info.size() > 0 {
                    let start = info.offset() as usize;
                    let data = &buf.buffer()[start..start + info.size() as usize];
                    let raw = ffi::AMediaCodecBufferInfo { offset: 0, size: info.size(), presentationTimeUs: info.presentation_time_us(), flags };
                    mux.write(audio, data, raw)?;
                }
                codec.release_output_buffer(buf, false).map_err(|e| e.to_string())?;
                if flags & FLAG_END_OF_STREAM != 0 {
                    return Ok(true);
                }
            }
        }
    }
}

/// Layout of the encoder's input buffer.
#[derive(Clone, Copy)]
struct YuvLayout {
    w: usize,
    h: usize,
    stride: usize,
    slice: usize,
    semi_planar: bool,
}

impl YuvLayout {
    fn size(&self) -> usize {
        self.stride * self.slice * 3 / 2
    }
}

/// RGBA → YUV 4:2:0 (BT.601 limited range) in the encoder's layout, row by row.
fn to_yuv(rgba: &[u8], l: YuvLayout) -> Vec<u8> {
    let mut out = vec![0u8; l.size()];
    let (w, h) = (l.w, l.h);
    for y in 0..h {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        let dst = &mut out[y * l.stride..y * l.stride + w];
        for (x, d) in dst.iter_mut().enumerate() {
            let (r, g, b) = (row[x * 4] as i32, row[x * 4 + 1] as i32, row[x * 4 + 2] as i32);
            *d = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(0, 255) as u8;
        }
    }
    let y_size = l.stride * l.slice;
    let cstride = l.stride / 2;
    for y in 0..h / 2 {
        for x in 0..w / 2 {
            // Average the 2×2 block.
            let mut acc = [0i32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let o = ((y * 2 + dy) * w + x * 2 + dx) * 4;
                for (c, a) in acc.iter_mut().enumerate() {
                    *a += rgba[o + c] as i32;
                }
            }
            let (r, g, b) = (acc[0] / 4, acc[1] / 4, acc[2] / 4);
            let u = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            let v = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            if l.semi_planar {
                let o = y_size + y * l.stride + x * 2;
                out[o] = u;
                out[o + 1] = v;
            } else {
                out[y_size + y * cstride + x] = u;
                out[y_size + (l.slice / 2) * cstride + y * cstride + x] = v;
            }
        }
    }
    out
}

/// Configure the H.264 encoder, preferring explicit NV12, then I420, then
/// "flexible" (treated as NV12, what most hardware encoders expect).
fn video_encoder(w: usize, h: usize, fps: u32, bitrate: u32) -> Result<(MediaCodec, YuvLayout), String> {
    let mut last_err = String::new();
    for color in [COLOR_YUV420_SEMIPLANAR, COLOR_YUV420_PLANAR, COLOR_YUV420_FLEXIBLE] {
        let enc = MediaCodec::from_encoder_type("video/avc").ok_or("este dispositivo no tiene codificador H.264")?;
        let mut vf = MediaFormat::new();
        vf.set_str("mime", "video/avc");
        vf.set_i32("width", w as i32);
        vf.set_i32("height", h as i32);
        vf.set_i32("color-format", color);
        vf.set_i32("bitrate", bitrate as i32);
        vf.set_i32("frame-rate", fps as i32);
        vf.set_i32("i-frame-interval", 1);
        match enc.configure(&vf, None, MediaCodecDirection::Encoder) {
            Ok(()) => {
                enc.start().map_err(|e| e.to_string())?;
                let inf = enc.input_format();
                let stride = inf.i32("stride").filter(|v| *v as usize >= w).unwrap_or(w as i32) as usize;
                let slice = inf.i32("slice-height").filter(|v| *v as usize >= h).unwrap_or(h as i32) as usize;
                let reported = inf.i32("color-format").unwrap_or(color);
                let semi_planar = match color {
                    COLOR_YUV420_SEMIPLANAR => true,
                    COLOR_YUV420_PLANAR => false,
                    _ => reported != COLOR_YUV420_PLANAR,
                };
                log::info!("H.264 {w}x{h} pedido={color:#x} informado={reported:#x} stride={stride} slice={slice} bitrate={bitrate}");
                return Ok((enc, YuvLayout { w, h, stride, slice, semi_planar }));
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(format!("el codificador H.264 no acepta {w}×{h} ({last_err}); prueba con 1080p o 720p"))
}

/// Encode the animation to an MP4 file at `path`. `progress` gets 0..=1 and
/// returns false to cancel.
pub fn export_mp4(p: &Project, s: &ExportSettings, path: &Path, progress: &mut dyn FnMut(f32) -> bool) -> Result<(), String> {
    let plan = plan(p, s);
    let (w, h) = (plan.width as usize, plan.height as usize);
    let fps = plan.fps;
    let with_audio = s.include_audio && plan.has_audio;
    let total_audio = if with_audio { plan.audio_frames() as usize } else { 0 };

    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mux_ptr = unsafe { ffi::AMediaMuxer_new(file.as_raw_fd(), ffi::OutputFormat::AMEDIAMUXER_OUTPUT_FORMAT_MPEG_4) };
    if mux_ptr.is_null() {
        return Err("no se pudo crear el archivo MP4".into());
    }
    let mut mux = Muxer { ptr: mux_ptr, started: false, video_track: None, audio_track: None, wants_audio: with_audio, pending: Vec::new() };

    let (venc, layout) = video_encoder(w, h, fps, mp4_bitrate(plan.width, plan.height, fps, s.quality))?;

    let aenc = if with_audio {
        let enc = MediaCodec::from_encoder_type("audio/mp4a-latm").ok_or("este dispositivo no tiene codificador AAC")?;
        let mut af = MediaFormat::new();
        af.set_str("mime", "audio/mp4a-latm");
        af.set_i32("sample-rate", SAMPLE_RATE as i32);
        af.set_i32("channel-count", 2);
        af.set_i32("bitrate", 192_000);
        af.set_i32("aac-profile", 2);
        af.set_i32("max-input-size", (AAC_FRAME * 4 * 4) as i32);
        enc.configure(&af, None, MediaCodecDirection::Encoder).map_err(|e| format!("AAC: {e}"))?;
        enc.start().map_err(|e| e.to_string())?;
        Some(enc)
    } else {
        None
    };

    // Audio fed so far (stereo frames) and whether its end-of-stream was sent.
    let mut audio_pos = 0usize;
    let mut audio_eos_sent = !with_audio;
    let mut video_done = false;
    let mut audio_done = !with_audio;

    let feed_audio = |enc: &MediaCodec, until: usize, eos: bool, pos: &mut usize, eos_sent: &mut bool| -> Result<(), String> {
        while *pos < until.min(total_audio) || (eos && !*eos_sent) {
            match enc.dequeue_input_buffer(TIMEOUT).map_err(|e| e.to_string())? {
                DequeuedInputBufferResult::TryAgainLater => return Ok(()),
                DequeuedInputBufferResult::Buffer(mut ib) => {
                    let buf = ib.buffer_mut();
                    let frames = (buf.len() / 4).min(AAC_FRAME * 4).min(until.min(total_audio).saturating_sub(*pos));
                    let pcm = audio_block(p, &plan, *pos as u64, frames);
                    for (i, v) in pcm.iter().enumerate() {
                        let b = ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes();
                        buf[i * 2].write(b[0]);
                        buf[i * 2 + 1].write(b[1]);
                    }
                    let pts = (*pos as u64) * 1_000_000 / SAMPLE_RATE as u64;
                    *pos += frames;
                    let last = eos && *pos >= total_audio;
                    enc.queue_input_buffer(ib, 0, frames * 4, pts, if last { FLAG_END_OF_STREAM } else { 0 }).map_err(|e| e.to_string())?;
                    if last {
                        *eos_sent = true;
                    }
                    if frames == 0 && !last {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    };

    let total_frames = plan.total_frames().max(1);
    let mut cache = FrameCache::new(p, &plan, true);
    let mut yuv: HashMap<usize, Arc<Vec<u8>>> = HashMap::new();
    let mut k = 0u64;
    // Frames to encode: every output frame, plus a copy of the last one just
    // before the end so the final drawing keeps its full duration.
    let last_frame = plan.spans.last().map(|s| s.0).unwrap_or(0);
    let end_us = total_frames * 1_000_000 / fps as u64;
    let jobs = plan.spans.iter().flat_map(|&(f, n)| std::iter::repeat_n(f, n as usize)).map(Some).chain(std::iter::once(None));
    for job in jobs {
        let (frame, pts) = match job {
            Some(f) => (f, k * 1_000_000 / fps as u64),
            None => (last_frame, end_us.saturating_sub(1_000).max((k.saturating_sub(1)) * 1_000_000 / fps as u64 + 1)),
        };
        let data = match yuv.get(&frame) {
            Some(d) => d.clone(),
            None => {
                let d = Arc::new(to_yuv(&cache.get(frame), layout));
                if yuv.len() > 64 {
                    yuv.clear();
                }
                yuv.insert(frame, d.clone());
                d
            }
        };
        // Wait for an input buffer, draining output meanwhile.
        let started = Instant::now();
        loop {
            match venc.dequeue_input_buffer(TIMEOUT).map_err(|e| e.to_string())? {
                DequeuedInputBufferResult::Buffer(mut ib) => {
                    let buf = ib.buffer_mut();
                    if buf.len() < data.len() {
                        return Err(format!("el codificador da búferes de {} bytes y el fotograma ocupa {}", buf.len(), data.len()));
                    }
                    for (d, s) in buf.iter_mut().zip(data.iter()) {
                        d.write(*s);
                    }
                    venc.queue_input_buffer(ib, 0, data.len(), pts, 0).map_err(|e| e.to_string())?;
                    break;
                }
                DequeuedInputBufferResult::TryAgainLater => {
                    video_done |= drain(&venc, &mut mux, false, Duration::ZERO)?;
                    if started.elapsed() > STALL_LIMIT {
                        return Err("el codificador de video dejó de responder".into());
                    }
                }
            }
        }
        if job.is_some() {
            k += 1;
        }
        if let Some(enc) = &aenc {
            let until = plan.sample_of_frame(k) as usize;
            feed_audio(enc, until, false, &mut audio_pos, &mut audio_eos_sent)?;
            audio_done |= drain(enc, &mut mux, true, Duration::ZERO)?;
        }
        video_done |= drain(&venc, &mut mux, false, Duration::ZERO)?;
        if !progress(k as f32 / total_frames as f32 * 0.97) {
            return Err(CANCELLED.into());
        }
    }

    // End of stream: flush both encoders.
    let started = Instant::now();
    loop {
        match venc.dequeue_input_buffer(TIMEOUT).map_err(|e| e.to_string())? {
            DequeuedInputBufferResult::Buffer(ib) => {
                venc.queue_input_buffer(ib, 0, 0, end_us, FLAG_END_OF_STREAM).map_err(|e| e.to_string())?;
                break;
            }
            DequeuedInputBufferResult::TryAgainLater => {
                video_done |= drain(&venc, &mut mux, false, Duration::ZERO)?;
                if started.elapsed() > STALL_LIMIT {
                    return Err("el codificador de video dejó de responder".into());
                }
            }
        }
    }
    let deadline = Instant::now() + STALL_LIMIT;
    while !(video_done && audio_done) {
        if Instant::now() > deadline {
            return Err("el codificador no terminó a tiempo".into());
        }
        if !progress(0.98) {
            return Err(CANCELLED.into());
        }
        if let Some(enc) = &aenc {
            if !audio_eos_sent {
                feed_audio(enc, usize::MAX, true, &mut audio_pos, &mut audio_eos_sent)?;
            }
            audio_done |= drain(enc, &mut mux, true, TIMEOUT)?;
        }
        video_done |= drain(&venc, &mut mux, false, TIMEOUT)?;
    }
    let _ = venc.stop();
    if let Some(enc) = &aenc {
        let _ = enc.stop();
    }
    if !mux.started {
        return Err("el codificador no produjo video".into());
    }
    mux.finish()?;
    drop(file);
    progress(1.0);
    Ok(())
}
