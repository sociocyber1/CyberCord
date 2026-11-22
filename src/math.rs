//! A small LaTeX math renderer for chat messages: `$…$` inline and `$$…$$`
//! as a block. Supports the everyday subset — fractions, roots, powers and
//! indices, Greek letters, operators and relations, sums/integrals with
//! limits, \left…\right, \text{} and common functions — laid out with egui.

use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Pos2, Rect, Response, Stroke, TextFormat, Vec2};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Row(Vec<Node>),
    /// a symbol; `italic` for single Latin letters (variables)
    Sym(String, bool),
    /// upright text: \text{}, function names
    Text(String),
    Frac(Box<Node>, Box<Node>),
    Sqrt(Box<Node>, Option<Box<Node>>),
    Scripts(Box<Node>, Option<Box<Node>>, Option<Box<Node>>),
    /// ∑ ∫ ∏ with optional lower/upper limits
    BigOp(String, Option<Box<Node>>, Option<Box<Node>>),
    Delim(String, Box<Node>, String),
    /// accent over a node: vec → arrow, hat, bar, dot
    Accent(char, Box<Node>),
}

fn symbol(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ", "epsilon" => "ε", "varepsilon" => "ε", "zeta" => "ζ", "eta" => "η", "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ", "lambda" => "λ", "mu" => "μ", "nu" => "ν", "xi" => "ξ", "pi" => "π", "rho" => "ρ", "sigma" => "σ", "tau" => "τ", "upsilon" => "υ", "phi" => "φ", "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ", "Xi" => "Ξ", "Pi" => "Π", "Sigma" => "Σ", "Upsilon" => "Υ", "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω",
        "cdot" => "·", "times" => "×", "div" => "÷", "pm" => "±", "mp" => "∓", "ast" => "∗", "circ" => "∘", "bullet" => "•",
        "le" | "leq" => "≤", "ge" | "geq" => "≥", "ne" | "neq" => "≠", "approx" => "≈", "equiv" => "≡", "sim" => "∼", "simeq" => "≃", "propto" => "∝", "ll" => "≪", "gg" => "≫",
        "infty" => "∞", "partial" => "∂", "nabla" => "∇", "hbar" => "ℏ", "ell" => "ℓ", "degree" => "°", "prime" => "′", "angle" => "∠", "perp" => "⊥", "parallel" => "∥", "emptyset" | "varnothing" => "∅",
        "to" | "rightarrow" => "→", "leftarrow" | "gets" => "←", "leftrightarrow" => "↔", "Rightarrow" | "implies" => "⇒", "Leftarrow" => "⇐", "Leftrightarrow" | "iff" => "⇔", "mapsto" => "↦", "uparrow" => "↑", "downarrow" => "↓",
        "in" => "∈", "notin" => "∉", "ni" => "∋", "subset" => "⊂", "subseteq" => "⊆", "supset" => "⊃", "supseteq" => "⊇", "cup" => "∪", "cap" => "∩", "setminus" => "∖",
        "forall" => "∀", "exists" => "∃", "nexists" => "∄", "neg" | "lnot" => "¬", "land" | "wedge" => "∧", "lor" | "vee" => "∨", "oplus" => "⊕", "otimes" => "⊗",
        "ldots" | "dots" => "…", "cdots" => "⋯", "vdots" => "⋮", "ddots" => "⋱",
        "langle" => "⟨", "rangle" => "⟩", "lfloor" => "⌊", "rfloor" => "⌋", "lceil" => "⌈", "rceil" => "⌉", "lbrace" | "{" => "{", "rbrace" | "}" => "}", "|" => "‖", "%" => "%", "$" => "$", "&" => "&", "#" => "#", "_" => "_",
        "quad" => "\u{2003}", "qquad" => "\u{2003}\u{2003}", "," | ";" | ":" | "!" | " " => "\u{2009}",
        _ => return None,
    })
}

fn big_op(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "sum" => "∑",
        "prod" => "∏",
        "coprod" => "∐",
        "int" => "∫",
        "iint" => "∬",
        "iiint" => "∭",
        "oint" => "∮",
        "bigcup" => "⋃",
        "bigcap" => "⋂",
        _ => return None,
    })
}

const FUNCS: &[&str] = &["sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "log", "ln", "lg", "exp", "lim", "max", "min", "sup", "inf", "det", "dim", "ker", "deg", "gcd", "arg", "mod"];

fn blackboard(c: char) -> char {
    match c {
        'R' => 'ℝ',
        'N' => 'ℕ',
        'Z' => 'ℤ',
        'Q' => 'ℚ',
        'C' => 'ℂ',
        'P' => 'ℙ',
        'H' => 'ℍ',
        _ => c,
    }
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while self.peek().map_or(false, |c| c.is_whitespace()) {
            self.i += 1;
        }
    }
    fn command(&mut self) -> String {
        // after '\'
        let mut name = String::new();
        if let Some(c) = self.peek() {
            if !c.is_alphabetic() {
                self.i += 1;
                return c.to_string();
            }
        }
        while let Some(c) = self.peek() {
            if c.is_alphabetic() {
                name.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        name
    }
    fn raw_group(&mut self) -> String {
        self.skip_ws();
        if self.peek() != Some('{') {
            return self.peek().map(|c| {
                self.i += 1;
                c.to_string()
            }).unwrap_or_default();
        }
        self.i += 1;
        let mut depth = 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            out.push(c);
        }
        out
    }
    /// one argument: {group} or a single token
    fn arg(&mut self) -> Node {
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.i += 1;
                let n = self.row(Some('}'));
                self.i += 1;
                n
            }
            Some(_) => self.atom().unwrap_or(Node::Row(vec![])),
            None => Node::Row(vec![]),
        }
    }
    fn delim_token(&mut self) -> String {
        self.skip_ws();
        match self.peek() {
            Some('\\') => {
                self.i += 1;
                let c = self.command();
                symbol(&c).unwrap_or("").to_string()
            }
            Some('.') => {
                self.i += 1;
                String::new()
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }
    fn atom(&mut self) -> Option<Node> {
        self.skip_ws();
        let c = self.peek()?;
        match c {
            '\\' => {
                self.i += 1;
                let cmd = self.command();
                Some(match cmd.as_str() {
                    "frac" | "dfrac" | "tfrac" => {
                        let a = self.arg();
                        let b = self.arg();
                        Node::Frac(Box::new(a), Box::new(b))
                    }
                    "sqrt" => {
                        self.skip_ws();
                        let idx = if self.peek() == Some('[') {
                            self.i += 1;
                            let n = self.row(Some(']'));
                            self.i += 1;
                            Some(Box::new(n))
                        } else {
                            None
                        };
                        Node::Sqrt(Box::new(self.arg()), idx)
                    }
                    "text" | "mathrm" | "textrm" | "operatorname" => Node::Text(self.raw_group()),
                    "mathbb" => Node::Sym(self.raw_group().chars().map(blackboard).collect(), false),
                    "mathbf" | "boldsymbol" | "mathit" | "mathcal" => self.arg(),
                    "vec" => Node::Accent('→', Box::new(self.arg())),
                    "hat" | "widehat" => Node::Accent('^', Box::new(self.arg())),
                    "bar" | "overline" => Node::Accent('‾', Box::new(self.arg())),
                    "dot" => Node::Accent('·', Box::new(self.arg())),
                    "left" => {
                        let l = self.delim_token();
                        let inner = self.row_until_right();
                        let r = self.delim_token();
                        Node::Delim(l, Box::new(inner), r)
                    }
                    "right" => Node::Row(vec![]),
                    c if FUNCS.contains(&c) => Node::Text(c.to_string()),
                    c => {
                        if let Some(op) = big_op(c) {
                            Node::BigOp(op.to_string(), None, None)
                        } else if let Some(s) = symbol(c) {
                            Node::Sym(s.to_string(), false)
                        } else {
                            Node::Text(format!("\\{c}"))
                        }
                    }
                })
            }
            '{' => {
                self.i += 1;
                let n = self.row(Some('}'));
                self.i += 1;
                Some(n)
            }
            _ => {
                self.i += 1;
                let italic = c.is_ascii_alphabetic();
                let s = match c {
                    '-' => "−".to_string(),
                    '*' => "∗".to_string(),
                    '\'' => "′".to_string(),
                    _ => c.to_string(),
                };
                Some(Node::Sym(s, italic))
            }
        }
    }
    fn row_until_right(&mut self) -> Node {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                break;
            }
            if self.s[self.i..].starts_with(&['\\', 'r', 'i', 'g', 'h', 't']) {
                self.i += 6;
                break;
            }
            if let Some(n) = self.scripted() {
                items.push(n);
            }
        }
        Node::Row(items)
    }
    fn scripted(&mut self) -> Option<Node> {
        let base = self.atom()?;
        let mut sub = None;
        let mut sup = None;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('_') if sub.is_none() => {
                    self.i += 1;
                    sub = Some(Box::new(self.arg()));
                }
                Some('^') if sup.is_none() => {
                    self.i += 1;
                    sup = Some(Box::new(self.arg()));
                }
                _ => break,
            }
        }
        if sub.is_none() && sup.is_none() {
            return Some(base);
        }
        Some(match base {
            Node::BigOp(op, _, _) => Node::BigOp(op, sub, sup),
            b => Node::Scripts(Box::new(b), sub, sup),
        })
    }
    fn row(&mut self, end: Option<char>) -> Node {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => break,
                Some(c) if Some(c) == end => break,
                Some('}') if end.is_none() => {
                    self.i += 1;
                }
                _ => {
                    if let Some(n) = self.scripted() {
                        items.push(n);
                    }
                }
            }
        }
        if items.len() == 1 {
            items.pop().unwrap()
        } else {
            Node::Row(items)
        }
    }
}

pub fn parse(src: &str) -> Node {
    let mut p = Parser { s: src.chars().collect(), i: 0 };
    p.row(None)
}

// ---- layout ----

/// A laid-out box: width, height above and depth below the baseline.
#[derive(Clone, Copy, Debug)]
struct Metrics {
    w: f32,
    asc: f32,
    desc: f32,
}

struct Ctx<'a> {
    fonts: &'a egui::text::Fonts,
    color: Color32,
}

enum Draw {
    Text(Pos2, std::sync::Arc<egui::Galley>),
    Line(Pos2, Pos2, f32),
}

fn galley(cx: &Ctx, s: &str, size: f32, italic: bool) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.append(s, 0.0, TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color: cx.color, italics: italic, ..Default::default() });
    cx.fonts.layout_job(job)
}

/// Lay out `n` at font size `size`; push drawing ops with the baseline at y=0, x from 0.
fn layout(cx: &Ctx, n: &Node, size: f32, ops: &mut Vec<Draw>, origin: Vec2) -> Metrics {
    let asc_of = |g: &egui::Galley| g.rows.first().map(|r| r.rect.height() * 0.78).unwrap_or(size * 0.78);
    match n {
        Node::Row(items) => {
            let mut x = 0.0;
            let mut asc: f32 = size * 0.7;
            let mut desc: f32 = size * 0.2;
            let is_op = |n: &Node| matches!(n, Node::Sym(s, _) if !s.is_empty() && "=<>≤≥≠≈≡→←⇒⇔∈∉⊂⊆∪∩±∓×÷·+−∼∝↦".contains(s.as_str()));
            for (k, it) in items.iter().enumerate() {
                // binary operators get space around them; a leading sign does not
                let unary = k == 0 || is_op(&items[k - 1]);
                let pad = if is_op(it) && !unary { size * 0.22 } else { 0.0 };
                x += pad;
                let m = layout(cx, it, size, ops, origin + Vec2::new(x, 0.0));
                x += m.w + pad;
                asc = asc.max(m.asc);
                desc = desc.max(m.desc);
            }
            Metrics { w: x, asc, desc }
        }
        Node::Sym(s, italic) => {
            let g = galley(cx, s, size, *italic);
            let a = asc_of(&g);
            let w = g.size().x + if *italic { size * 0.05 } else { 0.0 };
            let h = g.size().y;
            ops.push(Draw::Text(Pos2::new(origin.x, origin.y - a), g));
            Metrics { w, asc: a, desc: h - a }
        }
        Node::Text(s) => {
            let g = galley(cx, s, size, false);
            let a = asc_of(&g);
            let (w, h) = (g.size().x + size * 0.12, g.size().y);
            ops.push(Draw::Text(Pos2::new(origin.x + size * 0.06, origin.y - a), g));
            Metrics { w, asc: a, desc: h - a }
        }
        Node::Frac(a, b) => {
            let s2 = (size * 0.85).max(9.0);
            let mut na = Vec::new();
            let mut nb = Vec::new();
            let ma = layout(cx, a, s2, &mut na, Vec2::ZERO);
            let mb = layout(cx, b, s2, &mut nb, Vec2::ZERO);
            let w = ma.w.max(mb.w) + size * 0.3;
            let axis = size * 0.30;
            let gap = size * 0.12;
            let top_base = -axis - gap - ma.desc;
            let bot_base = -axis + gap + mb.asc;
            for op in na {
                ops.push(shift(op, origin + Vec2::new((w - ma.w) / 2.0, top_base)));
            }
            for op in nb {
                ops.push(shift(op, origin + Vec2::new((w - mb.w) / 2.0, bot_base)));
            }
            ops.push(Draw::Line(Pos2::new(origin.x + size * 0.08, origin.y - axis), Pos2::new(origin.x + w - size * 0.08, origin.y - axis), (size * 0.06).max(1.0)));
            Metrics { w, asc: axis + gap + ma.desc + ma.asc, desc: -axis + gap + mb.asc + mb.desc }
        }
        Node::Sqrt(body, idx) => {
            let mut nb = Vec::new();
            let mb = layout(cx, body, size, &mut nb, Vec2::ZERO);
            let mut lead = size * 0.55;
            let mut ni = Vec::new();
            let mut mi = None;
            if let Some(i) = idx {
                let m = layout(cx, i, size * 0.55, &mut ni, Vec2::ZERO);
                lead = lead.max(m.w + size * 0.2);
                mi = Some(m);
            }
            let top = mb.asc + size * 0.15;
            let x0 = origin.x;
            let lw = (size * 0.06).max(1.0);
            // radical sign
            ops.push(Draw::Line(Pos2::new(x0 + lead - size * 0.5, origin.y - mb.asc * 0.35), Pos2::new(x0 + lead - size * 0.32, origin.y + mb.desc * 0.6), lw));
            ops.push(Draw::Line(Pos2::new(x0 + lead - size * 0.32, origin.y + mb.desc * 0.6), Pos2::new(x0 + lead - size * 0.08, origin.y - top), lw));
            ops.push(Draw::Line(Pos2::new(x0 + lead - size * 0.08, origin.y - top), Pos2::new(x0 + lead + mb.w + size * 0.08, origin.y - top), lw));
            for op in nb {
                ops.push(shift(op, origin + Vec2::new(lead + size * 0.02, 0.0)));
            }
            if let Some(m) = mi {
                for op in ni {
                    ops.push(shift(op, origin + Vec2::new(lead - size * 0.45 - m.w, -mb.asc * 0.45)));
                }
            }
            Metrics { w: lead + mb.w + size * 0.15, asc: top + lw, desc: mb.desc }
        }
        Node::Scripts(base, sub, sup) => {
            let mb = layout(cx, base, size, ops, origin);
            let s2 = (size * 0.68).max(8.0);
            let mut w = mb.w;
            let mut asc = mb.asc;
            let mut desc = mb.desc;
            let mut sw: f32 = 0.0;
            if let Some(sp) = sup {
                let mut v = Vec::new();
                let m = layout(cx, sp, s2, &mut v, Vec2::ZERO);
                let y = -(mb.asc * 0.55).max(size * 0.38) ;
                for op in v {
                    ops.push(shift(op, origin + Vec2::new(mb.w + size * 0.03, y)));
                }
                asc = asc.max(-y + m.asc);
                sw = sw.max(m.w);
            }
            if let Some(sb) = sub {
                let mut v = Vec::new();
                let m = layout(cx, sb, s2, &mut v, Vec2::ZERO);
                let y = size * 0.22 + if sup.is_some() { size * 0.08 } else { 0.0 };
                for op in v {
                    ops.push(shift(op, origin + Vec2::new(mb.w + size * 0.02, y)));
                }
                desc = desc.max(y + m.desc);
                sw = sw.max(m.w);
            }
            w += sw + size * 0.06;
            Metrics { w, asc, desc }
        }
        Node::BigOp(op, lo, hi) => {
            let big = size * 1.45;
            let g = galley(cx, op, big, false);
            let a = asc_of(&g);
            let gw = g.size().x;
            let gh = g.size().y;
            let s2 = (size * 0.62).max(8.0);
            let mut lo_ops = Vec::new();
            let mut hi_ops = Vec::new();
            let ml = lo.as_ref().map(|n| layout(cx, n, s2, &mut lo_ops, Vec2::ZERO));
            let mh = hi.as_ref().map(|n| layout(cx, n, s2, &mut hi_ops, Vec2::ZERO));
            let w = gw.max(ml.map_or(0.0, |m| m.w)).max(mh.map_or(0.0, |m| m.w)) + size * 0.2;
            // centre the big symbol on the math axis
            let axis = size * 0.30;
            let top = -axis - gh / 2.0;
            ops.push(Draw::Text(Pos2::new(origin.x + (w - gw) / 2.0, origin.y + top), g));
            let _ = a;
            let mut asc = axis + gh / 2.0;
            let mut desc = gh / 2.0 - axis;
            if let Some(m) = mh {
                let y = top - m.desc - size * 0.05;
                for o in hi_ops {
                    ops.push(shift(o, origin + Vec2::new((w - m.w) / 2.0, y)));
                }
                asc = -y + m.asc;
            }
            if let Some(m) = ml {
                let y = top + gh + m.asc + size * 0.02;
                for o in lo_ops {
                    ops.push(shift(o, origin + Vec2::new((w - m.w) / 2.0, y)));
                }
                desc = y + m.desc;
            }
            Metrics { w, asc, desc }
        }
        Node::Delim(l, body, r) => {
            let mut v = Vec::new();
            let m = layout(cx, body, size, &mut v, Vec2::ZERO);
            let h = (m.asc + m.desc).max(size);
            let ds = (h * 1.05).max(size);
            let mut x = 0.0;
            let put = |s: &str, x: &mut f32, ops: &mut Vec<Draw>| {
                if s.is_empty() {
                    return;
                }
                let g = galley(cx, s, ds, false);
                let gy = origin.y - m.asc - (g.size().y - h) / 2.0;
                let gw = g.size().x;
                ops.push(Draw::Text(Pos2::new(origin.x + *x, gy), g));
                *x += gw;
            };
            put(l, &mut x, ops);
            for op in v {
                ops.push(shift(op, origin + Vec2::new(x, 0.0)));
            }
            x += m.w;
            put(r, &mut x, ops);
            Metrics { w: x, asc: m.asc + size * 0.05, desc: m.desc + size * 0.05 }
        }
        Node::Accent(ch, body) => {
            let m = layout(cx, body, size, ops, origin);
            let g = galley(cx, &ch.to_string(), size * 0.8, false);
            let gw = g.size().x;
            let y = origin.y - m.asc - g.size().y * 0.55;
            ops.push(Draw::Text(Pos2::new(origin.x + (m.w - gw) / 2.0, y), g));
            Metrics { w: m.w, asc: m.asc + size * 0.35, desc: m.desc }
        }
    }
}

fn shift(op: Draw, d: Vec2) -> Draw {
    match op {
        Draw::Text(p, g) => Draw::Text(p + d, g),
        Draw::Line(a, b, w) => Draw::Line(a + d, b + d, w),
    }
}

/// Draw a formula; returns its response (for hover).
pub fn show(ui: &mut egui::Ui, latex: &str, size: f32, color: Color32) -> Response {
    let node = parse(latex);
    let mut ops = Vec::new();
    let m = ui.fonts(|f| layout(&Ctx { fonts: f, color }, &node, size, &mut ops, Vec2::ZERO));
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(m.w.max(1.0), m.asc + m.desc + 2.0), egui::Sense::hover());
    let base = Pos2::new(rect.min.x, rect.min.y + m.asc + 1.0).to_vec2();
    let p = ui.painter_at(rect.expand(4.0));
    for op in ops {
        match op {
            Draw::Text(pos, g) => p.galley(pos + base, g, color),
            Draw::Line(a, b, w) => {
                p.line_segment([a + base, b + base], Stroke::new(w, color));
            }
        }
    }
    resp.on_hover_text(latex)
}

/// Pieces of a line: plain text and formulas (`$…$` / `$$…$$`).
#[derive(Debug, PartialEq)]
pub enum Piece<'a> {
    Text(&'a str),
    Inline(&'a str),
    Block(&'a str),
}

pub fn split(line: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find('$') {
        // \$ is a literal dollar
        if i > 0 && rest[..i].ends_with('\\') {
            out.push(Piece::Text(&rest[..i + 1]));
            rest = &rest[i + 1..];
            continue;
        }
        let block = rest[i..].starts_with("$$");
        let open = if block { 2 } else { 1 };
        let after = &rest[i + open..];
        let close = if block { after.find("$$") } else { after.find('$') };
        let Some(j) = close else { break };
        if i > 0 {
            out.push(Piece::Text(&rest[..i]));
        }
        let f = &after[..j];
        out.push(if block { Piece::Block(f) } else { Piece::Inline(f) });
        rest = &after[j + open..];
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    out
}

pub fn has_math(text: &str) -> bool {
    split(text).iter().any(|p| !matches!(p, Piece::Text(_)))
}

#[allow(dead_code)]
pub fn bounds(r: Rect) -> Rect {
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_formulas() {
        assert!(matches!(parse(r"\frac{a}{b}"), Node::Frac(_, _)));
        assert!(matches!(parse(r"x^2"), Node::Scripts(_, None, Some(_))));
        assert!(matches!(parse(r"\sum_{i=1}^{n} i"), Node::Row(_)));
        assert!(matches!(parse(r"\sqrt[3]{x}"), Node::Sqrt(_, Some(_))));
        assert_eq!(parse(r"\alpha"), Node::Sym("α".into(), false));
        assert!(matches!(parse(r"\left( \frac{1}{2} \right)"), Node::Delim(_, _, _)));
    }

    #[test]
    fn splits_text_and_math() {
        assert_eq!(split("E = $mc^2$, ok"), vec![Piece::Text("E = "), Piece::Inline("mc^2"), Piece::Text(", ok")]);
        assert_eq!(split("$$\\int_0^1 x dx$$"), vec![Piece::Block("\\int_0^1 x dx")]);
        assert_eq!(split("costs 5$ only"), vec![Piece::Text("costs 5$ only")]);
        assert!(!has_math("no math here"));
    }
}
