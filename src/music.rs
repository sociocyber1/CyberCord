//! Music messages: decode a shared audio file (mp3, flac, ogg/vorbis, wav,
//! m4a/aac) with symphonia, draw its waveform, and play it with seeking.
//! Playback needs the `voice` build (cpal); without it the file can still be
//! opened in the system's own player.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;

/// Longest track we decode (bytes are capped by MAX_BLOB anyway): 20 minutes.
const MAX_SECS: u64 = 20 * 60;

/// Cheap magic-number check for audio files.
pub fn is_audio(b: &[u8]) -> bool {
    b.starts_with(b"ID3")
        || b.starts_with(b"fLaC")
        || b.starts_with(b"OggS")
        || (b.len() > 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WAVE")
        || (b.len() > 12 && &b[4..8] == b"ftyp" && matches!(&b[8..11], b"M4A" | b"mp4" | b"iso" | b"M4B"))
        || (b.len() > 2 && b[0] == 0xFF && (b[1] & 0xE0) == 0xE0 && (b[1] & 0x06) != 0) // MPEG audio frame
}

/// A decoded track: interleaved f32 samples.
pub struct Track {
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
    pub title: Option<String>,
    pub artist: Option<String>,
}

impl Track {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
    pub fn dur_ms(&self) -> u32 {
        (self.frames() as u64 * 1000 / self.rate.max(1) as u64) as u32
    }
    /// `n` loudness bars (0..1) for the waveform.
    pub fn peaks(&self, n: usize) -> Vec<f32> {
        let frames = self.frames();
        if frames == 0 || n == 0 {
            return vec![0.0; n];
        }
        let per = (frames / n).max(1);
        let mut out: Vec<f32> = (0..n)
            .map(|i| {
                let a = i * per * self.channels;
                let b = ((i + 1) * per * self.channels).min(self.samples.len());
                if a >= b {
                    return 0.0;
                }
                // RMS of a strided subset: fast and smooth enough for bars.
                let step = ((b - a) / 512).max(1);
                let (mut sum, mut cnt) = (0.0f32, 0u32);
                let mut j = a;
                while j < b {
                    sum += self.samples[j] * self.samples[j];
                    cnt += 1;
                    j += step;
                }
                (sum / cnt.max(1) as f32).sqrt()
            })
            .collect();
        let max = out.iter().cloned().fold(0.0001f32, f32::max);
        for v in out.iter_mut() {
            *v = (*v / max).powf(0.8);
        }
        out
    }
}

fn open(bytes: &[u8]) -> Result<(symphonia::core::probe::ProbeResult, ()), String> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&Hint::new(), mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    Ok((probed, ()))
}

fn read_tags(probed: &mut symphonia::core::probe::ProbeResult) -> (Option<String>, Option<String>) {
    let mut title = None;
    let mut artist = None;
    let mut scan = |rev: &symphonia::core::meta::MetadataRevision| {
        for t in rev.tags() {
            match t.std_key {
                Some(StandardTagKey::TrackTitle) if title.is_none() => title = Some(t.value.to_string()),
                Some(StandardTagKey::Artist) if artist.is_none() => artist = Some(t.value.to_string()),
                _ => {}
            }
        }
    };
    if let Some(m) = probed.metadata.get() {
        if let Some(rev) = m.current() {
            scan(rev);
        }
    }
    if let Some(rev) = probed.format.metadata().current() {
        scan(rev);
    }
    let clean = |s: Option<String>| s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    (clean(title), clean(artist))
}

/// Decode the whole file.
pub fn decode(bytes: &[u8]) -> Result<Track, String> {
    let (mut probed, _) = open(bytes)?;
    let (title, artist) = read_tags(&mut probed);
    let mut format = probed.format;
    let track = format.default_track().ok_or("no audio track")?;
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
    let mut rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);
    let mut samples: Vec<f32> = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break, // end of stream (or a broken tail)
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        channels = spec.channels.count();
        let sb = buf.get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, spec));
        if sb.capacity() < decoded.capacity() {
            *sb = SampleBuffer::new(decoded.capacity() as u64, spec);
        }
        sb.copy_interleaved_ref(decoded);
        samples.extend_from_slice(sb.samples());
        if samples.len() as u64 > MAX_SECS * rate as u64 * channels as u64 {
            break;
        }
    }
    if samples.is_empty() {
        return Err("no audio".into());
    }
    Ok(Track { rate, channels: channels.max(1), samples, title, artist })
}

/// Label and length for a music message: "Artist — Title" from the tags, or
/// the file name; decodes the file once, which also checks it is playable.
pub fn describe(bytes: &[u8], file_name: &str) -> Result<(String, u32), String> {
    let t = decode(bytes)?;
    let label = match (&t.artist, &t.title) {
        (Some(a), Some(ti)) => format!("{a} — {ti}"),
        (None, Some(ti)) => ti.clone(),
        _ => file_name.to_string(),
    };
    Ok((label, t.dur_ms()))
}

/// Shared play state; the audio callback advances `pos` (source frames, f64 bits).
pub struct PlayState {
    pos: AtomicU64,
    playing: AtomicBool,
    frames: usize,
    rate: u32,
}

impl PlayState {
    pub fn position_ms(&self) -> u32 {
        (f64::from_bits(self.pos.load(Ordering::Relaxed)) * 1000.0 / self.rate.max(1) as f64) as u32
    }
    pub fn fraction(&self) -> f32 {
        (f64::from_bits(self.pos.load(Ordering::Relaxed)) / self.frames.max(1) as f64) as f32
    }
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
    pub fn finished(&self) -> bool {
        f64::from_bits(self.pos.load(Ordering::Relaxed)) as usize >= self.frames
    }
    pub fn set_playing(&self, on: bool) {
        if on && self.finished() {
            self.seek(0.0);
        }
        self.playing.store(on, Ordering::Relaxed);
    }
    pub fn seek(&self, frac: f32) {
        let f = (frac.clamp(0.0, 1.0) as f64) * self.frames as f64;
        self.pos.store(f.to_bits(), Ordering::Relaxed);
    }
}

/// A playing (or paused) track. Dropping it stops the sound.
pub struct Player {
    pub state: Arc<PlayState>,
    #[cfg(feature = "voice")]
    _stream: cpal::Stream,
}

#[cfg(not(feature = "voice"))]
pub fn play(_track: Arc<Track>) -> Option<Player> {
    None
}

pub const CAN_PLAY: bool = cfg!(feature = "voice");

#[cfg(feature = "voice")]
pub fn play(track: Arc<Track>) -> Option<Player> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let dev = cybercord_node::voice::output_device()?;
    let cfg = dev.default_output_config().ok()?;
    let out_ch = cfg.channels() as usize;
    let step = track.rate as f64 / cfg.sample_rate().0 as f64;
    let state = Arc::new(PlayState { pos: AtomicU64::new(0f64.to_bits()), playing: AtomicBool::new(true), frames: track.frames(), rate: track.rate });
    let st = state.clone();
    let t = track.clone();
    // Linear resampling + channel mapping (mono→all, stereo→mono average, else by index).
    let mut fill = move |out: &mut dyn FnMut(usize, f32), n_frames: usize| {
        let playing = st.playing.load(Ordering::Relaxed);
        let mut pos = f64::from_bits(st.pos.load(Ordering::Relaxed));
        let src_ch = t.channels;
        let frames = t.frames();
        for f in 0..n_frames {
            let i = pos as usize;
            for c in 0..out_ch {
                let v = if !playing || i + 1 >= frames {
                    0.0
                } else {
                    let frac = (pos - i as f64) as f32;
                    let sample = |k: usize| -> f32 {
                        if out_ch == 1 && src_ch >= 2 {
                            (t.samples[k * src_ch] + t.samples[k * src_ch + 1]) * 0.5
                        } else {
                            t.samples[k * src_ch + c.min(src_ch - 1)]
                        }
                    };
                    sample(i) * (1.0 - frac) + sample(i + 1) * frac
                };
                out(f * out_ch + c, (v * 0.9 * cybercord_node::voice::out_gain()).clamp(-1.0, 1.0));
            }
            if playing && i < frames {
                pos += step;
            }
        }
        if playing && pos as usize >= frames {
            st.playing.store(false, Ordering::Relaxed);
            pos = frames as f64;
        }
        st.pos.store(pos.to_bits(), Ordering::Relaxed);
    };
    let err = |e| eprintln!("music playback error: {e}");
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_output_stream(
            &cfg.into(),
            move |data: &mut [f32], _: &_| {
                let n = data.len() / out_ch;
                fill(&mut |i, v| data[i] = v, n);
            },
            err,
            None,
        ),
        cpal::SampleFormat::I16 => dev.build_output_stream(
            &cfg.into(),
            move |data: &mut [i16], _: &_| {
                let n = data.len() / out_ch;
                fill(&mut |i, v| data[i] = (v.clamp(-1.0, 1.0) * 32767.0) as i16, n);
            },
            err,
            None,
        ),
        _ => return None,
    }
    .ok()?;
    stream.play().ok()?;
    Some(Player { state, _stream: stream })
}

/// Hand the file to the system's music player (works in every build).
pub fn open_externally(hash: &str, bytes: &[u8], name: &str) -> Result<std::path::PathBuf, String> {
    let ext = std::path::Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).filter(|e| e.len() <= 4).unwrap_or_else(|| guess_ext(bytes).into());
    let dir = std::env::temp_dir().join("cybercord-music");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.{ext}", &hash[..hash.len().min(16)]));
    if !path.exists() {
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(&path).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(&path).spawn();
    r.map_err(|e| e.to_string())?;
    Ok(path)
}

fn guess_ext(b: &[u8]) -> &'static str {
    if b.starts_with(b"fLaC") {
        "flac"
    } else if b.starts_with(b"OggS") {
        "ogg"
    } else if b.starts_with(b"RIFF") {
        "wav"
    } else if b.len() > 8 && &b[4..8] == b"ftyp" {
        "m4a"
    } else {
        "mp3"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, secs: f32) -> Vec<u8> {
        let n = (rate as f32 * secs) as u32;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + n * 2).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * 2).to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(n * 2).to_le_bytes());
        for i in 0..n {
            let s = ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 12000.0) as i16;
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    #[test]
    fn decodes_wav_and_draws_peaks() {
        let b = wav(22_050, 1.5);
        assert!(is_audio(&b));
        let t = decode(&b).unwrap();
        assert_eq!(t.rate, 22_050);
        assert!((t.dur_ms() as i32 - 1500).abs() < 30);
        let p = t.peaks(40);
        assert_eq!(p.len(), 40);
        assert!(p.iter().all(|v| *v > 0.5));
    }

    #[test]
    fn sniff() {
        assert!(is_audio(b"ID3\x04\0\0\0\0\0\0"));
        assert!(is_audio(b"fLaC\0\0\0\x22"));
        assert!(!is_audio(&[0xFF, 0xD8, 0xFF, 0xE0])); // JPEG
        assert!(!is_audio(b"GIF89a"));
    }
}
