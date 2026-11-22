#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! CyberCord entry point. GUI by default; `--headless` runs the same network
//! engine with a terminal front-end. There are no servers: every copy is a node.

mod app;
mod dev;
mod update;
mod papers;
mod camera;
mod blocks;
mod docs;
mod fx;
mod intro;
mod math;
mod pdfview;
mod emoji;
mod i18n;
mod media;
mod theme;
mod tray;
mod music;
mod voice_rec;

use std::path::PathBuf;

use clap::Parser;

use cybercord_node::net::{self, Cmd, NetConfig, UiEvent};
use cybercord_node::types::{clock, parse_connect_code, ChannelId, Friend, MediaKind};

#[derive(Parser, Debug)]
#[command(name = "cybercord", about = "CyberCord — serverless P2P chat (SocioCyberneering Inc.)")]
struct Args {
    /// Display name shown to other peers.
    #[arg(short, long, default_value = "anon")]
    name: String,

    /// Ed25519 identity file (created on first run). Default: identity.key in
    /// the data folder (~/.local/share/cybercord, or $CYBERCORD_DATA_DIR).
    #[arg(short, long)]
    identity: Option<PathBuf>,

    /// Run without a window (terminal front-end).
    #[arg(long)]
    headless: bool,

    /// Start hidden in the system tray (used by autostart).
    #[arg(long)]
    minimized: bool,

    /// Optional: address of any known CyberCord node (e.g. a friend's) to dial at
    /// start. Not required — the app scans LAN, saved nodes, friends and the DHT.
    #[arg(long = "bootstrap")]
    bootstrap: Vec<String>,

    /// Disable local-network auto-discovery (mDNS).
    #[arg(long)]
    no_mdns: bool,

    /// TCP listen port (0 = default 47100, random if busy).
    #[arg(long, default_value_t = 0)]
    tcp_port: u16,

    /// UDP/QUIC listen port (0 = default 47100, random if busy).
    #[arg(long, default_value_t = 0)]
    quic_port: u16,

    /// Channel to open in headless mode, as guild/channel.
    #[arg(long, default_value = "sce/welcome")]
    channel: String,

    /// Developer mode (Settings → About → Developer mode does the same).
    #[arg(long)]
    dev: bool,

    /// Developer test bot (terminal mode): echo | tone | music. The bot joins
    /// --voice-room without opening any audio device.
    #[arg(long)]
    voice_bot: Option<String>,

    /// Voice room for the test bot, as guild/channel (e.g. sce/lounge).
    #[arg(long)]
    voice_room: Option<String>,
}

fn main() -> anyhow::Result<()> {
    // Sound devices chosen in Settings → Sound (both GUI and terminal mode).
    let st = cybercord_node::store::load_settings();
    st.apply_levels();
    cybercord_node::voice::set_devices(Some(st.in_dev), Some(st.out_dev));
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cybercord_node=info,libp2p_gossipsub=error".into()),
        )
        .with_target(false)
        .init();

    update::remember_exe();
    let args = Args::parse();
    let cfg = NetConfig {
        name: args.name.clone(),
        identity: args.identity.clone().unwrap_or_else(default_identity),
        listed: st.listed,
        global: st.global,
        scan: st.scan,
        status: st.status.clone(),
        bootstrap: args.bootstrap.clone(),
        no_mdns: args.no_mdns,
        tcp_port: args.tcp_port,
        quic_port: args.quic_port,
        voice_bot: args.voice_bot.as_deref().map(make_bot),
    };
    if args.dev {
        std::env::set_var("CYBERCORD_DEV", "1");
    }

    if args.headless {
        let room = if cfg.voice_bot.is_some() { args.voice_room.clone() } else { None };
        run_headless(cfg, &args.channel, room)
    } else {
        if std::env::var("CYBERCORD_DEV_CHILD").is_ok() {
            // a developer-mode test window lives as long as the CyberCord that started it
            std::thread::spawn(|| {
                use std::io::Read;
                let mut sink = [0u8; 64];
                while matches!(std::io::stdin().read(&mut sink), Ok(n) if n > 0) {}
                std::process::exit(0);
            });
        }
        app::run_gui(cfg, args.minimized).map_err(|e| anyhow::anyhow!("gui: {e}"))
    }
}

/// identity.key in the data folder; adopts a ./cybercord.key left by v0.4 and
/// older so the user keeps the same ID (friends' codes stay valid).
fn default_identity() -> PathBuf {
    let path = cybercord_node::store::data_dir().join("identity.key");
    if !path.exists() {
        let old = PathBuf::from("cybercord.key");
        if old.exists() {
            let _ = std::fs::create_dir_all(cybercord_node::store::data_dir());
            let _ = std::fs::copy(&old, &path);
        }
    }
    path
}

fn parse_channel(s: &str) -> ChannelId {
    let (g, c) = s.split_once('/').unwrap_or(("sce", "welcome"));
    ChannelId::new(g, c)
}

const HELP: &str = "\
commands:
  /id                            print your user ID (friends can add you by ID)
  /code                          print your connect code (ID + addresses)
  /addfriend <name> <id|code>    add a friend by ID or connect code
  /net                           network status (nodes known/connected, relays)
  /scan                          scan for nodes again
  /friends                       list friends
  /dm <name> <text>              direct-message a friend
  /node <address>                connect to a known node address
  /send <path> [text]            send a GIF/image/music file to the open channel
  /dmsend <friend> <path>        send a GIF/image file to a friend
  /open guild/channel            switch channel
  /circle [seconds]              record a video circle from the webcam and send it
  /play [file]                   play a music file, or the newest music message here
  /stop                          stop the music
  /voicemsg [seconds]            record a voice message and send it
  /devices                       list microphones and speakers
  /find <name>                   search people by name (or ID start)
  /share <path>                  share a file of any size in the open channel (torrent-like)
  /profile Rs8 El7 GS6            set my skills (catalog code + level 0–10) and publish the profile
  /edit <text>                   edit my last message in the open channel
  /del                           delete my last message in the open channel
  /get [name]                    download a file posted in the open channel (waits if nobody has it online)
  /files                         downloads and shared files
  /pause|/resume|/cancel <name>  control a download
  /nodes                         the nodes this node knows (the network map)
  /voice [guild/room]            join a voice channel (e.g. /voice sce/lounge)
  /leave                         leave voice
  /quit
anything else is sent to the open channel.";

/// Terminal front-end: print events; stdin lines are commands or channel chat.
/// Developer test bots: echo, a tone pattern, or the intro soundtrack.
fn make_bot(kind: &str) -> net::VoiceBot {
    use std::sync::Arc;
    const RATE: f32 = 48_000.0;
    match kind {
        "tone" => {
            // "beep-beep-beeeep" rising, 3 s: easy to tell apart from speech
            let mut v: Vec<i16> = Vec::new();
            for (f, ms) in [(523.0f32, 180), (0.0, 120), (659.0, 180), (0.0, 120), (784.0, 500), (0.0, 1900)] {
                let n = (RATE * ms as f32 / 1000.0) as usize;
                for i in 0..n {
                    let t = i as f32 / RATE;
                    let env = ((i as f32 / 480.0).min(1.0)) * (((n - i) as f32 / 480.0).min(1.0));
                    v.push(if f > 0.0 { ((t * f * std::f32::consts::TAU).sin() * 9000.0 * env) as i16 } else { 0 });
                }
            }
            net::VoiceBot::Play(Arc::new(v))
        }
        "music" => {
            let mut v: Vec<i16> = Vec::new();
            if let Ok(tr) = music::decode(intro::VIDEO) {
                let ch = tr.channels.max(1);
                let mono: Vec<i16> = tr.samples.chunks(ch).map(|c| (c.iter().sum::<f32>() / ch as f32 * 0.6 * 32767.0) as i16).collect();
                let mut rs = cybercord_node::voice::Resampler::new(tr.rate, 48_000);
                rs.push(&mono, &mut v);
            }
            net::VoiceBot::Play(Arc::new(v))
        }
        _ => net::VoiceBot::Echo { delay_ms: 1500 },
    }
}

fn run_headless(cfg: NetConfig, channel: &str, bot_room: Option<String>) -> anyhow::Result<()> {
    use std::io::BufRead;
    use std::sync::{Arc, Mutex};

    let name = cfg.name.clone();
    let handle = net::spawn(cfg);
    if let Some(r) = &bot_room {
        let _ = handle.cmd_tx.send(Cmd::JoinVoice { channel: parse_channel(r) });
        println!("· test bot in voice room {r}");
    }
    let mut current = parse_channel(channel);
    println!("CyberCord (headless) — {name} — channel {}/{}  (/help)", current.guild, current.channel);

    let addrs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let friends: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new())); // (peer_id, name)
    let my_id: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let stats: Arc<Mutex<String>> = Arc::new(Mutex::new("scanning…".into()));
    let nodes: Arc<Mutex<Vec<cybercord_node::types::NodeInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let nodes2 = nodes.clone();
    let xfers: Arc<Mutex<Vec<cybercord_node::transfer::TransferInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let xfers2 = xfers.clone();

    let ev_rx = handle.event_rx;
    let (a2, f2, id2, st2) = (addrs.clone(), friends.clone(), my_id.clone(), stats.clone());
    let printer = std::thread::spawn(move || {
        let mut seen: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        let mut rooms: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
        let mut talk_seen: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
        while let Ok(ev) = ev_rx.recv() {
            match ev {
                UiEvent::Ready { peer_id, .. } => {
                    println!("· you are {peer_id}");
                    *id2.lock().unwrap() = peer_id;
                }
                UiEvent::Net(n) => {
                    let line = format!(
                        "{} known · {} connected · relays {} · relaying for {} · reachable {} · found via [{}]",
                        n.known, n.connected, n.relays, n.relaying_for,
                        match n.reachable { Some(true) => "yes", Some(false) => "no", None => "?" },
                        n.sources
                    );
                    println!("· net: {line}");
                    *st2.lock().unwrap() = line;
                }
                UiEvent::Listening(a) => {
                    println!("· listening {a}");
                    a2.lock().unwrap().push(a);
                }
                UiEvent::Status(s) => println!("· {s}"),
                UiEvent::Profile(p) => println!("· profile {} [{}] v{}", p.name, p.code(), p.rev),
                UiEvent::Chat(e) => {
                    let room = if e.channel.is_dm() { "DM".to_string() } else { format!("{}/{}", e.channel.guild, e.channel.channel) };
                    let media = e.attach.as_ref().map(|a| format!(" [{:?} {} {} KB {}…]", a.kind, a.name, a.size / 1024, &a.hash[..12])).unwrap_or_default();
                    match (&e.edits, e.deleted) {
                        (Some(t), true) => println!("[{}] {room} {} deleted message {}", clock(e.ts_ms), e.name, &t[..t.len().min(8)]),
                        (Some(t), false) => println!("[{}] {room} {} edited {} → v{}: {}", clock(e.ts_ms), e.name, &t[..t.len().min(8)], e.rev, e.body),
                        _ => println!("[{}] {room} {}: {}{media}", clock(e.ts_ms), e.name, e.body),
                    }
                }
                UiEvent::Peer { id, connected, .. } => {
                    // print only real state changes (name refreshes re-send the same state)
                    if seen.insert(id.clone(), connected) != Some(connected) {
                        println!("· peer {} {}", &id[..id.len().min(12)], if connected { "connected" } else { "gone" })
                    }
                }
                UiEvent::Hub { hub, standby } => println!(
                    "· hub={} standby={}",
                    hub.as_deref().map(|h| &h[..h.len().min(12)]).unwrap_or("—"),
                    standby.as_deref().map(|h| &h[..h.len().min(12)]).unwrap_or("—")
                ),
                UiEvent::VoiceJoined { room } => println!("· voice {}", room.as_deref().unwrap_or("left")),
                UiEvent::Talking { name } => {
                    // once per speaker per 3 s, not per 20 ms frame
                    let now = std::time::Instant::now();
                    let last = talk_seen.get(&name).copied();
                    if last.map(|t| now.duration_since(t).as_secs() >= 3).unwrap_or(true) {
                        println!("· {name} is talking");
                    }
                    talk_seen.insert(name, now);
                }
                UiEvent::VoiceStats { sent, recv, buffer_ms } => println!("· voice: sent {sent} · received {recv} frames / 5 s · buffer {buffer_ms} ms"),
                UiEvent::Nodes(list) => *nodes2.lock().unwrap() = list,
                UiEvent::Transfers(list) => {
                    let mut old = xfers2.lock().unwrap();
                    for t in &list {
                        let pct = if t.size == 0 { 100 } else { t.have_bytes * 100 / t.size };
                        let prev = old.iter().find(|o| o.hash == t.hash);
                        let prev_pct = prev.map(|o| if o.size == 0 { 100 } else { o.have_bytes * 100 / o.size });
                        if prev.map(|o| o.state) != Some(t.state) || prev_pct.map(|p| p / 25 != pct / 25).unwrap_or(true) {
                            println!("· file {} {:>3}% {:<11} {} peers  {} KB/s", t.name, pct, t.state, t.peers, t.speed_bps / 1024);
                        }
                    }
                    *old = list;
                }
                UiEvent::Search { query, users } => {
                    println!("· search \"{query}\": {} found", users.len());
                    for u in users.iter().take(20) {
                        println!("    {:<20} {}  {}  [{}]", u.name, u.peer, if u.online { "online" } else { "offline" }, u.via);
                    }
                }
                UiEvent::VoiceRoster { peer, name, room } => {
                    if rooms.get(&peer) != Some(&room) {
                        println!("· voice roster: {name} {}", room.as_deref().map(|r| format!("joined {r}")).unwrap_or_else(|| "left voice".into()));
                        rooms.insert(peer, room);
                    }
                }
                UiEvent::BlobProgress { hash, got, total } => {
                    if got == 1 || got == total || got % 8 == 0 {
                        println!("· media {}… {got}/{total}", &hash[..12]);
                    }
                }
                UiEvent::BlobReady { hash } => println!("· media ready {}…", &hash[..12]),
                UiEvent::Friend { peer_id, name, online } => {
                    let mut f = f2.lock().unwrap();
                    if !f.iter().any(|(p, _)| *p == peer_id) {
                        f.push((peer_id.clone(), name.clone()));
                    }
                    println!("· friend {name} is {}", if online { "ONLINE" } else { "offline" });
                }
            }
        }
    });

    let _ = handle.cmd_tx.send(Cmd::RequestSync { channel: current.clone(), since_ms: 0 });

    let stdin = std::io::stdin();
    let mut player: Option<music::Player> = None;
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('/') {
            let mut it = rest.splitn(2, ' ');
            let cmd = it.next().unwrap_or("");
            let arg = it.next().unwrap_or("").trim();
            match cmd {
                "quit" | "exit" => break,
                "help" => println!("{HELP}"),
                "id" => println!("your ID: {}", my_id.lock().unwrap()),
                "net" => println!("{}", stats.lock().unwrap()),
                "voice" => {
                    let ch = if arg.is_empty() { current.clone() } else { parse_channel(arg) };
                    let _ = handle.cmd_tx.send(Cmd::JoinVoice { channel: ch });
                }
                "leave" => {
                    let _ = handle.cmd_tx.send(Cmd::LeaveVoice);
                }
                "scan" => {
                    let _ = handle.cmd_tx.send(Cmd::Rescan);
                }
                "code" => {
                    let a = addrs.lock().unwrap();
                    println!("your connect code:\n{}", a.join(" "));
                }
                "friends" => {
                    for (p, n) in friends.lock().unwrap().iter() {
                        println!("  {n}  {p}");
                    }
                }
                "addfriend" => {
                    let mut parts = arg.splitn(2, ' ');
                    let fname = parts.next().unwrap_or("").to_string();
                    let code = parts.next().unwrap_or("");
                    match parse_connect_code(code) {
                        Some((peer_id, addrs)) if !fname.is_empty() => {
                            let _ = handle.cmd_tx.send(Cmd::AddFriend(Friend { peer_id, name: fname, addrs }));
                        }
                        _ => println!("usage: /addfriend <name> <ID or connect code>"),
                    }
                }
                "dm" => {
                    let mut parts = arg.splitn(2, ' ');
                    let who = parts.next().unwrap_or("");
                    let text = parts.next().unwrap_or("").to_string();
                    let target = friends.lock().unwrap().iter().find(|(p, n)| n == who || p == who).map(|(p, _)| p.clone());
                    match target {
                        Some(peer_id) if !text.is_empty() => {
                            let _ = handle.cmd_tx.send(Cmd::SendDm { peer_id, body: text });
                        }
                        _ => println!("usage: /dm <friend-name> <text>  (add them first with /addfriend)"),
                    }
                }
                "send" | "dmsend" => {
                    let (target, rest) = if cmd == "dmsend" {
                        let mut p = arg.splitn(2, ' ');
                        let who = p.next().unwrap_or("");
                        let peer = friends.lock().unwrap().iter().find(|(id, n)| n == who || id == who).map(|(id, _)| id.clone());
                        match peer { Some(id) => (ChannelId::dm(id), p.next().unwrap_or("").to_string()), None => { println!("unknown friend"); continue; } }
                    } else {
                        (current.clone(), arg.to_string())
                    };
                    let mut p = rest.splitn(2, ' ');
                    let path = p.next().unwrap_or("");
                    let text = p.next().unwrap_or("").to_string();
                    match std::fs::read(path) {
                        Ok(bytes) => {
                            let file = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            match media::sniff(&bytes) {
                                Some(MediaKind::Audio) => match music::describe(&bytes, &file) {
                                    Ok((name, dur_ms)) => {
                                        println!("· music: {name} ({}:{:02})", dur_ms / 60_000, dur_ms / 1000 % 60);
                                        let _ = handle.cmd_tx.send(Cmd::SendMedia { channel: target, body: text, kind: MediaKind::Audio, name, bytes, w: 0, h: 0, dur_ms });
                                    }
                                    Err(e) => println!("cannot read audio {path}: {e}"),
                                },
                                Some(kind @ (MediaKind::Gif | MediaKind::Image)) => {
                                    let (w, h) = media::dimensions(&bytes);
                                    let _ = handle.cmd_tx.send(Cmd::SendMedia { channel: target, body: text, kind, name: file, bytes, w, h, dur_ms: 0 });
                                }
                                _ => println!("unsupported file (GIF, PNG, JPEG, mp3, flac, ogg, wav, m4a)"),
                            }
                        }
                        Err(e) => println!("cannot read {path}: {e}"),
                    }
                }
                "circle" => {
                    // Record a video circle from the webcam (or the synthetic test
                    // camera with CYBERCORD_TEST_CAMERA=1) and send it.
                    let secs: f32 = arg.parse().unwrap_or(3.0);
                    let rec = camera::Recorder::start();
                    std::thread::sleep(std::time::Duration::from_millis((secs * 1000.0) as u64));
                    if let Some(e) = rec.error() {
                        println!("camera: {e}");
                        rec.cancel();
                        continue;
                    }
                    match rec.finish() {
                        Some(clip) => {
                            let dur_ms = clip.dur_ms();
                            let frames = clip.frames.len();
                            let bytes = clip.encode();
                            println!("· circle: {frames} frames, {:.1} s, {} KB", dur_ms as f32 / 1000.0, bytes.len() / 1024);
                            let _ = handle.cmd_tx.send(Cmd::SendMedia {
                                channel: current.clone(),
                                body: String::new(),
                                kind: MediaKind::Circle,
                                name: "circle".into(),
                                bytes,
                                w: camera::SIZE,
                                h: camera::SIZE,
                                dur_ms,
                            });
                        }
                        None => println!("camera: nothing recorded"),
                    }
                }
                "play" => {
                    // Play a music file (path) or the newest music message here.
                    let bytes = if !arg.is_empty() {
                        std::fs::read(arg).ok()
                    } else {
                        cybercord_node::store::load_recent(&current, 200)
                            .iter()
                            .rev()
                            .find_map(|e| e.attach.as_ref().filter(|a| a.kind == MediaKind::Audio))
                            .and_then(|a| cybercord_node::store::load_blob(&a.hash))
                    };
                    match bytes.map(|b| music::decode(&b)) {
                        Some(Ok(t)) => {
                            let secs = t.dur_ms() / 1000;
                            match music::play(std::sync::Arc::new(t)) {
                                Some(p) => {
                                    println!("· playing ({secs} s) — /stop to stop");
                                    player = Some(p);
                                }
                                None => println!("· no sound in this build (rebuild with --features voice) or no output device"),
                            }
                        }
                        Some(Err(e)) => println!("cannot decode: {e}"),
                        None => println!("no music here (try /play <file>)"),
                    }
                }
                "stop" => {
                    player = None;
                }
                "profile" => {
                    // /profile Rs8 El7 GS6 — set skills (catalog code + level) and publish
                    let skills = arg.split_whitespace().filter_map(|t| {
                        let i = t.find(|c: char| c.is_ascii_digit())?;
                        Some(cybercord_node::profile::Skill { id: t[..i].to_string(), level: t[i..].parse().ok()?, note: String::new() })
                    }).collect();
                    let _ = handle.cmd_tx.send(Cmd::SetProfile(cybercord_node::profile::Profile { skills, ..Default::default() }));
                }
                "edit" | "del" => {
                    let me = my_id.lock().unwrap().clone();
                    let all = cybercord_node::store::load_recent(&current, 2000);
                    let (edits, msgs): (Vec<_>, Vec<_>) = all.into_iter().partition(|e| e.edits.is_some());
                    let last = msgs.iter().rev().filter(|m| m.from == me).map(|m| (m, cybercord_node::history::resolve(m, &edits))).find(|(_, r)| !r.deleted);
                    match last {
                        Some((m, r)) if cmd == "del" => {
                            let _ = handle.cmd_tx.send(Cmd::EditMessage { channel: current.clone(), target: m.id.clone(), body: None, rev: r.rev + 1 });
                        }
                        Some((m, r)) if !arg.is_empty() => {
                            let _ = handle.cmd_tx.send(Cmd::EditMessage { channel: current.clone(), target: m.id.clone(), body: Some(arg.to_string()), rev: r.rev + 1 });
                        }
                        Some(_) => println!("usage: /edit <new text>"),
                        None => println!("no message of yours here"),
                    }
                }
                "share" => {
                    let _ = handle.cmd_tx.send(Cmd::ShareFile { channel: current.clone(), path: std::path::PathBuf::from(arg), body: String::new() });
                }
                "get" => {
                    // download a file posted in the open channel: /get <name or hash start>
                    let found = cybercord_node::store::load_recent(&current, 500)
                        .into_iter()
                        .rev()
                        .filter_map(|e| e.attach)
                        .find(|a| a.kind == MediaKind::File && (arg.is_empty() || a.hash.starts_with(arg) || a.name.contains(arg)));
                    match found {
                        Some(a) => {
                            let _ = handle.cmd_tx.send(Cmd::Download { hash: a.hash, name: a.name, size: a.size, channel: current.clone() });
                        }
                        None => println!("no such file in this channel"),
                    }
                }
                "files" => {
                    for t in xfers.lock().unwrap().iter() {
                        let pct = if t.size == 0 { 100 } else { t.have_bytes * 100 / t.size };
                        println!("  {:<28} {:>3}%  {:<11} {} peers  {}", t.name, pct, t.state, t.peers, t.path.clone().unwrap_or_default());
                    }
                }
                "pause" | "resume" | "cancel" => {
                    if let Some(t) = xfers.lock().unwrap().iter().find(|t| t.name.contains(arg) || t.hash.starts_with(arg)) {
                        let cmdv = match cmd {
                            "pause" => Cmd::PauseDownload { hash: t.hash.clone(), paused: true },
                            "resume" => Cmd::PauseDownload { hash: t.hash.clone(), paused: false },
                            _ => Cmd::CancelDownload(t.hash.clone()),
                        };
                        let _ = handle.cmd_tx.send(cmdv);
                    }
                }
                "find" => {
                    let _ = handle.cmd_tx.send(Cmd::Search(arg.to_string()));
                }
                "nodes" => {
                    let list = nodes.lock().unwrap().clone();
                    println!("{} nodes known:", list.len());
                    for n in list.iter().take(40) {
                        println!(
                            "  {} {:<16} {:<8} found via {:<9} {}{}",
                            if n.connected { "●" } else if n.online { "○" } else { "·" },
                            n.name.clone().unwrap_or_else(|| "?".into()),
                            n.link,
                            n.source,
                            &n.peer[..n.peer.len().min(16)],
                            if n.relays_us { "  relays for us" } else if n.we_relay { "  we relay for it" } else { "" }
                        );
                    }
                }
                "devices" => {
                    let (i, o) = cybercord_node::voice::chosen_devices();
                    println!("microphones: {:?}\nspeakers:    {:?}\nchosen: in={} out={}", cybercord_node::voice::list_inputs(), cybercord_node::voice::list_outputs(), i.as_deref().unwrap_or("default"), o.as_deref().unwrap_or("default"));
                }
                "voicemsg" => {
                    // Record a voice message from the microphone and send it.
                    let secs: f32 = arg.parse().unwrap_or(3.0);
                    match voice_rec::MicRecorder::start() {
                        None => println!("no microphone (or this build has no sound)"),
                        Some(rec) => {
                            std::thread::sleep(std::time::Duration::from_millis((secs * 1000.0) as u64));
                            match voice_rec::finish_voice_message(rec) {
                                Some((bytes, dur_ms)) => {
                                    println!("· voice message: {:.1} s, {} KB", dur_ms as f32 / 1000.0, bytes.len() / 1024);
                                    let _ = handle.cmd_tx.send(Cmd::SendMedia {
                                        channel: current.clone(),
                                        body: String::new(),
                                        kind: MediaKind::Audio,
                                        name: voice_rec::VOICE_MSG_NAME.into(),
                                        bytes,
                                        w: 0,
                                        h: 0,
                                        dur_ms,
                                    });
                                }
                                None => println!("nothing recorded"),
                            }
                        }
                    }
                }
                "node" => {
                    let _ = handle.cmd_tx.send(Cmd::AddNode(arg.to_string()));
                }
                "open" => {
                    current = parse_channel(arg);
                    println!("· opened {}/{}", current.guild, current.channel);
                    let _ = handle.cmd_tx.send(Cmd::RequestSync { channel: current.clone(), since_ms: 0 });
                }
                other => println!("· unknown /{other} (try /help)"),
            }
            continue;
        }
        let _ = handle.cmd_tx.send(Cmd::SendChat { channel: current.clone(), body: line.to_string() });
    }

    let _ = handle.cmd_tx.send(Cmd::Quit);
    drop(handle.cmd_tx);
    let _ = printer.join();
    Ok(())
}
