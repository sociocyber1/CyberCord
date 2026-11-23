//! Safety without a central moderator.
//!
//! * A local hint on the sender's own device when a message looks like a drug
//!   sale offer (nothing is sent anywhere; the person decides).
//! * Signed reports (drugs, harm to children, threats, scam, other). A report
//!   carries a small proof of work (about a second of CPU), so mass reporting
//!   costs real time.
//! * Every node weighs reports itself, by how much it trusts the reporter
//!   (itself, friends, established users; new and unknown identities weigh
//!   nothing), with decay over time and a penalty for people who report many
//!   of your friends. There is no global ban, so an attack with reports cannot
//!   switch the network off: at worst some messages get hidden for some users,
//!   and the network's own traffic (search, keys, hub, relays, mail) never
//!   depends on reports.

use serde::{Deserialize, Serialize};

use crate::types::{now_ms, ChannelId};

/// Leading zero bits a report's hash must have (≈ 1 s of CPU).
pub const POW_BITS: u32 = 20;
/// Reports accepted per reporter per day.
pub const PER_DAY: usize = 10;
/// Reports older than this are forgotten.
pub const KEEP_MS: u64 = 90 * 24 * 3600 * 1000;
/// Weight halves every 30 days.
pub const HALF_LIFE_MS: f64 = 30.0 * 24.0 * 3600.0 * 1000.0;
/// A reporter must have been around this long (by my own observation) to count.
pub const MIN_AGE_MS: u64 = 7 * 24 * 3600 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Drugs,
    Child,
    Threat,
    Scam,
    Other,
}

pub const CATEGORIES: [Category; 5] = [Category::Drugs, Category::Child, Category::Threat, Category::Scam, Category::Other];

impl Category {
    pub fn key(self) -> &'static str {
        match self {
            Category::Drugs => "rep_drugs",
            Category::Child => "rep_child",
            Category::Threat => "rep_threat",
            Category::Scam => "rep_scam",
            Category::Other => "rep_other",
        }
    }
}

// ---- local hint: drug sale offers ----

const SUBSTANCES: &[&str] = &[
    "мефедрон", "меф ", "амфетамин", "метамфетамин", "кокаин", "героин", "гашиш", "марихуан", "экстази", "mdma", "мдма", "лсд", "спайс", "альфа-pvp", "a-pvp", "a pvp", "кетамин", "фентанил", "метадон", "lsd",
    "cocaine", "heroin", "meth", "amphetamine", "ketamine", "fentanyl", "mephedrone", "ecstasy", "weed", "hash",
];

const SALE: &[&str] = &[
    "продам", "продаю", "продаём", "куплю", "закладк", "клад ", "кладом", "в наличии", "прайс", "оптом", "розниц", "граммов", " гр ", "доставка", "оплата", "btc", "крипт", "магазин", "шоп",
    "sell", "selling", "buy", "price", "stash", "delivery", "in stock", "dm me", "for sale",
];

/// Does this text look like an offer to sell or buy drugs? (A substance word
/// together with a market word; talking *about* drugs does not trigger it.)
pub fn looks_like_drug_sale(text: &str) -> bool {
    let t = format!(" {} ", text.to_lowercase());
    let substance = SUBSTANCES.iter().any(|w| t.contains(w));
    let market = SALE.iter().any(|w| t.contains(w));
    let stash = t.contains("закладк") && (t.contains("район") || t.contains("адрес") || t.contains("оплат") || t.contains("прайс"));
    (substance && market) || stash
}

// ---- reports ----

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub reporter: String,
    /// The user the report is about.
    pub target: String,
    /// The message, if the report is about one.
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub channel: Option<ChannelId>,
    pub category: Category,
    pub ts_ms: u64,
    pub nonce: u64,
    pub sig: String,
}

impl Report {
    fn core(&self) -> String {
        format!(
            "cybercord/report/v1\n{}\n{}\n{}\n{:?}\n{}\n{}",
            self.reporter,
            self.target,
            self.msg.as_deref().unwrap_or(""),
            self.category,
            self.ts_ms,
            self.nonce
        )
    }

    fn pow_ok(&self) -> bool {
        let h = blake3::hash(self.core().as_bytes());
        leading_zeros(h.as_bytes()) >= POW_BITS
    }

    /// Make, prove (work) and sign a report. Takes about a second.
    pub fn create(identity: &libp2p::identity::Keypair, target: &str, msg: Option<String>, channel: Option<ChannelId>, category: Category) -> Report {
        let mut r = Report {
            reporter: identity.public().to_peer_id().to_base58(),
            target: target.to_string(),
            msg,
            channel,
            category,
            ts_ms: now_ms(),
            nonce: rand::random::<u32>() as u64,
            sig: String::new(),
        };
        while !r.pow_ok() {
            r.nonce = r.nonce.wrapping_add(1);
        }
        if let Ok(s) = identity.sign(r.core().as_bytes()) {
            use base64::Engine;
            r.sig = base64::engine::general_purpose::STANDARD.encode(s);
        }
        r
    }

    /// Signed by the reporter, with enough work, not from the future, not about oneself.
    pub fn verify(&self) -> bool {
        use base64::Engine;
        if self.reporter == self.target || self.ts_ms > now_ms() + 600_000 || !self.pow_ok() {
            return false;
        }
        let Some(pk) = crate::types::public_key_of(&self.reporter) else { return false };
        let Ok(sig) = base64::engine::general_purpose::STANDARD.decode(&self.sig) else { return false };
        pk.verify(self.core().as_bytes(), &sig)
    }
}

fn leading_zeros(b: &[u8]) -> u32 {
    let mut n = 0;
    for &x in b {
        if x == 0 {
            n += 8;
        } else {
            n += x.leading_zeros();
            break;
        }
    }
    n
}

/// Reports this node keeps, with limits.
#[derive(Default, Serialize, Deserialize)]
pub struct Reports {
    pub list: Vec<Report>,
}

fn path() -> std::path::PathBuf {
    crate::store::data_dir().join("reports.json")
}

impl Reports {
    pub fn load() -> Self {
        let mut r: Reports = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let now = now_ms();
        r.list.retain(|x| now.saturating_sub(x.ts_ms) < KEEP_MS);
        r
    }

    pub fn save(&self) {
        if let Ok(t) = serde_json::to_string(self) {
            let _ = std::fs::write(path(), t);
        }
    }

    /// Accept a verified report (once; ≤ 10 per reporter per day; ≤ 5000 kept).
    pub fn add(&mut self, r: Report) -> bool {
        if !r.verify() {
            return false;
        }
        if self.list.iter().any(|x| x.reporter == r.reporter && x.target == r.target && x.msg == r.msg && x.category == r.category) {
            return false;
        }
        let day = r.ts_ms.saturating_sub(24 * 3600 * 1000);
        if self.list.iter().filter(|x| x.reporter == r.reporter && x.ts_ms > day).count() >= PER_DAY {
            return false;
        }
        if self.list.len() >= 5000 {
            self.list.remove(0);
        }
        self.list.push(r);
        true
    }
}

/// How much I trust a reporter, 0…1.
pub struct Trust<'a> {
    pub me: &'a str,
    pub is_friend: &'a dyn Fn(&str) -> bool,
    /// When I first noticed this user on the network.
    pub first_seen: &'a dyn Fn(&str) -> Option<u64>,
}

impl Trust<'_> {
    pub fn weight(&self, reporter: &str, all: &[Report]) -> f64 {
        if reporter == self.me {
            return 1.0;
        }
        // reporting many of my friends looks like an attack: ignore that reporter
        let hits: std::collections::HashSet<&str> = all.iter().filter(|r| r.reporter == reporter && (self.is_friend)(&r.target)).map(|r| r.target.as_str()).collect();
        if hits.len() >= 3 {
            return 0.0;
        }
        if (self.is_friend)(reporter) {
            return 0.7;
        }
        match (self.first_seen)(reporter) {
            Some(t) if now_ms().saturating_sub(t) >= MIN_AGE_MS => 0.25,
            _ => 0.0,
        }
    }

    fn decayed(&self, r: &Report, all: &[Report]) -> f64 {
        let age = now_ms().saturating_sub(r.ts_ms) as f64;
        self.weight(&r.reporter, all) * 0.5f64.powf(age / HALF_LIFE_MS)
    }

    /// Score of one message: summed trust of distinct reporters, and the main category.
    pub fn message_score(&self, msg: &str, all: &[Report]) -> (f64, Option<Category>) {
        let mut seen = std::collections::HashSet::new();
        let mut score = 0.0;
        let mut by_cat: std::collections::HashMap<Category, f64> = Default::default();
        for r in all.iter().filter(|r| r.msg.as_deref() == Some(msg)) {
            if seen.insert(r.reporter.as_str()) {
                let w = self.decayed(r, all);
                score += w;
                *by_cat.entry(r.category).or_default() += w;
            }
        }
        let cat = by_cat.into_iter().filter(|(_, w)| *w > 0.0).max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(c, _)| c);
        (score, cat)
    }

    /// Score of a user: distinct reporters (any message), and the main category.
    pub fn user_score(&self, user: &str, all: &[Report]) -> (f64, Option<Category>) {
        let mut best: std::collections::HashMap<&str, (f64, Category)> = Default::default();
        for r in all.iter().filter(|r| r.target == user) {
            let w = self.decayed(r, all);
            let e = best.entry(r.reporter.as_str()).or_insert((0.0, r.category));
            if w > e.0 {
                *e = (w, r.category);
            }
        }
        let score = best.values().map(|(w, _)| w).sum();
        let mut by_cat: std::collections::HashMap<Category, f64> = Default::default();
        for (w, c) in best.values() {
            *by_cat.entry(*c).or_default() += w;
        }
        let cat = by_cat.into_iter().filter(|(_, w)| *w > 0.0).max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(c, _)| c);
        (score, cat)
    }
}

/// Hide a message for me at this score (me, a friend, or 3 established users).
pub const HIDE_MSG: f64 = 0.65;
/// Mark a user with a warning.
pub const WARN_USER: f64 = 0.95;
/// Restrict a user: their public messages hidden by default; my node keeps no
/// mail for them and does not resend their messages in history sync.
pub const RESTRICT_USER: f64 = 1.9;

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    #[test]
    fn hint_triggers_on_offers_only() {
        assert!(looks_like_drug_sale("Продам мефедрон, закладки по району, оплата в btc"));
        assert!(looks_like_drug_sale("selling cocaine, dm me for price"));
        assert!(!looks_like_drug_sale("Кокаин разрушает сердце и сосуды — вот статья"));
        assert!(!looks_like_drug_sale("Продам велосипед, оплата при встрече"));
        assert!(!looks_like_drug_sale("Положи клад на полку в кладовке"));
    }

    #[test]
    fn reports_are_costly_and_weighed() {
        let alice = Keypair::generate_ed25519();
        let target = Keypair::generate_ed25519().public().to_peer_id().to_base58();
        let r = Report::create(&alice, &target, Some("m1".into()), None, Category::Drugs);
        assert!(r.verify());
        let mut forged = r.clone();
        forged.category = Category::Child;
        assert!(!forged.verify(), "changing the report breaks work and signature");
        let mut store = Reports::default();
        assert!(store.add(r.clone()));
        assert!(!store.add(r.clone()), "only once");

        let me = "me".to_string();
        let a = alice.public().to_peer_id().to_base58();
        let friends = |p: &str| p == a;
        let seen = |_: &str| None;
        let t = Trust { me: &me, is_friend: &friends, first_seen: &seen };
        let (s, c) = t.message_score("m1", &store.list);
        assert!((s - 0.7).abs() < 0.01 && c == Some(Category::Drugs));
        assert!(s >= HIDE_MSG);
        // unknown, brand-new identities weigh nothing — an army of fakes does not hide anything
        let mut army = Reports::default();
        for _ in 0..5 {
            let fake = Keypair::generate_ed25519();
            army.add(Report::create(&fake, &target, Some("m2".into()), None, Category::Child));
        }
        let nobody = |_: &str| false;
        let t2 = Trust { me: &me, is_friend: &nobody, first_seen: &seen };
        assert_eq!(t2.message_score("m2", &army.list).0, 0.0);
    }
}
