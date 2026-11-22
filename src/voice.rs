//! Voice I/O and sound devices. Real microphone/speaker access is behind the
//! `voice` cargo feature (system audio: ALSA/PipeWire/PulseAudio on Linux,
//! WASAPI on Windows). Without the feature the app still compiles and the voice
//! *protocol* runs; there is simply no sound. The network path (20 ms PCM frames
//! over libp2p) is identical either way and is covered by `tests/voice_transport.rs`.
//!
//! The user can pick the input and output device (Settings → Sound); every part
//! of the app that opens audio asks [`input_device`] / [`output_device`].

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Network voice format: mono, 48 kHz, signed 16-bit — one frame is 20 ms.
/// Devices running at other rates are resampled to/from this.
pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 960;
const MAX_PLAYBACK_SAMPLES: usize = SAMPLE_RATE as usize * 2; // cap jitter buffer at 2 s

/// True if this build has real audio (the `voice` feature).
pub const AUDIO_ENABLED: bool = cfg!(feature = "voice");

// ---------------------------------------------------------------- devices

/// (input, output) device names chosen by the user; None = system default.
static DEVICES: RwLock<(Option<String>, Option<String>)> = RwLock::new((None, None));

/// Remember the user's device choice (None or "" = system default).
pub fn set_devices(input: Option<String>, output: Option<String>) {
    let clean = |s: Option<String>| s.filter(|v| !v.is_empty());
    if let Ok(mut d) = DEVICES.write() {
        *d = (clean(input), clean(output));
    }
}

pub fn chosen_devices() -> (Option<String>, Option<String>) {
    DEVICES.read().map(|d| d.clone()).unwrap_or((None, None))
}

/// Names of the microphones the system offers (empty without the `voice` feature).
pub fn list_inputs() -> Vec<String> {
    #[cfg(feature = "voice")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let mut v: Vec<String> = cpal::default_host().input_devices().map(|it| it.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
        v.dedup();
        v
    }
    #[cfg(not(feature = "voice"))]
    Vec::new()
}

/// Names of the speakers/headphones the system offers.
pub fn list_outputs() -> Vec<String> {
    #[cfg(feature = "voice")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let mut v: Vec<String> = cpal::default_host().output_devices().map(|it| it.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
        v.dedup();
        v
    }
    #[cfg(not(feature = "voice"))]
    Vec::new()
}

/// The chosen microphone, or the system default (also if the chosen one is gone).
#[cfg(feature = "voice")]
pub fn input_device() -> Option<cpal::Device> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Some(name) = chosen_devices().0 {
        if let Some(d) = host.input_devices().ok().and_then(|mut it| it.find(|d| d.name().map(|n| n == name).unwrap_or(false))) {
            return Some(d);
        }
    }
    host.default_input_device()
}

/// The chosen speakers/headphones, or the system default.
#[cfg(feature = "voice")]
pub fn output_device() -> Option<cpal::Device> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Some(name) = chosen_devices().1 {
        if let Some(d) = host.output_devices().ok().and_then(|mut it| it.find(|d| d.name().map(|n| n == name).unwrap_or(false))) {
            return Some(d);
        }
    }
    host.default_output_device()
}

/// Streaming linear resampler for mono i16 (keeps its phase between calls).
pub struct Resampler {
    step: f64,
    pos: f64,
    last: i16,
}

impl Resampler {
    pub fn new(from_rate: u32, to_rate: u32) -> Self {
        Self { step: from_rate as f64 / to_rate.max(1) as f64, pos: 0.0, last: 0 }
    }
    /// Feed input samples, append resampled output to `out`.
    pub fn push(&mut self, input: &[i16], out: &mut Vec<i16>) {
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend_from_slice(input);
            return;
        }
        // Position runs over [last, input...]: index 0 is the previous call's last sample.
        let n = input.len() as f64;
        while self.pos < n {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = if i == 0 { self.last } else { input[i - 1] } as f32;
            let b = input[i.min(input.len() - 1)] as f32;
            out.push((a + (b - a) * frac) as i16);
            self.pos += self.step;
        }
        self.pos -= n;
        if let Some(l) = input.last() {
            self.last = *l;
        }
    }
}

// ---------------------------------------------------------------- volume + sensitivity

static IN_GAIN: AtomicU32 = AtomicU32::new(1.0f32.to_bits());
static OUT_GAIN: AtomicU32 = AtomicU32::new(1.0f32.to_bits());
/// Manual gate threshold (linear peak); NaN = automatic.
static GATE: AtomicU32 = AtomicU32::new(f32::NAN.to_bits());
/// Threshold the gate is using right now (for the UI marker in auto mode).
static GATE_NOW: AtomicU32 = AtomicU32::new(0.01f32.to_bits());

/// Input/output volume in percent (0–200) and input sensitivity:
/// `sens_db` = None for automatic, or a threshold in dBFS (-100..0).
pub fn set_levels(in_pct: f32, out_pct: f32, sens_db: Option<f32>) {
    IN_GAIN.store((in_pct.clamp(0.0, 200.0) / 100.0).to_bits(), Ordering::Relaxed);
    OUT_GAIN.store((out_pct.clamp(0.0, 200.0) / 100.0).to_bits(), Ordering::Relaxed);
    GATE.store(sens_db.map(db_to_lin).unwrap_or(f32::NAN).to_bits(), Ordering::Relaxed);
}
pub fn in_gain() -> f32 {
    f32::from_bits(IN_GAIN.load(Ordering::Relaxed))
}
pub fn out_gain() -> f32 {
    f32::from_bits(OUT_GAIN.load(Ordering::Relaxed))
}
/// The threshold currently in use (linear), for drawing the sensitivity marker.
pub fn gate_threshold() -> f32 {
    f32::from_bits(GATE_NOW.load(Ordering::Relaxed))
}
pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}
pub fn lin_to_db(v: f32) -> f32 {
    20.0 * v.max(1e-5).log10()
}

/// Noise gate like Discord's "input sensitivity": sound quieter than the
/// threshold is not sent. Automatic mode follows the background noise level.
pub struct Gate {
    floor: f32,
    hold: u32,
}

impl Default for Gate {
    fn default() -> Self {
        Self { floor: 0.003, hold: 0 }
    }
}

impl Gate {
    /// Feed the peak of a buffer of `n` samples at `rate`; true = transmit.
    pub fn process(&mut self, peak: f32, n: usize, rate: u32) -> bool {
        // background estimate: drops fast, rises slowly
        if peak < self.floor {
            self.floor = self.floor * 0.7 + peak * 0.3;
        } else {
            self.floor = (self.floor * (1.0 + 0.1 * n as f32 / rate.max(1) as f32)).min(peak);
        }
        self.floor = self.floor.max(0.0005);
        let manual = f32::from_bits(GATE.load(Ordering::Relaxed));
        let th = if manual.is_nan() { (self.floor * 3.5).clamp(0.004, 0.06) } else { manual };
        GATE_NOW.store(th.to_bits(), Ordering::Relaxed);
        if peak >= th {
            self.hold = rate * 3 / 10; // keep open 300 ms after speech
            true
        } else if self.hold > 0 {
            self.hold = self.hold.saturating_sub(n as u32);
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------- live voice

/// Audio endpoint handle. The engine pushes received PCM into `playback`; with
/// the `voice` feature a microphone stream pushes captured frames to the engine.
pub struct VoiceIo {
    pub playback: Arc<Mutex<VecDeque<i16>>>,
    #[cfg(feature = "voice")]
    _streams: Vec<cpal::Stream>,
}

impl VoiceIo {
    /// Enqueue received 48 kHz samples for playback (bounded, drops oldest on overflow).
    pub fn push_playback(&self, pcm: &[i16]) {
        if let Ok(mut buf) = self.playback.lock() {
            buf.extend(pcm.iter().copied());
            let overflow = buf.len().saturating_sub(MAX_PLAYBACK_SAMPLES);
            for _ in 0..overflow {
                buf.pop_front();
            }
        }
    }
}

/// Voice without audio devices (developer test bots): only the playback
/// buffer, nothing is opened, the microphone stays free for the real user.
pub fn start_silent() -> VoiceIo {
    VoiceIo {
        playback: Arc::new(Mutex::new(VecDeque::new())),
        #[cfg(feature = "voice")]
        _streams: Vec::new(),
    }
}

/// Start audio on the chosen devices. `mic_active` gates capture (mic toggle);
/// `capture_tx` receives 20 ms 48 kHz frames to transmit over the network.
pub fn start(mic_active: Arc<AtomicBool>, capture_tx: tokio::sync::mpsc::UnboundedSender<Vec<i16>>) -> VoiceIo {
    let playback = Arc::new(Mutex::new(VecDeque::new()));

    #[cfg(not(feature = "voice"))]
    {
        // Silent build: no audio devices are opened.
        let _ = (&mic_active, &capture_tx);
        VoiceIo { playback }
    }

    #[cfg(feature = "voice")]
    {
        use cpal::traits::{DeviceTrait, StreamTrait};
        let mut streams: Vec<cpal::Stream> = Vec::new();
        let on_err = |e| eprintln!("voice stream error: {e}");

        // --- microphone (input) → 48 kHz frames ---
        if let Some(dev) = input_device() {
            if let Ok(cfg) = dev.default_input_config() {
                let ch = cfg.channels() as usize;
                let active = mic_active.clone();
                let tx = capture_tx.clone();
                let in_rate = cfg.sample_rate().0;
                let mut rs = Resampler::new(in_rate, SAMPLE_RATE);
                let mut mono: Vec<i16> = Vec::new();
                let mut acc: Vec<i16> = Vec::with_capacity(FRAME_SAMPLES * 2);
                let mut gate = Gate::default();
                let mut deliver = move |mono: &mut Vec<i16>| {
                    // input volume, then the sensitivity gate
                    let g = in_gain();
                    let mut peak = 0i32;
                    for v in mono.iter_mut() {
                        *v = (*v as f32 * g).clamp(-32767.0, 32767.0) as i16;
                        peak = peak.max((*v as i32).abs());
                    }
                    if !gate.process(peak as f32 / 32767.0, mono.len(), in_rate) {
                        return;
                    }
                    rs.push(mono, &mut acc);
                    while acc.len() >= FRAME_SAMPLES {
                        let rest = acc.split_off(FRAME_SAMPLES);
                        let _ = tx.send(std::mem::replace(&mut acc, rest));
                    }
                };
                let stream = match cfg.sample_format() {
                    cpal::SampleFormat::F32 => dev.build_input_stream(
                        &cfg.into(),
                        move |data: &[f32], _: &_| {
                            if !active.load(Ordering::Relaxed) {
                                return;
                            }
                            mono.clear();
                            mono.extend(data.chunks(ch).map(|f| (f[0].clamp(-1.0, 1.0) * 32767.0) as i16));
                            deliver(&mut mono);
                        },
                        on_err,
                        None,
                    ),
                    cpal::SampleFormat::I16 => dev.build_input_stream(
                        &cfg.into(),
                        move |data: &[i16], _: &_| {
                            if !active.load(Ordering::Relaxed) {
                                return;
                            }
                            mono.clear();
                            mono.extend(data.chunks(ch).map(|f| f[0]));
                            deliver(&mut mono);
                        },
                        on_err,
                        None,
                    ),
                    _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
                };
                if let Ok(s) = stream {
                    let _ = s.play();
                    streams.push(s);
                }
            }
        }

        // --- speakers (output): 48 kHz queue → device rate ---
        if let Some(dev) = output_device() {
            if let Ok(cfg) = dev.default_output_config() {
                let ch = cfg.channels() as usize;
                let step = SAMPLE_RATE as f64 / cfg.sample_rate().0 as f64;
                let buf = playback.clone();
                let mut pos = 0.0f64;
                let mut next = move |b: &mut VecDeque<i16>| -> f32 {
                    // Read position advances by `step` 48 kHz samples per output sample.
                    let s = (b.front().copied().unwrap_or(0) as f32 / 32767.0 * out_gain()).clamp(-1.0, 1.0);
                    pos += step;
                    while pos >= 1.0 {
                        b.pop_front();
                        pos -= 1.0;
                    }
                    s
                };
                let stream = match cfg.sample_format() {
                    cpal::SampleFormat::F32 => dev.build_output_stream(
                        &cfg.into(),
                        move |out: &mut [f32], _: &_| {
                            let mut b = buf.lock().unwrap();
                            for frame in out.chunks_mut(ch) {
                                let s = next(&mut b);
                                for c in frame.iter_mut() {
                                    *c = s;
                                }
                            }
                        },
                        on_err,
                        None,
                    ),
                    cpal::SampleFormat::I16 => dev.build_output_stream(
                        &cfg.into(),
                        move |out: &mut [i16], _: &_| {
                            let mut b = buf.lock().unwrap();
                            for frame in out.chunks_mut(ch) {
                                let s = (next(&mut b) * 32767.0) as i16;
                                for c in frame.iter_mut() {
                                    *c = s;
                                }
                            }
                        },
                        on_err,
                        None,
                    ),
                    _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
                };
                if let Ok(s) = stream {
                    let _ = s.play();
                    streams.push(s);
                }
            }
        }

        VoiceIo { playback, _streams: streams }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_keeps_duration_and_tone() {
        // 1 s of 440 Hz at 44.1 kHz, fed in odd-sized chunks → ~48000 samples
        let src: Vec<i16> = (0..44_100).map(|i| ((i as f32 / 44_100.0 * 440.0 * std::f32::consts::TAU).sin() * 10_000.0) as i16).collect();
        let mut rs = Resampler::new(44_100, 48_000);
        let mut out = Vec::new();
        for c in src.chunks(441) {
            rs.push(c, &mut out);
        }
        assert!((out.len() as i64 - 48_000).abs() < 3, "{}", out.len());
        // zero crossings ≈ 2 × 440
        let zc = out.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
        assert!((zc as i64 - 880).abs() < 6, "{zc}");
    }

    #[test]
    fn same_rate_is_passthrough() {
        let mut rs = Resampler::new(48_000, 48_000);
        let mut out = Vec::new();
        rs.push(&[1, 2, 3], &mut out);
        assert_eq!(out, vec![1, 2, 3]);
    }
}
