//! "Hacked text" effect for new messages: random characters resolve into
//! the real text from left to right, then a short RGB-split glitch.
//! About 0.9 s in total, then the text is drawn normally.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, Response, TextFormat};

pub const SCRAMBLE: f32 = 0.55;
pub const GLITCH: f32 = 0.32;
pub const TOTAL: f32 = SCRAMBLE + GLITCH;

const NOISE: &[char] = &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', '#', '$', '%', '&', '@', '*', '+', '=', '<', '>', '/', '\\', '|', '{', '}', '[', ']', '_', '?', '!'];

fn rnd(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

pub fn seed_of(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) | 1
}

/// The text as it looks `t` seconds into the effect.
pub fn scrambled(text: &str, t: f32, seed: u64) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len().max(1) as f32;
    let frame = (t * 30.0) as u64;
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            // each character resolves at its own moment, roughly left to right
            let jitter = (rnd(seed ^ (i as u64 * 7919)) % 100) as f32 / 100.0 * 0.12;
            let reveal = (i as f32 / n) * (SCRAMBLE - 0.15) + jitter;
            if c.is_whitespace() || t >= reveal {
                c
            } else {
                NOISE[(rnd(seed ^ (i as u64) ^ (frame << 20)) % NOISE.len() as u64) as usize]
            }
        })
        .collect()
}

/// Draw `text` with the effect `t` seconds after it started.
pub fn label(ui: &mut egui::Ui, text: &str, t: f32, seed: u64, font: FontId, color: Color32, accent: Color32) -> Response {
    let shown = if t < SCRAMBLE { scrambled(text, t, seed) } else { text.to_string() };
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let col = if t < SCRAMBLE { accent } else { color };
    job.append(&shown, 0.0, TextFormat { font_id: font, color: col, ..Default::default() });
    let galley = ui.fonts(|f| f.layout_job(job));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    let p = ui.painter();
    if (SCRAMBLE..TOTAL).contains(&t) {
        // glitch: chromatic split + jitter + one bright scan line
        let k = 1.0 - (t - SCRAMBLE) / GLITCH;
        let r = rnd(seed ^ ((t * 60.0) as u64));
        let dx = 1.0 + (r % 4) as f32 * k;
        let dy = if r % 5 == 0 { 1.0 } else { 0.0 };
        p.galley_with_override_text_color(rect.min + egui::vec2(-dx, dy), galley.clone(), Color32::from_rgba_unmultiplied(0, 220, 255, (150.0 * k) as u8));
        p.galley_with_override_text_color(rect.min + egui::vec2(dx, -dy), galley.clone(), Color32::from_rgba_unmultiplied(255, 0, 140, (150.0 * k) as u8));
        p.galley(rect.min, galley, color);
        if r % 3 == 0 {
            let y = rect.min.y + (r % 97) as f32 / 97.0 * rect.height();
            p.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.min.x, y), egui::vec2(rect.width(), 1.5)), 0.0, accent.gamma_multiply(0.5 * k));
        }
    } else {
        p.galley(rect.min, galley, col);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_to_the_text() {
        let s = "Карта — не территория";
        assert_eq!(scrambled(s, SCRAMBLE, 7), s);
        let early = scrambled(s, 0.01, 7);
        assert_eq!(early.chars().count(), s.chars().count());
        assert_ne!(early, s);
    }
}
