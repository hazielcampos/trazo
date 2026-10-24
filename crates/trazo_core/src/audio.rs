//! Audio: tracks, clips, the mixer, decoding/encoding and resampling.
//!
//! All audio inside a project is stereo `f32` at [`SAMPLE_RATE`]. Imported
//! files are converted once on import. Audio data lives in immutable
//! [`AudioAsset`]s behind `Arc`, and clips only reference them, so editing
//! clips (move, trim, split, fade) is cheap and undo just swaps the small
//! track list.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::io::{ProjectError, Result};

/// Project audio sample rate (frames per second, stereo).
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// Longest audio file accepted (protects memory on phones).
pub const MAX_SECONDS: u64 = 15 * 60;
/// Samples per waveform peak block.
pub const PEAK_BLOCK: usize = 256;

pub type AssetId = u64;
pub type TrackId = u64;
pub type ClipId = u64;

/// Min/max of the mono mix per [`PEAK_BLOCK`] frames, for drawing waveforms.
#[derive(Debug, Clone, Default)]
pub struct Peaks {
    pub min: Vec<f32>,
    pub max: Vec<f32>,
}

/// Decoded audio: stereo interleaved samples at [`SAMPLE_RATE`].
#[derive(Debug, Clone)]
pub struct AudioAsset {
    pub name: String,
    pub samples: Arc<[f32]>,
    pub peaks: Arc<Peaks>,
}

impl AudioAsset {
    pub fn new(name: impl Into<String>, samples: Vec<f32>) -> Self {
        let peaks = compute_peaks(&samples);
        Self { name: name.into(), samples: samples.into(), peaks: Arc::new(peaks) }
    }

    /// Length in sample frames.
    pub fn frames(&self) -> u64 {
        (self.samples.len() / CHANNELS) as u64
    }

    pub fn bytes(&self) -> usize {
        self.samples.len() * 4
    }
}

fn compute_peaks(samples: &[f32]) -> Peaks {
    let mut p = Peaks::default();
    for block in samples.chunks(PEAK_BLOCK * CHANNELS) {
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for fr in block.chunks_exact(CHANNELS) {
            let m = (fr[0] + fr[1]) * 0.5;
            lo = lo.min(m);
            hi = hi.max(m);
        }
        p.min.push(lo);
        p.max.push(hi);
    }
    p
}

/// A piece of an asset placed on a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub id: ClipId,
    pub asset: AssetId,
    /// Position on the timeline, in sample frames (can be negative = starts before 0).
    pub start: i64,
    /// First sample frame used from the asset.
    pub offset: u64,
    /// Length in sample frames.
    pub len: u64,
    pub gain: f32,
    pub fade_in: u64,
    pub fade_out: u64,
    #[serde(default)]
    pub muted: bool,
}

impl AudioClip {
    pub fn end(&self) -> i64 {
        self.start + self.len as i64
    }

    /// Gain envelope (clip gain × fades) at frame `i` inside the clip.
    #[inline]
    pub fn envelope(&self, i: u64) -> f32 {
        let mut g = self.gain;
        if self.fade_in > 0 && i < self.fade_in {
            g *= i as f32 / self.fade_in as f32;
        }
        if self.fade_out > 0 && i + self.fade_out > self.len {
            let left = self.len.saturating_sub(i);
            g *= left as f32 / self.fade_out as f32;
        }
        g
    }

    /// Split at timeline position `at`. The right part gets `new_id`.
    pub fn split(&self, at: i64, new_id: ClipId) -> Option<(AudioClip, AudioClip)> {
        if at <= self.start || at >= self.end() {
            return None;
        }
        let cut = (at - self.start) as u64;
        let total = self.offset + self.len;
        let mut left = AudioClip { len: cut, fade_out: 0, ..self.clone() };
        let mut right = AudioClip { id: new_id, start: at, offset: self.offset + cut, len: self.len - cut, fade_in: 0, ..self.clone() };
        left.clamp_to(total);
        right.clamp_to(total);
        Some((left, right))
    }

    /// Keep `len` and `offset` inside the asset and fades inside the clip.
    pub fn clamp_to(&mut self, asset_frames: u64) {
        let asset_frames = asset_frames.max(1);
        self.offset = self.offset.min(asset_frames.saturating_sub(1));
        self.len = self.len.clamp(1, asset_frames - self.offset);
        self.fade_in = self.fade_in.min(self.len);
        self.fade_out = self.fade_out.min(self.len - self.fade_in);
        self.gain = self.gain.clamp(0.0, 4.0);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    pub id: TrackId,
    pub name: String,
    pub gain: f32,
    /// -1 = left, 0 = center, 1 = right.
    #[serde(default)]
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub clips: Vec<AudioClip>,
}

impl AudioTrack {
    pub fn new(id: TrackId, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), gain: 1.0, pan: 0.0, muted: false, solo: false, clips: Vec::new() }
    }

    pub fn clip(&self, id: ClipId) -> Option<&AudioClip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn clip_mut(&mut self, id: ClipId) -> Option<&mut AudioClip> {
        self.clips.iter_mut().find(|c| c.id == id)
    }
}

/// Mouth shapes for lip-sync (the classic Preston Blair set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phoneme {
    Rest,
    MBP,
    AI,
    E,
    O,
    U,
    FV,
    L,
    WQ,
    Etc,
}

impl Phoneme {
    pub const ALL: [Phoneme; 10] =
        [Phoneme::Rest, Phoneme::MBP, Phoneme::AI, Phoneme::E, Phoneme::O, Phoneme::U, Phoneme::FV, Phoneme::L, Phoneme::WQ, Phoneme::Etc];

    pub fn label(self) -> &'static str {
        match self {
            Phoneme::Rest => "reposo",
            Phoneme::MBP => "MBP",
            Phoneme::AI => "AI",
            Phoneme::E => "E",
            Phoneme::O => "O",
            Phoneme::U => "U",
            Phoneme::FV => "FV",
            Phoneme::L => "L",
            Phoneme::WQ => "WQ",
            Phoneme::Etc => "etc",
        }
    }
}

/// A labelled point in time (dialogue, beat, phoneme).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: u64,
    /// Position in sample frames (stays put if the fps changes).
    pub at: i64,
    pub text: String,
    #[serde(default)]
    pub phoneme: Option<Phoneme>,
}

/// Every track plus the audio data they use.
#[derive(Debug, Clone)]
pub struct AudioTimeline {
    pub tracks: Vec<AudioTrack>,
    /// Sorted by position.
    pub markers: Vec<Marker>,
    /// Imported audio. Assets are never modified; unused ones are dropped on save.
    pub assets: BTreeMap<AssetId, Arc<AudioAsset>>,
    pub master_gain: f32,
}

impl Default for AudioTimeline {
    fn default() -> Self {
        Self { tracks: Vec::new(), markers: Vec::new(), assets: BTreeMap::new(), master_gain: 1.0 }
    }
}

impl AudioTimeline {
    pub fn is_empty(&self) -> bool {
        self.tracks.iter().all(|t| t.clips.is_empty())
    }

    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Track and clip that hold clip `id`.
    pub fn find_clip(&self, id: ClipId) -> Option<(usize, &AudioClip)> {
        self.tracks.iter().enumerate().find_map(|(i, t)| t.clip(id).map(|c| (i, c)))
    }

    /// End of the last clip, in sample frames.
    pub fn end(&self) -> i64 {
        self.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.end()).max().unwrap_or(0)
    }

    /// Drop assets that are neither used by a clip nor listed in `keep`
    /// (assets still needed by undo history). Returns the bytes freed.
    pub fn collect_garbage(&mut self, keep: &std::collections::BTreeSet<AssetId>) -> usize {
        let used = self.used_assets();
        let mut freed = 0;
        self.assets.retain(|id, a| {
            let k = used.contains(id) || keep.contains(id);
            if !k {
                freed += a.bytes();
            }
            k
        });
        freed
    }

    /// Assets referenced by some clip.
    pub fn used_assets(&self) -> std::collections::BTreeSet<AssetId> {
        self.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.asset)).collect()
    }

    /// Mix every audible track into `out` (stereo interleaved), starting at
    /// timeline frame `start`. `out` is overwritten and clamped to -1..1.
    pub fn mix(&self, start: i64, out: &mut [f32]) {
        self.mix_with_gain(start, out, 1.0);
    }

    /// Like [`Self::mix`] with an extra output gain applied before clamping.
    pub fn mix_with_gain(&self, start: i64, out: &mut [f32], extra_gain: f32) {
        out.fill(0.0);
        let frames = (out.len() / CHANNELS) as i64;
        let end = start + frames;
        let any_solo = self.tracks.iter().any(|t| t.solo);
        for track in &self.tracks {
            if track.muted || (any_solo && !track.solo) || track.gain <= 0.0 {
                continue;
            }
            let pan = track.pan.clamp(-1.0, 1.0);
            let (pl, pr) = ((1.0 - pan).min(1.0) * track.gain, (1.0 + pan).min(1.0) * track.gain);
            for clip in &track.clips {
                if clip.muted || clip.end() <= start || clip.start >= end {
                    continue;
                }
                let Some(asset) = self.assets.get(&clip.asset) else { continue };
                let s = &asset.samples;
                let from = clip.start.max(start);
                let to = clip.end().min(end);
                for t in from..to {
                    let i = (t - clip.start) as u64;
                    let src = ((clip.offset + i) as usize) * CHANNELS;
                    if src + 1 >= s.len() {
                        break;
                    }
                    let g = clip.envelope(i);
                    let o = ((t - start) as usize) * CHANNELS;
                    out[o] += s[src] * g * pl;
                    out[o + 1] += s[src + 1] * g * pr;
                }
            }
        }
        let master = self.master_gain * extra_gain;
        for v in out.iter_mut() {
            *v = (*v * master).clamp(-1.0, 1.0);
        }
    }
}

/// Mix `start..end` (sample frames) in one go: used for exporting.
pub fn render_mix(tl: &AudioTimeline, start: i64, end: i64) -> Vec<f32> {
    let frames = (end - start).max(0) as usize;
    let mut out = vec![0.0; frames * CHANNELS];
    // Mix in blocks to keep the per-call work bounded.
    const BLOCK: usize = 48_000;
    let mut done = 0;
    while done < frames {
        let n = BLOCK.min(frames - done);
        tl.mix(start + done as i64, &mut out[done * CHANNELS..(done + n) * CHANNELS]);
        done += n;
    }
    out
}

/// New samples scaled so the loudest peak reaches `target` (e.g. 0.95).
pub fn normalized(samples: &[f32], target: f32) -> Option<Vec<f32>> {
    let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak < 1e-4 {
        return None;
    }
    let k = target / peak;
    Some(samples.iter().map(|v| v * k).collect())
}

/// Places where the sound starts after silence (for splitting takes),
/// using the precomputed peaks. Returns positions in asset sample frames.
pub fn sound_starts(peaks: &Peaks, threshold: f32, min_silence_frames: u64) -> Vec<u64> {
    let min_blocks = (min_silence_frames as usize / PEAK_BLOCK).max(1);
    let mut out = Vec::new();
    let mut quiet = 0usize;
    for (i, (lo, hi)) in peaks.min.iter().zip(&peaks.max).enumerate() {
        let loud = hi.abs().max(lo.abs()) > threshold;
        if loud {
            if quiet >= min_blocks && i > 0 {
                out.push((i * PEAK_BLOCK) as u64);
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    out
}

/// Timeline ticks ↔ audio sample frames.
pub fn ticks_to_samples(ticks: f64, fps: u32) -> i64 {
    (ticks * SAMPLE_RATE as f64 / fps.max(1) as f64).round() as i64
}

pub fn samples_to_ticks(samples: i64, fps: u32) -> f64 {
    samples as f64 * fps.max(1) as f64 / SAMPLE_RATE as f64
}

/// Convert interleaved audio with any channel count and rate to project format.
pub fn to_project_format(input: &[f32], channels: usize, rate: u32) -> Vec<f32> {
    let channels = channels.max(1);
    let frames = input.len() / channels;
    // Down/up-mix to stereo first.
    let mut stereo = Vec::with_capacity(frames * 2);
    for f in input.chunks_exact(channels) {
        let (l, r) = if channels == 1 { (f[0], f[0]) } else { (f[0], f[1]) };
        stereo.push(l);
        stereo.push(r);
    }
    resample_stereo(&stereo, rate, SAMPLE_RATE)
}

/// Cubic (Catmull-Rom) resampling of stereo interleaved audio.
pub fn resample_stereo(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.len() < 4 {
        return input.to_vec();
    }
    let n_in = input.len() / 2;
    let ratio = from as f64 / to as f64;
    let n_out = ((n_in as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(n_out * 2);
    let at = |i: isize, c: usize| -> f32 { input[(i.clamp(0, n_in as isize - 1) as usize) * 2 + c] };
    for k in 0..n_out {
        let pos = k as f64 * ratio;
        let i = pos.floor() as isize;
        let t = (pos - i as f64) as f32;
        for c in 0..2 {
            let (p0, p1, p2, p3) = (at(i - 1, c), at(i, c), at(i + 1, c), at(i + 2, c));
            let v = p1 + 0.5 * t * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)));
            out.push(v);
        }
    }
    out
}

/// Decode WAV, MP3, OGG/Vorbis, FLAC, AAC/M4A… into project format.
pub fn decode_audio(bytes: Vec<u8>) -> Result<Vec<f32>> {
    decode_source(Box::new(Cursor::new(bytes)))
}

/// Decode the sound of a file (audio, or the audio track of an MP4/MOV
/// video) without loading the whole file in memory.
pub fn decode_audio_file(path: &std::path::Path) -> Result<Vec<f32>> {
    decode_source(Box::new(std::fs::File::open(path)?))
}

fn decode_source(source: Box<dyn symphonia::core::io::MediaSource>) -> Result<Vec<f32>> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let bad = |e: &dyn std::fmt::Display| ProjectError::Invalid(format!("audio no compatible: {e}"));
    let mss = MediaSourceStream::new(source, Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(&Hint::new(), mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| bad(&e))?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| bad(&"sin pista de audio"))?;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| bad(&"códec desconocido"))?;
    let mut decoder = symphonia::default::get_codecs().make_audio_decoder(params, &AudioDecoderOptions::default()).map_err(|e| bad(&e))?;
    let track_id = track.id;

    let mut samples: Vec<f32> = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    let (mut rate, mut channels) = (0u32, 0usize);
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::IoError(_)) => break,
            Err(e) => return Err(bad(&e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                rate = buf.spec().rate();
                channels = buf.spec().channels().count();
                chunk.resize(buf.samples_interleaved(), 0.0);
                buf.copy_to_slice_interleaved(&mut chunk);
                samples.extend_from_slice(&chunk);
                let limit = MAX_SECONDS as usize * rate.max(1) as usize * channels.max(1);
                if samples.len() > limit {
                    return Err(ProjectError::Invalid(format!("el audio dura más de {} minutos", MAX_SECONDS / 60)));
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(bad(&e)),
        }
    }
    if samples.is_empty() || rate == 0 {
        return Err(bad(&"archivo vacío"));
    }
    Ok(to_project_format(&samples, channels, rate))
}

/// 16-bit stereo WAV at the project rate (used inside project files).
pub fn encode_wav(samples: &[f32]) -> Result<Vec<u8>> {
    let spec = hound::WavSpec { channels: 2, sample_rate: SAMPLE_RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut buf, spec).map_err(|e| ProjectError::Invalid(e.to_string()))?;
        for &s in samples {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).map_err(|e| ProjectError::Invalid(e.to_string()))?;
        }
        w.finalize().map_err(|e| ProjectError::Invalid(e.to_string()))?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, v: f32) -> Arc<AudioAsset> {
        Arc::new(AudioAsset::new("t", vec![v; frames * 2]))
    }

    fn timeline() -> AudioTimeline {
        let mut tl = AudioTimeline::default();
        tl.assets.insert(1, tone(1000, 0.5));
        let mut t = AudioTrack::new(10, "A");
        t.clips.push(AudioClip { id: 100, asset: 1, start: 100, offset: 0, len: 1000, gain: 1.0, fade_in: 0, fade_out: 0, muted: false });
        tl.tracks.push(t);
        tl
    }

    #[test]
    fn mix_places_clip_in_time() {
        let tl = timeline();
        let mut out = vec![0.0; 2 * 200];
        tl.mix(0, &mut out);
        assert_eq!(out[2 * 99], 0.0);
        assert_eq!(out[2 * 100], 0.5);
        assert_eq!(out[2 * 150 + 1], 0.5);
    }

    #[test]
    fn mute_solo_and_overlay() {
        let mut tl = timeline();
        let mut t2 = AudioTrack::new(11, "B");
        t2.clips.push(AudioClip { id: 101, asset: 1, start: 100, offset: 0, len: 1000, gain: 1.0, fade_in: 0, fade_out: 0, muted: false });
        tl.tracks.push(t2);
        let mut out = vec![0.0; 2 * 10];
        tl.mix(100, &mut out);
        assert!((out[0] - 1.0).abs() < 1e-6, "two tracks add up");
        tl.tracks[1].solo = true;
        tl.mix(100, &mut out);
        assert_eq!(out[0], 0.5, "solo hides other tracks");
        tl.tracks[1].muted = true;
        tl.tracks[1].solo = false;
        tl.mix(100, &mut out);
        assert_eq!(out[0], 0.5);
    }

    #[test]
    fn fades_and_split() {
        let mut tl = timeline();
        tl.tracks[0].clips[0].fade_in = 100;
        let mut out = vec![0.0; 2 * 100];
        tl.mix(100, &mut out);
        assert_eq!(out[0], 0.0);
        assert!((out[2 * 50] - 0.25).abs() < 1e-3);
        let c = tl.tracks[0].clips[0].clone();
        let (a, b) = c.split(600, 200).unwrap();
        assert_eq!(a.len + b.len, c.len);
        assert_eq!(b.offset, 500);
        assert_eq!(b.start, 600);
        assert!(c.split(100, 1).is_none());
    }

    #[test]
    fn resample_changes_length() {
        let input = vec![0.1f32; 44_100 * 2];
        let out = resample_stereo(&input, 44_100, 48_000);
        assert!((out.len() as i64 / 2 - 48_000).abs() <= 2);
        assert!((out[1000] - 0.1).abs() < 1e-4);
    }

    #[test]
    fn wav_roundtrip_through_decoder() {
        let samples: Vec<f32> = (0..4800).flat_map(|i| {
            let v = (i as f32 * 0.05).sin() * 0.5;
            [v, -v]
        }).collect();
        let wav = encode_wav(&samples).unwrap();
        let back = decode_audio(wav).unwrap();
        assert_eq!(back.len(), samples.len());
        assert!((back[200] - samples[200]).abs() < 1e-3);
    }

    #[test]
    fn mono_44k_is_converted() {
        let spec = hound::WavSpec { channels: 1, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut buf, spec).unwrap();
            for _ in 0..44_100 {
                w.write_sample(8000i16).unwrap();
            }
            w.finalize().unwrap();
        }
        let out = decode_audio(buf.into_inner()).unwrap();
        assert!((out.len() as i64 / 2 - 48_000).abs() <= 2);
        assert!((out[0] - out[1]).abs() < 1e-6, "mono copied to both channels");
    }

    #[test]
    fn render_normalize_and_silence() {
        let tl = timeline();
        let mix = render_mix(&tl, 0, 2000);
        assert_eq!(mix.len(), 4000);
        assert_eq!(mix[2 * 500], 0.5);
        let n = normalized(&[0.1, -0.2, 0.05], 1.0).unwrap();
        assert!((n[1] + 1.0).abs() < 1e-6);
        assert!(normalized(&[0.0; 10], 1.0).is_none());
        // loud, silence (4 blocks), loud
        let p = Peaks { min: vec![-0.5, 0.0, 0.0, 0.0, 0.0, -0.5], max: vec![0.5, 0.0, 0.0, 0.0, 0.0, 0.5] };
        assert_eq!(sound_starts(&p, 0.1, (PEAK_BLOCK * 3) as u64), vec![(5 * PEAK_BLOCK) as u64]);
    }
}
