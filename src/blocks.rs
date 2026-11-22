//! Scales and charts inside chat messages. They travel as plain text, so any
//! client can read them and the author can edit them like any message:
//!
//! ```text
//! [scale: Готовность прототипа = 7/10 | осталось тестирование]
//!
//! [chart bar: Задачи за неделю]
//! Пн = 3
//! Вт = 5
//! [/chart]
//! ```

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};

#[derive(Debug, Clone, PartialEq)]
pub struct Scale {
    pub title: String,
    pub value: f64,
    pub max: f64,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChartKind {
    Bar,
    Line,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    pub kind: ChartKind,
    pub title: String,
    pub rows: Vec<(String, f64)>,
}

fn num(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse().ok()
}

impl Scale {
    pub fn parse(line: &str) -> Option<Scale> {
        let inner = line.trim().strip_prefix("[scale:")?.strip_suffix(']')?;
        let (main, note) = inner.split_once('|').unwrap_or((inner, ""));
        let (title, val) = main.rsplit_once('=')?;
        let (v, m) = val.split_once('/').unwrap_or((val, "10"));
        Some(Scale { title: title.trim().to_string(), value: num(v)?, max: num(m)?.max(0.0001), note: note.trim().to_string() })
    }
    pub fn to_text(&self) -> String {
        let note = if self.note.trim().is_empty() { String::new() } else { format!(" | {}", self.note.trim()) };
        format!("[scale: {} = {}/{}{note}]", self.title.trim(), fmt(self.value), fmt(self.max))
    }
}

pub fn fmt(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

impl Chart {
    /// Parse a chart starting at `lines[0]`; returns it and how many lines it used.
    pub fn parse(lines: &[&str]) -> Option<(Chart, usize)> {
        let head = lines.first()?.trim().strip_prefix("[chart")?.strip_suffix(']')?;
        let (kind_s, title) = head.split_once(':').unwrap_or((head, ""));
        let kind = if kind_s.trim() == "line" { ChartKind::Line } else { ChartKind::Bar };
        let mut rows = Vec::new();
        for (k, l) in lines.iter().enumerate().skip(1) {
            let t = l.trim();
            if t == "[/chart]" {
                return Some((Chart { kind, title: title.trim().to_string(), rows }, k + 1));
            }
            if let Some((a, b)) = t.rsplit_once('=') {
                if let Some(v) = num(b) {
                    rows.push((a.trim().to_string(), v));
                }
            }
        }
        None
    }
    pub fn to_text(&self) -> String {
        let mut s = format!("[chart {}: {}]\n", if self.kind == ChartKind::Line { "line" } else { "bar" }, self.title.trim());
        for (l, v) in &self.rows {
            s.push_str(&format!("{} = {}\n", l.trim(), fmt(*v)));
        }
        s.push_str("[/chart]");
        s
    }
}

/// Draw a scale: title, bar with ticks, value, note.
pub fn draw_scale(ui: &mut egui::Ui, s: &Scale, ink: Color32, ink2: Color32, accent: Color32, track: Color32) {
    let w = ui.available_width().min(420.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&s.title).color(ink).strong());
        ui.label(egui::RichText::new(format!("{} / {}", fmt(s.value), fmt(s.max))).color(accent).strong());
    });
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 14.0), egui::Sense::hover());
    let k = (s.value / s.max).clamp(0.0, 1.0) as f32;
    let p = ui.painter();
    p.rect_filled(r, Rounding::same(7.0), track);
    let col = if k < 0.34 { Color32::from_rgb(0xe0, 0x1b, 0x24) } else if k < 0.67 { Color32::from_rgb(0xe5, 0xa5, 0x0a) } else { Color32::from_rgb(0x26, 0xa2, 0x69) };
    p.rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * k, r.height())), Rounding::same(7.0), col);
    let steps = if s.max <= 10.0 && s.max >= 2.0 && (s.max - s.max.round()).abs() < 1e-9 { s.max as usize } else { 4 };
    for i in 1..steps {
        let x = r.min.x + r.width() * i as f32 / steps as f32;
        p.line_segment([Pos2::new(x, r.min.y + 3.0), Pos2::new(x, r.max.y - 3.0)], Stroke::new(1.0, Color32::from_white_alpha(120)));
    }
    if !s.note.is_empty() {
        ui.label(egui::RichText::new(&s.note).color(ink2).small());
    }
}

/// Draw a bar or line chart with labels and values.
pub fn draw_chart(ui: &mut egui::Ui, c: &Chart, ink: Color32, ink2: Color32, accent: Color32, grid: Color32) {
    if !c.title.is_empty() {
        ui.label(egui::RichText::new(&c.title).color(ink).strong());
    }
    let w = ui.available_width().min(460.0);
    let h = 170.0;
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::hover());
    let p = ui.painter();
    let plot = Rect::from_min_max(r.min + Vec2::new(34.0, 8.0), r.max - Vec2::new(8.0, 24.0));
    let maxv = c.rows.iter().map(|x| x.1).fold(0.0f64, f64::max).max(1e-9);
    let minv = c.rows.iter().map(|x| x.1).fold(0.0f64, f64::min);
    let span = (maxv - minv).max(1e-9);
    let y_of = |v: f64| plot.max.y - ((v - minv) / span) as f32 * plot.height();
    for i in 0..=4 {
        let v = minv + span * i as f64 / 4.0;
        let y = y_of(v);
        p.line_segment([Pos2::new(plot.min.x, y), Pos2::new(plot.max.x, y)], Stroke::new(1.0, grid));
        p.text(Pos2::new(plot.min.x - 4.0, y), Align2::RIGHT_CENTER, fmt((v * 100.0).round() / 100.0), FontId::proportional(10.0), ink2);
    }
    let n = c.rows.len().max(1);
    let slot = plot.width() / n as f32;
    let mut pts = Vec::new();
    let hover = resp.hover_pos();
    for (i, (label, v)) in c.rows.iter().enumerate() {
        let cx = plot.min.x + slot * (i as f32 + 0.5);
        let y = y_of(*v);
        let base = y_of(0.0f64.max(minv));
        if c.kind == ChartKind::Bar {
            let bw = (slot * 0.62).min(46.0);
            let bar = Rect::from_min_max(Pos2::new(cx - bw / 2.0, y.min(base)), Pos2::new(cx + bw / 2.0, y.max(base)));
            let hot = hover.map(|h| bar.expand2(Vec2::new(0.0, 6.0)).contains(h)).unwrap_or(false);
            p.rect_filled(bar, Rounding { nw: 4.0, ne: 4.0, sw: 0.0, se: 0.0 }, if hot { accent.gamma_multiply(1.2) } else { accent });
            p.text(Pos2::new(cx, bar.min.y - 2.0), Align2::CENTER_BOTTOM, fmt(*v), FontId::proportional(10.5), ink);
        } else {
            pts.push(Pos2::new(cx, y));
        }
        let short: String = label.chars().take(10).collect();
        p.text(Pos2::new(cx, plot.max.y + 4.0), Align2::CENTER_TOP, short, FontId::proportional(10.5), ink2);
    }
    if c.kind == ChartKind::Line && !pts.is_empty() {
        for w2 in pts.windows(2) {
            p.line_segment([w2[0], w2[1]], Stroke::new(2.5, accent));
        }
        for (i, q) in pts.iter().enumerate() {
            p.circle_filled(*q, 4.0, accent);
            p.text(*q - Vec2::new(0.0, 6.0), Align2::CENTER_BOTTOM, fmt(c.rows[i].1), FontId::proportional(10.5), ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_roundtrip() {
        let s = Scale::parse("[scale: Rust = 8/10 | уверенно]").unwrap();
        assert_eq!(s, Scale { title: "Rust".into(), value: 8.0, max: 10.0, note: "уверенно".into() });
        assert_eq!(Scale::parse(&s.to_text()).unwrap(), s);
        assert_eq!(Scale::parse("[scale: Готовность = 3,5/5]").unwrap().value, 3.5);
    }

    #[test]
    fn chart_roundtrip() {
        let text = "[chart line: Неделя]\nПн = 3\nВт = 5.5\n[/chart]";
        let lines: Vec<&str> = text.lines().collect();
        let (c, used) = Chart::parse(&lines).unwrap();
        assert_eq!(used, 4);
        assert_eq!(c.kind, ChartKind::Line);
        assert_eq!(c.rows, vec![("Пн".to_string(), 3.0), ("Вт".to_string(), 5.5)]);
        assert_eq!(c.to_text(), text);
    }
}
