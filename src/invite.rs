//! Inviting friends: a ready text, a QR code with the download link, and a
//! picture card (1200×630, the size social networks and messengers preview)
//! drawn here with the brand fonts — to post in a channel or send to a friend.

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};

pub const DOWNLOAD: &str = "https://github.com/sociocyber1/CyberCord/releases/latest";
const MONTSERRAT: &[u8] = include_bytes!("../assets/fonts/Montserrat-Regular.ttf");
const MONTSERRAT_MED: &[u8] = include_bytes!("../assets/fonts/Montserrat-Medium.ttf");
const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.otf");
const INTER_SEMI: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.otf");
const ICON: &[u8] = include_bytes!("../assets/icon-256.png");

/// The invitation text, ready to paste into any messenger.
pub fn text(ru: bool, my_handle: &str) -> String {
    if ru {
        format!(
            "Привет! Я общаюсь в CyberCord — чат и голосовые без серверов: каждый пользователь сам узел сети, переписка хранится у нас, а не у компании.\n\n\
             1. Скачай (Linux / Windows): {DOWNLOAD}\n\
             2. Нажми зелёный «+» слева и вставь мой ник: @{my_handle}\n\n\
             Мир без политики, нищеты и войн · SocioCyberneering Inc."
        )
    } else {
        format!(
            "Hi! I talk on CyberCord — chat and voice without servers: every user is a node, messages stay with us, not with a company.\n\n\
             1. Download (Linux / Windows): {DOWNLOAD}\n\
             2. Press the green \u{201c}+\u{201d} on the left and paste my handle: @{my_handle}\n\n\
             A world without politics, poverty and war · SocioCyberneering Inc."
        )
    }
}

/// QR modules as a square grid of booleans (true = dark).
pub fn qr_modules(data: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::with_error_correction_level(data.as_bytes(), qrcode::EcLevel::M).ok()?;
    let w = code.width();
    let v = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    Some((w, v))
}

/// QR code picture: `scale` pixels per module, a 4-module quiet zone.
pub fn qr_image(data: &str, scale: u32) -> Option<RgbaImage> {
    let (w, m) = qr_modules(data)?;
    let quiet = 4;
    let side = (w as u32 + quiet * 2) * scale;
    let mut img = RgbaImage::from_pixel(side, side, Rgba([255, 255, 255, 255]));
    for y in 0..w {
        for x in 0..w {
            if m[y * w + x] {
                for dy in 0..scale {
                    for dx in 0..scale {
                        img.put_pixel((x as u32 + quiet) * scale + dx, (y as u32 + quiet) * scale + dy, Rgba([11, 18, 32, 255]));
                    }
                }
            }
        }
    }
    Some(img)
}

fn blend(img: &mut RgbaImage, x: i32, y: i32, c: [u8; 3], a: f32) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 || a <= 0.0 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let a = a.min(1.0);
    for i in 0..3 {
        p[i] = (p[i] as f32 * (1.0 - a) + c[i] as f32 * a).round() as u8;
    }
}

/// Draw one line of text; returns its width. `spacing` is extra px between letters.
fn draw_text(img: &mut RgbaImage, font_bytes: &[u8], size: f32, x: f32, baseline: f32, text: &str, color: [u8; 3], spacing: f32) -> f32 {
    let Ok(font) = FontRef::try_from_slice(font_bytes) else { return 0.0 };
    let sf = font.as_scaled(PxScale::from(size));
    let mut pen = x;
    let mut prev = None;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            pen += sf.kern(p, id);
        }
        let g = id.with_scale_and_position(PxScale::from(size), point(pen, baseline));
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, cov| blend(img, b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, color, cov));
        }
        pen += sf.h_advance(id) + spacing;
        prev = Some(id);
    }
    pen - x - spacing
}

fn text_width(font_bytes: &[u8], size: f32, text: &str, spacing: f32) -> f32 {
    let Ok(font) = FontRef::try_from_slice(font_bytes) else { return 0.0 };
    let sf = font.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id) + spacing;
        prev = Some(id);
    }
    w - spacing
}

fn disc(img: &mut RgbaImage, cx: f32, cy: f32, r: f32, c: [u8; 3], a: f32) {
    for y in (cy - r - 1.0) as i32..=(cy + r + 1.0) as i32 {
        for x in (cx - r - 1.0) as i32..=(cx + r + 1.0) as i32 {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            blend(img, x, y, c, (r + 0.5 - d).clamp(0.0, 1.0) * a);
        }
    }
}

fn line(img: &mut RgbaImage, a: (f32, f32), b: (f32, f32), c: [u8; 3], alpha: f32) {
    let n = ((b.0 - a.0).hypot(b.1 - a.1)) as i32;
    for i in 0..=n {
        let t = i as f32 / n.max(1) as f32;
        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        blend(img, x as i32, y as i32, c, alpha);
        blend(img, x as i32, y as i32 + 1, c, alpha * 0.5);
    }
}

/// The invitation card. `tagline` and `motto` come in the UI language.
pub fn card(my_name: &str, tagline: &str, motto: &str, find_me: &str, scan: &str) -> RgbaImage {
    let (w, h) = (1200u32, 630u32);
    let mut img = RgbaImage::new(w, h);
    // night-sky gradient
    for y in 0..h {
        let t = y as f32 / h as f32;
        let c = [(11.0 + 6.0 * t) as u8, (18.0 + 24.0 * t) as u8, (32.0 + 35.0 * t) as u8];
        for x in 0..w {
            let glow = (1.0 - ((x as f32 - 300.0).hypot(y as f32 - 260.0) / 520.0)).max(0.0) * 26.0;
            img.put_pixel(x, y, Rgba([c[0], (c[1] as f32 + glow * 0.6) as u8, (c[2] as f32 + glow) as u8, 255]));
        }
    }
    // a faint peer-to-peer mesh on the right
    let nodes = [(820.0, 90.0), (1010.0, 60.0), (1140.0, 170.0), (900.0, 220.0), (1080.0, 330.0), (760.0, 380.0), (990.0, 590.0), (1150.0, 520.0), (820.0, 560.0)];
    let links = [(0, 1), (1, 2), (0, 3), (3, 2), (3, 4), (2, 4), (3, 5), (5, 6), (4, 6), (6, 7), (4, 7), (5, 8), (8, 6)];
    for (a, b) in links {
        line(&mut img, nodes[a], nodes[b], [92, 206, 255], 0.16);
    }
    for (x, y) in nodes {
        disc(&mut img, x, y, 7.0, [92, 206, 255], 0.35);
    }
    // planet logo
    if let Ok(icon) = image::load_from_memory(ICON) {
        let icon = image::imageops::resize(&icon.to_rgba8(), 150, 150, image::imageops::FilterType::Lanczos3);
        for (x, y, p) in icon.enumerate_pixels() {
            blend(&mut img, 70 + x as i32, 70 + y as i32, [p[0], p[1], p[2]], p[3] as f32 / 255.0);
        }
    }
    let white = [255, 255, 255];
    let soft = [176, 196, 222];
    let cyan = [120, 214, 255];
    draw_text(&mut img, MONTSERRAT_MED, 92.0, 245.0, 165.0, "CyberCord", white, 1.0);
    draw_text(&mut img, MONTSERRAT, 21.0, 250.0, 205.0, "SocioCyberneering Inc.", cyan, 6.0);
    // tagline, wrapped to the left column
    let mut y = 300.0;
    let mut cur = String::new();
    for word in tagline.split_whitespace() {
        let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if text_width(INTER_SEMI, 34.0, &next, 0.0) > 600.0 && !cur.is_empty() {
            draw_text(&mut img, INTER_SEMI, 34.0, 72.0, y, &cur, white, 0.0);
            y += 46.0;
            cur = word.to_string();
        } else {
            cur = next;
        }
    }
    if !cur.is_empty() {
        draw_text(&mut img, INTER_SEMI, 34.0, 72.0, y, &cur, white, 0.0);
        y += 46.0;
    }
    draw_text(&mut img, INTER, 22.0, 72.0, y + 14.0, motto, soft, 0.0);
    // who invites
    if !my_name.is_empty() {
        draw_text(&mut img, INTER_SEMI, 26.0, 72.0, 560.0, &find_me.replace("{name}", my_name), cyan, 0.0);
    }
    // QR card on the right
    if let Some(qr) = qr_image(DOWNLOAD, 7) {
        let (qx, qy) = (w as i32 - qr.width() as i32 - 80, 150);
        // rounded white backing
        let pad = 14;
        for yy in -pad..qr.height() as i32 + pad {
            for xx in -pad..qr.width() as i32 + pad {
                blend(&mut img, qx + xx, qy + yy, white, 1.0);
            }
        }
        for (x, yq, p) in qr.enumerate_pixels() {
            blend(&mut img, qx + x as i32, qy + yq as i32, [p[0], p[1], p[2]], 1.0);
        }
        let tw = text_width(INTER, 20.0, scan, 0.0);
        draw_text(&mut img, INTER, 20.0, qx as f32 + (qr.width() as f32 - tw) / 2.0, (qy + qr.height() as i32 + pad + 34) as f32, scan, soft, 0.0);
        let url = "github.com/sociocyber1/CyberCord";
        let uw = text_width(INTER, 18.0, url, 0.0);
        draw_text(&mut img, INTER, 18.0, qx as f32 + (qr.width() as f32 - uw) / 2.0, (qy + qr.height() as i32 + pad + 60) as f32, url, cyan, 0.0);
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_and_card() {
        let (w, m) = qr_modules(DOWNLOAD).unwrap();
        assert!(w >= 21 && m.len() == w * w);
        let img = card("Алиса", "Чат и голосовые без серверов", "Мир без политики, нищеты и войн", "Найди меня: {name}", "Скачать");
        assert_eq!((img.width(), img.height()), (1200, 630));
        assert!(text(true, "Алиса").contains(DOWNLOAD));
    }
}
