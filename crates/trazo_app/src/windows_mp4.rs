//! MP4 (H.264 + AAC) on Windows through Media Foundation, the encoder the
//! system already has (hardware when available).

#![cfg(windows)]

use std::path::Path;

use trazo_core::export::{audio_block, mp4_bitrate, plan, ExportSettings, FrameCache, CANCELLED};
use trazo_core::Project;
use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

/// Media Foundation time unit: 100 ns.
const SECOND: i64 = 10_000_000;
const SAMPLE_RATE: u32 = trazo_core::audio::SAMPLE_RATE;

fn err(e: windows::core::Error) -> String {
    format!("Media Foundation: {e}")
}

/// Two 32-bit values packed the way MF stores sizes and ratios.
fn pair(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn media_type(major: &GUID, sub: &GUID) -> Result<IMFMediaType, String> {
    unsafe {
        let t = MFCreateMediaType().map_err(err)?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, major).map_err(err)?;
        t.SetGUID(&MF_MT_SUBTYPE, sub).map_err(err)?;
        Ok(t)
    }
}

/// Media Foundation started for this thread; shut down when dropped.
struct Session {
    com: bool,
}

impl Session {
    fn start() -> Result<Self, String> {
        unsafe {
            // S_OK or S_FALSE (already initialised) both need a matching uninit.
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                if com {
                    CoUninitialize();
                }
                // Windows "N" editions ship without Media Foundation.
                return Err(tf!("Windows no tiene el codificador de video ({}). Exporta en GIF o PNG, o instala el «Media Feature Pack».", e.code()));
            }
            Ok(Session { com })
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

/// The file being written; removed unless it was finished.
struct Partial(std::path::PathBuf, bool);

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.1 {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

pub fn export_mp4(p: &Project, s: &ExportSettings, path: &Path, progress: &mut dyn FnMut(f32) -> bool) -> Result<(), String> {
    let plan = plan(p, s);
    // H.264 needs even sizes: drop the last column/row if needed.
    let (src_w, src_h) = (plan.width as usize, plan.height as usize);
    let (w, h) = (src_w & !1, src_h & !1);
    if w < 2 || h < 2 {
        return Err(crate::i18n::tr("demasiado pequeño para MP4").into());
    }
    let fps = plan.fps.max(1);
    let with_audio = s.include_audio && plan.has_audio;
    let _session = Session::start()?;
    // Written next to the target and renamed at the end, so a cancelled or
    // failed export never leaves a broken file (or replaces a good one).
    let mut part = Partial(path.with_extension("mp4.part"), false);

    let writer = unsafe {
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1).map_err(err)?;
        let attrs = attrs.ok_or("Media Foundation: atributos")?;
        attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1).map_err(err)?;
        attrs.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4).map_err(err)?;
        let url = HSTRING::from(part.0.as_os_str());
        MFCreateSinkWriterFromURL(PCWSTR(url.as_ptr()), None, &attrs).map_err(err)?
    };

    // Video: H.264 out, 32-bit BGRX in (the writer converts it).
    let video = unsafe {
        let out = media_type(&MFMediaType_Video, &MFVideoFormat_H264)?;
        out.SetUINT32(&MF_MT_AVG_BITRATE, mp4_bitrate(w as u32, h as u32, fps, s.quality)).map_err(err)?;
        out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(err)?;
        out.SetUINT64(&MF_MT_FRAME_SIZE, pair(w as u32, h as u32)).map_err(err)?;
        out.SetUINT64(&MF_MT_FRAME_RATE, pair(fps, 1)).map_err(err)?;
        out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1)).map_err(err)?;
        let stream = writer.AddStream(&out).map_err(err)?;
        let input = media_type(&MFMediaType_Video, &MFVideoFormat_RGB32)?;
        input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(err)?;
        input.SetUINT64(&MF_MT_FRAME_SIZE, pair(w as u32, h as u32)).map_err(err)?;
        input.SetUINT64(&MF_MT_FRAME_RATE, pair(fps, 1)).map_err(err)?;
        input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1)).map_err(err)?;
        // Positive stride: rows go top to bottom.
        input.SetUINT32(&MF_MT_DEFAULT_STRIDE, (w * 4) as u32).map_err(err)?;
        writer.SetInputMediaType(stream, &input, None).map_err(err)?;
        stream
    };

    // Audio: AAC out, 16-bit stereo PCM in.
    let audio = if with_audio {
        unsafe {
            let out = media_type(&MFMediaType_Audio, &MFAudioFormat_AAC)?;
            out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(err)?;
            out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, SAMPLE_RATE).map_err(err)?;
            out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(err)?;
            out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000).map_err(err)?;
            let stream = writer.AddStream(&out).map_err(err)?;
            let input = media_type(&MFMediaType_Audio, &MFAudioFormat_PCM)?;
            input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(err)?;
            input.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, SAMPLE_RATE).map_err(err)?;
            input.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(err)?;
            input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4).map_err(err)?;
            input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, SAMPLE_RATE * 4).map_err(err)?;
            writer.SetInputMediaType(stream, &input, None).map_err(err)?;
            Some(stream)
        }
    } else {
        None
    };

    unsafe { writer.BeginWriting().map_err(err)? };

    let write = |stream: u32, bytes: &[u8], time: i64, duration: i64| -> Result<(), String> {
        unsafe {
            let buffer = MFCreateMemoryBuffer(bytes.len() as u32).map_err(err)?;
            let mut ptr = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None).map_err(err)?;
            // SAFETY: Lock gave a writable buffer of at least `bytes.len()` bytes.
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            buffer.Unlock().map_err(err)?;
            buffer.SetCurrentLength(bytes.len() as u32).map_err(err)?;
            let sample = MFCreateSample().map_err(err)?;
            sample.AddBuffer(&buffer).map_err(err)?;
            sample.SetSampleTime(time).map_err(err)?;
            sample.SetSampleDuration(duration).map_err(err)?;
            writer.WriteSample(stream, &sample).map_err(err)
        }
    };

    let total = plan.total_frames().max(1);
    let total_audio = if with_audio { plan.audio_frames() } else { 0 };
    let mut cache = FrameCache::new(p, &plan, true);
    let mut bgra = vec![0u8; w * h * 4];
    let mut last: Option<usize> = None;
    let mut audio_pos = 0u64;
    let mut k = 0u64;
    let frame_time = |k: u64| (k as i64 * SECOND) / fps as i64;
    for &(frame, count) in &plan.spans {
        if last != Some(frame) {
            // RGBA (not premultiplied) → BGRX, cropped to even size.
            let rgba = cache.get(frame);
            for y in 0..h {
                let src = &rgba[y * src_w * 4..y * src_w * 4 + w * 4];
                let dst = &mut bgra[y * w * 4..(y + 1) * w * 4];
                for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                    d.copy_from_slice(&[s[2], s[1], s[0], 255]);
                }
            }
            last = Some(frame);
        }
        for _ in 0..count {
            write(video, &bgra, frame_time(k), frame_time(k + 1) - frame_time(k))?;
            k += 1;
            if let Some(stream) = audio {
                let until = plan.sample_of_frame(k).min(total_audio);
                if until > audio_pos {
                    let n = (until - audio_pos) as usize;
                    let mix = audio_block(p, &plan, audio_pos, n);
                    let pcm: Vec<u8> = mix.iter().flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
                    let t = audio_pos as i64 * SECOND / SAMPLE_RATE as i64;
                    write(stream, &pcm, t, n as i64 * SECOND / SAMPLE_RATE as i64)?;
                    audio_pos = until;
                }
            }
            if !progress(k as f32 / total as f32 * 0.97) {
                return Err(CANCELLED.into());
            }
        }
    }
    unsafe { writer.Finalize().map_err(err)? };
    drop(writer);
    std::fs::rename(&part.0, path).map_err(|e| e.to_string())?;
    part.1 = true;
    progress(1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use trazo_core::audio::{AudioAsset, AudioClip, AudioTrack};

    /// Real export through the system encoder: a short animation with sound.
    #[test]
    fn exports_a_playable_mp4_with_sound() {
        let mut p = trazo_core::templates::Template::BouncingBall.build("mp4");
        // One second of a tone on a track.
        let samples: Vec<f32> = (0..48_000 * 2).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.3).collect();
        let aid = p.alloc_id();
        p.audio.assets.insert(aid, std::sync::Arc::new(AudioAsset::new("tono", samples)));
        let (tid, cid) = (p.alloc_id(), p.alloc_id());
        let mut t = AudioTrack::new(tid, "Música");
        t.clips.push(AudioClip { id: cid, asset: aid, start: 0, offset: 0, len: 48_000, gain: 1.0, fade_in: 0, fade_out: 0, muted: false });
        p.audio.tracks.push(t);
        // Odd size on purpose: must be cropped to even.
        p.canvas.width = 641;
        p.canvas.height = 361;
        for f in &mut p.frames {
            for c in f.cels.values_mut() {
                *c = trazo_core::TiledImage::new(641, 361);
            }
        }
        let dir = std::env::temp_dir().join(format!("trazo_mp4_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prueba.mp4");
        let s = ExportSettings { include_audio: true, ..Default::default() };
        let mut last = 0.0;
        export_mp4(&p, &s, &path, &mut |v| {
            last = v;
            true
        })
        .unwrap();
        assert_eq!(last, 1.0);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() > 2_000, "{} bytes", bytes.len());
        assert_eq!(&bytes[4..8], b"ftyp", "an MP4 file");
        std::fs::remove_dir_all(&dir).ok();
    }
}
