//! Audio devices: playback (the master clock for animation playback),
//! scrubbing and microphone recording.
//!
//! The output callback mixes the project's audio timeline itself, so the
//! sample position it advances is the most accurate clock available. The UI
//! asks for the *audible* position: where the callback is, plus the time since
//! that callback, minus the device's output latency (Bluetooth can be 200 ms).
//!
//! Real-time rules in the callbacks: no allocation after warm-up, no blocking
//! locks (`try_lock` only), microphone samples go through a lock-free ring.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use trazo_core::audio::{to_project_format, AudioTimeline, CHANNELS, SAMPLE_RATE};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

/// Largest device buffer we pre-allocate for (frames).
const MAX_BLOCK: usize = 16_384;

/// State shared with the audio thread.
struct Shared {
    timeline: Mutex<Arc<AudioTimeline>>,
    playing: AtomicBool,
    /// Next project frame to be mixed.
    position: AtomicI64,
    /// When >= 0, playback stops automatically at this position (scrubbing).
    stop_at: AtomicI64,
    /// Bumped on every seek so the audio thread drops its fractional position.
    seek_gen: AtomicU64,
    volume: AtomicU32,
    /// Loop region in project frames (`loop_end <= loop_start` = no loop).
    loop_start: AtomicI64,
    loop_end: AtomicI64,
    /// Position and time (µs since `epoch`) at the start of the last callback.
    cb_pos: AtomicI64,
    cb_time_us: AtomicU64,
    /// Output latency reported by the device, µs.
    latency_us: AtomicU64,
    epoch: Instant,
}

impl Shared {
    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    fn now_us(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64
    }
}

pub struct AudioEngine {
    shared: Arc<Shared>,
    _output: cpal::Stream,
    recorder: Option<Recorder>,
    /// Extra manual A/V offset in ms (positive = picture later).
    pub av_offset_ms: f32,
}

/// Microphone capture: armed during the countdown, capturing after.
struct Recorder {
    _stream: cpal::Stream,
    consumer: rtrb::Consumer<f32>,
    capturing: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    level: Arc<AtomicU32>,
    take: Vec<f32>,
    rate: u32,
    channels: usize,
}

impl AudioEngine {
    /// Open the default output device. Fails if there is no sound device.
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no hay salida de audio")?;
        let supported = preferred_output_config(&device)?;
        let config = supported.config();
        let shared = Arc::new(Shared {
            timeline: Mutex::new(Arc::new(AudioTimeline::default())),
            playing: AtomicBool::new(false),
            position: AtomicI64::new(0),
            stop_at: AtomicI64::new(-1),
            seek_gen: AtomicU64::new(0),
            volume: AtomicU32::new(1.0f32.to_bits()),
            loop_start: AtomicI64::new(0),
            loop_end: AtomicI64::new(0),
            cb_pos: AtomicI64::new(0),
            cb_time_us: AtomicU64::new(0),
            latency_us: AtomicU64::new(0),
            epoch: Instant::now(),
        });
        use cpal::SampleFormat as F;
        let s = shared.clone();
        let stream = match supported.sample_format() {
            F::F32 => build_output::<f32>(&device, &config, s),
            F::I16 => build_output::<i16>(&device, &config, s),
            F::U16 => build_output::<u16>(&device, &config, s),
            F::I32 => build_output::<i32>(&device, &config, s),
            F::U8 => build_output::<u8>(&device, &config, s),
            F::F64 => build_output::<f64>(&device, &config, s),
            other => return Err(format!("formato de salida no soportado: {other:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self { shared, _output: stream, recorder: None, av_offset_ms: 0.0 })
    }

    /// Give the audio thread the latest tracks (cheap: audio data is shared).
    pub fn set_timeline(&self, tl: AudioTimeline) {
        let mut new = Arc::new(tl);
        if let Ok(mut t) = self.shared.timeline.lock() {
            std::mem::swap(&mut *t, &mut new);
        }
        // `new` now holds the old timeline and is dropped here, outside the lock.
    }

    pub fn set_volume(&self, v: f32) {
        self.shared.volume.store(v.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }

    pub fn seek(&self, sample: i64) {
        self.shared.position.store(sample, Ordering::Relaxed);
        self.shared.cb_pos.store(sample, Ordering::Relaxed);
        self.shared.cb_time_us.store(self.shared.now_us(), Ordering::Relaxed);
        self.shared.seek_gen.fetch_add(1, Ordering::AcqRel);
    }

    /// Loop `start..end` inside the audio thread (seamless, no overshoot).
    pub fn set_loop(&self, region: Option<(i64, i64)>) {
        let (a, b) = region.unwrap_or((0, 0));
        self.shared.loop_start.store(a, Ordering::Relaxed);
        self.shared.loop_end.store(b, Ordering::Relaxed);
    }

    pub fn play_from(&self, sample: i64) {
        self.shared.stop_at.store(-1, Ordering::Relaxed);
        self.seek(sample);
        self.shared.playing.store(true, Ordering::Release);
    }

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Release);
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }

    /// Output latency of the device in ms (for display).
    pub fn latency_ms(&self) -> f32 {
        self.shared.latency_us.load(Ordering::Relaxed) as f32 / 1000.0
    }

    /// The sample that is being heard right now (project frames).
    pub fn position(&self) -> i64 {
        let s = &self.shared;
        let base = s.cb_pos.load(Ordering::Relaxed);
        if !self.is_playing() {
            return s.position.load(Ordering::Relaxed);
        }
        let since = s.now_us().saturating_sub(s.cb_time_us.load(Ordering::Relaxed)) as f64 / 1e6;
        let latency = s.latency_us.load(Ordering::Relaxed) as f64 / 1e6 + self.av_offset_ms as f64 / 1000.0;
        let audible = base as f64 + (since - latency) * SAMPLE_RATE as f64;
        // Never ahead of what has actually been mixed.
        (audible as i64).min(s.position.load(Ordering::Relaxed))
    }

    /// Play a short piece of audio (one frame's worth) — "scrubbing".
    pub fn scrub(&self, start: i64, len: i64) {
        self.seek(start);
        self.shared.stop_at.store(start + len.max(1), Ordering::Relaxed);
        self.shared.playing.store(true, Ordering::Release);
    }

    // ---- recording --------------------------------------------------------

    pub fn is_recording(&self) -> bool {
        self.recorder.as_ref().is_some_and(|r| r.capturing.load(Ordering::Relaxed))
    }

    /// Open the microphone without keeping anything yet (called at the start
    /// of the countdown, so the device is warm when recording begins).
    pub fn arm_recording(&mut self) -> Result<(), String> {
        if self.recorder.is_some() {
            return Ok(());
        }
        request_mic_permission()?;
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("no se encontró ningún micrófono")?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        // ~10 s of headroom between UI drains.
        let cap = config.sample_rate as usize * config.channels as usize * 10;
        let (producer, consumer) = rtrb::RingBuffer::<f32>::new(cap);
        let capturing = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let level = Arc::new(AtomicU32::new(0));
        use cpal::SampleFormat as F;
        let (c, d, l) = (capturing.clone(), dropped.clone(), level.clone());
        let stream = match supported.sample_format() {
            F::F32 => build_input::<f32>(&device, &config, producer, c, d, l),
            F::I16 => build_input::<i16>(&device, &config, producer, c, d, l),
            F::U16 => build_input::<u16>(&device, &config, producer, c, d, l),
            F::I32 => build_input::<i32>(&device, &config, producer, c, d, l),
            F::U8 => build_input::<u8>(&device, &config, producer, c, d, l),
            F::F64 => build_input::<f64>(&device, &config, producer, c, d, l),
            other => return Err(format!("formato de micrófono no soportado: {other:?}")),
        }?;
        stream.play().map_err(|e| format!("no se pudo abrir el micrófono: {e}"))?;
        self.recorder = Some(Recorder {
            _stream: stream,
            consumer,
            capturing,
            dropped,
            level,
            take: Vec::new(),
            rate: config.sample_rate,
            channels: config.channels as usize,
        });
        Ok(())
    }

    /// Start keeping microphone samples (after `arm_recording`).
    pub fn begin_capture(&mut self) {
        if let Some(r) = &mut self.recorder {
            while r.consumer.pop().is_ok() {}
            r.capturing.store(true, Ordering::Release);
        }
    }

    /// Move captured samples from the ring into the take. Call every UI frame.
    pub fn drain_recording(&mut self) {
        let Some(r) = &mut self.recorder else { return };
        let n = r.consumer.slots();
        if n == 0 {
            return;
        }
        if let Ok(chunk) = r.consumer.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            let max = trazo_core::audio::MAX_SECONDS as usize * r.rate as usize * r.channels;
            if r.take.len() < max {
                r.take.extend_from_slice(a);
                r.take.extend_from_slice(b);
            }
            chunk.commit_all();
        }
    }

    /// Microphone peak level 0..1 (for the meter).
    pub fn input_level(&self) -> f32 {
        self.recorder.as_ref().map(|r| f32::from_bits(r.level.load(Ordering::Relaxed))).unwrap_or(0.0)
    }

    /// Seconds captured so far.
    pub fn recorded_seconds(&self) -> f32 {
        let Some(r) = &self.recorder else { return 0.0 };
        r.take.len() as f32 / (r.rate.max(1) as f32 * r.channels.max(1) as f32)
    }

    /// True if samples were lost because the UI didn't drain in time.
    pub fn recording_overflowed(&self) -> bool {
        self.recorder.as_ref().is_some_and(|r| r.dropped.load(Ordering::Relaxed) > 0)
    }

    /// Close the microphone and return the take in project format.
    pub fn stop_recording(&mut self) -> Option<Vec<f32>> {
        self.drain_recording();
        let r = self.recorder.take()?;
        let Recorder { _stream, take, rate, channels, .. } = r;
        drop(_stream);
        (!take.is_empty()).then(|| to_project_format(&take, channels, rate))
    }

    /// Close the microphone without keeping anything.
    pub fn cancel_recording(&mut self) {
        self.recorder = None;
    }
}

/// Prefer 48 kHz (no resampling at all); otherwise the device default.
fn preferred_output_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    let default = device.default_output_config().map_err(|e| e.to_string())?;
    if default.sample_rate() == SAMPLE_RATE {
        return Ok(default);
    }
    if let Ok(configs) = device.supported_output_configs() {
        for c in configs {
            if c.channels() >= 2
                && c.sample_format() == default.sample_format()
                && c.min_sample_rate() <= SAMPLE_RATE
                && c.max_sample_rate() >= SAMPLE_RATE
            {
                return Ok(c.with_sample_rate(SAMPLE_RATE));
            }
        }
    }
    Ok(default)
}

fn build_output<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: Arc<Shared>) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let out_channels = config.channels as usize;
    let step = SAMPLE_RATE as f64 / config.sample_rate as f64; // project frames per device frame
    let mut mix: Vec<f32> = vec![0.0; (MAX_BLOCK as f64 * step) as usize * CHANNELS + 16];
    let mut frac = 0.0f64;
    let mut gen = u64::MAX;
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let ts = info.timestamp();
                let lat = ts.playback.duration_since(ts.callback);
                shared.latency_us.store(lat.as_micros() as u64, Ordering::Relaxed);
                let frames = data.len() / out_channels.max(1);
                if !shared.playing.load(Ordering::Acquire) {
                    data.fill(T::from_sample(0.0f32));
                    return;
                }
                let g = shared.seek_gen.load(Ordering::Acquire);
                if g != gen {
                    gen = g;
                    frac = 0.0;
                }
                let pos = shared.position.load(Ordering::Relaxed);
                shared.cb_pos.store(pos, Ordering::Relaxed);
                shared.cb_time_us.store(shared.now_us(), Ordering::Relaxed);
                // Project frames needed for this callback (+2 for interpolation).
                let need = ((frames as f64) * step + frac).ceil() as usize + 2;
                if need * CHANNELS > mix.len() {
                    // Only if the device suddenly uses a huge buffer.
                    mix.resize(need * CHANNELS, 0.0);
                }
                let buf = &mut mix[..need * CHANNELS];
                let (ls, le) = (shared.loop_start.load(Ordering::Relaxed), shared.loop_end.load(Ordering::Relaxed));
                let looping = le > ls;
                match shared.timeline.try_lock() {
                    Ok(tl) => {
                        if looping && pos + need as i64 > le && pos < le {
                            // Mix up to the loop end, then continue from the loop start.
                            let first = (le - pos) as usize;
                            tl.mix_with_gain(pos, &mut buf[..first * CHANNELS], shared.volume());
                            let mut at = ls;
                            let mut done = first;
                            while done < need {
                                let n = ((le - ls) as usize).max(1).min(need - done);
                                tl.mix_with_gain(at, &mut buf[done * CHANNELS..(done + n) * CHANNELS], shared.volume());
                                done += n;
                                at = ls;
                            }
                        } else {
                            tl.mix_with_gain(pos, buf, shared.volume());
                        }
                    }
                    Err(_) => buf.fill(0.0), // UI is swapping the timeline: one silent block
                }
                let stop_at = shared.stop_at.load(Ordering::Relaxed);
                for f in 0..frames {
                    let p = frac + f as f64 * step;
                    let i = p.floor() as usize;
                    let t = (p - i as f64) as f32;
                    let (mut l, mut r) = if i + 1 < need {
                        let a = &buf[i * 2..i * 2 + 4];
                        (a[0] + (a[2] - a[0]) * t, a[1] + (a[3] - a[1]) * t)
                    } else {
                        (0.0, 0.0)
                    };
                    if stop_at >= 0 && pos + i as i64 >= stop_at {
                        l = 0.0;
                        r = 0.0;
                    }
                    let o = f * out_channels;
                    match out_channels {
                        1 => data[o] = T::from_sample((l + r) * 0.5),
                        _ => {
                            data[o] = T::from_sample(l);
                            data[o + 1] = T::from_sample(r);
                            for c in 2..out_channels {
                                data[o + c] = T::from_sample(0.0f32);
                            }
                        }
                    }
                }
                let advanced = frac + frames as f64 * step;
                let whole = advanced.floor();
                frac = advanced - whole;
                let mut new_pos = pos + whole as i64;
                if looping && pos < le && new_pos >= le {
                    new_pos = ls + (new_pos - le) % (le - ls).max(1);
                }
                // A seek from the UI wins over our update.
                let _ = shared.position.compare_exchange(pos, new_pos, Ordering::AcqRel, Ordering::Relaxed);
                // Only stop for the scrub this callback started with (not a newer play).
                if stop_at >= 0 && new_pos >= stop_at && shared.seek_gen.load(Ordering::Acquire) == gen {
                    shared.playing.store(false, Ordering::Release);
                    shared.stop_at.store(-1, Ordering::Relaxed);
                }
            },
            |e| log::error!("error de audio: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn build_input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut producer: rtrb::Producer<f32>,
    capturing: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    level: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                let keep = capturing.load(Ordering::Acquire);
                let mut peak = 0.0f32;
                for &s in data {
                    let v: f32 = s.to_sample();
                    peak = peak.max(v.abs());
                    if keep && producer.push(v).is_err() {
                        dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
                // Meter with a gentle fall-off.
                let old = f32::from_bits(level.load(Ordering::Relaxed));
                level.store(peak.max(old * 0.85).to_bits(), Ordering::Relaxed);
            },
            |e| log::error!("error del micrófono: {e}"),
            None,
        )
        .map_err(|e| format!("no se pudo abrir el micrófono: {e}"))
}

/// Android needs a runtime permission for the microphone (API 23+).
/// Android asks the user asynchronously: the panel requests the permission
/// and waits for it before arming (see `panels::audio`).
#[cfg(target_os = "android")]
fn request_mic_permission() -> Result<(), String> {
    if crate::android::has_permission(crate::android::RECORD_AUDIO) {
        Ok(())
    } else {
        Err("falta el permiso de micrófono".into())
    }
}

#[cfg(not(target_os = "android"))]
fn request_mic_permission() -> Result<(), String> {
    Ok(())
}
