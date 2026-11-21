//! Torrent-like file transfer.
//!
//! * A shared file is cut into 256 KB pieces; its **manifest** lists the BLAKE3
//!   hash of every piece. The file's ID is the BLAKE3 hash of the whole file.
//! * Pieces travel by direct request/response between two nodes
//!   (`/cybercord/files/1`), not by flooding the room.
//! * A download asks several nodes at once (up to 32 pieces in flight, 8 per
//!   node), checks every piece against the manifest, and writes it straight
//!   into `<name>.part`. Progress is saved, so it survives restarts.
//! * Who has the file: members of the room it was posted in, plus Kademlia
//!   providers. If nobody is online the download waits and starts by itself
//!   when someone who has the file comes online.
//! * Every node serves the pieces it has — finished files and unfinished ones
//!   (like a torrent swarm). Finished downloads are verified against the file
//!   ID, then announced in the DHT.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use libp2p::PeerId;
use serde::{Deserialize, Serialize};

use crate::store;
use crate::types::{now_ms, ChannelId};

pub const PIECE: usize = 256 * 1024;
/// Largest file we share/download: 8 GB.
pub const MAX_FILE: u64 = 8 * 1024 * 1024 * 1024;
const MAX_INFLIGHT: usize = 32;
const MAX_PER_PEER: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileReq {
    Manifest { hash: String },
    Piece { hash: String, idx: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileResp {
    Manifest(Manifest),
    Piece {
        idx: u32,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub size: u64,
    pub piece: u32,
    /// BLAKE3 of every piece, hex.
    pub pieces: Vec<String>,
}

impl Manifest {
    pub fn count(&self) -> usize {
        self.pieces.len()
    }
    fn piece_len(&self, idx: usize) -> usize {
        let start = idx as u64 * self.piece as u64;
        (self.size.saturating_sub(start)).min(self.piece as u64) as usize
    }
}

/// DHT key under which holders of a file announce themselves.
pub fn provider_key(hash: &str) -> Vec<u8> {
    blake3::hash(format!("cybercord/file/v1/{hash}").as_bytes()).as_bytes().to_vec()
}

/// Hash a file: (file ID, manifest). Streams, so any size works.
pub fn hash_file(path: &Path) -> std::io::Result<(String, Manifest)> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    let mut whole = blake3::Hasher::new();
    let mut pieces = Vec::new();
    let mut buf = vec![0u8; PIECE];
    loop {
        let mut n = 0;
        while n < PIECE {
            let r = f.read(&mut buf[n..])?;
            if r == 0 {
                break;
            }
            n += r;
        }
        if n == 0 {
            break;
        }
        whole.update(&buf[..n]);
        pieces.push(blake3::hash(&buf[..n]).to_hex().to_string());
        if n < PIECE {
            break;
        }
    }
    Ok((whole.finalize().to_hex().to_string(), Manifest { size, piece: PIECE as u32, pieces }))
}

/// Manifest of an in-memory blob (media already in `blobs/`).
fn manifest_of_bytes(b: &[u8]) -> Manifest {
    Manifest { size: b.len() as u64, piece: PIECE as u32, pieces: b.chunks(PIECE).map(|c| blake3::hash(c).to_hex().to_string()).collect() }
}

// ---------------------------------------------------------------- persistence

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedRec {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlRec {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub channel: ChannelId,
    pub added_ms: u64,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub failed: bool,
    /// Final file (when done) or the .part file.
    pub path: PathBuf,
    /// Pieces we have, packed bits as hex.
    #[serde(default)]
    pub have: String,
}

fn shared_path() -> PathBuf {
    store::data_dir().join("shared.json")
}
fn downloads_path() -> PathBuf {
    store::data_dir().join("downloads.json")
}
fn manifest_path(hash: &str) -> PathBuf {
    store::data_dir().join("manifests").join(format!("{}.json", &hash[..hash.len().min(64)]))
}

fn load_json<T: for<'a> Deserialize<'a> + Default>(p: &Path) -> T {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}
fn save_json<T: Serialize>(p: &Path, v: &T) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(j) = serde_json::to_string(v) {
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, j).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }
}

fn pack(bits: &[bool]) -> String {
    let mut bytes = vec![0u8; bits.len().div_ceil(8)];
    for (i, b) in bits.iter().enumerate() {
        if *b {
            bytes[i / 8] |= 1 << (i % 8);
        }
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unpack(hex: &str, n: usize) -> Vec<bool> {
    let bytes: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect();
    (0..n).map(|i| bytes.get(i / 8).map(|b| b & (1 << (i % 8)) != 0).unwrap_or(false)).collect()
}

/// Where downloads go: XDG download dir (or ~/Downloads) / CyberCord.
pub fn download_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CYBERCORD_DOWNLOADS") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| ".".into());
    let mut dl = PathBuf::from(&home).join("Downloads");
    // ~/.config/user-dirs.dirs: XDG_DOWNLOAD_DIR="$HOME/Загрузки"
    if let Ok(t) = std::fs::read_to_string(PathBuf::from(&home).join(".config/user-dirs.dirs")) {
        for l in t.lines() {
            if let Some(v) = l.strip_prefix("XDG_DOWNLOAD_DIR=") {
                dl = PathBuf::from(v.trim_matches('"').replace("$HOME", &home));
            }
        }
    }
    dl.join("CyberCord")
}

/// A file name that cannot escape the download folder.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("file");
    let clean: String = base.chars().filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')).take(120).collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    if clean.is_empty() {
        "file".into()
    } else {
        clean
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for i in 1..1000 {
        let p = dir.join(format!("{stem} ({i}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}{ext}", now_ms()))
}

// ---------------------------------------------------------------- engine

/// What the UI shows for one transfer.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferInfo {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub have_bytes: u64,
    /// "waiting", "downloading", "paused", "verifying", "done", "failed", "seeding"
    pub state: &'static str,
    pub peers: usize,
    pub speed_bps: u64,
    pub path: Option<String>,
    pub channel: Option<ChannelId>,
}

struct Dl {
    rec: DlRec,
    manifest: Option<Manifest>,
    have: Vec<bool>,
    /// piece -> (peer, sent)
    inflight: HashMap<u32, (PeerId, Instant)>,
    manifest_asked: Option<(PeerId, Instant)>,
    bad: HashMap<PeerId, Instant>,
    recent: Vec<(Instant, usize)>,
    peers_seen: HashSet<PeerId>,
    verifying: bool,
    dirty: bool,
}

impl Dl {
    fn have_bytes(&self) -> u64 {
        match &self.manifest {
            Some(m) => self.have.iter().enumerate().filter(|(_, h)| **h).map(|(i, _)| m.piece_len(i) as u64).sum(),
            None => 0,
        }
    }
    fn complete(&self) -> bool {
        self.manifest.is_some() && !self.have.is_empty() && self.have.iter().all(|h| *h)
    }
}

/// Network actions the engine asks for.
pub enum Action {
    Request(PeerId, FileReq),
    /// Find holders of this file in the DHT.
    FindProviders(String),
    /// Announce that we hold this file.
    Provide(String),
    /// Verify a finished file in the background: (hash, path).
    Verify(String, PathBuf),
    /// Ask the room who has this file (holders then connect to us).
    AskRoom(String, ChannelId),
}

pub struct Transfers {
    shared: HashMap<String, SharedRec>,
    manifests: HashMap<String, Manifest>,
    dls: HashMap<String, Dl>,
    /// request id (as u64 from the swarm) -> (hash, piece or None for manifest)
    pending: HashMap<u64, (String, Option<u32>, PeerId)>,
    last_find: HashMap<String, Instant>,
    last_ask: HashMap<String, Instant>,
    last_save: Option<Instant>,
}

impl Default for Transfers {
    fn default() -> Self {
        Self::load()
    }
}

impl Transfers {
    pub fn load() -> Self {
        let shared: Vec<SharedRec> = load_json(&shared_path());
        let recs: Vec<DlRec> = load_json(&downloads_path());
        let mut t = Self { shared: shared.into_iter().map(|s| (s.hash.clone(), s)).collect(), manifests: HashMap::new(), dls: HashMap::new(), pending: HashMap::new(), last_find: HashMap::new(), last_ask: HashMap::new(), last_save: None };
        for rec in recs {
            let manifest: Option<Manifest> = std::fs::read_to_string(manifest_path(&rec.hash)).ok().and_then(|j| serde_json::from_str(&j).ok());
            let n = manifest.as_ref().map(|m| m.count()).unwrap_or(0);
            let have = if rec.done { vec![true; n] } else { unpack(&rec.have, n) };
            t.dls.insert(
                rec.hash.clone(),
                Dl { rec, manifest, have, inflight: HashMap::new(), manifest_asked: None, bad: HashMap::new(), recent: Vec::new(), peers_seen: HashSet::new(), verifying: false, dirty: false },
            );
        }
        t
    }

    fn save_downloads(&mut self) {
        let recs: Vec<DlRec> = self
            .dls
            .values_mut()
            .map(|d| {
                d.dirty = false;
                let mut r = d.rec.clone();
                r.have = pack(&d.have);
                r
            })
            .collect();
        save_json(&downloads_path(), &recs);
    }

    fn save_shared(&self) {
        let v: Vec<&SharedRec> = self.shared.values().collect();
        save_json(&shared_path(), &v);
    }

    /// Hashes we can serve completely (shared + finished downloads), for DHT announcements.
    pub fn seeding(&self) -> Vec<String> {
        let mut v: Vec<String> = self.shared.keys().cloned().collect();
        v.extend(self.dls.values().filter(|d| d.rec.done).map(|d| d.rec.hash.clone()));
        v
    }

    /// Register a local file we share (hashing already done).
    pub fn add_shared(&mut self, rec: SharedRec, manifest: Manifest) {
        save_json(&manifest_path(&rec.hash), &manifest);
        self.manifests.insert(rec.hash.clone(), manifest);
        self.shared.insert(rec.hash.clone(), rec);
        self.save_shared();
    }

    pub fn is_shared(&self, hash: &str) -> bool {
        self.shared.contains_key(hash)
    }

    /// We can serve at least part of this file.
    pub fn has_any(&self, hash: &str) -> bool {
        self.shared.contains_key(hash) || self.dls.get(hash).map(|d| d.rec.done || d.have.iter().any(|h| *h)).unwrap_or(false)
    }

    /// Queue a download (no-op if known). Works while nobody is online.
    pub fn add_download(&mut self, hash: &str, name: &str, size: u64, channel: ChannelId) -> Result<(), String> {
        if self.dls.contains_key(hash) || self.shared.contains_key(hash) {
            return Ok(());
        }
        if size > MAX_FILE {
            return Err("file is too big".into());
        }
        let dir = download_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = unique_path(&dir, &format!("{}.part", safe_name(name)));
        let rec = DlRec { hash: hash.into(), name: safe_name(name), size, channel, added_ms: now_ms(), paused: false, done: false, failed: false, path, have: String::new() };
        self.dls.insert(
            hash.into(),
            Dl { rec, manifest: None, have: Vec::new(), inflight: HashMap::new(), manifest_asked: None, bad: HashMap::new(), recent: Vec::new(), peers_seen: HashSet::new(), verifying: false, dirty: true },
        );
        self.save_downloads();
        Ok(())
    }

    pub fn set_paused(&mut self, hash: &str, paused: bool) {
        if let Some(d) = self.dls.get_mut(hash) {
            d.rec.paused = paused;
            d.rec.failed = false;
            d.inflight.clear();
        }
        self.save_downloads();
    }

    /// Cancel and delete the unfinished file.
    pub fn cancel(&mut self, hash: &str) {
        if let Some(d) = self.dls.remove(hash) {
            if !d.rec.done {
                let _ = std::fs::remove_file(&d.rec.path);
            }
        }
        let _ = std::fs::remove_file(manifest_path(hash));
        self.save_downloads();
    }

    // ---------------- serving

    fn manifest_for(&mut self, hash: &str) -> Option<Manifest> {
        if let Some(m) = self.manifests.get(hash) {
            return Some(m.clone());
        }
        if let Some(d) = self.dls.get(hash) {
            if let Some(m) = &d.manifest {
                return Some(m.clone());
            }
        }
        if let Ok(j) = std::fs::read_to_string(manifest_path(hash)) {
            if let Ok(m) = serde_json::from_str::<Manifest>(&j) {
                self.manifests.insert(hash.into(), m.clone());
                return Some(m);
            }
        }
        // media blobs can be served the same way
        if let Some(b) = store::load_blob(hash) {
            let m = manifest_of_bytes(&b);
            self.manifests.insert(hash.into(), m.clone());
            return Some(m);
        }
        None
    }

    /// Answer a request from another node.
    pub fn serve(&mut self, req: FileReq) -> FileResp {
        match req {
            FileReq::Manifest { hash } => self.manifest_for(&hash).map(FileResp::Manifest).unwrap_or(FileResp::NotFound),
            FileReq::Piece { hash, idx } => {
                let Some(m) = self.manifest_for(&hash) else { return FileResp::NotFound };
                let i = idx as usize;
                if i >= m.count() {
                    return FileResp::NotFound;
                }
                // which file holds it?
                let path: Option<PathBuf> = if let Some(s) = self.shared.get(&hash) {
                    Some(s.path.clone())
                } else if let Some(d) = self.dls.get(&hash) {
                    if d.have.get(i).copied().unwrap_or(false) {
                        Some(d.rec.path.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                let data = match path {
                    Some(p) => read_piece(&p, &m, i),
                    None => store::load_blob(&hash).map(|b| b[i * PIECE..(i * PIECE + m.piece_len(i)).min(b.len())].to_vec()),
                };
                match data {
                    Some(data) if blake3::hash(&data).to_hex().as_str() == m.pieces[i] => FileResp::Piece { idx, data },
                    _ => FileResp::NotFound,
                }
            }
        }
    }

    // ---------------- downloading

    /// Plan requests. `candidates(hash, channel)` = nodes that may have it.
    pub fn tick(&mut self, connected: &HashSet<PeerId>, mut candidates: impl FnMut(&str, &ChannelId) -> Vec<PeerId>, alloc_id: &mut dyn FnMut(PeerId, FileReq) -> u64) -> Vec<Action> {
        let mut out = Vec::new();
        let now = Instant::now();
        let mut inflight_by_peer: HashMap<PeerId, usize> = HashMap::new();
        for d in self.dls.values() {
            for (p, _) in d.inflight.values() {
                *inflight_by_peer.entry(*p).or_default() += 1;
            }
        }
        let hashes: Vec<String> = self.dls.keys().cloned().collect();
        for h in hashes {
            let d = self.dls.get_mut(&h).expect("present");
            if d.rec.done || d.rec.paused || d.rec.failed || d.verifying {
                continue;
            }
            // time out stuck requests (30 s)
            d.inflight.retain(|_, (_, t)| t.elapsed().as_secs() < 30);
            d.bad.retain(|_, t| t.elapsed().as_secs() < 20);
            let mut cands: Vec<PeerId> = candidates(&h, &d.rec.channel).into_iter().filter(|p| connected.contains(p) && !d.bad.contains_key(p)).collect();
            cands.sort();
            cands.dedup();
            d.peers_seen = cands.iter().copied().collect();
            // No data flowing: ask the room every 15 s and the DHT every 20 s
            // (every 60 s once pieces arrive).
            let flowing = d.recent.iter().any(|(t, _)| t.elapsed().as_secs() < 5);
            let find_every = if flowing { 60 } else { 20 };
            if self.last_find.get(&h).map(|t| t.elapsed().as_secs() >= find_every).unwrap_or(true) {
                self.last_find.insert(h.clone(), now);
                out.push(Action::FindProviders(h.clone()));
            }
            if !flowing && self.last_ask.get(&h).map(|t| t.elapsed().as_secs() >= 15).unwrap_or(true) {
                self.last_ask.insert(h.clone(), now);
                out.push(Action::AskRoom(h.clone(), d.rec.channel.clone()));
            }
            if cands.is_empty() {
                continue;
            }
            if d.manifest.is_none() {
                let ask = d.manifest_asked.map(|(_, t)| t.elapsed().as_secs() >= 15).unwrap_or(true);
                if ask {
                    let p = cands[(now_ms() as usize) % cands.len()];
                    d.manifest_asked = Some((p, now));
                    let req = FileReq::Manifest { hash: h.clone() };
                    let id = alloc_id(p, req);
                    self.pending.insert(id, (h.clone(), None, p));
                }
                continue;
            }
            // pieces: missing and not in flight, random start so peers get different pieces
            let n = d.have.len();
            let missing: Vec<u32> = (0..n).filter(|i| !d.have[*i] && !d.inflight.contains_key(&(*i as u32))).map(|i| i as u32).collect();
            if missing.is_empty() {
                continue;
            }
            let start = (rand::random::<u64>() as usize) % missing.len();
            let mut ci = 0usize;
            for k in 0..missing.len() {
                if d.inflight.len() >= MAX_INFLIGHT {
                    break;
                }
                // next peer with capacity
                let mut chosen = None;
                for _ in 0..cands.len() {
                    let p = cands[ci % cands.len()];
                    ci += 1;
                    if inflight_by_peer.get(&p).copied().unwrap_or(0) < MAX_PER_PEER {
                        chosen = Some(p);
                        break;
                    }
                }
                let Some(p) = chosen else { break };
                let idx = missing[(start + k) % missing.len()];
                let id = alloc_id(p, FileReq::Piece { hash: h.clone(), idx });
                d.inflight.insert(idx, (p, now));
                *inflight_by_peer.entry(p).or_default() += 1;
                self.pending.insert(id, (h.clone(), Some(idx), p));
            }
        }
        // progress is saved at most every 2 s
        if self.dls.values().any(|d| d.dirty) && self.last_save.map(|t| t.elapsed().as_secs() >= 2).unwrap_or(true) {
            self.last_save = Some(now);
            self.save_downloads();
        }
        out
    }

    /// A response arrived. Returns actions (e.g. verification of a finished file).
    pub fn on_response(&mut self, id: u64, resp: FileResp) -> Vec<Action> {
        let mut out = Vec::new();
        let Some((h, piece, peer)) = self.pending.remove(&id) else { return out };
        let Some(d) = self.dls.get_mut(&h) else { return out };
        match (piece, resp) {
            (None, FileResp::Manifest(m)) => {
                let plausible = m.size == d.rec.size && m.piece as usize == PIECE && m.count() == (m.size as usize).div_ceil(PIECE).max(if m.size == 0 { 0 } else { 1 });
                if plausible && d.manifest.is_none() {
                    save_json(&manifest_path(&h), &m);
                    d.have = vec![false; m.count()];
                    d.manifest = Some(m);
                    // sparse file of the right size
                    if let Ok(f) = OpenOptions::new().create(true).truncate(false).write(true).open(&d.rec.path) {
                        let _ = f.set_len(d.rec.size);
                    }
                    d.dirty = true;
                } else if !plausible {
                    d.bad.insert(peer, Instant::now());
                }
                d.manifest_asked = None;
            }
            (Some(idx), FileResp::Piece { idx: ridx, data }) if idx == ridx => {
                d.inflight.remove(&idx);
                let ok = d.manifest.as_ref().map(|m| (idx as usize) < m.count() && blake3::hash(&data).to_hex().as_str() == m.pieces[idx as usize]).unwrap_or(false);
                if ok && write_piece(&d.rec.path, idx as usize, &data).is_ok() {
                    d.have[idx as usize] = true;
                    d.recent.push((Instant::now(), data.len()));
                    d.dirty = true;
                    if d.complete() {
                        d.verifying = true;
                        out.push(Action::Verify(h.clone(), d.rec.path.clone()));
                    }
                } else {
                    d.bad.insert(peer, Instant::now());
                }
            }
            (Some(idx), _) => {
                d.inflight.remove(&idx);
                d.bad.insert(peer, Instant::now());
            }
            (None, _) => {
                d.manifest_asked = None;
                d.bad.insert(peer, Instant::now());
            }
        }
        out
    }

    pub fn on_failure(&mut self, id: u64) {
        if let Some((h, piece, peer)) = self.pending.remove(&id) {
            if let Some(d) = self.dls.get_mut(&h) {
                match piece {
                    Some(i) => {
                        d.inflight.remove(&i);
                    }
                    None => d.manifest_asked = None,
                }
                d.bad.insert(peer, Instant::now());
            }
        }
    }

    /// Background verification finished.
    pub fn on_verified(&mut self, hash: &str, ok: bool) -> Vec<Action> {
        let mut out = Vec::new();
        if let Some(d) = self.dls.get_mut(hash) {
            d.verifying = false;
            if ok {
                // drop ".part"
                let final_name = d.rec.name.clone();
                let dir = d.rec.path.parent().map(Path::to_path_buf).unwrap_or_else(download_dir);
                let dest = unique_path(&dir, &final_name);
                if std::fs::rename(&d.rec.path, &dest).is_ok() {
                    d.rec.path = dest;
                }
                d.rec.done = true;
                out.push(Action::Provide(hash.to_string()));
            } else {
                // wrong manifest or data: start over with a manifest from someone else
                d.have.iter_mut().for_each(|h| *h = false);
                d.manifest = None;
                let _ = std::fs::remove_file(manifest_path(hash));
                d.rec.failed = false;
            }
            d.dirty = true;
        }
        self.save_downloads();
        out
    }

    pub fn infos(&mut self) -> Vec<TransferInfo> {
        let now = Instant::now();
        let mut v: Vec<TransferInfo> = self
            .dls
            .values_mut()
            .map(|d| {
                d.recent.retain(|(t, _)| now.duration_since(*t).as_secs() < 5);
                let speed = d.recent.iter().map(|(_, n)| *n as u64).sum::<u64>() / 5;
                let state = if d.rec.done {
                    "done"
                } else if d.verifying {
                    "verifying"
                } else if d.rec.paused {
                    "paused"
                } else if d.rec.failed {
                    "failed"
                } else if d.recent.is_empty() {
                    "waiting"
                } else {
                    "downloading"
                };
                TransferInfo {
                    hash: d.rec.hash.clone(),
                    name: d.rec.name.clone(),
                    size: d.rec.size,
                    have_bytes: if d.rec.done { d.rec.size } else { d.have_bytes() },
                    state,
                    peers: d.peers_seen.len(),
                    speed_bps: speed,
                    path: Some(d.rec.path.display().to_string()),
                    channel: Some(d.rec.channel.clone()),
                }
            })
            .collect();
        v.extend(self.shared.values().map(|s| TransferInfo {
            hash: s.hash.clone(),
            name: s.name.clone(),
            size: s.size,
            have_bytes: s.size,
            state: "seeding",
            peers: 0,
            speed_bps: 0,
            path: Some(s.path.display().to_string()),
            channel: None,
        }));
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}

fn read_piece(path: &Path, m: &Manifest, idx: usize) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    f.seek(SeekFrom::Start(idx as u64 * m.piece as u64)).ok()?;
    let mut buf = vec![0u8; m.piece_len(idx)];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn write_piece(path: &Path, idx: usize, data: &[u8]) -> std::io::Result<()> {
    let mut f = OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    f.seek(SeekFrom::Start((idx * PIECE) as u64))?;
    f.write_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_roundtrip() {
        let b = vec![true, false, true, true, false, false, false, true, true];
        assert_eq!(unpack(&pack(&b), b.len()), b);
    }

    #[test]
    fn names_stay_inside_the_folder() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("C:\\x\\..\\evil.exe"), "evil.exe");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name(""), "file");
    }

    #[test]
    fn manifest_matches_pieces() {
        let dir = std::env::temp_dir().join(format!("cc-tr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.bin");
        let data: Vec<u8> = (0..(PIECE * 2 + 1000)).map(|i| (i % 251) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        let (h, m) = hash_file(&p).unwrap();
        assert_eq!(h, blake3::hash(&data).to_hex().to_string());
        assert_eq!(m.count(), 3);
        assert_eq!(m.piece_len(2), 1000);
        assert_eq!(read_piece(&p, &m, 2).unwrap(), data[PIECE * 2..].to_vec());
    }
}
