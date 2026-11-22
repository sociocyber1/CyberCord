//! Scientific article search: OpenAlex (250+ million works, open-access PDF
//! links) and arXiv (preprints). Both are free, need no key and no account.
//! Requests go straight from this computer to those sites; nothing passes
//! through CyberCord nodes.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paper {
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub venue: String,
    pub doi: Option<String>,
    /// Landing page (DOI link, arXiv abstract page…).
    pub page: String,
    /// Direct open-access PDF, if known.
    pub pdf: Option<String>,
    pub summary: String,
    pub cited: Option<u64>,
    pub source: &'static str,
}

impl Paper {
    /// "A, B, C et al."
    pub fn authors_short(&self) -> String {
        let n = self.authors.len();
        let head = self.authors.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        if n > 3 { format!("{head} et al.") } else { head }
    }

    /// One line to paste into a chat.
    pub fn citation(&self) -> String {
        let mut s = format!("«{}»", self.title);
        let a = self.authors_short();
        if !a.is_empty() {
            s.push_str(&format!(" — {a}"));
        }
        if let Some(y) = self.year {
            s.push_str(&format!(" ({y})"));
        }
        if !self.venue.is_empty() {
            s.push_str(&format!(", {}", self.venue));
        }
        let link = self.doi.as_ref().map(|d| format!("https://doi.org/{d}")).unwrap_or_else(|| self.page.clone());
        if !link.is_empty() {
            s.push_str(&format!(". {link}"));
        }
        if let Some(p) = &self.pdf {
            if !link.contains(p.as_str()) {
                s.push_str(&format!(" · PDF: {p}"));
            }
        }
        s
    }

    /// A file name for the downloaded PDF.
    pub fn file_name(&self) -> String {
        let base: String = self.title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { ' ' }).collect();
        let base = base.split_whitespace().take(10).collect::<Vec<_>>().join(" ");
        let year = self.year.map(|y| format!(" ({y})")).unwrap_or_default();
        format!("{}{}.pdf", if base.is_empty() { "paper".to_string() } else { base }, year)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    OpenAlex,
    Arxiv,
}

pub enum Msg {
    Results(u64, Result<Vec<Paper>, String>),
    Pdf(String, Result<Vec<u8>, String>),
}

pub struct Search {
    pub tx: Sender<Msg>,
    pub rx: Receiver<Msg>,
    pub gen: u64,
}

impl Default for Search {
    fn default() -> Self {
        let (tx, rx) = channel();
        Search { tx, rx, gen: 0 }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent(&format!("CyberCord/{} (SocioCyberneering Inc.; article search)", env!("CARGO_PKG_VERSION")))
        .build()
}

impl Search {
    pub fn run(&mut self, query: String, source: Source, open_only: bool, ctx: egui::Context) {
        self.gen += 1;
        let gen = self.gen;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = match source {
                Source::OpenAlex => openalex(&query, open_only),
                Source::Arxiv => arxiv(&query),
            };
            let _ = tx.send(Msg::Results(gen, r));
            ctx.request_repaint();
        });
    }

    pub fn fetch_pdf(&self, url: String, ctx: egui::Context) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = download_pdf(&url);
            let _ = tx.send(Msg::Pdf(url, r));
            ctx.request_repaint();
        });
    }
}

fn enc(q: &str) -> String {
    let mut s = String::new();
    for b in q.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => s.push(b as char),
            b' ' => s.push_str("%20"),
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    s
}

pub fn openalex(query: &str, open_only: bool) -> Result<Vec<Paper>, String> {
    // test switch: a saved OpenAlex response instead of the network
    if let Ok(f) = std::env::var("CYBERCORD_PAPERS_SAMPLE") {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        return Ok(parse_openalex(&v));
    }
    let mut url = format!(
        "https://api.openalex.org/works?search={}&per-page=25&select=id,doi,display_name,publication_year,authorships,primary_location,best_oa_location,cited_by_count,abstract_inverted_index",
        enc(query)
    );
    if open_only {
        url.push_str("&filter=is_oa:true");
    }
    let v: serde_json::Value = agent().get(&url).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    Ok(parse_openalex(&v))
}

pub fn parse_openalex(v: &serde_json::Value) -> Vec<Paper> {
    let mut out = Vec::new();
    for w in v["results"].as_array().into_iter().flatten() {
        let title = w["display_name"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            continue;
        }
        let authors = w["authorships"].as_array().into_iter().flatten().filter_map(|a| a["author"]["display_name"].as_str().map(str::to_string)).collect();
        let doi = w["doi"].as_str().map(|d| d.trim_start_matches("https://doi.org/").to_string());
        let page = w["primary_location"]["landing_page_url"].as_str().map(str::to_string).or_else(|| doi.as_ref().map(|d| format!("https://doi.org/{d}"))).unwrap_or_else(|| w["id"].as_str().unwrap_or("").to_string());
        let pdf = w["best_oa_location"]["pdf_url"].as_str().or_else(|| w["primary_location"]["pdf_url"].as_str()).map(str::to_string);
        out.push(Paper {
            title,
            authors,
            year: w["publication_year"].as_i64().map(|y| y as i32),
            venue: w["primary_location"]["source"]["display_name"].as_str().unwrap_or("").to_string(),
            doi,
            page,
            pdf,
            summary: abstract_from_index(&w["abstract_inverted_index"]),
            cited: w["cited_by_count"].as_u64(),
            source: "OpenAlex",
        });
    }
    out
}

/// OpenAlex keeps abstracts as {word: [positions]}; put the words back in order.
pub fn abstract_from_index(v: &serde_json::Value) -> String {
    let Some(m) = v.as_object() else { return String::new() };
    let mut words: Vec<(u64, &str)> = Vec::new();
    for (w, pos) in m {
        for p in pos.as_array().into_iter().flatten() {
            if let Some(p) = p.as_u64() {
                words.push((p, w.as_str()));
            }
        }
    }
    words.sort_by_key(|(p, _)| *p);
    words.into_iter().map(|(_, w)| w).collect::<Vec<_>>().join(" ")
}

pub fn arxiv(query: &str) -> Result<Vec<Paper>, String> {
    let terms: Vec<String> = query.split_whitespace().map(|t| format!("all:{}", enc(t))).collect();
    let url = format!("https://export.arxiv.org/api/query?search_query={}&max_results=25&sortBy=relevance", terms.join("+AND+"));
    let xml = agent().get(&url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    Ok(parse_arxiv(&xml))
}

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}");
    let i = s.find(&open)?;
    let rest = &s[i + open.len()..];
    let gt = rest.find('>')?;
    if rest[..gt].ends_with('/') {
        return Some("");
    }
    let body = &rest[gt + 1..];
    let end = body.find(&format!("</{name}>"))?;
    Some(&body[..end])
}

fn unxml(s: &str) -> String {
    let s = s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&amp;", "&");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn attr<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let k = format!("{name}=\"");
    let i = s.find(&k)?;
    let rest = &s[i + k.len()..];
    Some(&rest[..rest.find('"')?])
}

pub fn parse_arxiv(xml: &str) -> Vec<Paper> {
    let mut out = Vec::new();
    for e in xml.split("<entry>").skip(1) {
        let e = e.split("</entry>").next().unwrap_or(e);
        let title = tag(e, "title").map(unxml).unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let page = tag(e, "id").map(|s| s.trim().replace("http://", "https://")).unwrap_or_default();
        let authors = e.split("<author>").skip(1).filter_map(|a| tag(a, "name").map(unxml)).collect();
        let year = tag(e, "published").and_then(|p| p.trim().get(..4)).and_then(|y| y.parse().ok());
        let mut pdf = None;
        for l in e.split("<link").skip(1) {
            let l = &l[..l.find('>').unwrap_or(l.len())];
            if attr(l, "title") == Some("pdf") || attr(l, "type") == Some("application/pdf") {
                pdf = attr(l, "href").map(|h| h.replace("http://", "https://"));
            }
        }
        let doi = tag(e, "arxiv:doi").map(|d| unxml(d));
        out.push(Paper {
            title,
            authors,
            year,
            venue: "arXiv".into(),
            doi,
            page,
            pdf,
            summary: tag(e, "summary").map(unxml).unwrap_or_default(),
            cited: None,
            source: "arXiv",
        });
    }
    out
}

const MAX_PDF: u64 = 80 * 1024 * 1024;

pub fn download_pdf(url: &str) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(resp.into_reader(), MAX_PDF), &mut buf).map_err(|e| e.to_string())?;
    if !buf.starts_with(b"%PDF") {
        return Err("not a PDF (the site returned a web page)".into());
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openalex_parse() {
        let v: serde_json::Value = serde_json::from_str(r#"{"results":[{"id":"https://openalex.org/W1","doi":"https://doi.org/10.1017/9781108344227","display_name":"Astrophysics of Planet Formation","publication_year":2009,
            "authorships":[{"author":{"display_name":"Philip J. Armitage"}}],
            "primary_location":{"landing_page_url":"https://doi.org/10.1017/9781108344227","source":{"display_name":"Cambridge University Press eBooks"}},
            "best_oa_location":{"pdf_url":"https://example.org/a.pdf"},"cited_by_count":646,
            "abstract_inverted_index":{"Concise":[0],"and":[1],"self-contained,":[2],"this":[3],"textbook":[4]}}]}"#).unwrap();
        let p = &parse_openalex(&v)[0];
        assert_eq!(p.title, "Astrophysics of Planet Formation");
        assert_eq!(p.doi.as_deref(), Some("10.1017/9781108344227"));
        assert_eq!(p.summary, "Concise and self-contained, this textbook");
        assert_eq!(p.pdf.as_deref(), Some("https://example.org/a.pdf"));
        assert_eq!(p.cited, Some(646));
        assert!(p.citation().contains("Armitage (2009)"));
        assert_eq!(p.file_name(), "Astrophysics of Planet Formation (2009).pdf");
    }

    #[test]
    fn arxiv_parse() {
        let xml = r#"<feed><entry>
<id>http://arxiv.org/abs/2101.00001v1</id><published>2021-01-02T00:00:00Z</published>
<title>Helium &amp; hydrogen
  in giant planets</title><summary>  We study H/He envelopes.  </summary>
<author><name>A. Author</name></author><author><name>B. Author</name></author>
<link href="http://arxiv.org/abs/2101.00001v1" rel="alternate" type="text/html"/>
<link title="pdf" href="http://arxiv.org/pdf/2101.00001v1" rel="related" type="application/pdf"/>
</entry></feed>"#;
        let p = &parse_arxiv(xml)[0];
        assert_eq!(p.title, "Helium & hydrogen in giant planets");
        assert_eq!(p.authors, vec!["A. Author", "B. Author"]);
        assert_eq!(p.year, Some(2021));
        assert_eq!(p.pdf.as_deref(), Some("https://arxiv.org/pdf/2101.00001v1"));
        assert_eq!(p.summary, "We study H/He envelopes.");
    }
}
