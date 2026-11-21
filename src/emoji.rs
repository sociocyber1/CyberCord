//! Color emoji (Twemoji, CC-BY 4.0) rendered as images, a picker, and
//! `:shortcode:` support. Messages carry plain Unicode, so any client can read them.

use std::collections::HashMap;
use std::sync::OnceLock;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

#[path = "emoji_data.rs"]
mod data;
#[path = "emoji_sce.rs"]
mod sce;

/// Categories in picker order: (category id, icon emoji index found at runtime).
/// "sce" comes first: our own general-semantics / SocioCyberneering set.
/// Those have no Unicode code point, so messages carry their `:shortcode:`.
pub const CATEGORIES: &[&str] = &["sce", "smileys", "people", "hearts", "nature", "food", "activity", "symbols"];

pub fn count() -> usize {
    data::EMOJI.len() + sce::SCE.len()
}
/// What goes into the message: the Unicode emoji, or `:code:` for our own set.
pub fn ch(i: usize) -> &'static str {
    if i < data::EMOJI.len() {
        data::EMOJI[i].0
    } else {
        sce::SCE[i - data::EMOJI.len()].0
    }
}
pub fn code(i: usize) -> &'static str {
    if i < data::EMOJI.len() {
        data::EMOJI[i].1
    } else {
        sce::SCE[i - data::EMOJI.len()].0.trim_matches(':')
    }
}
pub fn cat(i: usize) -> &'static str {
    if i < data::EMOJI.len() {
        data::EMOJI[i].2
    } else {
        "sce"
    }
}
/// Short English description (our own set only).
pub fn about(i: usize) -> Option<&'static str> {
    i.checked_sub(data::EMOJI.len()).and_then(|k| sce::SCE.get(k)).map(|e| e.1)
}
fn png(i: usize) -> &'static [u8] {
    if i < data::EMOJI.len() {
        data::EMOJI[i].3
    } else {
        sce::SCE[i - data::EMOJI.len()].2
    }
}

fn key(s: &str) -> String {
    s.chars().filter(|c| *c != '\u{FE0F}').collect()
}

fn by_char() -> &'static HashMap<String, usize> {
    static M: OnceLock<HashMap<String, usize>> = OnceLock::new();
    M.get_or_init(|| (0..data::EMOJI.len()).map(|i| (key(ch(i)), i)).collect())
}

fn by_code() -> &'static HashMap<&'static str, usize> {
    static M: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    M.get_or_init(|| (0..count()).map(|i| (code(i), i)).collect())
}

pub fn find_code(code: &str) -> Option<usize> {
    by_code().get(code).copied()
}

/// First emoji of a category (used as the tab icon).
pub fn cat_icon(cat_id: &str) -> usize {
    (0..count()).find(|i| cat(*i) == cat_id).unwrap_or(0)
}

/// A piece of a message: text, or one emoji we can draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Text(String),
    Emoji(usize),
}

/// Split text into text runs and emoji (Unicode or `:shortcode:`).
pub fn segment(text: &str) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let flush = |buf: &mut String, out: &mut Vec<Seg>| {
        if !buf.is_empty() {
            out.push(Seg::Text(std::mem::take(buf)));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        // :shortcode:
        if c == ':' {
            if let Some(end) = chars[i + 1..].iter().take(32).position(|x| *x == ':') {
                let name: String = chars[i + 1..i + 1 + end].iter().collect();
                if let Some(idx) = find_code(&name) {
                    flush(&mut buf, &mut out);
                    out.push(Seg::Emoji(idx));
                    i += end + 2;
                    continue;
                }
            }
        }
        if let Some(&idx) = by_char().get(&c.to_string()) {
            flush(&mut buf, &mut out);
            out.push(Seg::Emoji(idx));
            i += 1;
            if i < chars.len() && chars[i] == '\u{FE0F}' {
                i += 1;
            }
            continue;
        }
        if c != '\u{FE0F}' {
            buf.push(c);
        }
        i += 1;
    }
    flush(&mut buf, &mut out);
    out
}

/// `:smile:` → 😄 before sending, so other clients see real emoji.
pub fn shortcodes_to_unicode(text: &str) -> String {
    segment(text)
        .into_iter()
        .map(|s| match s {
            Seg::Text(t) => t,
            Seg::Emoji(i) => ch(i).to_string(),
        })
        .collect()
}

/// 1–4 emoji and nothing else: shown big, like in Telegram/Discord.
pub fn is_emoji_only(segs: &[Seg]) -> bool {
    let n = segs.iter().filter(|s| matches!(s, Seg::Emoji(_))).count();
    n > 0 && n <= 4 && segs.iter().all(|s| matches!(s, Seg::Emoji(_)) || matches!(s, Seg::Text(t) if t.trim().is_empty()))
}

/// Decoded emoji textures, created on first use.
#[derive(Default)]
pub struct EmojiCache {
    tex: HashMap<usize, TextureHandle>,
}

impl EmojiCache {
    pub fn tex(&mut self, ctx: &egui::Context, idx: usize) -> egui::TextureId {
        self.tex
            .entry(idx)
            .or_insert_with(|| {
                let img = image::load_from_memory(png(idx))
                    .map(|i| i.to_rgba8())
                    .unwrap_or_else(|_| image::RgbaImage::new(1, 1));
                let ci = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                ctx.load_texture(format!("emoji{idx}"), ci, TextureOptions::LINEAR)
            })
            .id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn segments_unicode_and_shortcodes() {
        let s = segment("hi 😀 and :fire: :nope: ❤️!");
        assert!(s.contains(&Seg::Emoji(find_code("grinning").unwrap())));
        assert!(s.contains(&Seg::Emoji(find_code("fire").unwrap())));
        assert!(s.contains(&Seg::Emoji(find_code("heart").unwrap())));
        assert!(s.iter().any(|x| matches!(x, Seg::Text(t) if t.contains(":nope:"))));
        assert_eq!(shortcodes_to_unicode(":thumbsup:"), "👍");
        assert!(is_emoji_only(&segment("🔥🔥")));
        assert!(!is_emoji_only(&segment("ok 🔥")));
    }
}
