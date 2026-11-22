//! On-disk persistence: one append-only JSON-lines file per channel, under the
//! user's data directory. Chats survive restarts, and the file is the source a
//! peer replays from when answering a history-sync request.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::types::{ChannelId, ChatEvent};

/// Base data directory: $XDG_DATA_HOME/cybercord or ~/.local/share/cybercord,
/// overridable with $CYBERCORD_DATA_DIR (used by tests to isolate state).
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CYBERCORD_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".local").join("share")
        });
    base.join("cybercord")
}

fn channel_path(channel: &ChannelId) -> PathBuf {
    data_dir().join("channels").join(format!("{}.jsonl", channel.file_stem()))
}

fn ensure_parent(path: &PathBuf) {
    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
}

/// Append one event. Best-effort: a failure to persist never crashes the app.
pub fn append(event: &ChatEvent) {
    let path = channel_path(&event.channel);
    ensure_parent(&path);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        if let Ok(line) = serde_json::to_string(event) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Load the most recent `limit` events for a channel, oldest-first.
pub fn load_recent(channel: &ChannelId, limit: usize) -> Vec<ChatEvent> {
    let path = channel_path(channel);
    let Ok(file) = File::open(&path) else { return Vec::new() };
    let mut all: Vec<ChatEvent> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<ChatEvent>(&l).ok())
        .collect();
    if all.len() > limit {
        all.drain(0..all.len() - limit);
    }
    all
}

/// Set of event ids already stored for a channel (for dedup).
pub fn load_ids(channel: &ChannelId) -> HashSet<String> {
    let path = channel_path(channel);
    let Ok(file) = File::open(&path) else { return HashSet::new() };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<ChatEvent>(&l).ok().map(|e| e.id))
        .collect()
}

/// A valid delete arrived: blank the original and all its earlier versions on
/// disk (only lines by the same author). The delete event itself stays, so it
/// can reach peers that still hold the message. Blanked lines lose their
/// signature, so nobody accepts them again from us.
pub fn redact(channel: &ChannelId, target: &str, author: &str) {
    if author.is_empty() {
        return;
    }
    let path = channel_path(channel);
    let Ok(text) = fs::read_to_string(&path) else { return };
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut keep = line.to_string();
        if let Ok(mut ev) = serde_json::from_str::<ChatEvent>(line) {
            let is_target = ev.id == target && ev.edits.is_none();
            let is_old_version = ev.edits.as_deref() == Some(target) && !ev.deleted;
            if (is_target || is_old_version) && ev.from == author && !ev.sig.is_empty() {
                ev.body.clear();
                ev.attach = None;
                ev.sig.clear();
                ev.deleted = true;
                if let Ok(l) = serde_json::to_string(&ev) {
                    keep = l;
                    changed = true;
                }
            }
        }
        out.push_str(&keep);
        out.push('\n');
    }
    if changed {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, out).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

/// Events in a channel strictly after `since_ms`, capped at `limit` (newest kept),
/// oldest-first. Used to answer a peer's history-sync request.
pub fn load_since(channel: &ChannelId, since_ms: u64, limit: usize) -> Vec<ChatEvent> {
    let mut v: Vec<ChatEvent> = load_recent(channel, 10_000)
        .into_iter()
        .filter(|e| e.ts_ms > since_ms)
        // blanked (deleted) lines are not worth sending: nobody would accept them
        .filter(|e| !(e.deleted && e.sig.is_empty()))
        .collect();
    if v.len() > limit {
        v.drain(0..v.len() - limit);
    }
    v
}

// ---- friends ----

fn friends_path() -> PathBuf {
    data_dir().join("friends.jsonl")
}

/// Save (or update) a friend. Append-only; the latest record per peer wins on load.
pub fn save_friend(f: &crate::types::Friend) {
    let path = friends_path();
    ensure_parent(&path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        if let Ok(line) = serde_json::to_string(f) {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// All saved friends, latest record per peer id, in insertion order.
pub fn load_friends() -> Vec<crate::types::Friend> {
    let Ok(file) = File::open(friends_path()) else { return Vec::new() };
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, crate::types::Friend> = std::collections::HashMap::new();
    for f in BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<crate::types::Friend>(&l).ok())
    {
        if !map.contains_key(&f.peer_id) {
            order.push(f.peer_id.clone());
        }
        map.insert(f.peer_id.clone(), f);
    }
    order.into_iter().filter_map(|p| map.remove(&p)).collect()
}

// ---- peer address book (every node we have learned about) ----

fn peers_path() -> PathBuf {
    data_dir().join("peers.json")
}

/// Persist the peer address book so the next start can re-join the network
/// without any server.
pub fn save_peers(peers: &[crate::types::PeerRec]) {
    let path = peers_path();
    ensure_parent(&path);
    if let Ok(json) = serde_json::to_string(peers) {
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

pub fn load_peers() -> Vec<crate::types::PeerRec> {
    fs::read_to_string(peers_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

// ---- people directory (names heard on the network, for search) ----

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DirRec {
    pub peer: String,
    pub name: String,
    pub last_seen_ms: u64,
}

fn directory_path() -> PathBuf {
    data_dir().join("directory.json")
}

pub fn save_directory(recs: &[DirRec]) {
    let path = directory_path();
    ensure_parent(&path);
    if let Ok(json) = serde_json::to_string(recs) {
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

pub fn load_directory() -> Vec<DirRec> {
    fs::read_to_string(directory_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

// ---- media blobs (content-addressed by BLAKE3) ----

fn blob_path(hash: &str) -> PathBuf {
    let safe: String = hash.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    data_dir().join("blobs").join(safe)
}

/// Store bytes and return their BLAKE3 hash (hex).
pub fn save_blob(bytes: &[u8]) -> String {
    let hash = blake3::hash(bytes).to_hex().to_string();
    let path = blob_path(&hash);
    if !path.exists() {
        ensure_parent(&path);
        let tmp = path.with_extension("part");
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
    hash
}

pub fn has_blob(hash: &str) -> bool {
    blob_path(hash).exists()
}

pub fn load_blob(hash: &str) -> Option<Vec<u8>> {
    fs::read(blob_path(hash)).ok()
}

// ---- settings ----

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// UI language code, or "auto" / empty for the system language.
    #[serde(default)]
    pub lang: String,
    /// The onboarding tour was completed or skipped.
    #[serde(default)]
    pub tour_done: bool,
    /// Saved GIFs (blob hashes) for quick re-sending.
    #[serde(default)]
    pub gifs: Vec<String>,
    /// Microphone name (empty = system default).
    #[serde(default)]
    pub in_dev: String,
    /// Speakers / headphones name (empty = system default).
    #[serde(default)]
    pub out_dev: String,
    /// Input and output volume, percent (0–200).
    #[serde(default = "hundred")]
    pub in_vol: f32,
    #[serde(default = "hundred")]
    pub out_vol: f32,
    /// Input sensitivity: automatic, or a manual threshold in dBFS.
    #[serde(default = "yes")]
    pub sens_auto: bool,
    #[serde(default = "minus50")]
    pub sens_db: f32,
    /// Colour theme id (see the app's theme list); empty = GNOME light.
    #[serde(default)]
    pub theme: String,
    /// Interface zoom, percent.
    #[serde(default = "hundred")]
    pub ui_scale: f32,
    /// Others can find me by name in people search.
    #[serde(default = "yes")]
    pub listed: bool,
    /// Find other CyberCord users worldwide through the BitTorrent DHT.
    #[serde(default = "yes")]
    pub global: bool,
    /// Closing the window keeps CyberCord running in the tray.
    #[serde(default = "yes")]
    pub background: bool,
    /// Start with the system (hidden in the tray).
    #[serde(default)]
    pub autostart: bool,
    /// Look for new users.
    #[serde(default = "yes")]
    pub scan: bool,
    /// Desktop notifications for direct messages and mentions.
    #[serde(default = "yes")]
    pub notify: bool,
    /// Status shown to others.
    #[serde(default)]
    pub status: String,
    /// "Words matter": offer a short document when my own message contains an insult.
    /// Nothing is ever blocked; the counter stays on this computer.
    #[serde(default = "yes")]
    pub words_hints: bool,
    #[serde(default)]
    pub words_count: u32,
    /// "Hacked text" animation for new messages (and the brand on start).
    #[serde(default = "yes")]
    pub hack_fx: bool,
    /// Show channel names in the interface language.
    #[serde(default = "yes")]
    pub ch_translate: bool,
    /// Intro video at start: "always" (default), "first" or "never".
    #[serde(default)]
    pub intro_mode: String,
    #[serde(default)]
    pub intro_seen: bool,
}

fn hundred() -> f32 {
    100.0
}
fn yes() -> bool {
    true
}
fn minus50() -> f32 {
    -50.0
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_str("{}").expect("defaults")
    }
}

impl Settings {
    /// Push the sound levels to the audio engine.
    pub fn apply_levels(&self) {
        crate::voice::set_levels(self.in_vol, self.out_vol, (!self.sens_auto).then_some(self.sens_db));
    }
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn load_settings() -> Settings {
    fs::read_to_string(settings_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_settings(s: &Settings) {
    let path = settings_path();
    ensure_parent(&path);
    if let Ok(json) = serde_json::to_string_pretty(s) {
        let _ = fs::write(path, json);
    }
}
