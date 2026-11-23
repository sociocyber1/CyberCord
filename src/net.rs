//! The network engine: owns the libp2p swarm on a background thread and talks to
//! the GUI (or a headless loop) over channels.
//!
//! Network model (concept §4.3–4.4): **no servers**. Every CyberCord is a node.
//! On start a node *scans* for other nodes from every source that does not need a
//! server — the local network (mDNS), its saved peer address book, its friends,
//! peer exchange (nodes tell each other about nodes) and the Kademlia DHT. Nodes
//! behind a home router reach each other through *other users' nodes*: a
//! reachable node relays (Circuit Relay v2) and hole punching (DCUtR) then turns
//! the relayed link into a direct one. Only nodes that announced themselves are
//! ever dialed, and dialing is rate-limited — no address-range scanning.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use libp2p::gossipsub::{self, IdentTopic, TopicHash};
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::SwarmEvent;
use libp2p::{dcutr, identify, kad, mdns, relay, upnp, Multiaddr, PeerId, Swarm};

use base64::Engine as _;

use crate::types::{dm_topic, name_key, name_prefixes, now_ms, Attachment, ChannelId, ChatEvent, Friend, MediaKind, NodeInfo, PeerRec, ScoreRec, UserCard, VoiceFrame, Wire};
use crate::{build_swarm, servers, store, voice, CyberBehaviour, CyberBehaviourEvent, KAD_PROTOCOL, RELAY_HOP_PROTOCOL};

const CONTROL_TOPIC: &str = "cc/control";
const HISTORY_ON_OPEN: usize = 200;
const SYNC_REPLY_CAP: usize = 50;
/// Default port (concept: "one UDP port per peer, default 47100, random fallback").
const DEFAULT_PORT: u16 = 47100;
/// Keep about this many live connections; below it we dial more known nodes.
const TARGET_CONNECTIONS: usize = 8;
/// At most this many new dials per round (bounded probing).
const DIALS_PER_ROUND: usize = 5;
/// Use up to this many other nodes as relays when we are not reachable.
const MAX_RELAYS: usize = 2;
const MAX_BOOK: usize = 500;
/// Media travels in chunks of this size (raw bytes, before base64).
const CHUNK: usize = 192 * 1024;
/// Largest media file we send or accept (GIFs, images, video circles).
pub const MAX_BLOB: usize = 16 * 1024 * 1024;
const MAX_CHUNKS: u32 = (MAX_BLOB / CHUNK) as u32 + 1;
const MAX_ASSEMBLIES: usize = 8;

/// Commands from the UI to the network engine.
#[derive(Debug)]
pub enum Cmd {
    SendChat { channel: ChannelId, body: String },
    RequestSync { channel: ChannelId, since_ms: u64 },
    JoinVoice { channel: ChannelId },
    /// Make (proof of work, sign) and send a report.
    MakeReport { target: String, msg: Option<String>, channel: Option<ChannelId>, category: crate::safety::Category },
    /// Users this node restricts (by its own weighing of reports): it keeps no
    /// mail for them and does not resend their messages in history sync.
    SetRestricted(HashSet<String>),
    /// Keep (or stop keeping) offline mail for others.
    SetKeeper(bool),
    /// Send a call signal to a friend (DM topic).
    Call { peer: String, id: u64, kind: crate::types::CallKind, round: u32 },
    LeaveVoice,
    SetMic(bool),
    /// Save a friend (connect code or bare ID), find and dial them, open the DM room.
    AddFriend(Friend),
    SendDm { peer_id: String, body: String },
    /// Edit (`body = Some`) or delete (`None`) one of my own messages.
    /// `rev` is the new version index (2 for the first edit).
    EditMessage { channel: ChannelId, target: String, body: Option<String>, rev: u32 },
    /// Send media (GIF, image, video circle) to a channel or DM (`channel.is_dm()`).
    SendMedia {
        channel: ChannelId,
        body: String,
        kind: MediaKind,
        name: String,
        bytes: Vec<u8>,
        w: u32,
        h: u32,
        dur_ms: u32,
    },
    /// We show a message whose media we don't have: ask the room for it.
    FetchBlob { channel: ChannelId, hash: String },
    /// Connect to any known CyberCord node by address (e.g. from a friend's code).
    AddNode(String),
    /// People search by name or ID; answers with UiEvent::Search (and updates).
    Search(String),
    /// Be findable by name (or not).
    SetListed(bool),
    /// Look for new users or not.
    SetScan(bool),
    /// Status shown to others.
    SetStatus(String),
    /// Save and publish my profile (the node sets peer, name, version and signs it).
    SetProfile(crate::profile::Profile),
    /// Join (subscribe to) a channel that is not one of the built-in servers' — e.g. a city branch.
    Subscribe(ChannelId),
    /// Share a local file of any size in a room (hashing runs in the background).
    ShareFile { channel: ChannelId, path: PathBuf, body: String },
    /// Queue a download; it starts whenever someone who has the file is online.
    Download { hash: String, name: String, size: u64, channel: ChannelId },
    PauseDownload { hash: String, paused: bool },
    CancelDownload(String),
    /// Scan again now: saved nodes, friends, DHT.
    Rescan,
    Quit,
}

/// Live network state for the UI.
#[derive(Debug, Clone, Default)]
pub struct NetStats {
    /// Nodes in our address book.
    pub known: usize,
    /// Nodes we hold a live connection to.
    pub connected: usize,
    /// Other users' nodes currently relaying for us.
    pub relays: usize,
    /// Nodes we are relaying for (we help others connect).
    pub relaying_for: usize,
    /// Some(true) = reachable from the internet (public IP or router opened a port).
    pub reachable: Option<bool>,
    /// Still looking for the first nodes.
    pub scanning: bool,
    /// Where nodes were found, e.g. "LAN 2 · saved 5 · exchange 9 · DHT 1".
    pub sources: String,
}

/// Events from the network engine to the UI.
#[derive(Debug)]
pub enum UiEvent {
    Ready { peer_id: String, name: String },
    /// A known (verified) profile, mine included.
    Profile(crate::profile::Profile),
    Listening(String),
    Chat(ChatEvent),
    Peer { id: String, name: Option<String>, connected: bool },
    Hub { hub: Option<String>, standby: Option<String> },
    VoiceJoined { room: Option<String> },
    Talking { name: String },
    /// Voice counters for developer mode: frames sent / received since the
    /// last report (every 5 s), and the playback buffer in ms.
    VoiceStats { sent: u32, recv: u32, buffer_ms: u32 },
    /// This friend's encryption keys are known: direct messages are sealed end to end.
    E2e { peer: String },
    /// A verified report (also all stored ones at start).
    Report(crate::safety::Report),
    /// When this node first noticed each user (for weighing reports).
    FirstSeen(HashMap<String, u64>),
    /// Offline mail I keep for others: letters, for how many people.
    Mail { kept: usize, users: usize },
    /// A call signal from a friend.
    Call { peer: String, name: String, id: u64, kind: crate::types::CallKind },
    /// Someone's public voice room (from their heartbeat); None = not in voice.
    VoiceRoster { peer: String, name: String, room: Option<String> },
    Friend { peer_id: String, name: String, online: bool },
    Net(NetStats),
    /// Snapshot of every node we know, for the network map (every ~2 s).
    Nodes(Vec<NodeInfo>),
    /// People search results for `query`.
    Search { query: String, users: Vec<UserCard> },
    /// Downloads and shared files (every second while anything changes).
    Transfers(Vec<crate::transfer::TransferInfo>),
    /// Media download progress (chunks received / total).
    BlobProgress { hash: String, got: u32, total: u32 },
    /// Media is complete and verified on disk.
    BlobReady { hash: String },
    Status(String),
}

pub struct NetworkHandle {
    pub cmd_tx: tokio::sync::mpsc::UnboundedSender<Cmd>,
    pub event_rx: std::sync::mpsc::Receiver<UiEvent>,
}

/// How to start the engine.
#[derive(Debug, Clone, Default)]
pub struct NetConfig {
    pub name: String,
    pub identity: PathBuf,
    /// Addresses of known nodes to dial at start (any user's node; optional).
    pub bootstrap: Vec<String>,
    /// Disable local-network discovery (mDNS).
    pub no_mdns: bool,
    /// Listen ports. 0 = try the default 47100, fall back to a random port.
    pub tcp_port: u16,
    pub quic_port: u16,
    /// Let others find this user by name (people search).
    pub listed: bool,
    /// Use the public BitTorrent DHT as a global meeting point.
    pub global: bool,
    /// Actively look for new users (peer exchange, LAN, global DHT). Off: keep
    /// known nodes and friends only.
    pub scan: bool,
    /// User status shown to others ("", online, offline, work, sleep, …).
    pub status: String,
    /// Developer mode: this node is a voice test bot (no audio devices).
    pub voice_bot: Option<VoiceBot>,
    /// Keep sealed offline mail for other users (Settings → Privacy).
    pub keeper: bool,
}

/// Developer test bots for checking voice on one computer.
#[derive(Clone, Debug)]
pub enum VoiceBot {
    /// Repeats what it hears after a delay — you hear yourself through the
    /// whole network path (capture → gossip → another node → back → playback).
    Echo { delay_ms: u64 },
    /// Speaks a fixed 48 kHz mono clip in a loop (tone pattern, music).
    Play(Arc<Vec<i16>>),
}

pub fn spawn(cfg: NetConfig) -> NetworkHandle {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Cmd>();
    let (event_tx, event_rx) = std::sync::mpsc::channel::<UiEvent>();
    std::thread::Builder::new()
        .name("cybercord-net".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            if let Err(e) = rt.block_on(run(cfg, cmd_rx, event_tx.clone())) {
                let _ = event_tx.send(UiEvent::Status(format!("network stopped: {e}")));
            }
        })
        .expect("spawn net thread");
    NetworkHandle { cmd_tx, event_rx }
}

fn load_or_create_identity(path: &PathBuf) -> anyhow::Result<Keypair> {
    if path.exists() {
        Ok(Keypair::from_protobuf_encoding(&std::fs::read(path)?)?)
    } else {
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let kp = Keypair::generate_ed25519();
        std::fs::write(path, kp.to_protobuf_encoding()?)?;
        Ok(kp)
    }
}

fn stable_rand(peer: &str) -> f64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    peer.hash(&mut h);
    (h.finish() as f64) / (u64::MAX as f64)
}

fn emit(tx: &std::sync::mpsc::Sender<UiEvent>, ev: UiEvent) {
    let _ = tx.send(ev);
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).map(|v| v == "1" || v == "true").unwrap_or(false)
}

// ---------------------------------------------------------------- addresses

fn ip_of(ma: &Multiaddr) -> Option<std::net::IpAddr> {
    ma.iter().find_map(|p| match p {
        Protocol::Ip4(ip) => Some(std::net::IpAddr::V4(ip)),
        Protocol::Ip6(ip) => Some(std::net::IpAddr::V6(ip)),
        _ => None,
    })
}

fn is_circuit(ma: &Multiaddr) -> bool {
    ma.iter().any(|p| matches!(p, Protocol::P2pCircuit))
}

fn is_loopback(ma: &Multiaddr) -> bool {
    ip_of(ma).map(|ip| ip.is_loopback()).unwrap_or(false)
}

/// A globally routable unicast IP (not private, loopback, link-local, CGNAT, docs).
fn is_public(ma: &Multiaddr) -> bool {
    match ip_of(ma) {
        Some(std::net::IpAddr::V4(ip)) => {
            let o = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_documentation()
                || (o[0] == 100 && (o[1] & 0xC0) == 64) // 100.64/10 carrier-grade NAT
                || o[0] == 0)
        }
        Some(std::net::IpAddr::V6(ip)) => {
            let s0 = ip.segments()[0];
            !(ip.is_loopback() || ip.is_unspecified() || (s0 & 0xfe00) == 0xfc00 || (s0 & 0xffc0) == 0xfe80)
        }
        None => false,
    }
}

/// Worth telling other nodes about.
fn shareable(ma: &Multiaddr, allow_loopback: bool) -> bool {
    match ip_of(ma) {
        Some(ip) if ip.is_unspecified() => false,
        Some(_) if is_loopback(ma) => allow_loopback,
        Some(_) => true,
        None => false,
    }
}

/// Drop a trailing /p2p/<peer> so the address can be stored and re-used.
fn strip_p2p(ma: &Multiaddr, peer: &PeerId) -> Multiaddr {
    let mut m = ma.clone();
    if let Some(Protocol::P2p(p)) = m.iter().last() {
        if &p == peer {
            m.pop();
        }
    }
    m
}

fn with_p2p(ma: &Multiaddr, peer: &PeerId) -> Multiaddr {
    match ma.iter().last() {
        Some(Protocol::P2p(p)) if &p == peer => ma.clone(),
        _ => ma.clone().with(Protocol::P2p(*peer)),
    }
}

// ---------------------------------------------------------------- peer book

struct BookEntry {
    addrs: BTreeSet<Multiaddr>,
    last_seen: u64,
    /// How we first learned about this node.
    source: &'static str,
}

/// The peer address book: every node we have learned about, and how.
struct Book {
    local: PeerId,
    allow_loopback: bool,
    entries: HashMap<PeerId, BookEntry>,
    found_by: HashMap<&'static str, usize>,
}

impl Book {
    /// Learn addresses for a node. Returns true if the node was new to us.
    fn learn(&mut self, peer: PeerId, addrs: impl IntoIterator<Item = Multiaddr>, source: &'static str) -> bool {
        if peer == self.local {
            return false;
        }
        let allow = self.allow_loopback;
        let new = !self.entries.contains_key(&peer);
        let e = self.entries.entry(peer).or_insert_with(|| BookEntry { addrs: BTreeSet::new(), last_seen: 0, source });
        for a in addrs {
            let a = strip_p2p(&a, &peer);
            if shareable(&a, allow) && e.addrs.len() < 12 {
                e.addrs.insert(a);
            }
        }
        if new {
            *self.found_by.entry(source).or_default() += 1;
        }
        if self.entries.len() > MAX_BOOK {
            if let Some(oldest) = self.entries.iter().min_by_key(|(_, e)| e.last_seen).map(|(p, _)| *p) {
                self.entries.remove(&oldest);
            }
        }
        new
    }
    fn seen(&mut self, peer: &PeerId) {
        if let Some(e) = self.entries.get_mut(peer) {
            e.last_seen = now_ms();
        }
    }
    fn addrs(&self, peer: &PeerId) -> Vec<Multiaddr> {
        self.entries.get(peer).map(|e| e.addrs.iter().cloned().collect()).unwrap_or_default()
    }
    fn sources(&self) -> String {
        let order = [("lan", "LAN"), ("saved", "saved"), ("friend", "friends"), ("node", "invite"), ("global", "global DHT"), ("exchange", "exchange"), ("dht", "DHT"), ("search", "search"), ("identify", "direct")];
        order
            .iter()
            .filter_map(|(k, label)| self.found_by.get(k).filter(|n| **n > 0).map(|n| format!("{label} {n}")))
            .collect::<Vec<_>>()
            .join(" · ")
    }
    fn to_recs(&self) -> Vec<PeerRec> {
        self.entries
            .iter()
            .filter(|(_, e)| !e.addrs.is_empty())
            .map(|(p, e)| PeerRec {
                peer: p.to_base58(),
                addrs: e.addrs.iter().map(|a| a.to_string()).collect(),
                last_seen_ms: e.last_seen,
            })
            .collect()
    }
    /// Most recently seen first.
    fn ranked(&self) -> Vec<PeerId> {
        let mut v: Vec<(&PeerId, u64)> = self.entries.iter().map(|(p, e)| (p, e.last_seen)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.into_iter().map(|(p, _)| *p).collect()
    }
}

/// Results of background work (hashing a shared file, verifying a download).
enum Bg {
    Shared { channel: ChannelId, body: String, rec: crate::transfer::SharedRec, manifest: crate::transfer::Manifest },
    ShareFailed(String),
    Verified(String, bool),
    /// A report finished its proof of work.
    Report(crate::safety::Report),
}

fn spawn_verify(tx: tokio::sync::mpsc::UnboundedSender<Bg>, hash: String, path: PathBuf) {
    std::thread::spawn(move || {
        let ok = crate::transfer::hash_file(&path).map(|(h, _)| h == hash).unwrap_or(false);
        let _ = tx.send(Bg::Verified(hash, ok));
    });
}

/// Announce our display name in the DHT name index (one provider record per prefix).
fn provide_name(swarm: &mut Swarm<CyberBehaviour>, name: &str, provided: &mut Vec<kad::RecordKey>) {
    for k in provided.drain(..) {
        swarm.behaviour_mut().kad.stop_providing(&k);
    }
    // the handle tag: findable by "@name-xxxxxx" (shared on purpose by its owner)
    let me = swarm.local_peer_id().to_base58();
    let key = kad::RecordKey::new(&crate::types::handle_key(&crate::types::handle_tag(&me)));
    let _ = swarm.behaviour_mut().kad.start_providing(key);
    for p in name_prefixes(name) {
        let key = kad::RecordKey::new(&name_key(&p));
        if swarm.behaviour_mut().kad.start_providing(key.clone()).is_ok() {
            provided.push(key);
        }
    }
}

/// Bytes to publish for a chat event: a DM to a friend whose keys we know is
/// sealed end to end (and returned, for offline mail); anything else as is.
fn chat_payload(ev: &ChatEvent, local_id: &str, bundles: &HashMap<String, crate::e2e::KeyBundle>, history: Option<u64>) -> (Vec<u8>, Option<crate::e2e::Sealed>) {
    if ev.channel.is_dm() {
        if let (Some(b), Ok(plain)) = (bundles.get(&ev.channel.channel), serde_json::to_vec(ev)) {
            if let Some(s) = crate::e2e::seal(local_id, b, &ev.id, &plain) {
                return (Wire::Sealed(s.clone()).encode(), Some(s));
            }
        }
    }
    let w = match history {
        Some(nonce) => Wire::History { ev: ev.clone(), nonce },
        None => Wire::Chat(ev.clone()),
    };
    (w.encode(), None)
}

/// Open a sealed DM: it must be for me, and the signed inner event must be by its sender.
fn open_sealed(me: &crate::e2e::Secrets, local_id: &str, s: &crate::e2e::Sealed) -> Option<ChatEvent> {
    let plain = crate::e2e::open(me, local_id, s)?;
    let mut ev: ChatEvent = serde_json::from_slice(&plain).ok()?;
    if ev.from != s.from {
        return None;
    }
    ev.channel = ChannelId::dm(s.from.clone());
    Some(ev)
}

fn load_bundles() -> HashMap<String, crate::e2e::KeyBundle> {
    let list: Vec<crate::e2e::KeyBundle> = std::fs::read_to_string(store::data_dir().join("keys.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    list.into_iter().filter(|b| b.verify()).map(|b| (b.peer.clone(), b)).collect()
}

fn save_bundles(b: &HashMap<String, crate::e2e::KeyBundle>) {
    let list: Vec<&crate::e2e::KeyBundle> = b.values().collect();
    if let Ok(t) = serde_json::to_string(&list) {
        let _ = std::fs::write(store::data_dir().join("keys.json"), t);
    }
}

fn dial_peer(swarm: &mut Swarm<CyberBehaviour>, peer: PeerId, addrs: Vec<Multiaddr>) {
    let opts = DialOpts::peer_id(peer)
        .addresses(addrs)
        .extend_addresses_through_behaviour()
        .condition(PeerCondition::DisconnectedAndNotDialing)
        .build();
    let _ = swarm.dial(opts);
}

fn parse_peer(s: &str) -> Option<PeerId> {
    s.parse::<PeerId>().ok()
}

/// Gossip topic of a room: a guild channel or the symmetric DM pair topic.
fn topic_for(local_id: &str, ch: &ChannelId) -> IdentTopic {
    if ch.is_dm() {
        IdentTopic::new(dm_topic(local_id, &ch.channel))
    } else {
        IdentTopic::new(ch.topic())
    }
}

/// Publish a blob as chunks on a topic. `round` makes re-sends unique.
fn publish_blob(swarm: &mut Swarm<CyberBehaviour>, topic: &IdentTopic, hash: &str, bytes: &[u8]) -> usize {
    let total = bytes.len().div_ceil(CHUNK).max(1) as u32;
    let round: u64 = rand::random();
    let mut sent = 0;
    for (idx, chunk) in bytes.chunks(CHUNK).enumerate() {
        let data = base64::engine::general_purpose::STANDARD.encode(chunk);
        let w = Wire::BlobChunk { hash: hash.to_string(), idx: idx as u32, total, round, data };
        if swarm.behaviour_mut().gossipsub.publish(topic.clone(), w.encode()).is_ok() {
            sent += 1;
        }
    }
    sent
}

/// A blob being received chunk by chunk.
struct Assembly {
    total: u32,
    parts: Vec<Option<Vec<u8>>>,
    got: u32,
    started: Instant,
}

// ---------------------------------------------------------------- engine

async fn run(
    cfg: NetConfig,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<Cmd>,
    event_tx: std::sync::mpsc::Sender<UiEvent>,
) -> anyhow::Result<()> {
    // Test/diagnostic switches (environment):
    //   CYBERCORD_NO_LISTEN=1      act like a strict-NAT node (accept no direct connections)
    //   CYBERCORD_FORCE_RELAY=1    always ask reachable neighbours to relay for us
    //   CYBERCORD_ALLOW_LOOPBACK=1 share 127.0.0.1 addresses (single-machine tests)
    let no_listen = env_flag("CYBERCORD_NO_LISTEN");
    let force_relay = env_flag("CYBERCORD_FORCE_RELAY") || no_listen;
    let allow_loopback = env_flag("CYBERCORD_ALLOW_LOOPBACK");
    //   CYBERCORD_BOT_AS_HUMAN=1  a test bot's frames look like a person's (to test the echo bot)
    let bot_as_human = env_flag("CYBERCORD_BOT_AS_HUMAN");

    let name = cfg.name.clone();
    let keypair = load_or_create_identity(&cfg.identity)?;
    let local_peer = keypair.public().to_peer_id();
    let local_id = local_peer.to_base58();
    let mut swarm = build_swarm(keypair.clone())?;

    let mut topic_of_hash: HashMap<TopicHash, ChannelId> = HashMap::new();
    for ch in servers::all_channels() {
        let t = IdentTopic::new(ch.topic());
        swarm.behaviour_mut().gossipsub.subscribe(&t)?;
        topic_of_hash.insert(t.hash(), ch);
    }
    let control = IdentTopic::new(CONTROL_TOPIC);
    swarm.behaviour_mut().gossipsub.subscribe(&control)?;
    let control_hash = control.hash();

    // End-to-end encryption (hybrid X25519 + ML-KEM-768) and offline mail.
    let e2e = crate::e2e::Secrets::load_or_create();
    let my_keys = e2e.bundle(&keypair);
    let mut bundles = load_bundles();
    for p in bundles.keys() {
        emit(&event_tx, UiEvent::E2e { peer: p.clone() });
    }
    let mut mailbox = crate::mailbox::Mailbox::load();
    let mut keeper_on = cfg.keeper;
    let my_mail = IdentTopic::new(format!("cc/mail/{local_id}"));
    swarm.behaviour_mut().gossipsub.subscribe(&my_mail)?;
    let my_mail_hash = my_mail.hash();
    for r in mailbox.recipients() {
        let _ = swarm.behaviour_mut().kad.start_providing(kad::RecordKey::new(&crate::mailbox::key(&r)));
    }
    emit(&event_tx, UiEvent::Mail { kept: mailbox.total(), users: mailbox.items.len() });
    let mut mail_sent: HashMap<String, Instant> = HashMap::new();
    let mut mailbox_queries: HashSet<kad::QueryId> = HashSet::new();
    let mut mail_looked: Option<Instant> = None;
    // reports and when I first noticed each user
    let mut reports = crate::safety::Reports::load();
    for r in &reports.list {
        emit(&event_tx, UiEvent::Report(r.clone()));
    }
    let first_seen_path = store::data_dir().join("first_seen.json");
    let mut first_seen: HashMap<String, u64> = std::fs::read_to_string(&first_seen_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut first_seen_dirty = false;
    emit(&event_tx, UiEvent::FirstSeen(first_seen.clone()));
    let mut restricted: HashSet<String> = HashSet::new();
    // who keeps mail right now: peer → (score, heard at)
    let mut keepers: HashMap<String, (f64, u64)> = HashMap::new();

    // Listen: default port 47100, random fallback (e.g. second copy on one machine).
    if !no_listen {
        let tcp = if cfg.tcp_port == 0 { DEFAULT_PORT } else { cfg.tcp_port };
        let quic = if cfg.quic_port == 0 { DEFAULT_PORT } else { cfg.quic_port };
        if swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{tcp}").parse()?).is_err() {
            swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        }
        if swarm.listen_on(format!("/ip4/0.0.0.0/udp/{quic}/quic-v1").parse()?).is_err() {
            swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
        }
    }

    emit(&event_tx, UiEvent::Ready { peer_id: local_id.clone(), name: name.clone() });

    let mut book = Book { local: local_peer, allow_loopback, entries: HashMap::new(), found_by: HashMap::new() };
    let start = Instant::now();

    // ---- scan, step 1: saved nodes from earlier sessions ----
    let saved = store::load_peers();
    for rec in &saved {
        if let Some(pid) = parse_peer(&rec.peer) {
            let addrs: Vec<Multiaddr> = rec.addrs.iter().filter_map(|a| a.parse().ok()).collect();
            book.learn(pid, addrs.clone(), "saved");
            if let Some(e) = book.entries.get_mut(&pid) {
                e.last_seen = rec.last_seen_ms;
            }
            for a in addrs {
                swarm.behaviour_mut().kad.add_address(&pid, a);
            }
        }
    }

    // ---- scan, step 2: friends ----
    let mut friends: HashMap<String, Friend> = HashMap::new();
    let mut friend_online: HashMap<String, bool> = HashMap::new();
    for f in store::load_friends() {
        friend_online.insert(f.peer_id.clone(), false);
        let t = IdentTopic::new(dm_topic(&local_id, &f.peer_id));
        let _ = swarm.behaviour_mut().gossipsub.subscribe(&t);
        topic_of_hash.insert(t.hash(), ChannelId::dm(f.peer_id.clone()));
        if let Some(pid) = parse_peer(&f.peer_id) {
            book.learn(pid, f.addrs.iter().filter_map(|a| a.parse().ok()), "friend");
        }
        emit(&event_tx, UiEvent::Friend { peer_id: f.peer_id.clone(), name: f.name.clone(), online: false });
        friends.insert(f.peer_id.clone(), f);
    }

    // ---- scan, step 3: nodes given on the command line (any user's node) ----
    for a in &cfg.bootstrap {
        match a.parse::<Multiaddr>() {
            Ok(ma) => match ma.iter().last() {
                Some(Protocol::P2p(pid)) => {
                    book.learn(pid, [ma.clone()], "node");
                }
                _ => emit(&event_tx, UiEvent::Status(format!("node address must end with /p2p/<id>: {a}"))),
            },
            Err(e) => emit(&event_tx, UiEvent::Status(format!("bad node address {a}: {e}"))),
        }
    }

    // Dial what we know, most recently seen first (bounded).
    for pid in book.ranked().into_iter().take(25) {
        let addrs = book.addrs(&pid);
        dial_peer(&mut swarm, pid, addrs);
    }
    // Friends known only by ID: ask the DHT once we have neighbours.
    let mut pending_lookups: HashMap<kad::QueryId, PeerId> = HashMap::new();
    emit(
        &event_tx,
        UiEvent::Status(format!(
            "scanning for nodes: LAN{} · {} saved · {} friends · DHT + peer exchange",
            if cfg.no_mdns { " (off)" } else { "" },
            saved.len(),
            friends.len()
        )),
    );

    // Engine state.
    let mut tick_n: u64 = 0;
    let mut conns: usize = 0;
    let mut connected_peers: HashSet<PeerId> = HashSet::new();
    let mut ever_connected = false;
    let mut names: HashMap<String, String> = HashMap::new();
    let mut seen_ids: HashMap<ChannelId, HashSet<String>> = HashMap::new();
    // deletes seen this session (target id → author), to blank an original that arrives after its delete
    let mut deletes: HashMap<String, String> = HashMap::new();
    let mut scores: HashMap<String, (f64, u64)> = HashMap::new();
    let mut reachable_peers: HashSet<PeerId> = HashSet::new();
    let mut last_hub: (Option<String>, Option<String>) = (None, None);
    names.insert(local_id.clone(), name.clone());

    // Reachability + relays.
    let mut reachable: Option<bool> = None;
    let mut local_addrs: BTreeSet<Multiaddr> = BTreeSet::new();
    let mut dial_addr_of: HashMap<PeerId, Multiaddr> = HashMap::new();
    let mut relay_requested: HashSet<PeerId> = HashSet::new();
    let mut relays_ok: HashSet<PeerId> = HashSet::new();
    let mut relaying_for: HashSet<PeerId> = HashSet::new();
    let mut last_stats = String::new();

    // Media transfer state.
    let mut incoming: HashMap<String, Assembly> = HashMap::new();
    let mut wanted: HashMap<String, (ChannelId, Instant, u8)> = HashMap::new();
    // Accept a (verified) chat event: store it, fetch its media, show it.
    macro_rules! accept_chat {
        ($ev:expr, $author:expr) => {{
            let ev: ChatEvent = $ev;
            let set = seen_ids.entry(ev.channel.clone()).or_insert_with(|| store::load_ids(&ev.channel));
            if set.insert(ev.id.clone()) {
                if !ev.name.is_empty() {
                    names.entry($author).or_insert(ev.name.clone());
                }
                if let Some(a) = &ev.attach {
                    if a.kind != MediaKind::File && !store::has_blob(&a.hash) && a.size as usize <= MAX_BLOB {
                        wanted.entry(a.hash.clone()).or_insert((ev.channel.clone(), Instant::now(), 0));
                    }
                }
                store::append(&ev);
                if let (true, Some(target)) = (ev.deleted, ev.edits.as_deref()) {
                    store::redact(&ev.channel, target, &ev.from);
                    deletes.insert(target.to_string(), ev.from.clone());
                } else if ev.edits.is_some() || deletes.contains_key(&ev.id) {
                    if let Some(author) = deletes.get(ev.edits.as_deref().unwrap_or(&ev.id)) {
                        store::redact(&ev.channel, ev.edits.as_deref().unwrap_or(&ev.id), author);
                    }
                }
                emit(&event_tx, UiEvent::Chat(ev));
            }
        }};
    }
    // Send a chat event: sealed for a DM friend whose keys are known; if the
    // friend is offline, the sealed letter also goes to up to 3 keepers.
    macro_rules! publish_chat {
        ($topic:expr, $ev:expr) => {{
            let ev: &ChatEvent = $ev;
            let (bytes, sealed) = chat_payload(ev, &local_id, &bundles, None);
            let r = swarm.behaviour_mut().gossipsub.publish($topic, bytes);
            if let Some(s) = sealed {
                let friend_on = parse_peer(&ev.channel.channel).map(|p| connected_peers.contains(&p)).unwrap_or(false);
                if !friend_on {
                    let now = now_ms();
                    let mut ks: Vec<(&String, &(f64, u64))> = keepers.iter().filter(|(k, (_, t))| **k != ev.channel.channel && now.saturating_sub(*t) < 30_000 && parse_peer(k).map(|p| connected_peers.contains(&p)).unwrap_or(false)).collect();
                    ks.sort_by(|a, b| b.1 .0.partial_cmp(&a.1 .0).unwrap_or(std::cmp::Ordering::Equal));
                    for (k, _) in ks.into_iter().take(3) {
                        let _ = swarm.behaviour_mut().gossipsub.publish(IdentTopic::new(format!("cc/mail/{k}")), Wire::MailPut { item: s.clone() }.encode());
                    }
                }
            }
            r
        }};
    }
    // Rooms the user has opened: when a new peer subscribes to one of them we ask
    // it for what we missed (the request can only reach peers known to be in the room).
    let mut opened: HashSet<ChannelId> = HashSet::new();
    let mut synced_with: HashMap<ChannelId, HashSet<PeerId>> = HashMap::new();
    let mut last_served: HashMap<String, Instant> = HashMap::new();
    let mut served_to: HashMap<(String, PeerId), Instant> = HashMap::new();

    // Voice state.
    let mic = Arc::new(AtomicBool::new(false));
    let (voice_cap_tx, mut voice_cap_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<i16>>();
    let mut voice_io: Option<voice::VoiceIo> = None;
    let mut voice_room: Option<ChannelId> = None;
    let mut voice_seq: u32 = 0;
    let voice_bot = cfg.voice_bot.clone();
    let mut bot_iv = tokio::time::interval(Duration::from_millis(if voice_bot.is_some() { 20 } else { 3_600_000 }));
    let mut echo_q: VecDeque<(Instant, Vec<i16>)> = VecDeque::new();
    let mut play_pos: usize = 0;
    let mut play_pause: u32 = 0;
    let (mut v_sent, mut v_recv) = (0u32, 0u32);
    let mut last_score: f64 = 0.0;

    // Map + people directory.
    let mut link_of: HashMap<PeerId, &'static str> = HashMap::new();
    let mut dir: HashMap<String, (String, u64, bool)> = HashMap::new(); // peer -> (name, last heartbeat, listed)
    let mut statuses: HashMap<String, String> = HashMap::new(); // peer -> status
    for r in store::load_directory() {
        dir.insert(r.peer, (r.name, r.last_seen_ms, true));
    }
    let mut listed = cfg.listed;
    let mut scan = cfg.scan;
    let mut status = cfg.status.clone();
    let mut provided: Vec<kad::RecordKey> = Vec::new();
    let mut search: Option<(String, Instant)> = None;
    let mut search_sig = String::new();
    let mut dht_found: HashSet<String> = HashSet::new();
    let mut search_queries: HashMap<kad::QueryId, String> = HashMap::new();
    let mut last_nodes: Vec<NodeInfo> = Vec::new();
    let mut nodes_tick = tokio::time::interval(Duration::from_millis(1500));

    // Global rendezvous through the BitTorrent DHT (no servers of our own).
    let rv = crate::rendezvous::Control::default();
    let (rv_tx, mut rv_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<std::net::SocketAddrV4>>();
    if cfg.global && std::env::var("CYBERCORD_NO_GLOBAL").is_err() {
        crate::rendezvous::spawn(rv.clone(), rv_tx);
    } else {
        drop(rv_tx);
    }
    let mut global_dialed: HashMap<std::net::SocketAddrV4, Instant> = HashMap::new();

    // Torrent-like transfers.
    let mut transfers = crate::transfer::Transfers::load();
    let mut req_ids: HashMap<libp2p::request_response::OutboundRequestId, u64> = HashMap::new();
    let mut next_req: u64 = 1;
    let mut file_providers: HashMap<String, HashSet<PeerId>> = HashMap::new();
    let mut provider_queries: HashMap<kad::QueryId, String> = HashMap::new();
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel::<Bg>();
    let mut last_transfers: Vec<crate::transfer::TransferInfo> = Vec::new();
    let mut files_tick = tokio::time::interval(Duration::from_millis(250));
    let mut seeding_announced = false;
    let mut global_addrs: HashSet<Multiaddr> = HashSet::new();

    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    // profiles: everyone's known, signed profiles (mine included)
    let mut profiles: HashMap<String, crate::profile::Profile> = crate::profile::load_all().into_iter().filter(|p| p.verify()).map(|p| (p.peer.clone(), p)).collect();
    for p in profiles.values() {
        emit(&event_tx, UiEvent::Profile(p.clone()));
    }
    let mut profile_due = true;

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break; };
                match cmd {
                    Cmd::Quit => break,
                    Cmd::SendChat { channel, body } => {
                        let mut ev = ChatEvent::new(channel.clone(), name.clone(), body);
                        ev.sign(&keypair);
                        seen_ids.entry(channel.clone()).or_default().insert(ev.id.clone());
                        store::append(&ev);
                        emit(&event_tx, UiEvent::Chat(ev.clone()));
                        let topic = IdentTopic::new(channel.topic());
                        if let Err(e) = publish_chat!(topic, &ev) {
                            emit(&event_tx, UiEvent::Status(format!("saved; will sync when nodes are connected ({e})")));
                        }
                    }
                    Cmd::RequestSync { channel, since_ms } => {
                        opened.insert(channel.clone());
                        let topic = if channel.is_dm() {
                            IdentTopic::new(dm_topic(&local_id, &channel.channel))
                        } else {
                            IdentTopic::new(channel.topic())
                        };
                        let _ = swarm.behaviour_mut().gossipsub.publish(topic, Wire::SyncReq { channel, since_ms, nonce: rand::random() }.encode());
                    }
                    Cmd::JoinVoice { channel } => {
                        if let Some(prev) = voice_room.take() {
                            let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&IdentTopic::new(voice_topic_for(&local_id, &prev)));
                        }
                        let vt = IdentTopic::new(voice_topic_for(&local_id, &channel));
                        let _ = swarm.behaviour_mut().gossipsub.subscribe(&vt);
                        voice_io = Some(if voice_bot.is_some() { voice::start_silent() } else { voice::start(mic.clone(), voice_cap_tx.clone()) });
                        voice_room = Some(channel.clone());
                        mic.store(false, Ordering::Relaxed);
                        emit(&event_tx, UiEvent::VoiceJoined { room: Some(format!("{}/{}", channel.guild, channel.channel)) });
                        let rec = ScoreRec { peer: local_id.clone(), name: name.clone(), score: last_score, ts_ms: now_ms(), reachable: reachable == Some(true), voice: public_room(&voice_room), listed, status: status.clone(), keeper: keeper_on && voice_room.is_none() };
                        let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Score(rec).encode());
                    }
                    Cmd::MakeReport { target, msg, channel, category } => {
                        let (kp, tx) = (keypair.clone(), bg_tx.clone());
                        std::thread::spawn(move || {
                            let r = crate::safety::Report::create(&kp, &target, msg, channel, category);
                            let _ = tx.send(Bg::Report(r));
                        });
                    }
                    Cmd::SetRestricted(set) => restricted = set,
                    Cmd::SetKeeper(on) => {
                        keeper_on = on;
                    }
                    Cmd::Call { peer, id, kind, round } => {
                        let t = IdentTopic::new(dm_topic(&local_id, &peer));
                        let _ = swarm.behaviour_mut().gossipsub.publish(t, Wire::Call { id, kind, name: name.clone(), round }.encode());
                    }
                    Cmd::LeaveVoice => {
                        if let Some(prev) = voice_room.take() {
                            let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&IdentTopic::new(voice_topic_for(&local_id, &prev)));
                        }
                        mic.store(false, Ordering::Relaxed);
                        voice_io = None;
                        emit(&event_tx, UiEvent::VoiceJoined { room: None });
                        let rec = ScoreRec { peer: local_id.clone(), name: name.clone(), score: last_score, ts_ms: now_ms(), reachable: reachable == Some(true), voice: public_room(&voice_room), listed, status: status.clone(), keeper: keeper_on && voice_room.is_none() };
                        let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Score(rec).encode());
                    }
                    Cmd::SetMic(on) => mic.store(on, Ordering::Relaxed),
                    Cmd::AddFriend(f) => {
                        if f.peer_id == local_id { continue; }
                        store::save_friend(&f);
                        let t = IdentTopic::new(dm_topic(&local_id, &f.peer_id));
                        let _ = swarm.behaviour_mut().gossipsub.subscribe(&t);
                        topic_of_hash.insert(t.hash(), ChannelId::dm(f.peer_id.clone()));
                        if let Some(pid) = parse_peer(&f.peer_id) {
                            book.learn(pid, f.addrs.iter().filter_map(|a| a.parse().ok()), "friend");
                            let addrs = book.addrs(&pid);
                            if !connected_peers.contains(&pid) {
                                dial_peer(&mut swarm, pid, addrs);
                                // Also look them up in the DHT through the nodes we know.
                                let qid = swarm.behaviour_mut().kad.get_closest_peers(pid);
                                pending_lookups.insert(qid, pid);
                                emit(&event_tx, UiEvent::Status(format!("looking for {} through the network…", f.name)));
                            }
                        }
                        friend_online.remove(&f.peer_id);
                        friends.insert(f.peer_id.clone(), f);
                        update_presence(&friends, &connected_peers, &scores, &mut friend_online, &event_tx);
                    }
                    Cmd::AddNode(addr) => {
                        match addr.trim().parse::<Multiaddr>() {
                            Ok(ma) => match ma.iter().last() {
                                Some(Protocol::P2p(pid)) => {
                                    book.learn(pid, [ma.clone()], "node");
                                    dial_peer(&mut swarm, pid, book.addrs(&pid));
                                    emit(&event_tx, UiEvent::Status(format!("connecting to node {}", crate::short_peer(&pid))));
                                }
                                _ => emit(&event_tx, UiEvent::Status("node address must end with /p2p/<id>".into())),
                            },
                            Err(e) => emit(&event_tx, UiEvent::Status(format!("bad node address: {e}"))),
                        }
                    }
                    Cmd::Search(q) => {
                        let q = q.trim().to_string();
                        if q.is_empty() {
                            search = None;
                        } else {
                            // A handle: ask who provides its tag (users anywhere, not just here).
                            if let Some(tag) = crate::types::parse_handle(&q) {
                                let id = swarm.behaviour_mut().kad.get_providers(kad::RecordKey::new(&crate::types::handle_key(&tag)));
                                search_queries.insert(id, q.to_lowercase());
                            } else if q.chars().count() >= 3 && parse_peer(&q).is_none() {
                                // Ask the DHT name index too (finds users our gossip hasn't reached).
                                let id = swarm.behaviour_mut().kad.get_providers(kad::RecordKey::new(&name_key(&q)));
                                search_queries.insert(id, q.to_lowercase());
                            }
                            search = Some((q, Instant::now()));
                            search_sig.clear();
                        }
                    }
                    Cmd::SetScan(on) => {
                        scan = on;
                        emit(&event_tx, UiEvent::Status(if on { "looking for new users".into() } else { "not looking for new users (known nodes and friends only)".into() }));
                    }
                    Cmd::Subscribe(ch) => {
                        let t = IdentTopic::new(ch.topic());
                        if !topic_of_hash.contains_key(&t.hash()) {
                            let _ = swarm.behaviour_mut().gossipsub.subscribe(&t);
                            topic_of_hash.insert(t.hash(), ch);
                        }
                    }
                    Cmd::SetProfile(mut p) => {
                        let old_rev = profiles.get(&local_id).map(|o| o.rev).unwrap_or(0);
                        p.rev = old_rev + 1;
                        p.ts_ms = now_ms();
                        p.name = name.clone();
                        p.sign(&keypair);
                        profiles.insert(local_id.clone(), p.clone());
                        crate::profile::save_all(&profiles.values().cloned().collect::<Vec<_>>());
                        emit(&event_tx, UiEvent::Profile(p.clone()));
                        let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Profile(p).encode());
                    }
                    Cmd::SetStatus(st) => {
                        status = st;
                        let rec = ScoreRec { peer: local_id.clone(), name: name.clone(), score: last_score, ts_ms: now_ms(), reachable: reachable == Some(true), voice: public_room(&voice_room), listed, status: status.clone(), keeper: keeper_on && voice_room.is_none() };
                        let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Score(rec).encode());
                    }
                    Cmd::ShareFile { channel, path, body } => {
                        let tx = bg_tx.clone();
                        std::thread::spawn(move || {
                            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            let msg = if size > crate::transfer::MAX_FILE {
                                Bg::ShareFailed(format!("file is too big (max {} GB)", crate::transfer::MAX_FILE >> 30))
                            } else {
                                match crate::transfer::hash_file(&path) {
                                    Ok((hash, manifest)) => {
                                        let name = crate::transfer::safe_name(&path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                                        Bg::Shared { channel, body, rec: crate::transfer::SharedRec { hash, name, size, path }, manifest }
                                    }
                                    Err(e) => Bg::ShareFailed(e.to_string()),
                                }
                            };
                            let _ = tx.send(msg);
                        });
                    }
                    Cmd::Download { hash, name: fname, size, channel } => {
                        match transfers.add_download(&hash, &fname, size, channel) {
                            Ok(()) => emit(&event_tx, UiEvent::Status(format!("{fname}: queued — downloads piece by piece whenever someone who has it is online"))),
                            Err(e) => emit(&event_tx, UiEvent::Status(format!("{fname}: {e}"))),
                        }
                    }
                    Cmd::PauseDownload { hash, paused } => transfers.set_paused(&hash, paused),
                    Cmd::CancelDownload(hash) => transfers.cancel(&hash),
                    Cmd::SetListed(on) => {
                        listed = on;
                        if !on {
                            for k in provided.drain(..) {
                                swarm.behaviour_mut().kad.stop_providing(&k);
                            }
                        } else {
                            provide_name(&mut swarm, &name, &mut provided);
                        }
                    }
                    Cmd::Rescan => {
                        rv.poke.store(true, Ordering::Relaxed);
                        let mut n = 0;
                        for pid in book.ranked() {
                            if n >= 30 { break; }
                            if !connected_peers.contains(&pid) {
                                let addrs = book.addrs(&pid);
                                dial_peer(&mut swarm, pid, addrs);
                                n += 1;
                            }
                        }
                        let _ = swarm.behaviour_mut().kad.bootstrap();
                        emit(&event_tx, UiEvent::Status(format!("scanning: dialing {n} known nodes + DHT")));
                    }
                    Cmd::SendMedia { channel, body, kind, name: fname, bytes, w, h, dur_ms } => {
                        if bytes.len() > MAX_BLOB {
                            emit(&event_tx, UiEvent::Status(format!("file is too big ({} MB, max {} MB)", bytes.len() / 1_048_576, MAX_BLOB / 1_048_576)));
                            continue;
                        }
                        let hash = store::save_blob(&bytes);
                        let mut ev = ChatEvent::new(channel.clone(), name.clone(), body);
                        ev.attach = Some(Attachment { hash: hash.clone(), kind, name: fname, size: bytes.len() as u64, w, h, dur_ms });
                        ev.sign(&keypair);
                        seen_ids.entry(channel.clone()).or_default().insert(ev.id.clone());
                        store::append(&ev);
                        emit(&event_tx, UiEvent::Chat(ev.clone()));
                        emit(&event_tx, UiEvent::BlobReady { hash: hash.clone() });
                        let topic = topic_for(&local_id, &channel);
                        let _ = publish_chat!(topic.clone(), &ev);
                        let n = publish_blob(&mut swarm, &topic, &hash, &bytes);
                        last_served.insert(hash, Instant::now());
                        if n == 0 {
                            emit(&event_tx, UiEvent::Status("media saved; it is delivered when someone in the room is online".into()));
                        }
                    }
                    Cmd::FetchBlob { channel, hash } => {
                        if !store::has_blob(&hash) && !wanted.contains_key(&hash) {
                            // Ask right away; the tick retries a few times.
                            let topic = topic_for(&local_id, &channel);
                            let _ = swarm.behaviour_mut().gossipsub.publish(topic, Wire::BlobReq { hash: hash.clone(), nonce: rand::random() }.encode());
                            wanted.insert(hash, (channel, Instant::now(), 1));
                        }
                    }
                    Cmd::EditMessage { channel, target, body, rev } => {
                        let mut ev = ChatEvent::new(channel.clone(), name.clone(), body.clone().unwrap_or_default());
                        ev.edits = Some(target.clone());
                        ev.rev = rev.max(2);
                        ev.deleted = body.is_none();
                        ev.sign(&keypair);
                        seen_ids.entry(channel.clone()).or_default().insert(ev.id.clone());
                        store::append(&ev);
                        if ev.deleted {
                            store::redact(&channel, &target, &local_id);
                        }
                        emit(&event_tx, UiEvent::Chat(ev.clone()));
                        let _ = publish_chat!(topic_for(&local_id, &channel), &ev);
                    }
                    Cmd::SendDm { peer_id, body } => {
                        let ch = ChannelId::dm(peer_id.clone());
                        let mut ev = ChatEvent::new(ch.clone(), name.clone(), body);
                        ev.sign(&keypair);
                        seen_ids.entry(ch).or_default().insert(ev.id.clone());
                        store::append(&ev);
                        emit(&event_tx, UiEvent::Chat(ev.clone()));
                        let t = IdentTopic::new(dm_topic(&local_id, &peer_id));
                        if let Err(e) = publish_chat!(t, &ev) {
                            emit(&event_tx, UiEvent::Status(format!("DM saved; it syncs when your friend is reachable ({e})")));
                        }
                    }
                }
            }

            Some(frame) = voice_cap_rx.recv() => {
                if mic.load(Ordering::Relaxed) {
                    if let Some(room) = &voice_room {
                        let vf = VoiceFrame { room: room.clone(), name: name.clone(), seq: voice_seq, pcm: frame, bot: false };
                        voice_seq = voice_seq.wrapping_add(1);
                        let topic = IdentTopic::new(voice_topic_for(&local_id, room));
                        if swarm.behaviour_mut().gossipsub.publish(topic, Wire::Voice(vf).encode()).is_ok() {
                            v_sent += 1;
                        }
                    }
                }
            }

            _ = bot_iv.tick(), if voice_bot.is_some() => {
                // a developer test bot speaks one 20 ms frame per tick
                let Some(room) = voice_room.clone() else { continue };
                let frame: Option<Vec<i16>> = match &voice_bot {
                    Some(VoiceBot::Echo { .. }) => match echo_q.front() {
                        Some((due, _)) if *due <= Instant::now() => echo_q.pop_front().map(|(_, f)| f),
                        _ => None,
                    },
                    Some(VoiceBot::Play(clip)) if !clip.is_empty() => {
                        if play_pause > 0 {
                            play_pause -= 1;
                            None
                        } else {
                            let end = (play_pos + voice::FRAME_SAMPLES).min(clip.len());
                            let f = clip[play_pos..end].to_vec();
                            play_pos = end;
                            if play_pos >= clip.len() {
                                play_pos = 0;
                                play_pause = 50; // 1 s of silence between loops
                            }
                            Some(f)
                        }
                    }
                    _ => None,
                };
                if let Some(pcm) = frame {
                    let vf = VoiceFrame { room: room.clone(), name: name.clone(), seq: voice_seq, pcm, bot: !bot_as_human };
                    voice_seq = voice_seq.wrapping_add(1);
                    let topic = IdentTopic::new(voice_topic_for(&local_id, &room));
                    if swarm.behaviour_mut().gossipsub.publish(topic, Wire::Voice(vf).encode()).is_ok() {
                        v_sent += 1;
                    }
                }
            }

            _ = ticker.tick() => {
                tick_n += 1;
                if first_seen_dirty && tick_n % 12 == 0 {
                    first_seen_dirty = false;
                    if let Ok(t) = serde_json::to_string(&first_seen) {
                        let _ = std::fs::write(&first_seen_path, t);
                    }
                    emit(&event_tx, UiEvent::FirstSeen(first_seen.clone()));
                }
                if (profile_due || tick_n % 24 == 0) && !connected_peers.is_empty() {
                    let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Keys(my_keys.clone()).encode());
                }
                // a keeper hands letters to recipients who are here now
                if !mailbox.items.is_empty() {
                    mailbox.prune(now_ms());
                    for r in mailbox.recipients() {
                        let here = parse_peer(&r).map(|p| connected_peers.contains(&p)).unwrap_or(false);
                        if here && mail_sent.get(&r).map(|t| t.elapsed().as_secs() >= 20).unwrap_or(true) {
                            for chunk in mailbox.for_recipient(&r).chunks(40) {
                                let _ = swarm.behaviour_mut().gossipsub.publish(IdentTopic::new(format!("cc/mail/{r}")), Wire::MailDrop { items: chunk.to_vec() }.encode());
                            }
                            mail_sent.insert(r, Instant::now());
                        }
                    }
                }
                // look for keepers holding mail for me (soon after start, then every 10 min)
                if !connected_peers.is_empty() && mail_looked.map(|t| t.elapsed().as_secs() >= 600).unwrap_or(tick_n >= 2) {
                    mail_looked = Some(Instant::now());
                    let id = swarm.behaviour_mut().kad.get_providers(kad::RecordKey::new(&crate::mailbox::key(&local_id)));
                    mailbox_queries.insert(id);
                }
                if voice_room.is_some() {
                    let buffer_ms = voice_io.as_ref().and_then(|io| io.playback.lock().ok().map(|b| (b.len() as u64 * 1000 / voice::SAMPLE_RATE as u64) as u32)).unwrap_or(0);
                    emit(&event_tx, UiEvent::VoiceStats { sent: v_sent, recv: v_recv, buffer_ms });
                }
                v_sent = 0;
                v_recv = 0;
                // re-announce my profile now and then (and soon after new peers connect)
                if (profile_due || tick_n % 24 == 0) && !connected_peers.is_empty() {
                    if let Some(p) = profiles.get(&local_id) {
                        let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Profile(p.clone()).encode());
                    }
                    profile_due = false;
                }
                // History sync: for every room the user opened, ask each newly seen
                // member of that room once (covers any order of connect/subscribe/open).
                for ch in opened.iter() {
                    let topic = if ch.is_dm() { IdentTopic::new(dm_topic(&local_id, &ch.channel)) } else { IdentTopic::new(ch.topic()) };
                    let h = topic.hash();
                    let members: Vec<PeerId> = swarm.behaviour().gossipsub.all_peers().filter(|(_, ts)| ts.contains(&&h)).map(|(p, _)| *p).collect();
                    let done = synced_with.entry(ch.clone()).or_default();
                    if members.iter().any(|p| !done.contains(p)) {
                        done.extend(members);
                        let since = store::load_recent(ch, 1).last().map(|e| e.ts_ms).unwrap_or(0);
                        let _ = swarm.behaviour_mut().gossipsub.publish(topic, Wire::SyncReq { channel: ch.clone(), since_ms: since, nonce: rand::random() }.encode());
                    }
                }
                update_presence(&friends, &connected_peers, &scores, &mut friend_online, &event_tx);

                // Hub-election score; reachable nodes are preferred (concept §4.6.1).
                let am_reachable = reachable == Some(true);
                let uptime_n = ((start.elapsed().as_secs() as f64) / 1800.0).min(1.0);
                let conns_n = ((conns as f64) / 8.0).min(1.0);
                let score = 0.35 * (am_reachable as u8 as f64) + 0.3 * uptime_n + 0.25 * conns_n + 0.1 * stable_rand(&local_id);
                scores.insert(local_id.clone(), (score, now_ms()));
                last_score = score;
                let rec = ScoreRec { peer: local_id.clone(), name: name.clone(), score, ts_ms: now_ms(), reachable: am_reachable, voice: public_room(&voice_room), listed, status: status.clone(), keeper: keeper_on && voice_room.is_none() };
                let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Score(rec).encode());
                recompute_hub(&scores, &mut last_hub, &event_tx);

                if tick_n % 3 == 0 {
                    // Peer exchange: tell neighbours about the nodes we know + ourselves.
                    let mut recs: Vec<PeerRec> = Vec::new();
                    let me: Vec<String> = local_addrs.iter()
                        .chain(swarm.external_addresses())
                        .filter(|a| shareable(a, allow_loopback))
                        .map(|a| with_p2p(a, &local_peer).to_string())
                        .collect();
                    if !me.is_empty() {
                        recs.push(PeerRec { peer: local_id.clone(), addrs: me, last_seen_ms: now_ms() });
                    }
                    let mut others = book.to_recs();
                    others.sort_by(|a, b| b.last_seen_ms.cmp(&a.last_seen_ms));
                    recs.extend(others.into_iter().take(24));
                    if recs.len() > 0 {
                        let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Peers(recs).encode());
                    }
                    store::save_peers(&book.to_recs());

                    // Friends we can't see: dial again and ask the DHT.
                    for f in friends.values() {
                        if let Some(pid) = parse_peer(&f.peer_id) {
                            if !connected_peers.contains(&pid) {
                                let addrs = book.addrs(&pid);
                                dial_peer(&mut swarm, pid, addrs);
                                if !connected_peers.is_empty() && !pending_lookups.values().any(|p| *p == pid) {
                                    let qid = swarm.behaviour_mut().kad.get_closest_peers(pid);
                                    pending_lookups.insert(qid, pid);
                                }
                            }
                        }
                    }

                    // Too few neighbours: dial a few more known nodes (bounded).
                    if connected_peers.len() < TARGET_CONNECTIONS {
                        let mut n = 0;
                        for pid in book.ranked() {
                            if n >= DIALS_PER_ROUND { break; }
                            if !connected_peers.contains(&pid) {
                                let addrs = book.addrs(&pid);
                                dial_peer(&mut swarm, pid, addrs);
                                n += 1;
                            }
                        }
                    }
                }
                // Media we still miss: ask the room again (up to 6 times).
                let mut ask: Vec<(String, ChannelId)> = Vec::new();
                wanted.retain(|h, (ch, at, tries)| {
                    if store::has_blob(h) || *tries >= 6 { return false; }
                    let stalled = incoming.get(h).map(|a| a.started.elapsed() > Duration::from_secs(20)).unwrap_or(true);
                    if stalled && at.elapsed() > Duration::from_secs(6) {
                        *at = Instant::now();
                        *tries += 1;
                        ask.push((h.clone(), ch.clone()));
                    }
                    true
                });
                for (h, ch) in ask {
                    let topic = topic_for(&local_id, &ch);
                    let _ = swarm.behaviour_mut().gossipsub.publish(topic, Wire::BlobReq { hash: h, nonce: rand::random() }.encode());
                }
                if tick_n % 12 == 0 && !connected_peers.is_empty() {
                    let _ = swarm.behaviour_mut().kad.bootstrap();
                }
                // Global rendezvous: announce only when others can reach us.
                let quic_port = local_addrs.iter().find_map(|a| {
                    let s = a.to_string();
                    s.contains("/quic-v1").then(|| s.split("/udp/").nth(1).and_then(|r| r.split('/').next()).and_then(|p| p.parse::<u16>().ok())).flatten()
                });
                rv.port.store(quic_port.unwrap_or(0), Ordering::Relaxed);
                rv.announce.store(reachable == Some(true) || (allow_loopback && std::env::var("CYBERCORD_MAINLINE_BOOTSTRAP").is_ok()), Ordering::Relaxed);

                // Name index for people search: (re)announce once connected, then every 30 min.
                if listed && !connected_peers.is_empty() && (provided.is_empty() || tick_n % 360 == 0) {
                    provide_name(&mut swarm, &name, &mut provided);
                }
                if tick_n % 12 == 0 {
                    let mut recs: Vec<store::DirRec> = dir
                        .iter()
                        .filter(|(_, (_, _, l))| *l)
                        .map(|(p, (n, t, _))| store::DirRec { peer: p.clone(), name: n.clone(), last_seen_ms: *t })
                        .collect();
                    recs.sort_by(|a, b| b.last_seen_ms.cmp(&a.last_seen_ms));
                    recs.truncate(5000);
                    store::save_directory(&recs);
                }

                let stats = NetStats {
                    known: book.entries.len(),
                    connected: connected_peers.len(),
                    relays: relays_ok.len(),
                    relaying_for: relaying_for.len(),
                    reachable,
                    scanning: connected_peers.is_empty(),
                    sources: book.sources(),
                };
                let sig = format!("{stats:?}");
                if sig != last_stats {
                    last_stats = sig;
                    emit(&event_tx, UiEvent::Net(stats));
                }
            }

            Some(found) = rv_rx.recv() => {
                // Addresses that announced themselves under the CyberCord key.
                let mut n = 0;
                if !scan { continue; }
                for sa in found {
                    if n >= 16 { break; }
                    let own = local_addrs.iter().any(|a| a.to_string().contains(&format!("/udp/{}/", sa.port())) && (sa.ip().is_loopback() || a.to_string().contains(&sa.ip().to_string())));
                    let recent = global_dialed.get(&sa).map(|t| t.elapsed() < Duration::from_secs(3600)).unwrap_or(false);
                    if own || recent || (sa.ip().is_loopback() && !allow_loopback) || sa.port() == 0 {
                        continue;
                    }
                    global_dialed.insert(sa, Instant::now());
                    let addr: Multiaddr = format!("/ip4/{}/udp/{}/quic-v1", sa.ip(), sa.port()).parse().expect("valid multiaddr");
                    global_addrs.insert(addr.clone());
                    let _ = swarm.dial(addr);
                    n += 1;
                }
                if n > 0 {
                    emit(&event_tx, UiEvent::Status(format!("global DHT: trying {n} announced CyberCord nodes")));
                }
            }

            Some(bg) = bg_rx.recv() => {
                match bg {
                    Bg::Shared { channel, body, rec, manifest } => {
                        let (hash, fname, size) = (rec.hash.clone(), rec.name.clone(), rec.size);
                        transfers.add_shared(rec, manifest);
                        let _ = swarm.behaviour_mut().kad.start_providing(kad::RecordKey::new(&crate::transfer::provider_key(&hash)));
                        let mut ev = ChatEvent::new(channel.clone(), name.clone(), body);
                        ev.attach = Some(Attachment { hash, kind: MediaKind::File, name: fname, size, w: 0, h: 0, dur_ms: 0 });
                        ev.sign(&keypair);
                        seen_ids.entry(channel.clone()).or_default().insert(ev.id.clone());
                        store::append(&ev);
                        emit(&event_tx, UiEvent::Chat(ev.clone()));
                        let _ = publish_chat!(topic_for(&local_id, &channel), &ev);
                    }
                    Bg::ShareFailed(e) => emit(&event_tx, UiEvent::Status(format!("cannot share the file: {e}"))),
                    Bg::Report(r) => {
                        if reports.add(r.clone()) {
                            reports.save();
                            emit(&event_tx, UiEvent::Report(r.clone()));
                        }
                        let _ = swarm.behaviour_mut().gossipsub.publish(control.clone(), Wire::Report(r).encode());
                    }
                    Bg::Verified(hash, ok) => {
                        for a in transfers.on_verified(&hash, ok) {
                            if let crate::transfer::Action::Provide(h) = a {
                                let _ = swarm.behaviour_mut().kad.start_providing(kad::RecordKey::new(&crate::transfer::provider_key(&h)));
                            }
                        }
                        emit(&event_tx, UiEvent::Status(if ok { "download finished and verified — now sharing it too".into() } else { "a download failed verification; starting it again from other nodes".into() }));
                    }
                }
            }

            _ = files_tick.tick() => {
                if !seeding_announced && !connected_peers.is_empty() {
                    seeding_announced = true;
                    for h in transfers.seeding() {
                        let _ = swarm.behaviour_mut().kad.start_providing(kad::RecordKey::new(&crate::transfer::provider_key(&h)));
                    }
                }
                // who is in which room (topic) right now
                let mut members: HashMap<gossipsub::TopicHash, Vec<PeerId>> = HashMap::new();
                for (p, topics) in swarm.behaviour().gossipsub.all_peers() {
                    for t in topics {
                        members.entry(t.clone()).or_default().push(*p);
                    }
                }
                let provs = file_providers.clone();
                let lid = local_id.clone();
                let mut sends: Vec<(PeerId, crate::transfer::FileReq, u64)> = Vec::new();
                let actions = transfers.tick(
                    &connected_peers,
                    |h, ch| {
                        let mut v: Vec<PeerId> = members.get(&topic_for(&lid, ch).hash()).cloned().unwrap_or_default();
                        if let Some(ps) = provs.get(h) {
                            v.extend(ps.iter().copied());
                        }
                        v
                    },
                    &mut |peer, req| {
                        let id = next_req;
                        next_req += 1;
                        sends.push((peer, req, id));
                        id
                    },
                );
                for (peer, req, id) in sends {
                    let rid = swarm.behaviour_mut().files.send_request(&peer, req);
                    req_ids.insert(rid, id);
                }
                for a in actions {
                    match a {
                        crate::transfer::Action::FindProviders(h) => {
                            let q = swarm.behaviour_mut().kad.get_providers(kad::RecordKey::new(&crate::transfer::provider_key(&h)));
                            provider_queries.insert(q, h);
                        }
                        crate::transfer::Action::Provide(h) => {
                            let _ = swarm.behaviour_mut().kad.start_providing(kad::RecordKey::new(&crate::transfer::provider_key(&h)));
                        }
                        crate::transfer::Action::Verify(h, path) => spawn_verify(bg_tx.clone(), h, path),
                        crate::transfer::Action::AskRoom(h, ch) => {
                            let _ = swarm.behaviour_mut().gossipsub.publish(topic_for(&local_id, &ch), Wire::FileWant { hash: h, nonce: rand::random() }.encode());
                        }
                        crate::transfer::Action::Request(..) => {}
                    }
                }
                let infos = transfers.infos();
                if infos != last_transfers {
                    last_transfers = infos.clone();
                    emit(&event_tx, UiEvent::Transfers(infos));
                }
            }

            _ = nodes_tick.tick() => {
                let now = now_ms();
                let online = |p: &str| dir.get(p).map(|(_, t, _)| now.saturating_sub(*t) < 20_000).unwrap_or(false);
                let mut nodes: Vec<NodeInfo> = book
                    .entries
                    .iter()
                    .map(|(pid, e)| {
                        let id = pid.to_base58();
                        NodeInfo {
                            name: dir.get(&id).map(|(n, _, _)| n.clone()).or_else(|| names.get(&id).cloned()),
                            connected: connected_peers.contains(pid),
                            link: link_of.get(pid).copied().unwrap_or(""),
                            source: if friends.contains_key(&id) { "friend" } else { e.source },
                            relays_us: relays_ok.contains(pid),
                            we_relay: relaying_for.contains(pid),
                            online: online(&id) || connected_peers.contains(pid),
                            last_seen_ms: e.last_seen.max(dir.get(&id).map(|d| d.1).unwrap_or(0)),
                            status: statuses.get(&id).cloned().unwrap_or_default(),
                            peer: id,
                        }
                    })
                    .collect();
                // users heard on the network but not (yet) in the address book
                for (p, (n, t, _)) in dir.iter() {
                    if now.saturating_sub(*t) < 20_000 && !nodes.iter().any(|x| &x.peer == p) {
                        nodes.push(NodeInfo { peer: p.clone(), name: Some(n.clone()), connected: false, link: "", source: "exchange", relays_us: false, we_relay: false, online: true, last_seen_ms: *t, status: statuses.get(p).cloned().unwrap_or_default() });
                    }
                }
                nodes.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.online.cmp(&a.online)).then(b.last_seen_ms.cmp(&a.last_seen_ms)));
                nodes.truncate(300);
                if nodes != last_nodes {
                    last_nodes = nodes.clone();
                    emit(&event_tx, UiEvent::Nodes(nodes));
                }
                // live people search (results keep improving for 30 s)
                if let Some((q, at)) = &search {
                    if at.elapsed() < Duration::from_secs(30) {
                        let ql = q.to_lowercase();
                        let tag = crate::types::parse_handle(q);
                        let mut users: Vec<UserCard> = dir
                            .iter()
                            .filter(|(p, (n, _, l))| {
                                **p != local_id
                                    && match &tag {
                                        // a handle is shared on purpose: found even if not listed by name
                                        Some(t) => crate::types::handle_tag(p) == *t,
                                        None => *l && (n.to_lowercase().contains(&ql) || p.starts_with(q.as_str())),
                                    }
                            })
                            .map(|(p, (n, t, _))| UserCard {
                                peer: p.clone(),
                                name: n.clone(),
                                online: now.saturating_sub(*t) < 20_000,
                                last_seen_ms: *t,
                                via: if dht_found.contains(p) { "dht" } else { "network" },
                                status: statuses.get(p).cloned().unwrap_or_default(),
                            })
                            .collect();
                        users.sort_by(|a, b| b.online.cmp(&a.online).then(a.name.to_lowercase().starts_with(&ql).cmp(&b.name.to_lowercase().starts_with(&ql)).reverse()).then(b.last_seen_ms.cmp(&a.last_seen_ms)));
                        users.truncate(50);
                        let sig = format!("{q}{users:?}");
                        if sig != search_sig {
                            search_sig = sig;
                            emit(&event_tx, UiEvent::Search { query: q.clone(), users });
                        }
                    }
                }
            }

            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        if is_circuit(&address) {
                            emit(&event_tx, UiEvent::Status("reachable through a neighbour's relay".into()));
                        } else {
                            local_addrs.insert(address.clone());
                            // Only a node others can actually reach advertises an external
                            // address — that is what makes it usable as a relay. Behind a
                            // router this comes from UPnP instead.
                            if is_public(&address) || (allow_loopback && is_loopback(&address)) {
                                swarm.add_external_address(address.clone());
                            }
                            if is_public(&address) && reachable != Some(true) {
                                reachable = Some(true);
                                emit(&event_tx, UiEvent::Status("you have a public address — you can relay for others".into()));
                            }
                        }
                        emit(&event_tx, UiEvent::Listening(with_p2p(&address, &local_peer).to_string()));
                    }
                    SwarmEvent::ListenerClosed { addresses, reason: Err(e), .. } => {
                        if addresses.iter().any(is_circuit) || format!("{e}").contains("Reservation") {
                            tracing::debug!("relay listener closed: {e}");
                        }
                    }
                    SwarmEvent::ExternalAddrConfirmed { address } => {
                        if is_public(&address) {
                            reachable = Some(true);
                        }
                        emit(&event_tx, UiEvent::Listening(with_p2p(&address, &local_peer).to_string()));
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, num_established, .. } => {
                        profile_due = true;
                        conns += 1;
                        if endpoint.is_dialer() && !is_circuit(endpoint.get_remote_address()) {
                            dial_addr_of.insert(peer_id, endpoint.get_remote_address().clone());
                        }
                        book.seen(&peer_id);
                        let ra = endpoint.get_remote_address();
                        if global_addrs.contains(ra) {
                            book.learn(peer_id, [ra.clone()], "global");
                        }
                        let link = if is_circuit(ra) { "relay" } else if is_public(ra) { "direct" } else { "lan" };
                        if num_established.get() == 1 || link != "relay" {
                            link_of.insert(peer_id, link);
                        }
                        if num_established.get() == 1 {
                            connected_peers.insert(peer_id);
                            if !ever_connected {
                                ever_connected = true;
                                let _ = swarm.behaviour_mut().kad.bootstrap();
                                emit(&event_tx, UiEvent::Status("found the network — now discovering more nodes".into()));
                            }
                            if is_circuit(endpoint.get_remote_address()) {
                                emit(&event_tx, UiEvent::Status(format!("connected to {} via another user's relay", crate::short_peer(&peer_id))));
                            }
                            let id = peer_id.to_base58();
                            update_presence(&friends, &connected_peers, &scores, &mut friend_online, &event_tx);
                            emit(&event_tx, UiEvent::Peer { name: names.get(&id).cloned(), id, connected: true });
                        }
                    }
                    SwarmEvent::ConnectionClosed { peer_id, num_established, .. } => {
                        conns = conns.saturating_sub(1);
                        if num_established == 0 {
                            connected_peers.remove(&peer_id);
                            link_of.remove(&peer_id);
                            relays_ok.remove(&peer_id);
                            relay_requested.remove(&peer_id);
                            relaying_for.remove(&peer_id);
                            update_presence(&friends, &connected_peers, &scores, &mut friend_online, &event_tx);
                            emit(&event_tx, UiEvent::Peer { id: peer_id.to_base58(), name: None, connected: false });
                        }
                    }

                    // ---- scan source: local network ----
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                        if cfg.no_mdns { continue; }
                        for (peer, addr) in list {
                            let known = book.entries.contains_key(&peer) || friends.contains_key(&peer.to_base58());
                            if !scan && !known { continue; }
                            book.learn(peer, [addr.clone()], "lan");
                            swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer);
                            dial_peer(&mut swarm, peer, vec![addr]);
                        }
                    }
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                        for (peer, _addr) in list {
                            swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer);
                        }
                    }

                    // ---- what a neighbour tells us about itself ----
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. })) => {
                        book.learn(peer_id, info.listen_addrs.clone(), "identify");
                        let speaks_kad = info.protocols.iter().any(|p| p.as_ref() == KAD_PROTOCOL);
                        if speaks_kad {
                            for a in &info.listen_addrs {
                                if shareable(a, allow_loopback) {
                                    swarm.behaviour_mut().kad.add_address(&peer_id, strip_p2p(a, &peer_id));
                                }
                            }
                        }
                        // Not reachable ourselves? Ask a reachable neighbour to relay for us.
                        let can_relay = info.protocols.iter().any(|p| p.as_ref() == RELAY_HOP_PROTOCOL);
                        let need_relay = force_relay || reachable != Some(true);
                        if can_relay && need_relay && relay_requested.len() < MAX_RELAYS && !relay_requested.contains(&peer_id) {
                            let base = dial_addr_of.get(&peer_id).cloned().or_else(|| {
                                info.listen_addrs.iter().find(|a| !is_circuit(a) && shareable(a, allow_loopback)).cloned()
                            });
                            if let Some(base) = base {
                                let relay_ok = force_relay || reachable_peers.contains(&peer_id) || is_public(&base);
                                if relay_ok {
                                    let circuit = with_p2p(&strip_p2p(&base, &peer_id), &peer_id).with(Protocol::P2pCircuit);
                                    if swarm.listen_on(circuit).is_ok() {
                                        relay_requested.insert(peer_id);
                                    }
                                }
                            }
                        }
                    }

                    // ---- relays and hole punching ----
                    SwarmEvent::Behaviour(CyberBehaviourEvent::RelayClient(relay::client::Event::ReservationReqAccepted { relay_peer_id, .. })) => {
                        if relays_ok.insert(relay_peer_id) {
                            emit(&event_tx, UiEvent::Status(format!("user node {} relays for you — friends behind NAT can reach you", crate::short_peer(&relay_peer_id))));
                        }
                    }
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Relay(relay::Event::ReservationReqAccepted { src_peer_id, .. })) => {
                        if relaying_for.insert(src_peer_id) {
                            emit(&event_tx, UiEvent::Status(format!("you are relaying for {} (helping them connect)", crate::short_peer(&src_peer_id))));
                        }
                    }
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Dcutr(dcutr::Event { remote_peer_id, result })) => {
                        match result {
                            Ok(_) => emit(&event_tx, UiEvent::Status(format!("hole punched: direct link with {}", crate::short_peer(&remote_peer_id)))),
                            Err(_) => emit(&event_tx, UiEvent::Status(format!("direct link with {} not possible — staying on relay", crate::short_peer(&remote_peer_id)))),
                        }
                    }
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Upnp(ev)) => match ev {
                        upnp::Event::NewExternalAddr(addr) => {
                            reachable = Some(true);
                            emit(&event_tx, UiEvent::Status(format!("router opened a port (UPnP): {addr} — you can relay for others")));
                        }
                        upnp::Event::ExpiredExternalAddr(_) => {
                            reachable = None;
                        }
                        upnp::Event::GatewayNotFound | upnp::Event::NonRoutableGateway => {
                            if reachable.is_none() {
                                reachable = Some(false);
                                emit(&event_tx, UiEvent::Status("no UPnP router — using other users' nodes as relays".into()));
                            }
                        }
                    },

                    // ---- DHT ----
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Kad(kad::Event::RoutingUpdated { peer, addresses, .. })) => {
                        book.learn(peer, addresses.iter().cloned(), "dht");
                    }
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Kad(kad::Event::OutboundQueryProgressed { id, result, step, .. })) => {
                        if let kad::QueryResult::GetClosestPeers(Ok(ok)) = result {
                            for info in ok.peers {
                                book.learn(info.peer_id, info.addrs.clone(), "dht");
                            }
                        } else if let (true, kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders { providers, .. }))) = (mailbox_queries.contains(&id), &result) {
                            // keepers with mail for me: meet them, they hand it over
                            for pid in providers.iter().filter(|p| **p != local_peer).take(8) {
                                if !connected_peers.contains(pid) {
                                    let addrs = book.addrs(pid);
                                    dial_peer(&mut swarm, *pid, addrs);
                                }
                            }
                        } else if let (Some(h), kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders { providers, .. }))) = (provider_queries.get(&id).cloned(), &result) {
                            // Holders of a file we want: remember and meet them (≤ 8 dials).
                            let set = file_providers.entry(h).or_default();
                            for pid in providers.iter().filter(|p| **p != local_peer).take(8) {
                                set.insert(*pid);
                                if !connected_peers.contains(pid) {
                                    let addrs = book.addrs(pid);
                                    dial_peer(&mut swarm, *pid, addrs);
                                }
                            }
                        } else if let kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders { providers, .. })) = result {
                            // People search through the DHT name index: meet these users so
                            // their heartbeat (name) reaches us. At most 8 dials per answer.
                            if let Some(q) = search_queries.get(&id).cloned() {
                                // for a handle, only nodes whose own ID gives that tag
                                let tag = crate::types::parse_handle(&q);
                                for pid in providers.into_iter().filter(|p| *p != local_peer).filter(|p| tag.as_ref().map(|t| crate::types::handle_tag(&p.to_base58()) == *t).unwrap_or(true)).take(8) {
                                    dht_found.insert(pid.to_base58());
                                    book.learn(pid, std::iter::empty(), "search");
                                    if !connected_peers.contains(&pid) {
                                        let addrs = book.addrs(&pid);
                                        dial_peer(&mut swarm, pid, addrs);
                                    }
                                }
                                search_sig.clear();
                            }
                        }
                        if step.last {
                            mailbox_queries.remove(&id);
                            search_queries.remove(&id);
                            provider_queries.remove(&id);
                        }
                        if step.last {
                            if let Some(target) = pending_lookups.remove(&id) {
                                if !connected_peers.contains(&target) {
                                    let addrs = book.addrs(&target);
                                    if !addrs.is_empty() {
                                        dial_peer(&mut swarm, target, addrs);
                                    }
                                }
                            }
                        }
                    }

                    // ---- file pieces (torrent-like) ----
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Files(ev)) => {
                        use libp2p::request_response::{Event as RrEvent, Message as RrMsg};
                        match ev {
                            RrEvent::Message { message: RrMsg::Request { request, channel, .. }, .. } => {
                                let resp = transfers.serve(request);
                                let _ = swarm.behaviour_mut().files.send_response(channel, resp);
                            }
                            RrEvent::Message { message: RrMsg::Response { request_id, response }, .. } => {
                                if let Some(id) = req_ids.remove(&request_id) {
                                    for a in transfers.on_response(id, response) {
                                        if let crate::transfer::Action::Verify(h, path) = a {
                                            spawn_verify(bg_tx.clone(), h, path);
                                        }
                                    }
                                }
                            }
                            RrEvent::OutboundFailure { request_id, .. } => {
                                if let Some(id) = req_ids.remove(&request_id) {
                                    transfers.on_failure(id);
                                }
                            }
                            _ => {}
                        }
                    }

                    // ---- gossip ----
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic })) => {
                        // A friend just joined our shared DM room: ask them for anything we
                        // missed while one of us was offline (now the request can reach them).
                        if let Some(ch) = topic_of_hash.get(&topic).cloned() {
                            if ch.is_dm() && ch.channel == peer_id.to_base58() {
                                let since = store::load_recent(&ch, 1).last().map(|e| e.ts_ms).unwrap_or(0);
                                let t = IdentTopic::new(dm_topic(&local_id, &ch.channel));
                                let _ = swarm.behaviour_mut().gossipsub.publish(t, Wire::SyncReq { channel: ch, since_ms: since, nonce: rand::random() }.encode());
                            } else if !ch.is_dm() && opened.contains(&ch) {
                                let since = store::load_recent(&ch, 1).last().map(|e| e.ts_ms).unwrap_or(0);
                                let _ = swarm.behaviour_mut().gossipsub.publish(IdentTopic::new(ch.topic()), Wire::SyncReq { channel: ch, since_ms: since, nonce: rand::random() }.encode());
                            }
                        }
                    }
                    SwarmEvent::Behaviour(CyberBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. })) => {
                        let Some(mut wire) = Wire::decode(&message.data) else { continue };
                        let th = message.topic.clone();
                        if let Wire::Sealed(s) = &wire {
                            // a sealed DM: only for me, and the inner author must be the sender
                            match open_sealed(&e2e, &local_id, s) {
                                Some(ev) => wire = Wire::Chat(ev),
                                None => continue,
                            }
                        }
                        match wire {
                            Wire::Chat(mut ev) | Wire::History { mut ev, .. } => {
                                if let Some(local) = topic_of_hash.get(&th) {
                                    if local.is_dm() {
                                        ev.channel = local.clone();
                                    }
                                }
                                if !ev.admit() {
                                    continue; // forged signature, or an unsigned edit/delete
                                }
                                accept_chat!(ev, message_author(&message).unwrap_or_default());
                            }
                            Wire::Sealed(_) => {}
                            Wire::Keys(b) => {
                                if b.peer != local_id && b.verify() {
                                    let newer = bundles.get(&b.peer).map(|o| b.ts_ms > o.ts_ms).unwrap_or(true);
                                    if newer {
                                        emit(&event_tx, UiEvent::E2e { peer: b.peer.clone() });
                                        bundles.insert(b.peer.clone(), b);
                                        save_bundles(&bundles);
                                    }
                                }
                            }
                            Wire::MailPut { item } if th == my_mail_hash => {
                                let by = message_author(&message).unwrap_or_default();
                                let blocked = restricted.contains(&item.to) || restricted.contains(&item.from) || restricted.contains(&by);
                                if keeper_on && !blocked && voice_room.is_none() && item.to != local_id && mailbox.put(item.clone(), &by, now_ms()) {
                                    let _ = swarm.behaviour_mut().kad.start_providing(kad::RecordKey::new(&crate::mailbox::key(&item.to)));
                                    mailbox.save();
                                    emit(&event_tx, UiEvent::Mail { kept: mailbox.total(), users: mailbox.items.len() });
                                }
                            }
                            Wire::MailDrop { items } if th == my_mail_hash => {
                                let mut ids = Vec::new();
                                for s in items {
                                    ids.push(s.id.clone());
                                    if let Some(mut ev) = open_sealed(&e2e, &local_id, &s) {
                                        if ev.admit() {
                                            accept_chat!(ev, s.from.clone());
                                        }
                                    }
                                }
                                if let Some(k) = message_author(&message) {
                                    let _ = swarm.behaviour_mut().gossipsub.publish(IdentTopic::new(format!("cc/mail/{k}")), Wire::MailAck { ids }.encode());
                                }
                            }
                            Wire::MailAck { ids } if th == my_mail_hash => {
                                if let Some(by) = message_author(&message) {
                                    if mailbox.ack(&by, &ids) {
                                        swarm.behaviour_mut().kad.stop_providing(&kad::RecordKey::new(&crate::mailbox::key(&by)));
                                    }
                                    mailbox.save();
                                    emit(&event_tx, UiEvent::Mail { kept: mailbox.total(), users: mailbox.items.len() });
                                }
                            }
                            Wire::MailPut { .. } | Wire::MailDrop { .. } | Wire::MailAck { .. } => {}
                            Wire::Call { id, kind, name: caller, .. } => {
                                // only on the DM topic, and only from that friend
                                if let (Some(local), Some(author)) = (topic_of_hash.get(&th), message_author(&message)) {
                                    if local.is_dm() && local.channel == author {
                                        emit(&event_tx, UiEvent::Call { peer: author, name: caller, id, kind });
                                    }
                                }
                            }
                            Wire::SyncReq { since_ms, .. } => {
                                if let Some(local) = topic_of_hash.get(&th).cloned() {
                                    let nonce: u64 = rand::random();
                                    for ev in store::load_since(&local, since_ms, SYNC_REPLY_CAP).into_iter().filter(|e| !restricted.contains(&e.from)) {
                                        let topic = IdentTopic::new(message.topic.as_str());
                                        let (bytes, _) = chat_payload(&ev, &local_id, &bundles, Some(nonce));
                                        let _ = swarm.behaviour_mut().gossipsub.publish(topic, bytes);
                                    }
                                }
                            }
                            Wire::Profile(p) if th == control_hash => {
                                let newer = profiles.get(&p.peer).map(|o| (p.rev, p.ts_ms) > (o.rev, o.ts_ms)).unwrap_or(true);
                                if newer && p.peer != local_id && p.verify() {
                                    profiles.insert(p.peer.clone(), p.clone());
                                    crate::profile::save_all(&profiles.values().cloned().collect::<Vec<_>>());
                                    emit(&event_tx, UiEvent::Profile(p));
                                }
                            }
                            Wire::Report(r) if th == control_hash => {
                                if reports.add(r.clone()) {
                                    reports.save();
                                    emit(&event_tx, UiEvent::Report(r));
                                }
                            }
                            Wire::Report(_) => {}
                            Wire::Score(rec) if th == control_hash => {
                                if !first_seen.contains_key(&rec.peer) {
                                    first_seen.insert(rec.peer.clone(), now_ms());
                                    first_seen_dirty = true;
                                }
                                if rec.keeper && rec.peer != local_id {
                                    keepers.insert(rec.peer.clone(), (rec.score, now_ms()));
                                } else {
                                    keepers.remove(&rec.peer);
                                }
                                if rec.reachable {
                                    if let Some(pid) = parse_peer(&rec.peer) {
                                        reachable_peers.insert(pid);
                                    }
                                }
                                names.insert(rec.peer.clone(), rec.name.clone());
                                if rec.peer != local_id {
                                    dir.insert(rec.peer.clone(), (rec.name.clone(), now_ms(), rec.listed));
                                    statuses.insert(rec.peer.clone(), rec.status.clone());
                                }
                                scores.insert(rec.peer.clone(), (rec.score, rec.ts_ms));
                                emit(&event_tx, UiEvent::VoiceRoster { peer: rec.peer.clone(), name: rec.name.clone(), room: rec.voice.clone() });
                                if connected_peers.iter().any(|p| p.to_base58() == rec.peer) {
                                    emit(&event_tx, UiEvent::Peer { id: rec.peer.clone(), name: Some(rec.name), connected: true });
                                }
                                recompute_hub(&scores, &mut last_hub, &event_tx);
                                update_presence(&friends, &connected_peers, &scores, &mut friend_online, &event_tx);
                            }
                            Wire::Peers(list) if th == control_hash => {
                                // ---- scan source: peer exchange ----
                                let mut fresh: Vec<PeerId> = Vec::new();
                                for rec in list.into_iter().take(64) {
                                    let Some(pid) = parse_peer(&rec.peer) else { continue };
                                    let addrs: Vec<Multiaddr> = rec.addrs.iter().filter_map(|a| a.parse().ok()).collect();
                                    for a in &addrs {
                                        if shareable(a, allow_loopback) {
                                            swarm.behaviour_mut().kad.add_address(&pid, strip_p2p(a, &pid));
                                        }
                                    }
                                    if book.learn(pid, addrs, "exchange") {
                                        fresh.push(pid);
                                    }
                                }
                                if scan && connected_peers.len() < TARGET_CONNECTIONS {
                                    for pid in fresh.into_iter().take(DIALS_PER_ROUND) {
                                        let addrs = book.addrs(&pid);
                                        dial_peer(&mut swarm, pid, addrs);
                                    }
                                }
                            }
                            Wire::Score(_) | Wire::Peers(_) | Wire::Profile(_) => {}
                            Wire::BlobChunk { hash, idx, total, data, .. } => {
                                if store::has_blob(&hash) || total == 0 || total > MAX_CHUNKS || idx >= total {
                                    continue;
                                }
                                if !incoming.contains_key(&hash) && incoming.len() >= MAX_ASSEMBLIES {
                                    // Drop the oldest unfinished download to bound memory.
                                    if let Some(old) = incoming.iter().min_by_key(|(_, a)| a.started).map(|(h, _)| h.clone()) {
                                        incoming.remove(&old);
                                    }
                                }
                                let asm = incoming.entry(hash.clone()).or_insert_with(|| Assembly {
                                    total, parts: vec![None; total as usize], got: 0, started: Instant::now(),
                                });
                                if asm.total != total { continue; }
                                let slot = &mut asm.parts[idx as usize];
                                if slot.is_none() {
                                    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()) else { continue };
                                    if bytes.len() > CHUNK { continue; }
                                    *slot = Some(bytes);
                                    asm.got += 1;
                                    emit(&event_tx, UiEvent::BlobProgress { hash: hash.clone(), got: asm.got, total });
                                }
                                if asm.got == asm.total {
                                    let asm = incoming.remove(&hash).expect("present");
                                    let all: Vec<u8> = asm.parts.into_iter().flatten().flatten().collect();
                                    if blake3::hash(&all).to_hex().as_str() == hash {
                                        store::save_blob(&all);
                                        wanted.remove(&hash);
                                        emit(&event_tx, UiEvent::BlobReady { hash });
                                    } else {
                                        tracing::warn!("media {hash} failed verification; discarded");
                                    }
                                }
                            }
                            Wire::FileWant { hash, .. } => {
                                // We have (part of) it: connect to the asker so it can download from us.
                                if transfers.has_any(&hash) {
                                    if let Some(src) = message.source {
                                        if src != local_peer && !connected_peers.contains(&src) {
                                            let addrs = book.addrs(&src);
                                            dial_peer(&mut swarm, src, addrs);
                                        }
                                    }
                                }
                            }
                            Wire::BlobReq { hash, .. } => {
                                // One answer reaches everyone in the room, so answer at most every
                                // 4 s per file, and each asking node at most every 15 s.
                                let who = message.source;
                                let recently = last_served.get(&hash).map(|t| t.elapsed() < Duration::from_secs(4)).unwrap_or(false)
                                    || who.and_then(|p| served_to.get(&(hash.clone(), p))).map(|t| t.elapsed() < Duration::from_secs(15)).unwrap_or(false);
                                if served_to.len() > 4096 {
                                    served_to.retain(|_, t| t.elapsed() < Duration::from_secs(15));
                                }
                                if !recently {
                                    if let Some(p) = who {
                                        served_to.insert((hash.clone(), p), Instant::now());
                                    }
                                    if let Some(bytes) = store::load_blob(&hash) {
                                        let topic = IdentTopic::new(message.topic.as_str());
                                        publish_blob(&mut swarm, &topic, &hash, &bytes);
                                        last_served.insert(hash, Instant::now());
                                    }
                                }
                            }
                            Wire::Voice(vf) => {
                                v_recv += 1;
                                if let (Some(io), None) = (&voice_io, &voice_bot) {
                                    io.push_playback(&vf.pcm);
                                }
                                if let Some(VoiceBot::Echo { delay_ms }) = &voice_bot {
                                    if !vf.bot && echo_q.len() < 50 * 10 {
                                        echo_q.push_back((Instant::now() + Duration::from_millis(*delay_ms), vf.pcm.clone()));
                                    }
                                }
                                emit(&event_tx, UiEvent::Talking { name: vf.name });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    store::save_peers(&book.to_recs());
    let _ = HISTORY_ON_OPEN;
    Ok(())
}

fn message_author(message: &gossipsub::Message) -> Option<String> {
    message.source.map(|p| p.to_base58())
}

/// Deterministic election: freshest scores win; hub = top, standby = second.
fn recompute_hub(
    scores: &HashMap<String, (f64, u64)>,
    last: &mut (Option<String>, Option<String>),
    event_tx: &std::sync::mpsc::Sender<UiEvent>,
) {
    let now = now_ms();
    let mut ranked: Vec<(&String, f64)> = scores
        .iter()
        .filter(|(_, (_, ts))| now.saturating_sub(*ts) < 20_000)
        .map(|(p, (s, _))| (p, *s))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(b.0)));
    let hub = ranked.first().map(|(p, _)| (*p).clone());
    let standby = ranked.get(1).map(|(p, _)| (*p).clone());
    if (hub.clone(), standby.clone()) != *last {
        *last = (hub.clone(), standby.clone());
        emit(event_tx, UiEvent::Hub { hub, standby });
    }
}

pub const HISTORY_ON_OPEN_PUB: usize = HISTORY_ON_OPEN;

/// "guild/channel" for a public voice room; private calls are not announced.
fn public_room(room: &Option<ChannelId>) -> Option<String> {
    room.as_ref().filter(|c| !c.is_dm()).map(|c| format!("{}/{}", c.guild, c.channel))
}

fn voice_topic_for(local_id: &str, ch: &ChannelId) -> String {
    if ch.is_dm() {
        format!("cc/voice/{}", dm_topic(local_id, &ch.channel))
    } else {
        ch.voice_topic()
    }
}

/// A friend is online if we hold a direct/relayed connection to them, or we
/// have heard their heartbeat in the last 20 s through the mesh.
fn update_presence(
    friends: &HashMap<String, Friend>,
    connected: &HashSet<PeerId>,
    scores: &HashMap<String, (f64, u64)>,
    friend_online: &mut HashMap<String, bool>,
    event_tx: &std::sync::mpsc::Sender<UiEvent>,
) {
    let now = now_ms();
    for f in friends.values() {
        let direct = connected.iter().any(|p| p.to_base58() == f.peer_id);
        let heard = scores.get(&f.peer_id).map(|(_, ts)| now.saturating_sub(*ts) < 20_000).unwrap_or(false);
        let online = direct || heard;
        if friend_online.get(&f.peer_id) != Some(&online) {
            friend_online.insert(f.peer_id.clone(), online);
            emit(event_tx, UiEvent::Friend { peer_id: f.peer_id.clone(), name: f.name.clone(), online });
        }
    }
}
