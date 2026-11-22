//! Start-up: the intro video (skippable), then a short splash with the
//! CyberCord planet, the motto and the SocioCyberneering Inc. wordmark.
//!
//! The video is H.264 (baseline) + AAC in MP4, shipped inside the program.
//! Frames are decoded with OpenH264 on a background thread; the sound is
//! decoded with symphonia and played through the normal music player.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};

pub const VIDEO: &[u8] = include_bytes!("../assets/video/intro.mp4");

pub struct Intro {
    rx: Receiver<(f64, ColorImage)>,
    queue: VecDeque<(f64, ColorImage)>,
    tex: Option<TextureHandle>,
    start: Option<Instant>,
    pub duration: f64,
    finished_decoding: bool,
    _audio: Option<crate::music::Player>,
}

impl Intro {
    pub fn start() -> Option<Intro> {
        let duration = {
            let mp4 = mp4::Mp4Reader::read_header(Cursor::new(VIDEO), VIDEO.len() as u64).ok()?;
            mp4.duration().as_secs_f64()
        };
        let (tx, rx) = sync_channel::<(f64, ColorImage)>(24);
        std::thread::Builder::new().name("intro-video".into()).spawn(move || decode(tx)).ok()?;
        // sound: decode the AAC track up front (a few MB of samples) and play it
        let audio = if crate::music::CAN_PLAY { crate::music::decode(VIDEO).ok().and_then(|t| crate::music::play(Arc::new(t))) } else { None };
        Some(Intro { rx, queue: VecDeque::new(), tex: None, start: None, duration, finished_decoding: false, _audio: audio })
    }

    /// Seconds since the first frame was shown.
    pub fn elapsed(&self) -> f64 {
        self.start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0)
    }

    /// Draw the current frame letterboxed into `rect`. Returns false when the video is over.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
        let ctx = ui.ctx().clone();
        loop {
            match self.rx.try_recv() {
                Ok(f) => {
                    self.queue.push_back(f);
                    if self.queue.len() >= 24 {
                        break;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.finished_decoding = true;
                    break;
                }
            }
        }
        if self.start.is_none() && !self.queue.is_empty() {
            self.start = Some(Instant::now());
        }
        let t = self.elapsed();
        // show the newest frame whose time has come
        let mut next: Option<ColorImage> = None;
        while self.queue.front().map(|(pts, _)| *pts <= t).unwrap_or(false) {
            next = self.queue.pop_front().map(|(_, img)| img);
        }
        if let Some(img) = next {
            match &mut self.tex {
                Some(tex) => tex.set(img, TextureOptions::LINEAR),
                None => self.tex = Some(ctx.load_texture("intro", img, TextureOptions::LINEAR)),
            }
        }
        ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
        if let Some(tex) = &self.tex {
            let [w, h] = tex.size();
            let k = (rect.width() / w as f32).min(rect.height() / h as f32);
            let r = egui::Rect::from_center_size(rect.center(), egui::vec2(w as f32 * k, h as f32 * k));
            ui.painter().image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        ctx.request_repaint();
        !(self.finished_decoding && self.queue.is_empty() && t > 0.5) && t < self.duration + 1.0
    }
}

fn decode(tx: std::sync::mpsc::SyncSender<(f64, ColorImage)>) {
    use openh264::decoder::Decoder;
    use openh264::formats::YUVSource;
    let Ok(mut mp4) = mp4::Mp4Reader::read_header(Cursor::new(VIDEO), VIDEO.len() as u64) else { return };
    let Some((tid, n, ts, sps, pps)) = mp4.tracks().values().find(|t| t.media_type().ok() == Some(mp4::MediaType::H264)).and_then(|t| {
        Some((t.track_id(), t.sample_count(), t.timescale() as f64, t.sequence_parameter_set().ok()?.to_vec(), t.picture_parameter_set().ok()?.to_vec()))
    }) else {
        return;
    };
    let Ok(mut dec) = Decoder::new() else { return };
    let mut buf = Vec::new();
    for i in 1..=n {
        let Ok(Some(s)) = mp4.read_sample(tid, i) else { continue };
        // MP4 stores length-prefixed NAL units; OpenH264 wants start codes
        buf.clear();
        if i == 1 {
            for ps in [&sps, &pps] {
                buf.extend_from_slice(&[0, 0, 0, 1]);
                buf.extend_from_slice(ps);
            }
        }
        let b = &s.bytes;
        let mut p = 0;
        while p + 4 <= b.len() {
            let l = u32::from_be_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]) as usize;
            p += 4;
            buf.extend_from_slice(&[0, 0, 0, 1]);
            buf.extend_from_slice(&b[p..(p + l).min(b.len())]);
            p += l;
        }
        if let Ok(Some(img)) = dec.decode(&buf) {
            let (w, h) = img.dimensions();
            let mut rgb = vec![0u8; w * h * 3];
            img.write_rgb8(&mut rgb);
            let ci = ColorImage::from_rgb([w, h], &rgb);
            let pts = (s.start_time as f64 + s.rendering_offset as f64) / ts;
            if tx.send((pts, ci)).is_err() {
                return; // skipped
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn intro_has_video_and_sound() {
        let mp4 = mp4::Mp4Reader::read_header(std::io::Cursor::new(super::VIDEO), super::VIDEO.len() as u64).unwrap();
        assert!(mp4.duration().as_secs() > 100);
        let t = crate::music::decode(super::VIDEO).expect("AAC track decodes");
        assert!(t.frames() as f64 / t.rate as f64 > 100.0);
    }
}
