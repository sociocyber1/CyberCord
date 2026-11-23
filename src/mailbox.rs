//! Offline mail, kept by other users — without servers.
//!
//! When a friend is offline, a sealed (end-to-end encrypted) message is also
//! handed to a few "keepers": other users' nodes that have been online for a
//! long time, have not switched this off, and are not in a call right now.
//! A keeper cannot read the mail; it only knows who it is for. It announces
//! "I hold mail for X" in the DHT, so X finds it from anywhere when X comes
//! online, takes the mail and confirms; then the keeper deletes it.
//!
//! Limits keep this light for the keeper: text only (≤ 64 KB per letter),
//! ≤ 200 letters per recipient, ≤ 2000 in total, ≤ 120 per depositor per hour,
//! 14 days at most. A keeper in a call accepts nothing.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::e2e::Sealed;

pub const MAX_ITEM: usize = 64 * 1024;
pub const MAX_PER_RECIPIENT: usize = 200;
pub const MAX_TOTAL: usize = 2000;
pub const MAX_PER_HOUR: usize = 120;
pub const TTL_MS: u64 = 14 * 24 * 3600 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub sealed: Sealed,
    pub ts_ms: u64,
    /// Who handed it over (for the per-hour limit).
    pub by: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Mailbox {
    pub items: HashMap<String, Vec<Item>>,
    #[serde(skip)]
    recent: Vec<(String, u64)>,
}

/// DHT key: "I hold mail for this user".
pub fn key(recipient: &str) -> Vec<u8> {
    blake3::hash(format!("cybercord/mailbox/v1/{recipient}").as_bytes()).as_bytes().to_vec()
}

fn path() -> std::path::PathBuf {
    crate::store::data_dir().join("mailbox.json")
}

impl Mailbox {
    pub fn load() -> Self {
        let mut m: Mailbox = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        m.prune(crate::types::now_ms());
        m
    }

    pub fn save(&self) {
        if let Ok(t) = serde_json::to_string(self) {
            let _ = std::fs::write(path(), t);
        }
    }

    pub fn total(&self) -> usize {
        self.items.values().map(|v| v.len()).sum()
    }

    /// Keep a letter. Returns false if it is refused (limits, duplicate, too big).
    pub fn put(&mut self, s: Sealed, by: &str, now: u64) -> bool {
        let size = s.ct.len() + s.kem.len() + s.epk.len() + 200;
        if size > MAX_ITEM || s.to.is_empty() {
            return false;
        }
        self.recent.retain(|(_, t)| now.saturating_sub(*t) < 3_600_000);
        if self.recent.iter().filter(|(b, _)| b == by).count() >= MAX_PER_HOUR || self.total() >= MAX_TOTAL {
            return false;
        }
        let list = self.items.entry(s.to.clone()).or_default();
        if list.len() >= MAX_PER_RECIPIENT || list.iter().any(|i| i.sealed.id == s.id) {
            return false;
        }
        self.recent.push((by.to_string(), now));
        list.push(Item { sealed: s, ts_ms: now, by: by.to_string() });
        true
    }

    pub fn for_recipient(&self, to: &str) -> Vec<Sealed> {
        self.items.get(to).map(|v| v.iter().map(|i| i.sealed.clone()).collect()).unwrap_or_default()
    }

    /// The recipient confirmed: forget these. Returns true if nothing is left for them.
    pub fn ack(&mut self, to: &str, ids: &[String]) -> bool {
        if let Some(v) = self.items.get_mut(to) {
            v.retain(|i| !ids.contains(&i.sealed.id));
            if v.is_empty() {
                self.items.remove(to);
                return true;
            }
            return false;
        }
        true
    }

    pub fn prune(&mut self, now: u64) {
        for v in self.items.values_mut() {
            v.retain(|i| now.saturating_sub(i.ts_ms) < TTL_MS);
        }
        self.items.retain(|_, v| !v.is_empty());
    }

    pub fn recipients(&self) -> Vec<String> {
        self.items.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter(to: &str, id: &str) -> Sealed {
        Sealed { from: "a".into(), to: to.into(), id: id.into(), epk: "e".into(), kem: "k".into(), nonce: "n".into(), ct: "c".into() }
    }

    #[test]
    fn keeps_and_forgets() {
        let mut m = Mailbox::default();
        assert!(m.put(letter("bob", "1"), "a", 0));
        assert!(!m.put(letter("bob", "1"), "a", 1), "duplicate");
        assert!(m.put(letter("bob", "2"), "a", 2));
        assert_eq!(m.for_recipient("bob").len(), 2);
        assert!(!m.ack("bob", &["1".into()]));
        assert!(m.ack("bob", &["2".into()]));
        assert_eq!(m.total(), 0);
        // per-hour limit per depositor
        for i in 0..MAX_PER_HOUR {
            assert!(m.put(letter("carol", &format!("x{i}")), "spammer", 10));
        }
        assert!(!m.put(letter("dave", "y"), "spammer", 11));
        assert!(m.put(letter("dave", "y"), "someone", 11));
        // too big
        let mut big = letter("bob", "big");
        big.ct = "x".repeat(MAX_ITEM);
        assert!(!m.put(big, "a", 12));
        // expiry
        m.prune(TTL_MS + 20);
        assert_eq!(m.total(), 0);
    }
}
