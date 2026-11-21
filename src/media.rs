//! Media in chat: decoding GIFs/images/video circles off the UI thread,
//! textures, and drawing round video (Telegram-style circles).

use std::collections::HashMap;
use std::sync::mpsc;

use base64::Engine as _;
use eframe::egui::{self, Color32, ColorImage, Pos2, Stroke, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};

use cybercord_node::store;
use cybercord_node::types::MediaKind;

/// A round video message: JPEG frames (+ optional mono PCM audio).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleClip {
    pub fps: u32,
    pub size: u32,
    /// Base64 JPEG frames.
    pub frames: Vec<String>,
    #[serde(default)]
    pub audio_rate: u32,
    /// Base64 little-endian i16 mono PCM.
    #[serde(default)]
    pub audio: String,
}

impl CircleClip {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
    pub fn dur_ms(&self) -> u32 {
        (self.frames.len() as u32 * 1000) / self.fps.max(1)
    }
}

pub fn pcm_to_b64(pcm: &[i16]) -> String {
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn pcm_from_b64(s: &str) -> Vec<i16> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map(|b| b.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
        .unwrap_or_default()
}

/// Guess the kind of a file from its bytes.
pub fn sniff(bytes: &[u8]) -> Option<MediaKind> {
    if bytes.starts_with(b"GIF8") {
        Some(MediaKind::Gif)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) || bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(MediaKind::Image)
    } else if bytes.starts_with(b"{\"fps\"") && CircleClip::decode(bytes).is_some() {
        Some(MediaKind::Circle)
    } else if crate::music::is_audio(bytes) {
        Some(MediaKind::Audio)
    } else {
        None
    }
}

/// Pixel size of an image/GIF, for the message metadata.
pub fn dimensions(bytes: &[u8]) -> (u32, u32) {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .unwrap_or((0, 0))
}

enum Decoded {
    Still(ColorImage),
    Anim(Vec<(ColorImage, u32)>),
    Circle { frames: Vec<ColorImage>, fps: u32, audio: Option<(u32, Vec<i16>)> },
}

pub enum Shown {
    Still { tex: TextureHandle, size: egui::Vec2 },
    Anim { frames: Vec<(TextureHandle, u32)>, total_ms: u32, size: egui::Vec2 },
    Circle { frames: Vec<TextureHandle>, fps: u32, audio: Option<(u32, Vec<i16>)> },
}

enum Entry {
    Loading,
    Ready(Shown),
    Failed,
}

fn to_color(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

fn shrink(img: image::RgbaImage, max: u32) -> image::RgbaImage {
    let (w, h) = img.dimensions();
    if w <= max && h <= max {
        return img;
    }
    let s = max as f32 / w.max(h) as f32;
    image::imageops::resize(&img, (w as f32 * s) as u32, (h as f32 * s) as u32, image::imageops::FilterType::Triangle)
}

fn decode(kind: MediaKind, bytes: &[u8]) -> Result<Decoded, String> {
    match kind {
        MediaKind::Gif => {
            use image::AnimationDecoder;
            let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for f in dec.into_frames().take(400) {
                let f = f.map_err(|e| e.to_string())?;
                let (n, d) = f.delay().numer_denom_ms();
                let ms = if d == 0 { 100 } else { (n / d).max(20) };
                out.push((to_color(&shrink(f.into_buffer(), 360)), ms));
            }
            if out.is_empty() {
                return Err("empty gif".into());
            }
            Ok(Decoded::Anim(out))
        }
        MediaKind::Image => {
            let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
            Ok(Decoded::Still(to_color(&shrink(img, 480))))
        }
        MediaKind::Circle => {
            let clip = CircleClip::decode(bytes).ok_or("bad circle")?;
            let mut frames = Vec::with_capacity(clip.frames.len());
            for b in &clip.frames {
                let jpg = base64::engine::general_purpose::STANDARD.decode(b).map_err(|e| e.to_string())?;
                let img = image::load_from_memory(&jpg).map_err(|e| e.to_string())?.to_rgba8();
                frames.push(to_color(&img));
            }
            let audio = (!clip.audio.is_empty() && clip.audio_rate > 0).then(|| (clip.audio_rate, pcm_from_b64(&clip.audio)));
            Ok(Decoded::Circle { frames, fps: clip.fps.max(1), audio })
        }
        MediaKind::Audio => Err("audio is shown by the music player".into()),
        MediaKind::File => Err("files are downloaded on demand".into()),
    }
}

/// Decoded media keyed by blob hash; decoding happens on a worker thread.
pub struct MediaCache {
    entries: HashMap<String, Entry>,
    tx: mpsc::Sender<(String, Result<Decoded, String>)>,
    rx: mpsc::Receiver<(String, Result<Decoded, String>)>,
}

impl Default for MediaCache {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { entries: HashMap::new(), tx, rx }
    }
}

impl MediaCache {
    /// Upload finished decodes as textures. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((hash, res)) = self.rx.try_recv() {
            let entry = match res {
                Ok(Decoded::Still(img)) => {
                    let size = egui::vec2(img.size[0] as f32, img.size[1] as f32);
                    Entry::Ready(Shown::Still { tex: ctx.load_texture(&hash, img, TextureOptions::LINEAR), size })
                }
                Ok(Decoded::Anim(frames)) => {
                    let size = egui::vec2(frames[0].0.size[0] as f32, frames[0].0.size[1] as f32);
                    let total_ms = frames.iter().map(|f| f.1).sum::<u32>().max(1);
                    let frames = frames
                        .into_iter()
                        .enumerate()
                        .map(|(i, (img, ms))| (ctx.load_texture(format!("{hash}#{i}"), img, TextureOptions::LINEAR), ms))
                        .collect();
                    Entry::Ready(Shown::Anim { frames, total_ms, size })
                }
                Ok(Decoded::Circle { frames, fps, audio }) => {
                    let frames = frames
                        .into_iter()
                        .enumerate()
                        .map(|(i, img)| ctx.load_texture(format!("{hash}#{i}"), img, TextureOptions::LINEAR))
                        .collect();
                    Entry::Ready(Shown::Circle { frames, fps, audio })
                }
                Err(e) => {
                    tracing::warn!("media {hash}: {e}");
                    Entry::Failed
                }
            };
            self.entries.insert(hash, entry);
            ctx.request_repaint();
        }
    }

    /// The decoded media if ready; starts decoding when the blob is on disk.
    /// Returns Err(()) if decoding failed.
    pub fn get(&mut self, hash: &str, kind: MediaKind) -> Result<Option<&Shown>, ()> {
        if !self.entries.contains_key(hash) {
            if !store::has_blob(hash) {
                return Ok(None);
            }
            self.entries.insert(hash.to_string(), Entry::Loading);
            let tx = self.tx.clone();
            let h = hash.to_string();
            std::thread::spawn(move || {
                let res = store::load_blob(&h).ok_or_else(|| "missing".to_string()).and_then(|b| decode(kind, &b));
                let _ = tx.send((h, res));
            });
        }
        match self.entries.get(hash) {
            Some(Entry::Ready(s)) => Ok(Some(s)),
            Some(Entry::Failed) => Err(()),
            _ => Ok(None),
        }
    }
}

/// Paint a texture clipped to a circle (triangle fan with circular UVs).
pub fn paint_circle(painter: &egui::Painter, center: Pos2, radius: f32, tex: egui::TextureId, mirror: bool) {
    let n = 72;
    let mut mesh = egui::Mesh::with_texture(tex);
    mesh.vertices.push(egui::epaint::Vertex { pos: center, uv: Pos2::new(0.5, 0.5), color: Color32::WHITE });
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        let u = if mirror { 0.5 - 0.5 * c } else { 0.5 + 0.5 * c };
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center + egui::vec2(c, s) * radius,
            uv: Pos2::new(u, 0.5 + 0.5 * s),
            color: Color32::WHITE,
        });
    }
    for i in 1..=n as u32 {
        mesh.indices.extend_from_slice(&[0, i, i + 1]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Arc from 12 o'clock, clockwise, covering `frac` (0..1) of the circle.
pub fn progress_ring(painter: &egui::Painter, center: Pos2, radius: f32, frac: f32, color: Color32, width: f32) {
    let frac = frac.clamp(0.0, 1.0);
    if frac <= 0.0 {
        return;
    }
    let steps = (96.0 * frac).max(2.0) as usize;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU * (i as f32 / steps as f32);
            center + egui::vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(width, color)));
}
