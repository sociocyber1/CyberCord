//! Shared wire + domain types for the CyberCord desktop app.

use serde::{Deserialize, Serialize};

/// A channel inside a guild (server). The pair is the room identity; its gossip
/// topic and on-disk file are both derived from it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId {
    pub guild: String,
    pub channel: String,
}

impl ChannelId {
    pub fn new(guild: impl Into<String>, channel: impl Into<String>) -> Self {
        Self { guild: guild.into(), channel: channel.into() }
    }
    /// A direct-message conversation, keyed by the *other* person's peer id.
    pub fn dm(other_peer: impl Into<String>) -> Self {
        Self { guild: "dm".into(), channel: other_peer.into() }
    }
    pub fn is_dm(&self) -> bool {
        self.guild == "dm"
    }
    /// Gossip topic string for this channel.
    pub fn topic(&self) -> String {
        format!("cc/ch/{}/{}", self.guild, self.channel)
    }
    /// Safe file stem for persistence.
    pub fn file_stem(&self) -> String {
        let san = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect::<String>();
        format!("{}__{}", san(&self.guild), san(&self.channel))
    }
}

/// One chat event; `name` is a cosmetic display name.
///
/// Since 0.12 every event carries its author's peer id (`from`) and an Ed25519
/// signature (`sig`) by that peer's key, so the author stays provable when the
/// event is replayed by someone else during history sync. That is what makes
/// editing and deleting safe without a server: an edit or delete is itself a
/// signed event (`edits` = target id) and is accepted only from the target's
/// own author. Older unsigned events are still shown but cannot be edited.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatEvent {
    /// Stable unique id (dedup across re-publish / history sync).
    pub id: String,
    pub channel: ChannelId,
    pub name: String,
    pub body: String,
    pub ts_ms: u64,
    /// Optional media: GIF, image or video circle (content-addressed blob).
    #[serde(default)]
    pub attach: Option<Attachment>,
    /// Author's peer id (base58); empty for legacy/unsigned events.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    /// Base64 Ed25519 signature over [`ChatEvent::signing_bytes`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sig: String,
    /// This event edits (new `body`) or deletes (`deleted`) the event with this id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edits: Option<String>,
    /// Version index of the edited message: 1 = original, 2 = first edit, …
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rev: u32,
    /// Delete the target (with `edits`), or "this original was deleted" on disk.
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}
fn is_false(v: &bool) -> bool {
    !*v
}

impl ChatEvent {
    pub fn new(channel: ChannelId, name: impl Into<String>, body: impl Into<String>) -> Self {
        let ts_ms = now_ms();
        let id = format!("{:032x}", rand::random::<u128>());
        Self { id, channel, name: name.into(), body: body.into(), ts_ms, attach: None, from: String::new(), sig: String::new(), edits: None, rev: 0, deleted: false }
    }

    /// Everything the author vouches for. The channel is left out on purpose:
    /// a DM is filed under the *other* person's id on each side.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let a = self.attach.as_ref().map(|a| format!("{}:{}:{}", a.hash, a.size, a.name)).unwrap_or_default();
        format!(
            "cybercord/msg/v1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.id,
            self.from,
            self.ts_ms,
            self.name,
            self.edits.as_deref().unwrap_or(""),
            self.rev,
            self.deleted,
            a,
            self.body
        )
        .into_bytes()
    }

    /// Set `from` and sign with the local identity.
    pub fn sign(&mut self, key: &libp2p::identity::Keypair) {
        use base64::Engine;
        self.from = key.public().to_peer_id().to_base58();
        if let Ok(sig) = key.sign(&self.signing_bytes()) {
            self.sig = base64::engine::general_purpose::STANDARD_NO_PAD.encode(sig);
        }
    }

    /// True if `sig` is a valid signature by the key behind `from`.
    pub fn verify(&self) -> bool {
        use base64::Engine;
        let (Some(pk), Ok(sig)) = (public_key_of(&self.from), base64::engine::general_purpose::STANDARD_NO_PAD.decode(&self.sig)) else {
            return false;
        };
        pk.verify(&self.signing_bytes(), &sig)
    }

    /// Accept or reject an incoming event: signed events must verify; unsigned
    /// (legacy) events lose any claimed author and may not edit or delete.
    pub fn admit(&mut self) -> bool {
        if self.sig.is_empty() {
            self.from.clear();
            return self.edits.is_none() && !self.deleted;
        }
        self.verify()
    }
}

/// The public key embedded in an Ed25519 peer id (identity multihash).
pub fn public_key_of(peer: &str) -> Option<libp2p::identity::PublicKey> {
    let id: libp2p::PeerId = peer.parse().ok()?;
    let bytes = id.to_bytes();
    if bytes.len() < 3 || bytes[0] != 0x00 || bytes[1] as usize != bytes.len() - 2 {
        return None;
    }
    let pk = libp2p::identity::PublicKey::try_decode_protobuf(&bytes[2..]).ok()?;
    (pk.to_peer_id() == id).then_some(pk)
}

/// Local wall-clock time of day, `HH:MM:SS`.
pub fn clock(ts_ms: u64) -> String {
    local(ts_ms).map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_else(|| {
        let secs = ts_ms / 1000;
        format!("{:02}:{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60, secs % 60)
    })
}

/// Short local date + time for "edited" marks: `21.11 14:35`, or `21.11.2025 14:35`
/// when it is not this year.
pub fn short_date(ts_ms: u64) -> String {
    use chrono::Datelike;
    let Some(t) = local(ts_ms) else { return clock(ts_ms) };
    if t.year() == chrono::Local::now().year() {
        t.format("%d.%m %H:%M").to_string()
    } else {
        t.format("%d.%m.%Y %H:%M").to_string()
    }
}

fn local(ts_ms: u64) -> Option<chrono::DateTime<chrono::Local>> {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ts_ms as i64).single()
}

/// Subscript digits for version indices: 2 → "₂", 12 → "₁₂".
pub fn subscript(n: u32) -> String {
    n.to_string().chars().map(|c| char::from_u32(0x2080 + c.to_digit(10).unwrap_or(0)).unwrap_or(c)).collect()
}

/// What kind of media an attachment is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Image,
    Gif,
    /// A round video message (like Telegram's video circles).
    Circle,
    /// A music / audio file (mp3, flac, ogg, wav, m4a) with an in-chat player.
    Audio,
    /// Any file, downloaded on demand piece by piece (torrent-like).
    File,
}

/// Media attached to a message. The bytes travel separately as a blob identified
/// by its BLAKE3 hash, so any node that has it can serve it and it is verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub hash: String,
    pub kind: MediaKind,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub w: u32,
    #[serde(default)]
    pub h: u32,
    #[serde(default)]
    pub dur_ms: u32,
}

/// A node's self-reported hub-election score, gossiped on the control topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreRec {
    pub peer: String, // base58 peer id of the sender
    pub name: String,
    pub score: f64,
    pub ts_ms: u64,
    /// Sender believes it is reachable from the internet (public IP or UPnP),
    /// so others may use it as a relay.
    #[serde(default)]
    pub reachable: bool,
    /// Public voice room the sender is in ("guild/channel"), so everyone can
    /// show who sits in which voice channel. Never set for private calls.
    #[serde(default)]
    pub voice: Option<String>,
    /// The user agreed to be found by name in people search.
    #[serde(default = "yes")]
    pub listed: bool,
    /// User status: "", online, offline (appear offline), work, sleep, busy,
    /// learning, relax, play, read.
    #[serde(default)]
    pub status: String,
}

fn yes() -> bool {
    true
}

/// One node as the network map shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeInfo {
    pub peer: String,
    pub name: Option<String>,
    /// Live connection right now.
    pub connected: bool,
    /// "lan", "direct" or "relay" when connected.
    pub link: &'static str,
    /// How we first learned about it: lan, saved, friend, node, exchange, dht, identify, search, global.
    pub source: &'static str,
    /// It relays for us / we relay for it.
    pub relays_us: bool,
    pub we_relay: bool,
    /// Seen a heartbeat from it recently (online somewhere in the network).
    pub online: bool,
    pub last_seen_ms: u64,
    pub status: String,
}

/// A person found by people search.
#[derive(Debug, Clone, PartialEq)]
pub struct UserCard {
    pub peer: String,
    pub name: String,
    pub online: bool,
    pub last_seen_ms: u64,
    /// "network" (heard on the network), "dht" (found through the DHT name index).
    pub via: &'static str,
    pub status: String,
}

/// DHT key for a name prefix (people search): providers of this key are users
/// whose display name starts with the prefix.
pub fn name_key(prefix: &str) -> Vec<u8> {
    let norm: String = prefix.trim().to_lowercase().chars().take(24).collect();
    blake3::hash(format!("cybercord/name/v1/{norm}").as_bytes()).as_bytes().to_vec()
}

/// All prefixes (3..=24 chars) of a display name, for the DHT name index.
pub fn name_prefixes(name: &str) -> Vec<String> {
    let norm: Vec<char> = name.trim().to_lowercase().chars().take(24).collect();
    let mut out = Vec::new();
    for n in 3..=norm.len() {
        out.push(norm[..n].iter().collect());
    }
    // every word too, so "Ivan Petrov" is found by "petr"
    for w in name.trim().to_lowercase().split_whitespace().skip(1) {
        let wc: Vec<char> = w.chars().take(24).collect();
        for n in 3..=wc.len() {
            let s: String = wc[..n].iter().collect();
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

/// One entry of the peer address book, as shared by peer exchange (PEX).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRec {
    pub peer: String,
    pub addrs: Vec<String>,
    #[serde(default)]
    pub last_seen_ms: u64,
}

/// One ~20 ms frame of mono voice audio (48 kHz, signed 16-bit PCM), as it
/// travels over a voice topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceFrame {
    pub room: ChannelId,
    pub name: String,
    pub seq: u32,
    pub pcm: Vec<i16>,
}

/// Everything that travels over gossip, on a channel, voice or control topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Wire {
    /// A chat message (channel topic).
    Chat(ChatEvent),
    /// A stored message re-sent in answer to SyncReq. The nonce makes the bytes
    /// unique, so gossipsub's content-hash de-duplication doesn't drop a replay of
    /// a message it has already seen.
    History { ev: ChatEvent, nonce: u64 },
    /// "Send me messages in this channel after `since_ms`" (channel topic).
    SyncReq {
        channel: ChannelId,
        since_ms: u64,
        /// Random; keeps two identical requests from being de-duplicated.
        #[serde(default)]
        nonce: u64,
    },
    /// A hub-election score (control topic).
    Score(ScoreRec),
    /// A frame of voice audio (voice topic).
    Voice(VoiceFrame),
    /// Peer exchange: nodes this sender knows and how to reach them (control topic).
    Peers(Vec<PeerRec>),
    /// A signed user profile (control topic).
    Profile(crate::profile::Profile),
    /// One chunk of a media blob (channel / DM topic). `data` is base64.
    /// `round` differs between re-sends so gossip does not drop them as duplicates.
    BlobChunk { hash: String, idx: u32, total: u32, round: u64, data: String },
    /// "Who has this blob? Please send it again" (channel / DM topic).
    /// "Who has this file?" — holders connect to the asker for a torrent-like download.
    FileWant {
        hash: String,
        #[serde(default)]
        nonce: u64,
    },
    BlobReq {
        hash: String,
        #[serde(default)]
        nonce: u64,
    },
}

impl ChannelId {
    /// Gossip topic for this channel's voice room.
    pub fn voice_topic(&self) -> String {
        format!("cc/voice/{}/{}", self.guild, self.channel)
    }
}

impl Wire {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("Wire serialises")
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// HH:MM:SS (UTC) from unix millis — small helper shared by UI and logs.
/// Symmetric gossip topic for a DM between two peers (same string on both sides).
pub fn dm_topic(a: &str, b: &str) -> String {
    if a <= b { format!("cc/dm/{a}/{b}") } else { format!("cc/dm/{b}/{a}") }
}

/// A saved friend: their peer id, the name you gave them, and the addresses to
/// dial them at (from their connect code).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Friend {
    pub peer_id: String,
    pub name: String,
    pub addrs: Vec<String>,
}

/// Parse a connect code: one or more multiaddrs (space/comma/newline separated),
/// each ending in /p2p/<peer-id>. Returns the peer id and the dialable addresses.
pub fn parse_connect_code(code: &str) -> Option<(String, Vec<String>)> {
    let mut peer: Option<String> = None;
    let mut addrs = Vec::new();
    for tok in code.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()) {
        if !tok.starts_with('/') {
            // A bare user ID is enough: the DHT finds their address through other users.
            if peer.is_none() && tok.parse::<libp2p::PeerId>().is_ok() {
                peer = Some(tok.to_string());
            }
            continue;
        }
        if let Some(idx) = tok.find("/p2p/") {
            let id = tok[idx + 5..].trim_end_matches('/').to_string();
            if id.is_empty() {
                continue;
            }
            match &peer {
                None => peer = Some(id),
                Some(p) if *p != id => continue, // ignore addresses of a different peer
                _ => {}
            }
            addrs.push(tok.to_string());
        }
    }
    peer.map(|p| (p, addrs))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dm_topic_is_symmetric() {
        assert_eq!(dm_topic("aaa", "bbb"), dm_topic("bbb", "aaa"));
    }
    #[test]
    fn parses_connect_code() {
        let code = "/ip4/1.2.3.4/udp/4710/quic-v1/p2p/12D3KooWX  /ip4/10.0.0.2/tcp/9/p2p/12D3KooWX";
        let (p, a) = parse_connect_code(code).unwrap();
        assert_eq!(p, "12D3KooWX");
        assert_eq!(a.len(), 2);
        assert!(parse_connect_code("hello world").is_none());
        let id = libp2p::PeerId::random().to_base58();
        let (p, a) = parse_connect_code(&id).unwrap();
        assert_eq!(p, id);
        assert!(a.is_empty());
    }
}
