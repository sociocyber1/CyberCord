//! Recording round video messages from the webcam (Telegram-style circles).
//!
//! Frames are captured on a worker thread, center-cropped to a square, scaled to
//! 240×240 and stored as JPEG at 15 fps (≈ 7 KB per frame, ≈ 6 MB per minute).
//! Real cameras need the `camera` feature (V4L2 on Linux, Media Foundation on
//! Windows). `CYBERCORD_TEST_CAMERA=1` uses a generated test picture instead,
//! so the whole pipeline can be tried on a machine without a webcam.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use eframe::egui::ColorImage;
use image::RgbImage;

use crate::media::{pcm_to_b64, CircleClip};

pub const FPS: u32 = 15;
pub const SIZE: u32 = 240;
pub const MAX_SECS: f32 = 60.0;

pub fn test_camera() -> bool {
    std::env::var("CYBERCORD_TEST_CAMERA").map(|v| v == "1").unwrap_or(false)
}

/// Can this build record at all (real camera support, or the test picture)?
pub fn supported() -> bool {
    cfg!(feature = "camera") || test_camera()
}

trait Source {
    fn frame(&mut self) -> Result<RgbImage, String>;
}

struct TestPattern {
    t: f32,
}

impl Source for TestPattern {
    fn frame(&mut self) -> Result<RgbImage, String> {
        std::thread::sleep(Duration::from_millis(1000 / FPS as u64));
        self.t += 1.0 / FPS as f32;
        let (w, h) = (320u32, 240u32);
        let t = self.t;
        let img = RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            // soft periwinkle → white gradient that drifts
            let g = 0.5 + 0.5 * ((fx * 3.0 + t * 0.8).sin() * (fy * 2.0 - t * 0.5).cos());
            let mut r = 153.0 + (255.0 - 153.0) * g;
            let mut gg = 193.0 + (255.0 - 193.0) * g;
            let mut b = 241.0 + (255.0 - 241.0) * g;
            // an orbiting accent dot
            let (cx, cy) = (160.0 + 70.0 * (t * 1.7).cos(), 120.0 + 50.0 * (t * 1.7).sin());
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d < 22.0 {
                r = 53.0;
                gg = 132.0;
                b = 228.0;
            }
            image::Rgb([r as u8, gg as u8, b as u8])
        });
        Ok(img)
    }
}

#[cfg(feature = "camera")]
struct Webcam {
    cam: nokhwa::Camera,
}

#[cfg(feature = "camera")]
impl Webcam {
    fn open() -> Result<Self, String> {
        use nokhwa::pixel_format::RgbFormat;
        use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution};
        let want = CameraFormat::new(Resolution::new(640, 480), FrameFormat::MJPEG, 30);
        let req = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(want));
        let mut cam = nokhwa::Camera::new(CameraIndex::Index(0), req).map_err(|e| e.to_string())?;
        cam.open_stream().map_err(|e| e.to_string())?;
        Ok(Self { cam })
    }
}

#[cfg(feature = "camera")]
impl Source for Webcam {
    fn frame(&mut self) -> Result<RgbImage, String> {
        use nokhwa::pixel_format::RgbFormat;
        let buf = self.cam.frame().map_err(|e| e.to_string())?;
        let img = buf.decode_image::<RgbFormat>().map_err(|e| e.to_string())?;
        let (w, h) = (img.width(), img.height());
        RgbImage::from_raw(w, h, img.into_raw()).ok_or_else(|| "bad frame".to_string())
    }
}

fn open_source() -> Result<Box<dyn Source>, String> {
    if test_camera() {
        return Ok(Box::new(TestPattern { t: 0.0 }));
    }
    #[cfg(feature = "camera")]
    {
        return Webcam::open().map(|w| Box::new(w) as Box<dyn Source>);
    }
    #[allow(unreachable_code)]
    Err("no-camera-build".into())
}

/// Square-crop the center, mirror like a selfie camera, scale to SIZE.
fn to_square(img: &RgbImage) -> RgbImage {
    let (w, h) = img.dimensions();
    let s = w.min(h);
    let crop = image::imageops::crop_imm(img, (w - s) / 2, (h - s) / 2, s, s).to_image();
    let flipped = image::imageops::flip_horizontal(&crop);
    image::imageops::resize(&flipped, SIZE, SIZE, image::imageops::FilterType::Triangle)
}

pub struct Recorder {
    preview: Arc<Mutex<Option<ColorImage>>>,
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    pub error: Arc<Mutex<Option<String>>>,
    started: Instant,
    thread: Option<std::thread::JoinHandle<()>>,
    mic: Option<crate::voice_rec::MicRecorder>,
}

impl Recorder {
    pub fn start() -> Self {
        let preview = Arc::new(Mutex::new(None));
        let frames = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let (p, f, s, e) = (preview.clone(), frames.clone(), stop.clone(), error.clone());
        let thread = std::thread::spawn(move || {
            let mut src = match open_source() {
                Ok(s) => s,
                Err(err) => {
                    *e.lock().unwrap() = Some(err);
                    return;
                }
            };
            let start = Instant::now();
            let mut next = 0u32;
            while !s.load(Ordering::Relaxed) && start.elapsed().as_secs_f32() < MAX_SECS {
                let img = match src.frame() {
                    Ok(i) => i,
                    Err(err) => {
                        *e.lock().unwrap() = Some(err);
                        return;
                    }
                };
                // Keep a steady 15 fps even if the camera delivers 30.
                let due = (start.elapsed().as_secs_f32() * FPS as f32) as u32;
                if due < next {
                    continue;
                }
                next = due + 1;
                let sq = to_square(&img);
                let mut jpg = Vec::new();
                let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 72);
                if enc.encode_image(&sq).is_ok() {
                    f.lock().unwrap().push(jpg);
                }
                let rgba = image::DynamicImage::ImageRgb8(sq).to_rgba8();
                *p.lock().unwrap() = Some(ColorImage::from_rgba_unmultiplied([SIZE as usize, SIZE as usize], rgba.as_raw()));
            }
        });
        Self { preview, frames, stop, error, started: Instant::now(), thread: Some(thread), mic: crate::voice_rec::MicRecorder::start() }
    }

    pub fn elapsed(&self) -> f32 {
        self.started.elapsed().as_secs_f32().min(MAX_SECS)
    }
    pub fn latest(&self) -> Option<ColorImage> {
        self.preview.lock().unwrap().clone()
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
    pub fn has_sound(&self) -> bool {
        self.mic.is_some()
    }
    pub fn auto_finished(&self) -> bool {
        self.started.elapsed().as_secs_f32() >= MAX_SECS
    }

    /// Stop and return the finished clip (None if nothing was recorded).
    pub fn finish(mut self) -> Option<CircleClip> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let frames = std::mem::take(&mut *self.frames.lock().unwrap());
        if frames.len() < 3 {
            return None;
        }
        let (audio_rate, audio) = match self.mic.take().map(|m| m.finish()) {
            Some((rate, pcm)) if !pcm.is_empty() => (rate, pcm_to_b64(&pcm)),
            _ => (0, String::new()),
        };
        Some(CircleClip {
            fps: FPS,
            size: SIZE,
            frames: frames.iter().map(|j| base64::engine::general_purpose::STANDARD.encode(j)).collect(),
            audio_rate,
            audio,
        })
    }

    pub fn cancel(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
