//! A small PDF reader: pages are rasterised with `hayro` (pure Rust) on a
//! background thread, only around what is on screen.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use eframe::egui::{self, Color32, ColorImage, RichText, Rounding, TextureHandle, TextureOptions};

/// Built-in documents (shipped inside the program).
pub const VENUS: &[u8] = include_bytes!("../assets/pdf/venus-project.pdf");
pub const GS: &[u8] = include_bytes!("../assets/pdf/general-semantics.pdf");

/// Title and bytes of a built-in PDF by file name (as linked from the docs).
pub fn builtin(name: &str) -> Option<(&'static str, &'static str, &'static [u8])> {
    let file = name.rsplit('/').next().unwrap_or(name);
    match file {
        "venus-project.pdf" => Some(("Жак Фреско и Проект Венера", "venus-project.pdf", VENUS)),
        "general-semantics.pdf" => Some(("Наука о Человеке: Общая семантика — что это и зачем нужно?", "general-semantics.pdf", GS)),
        _ => None,
    }
}

enum Msg {
    Meta(Vec<(f32, f32)>),
    Page(usize, f32, ColorImage),
    Error(String),
}

pub struct PdfView {
    pub title: String,
    pub file_name: String,
    data: Arc<Vec<u8>>,
    sizes: Vec<(f32, f32)>,
    tex: HashMap<usize, (f32, TextureHandle)>,
    pending: HashSet<(usize, u32)>,
    req: Sender<(usize, f32)>,
    res: Receiver<Msg>,
    zoom: f32,
    page_now: usize,
    jump: Option<usize>,
    error: Option<String>,
}

impl PdfView {
    pub fn open(title: impl Into<String>, file_name: impl Into<String>, data: Arc<Vec<u8>>) -> Self {
        let (req_tx, req_rx) = channel::<(usize, f32)>();
        let (res_tx, res_rx) = channel::<Msg>();
        let d = data.clone();
        std::thread::Builder::new()
            .name("pdf".into())
            .spawn(move || render_thread(d, req_rx, res_tx))
            .ok();
        Self { title: title.into(), file_name: file_name.into(), data, sizes: Vec::new(), tex: HashMap::new(), pending: HashSet::new(), req: req_tx, res: res_rx, zoom: 1.0, page_now: 0, jump: None, error: None }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Draw the reader filling `ui`. Returns what the toolbar asked for.
    pub fn show(&mut self, ui: &mut egui::Ui, labels: &Labels) -> Action {
        let ctx = ui.ctx().clone();
        while let Ok(m) = self.res.try_recv() {
            match m {
                Msg::Meta(s) => self.sizes = s,
                Msg::Page(i, scale, img) => {
                    self.pending.retain(|(p, _)| *p != i);
                    let t = ctx.load_texture(format!("pdf{i}"), img, TextureOptions::LINEAR);
                    self.tex.insert(i, (scale, t));
                }
                Msg::Error(e) => self.error = Some(e),
            }
        }
        let c = crate::theme::p();
        let mut act = Action::None;
        // toolbar
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.set_height(44.0);
            ui.label(RichText::new(&self.title).color(c.ink).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                if ui.add(egui::Button::new(RichText::new(labels.close).color(c.on_accent).strong()).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                    act = Action::Close;
                }
                if ui.button(labels.open).clicked() {
                    act = Action::OpenExternal;
                }
                if ui.button(labels.save).clicked() {
                    act = Action::Save;
                }
                ui.separator();
                if ui.button("＋").clicked() {
                    self.zoom = (self.zoom * 1.2).min(3.0);
                }
                ui.label(RichText::new(format!("{:.0}%", self.zoom * 100.0)).color(c.ink2));
                if ui.button("－").clicked() {
                    self.zoom = (self.zoom / 1.2).max(0.4);
                }
                ui.separator();
                let n = self.sizes.len().max(1);
                if ui.add_enabled(self.page_now + 1 < n, egui::Button::new("›")).clicked() {
                    self.jump = Some(self.page_now + 1);
                }
                ui.label(RichText::new(format!("{} {} / {}", labels.page, self.page_now + 1, n)).color(c.ink2));
                if ui.add_enabled(self.page_now > 0, egui::Button::new("‹")).clicked() {
                    self.jump = Some(self.page_now - 1);
                }
            });
        });
        ui.separator();
        if let Some(e) = &self.error {
            ui.colored_label(c.red, format!("{}: {e}", labels.cannot));
            return act;
        }
        if self.sizes.is_empty() {
            ui.spinner();
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return act;
        }
        let avail_w = ui.available_width();
        let ppp = ctx.pixels_per_point();
        let gap = 12.0;
        let widths: Vec<f32> = self.sizes.iter().map(|(w, _)| (avail_w - 40.0).min(860.0) * self.zoom * (*w / self.sizes[0].0.max(1.0))).collect();
        let heights: Vec<f32> = self.sizes.iter().zip(&widths).map(|((w, h), dw)| dw * h / w.max(1.0)).collect();
        let tops: Vec<f32> = heights.iter().scan(0.0, |acc, h| {
            let t = *acc;
            *acc += h + gap;
            Some(t)
        }).collect();
        let total = tops.last().copied().unwrap_or(0.0) + heights.last().copied().unwrap_or(0.0);
        let mut sa = egui::ScrollArea::both().id_salt(("pdf", self.file_name.clone())).auto_shrink([false, false]);
        if let Some(j) = self.jump.take() {
            sa = sa.vertical_scroll_offset(tops.get(j).copied().unwrap_or(0.0));
        }
        let mut wanted: Vec<(usize, f32)> = Vec::new();
        sa.show_viewport(ui, |ui, vp| {
            let content_w = widths.iter().cloned().fold(avail_w, f32::max);
            ui.set_min_size(egui::vec2(content_w, total));
            let origin = ui.min_rect().min;
            ui.painter().rect_filled(ui.clip_rect(), Rounding::ZERO, c.surface);
            let mut current = 0;
            for i in 0..self.sizes.len() {
                let top = tops[i];
                let r = egui::Rect::from_min_size(origin + egui::vec2(((content_w - widths[i]) / 2.0).max(0.0), top), egui::vec2(widths[i], heights[i]));
                if top <= vp.min.y + vp.height() * 0.4 {
                    current = i;
                }
                let visible = top + heights[i] >= vp.min.y - heights[i] && top <= vp.max.y + heights[i];
                if !visible {
                    continue;
                }
                ui.painter().rect_filled(r.translate(egui::vec2(0.0, 3.0)), Rounding::same(2.0), Color32::from_black_alpha(40));
                ui.painter().rect_filled(r, Rounding::same(2.0), Color32::WHITE);
                let scale = widths[i] * ppp / self.sizes[i].0.max(1.0);
                match self.tex.get(&i) {
                    Some((s, t)) => {
                        ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                        if (s / scale - 1.0).abs() > 0.2 {
                            wanted.push((i, scale));
                        }
                    }
                    None => {
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(18.0), Color32::from_gray(180));
                        wanted.push((i, scale));
                    }
                }
            }
            self.page_now = current;
        });
        // render what is visible; forget what is far away (memory)
        let keep: Vec<usize> = wanted.iter().map(|w| w.0).collect();
        for (i, scale) in wanted {
            let key = (i, (scale * 10.0) as u32);
            if !self.pending.contains(&key) {
                self.pending.insert(key);
                let _ = self.req.send((i, scale.min(4.0)));
            }
        }
        let now = self.page_now;
        self.tex.retain(|i, _| (*i as i64 - now as i64).abs() <= 4 || keep.contains(i));
        if !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        act
    }
}

pub struct Labels {
    pub close: &'static str,
    pub save: &'static str,
    pub open: &'static str,
    pub page: &'static str,
    pub cannot: &'static str,
}

pub enum Action {
    None,
    Close,
    Save,
    OpenExternal,
}

fn render_thread(data: Arc<Vec<u8>>, req: Receiver<(usize, f32)>, res: Sender<Msg>) {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
    let pdf = match Pdf::new(data) {
        Ok(p) => p,
        Err(e) => {
            let _ = res.send(Msg::Error(format!("{e:?}")));
            return;
        }
    };
    let pages = pdf.pages();
    let _ = res.send(Msg::Meta(pages.iter().map(|p| p.render_dimensions()).collect()));
    let cache = RenderCache::new();
    let is = InterpreterSettings::default();
    let rs = RenderSettings::default();
    while let Ok(first) = req.recv() {
        // take everything queued; render the newest request per page
        let mut jobs: Vec<(usize, f32)> = vec![first];
        while let Ok(j) = req.try_recv() {
            jobs.retain(|x| x.0 != j.0);
            jobs.push(j);
        }
        for (i, scale) in jobs {
            let Some(page) = pages.get(i) else { continue };
            let ps = PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
            let px = render(page, &cache, &is, &rs, &ps);
            let (w, h) = (px.width() as usize, px.height() as usize);
            let rgba: Vec<u8> = px.data_as_u8_slice().to_vec();
            if rgba.len() == w * h * 4 {
                let img = ColorImage::from_rgba_premultiplied([w, h], &rgba);
                if res.send(Msg::Page(i, scale, img)).is_err() {
                    return;
                }
            }
        }
    }
}
