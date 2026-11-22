//! In-app reader for the small Markdown documents in `docs/`
//! ("Words matter", "The price of words", "About SocioCyberneering Inc.").
//!
//! The same `.md` files are shown on GitHub, so pictures are written as
//! `![alt](img/name.svg)`; here they are mapped to embedded PNG renders.
//! Supported: `#`/`##` headings, paragraphs with `**bold**` / `*italic*`,
//! `- ` lists, `---`, and links `[text](url)` (clickable in lists and on
//! their own line; shown as text inside paragraphs).

use crate::i18n::Lang;
use crate::theme::p as th;
use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, RichText, TextFormat};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Doc {
    WordsMatter,
    Consequences,
    Sce,
    Atom,
}

impl Doc {
    pub fn from_id(id: &str) -> Option<Doc> {
        match id {
            "words" => Some(Doc::WordsMatter),
            "cons" | "consequences" => Some(Doc::Consequences),
            "sce" => Some(Doc::Sce),
            "atom" => Some(Doc::Atom),
            _ => None,
        }
    }

    /// Russian text for ru/uk, English for everyone else.
    pub fn text(self, lang: Lang) -> &'static str {
        let ru = matches!(lang, Lang::Ru | Lang::Uk);
        match (self, ru) {
            (Doc::WordsMatter, true) => include_str!("../docs/words-matter.ru.md"),
            (Doc::WordsMatter, false) => include_str!("../docs/words-matter.en.md"),
            (Doc::Consequences, true) => include_str!("../docs/consequences.ru.md"),
            (Doc::Consequences, false) => include_str!("../docs/consequences.en.md"),
            (Doc::Sce, true) => include_str!("../docs/sociocyberneering.ru.md"),
            (Doc::Sce, false) => include_str!("../docs/sociocyberneering.en.md"),
            (Doc::Atom, true) => include_str!("../docs/atom-generator.ru.md"),
            (Doc::Atom, false) => include_str!("../docs/atom-generator.en.md"),
        }
    }
}

pub fn image(stem: &str) -> Option<(&'static str, &'static [u8])> {
    Some(match stem {
        "words-map" => ("doc_words_map", include_bytes!("../assets/docs/words-map.png")),
        "words-bridge" => ("doc_words_bridge", include_bytes!("../assets/docs/words-bridge.png")),
        "consequences-chain" => ("doc_cons_chain", include_bytes!("../assets/docs/consequences-chain.png")),
        "consequences-planet" => ("doc_cons_planet", include_bytes!("../assets/docs/consequences-planet.png")),
        "sce-city" => ("doc_sce_city", include_bytes!("../assets/docs/sce-city.png")),
        "atom-generator" => ("doc_atom", include_bytes!("../assets/docs/atom-generator.png")),
        _ => return None,
    })
}

/// One parsed block of a document.
#[derive(Debug, PartialEq)]
pub enum Block<'a> {
    H1(&'a str),
    H2(&'a str),
    Para(String),
    Item(String),
    Image(&'a str),
    Link(&'a str, &'a str),
    Rule,
}

/// `[text](url)` → (text, url) if the whole string is a link.
fn whole_link(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    let rest = s.strip_prefix('[')?;
    let close = rest.find("](")?;
    let url = rest[close + 2..].strip_suffix(')')?;
    if url.contains(' ') {
        return None;
    }
    Some((&rest[..close], url))
}

pub fn parse(md: &str) -> Vec<Block<'_>> {
    let mut out = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    let flush = |para: &mut Vec<&str>, out: &mut Vec<Block>| {
        if !para.is_empty() {
            out.push(Block::Para(para.join(" ")));
            para.clear();
        }
    };
    for line in md.lines() {
        let l = line.trim_end();
        if l.trim().is_empty() {
            flush(&mut para, &mut out);
        } else if let Some(h) = l.strip_prefix("# ") {
            flush(&mut para, &mut out);
            out.push(Block::H1(h.trim()));
        } else if let Some(h) = l.strip_prefix("## ") {
            flush(&mut para, &mut out);
            out.push(Block::H2(h.trim()));
        } else if l.trim() == "---" {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
        } else if let Some(img) = l.strip_prefix("![") {
            flush(&mut para, &mut out);
            if let Some(start) = img.find("](") {
                let path = img[start + 2..].trim_end_matches(')');
                let stem = path.rsplit('/').next().unwrap_or(path);
                let stem = stem.rsplit_once('.').map(|(a, _)| a).unwrap_or(stem);
                out.push(Block::Image(stem));
            }
        } else if let Some(it) = l.strip_prefix("- ") {
            flush(&mut para, &mut out);
            out.push(Block::Item(it.trim().to_string()));
        } else if l.starts_with("  ") && para.is_empty() && matches!(out.last(), Some(Block::Item(_))) {
            // continuation of a list item
            if let Some(Block::Item(t)) = out.last_mut() {
                t.push(' ');
                t.push_str(l.trim());
            }
        } else if let Some((t, u)) = whole_link(l) {
            flush(&mut para, &mut out);
            out.push(Block::Link(t, u));
        } else {
            para.push(l.trim());
        }
    }
    flush(&mut para, &mut out);
    out
}

/// Inline `**bold**`, `*italic*` and `[t](u)` → a wrapped layout job.
fn inline_job(text: &str, size: f32, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    let mut bold = false;
    let mut italic = false;
    let mut buf = String::new();
    let push = |job: &mut LayoutJob, buf: &mut String, bold: bool, italic: bool| {
        if buf.is_empty() {
            return;
        }
        let family = if bold { FontFamily::Name("bold".into()) } else { FontFamily::Proportional };
        let fmt = TextFormat { font_id: FontId::new(size, family), color: if bold { th().ink } else { color }, italics: italic, line_height: Some(size * 1.5), ..Default::default() };
        job.append(buf, 0.0, fmt);
        buf.clear();
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            push(&mut job, &mut buf, bold, italic);
            bold = !bold;
            i += 2;
            continue;
        }
        if c == '*' {
            push(&mut job, &mut buf, bold, italic);
            italic = !italic;
            i += 1;
            continue;
        }
        if c == '[' {
            // [text](url) inside a paragraph: keep the text only
            let rest: String = chars[i + 1..].iter().collect();
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close + 2..].find(')') {
                    buf.push_str(&rest[..close]);
                    let consumed = rest[..close + 2 + end + 1].chars().count();
                    i += 1 + consumed;
                    continue;
                }
            }
        }
        buf.push(c);
        i += 1;
    }
    push(&mut job, &mut buf, bold, italic);
    job
}

/// Draw a document. `tex` turns an embedded PNG into a texture.
/// The brand name, always in Montserrat with wide letter spacing.
pub fn brand(ui: &mut egui::Ui, text: &str, size: f32, color: Color32) -> egui::Response {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat { font_id: FontId::new(size, FontFamily::Name("brand".into())), color, extra_letter_spacing: size * 0.12, ..Default::default() });
    ui.label(job)
}

/// A link to a file shipped with the docs (e.g. a PDF): opened in the app.
fn internal(url: &str) -> bool {
    !url.contains("://") && url.ends_with(".pdf")
}

/// Draw a document. `tex` turns an embedded PNG into a texture. Returns the
/// target of a clicked internal link (a built-in PDF), if any.
pub fn show(ui: &mut egui::Ui, md: &str, tex: &mut dyn FnMut(&egui::Context, &'static str, &'static [u8]) -> egui::TextureId) -> Option<String> {
    let c = th();
    let w = ui.available_width();
    let mut clicked: Option<String> = None;
    for b in parse(md) {
        match b {
            Block::H1(t) if t == "SocioCyberneering Inc." => {
                brand(ui, t, 26.0, c.ink);
                ui.add_space(10.0);
            }
            Block::H1(t) => {
                ui.add(egui::Label::new(RichText::new(t).color(c.ink).size(26.0).family(FontFamily::Name("bold".into()))).wrap());
                ui.add_space(10.0);
            }
            Block::H2(t) => {
                ui.add_space(12.0);
                ui.add(egui::Label::new(RichText::new(t).color(c.ink).size(18.0).family(FontFamily::Name("bold".into()))).wrap());
                ui.add_space(4.0);
            }
            Block::Para(t) => {
                ui.add(egui::Label::new(inline_job(&t, 15.0, c.ink, w)).wrap());
                ui.add_space(8.0);
            }
            Block::Item(t) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new("•").color(c.accent).size(15.0));
                    if let Some((text, url)) = whole_link(&t).filter(|(_, u)| internal(u)) {
                        if ui.link(RichText::new(format!("PDF · {text}")).size(15.0).color(c.accent)).clicked() {
                            clicked = Some(url.to_string());
                        }
                    } else if let Some((text, url)) = whole_link(&t) {
                        ui.hyperlink_to(RichText::new(text).size(15.0).color(c.accent), url).on_hover_text(url);
                    } else {
                        ui.add(egui::Label::new(inline_job(&t, 15.0, c.ink, w - 28.0)).wrap());
                    }
                });
                ui.add_space(3.0);
            }
            Block::Link(text, url) if internal(url) => {
                if ui.link(RichText::new(format!("PDF · {text}")).size(15.0).color(c.accent)).clicked() {
                    clicked = Some(url.to_string());
                }
                ui.add_space(6.0);
            }
            Block::Link(text, url) => {
                ui.hyperlink_to(RichText::new(text).size(15.0).color(c.accent), url).on_hover_text(url);
                ui.add_space(6.0);
            }
            Block::Image(stem) => {
                if let Some((key, png)) = image(stem) {
                    let id = tex(ui.ctx(), key, png);
                    let iw = w.min(560.0);
                    ui.add_space(4.0);
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(id, egui::vec2(iw, iw * 300.0 / 560.0))).rounding(egui::Rounding::same(14.0)));
                    });
                    ui.add_space(12.0);
                }
            }
            Block::Rule => {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
            }
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_documents_parse_and_have_pictures() {
        for d in [Doc::WordsMatter, Doc::Consequences, Doc::Sce, Doc::Atom] {
            for l in [Lang::Ru, Lang::En] {
                let blocks = parse(d.text(l));
                assert!(matches!(blocks.first(), Some(Block::H1(_))), "{d:?}");
                for b in &blocks {
                    if let Block::Image(s) = b {
                        assert!(image(s).is_some(), "missing picture {s}");
                    }
                }
                assert!(blocks.iter().any(|b| matches!(b, Block::Image(_))));
                assert!(blocks.iter().any(|b| matches!(b, Block::Item(t) if whole_link(t).is_some())));
            }
        }
        assert_eq!(whole_link("[a b](https://x.y/z)"), Some(("a b", "https://x.y/z")));
    }
}
