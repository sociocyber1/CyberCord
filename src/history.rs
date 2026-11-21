//! Edits and deletions of chat messages, resolved locally from signed events.
//!
//! An edit or delete is an ordinary [`ChatEvent`] with `edits = Some(target)`,
//! so it travels, syncs and is stored exactly like a message. It counts only
//! if it is signed by the same peer that wrote the target. Versions are
//! indexed like in general semantics: message₁ is the original, message₂ the
//! first edit, and each version keeps its own date.

use crate::types::ChatEvent;

/// What to show for one message after applying its edits.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// Text to show now.
    pub body: String,
    /// Current version index (1 = never edited).
    pub rev: u32,
    /// When the current version was written.
    pub ts_ms: u64,
    pub deleted: bool,
    /// All versions, oldest first: (index, date, text).
    pub versions: Vec<(u32, u64, String)>,
}

/// True if `e` is a valid edit/delete of `original`.
pub fn applies(original: &ChatEvent, e: &ChatEvent) -> bool {
    e.edits.as_deref() == Some(original.id.as_str()) && !original.from.is_empty() && e.from == original.from
}

pub fn resolve(original: &ChatEvent, edits: &[ChatEvent]) -> Resolved {
    let mut mine: Vec<&ChatEvent> = edits.iter().filter(|e| applies(original, e)).collect();
    mine.sort_by(|a, b| (a.rev, a.ts_ms, &a.id).cmp(&(b.rev, b.ts_ms, &b.id)));
    let deleted = original.deleted || mine.iter().any(|e| e.deleted);
    let mut versions = vec![(1, original.ts_ms, original.body.clone())];
    for e in mine.iter().filter(|e| !e.deleted) {
        let rev = e.rev.max(2);
        match versions.last_mut() {
            // two edits with the same index (e.g. from two devices): the later wins
            Some(last) if last.0 == rev => *last = (rev, e.ts_ms, e.body.clone()),
            _ => versions.push((rev, e.ts_ms, e.body.clone())),
        }
    }
    let (rev, ts_ms, body) = versions.last().cloned().unwrap_or((1, original.ts_ms, original.body.clone()));
    Resolved { body, rev, ts_ms, deleted, versions }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChannelId;
    use libp2p::identity::Keypair;

    fn msg(k: &Keypair, body: &str) -> ChatEvent {
        let mut e = ChatEvent::new(ChannelId::new("g", "c"), "me", body);
        e.sign(k);
        e
    }
    fn edit(k: &Keypair, target: &ChatEvent, rev: u32, body: Option<&str>) -> ChatEvent {
        let mut e = ChatEvent::new(target.channel.clone(), "me", body.unwrap_or(""));
        e.edits = Some(target.id.clone());
        e.rev = rev;
        e.deleted = body.is_none();
        e.ts_ms = target.ts_ms + rev as u64 * 1000;
        e.sign(k);
        e
    }

    #[test]
    fn signatures_bind_author_and_content() {
        let k = Keypair::generate_ed25519();
        let mut e = msg(&k, "hello");
        assert!(e.clone().admit());
        e.body = "forged".into();
        assert!(!e.clone().admit(), "changed body must fail");
        let mut legacy = ChatEvent::new(ChannelId::new("g", "c"), "old", "hi");
        legacy.from = "12D3KooWfake".into();
        assert!(legacy.admit() && legacy.from.is_empty(), "unsigned: shown, author dropped");
        let mut bad_edit = ChatEvent::new(ChannelId::new("g", "c"), "x", "y");
        bad_edit.edits = Some("abc".into());
        assert!(!bad_edit.admit(), "unsigned edits are refused");
    }

    #[test]
    fn edits_versions_and_deletes() {
        let me = Keypair::generate_ed25519();
        let other = Keypair::generate_ed25519();
        let m = msg(&me, "teh text");
        let e2 = edit(&me, &m, 2, Some("the text"));
        let e3 = edit(&me, &m, 3, Some("the text, fixed"));
        let fake = edit(&other, &m, 4, Some("hijacked"));
        let r = resolve(&m, &[e3.clone(), fake.clone(), e2.clone()]);
        assert_eq!(r.body, "the text, fixed");
        assert_eq!(r.rev, 3);
        assert_eq!(r.versions.len(), 3);
        assert!(!r.deleted);
        let del_fake = edit(&other, &m, 5, None);
        assert!(!resolve(&m, &[del_fake]).deleted, "only the author may delete");
        let del = edit(&me, &m, 4, None);
        assert!(resolve(&m, &[e2, del]).deleted);
        let unsigned_original = ChatEvent::new(ChannelId::new("g", "c"), "me", "x");
        assert_eq!(resolve(&unsigned_original, &[e3]).rev, 1, "legacy messages cannot be edited");
    }
}
