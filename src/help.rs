//! Asking for help — honestly, without shame, without turning chats into a
//! money farm.
//!
//! A request travels as readable text inside a message:
//!
//! ```text
//! [help: food]
//! Lost my job, two kids, need groceries until the first salary.
//! wants: work, teach
//! amount: 3000 ₽
//! details: card 2200 …
//! [/help]
//! ```
//!
//! Limits (every node counts them itself, from the messages it has):
//! * at most 1 request per person per day, across all chats;
//! * at most 5 requests per person per chat in 90 days.
//!   A request over the limit is folded as spam and lowers that person's
//!   standing on the counting node — that is what keeps it from becoming a farm.
//! * a request must say what the help is for (at least 30 characters).

use crate::types::now_ms;

pub const PER_CHAT: usize = 5;
pub const CHAT_WINDOW_MS: u64 = 90 * 24 * 3600 * 1000;
pub const DAY_MS: u64 = 24 * 3600 * 1000;
pub const MIN_TEXT: usize = 30;

/// What the help is for.
pub const KINDS: [&str; 8] = ["food", "housing", "health", "family", "debt", "study", "work", "other"];
/// Ways to help besides money (the card shows buttons for these).
pub const WANTS: [&str; 6] = ["work", "teach", "course", "food", "things", "talk"];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HelpRequest {
    pub kind: String,
    pub text: String,
    pub wants: Vec<String>,
    pub amount: String,
    pub details: String,
}

impl HelpRequest {
    pub fn parse(body: &str) -> Option<HelpRequest> {
        let mut lines = body.trim().lines();
        let kind = lines.next()?.trim().strip_prefix("[help:")?.strip_suffix(']')?.trim().to_string();
        let mut r = HelpRequest { kind: if KINDS.contains(&kind.as_str()) { kind } else { "other".into() }, ..Default::default() };
        let mut text = Vec::new();
        let mut closed = false;
        for l in lines {
            let t = l.trim();
            if t == "[/help]" {
                closed = true;
                break;
            }
            if let Some(v) = t.strip_prefix("wants:") {
                r.wants = v.split(',').map(|w| w.trim().to_string()).filter(|w| WANTS.contains(&w.as_str())).collect();
            } else if let Some(v) = t.strip_prefix("amount:") {
                r.amount = v.trim().to_string();
            } else if let Some(v) = t.strip_prefix("details:") {
                r.details = v.trim().to_string();
            } else {
                text.push(l.to_string());
            }
        }
        r.text = text.join("\n").trim().to_string();
        closed.then_some(r)
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("[help: {}]\n{}\n", self.kind, self.text.trim());
        if !self.wants.is_empty() {
            s.push_str(&format!("wants: {}\n", self.wants.join(", ")));
        }
        if !self.amount.trim().is_empty() {
            s.push_str(&format!("amount: {}\n", self.amount.trim()));
        }
        if !self.details.trim().is_empty() {
            s.push_str(&format!("details: {}\n", self.details.trim().replace('\n', " ")));
        }
        s.push_str("[/help]");
        s
    }

    /// Says what it is for.
    pub fn complete(&self) -> bool {
        self.text.trim().chars().count() >= MIN_TEXT
    }
}

/// One request seen somewhere: (author, chat, message id, time).
#[derive(Debug, Clone)]
pub struct Seen {
    pub author: String,
    pub chat: String,
    pub id: String,
    pub ts_ms: u64,
}

/// Which requests break the limits (folded as spam), oldest first wins.
pub fn over_limit(all: &[Seen]) -> std::collections::HashSet<String> {
    let mut v: Vec<&Seen> = all.iter().collect();
    v.sort_by_key(|s| s.ts_ms);
    let mut out = std::collections::HashSet::new();
    let mut kept: Vec<&Seen> = Vec::new();
    for s in v {
        let today = kept.iter().any(|k| k.author == s.author && s.ts_ms.saturating_sub(k.ts_ms) < DAY_MS);
        let in_chat = kept.iter().filter(|k| k.author == s.author && k.chat == s.chat && s.ts_ms.saturating_sub(k.ts_ms) < CHAT_WINDOW_MS).count();
        if today || in_chat >= PER_CHAT {
            out.insert(s.id.clone());
        } else {
            kept.push(s);
        }
    }
    out
}

/// How many more requests this person may post in this chat now, and when
/// the next one is allowed (ms from now; 0 = now).
pub fn allowance(all: &[Seen], author: &str, chat: &str) -> (usize, u64) {
    let now = now_ms();
    let over = over_limit(all);
    let mine: Vec<&Seen> = all.iter().filter(|s| s.author == author && !over.contains(&s.id)).collect();
    let in_chat = mine.iter().filter(|s| s.chat == chat && now.saturating_sub(s.ts_ms) < CHAT_WINDOW_MS).count();
    let wait = mine.iter().map(|s| (s.ts_ms + DAY_MS).saturating_sub(now)).max().unwrap_or(0);
    (PER_CHAT.saturating_sub(in_chat), wait)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_limits() {
        let r = HelpRequest { kind: "food".into(), text: "Потерял работу, двое детей, нужны продукты до первой зарплаты.".into(), wants: vec!["work".into(), "teach".into()], amount: "3000 ₽".into(), details: "карта 2200 …".into() };
        assert!(r.complete());
        assert_eq!(HelpRequest::parse(&r.to_text()), Some(r));
        assert!(HelpRequest::parse("[help: food]\nне закрыто").is_none());

        let day = DAY_MS;
        let mk = |a: &str, c: &str, id: &str, t: u64| Seen { author: a.into(), chat: c.into(), id: id.into(), ts_ms: t };
        // two on the same day: the second is spam
        let over = over_limit(&[mk("a", "x", "1", 0), mk("a", "y", "2", day / 2)]);
        assert!(over.contains("2") && !over.contains("1"));
        // daily for 7 days in one chat: the 6th and 7th are over the chat limit
        let seq: Vec<Seen> = (0..7).map(|i| mk("a", "x", &format!("d{i}"), i * (day + 1))).collect();
        let over = over_limit(&seq);
        assert_eq!(over.len(), 2);
        assert!(over.contains("d5") && over.contains("d6"));
        // other people are not affected
        assert!(over_limit(&[mk("a", "x", "1", 0), mk("b", "x", "2", 1)]).is_empty());
    }
}
