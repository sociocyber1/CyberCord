//! Sound for video circles: record the microphone while filming, and play a
//! clip's audio. Needs the `voice` feature (system audio); without it circles
//! are silent and these functions return None.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
#[cfg(feature = "voice")]
use std::sync::Mutex;

pub struct MicRecorder {
    /// Current input loudness 0..1 (peak of the last buffer), for level meters.
    level: Arc<AtomicU32>,
    #[cfg(feature = "voice")]
    _stream: cpal::Stream,
    #[cfg(feature = "voice")]
    buf: Arc<Mutex<Vec<i16>>>,
    #[cfg(feature = "voice")]
    rate: u32,
}

impl MicRecorder {
    #[cfg(not(feature = "voice"))]
    pub fn start() -> Option<Self> {
        None
    }

    #[cfg(feature = "voice")]
    pub fn start() -> Option<Self> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let dev = cybercord_node::voice::input_device()?;
        let cfg = dev.default_input_config().ok()?;
        let ch = cfg.channels() as usize;
        let rate = cfg.sample_rate().0;
        let buf: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
        let b = buf.clone();
        let level = Arc::new(AtomicU32::new(0));
        // Never keep more than 6 minutes, whatever the device does.
        let cap = rate as usize * 360;
        let (lv1, lv2) = (level.clone(), level.clone());
        let err = |e| eprintln!("circle mic error: {e}");
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => dev.build_input_stream(
                &cfg.into(),
                move |d: &[f32], _: &_| {
                    let mut v = b.lock().unwrap();
                    let g = cybercord_node::voice::in_gain();
                    let peak = d.iter().fold(0.0f32, |m, x| m.max(x.abs())) * g;
                    lv1.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
                    if v.len() < cap {
                        v.extend(d.chunks(ch).map(|f| ((f[0] * g).clamp(-1.0, 1.0) * 32767.0) as i16));
                    }
                },
                err,
                None,
            ),
            cpal::SampleFormat::I16 => dev.build_input_stream(
                &cfg.into(),
                move |d: &[i16], _: &_| {
                    let mut v = b.lock().unwrap();
                    let g = cybercord_node::voice::in_gain();
                    let peak = d.iter().fold(0i32, |m, x| m.max((*x as i32).abs())) as f32 / 32767.0 * g;
                    lv2.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
                    if v.len() < cap {
                        v.extend(d.chunks(ch).map(|f| (f[0] as f32 * g).clamp(-32767.0, 32767.0) as i16));
                    }
                },
                err,
                None,
            ),
            cpal::SampleFormat::I32 => {
                let b2 = buf.clone();
                let lv3 = level.clone();
                dev.build_input_stream(
                    &cfg.into(),
                    move |d: &[i32], _: &_| {
                        let mut v = b2.lock().unwrap();
                        let g = cybercord_node::voice::in_gain();
                        let peak = d.iter().fold(0f32, |m, x| m.max((*x as f32 / i32::MAX as f32).abs())) * g;
                        lv3.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
                        if v.len() < cap {
                            v.extend(d.chunks(ch).map(|f| ((f[0] as f32 / i32::MAX as f32 * g).clamp(-1.0, 1.0) * 32767.0) as i16));
                        }
                    },
                    err,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let b2 = buf.clone();
                let lv3 = level.clone();
                dev.build_input_stream(
                    &cfg.into(),
                    move |d: &[u16], _: &_| {
                        let mut v = b2.lock().unwrap();
                        let g = cybercord_node::voice::in_gain();
                        let to = |x: u16| (x as f32 - 32768.0) / 32768.0;
                        let peak = d.iter().fold(0f32, |m, x| m.max(to(*x).abs())) * g;
                        lv3.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
                        if v.len() < cap {
                            v.extend(d.chunks(ch).map(|f| ((to(f[0]) * g).clamp(-1.0, 1.0) * 32767.0) as i16));
                        }
                    },
                    err,
                    None,
                )
            }
            other => {
                eprintln!("circle mic: unsupported sample format {other:?}");
                return None;
            }
        }
        .map_err(|e| eprintln!("circle mic: cannot open input: {e}"))
        .ok()?;
        stream.play().map_err(|e| eprintln!("circle mic: cannot start: {e}")).ok()?;
        Some(Self { level, _stream: stream, buf, rate })
    }

    /// Input loudness 0..1 right now.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Recorded length so far, in milliseconds.
    #[cfg(feature = "voice")]
    pub fn recorded_ms(&self) -> u32 {
        (self.buf.lock().map(|b| b.len()).unwrap_or(0) as u64 * 1000 / self.rate.max(1) as u64) as u32
    }
    #[cfg(not(feature = "voice"))]
    pub fn recorded_ms(&self) -> u32 {
        0
    }

    /// Stop and return (sample rate, mono samples), downsampled to ≤ 24 kHz.
    pub fn finish(self) -> (u32, Vec<i16>) {
        let (rate, pcm) = self.finish_raw();
        if rate >= 44_100 {
            (rate / 2, lowpass(&pcm, 2).into_iter().step_by(2).collect())
        } else {
            (rate, pcm)
        }
    }

    /// Stop and return everything at the device rate.
    #[cfg(feature = "voice")]
    pub fn finish_raw(self) -> (u32, Vec<i16>) {
        (self.rate, std::mem::take(&mut *self.buf.lock().unwrap()))
    }

    #[cfg(not(feature = "voice"))]
    pub fn finish_raw(self) -> (u32, Vec<i16>) {
        (0, Vec::new())
    }
}

/// Plays a clip's sound; playback stops when this is dropped.
pub struct Player {
    #[cfg(feature = "voice")]
    _stream: cpal::Stream,
}

#[cfg(not(feature = "voice"))]
pub fn play(_rate: u32, _pcm: Vec<i16>) -> Option<Player> {
    None
}

#[cfg(feature = "voice")]
pub fn play(rate: u32, pcm: Vec<i16>) -> Option<Player> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    if pcm.is_empty() || rate == 0 {
        return None;
    }
    let dev = cybercord_node::voice::output_device()?;
    let cfg = dev.default_output_config().ok()?;
    let ch = cfg.channels() as usize;
    let step = rate as f64 / cfg.sample_rate().0 as f64; // simple resampling by position
    let mut pos = 0.0f64;
    let err = |e| eprintln!("circle playback error: {e}");
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_output_stream(
            &cfg.into(),
            move |out: &mut [f32], _: &_| {
                for frame in out.chunks_mut(ch) {
                    let s = (pcm.get(pos as usize).map(|v| *v as f32 / 32767.0).unwrap_or(0.0) * cybercord_node::voice::out_gain()).clamp(-1.0, 1.0);
                    pos += step;
                    for c in frame.iter_mut() {
                        *c = s;
                    }
                }
            },
            err,
            None,
        ),
        cpal::SampleFormat::I16 => dev.build_output_stream(
            &cfg.into(),
            move |out: &mut [i16], _: &_| {
                for frame in out.chunks_mut(ch) {
                    let s = (pcm.get(pos as usize).copied().unwrap_or(0) as f32 * cybercord_node::voice::out_gain()).clamp(-32767.0, 32767.0) as i16;
                    pos += step;
                    for c in frame.iter_mut() {
                        *c = s;
                    }
                }
            },
            err,
            None,
        ),
        _ => return None,
    }
    .ok()?;
    stream.play().ok()?;
    Some(Player { _stream: stream })
}

/// Voice messages are stored as 16 kHz mono 16-bit WAV (≈ 32 KB per second,
/// readable by any player and by older CyberCord versions as a music file).
pub const VOICE_MSG_RATE: u32 = 16_000;
/// File name that marks an audio attachment as a voice message.
pub const VOICE_MSG_NAME: &str = "voice-message.wav";
/// Longest voice message.
pub const VOICE_MSG_MAX_SECS: u32 = 300;

/// Stop a recording and return it as a voice-message WAV, plus its length.
pub fn finish_voice_message(rec: MicRecorder) -> Option<(Vec<u8>, u32)> {
    let (rate, mut pcm) = rec.finish_raw();
    pcm.truncate(rate as usize * VOICE_MSG_MAX_SECS as usize);
    if rate == 0 || pcm.len() < rate as usize / 2 {
        return None; // nothing (or under half a second) recorded
    }
    // Crude anti-alias filter before going down to 16 kHz.
    let pcm = lowpass(&pcm, (rate as f32 / VOICE_MSG_RATE as f32).round().max(1.0) as usize);
    let mut out = Vec::with_capacity(pcm.len() * VOICE_MSG_RATE as usize / rate as usize + 16);
    let mut rs = cybercord_node::voice::Resampler::new(rate, VOICE_MSG_RATE);
    rs.push(&pcm, &mut out);
    let dur_ms = (out.len() as u64 * 1000 / VOICE_MSG_RATE as u64) as u32;
    Some((wav(VOICE_MSG_RATE, &out), dur_ms))
}

/// Moving-average low-pass over `k` samples (k = 1: unchanged).
fn lowpass(pcm: &[i16], k: usize) -> Vec<i16> {
    if k <= 1 {
        return pcm.to_vec();
    }
    let mut acc: i64 = 0;
    let mut out = Vec::with_capacity(pcm.len());
    for (i, s) in pcm.iter().enumerate() {
        acc += *s as i64;
        if i >= k {
            acc -= pcm[i - k] as i64;
        }
        out.push((acc / (i + 1).min(k) as i64) as i16);
    }
    out
}

/// Mono 16-bit PCM → WAV bytes.
pub fn wav(rate: u32, pcm: &[i16]) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// A short two-note chime for "Test sound".
pub fn chime() -> (u32, Vec<i16>) {
    let rate = 24_000u32;
    let mut pcm = Vec::new();
    for (f, len) in [(659.25f32, 0.22f32), (987.77, 0.45)] {
        let n = (rate as f32 * len) as usize;
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let env = (t * 60.0).min(1.0) * (-t * 5.5).exp();
            let v = ((t * f * std::f32::consts::TAU).sin() + 0.25 * (t * f * 2.0 * std::f32::consts::TAU).sin()) * env * 0.45;
            pcm.push((v * 32767.0) as i16);
        }
    }
    (rate, pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_roundtrips_through_the_music_decoder() {
        let (rate, pcm) = chime();
        let w = wav(rate, &pcm);
        let t = crate::music::decode(&w).unwrap();
        assert_eq!(t.rate, rate);
        assert_eq!(t.frames(), pcm.len());
    }
}

// ---------------------------------------------------------------- live mic check

/// Discord-style "Let's check": the microphone is played straight back to the
/// chosen output (after input volume and the sensitivity gate), and the recent
/// loudness is kept for a scrolling waveform.
pub struct Monitor {
    /// Peak per 10 ms of the last ~4 s (after input volume), oldest first.
    pub peaks: Arc<std::sync::Mutex<std::collections::VecDeque<f32>>>,
    /// Whether the gate lets the current sound through.
    pub open: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(feature = "voice")]
    _streams: Vec<cpal::Stream>,
}

impl Monitor {
    #[cfg(not(feature = "voice"))]
    pub fn start() -> Option<Self> {
        None
    }

    #[cfg(feature = "voice")]
    pub fn start() -> Option<Self> {
        use cpal::traits::{DeviceTrait, StreamTrait};
        use std::collections::VecDeque;
        use std::sync::atomic::AtomicBool;
        let idev = cybercord_node::voice::input_device()?;
        let icfg = idev.default_input_config().ok()?;
        let ich = icfg.channels() as usize;
        let irate = icfg.sample_rate().0;
        let peaks = Arc::new(Mutex::new(VecDeque::with_capacity(400)));
        let open = Arc::new(AtomicBool::new(false));
        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let (pk, op, q) = (peaks.clone(), open.clone(), queue.clone());
        let bucket = (irate / 100).max(1) as usize;
        let mut gate = cybercord_node::voice::Gate::default();
        let (mut acc_n, mut acc_peak) = (0usize, 0f32);
        let mut on_input = move |mono: &mut dyn Iterator<Item = f32>| {
            let g = cybercord_node::voice::in_gain();
            let mut buf: Vec<f32> = mono.map(|v| (v * g).clamp(-1.0, 1.0)).collect();
            let peak = buf.iter().fold(0f32, |m, v| m.max(v.abs()));
            let pass = gate.process(peak, buf.len(), irate);
            op.store(pass, Ordering::Relaxed);
            {
                let mut p = pk.lock().unwrap();
                for v in &buf {
                    acc_peak = acc_peak.max(v.abs());
                    acc_n += 1;
                    if acc_n >= bucket {
                        p.push_back(acc_peak);
                        if p.len() > 400 {
                            p.pop_front();
                        }
                        acc_n = 0;
                        acc_peak = 0.0;
                    }
                }
            }
            if !pass {
                buf.iter_mut().for_each(|v| *v = 0.0);
            }
            let mut qq = q.lock().unwrap();
            qq.extend(buf);
            let max = irate as usize / 4; // keep latency under 250 ms
            while qq.len() > max {
                qq.pop_front();
            }
        };
        let err = |e| eprintln!("mic check error: {e}");
        let istream = match icfg.sample_format() {
            cpal::SampleFormat::F32 => idev.build_input_stream(&icfg.into(), move |d: &[f32], _: &_| on_input(&mut d.chunks(ich).map(|f| f[0])), err, None),
            cpal::SampleFormat::I16 => idev.build_input_stream(&icfg.into(), move |d: &[i16], _: &_| on_input(&mut d.chunks(ich).map(|f| f[0] as f32 / 32767.0)), err, None),
            _ => return None,
        }
        .ok()?;
        istream.play().ok()?;
        let mut streams = vec![istream];
        // loopback to the speakers / headphones
        if let Some(odev) = cybercord_node::voice::output_device() {
            if let Ok(ocfg) = odev.default_output_config() {
                let och = ocfg.channels() as usize;
                let step = irate as f64 / ocfg.sample_rate().0 as f64;
                let mut pos = 0.0f64;
                let qo = queue.clone();
                let mut next = move || -> f32 {
                    let mut qq = qo.lock().unwrap();
                    let v = qq.front().copied().unwrap_or(0.0);
                    pos += step;
                    while pos >= 1.0 {
                        qq.pop_front();
                        pos -= 1.0;
                    }
                    (v * cybercord_node::voice::out_gain()).clamp(-1.0, 1.0)
                };
                let s = match ocfg.sample_format() {
                    cpal::SampleFormat::F32 => odev.build_output_stream(
                        &ocfg.into(),
                        move |o: &mut [f32], _: &_| {
                            for fr in o.chunks_mut(och) {
                                let v = next();
                                fr.iter_mut().for_each(|c| *c = v);
                            }
                        },
                        err,
                        None,
                    ),
                    cpal::SampleFormat::I16 => odev.build_output_stream(
                        &ocfg.into(),
                        move |o: &mut [i16], _: &_| {
                            for fr in o.chunks_mut(och) {
                                let v = (next() * 32767.0) as i16;
                                fr.iter_mut().for_each(|c| *c = v);
                            }
                        },
                        err,
                        None,
                    ),
                    _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
                };
                if let Ok(s) = s {
                    let _ = s.play();
                    streams.push(s);
                }
            }
        }
        Some(Self { peaks, open, _streams: streams })
    }

    /// Latest peaks (oldest first), copied for drawing.
    pub fn recent(&self) -> Vec<f32> {
        self.peaks.lock().map(|p| p.iter().copied().collect()).unwrap_or_default()
    }
}
