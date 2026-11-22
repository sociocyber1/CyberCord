//! Interface sounds: soft, calm, synthesised here (no sound files, no
//! licences). Sine tones with a gentle attack, a long natural decay, a little
//! second harmonic for warmth and a short room echo. Pentatonic notes only, so
//! any two sounds overlapping still agree with each other.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::music::Track;

const RATE: f32 = 48_000.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sfx {
    /// CyberCord's sound logo (start).
    Brand,
    Message,
    Mention,
    Sent,
    VoiceJoin,
    VoiceLeave,
    PeerJoin,
    PeerLeave,
    /// Incoming call (repeated while ringing).
    Ring,
    /// Outgoing call, waiting for an answer (repeated).
    Ringback,
    CallStart,
    CallEnd,
}

pub const ALL: [Sfx; 12] = [Sfx::Brand, Sfx::Message, Sfx::Mention, Sfx::Sent, Sfx::VoiceJoin, Sfx::VoiceLeave, Sfx::PeerJoin, Sfx::PeerLeave, Sfx::Ring, Sfx::Ringback, Sfx::CallStart, Sfx::CallEnd];

impl Sfx {
    /// i18n key of the sound's name.
    pub fn key(self) -> &'static str {
        match self {
            Sfx::Brand => "sfx_brand",
            Sfx::Message => "sfx_message",
            Sfx::Mention => "sfx_mention",
            Sfx::Sent => "sfx_sent",
            Sfx::VoiceJoin => "sfx_voice_join",
            Sfx::VoiceLeave => "sfx_voice_leave",
            Sfx::PeerJoin => "sfx_peer_join",
            Sfx::PeerLeave => "sfx_peer_leave",
            Sfx::Ring => "sfx_ring",
            Sfx::Ringback => "sfx_ringback",
            Sfx::CallStart => "sfx_call_start",
            Sfx::CallEnd => "sfx_call_end",
        }
    }
}

/// Note frequency: semitones from A4 (440 Hz).
fn hz(semi: f32) -> f32 {
    440.0 * 2f32.powf(semi / 12.0)
}

// pentatonic in D: D E F# A B
const D4: f32 = -7.0;
const FS4: f32 = -3.0;
const A4: f32 = 0.0;
const B4: f32 = 2.0;
const D5: f32 = 5.0;
const E5: f32 = 7.0;
const FS5: f32 = 9.0;
const A5: f32 = 12.0;
const B5: f32 = 14.0;
const D6: f32 = 17.0;

/// One soft note: start (s), pitch, length of the decay (s), loudness, and
/// how "bell-like" it is (upper partial that fades fast).
struct Note(f32, f32, f32, f32, f32);

fn render(notes: &[Note], total: f32) -> Vec<f32> {
    let n = (total * RATE) as usize;
    let mut dry = vec![0f32; n];
    for &Note(start, semi, decay, amp, bell) in notes {
        let f = hz(semi);
        let s0 = (start * RATE) as usize;
        let len = ((decay * 5.0) * RATE) as usize;
        for i in 0..len {
            let k = s0 + i;
            if k >= n {
                break;
            }
            let t = i as f32 / RATE;
            let attack = (t / 0.012).min(1.0); // 12 ms: no click
            let env = attack * (-t / decay).exp();
            let ph = std::f32::consts::TAU * f * t;
            let v = ph.sin() + 0.18 * (2.0 * ph).sin() * (-t / (decay * 0.5)).exp() + bell * (4.0 * ph).sin() * (-t / 0.06).exp();
            dry[k] += v * env * amp;
        }
    }
    // small room: three soft reflections
    let mut out = dry.clone();
    for (ms, g) in [(37.0, 0.22), (71.0, 0.14), (113.0, 0.08)] {
        let d = (ms / 1000.0 * RATE) as usize;
        for i in d..n {
            out[i] += dry[i - d] * g;
        }
    }
    // fade out the very end, normalise gently
    let fade = (0.03 * RATE) as usize;
    for i in 0..fade.min(n) {
        out[n - 1 - i] *= i as f32 / fade as f32;
    }
    let peak = out.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-6);
    let norm = 0.8 / peak.max(0.8);
    out.iter_mut().for_each(|v| *v *= norm);
    out
}

fn synth(s: Sfx) -> Vec<f32> {
    match s {
        // D – F# – A – D: rising, open, unhurried; a low D underneath
        Sfx::Brand => render(&[Note(0.0, D4, 0.9, 0.35, 0.0), Note(0.0, D5, 0.45, 0.5, 0.25), Note(0.14, FS5, 0.45, 0.45, 0.25), Note(0.28, A5, 0.5, 0.45, 0.25), Note(0.46, D6, 0.8, 0.4, 0.3)], 2.0),
        Sfx::Message => render(&[Note(0.0, A5, 0.22, 0.5, 0.35), Note(0.075, E5, 0.3, 0.45, 0.3)], 0.9),
        Sfx::Mention => render(&[Note(0.0, D5, 0.22, 0.45, 0.3), Note(0.07, FS5, 0.22, 0.45, 0.3), Note(0.14, A5, 0.35, 0.5, 0.35)], 1.0),
        Sfx::Sent => render(&[Note(0.0, B5, 0.05, 0.35, 0.1)], 0.3),
        Sfx::VoiceJoin => render(&[Note(0.0, A4, 0.2, 0.5, 0.15), Note(0.09, E5, 0.32, 0.5, 0.2)], 0.9),
        Sfx::VoiceLeave => render(&[Note(0.0, E5, 0.2, 0.45, 0.15), Note(0.09, A4, 0.32, 0.45, 0.1)], 0.9),
        Sfx::PeerJoin => render(&[Note(0.0, B5, 0.12, 0.35, 0.2)], 0.5),
        Sfx::PeerLeave => render(&[Note(0.0, FS5, 0.12, 0.3, 0.1)], 0.5),
        // a soft marimba phrase, then a breath of silence (the loop period)
        Sfx::Ring => render(
            &[Note(0.0, D5, 0.18, 0.5, 0.6), Note(0.15, FS5, 0.18, 0.5, 0.6), Note(0.30, A5, 0.18, 0.5, 0.6), Note(0.45, FS5, 0.18, 0.45, 0.6), Note(0.75, A5, 0.18, 0.5, 0.6), Note(0.90, D6, 0.35, 0.5, 0.6)],
            2.6,
        ),
        Sfx::Ringback => render(&[Note(0.0, A4, 0.35, 0.35, 0.0), Note(0.0, D5, 0.35, 0.2, 0.0)], 3.0),
        Sfx::CallStart => render(&[Note(0.0, E5, 0.2, 0.45, 0.2), Note(0.1, A5, 0.4, 0.5, 0.25)], 1.0),
        Sfx::CallEnd => render(&[Note(0.0, A5, 0.18, 0.4, 0.15), Note(0.1, E5, 0.18, 0.4, 0.1), Note(0.2, B4, 0.35, 0.4, 0.05)], 1.1),
    }
}

/// The rendered sound (made once, then cached).
pub fn track(s: Sfx) -> Arc<Track> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<Sfx, Arc<Track>>>> = OnceLock::new();
    let m = CACHE.get_or_init(Default::default);
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    g.entry(s).or_insert_with(|| Arc::new(Track { rate: RATE as u32, channels: 1, samples: synth(s), title: None, artist: None })).clone()
}

/// The same sound scaled to a volume 0…1.
pub fn track_at(s: Sfx, vol: f32) -> Arc<Track> {
    let t = track(s);
    if (vol - 1.0).abs() < 0.01 {
        return t;
    }
    let v = vol.clamp(0.0, 1.0);
    Arc::new(Track { rate: t.rate, channels: 1, samples: t.samples.iter().map(|x| x * v).collect(), title: None, artist: None })
}

/// How long the sound lasts (the loop period for Ring / Ringback).
pub fn seconds(s: Sfx) -> f32 {
    track(s).samples.len() as f32 / RATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sounds_are_soft_and_clean() {
        for s in ALL {
            let t = track(s);
            assert!(!t.samples.is_empty(), "{s:?}");
            let peak = t.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.05 && peak <= 0.81, "{s:?} peak {peak}");
            // starts and ends silently (no clicks)
            assert!(t.samples[0].abs() < 0.01, "{s:?} starts with a click");
            assert!(t.samples.last().unwrap().abs() < 0.01, "{s:?} ends with a click");
        }
        assert!(seconds(Sfx::Ring) > 2.0);
    }
}
