//! CyberCord desktop GUI (egui/eframe) — white, Adwaita-styled client.
//!
//! Palette follows the GNOME HIG: white surfaces (Light 1/2), near-black text
//! (Dark 4), sky-blue accent (Blue 3) and the brand periwinkle (Blue 1).
//! Every visible string goes through `self.t(..)` (see `i18n.tsv`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, load::SizedTexture, Align2, Color32, Id, LayerId, Order, Pos2, Rect, RichText, Rounding, Stroke, Vec2};

use cybercord_node::net::{self, Cmd, NetConfig, NetStats, NetworkHandle, UiEvent, MAX_BLOB};
use cybercord_node::servers::SERVERS;
use cybercord_node::store::{self, Settings};
use cybercord_node::profile::{self as prof, Profile, Skill};
use cybercord_node::semantics::{self as sem, Kind, Level};
use cybercord_node::types::{CallKind, clock, now_ms, short_date, subscript, parse_connect_code, Attachment, ChannelId, ChatEvent, Friend, MediaKind};
use cybercord_node::voice;

use crate::camera;
use crate::docs::{self, Doc};
use crate::emoji::{self, EmojiCache, Seg};
use crate::i18n::{self, Lang};
use crate::media::{self, MediaCache, Shown};
use crate::music;
use crate::voice_rec;
use crate::theme::{self, p as th};

// Colours come from the active theme: see `theme.rs` and `p()`.

const VERSION: &str = env!("CARGO_PKG_VERSION");


fn short(id: &str) -> String {
    if id.chars().count() <= 10 {
        id.to_string()
    } else {
        let c: Vec<char> = id.chars().collect();
        format!("{}…{}", c[..6].iter().collect::<String>(), c[c.len() - 4..].iter().collect::<String>())
    }
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// Scale / chart editor state. `target`: (message id, first line, line count).
struct BlockEd {
    chart_mode: bool,
    scale: crate::blocks::Scale,
    chart: crate::blocks::Chart,
    target: Option<(String, usize, usize)>,
}

impl BlockEd {
    fn new_scale() -> Self {
        BlockEd { chart_mode: false, scale: crate::blocks::Scale { title: String::new(), value: 5.0, max: 10.0, note: String::new() }, chart: Self::empty_chart(), target: None }
    }
    fn new_chart() -> Self {
        BlockEd { chart_mode: true, scale: Self::new_scale().scale, chart: Self::empty_chart(), target: None }
    }
    fn empty_chart() -> crate::blocks::Chart {
        crate::blocks::Chart { kind: crate::blocks::ChartKind::Bar, title: String::new(), rows: vec![("A".into(), 3.0), ("B".into(), 5.0), ("C".into(), 2.0)] }
    }
}

#[derive(Default)]
struct ChannelView {
    msgs: Vec<ChatEvent>,
    /// edit/delete events (applied to `msgs` when drawing)
    edits: Vec<ChatEvent>,
    seen: HashSet<String>,
}

impl ChannelView {
    fn add(&mut self, ev: ChatEvent) {
        if self.seen.insert(ev.id.clone()) {
            if ev.edits.is_some() {
                self.edits.push(ev);
            } else {
                self.msgs.push(ev);
            }
        }
    }
}
struct PeerView {
    name: Option<String>,
    connected: bool,
}
struct FriendView {
    name: String,
    online: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Need {
    Home,
    Friends,
    Chat,
    Any,
}

/// One step of the onboarding tour: (title key, body key, anchor, view it needs).
const TOUR: &[(&str, &str, Option<&str>, Need)] = &[
    ("t0_t", "t0_b", None, Need::Home),
    ("t1_t", "t1_b", Some("network"), Need::Home),
    ("t2_t", "t2_b", Some("rail"), Need::Any),
    ("t3_t", "t3_b", Some("friends_btn"), Need::Any),
    ("t4_t", "t4_b", Some("code"), Need::Friends),
    ("t5_t", "t5_b", Some("composer"), Need::Chat),
    ("t6_t", "t6_b", Some("media_btns"), Need::Chat),
    ("t7_t", "t7_b", Some("voice"), Need::Chat),
    ("t8_t", "t8_b", Some("settings_btn"), Need::Any),
];

#[derive(Clone, Copy, PartialEq)]
enum Popup {
    None,
    Emoji,
    Gifs,
}

pub struct CyberApp {
    handle: NetworkHandle,
    self_id: String,
    self_name: String,
    show_home: bool,
    sel_server: usize,
    sel_channel: usize,
    views: HashMap<ChannelId, ChannelView>,
    opened: HashSet<ChannelId>,
    peers: BTreeMap<String, PeerView>,
    hub: Option<String>,
    standby: Option<String>,
    status: String,
    input: String,
    // voice
    in_voice: Option<String>,
    mic_on: bool,
    talkers: HashMap<String, u64>,
    // friends + network
    show_friends: bool,
    friends: BTreeMap<String, FriendView>,
    dm_open: Option<String>,
    listen_addrs: Vec<String>,
    add_code: String,
    add_name: String,
    add_msg: String,
    node_addr: String,
    net: NetStats,
    resync: bool,
    resync_again: Option<Instant>,
    // language + settings + tour
    settings: Settings,
    lang: Lang,
    show_settings: bool,
    tour: Option<usize>,
    tour_changed: Instant,
    anchors: HashMap<&'static str, Rect>,
    toast: Option<(String, Instant)>,
    // media
    emoji: EmojiCache,
    media: MediaCache,
    popup: Popup,
    popup_rect: Rect,
    emoji_tab: usize,
    emoji_query: String,
    file_path: String,
    progress: HashMap<String, (u32, u32)>,
    fetch_asked: HashSet<String>,
    circle_sound: Option<(String, Instant, Option<voice_rec::Player>)>,
    // recording a video circle
    rec: Option<camera::Recorder>,
    rec_tex: Option<egui::TextureHandle>,
    rec_error: Option<String>,
    // icons, voice roster (peer -> (name, room, last seen)), guide page
    icons: Icons,
    roster: HashMap<String, (String, Option<String>, u64)>,
    guide_step: usize,
    guide_changed: Instant,
    music: Music,
    // background + tray
    tray: Option<crate::tray::Tray>,
    tray_rx: std::sync::mpsc::Receiver<crate::tray::TrayCmd>,
    quitting: Arc<std::sync::atomic::AtomicBool>,
    notify: Arc<std::sync::Mutex<NotifyState>>,
    hidden: bool,
    bg_hint_shown: bool,
    tray_sig: String,
    pending_minimize: bool,
    /// Demo/test: press the window's close button after a delay.
    demo_close_at: Option<Instant>,
    /// Demo/screenshot helper ($CYBERCORD_START=play): start the first track shown.
    autoplay: bool,
    // voice message being recorded: recorder, start, level history for the live waveform
    vm: Option<(voice_rec::MicRecorder, Instant, Vec<f32>)>,
    vm_last_level: Instant,
    // Settings → Sound
    dev_lists: Option<(Vec<String>, Vec<String>)>,
    monitor: Option<voice_rec::Monitor>,
    mic_heard: f32,
    set_tab: SetTab,
    set_opened: Instant,
    applied_theme: String,
    mic_test_msg: Option<&'static str>,
    test_player: Option<(voice_rec::Player, Instant)>,
    // network map + people search
    nodes: Vec<cybercord_node::types::NodeInfo>,
    search_q: String,
    search_res: Vec<cybercord_node::types::UserCard>,
    show_map: bool,
    /// SocioCyberneering city map; my subscribed branches in the side panel
    show_city: bool,
    /// Developer mode window; voice counters from the node
    show_dev: bool,
    started_ms: u64,
    brand_played: bool,
    /// interface sounds that are playing, and when each last played
    sfx: Vec<crate::music::Player>,
    sfx_last: HashMap<crate::sounds::Sfx, Instant>,
    /// calls between friends
    call: CallState,
    call_tone_at: Option<Instant>,
    show_invite: bool,
    show_add_friend: bool,
    /// friends whose encryption keys are known (DMs sealed end to end)
    e2e_peers: HashSet<String>,
    mail_kept: (usize, usize),
    /// adding a friend by handle: (tag, name to give them, since)
    pending_handle: Option<(String, String, Instant)>,
    invite_qr: Option<egui::TextureHandle>,
    /// Scientific article search
    show_papers: bool,
    papers: crate::papers::Search,
    papers_q: String,
    papers_src: crate::papers::Source,
    papers_oa: bool,
    papers_res: Vec<crate::papers::Paper>,
    papers_busy: bool,
    papers_err: Option<String>,
    papers_expanded: HashSet<usize>,
    /// PDFs being downloaded: url → (paper, send to the chat when done)
    papers_pending: HashMap<String, (crate::papers::Paper, bool)>,
    updater: crate::update::Updater,
    update_checked: bool,
    voice_stats: Option<(u32, u32, u32)>,
    show_subs: bool,
    sub_sel: (String, String),
    city_sel: Option<String>,
    city_query: String,
    city_welcome: Option<String>,
    map: MapState,
    transfers: Vec<cybercord_node::transfer::TransferInfo>,
    show_downloads: bool,
    /// open in-app document ("Words matter" etc.) and when the last hint toast was shown
    doc_open: Option<Doc>,
    /// editing one of my messages: (channel, message id, new version index) and the draft it replaced
    editing: Option<(ChannelId, String, u32)>,
    edit_draft: String,
    /// frames left to (re)request keyboard focus for the composer
    focus_input: u8,
    /// message under the pointer (last frame) and a pending \"delete?\" confirmation
    hover_msg: Option<String>,
    confirm_delete: Option<(String, Instant)>,
    /// selected text in the composer (char range) for the extensional tip
    sel: Option<(usize, usize)>,
    composer_rect: Rect,
    /// composer height with one line of text
    composer_base: f32,
    /// "levels of abstraction" view for one message: (author, text, time)
    analyze: Option<(String, String, u64)>,
    /// conversation banners hidden until …
    banner_hidden: HashMap<ChannelId, Instant>,
    builtin_gifs: Vec<String>,
    /// messages playing the "hacked text" effect, and when it started
    anims: HashMap<String, Instant>,
    started: Instant,
    /// open PDF reader
    pdf: Option<crate::pdfview::PdfView>,
    index_pick: bool,
    /// formula mode in the composer (LaTeX with live preview)
    math_mode: bool,
    /// editor for a scale or chart block (new, or one in my message)
    block_ed: Option<BlockEd>,
    /// start-up: intro video, then the splash (planet + motto)
    intro: Option<crate::intro::Intro>,
    /// signed profiles by peer id; whose profile is open; my profile draft
    profiles: HashMap<String, Profile>,
    profile_view: Option<String>,
    profile_draft: Option<Profile>,
    skill_query: String,
    avatar_tex: HashMap<String, (u32, egui::TextureHandle)>,
    avatar_rx: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    splash: Option<Instant>,
    preview_h: f32,
    /// picked image file from the system dialog (runs on its own thread)
    pick_rx: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    words_toast_at: Option<Instant>,
}

impl CyberApp {
    pub fn new(cc: &eframe::CreationContext<'_>, cfg: NetConfig) -> Self {
        crate::tray::set_native_window(cc);
        let settings = store::load_settings();
        theme::set(&settings.theme);
        install_theme(&cc.egui_ctx);
        let lang = i18n::resolve(&settings.lang);
        install_fonts(&cc.egui_ctx, lang);
        let name = cfg.name.clone();
        let mut handle = net::spawn(cfg);
        let tour = if settings.tour_done { None } else { Some(0) };
        // Events go through a pump thread: it wakes the UI and shows desktop
        // notifications even while the window is hidden in the tray.
        let notify = Arc::new(std::sync::Mutex::new(NotifyState { enabled: settings.notify, my_name: name.clone(), active: true, lang }));
        let (ev_tx, ev_rx) = std::sync::mpsc::channel();
        let net_rx = std::mem::replace(&mut handle.event_rx, ev_rx);
        spawn_event_pump(net_rx, ev_tx, cc.egui_ctx.clone(), notify.clone());
        // Tray (+ a router so tray actions work while no frames are drawn).
        let (tray_tx, tray_rx) = std::sync::mpsc::channel::<crate::tray::TrayCmd>();
        let (ui_tray_tx, ui_tray_rx) = std::sync::mpsc::channel::<crate::tray::TrayCmd>();
        let quitting = Arc::new(std::sync::atomic::AtomicBool::new(false));
        spawn_tray_router(tray_rx, ui_tray_tx, handle.cmd_tx.clone(), notify.clone(), quitting.clone());
        let tray = crate::tray::start(tray_state(&settings, lang, 0, 0), tray_tx, cc.egui_ctx.clone(), APP_ICON);
        Self {
            handle,
            self_id: String::new(),
            self_name: name,
            show_home: true,
            sel_server: 0,
            sel_channel: 0,
            views: HashMap::new(),
            opened: HashSet::new(),
            peers: BTreeMap::new(),
            hub: None,
            standby: None,
            status: String::new(),
            input: String::new(),
            in_voice: None,
            mic_on: false,
            talkers: HashMap::new(),
            show_friends: false,
            friends: BTreeMap::new(),
            dm_open: None,
            listen_addrs: Vec::new(),
            add_code: String::new(),
            add_name: String::new(),
            add_msg: String::new(),
            node_addr: String::new(),
            net: NetStats { scanning: true, ..Default::default() },
            resync: false,
            resync_again: None,
            settings,
            lang,
            show_settings: false,
            tour,
            tour_changed: Instant::now(),
            anchors: HashMap::new(),
            toast: None,
            emoji: EmojiCache::default(),
            media: MediaCache::default(),
            popup: Popup::None,
            popup_rect: Rect::NOTHING,
            emoji_tab: 0,
            emoji_query: String::new(),
            file_path: String::new(),
            progress: HashMap::new(),
            fetch_asked: HashSet::new(),
            circle_sound: None,
            rec: None,
            rec_tex: None,
            rec_error: None,
            icons: Icons::default(),
            roster: HashMap::new(),
            guide_step: 0,
            guide_changed: Instant::now(),
            music: Music::default(),
            tray,
            tray_rx: ui_tray_rx,
            quitting,
            notify,
            hidden: false,
            bg_hint_shown: false,
            tray_sig: String::new(),
            pending_minimize: false,
            demo_close_at: None,
            autoplay: false,
            vm: None,
            vm_last_level: Instant::now(),
            dev_lists: None,
            monitor: None,
            mic_heard: 0.0,
            set_tab: SetTab::Appearance,
            set_opened: Instant::now(),
            applied_theme: String::from("\u{0}"),
            nodes: Vec::new(),
            search_q: String::new(),
            search_res: Vec::new(),
            show_map: false,
            show_city: false,
            show_dev: false,
            started_ms: now_ms(),
            brand_played: false,
            sfx: Vec::new(),
            sfx_last: HashMap::new(),
            call: CallState::Idle,
            call_tone_at: None,
            show_invite: false,
            show_add_friend: false,
            e2e_peers: HashSet::new(),
            mail_kept: (0, 0),
            pending_handle: None,
            invite_qr: None,
            show_papers: false,
            papers: crate::papers::Search::default(),
            papers_q: String::new(),
            papers_src: crate::papers::Source::OpenAlex,
            papers_oa: false,
            papers_res: Vec::new(),
            papers_busy: false,
            papers_err: None,
            papers_expanded: HashSet::new(),
            papers_pending: HashMap::new(),
            updater: crate::update::Updater::default(),
            update_checked: false,
            voice_stats: None,
            show_subs: false,
            sub_sel: (String::new(), String::new()),
            city_sel: None,
            city_query: String::new(),
            city_welcome: None,
            map: MapState::default(),
            transfers: Vec::new(),
            show_downloads: false,
            doc_open: None,
            editing: None,
            edit_draft: String::new(),
            focus_input: 0,
            hover_msg: None,
            confirm_delete: None,
            sel: None,
            composer_rect: Rect::NOTHING,
            composer_base: f32::INFINITY,
            analyze: None,
            banner_hidden: HashMap::new(),
            builtin_gifs: Vec::new(),
            anims: HashMap::new(),
            started: Instant::now(),
            pdf: None,
            index_pick: false,
            math_mode: false,
            block_ed: None,
            intro: None,
            profiles: HashMap::new(),
            profile_view: None,
            profile_draft: None,
            skill_query: String::new(),
            avatar_tex: HashMap::new(),
            avatar_rx: None,
            splash: None,
            preview_h: 0.0,
            pick_rx: None,
            words_toast_at: None,
            mic_test_msg: None,
            test_player: None,
        }
        .with_start_view()
    }

    /// Optional start view via $CYBERCORD_START, for demos and screenshots:
    /// "friends", "dm:<peer>", "chat", "tour:<n>", "emoji", "gifs", "rec", "settings".
    fn with_start_view(mut self) -> Self {
        crate::update::cleanup_old();
        if let Some((from, to, backup)) = crate::update::take_last_note() {
            self.toast(self.t("upd_done_note").replace("{a}", &from).replace("{b}", &to).replace("{p}", &backup));
        }
        // join the gossip topics of my city subscriptions
        for slug in self.settings.subs.clone() {
            for (c, _, _) in cybercord_node::city::TEXT {
                let _ = self.handle.cmd_tx.send(Cmd::Subscribe(ChannelId::new(cybercord_node::city::guild(&slug), *c)));
            }
        }
        if let Ok(v) = std::env::var("CYBERCORD_START") {
            for part in v.split(',') {
                match part {
                    "city" => {
                        self.show_city = true;
                        self.show_papers = false;
                        self.show_home = false;
                        self.city_sel = Some("physics".into());
                    }
                    "subs" if !self.settings.subs.is_empty() => {
                        let b = self.settings.subs[0].clone();
                        self.open_branch(&b, "chat");
                    }
                    "friends" => {
                        self.show_home = false;
                        self.show_friends = true;
                    }
                    "chat" => self.show_home = false,
                    "emoji" => self.popup = Popup::Emoji,
                    "gifs" => self.popup = Popup::Gifs,
                    "settings" => self.show_settings = true,
                    "play" => self.autoplay = true,
                    "vm" => self.start_voice_message(),
                    "mictest" => {
                        self.show_settings = true;
                        self.set_tab = SetTab::Voice;
                        self.monitor = voice_rec::Monitor::start();
                    }
                    "voicetab" => self.set_tab = SetTab::Voice,
                    "downloads" => {
                        self.show_downloads = true;
                        self.show_home = false;
                    }
                    p if p.starts_with("doc:") => self.doc_open = Doc::from_id(&p[4..]),
                    p if p.starts_with("pdf:") => self.open_builtin_pdf(if &p[4..] == "gs" { "general-semantics.pdf" } else { "venus-project.pdf" }),
                    "dev" => {
                        self.settings.dev = true;
                        self.show_dev = true;
                    }
                    "invite" => self.show_invite = true,
                    "addfriend" => self.show_add_friend = true,
                    "proftab" => {
                        self.show_settings = true;
                        self.set_tab = SetTab::Profile;
                    }
                    "wordstab" => {
                        self.show_settings = true;
                        self.set_tab = SetTab::Help;
                    }
                    "bgtab" => {
                        self.show_settings = true;
                        self.set_tab = SetTab::Background;
                    }
                    p if p.starts_with("closeafter:") => {
                        let secs: u64 = p[11..].parse().unwrap_or(3);
                        self.demo_close_at = Some(Instant::now() + Duration::from_secs(secs));
                    }
                    "map" => {
                        self.show_map = true;
                            self.show_city = false;
                            self.show_papers = false;
                            self.show_subs = false;
                        self.show_home = false;
                    }
                    p if p.starts_with("find:") => {
                        self.show_home = false;
                        self.show_friends = true;
                        self.search_q = p[5..].to_string();
                        let _ = self.handle.cmd_tx.send(Cmd::Search(self.search_q.clone()));
                    }
                    p if p.starts_with("select:") => self.map.selected = Some(p[7..].to_string()),
                    p if p.starts_with("theme:") => {
                        self.settings.theme = p[6..].to_string();
                        theme::set(&self.settings.theme);
                    }
                    "rec" => self.rec = Some(camera::Recorder::start()),
                    p if p.starts_with("dm:") => {
                        self.show_home = false;
                        self.show_friends = true;
                        self.dm_open = Some(p[3..].to_string());
                    }
                    p if p.starts_with("tour:") => {
                        let n: usize = p[5..].parse().unwrap_or(0);
                        self.go_tour(n.min(TOUR.len() - 1));
                    }
                    p if p.starts_with("notour") => self.tour = None,
                    p if p.starts_with("voice:") => {
                        if let Some((g, c)) = p[6..].split_once('/') {
                            let _ = self.handle.cmd_tx.send(Cmd::JoinVoice { channel: ChannelId::new(g, c) });
                        }
                    }
                    _ => {}
                }
            }
        }
        self
    }

    fn t(&self, k: &'static str) -> &'static str {
        i18n::tr(self.lang, k)
    }

    fn td(&self, k: &str) -> &'static str {
        i18n::tr_dyn(self.lang, k)
    }

    /// A channel's name in the interface language (the network id stays the slug).
    fn ch_label(&self, slug: &str) -> String {
        if !self.settings.ch_translate {
            return slug.to_string();
        }
        let t = self.td(&format!("ch_{slug}"));
        if t.is_empty() { slug.to_string() } else { t.to_string() }
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn active_channel(&self) -> ChannelId {
        match (&self.dm_open, self.show_friends) {
            (Some(p), true) => ChannelId::dm(p.clone()),
            _ => self.current_channel(),
        }
    }

    fn chat_visible(&self) -> bool {
        !self.show_downloads && !self.show_map && !self.show_city && !self.show_papers && !self.show_home && (!self.show_friends || self.dm_open.is_some())
    }

    fn friend_name(&self, peer: &str) -> String {
        self.friends.get(peer).map(|f| f.name.clone()).unwrap_or_else(|| short(peer))
    }

    fn connect_code(&self) -> String {
        let mut v: Vec<&String> = self.listen_addrs.iter().collect();
        v.sort_by_key(|a| a.contains("/127.0.0.1/"));
        v.into_iter().cloned().collect::<Vec<_>>().join("\n")
    }

    fn current_channel(&self) -> ChannelId {
        if self.show_subs && !self.sub_sel.0.is_empty() {
            return ChannelId::new(cybercord_node::city::guild(&self.sub_sel.0), self.sub_sel.1.clone());
        }
        let s = &SERVERS[self.sel_server];
        ChannelId::new(s.slug, s.channels[self.sel_channel])
    }

    fn anchor(&mut self, key: &'static str, rect: Rect) {
        self.anchors.insert(key, rect);
    }

    // ------------------------------------------------------------- events

    fn drain_events(&mut self) {
        while let Ok(ev) = self.handle.event_rx.try_recv() {
            match ev {
                UiEvent::Profile(p) => {
                    self.profiles.insert(p.peer.clone(), p);
                }
                UiEvent::Ready { peer_id, name } => {
                    self.self_id = peer_id;
                    self.self_name = name;
                }
                UiEvent::Listening(a) => {
                    if !self.listen_addrs.contains(&a) {
                        self.listen_addrs.push(a);
                    }
                }
                UiEvent::Friend { peer_id, name, online } => {
                    if online {
                        self.resync = true;
                    }
                    self.friends.insert(peer_id, FriendView { name, online });
                }
                UiEvent::Net(n) => self.net = n,
                UiEvent::BlobProgress { hash, got, total } => {
                    self.progress.insert(hash, (got, total));
                }
                UiEvent::BlobReady { hash } => {
                    self.progress.remove(&hash);
                }
                UiEvent::Status(s) => self.status = s,
                UiEvent::Chat(ev) => {
                    if ev.ts_ms + 15_000 > now_ms() && ev.ts_ms + 3_000 > self.started_ms && ev.from != self.self_id && ev.name != self.self_name && ev.edits.is_none() && !ev.deleted {
                        let mention = self.self_name.chars().count() >= 2 && ev.body.to_lowercase().contains(&self.self_name.to_lowercase());
                        if ev.channel.is_dm() || mention {
                            self.play_sfx(crate::sounds::Sfx::Mention);
                        } else if ev.channel != self.active_channel() || !self.chat_visible() || self.hidden {
                            self.play_sfx(crate::sounds::Sfx::Message);
                        }
                    }
                    if self.settings.hack_fx && ev.ts_ms + 10_000 > now_ms() && !ev.deleted {
                        self.anims.insert(ev.edits.clone().unwrap_or_else(|| ev.id.clone()), Instant::now());
                    }
                    let view = self.views.entry(ev.channel.clone()).or_default();
                    view.add(ev);
                    view.msgs.sort_by_key(|m| m.ts_ms);
                }
                UiEvent::Peer { id, name, connected } => {
                    if connected && !self.peers.get(&id).map(|p| p.connected).unwrap_or(false) {
                        self.resync = true;
                    }
                    let entry = self.peers.entry(id).or_insert(PeerView { name: None, connected: false });
                    entry.connected = connected;
                    if name.is_some() {
                        entry.name = name;
                    }
                }
                UiEvent::Hub { hub, standby } => {
                    self.hub = hub;
                    self.standby = standby;
                }
                UiEvent::VoiceJoined { room } => {
                    let was = self.in_voice.clone();
                    if room.is_some() && was != room && !matches!(self.call, CallState::Outgoing { .. } | CallState::Active { .. }) {
                        self.play_sfx(crate::sounds::Sfx::VoiceJoin);
                    }
                    if room.is_none() && was.is_some() {
                        // left the room: a call in it is over
                        match std::mem::replace(&mut self.call, CallState::Idle) {
                            CallState::Outgoing { peer, id, .. } => {
                                let _ = self.handle.cmd_tx.send(Cmd::Call { peer, id, kind: CallKind::Cancel, round: 0 });
                                self.play_sfx(crate::sounds::Sfx::CallEnd);
                            }
                            CallState::Active { peer, id, .. } => {
                                let _ = self.handle.cmd_tx.send(Cmd::Call { peer, id, kind: CallKind::End, round: 0 });
                                self.play_sfx(crate::sounds::Sfx::CallEnd);
                            }
                            other => {
                                self.call = other;
                                self.play_sfx(crate::sounds::Sfx::VoiceLeave);
                            }
                        }
                    }
                    self.in_voice = room;
                    if self.in_voice.is_none() {
                        self.mic_on = false;
                    }
                }
                UiEvent::Nodes(list) => self.nodes = list,
                UiEvent::Transfers(list) => self.transfers = list,
                UiEvent::Search { query, users } => {
                    if let Some((tag, nm, _)) = self.pending_handle.clone() {
                        if let Some(u) = users.iter().find(|u| cybercord_node::types::handle_tag(&u.peer) == tag) {
                            let name = if nm.is_empty() { u.name.clone() } else { nm };
                            self.pending_handle = None;
                            self.befriend(&u.peer.clone(), &name, true);
                            self.add_msg = self.t("added").replace("{name}", &name);
                            self.show_add_friend = false;
                        }
                    }
                    if query == self.search_q.trim() {
                        self.search_res = users;
                    }
                }
                UiEvent::VoiceRoster { peer, name, room } => {
                    if peer != self.self_id {
                        let before = self.roster.get(&peer).and_then(|r| r.1.clone());
                        if let Some(mine) = self.in_voice.clone() {
                            if room.as_deref() == Some(mine.as_str()) && before.as_deref() != Some(mine.as_str()) {
                                self.play_sfx(crate::sounds::Sfx::PeerJoin);
                            } else if before.as_deref() == Some(mine.as_str()) && room.as_deref() != Some(mine.as_str()) {
                                self.play_sfx(crate::sounds::Sfx::PeerLeave);
                            }
                        }
                    }
                    self.roster.insert(peer, (name, room, now_ms()));
                }
                UiEvent::Talking { name } => {
                    self.talkers.insert(name, now_ms());
                }
                UiEvent::Call { peer, name, id, kind } => self.on_call(peer, name, id, kind),
                UiEvent::E2e { peer } => {
                    self.e2e_peers.insert(peer);
                }
                UiEvent::Mail { kept, users } => self.mail_kept = (kept, users),
                UiEvent::VoiceStats { sent, recv, buffer_ms } => {
                    self.voice_stats = Some((sent, recv, buffer_ms));
                }
            }
        }
    }

    fn ensure_open(&mut self, ch: &ChannelId) {
        if self.opened.contains(ch) {
            return;
        }
        self.opened.insert(ch.clone());
        let recent = store::load_recent(ch, net::HISTORY_ON_OPEN_PUB);
        let view = self.views.entry(ch.clone()).or_default();
        for ev in recent {
            view.add(ev);
        }
        view.msgs.sort_by_key(|m| m.ts_ms);
        let since = view.msgs.last().map(|m| m.ts_ms).unwrap_or(0);
        let _ = self.handle.cmd_tx.send(Cmd::RequestSync { channel: ch.clone(), since_ms: since });
    }

    fn role_of(&self, id: &str) -> Option<&'static str> {
        if self.hub.as_deref() == Some(id) {
            Some(self.t("role_hub"))
        } else if self.standby.as_deref() == Some(id) {
            Some(self.t("role_standby"))
        } else {
            None
        }
    }

    fn send_current(&mut self) {
        let body = emoji::shortcodes_to_unicode(self.input.trim());
        if let Some((channel, target, rev)) = self.editing.take() {
            self.input = std::mem::take(&mut self.edit_draft);
            let current = self.views.get(&channel).and_then(|v| v.msgs.iter().find(|m| m.id == target).map(|m| cybercord_node::history::resolve(m, &v.edits).body));
            if !body.is_empty() && current.as_deref() != Some(body.as_str()) {
                self.words_check(&body);
                let _ = self.handle.cmd_tx.send(Cmd::EditMessage { channel, target, body: Some(body), rev });
            }
            return;
        }
        if body.is_empty() {
            return;
        }
        self.input.clear();
        self.words_check(&body);
        let ch = self.active_channel();
        self.play_sfx(crate::sounds::Sfx::Sent);
        if ch.is_dm() {
            let _ = self.handle.cmd_tx.send(Cmd::SendDm { peer_id: ch.channel, body });
        } else {
            let _ = self.handle.cmd_tx.send(Cmd::SendChat { channel: ch, body });
        }
    }

    /// Put one of my messages into the composer for editing.
    fn start_edit(&mut self, ch: &ChannelId, id: &str) {
        let Some(view) = self.views.get(ch) else { return };
        let Some(m) = view.msgs.iter().find(|m| m.id == id) else { return };
        if m.from != self.self_id {
            return;
        }
        let r = cybercord_node::history::resolve(m, &view.edits);
        if self.editing.is_none() {
            self.edit_draft = std::mem::take(&mut self.input);
        }
        self.input = r.body;
        self.editing = Some((ch.clone(), id.to_string(), r.rev + 1));
        self.focus_input = 3;
    }

    fn cancel_edit(&mut self) {
        if self.editing.take().is_some() {
            self.input = std::mem::take(&mut self.edit_draft);
        }
    }

    fn delete_message(&mut self, ch: &ChannelId, id: &str) {
        let Some(view) = self.views.get(ch) else { return };
        let Some(m) = view.msgs.iter().find(|m| m.id == id) else { return };
        if m.from != self.self_id {
            return;
        }
        let rev = cybercord_node::history::resolve(m, &view.edits).rev + 1;
        if self.editing.as_ref().map(|e| e.1 == id).unwrap_or(false) {
            self.cancel_edit();
        }
        self.confirm_delete = None;
        let _ = self.handle.cmd_tx.send(Cmd::EditMessage { channel: ch.clone(), target: id.to_string(), body: None, rev });
    }

    /// "Words matter": the message is always sent as written. If it contains
    /// an insult, the sender (only) gets a document the 1st and 10th time and
    /// a quiet one-line hint otherwise (at most every 10 minutes).
    fn words_check(&mut self, body: &str) {
        if !self.settings.words_hints || !cybercord_node::words::is_insult(body) {
            return;
        }
        self.settings.words_count = self.settings.words_count.saturating_add(1);
        store::save_settings(&self.settings);
        match self.settings.words_count {
            1 => self.doc_open = Some(Doc::WordsMatter),
            10 => self.doc_open = Some(Doc::Consequences),
            _ => {
                if self.words_toast_at.map_or(true, |t| t.elapsed() > Duration::from_secs(600)) {
                    self.words_toast_at = Some(Instant::now());
                    let m = self.t("words_toast").to_string();
                    self.toast(m);
                }
            }
        }
    }

    /// The document reader, drawn over everything (also over settings).
    fn doc_overlay(&mut self, ctx: &egui::Context) {
        if self.pdf.is_some() {
            return;
        }
        let Some(doc) = self.doc_open else { return };
        let mut open_link: Option<String> = None;
        let c = th();
        let screen = ctx.screen_rect();
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Area::new(Id::new("doc_dim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let r = ui.allocate_rect(screen, egui::Sense::click());
            ui.painter().rect_filled(screen, Rounding::ZERO, Color32::from_black_alpha(150));
            if r.clicked() {
                close = true;
            }
        });
        let w = (screen.width() - 48.0).clamp(320.0, 720.0);
        let h = screen.height() - 56.0;
        let md = doc.text(self.lang);
        egui::Area::new(Id::new("doc_win")).order(Order::Tooltip).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Frame::none().fill(c.window).rounding(Rounding::same(14.0)).stroke(Stroke::new(1.0, c.line)).inner_margin(egui::Margin::same(0.0)).shadow(egui::epaint::Shadow { offset: egui::vec2(0.0, 8.0), blur: 28.0, spread: 0.0, color: Color32::from_black_alpha(90) }).show(ui, |ui| {
                ui.set_width(w);
                ui.set_height(h);
                // header
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.set_height(48.0);
                    if doc == Doc::Sce {
                        docs::brand(ui, "SocioCyberneering Inc.", 13.0, c.ink2);
                    } else if doc == Doc::Atom {
                        ui.label(RichText::new("Atom-Generator").color(c.ink2).size(13.0).b());
                    } else {
                        ui.label(RichText::new(self.t("words_h")).color(c.ink2).size(13.0).b());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        if ui.add(egui::Button::new(RichText::new(self.t("close")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(96.0, 30.0))).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add(egui::Separator::default().spacing(0.0));
                egui::ScrollArea::vertical().id_salt(("doc", doc as u8)).auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::none().inner_margin(egui::Margin { left: 32.0, right: 32.0, top: 22.0, bottom: 28.0 }).show(ui, |ui| {
                        let icons = &mut self.icons;
                        if let Some(link) = docs::show(ui, md, &mut |ctx, key, png| icons.get(ctx, key, png)) {
                            open_link = Some(link);
                        }
                        ui.add_space(14.0);
                        if matches!(doc, Doc::WordsMatter | Doc::Consequences) {
                            ui.colored_label(c.ink2, RichText::new(self.t("words_private")).small());
                        }
                    });
                });
            });
        });
        if close {
            self.doc_open = None;
        }
        if let Some(l) = open_link {
            self.open_builtin_pdf(&l);
        }
    }

    /// Intro video, then the splash. Returns true while one of them covers the window.
    fn startup_screens(&mut self, ctx: &egui::Context) -> bool {
        if self.intro.is_none() && self.splash.is_none() {
            return false;
        }
        let skip_key = ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
        let mut skip = false;
        let skip_label = self.t("intro_skip");
        if let Some(intro) = self.intro.as_mut() {
            let playing = egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::BLACK)).show(ctx, |ui| {
                let rect = ui.max_rect();
                let go = intro.show(ui, rect);
                // progress line and the skip button
                let k = (intro.elapsed() / intro.duration.max(1.0)).clamp(0.0, 1.0) as f32;
                ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(rect.min.x, rect.max.y - 3.0), egui::vec2(rect.width() * k, 3.0)), 0.0, th().accent);
                let b = egui::Rect::from_min_size(egui::pos2(rect.max.x - 150.0, rect.max.y - 56.0), egui::vec2(130.0, 36.0));
                if ui.put(b, egui::Button::new(RichText::new(skip_label).color(Color32::WHITE).strong()).fill(Color32::from_black_alpha(140)).stroke(Stroke::new(1.0, Color32::from_white_alpha(90))).rounding(Rounding::same(18.0))).clicked() {
                    skip = true;
                }
                go
            });
            if !playing.inner || skip || skip_key {
                self.intro = None;
                self.splash = Some(Instant::now());
                if !self.settings.intro_seen {
                    self.settings.intro_seen = true;
                    store::save_settings(&self.settings);
                }
            }
            return true;
        }
        let Some(at) = self.splash else { return false };
        let t = at.elapsed().as_secs_f32();
        const LEN: f32 = 2.6;
        let clicked = ctx.input(|i| i.pointer.any_click());
        if t > LEN || skip_key || clicked {
            self.splash = None;
            return false;
        }
        let fade = if t > LEN - 0.45 { (LEN - t) / 0.45 } else { 1.0 };
        let a = |c: Color32| c.gamma_multiply(fade.clamp(0.0, 1.0));
        let motto = self.t("splash_motto");
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(0x0b, 0x12, 0x20))).show(ctx, |ui| {
            let r = ui.max_rect();
            let p = ui.painter();
            // soft glow behind the planet
            {
                // radial glow: a triangle fan from a bright centre to a transparent rim
                let c0 = r.center() - egui::vec2(0.0, 60.0);
                let mut mesh = egui::epaint::Mesh::default();
                mesh.colored_vertex(c0, a(Color32::from_rgba_premultiplied(30, 70, 125, 125)));
                let n = 96;
                for k in 0..=n {
                    let ang = std::f32::consts::TAU * k as f32 / n as f32;
                    mesh.colored_vertex(c0 + egui::vec2(ang.cos(), ang.sin()) * 260.0, Color32::TRANSPARENT);
                }
                for k in 1..=n as u32 {
                    mesh.add_triangle(0, k, k + 1);
                }
                p.add(egui::Shape::mesh(mesh));
            }
            let grow = (t / 0.6).min(1.0);
            let s = 150.0 * (0.8 + 0.2 * ease_out(grow));
            let tex = self.icons.get(ui.ctx(), "app256", APP_ICON_256);
            p.image(tex, egui::Rect::from_center_size(r.center() - egui::vec2(0.0, 60.0), egui::vec2(s, s)), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), a(Color32::WHITE));
            let brand = |size: f32| egui::FontId::new(size, egui::FontFamily::Name("brand".into()));
            let job = |text: &str, size: f32, col: Color32, spacing: f32| {
                let mut j = egui::text::LayoutJob::default();
                j.append(text, 0.0, egui::TextFormat { font_id: brand(size), color: col, extra_letter_spacing: size * spacing, ..Default::default() });
                j.wrap.max_width = r.width() - 60.0;
                j.halign = egui::Align::Center;
                ui.fonts(|f| f.layout_job(j))
            };
            let g = job("CyberCord", 40.0, a(Color32::WHITE), 0.06);
            p.galley(egui::pos2(r.center().x, r.center().y + 40.0), g, Color32::WHITE);
            // the motto "decrypts" like the messages do
            let shown = if self.settings.hack_fx { crate::fx::scrambled(motto, (t - 0.3).max(0.0) * 0.55, 0x5ce) } else { motto.to_string() };
            let g = job(&shown, 17.0, a(Color32::from_rgb(0xc0, 0xd8, 0xf5)), 0.04);
            p.galley(egui::pos2(r.center().x, r.center().y + 100.0), g, Color32::WHITE);
            let g = job("SocioCyberneering Inc.", 13.0, a(Color32::from_white_alpha(170)), 0.12);
            p.galley(egui::pos2(r.center().x, r.max.y - 44.0), g, Color32::WHITE);
        });
        ctx.request_repaint();
        true
    }

    fn open_builtin_pdf(&mut self, file: &str) {
        if let Some((title, name, bytes)) = crate::pdfview::builtin(file) {
            self.pdf = Some(crate::pdfview::PdfView::open(title, name, Arc::new(bytes.to_vec())));
        }
    }

    fn open_pdf_file(&mut self, path: &std::path::Path) {
        match std::fs::read(path) {
            Ok(b) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "document.pdf".into());
                self.pdf = Some(crate::pdfview::PdfView::open(name.clone(), name, Arc::new(b)));
            }
            Err(e) => self.toast(format!("{}: {e}", path.display())),
        }
    }

    /// The PDF reader, over everything.
    fn pdf_overlay(&mut self, ctx: &egui::Context) {
        if self.pdf.is_none() {
            return;
        }
        let c = th();
        let screen = ctx.screen_rect();
        let labels = crate::pdfview::Labels { close: self.t("close"), save: self.t("pdf_save"), open: self.t("pdf_open"), page: self.t("pdf_page"), cannot: self.t("pdf_cannot") };
        let mut act = crate::pdfview::Action::None;
        egui::Area::new(Id::new("pdf_dim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_rect(screen, egui::Sense::click());
            ui.painter().rect_filled(screen, Rounding::ZERO, Color32::from_black_alpha(170));
        });
        let r = screen.shrink(16.0);
        egui::Area::new(Id::new("pdf_win")).order(Order::Tooltip).fixed_pos(r.min).show(ctx, |ui| {
            egui::Frame::none().fill(c.window).rounding(Rounding::same(12.0)).stroke(Stroke::new(1.0, c.line)).show(ui, |ui| {
                ui.set_width(r.width());
                ui.set_height(r.height());
                if let Some(p) = self.pdf.as_mut() {
                    act = p.show(ui, &labels);
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            act = crate::pdfview::Action::Close;
        }
        match act {
            crate::pdfview::Action::Close => self.pdf = None,
            crate::pdfview::Action::Save | crate::pdfview::Action::OpenExternal => {
                let Some(p) = self.pdf.as_ref() else { return };
                let dir = cybercord_node::transfer::download_dir();
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(&p.file_name);
                let ok = path.exists() || std::fs::write(&path, p.bytes()).is_ok();
                if matches!(act, crate::pdfview::Action::OpenExternal) && ok {
                    open_path(&path);
                } else if ok {
                    let m = self.t("pdf_saved").replace("{path}", &path.display().to_string());
                    self.toast(m);
                }
            }
            crate::pdfview::Action::None => {}
        }
    }

    /// Settings → Help: the "Words matter" block.
    fn words_section(&mut self, ui: &mut egui::Ui) {
        let c = th();
        ui.label(RichText::new(self.t("words_h").to_uppercase()).color(c.ink2).size(12.0).b());
        ui.add_space(6.0);
        let label = self.t("words_opt");
        if ui.checkbox(&mut self.settings.words_hints, label).changed() {
            store::save_settings(&self.settings);
        }
        ui.add_space(4.0);
        ui.colored_label(c.ink2, self.t("words_note"));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for (doc, key) in [(Doc::WordsMatter, "words_doc1"), (Doc::Consequences, "words_doc2"), (Doc::Sce, "sce_doc")] {
                if ui.add(egui::Button::new(RichText::new(self.t(key)).color(c.ink)).fill(c.card).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 32.0))).clicked() {
                    self.doc_open = Some(doc);
                }
            }
        });
        if self.settings.words_count > 0 {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.colored_label(c.ink2, RichText::new(self.t("words_count").replace("{n}", &self.settings.words_count.to_string())).small());
                if ui.small_button(self.t("words_reset")).clicked() {
                    self.settings.words_count = 0;
                    store::save_settings(&self.settings);
                }
            });
        }
    }

    /// Send a GIF / image file (from a path or dropped bytes) to the open room.
    /// Send a file from disk: small pictures/GIFs/music inline, anything else
    /// (any type, up to 8 GB) as a torrent-like shared file.
    fn send_path(&mut self, path: std::path::PathBuf) {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut head = [0u8; 64];
        let n = std::fs::File::open(&path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        let inline = size as usize <= MAX_BLOB && matches!(media::sniff(&head[..n]), Some(MediaKind::Gif | MediaKind::Image | MediaKind::Audio));
        if inline {
            if let Ok(b) = std::fs::read(&path) {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.send_file(name, b);
                return;
            }
        }
        if !self.chat_visible() {
            self.show_home = false;
        }
        let channel = self.active_channel();
        let _ = self.handle.cmd_tx.send(Cmd::ShareFile { channel, path, body: String::new() });
        self.popup = Popup::None;
        let m = self.t("file_sharing_prep").to_string();
        self.toast(m);
    }

    fn send_file(&mut self, name: String, bytes: Vec<u8>) {
        if !self.chat_visible() {
            self.show_home = false;
        }
        let kind = media::sniff(&bytes);
        match kind {
            Some(MediaKind::Audio) if bytes.len() <= MAX_BLOB => {
                // Read tags + length off the UI thread, then send.
                let channel = self.active_channel();
                let tx = self.music.tx.clone();
                std::thread::spawn(move || {
                    let msg = match music::describe(&bytes, &name) {
                        Ok((label, dur_ms)) => MusicMsg::Send { channel, name: label, bytes, dur_ms },
                        Err(_) => MusicMsg::SendFailed,
                    };
                    let _ = tx.send(msg);
                });
                self.popup = Popup::None;
            }
            Some(kind) if bytes.len() <= MAX_BLOB && kind != MediaKind::Circle => {
                let (w, h) = media::dimensions(&bytes);
                let channel = self.active_channel();
                let _ = self.handle.cmd_tx.send(Cmd::SendMedia { channel, body: String::new(), kind, name, bytes, w, h, dur_ms: 0 });
                self.popup = Popup::None;
            }
            _ => {
                let msg = self.t("file_bad").to_string();
                self.toast(msg);
            }
        }
    }

    // ------------------------------------------------------------- tour

    fn go_tour(&mut self, step: usize) {
        self.tour = Some(step);
        self.tour_changed = Instant::now();
        self.popup = Popup::None;
        self.show_map = false;
                            self.show_city = false;
                            self.show_papers = false;
                            self.show_subs = false;
        match TOUR[step].3 {
            Need::Home => {
                self.show_home = true;
                self.show_friends = false;
            }
            Need::Friends => {
                self.show_home = false;
                self.show_friends = true;
                self.dm_open = None;
            }
            Need::Chat => {
                self.show_home = false;
                self.show_friends = false;
                self.sel_server = 0;
                self.sel_channel = 0;
            }
            Need::Any => {}
        }
    }

    fn end_tour(&mut self) {
        self.tour = None;
        self.settings.tour_done = true;
        store::save_settings(&self.settings);
        self.show_home = true;
        self.show_friends = false;
    }

    fn draw_tour(&mut self, ctx: &egui::Context) {
        let Some(step) = self.tour else { return };
        let (title, body, anchor, _) = TOUR[step];
        let screen = ctx.screen_rect();
        let target = anchor.and_then(|a| self.anchors.get(a)).copied().map(|r| r.expand(6.0).intersect(screen));
        let time = ctx.input(|i| i.time) as f32;
        let age = self.tour_changed.elapsed().as_secs_f32();
        let k = ease_out(age / 0.35);

        // Dim everything except the highlighted element; eat clicks under the overlay.
        egui::Area::new(Id::new("tour_block")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_rect(screen, egui::Sense::click());
        });
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("tour_dim")));
        let dim = Color32::from_black_alpha((120.0 * k.min(1.0)) as u8);
        if let Some(r) = target {
            painter.rect_filled(Rect::from_min_max(screen.min, egui::pos2(screen.max.x, r.min.y)), 0.0, dim);
            painter.rect_filled(Rect::from_min_max(egui::pos2(screen.min.x, r.max.y), screen.max), 0.0, dim);
            painter.rect_filled(Rect::from_min_max(egui::pos2(screen.min.x, r.min.y), egui::pos2(r.min.x, r.max.y)), 0.0, dim);
            painter.rect_filled(Rect::from_min_max(egui::pos2(r.max.x, r.min.y), egui::pos2(screen.max.x, r.max.y)), 0.0, dim);
            let pulse = 0.5 + 0.5 * (time * 3.5).sin();
            painter.rect_stroke(r, Rounding::same(12.0), Stroke::new(2.0 + 2.5 * pulse, th().accent.gamma_multiply(0.55 + 0.45 * pulse)));
            painter.rect_stroke(r.expand(5.0 + 5.0 * pulse), Rounding::same(16.0), Stroke::new(1.5, th().peri.gamma_multiply(0.6 * (1.0 - pulse))));
        } else {
            painter.rect_filled(screen, 0.0, dim);
        }

        // Card next to the target (right, left, below, above), sliding in.
        let card_w = 360.0;
        let card_h = 210.0;
        let pos = match target {
            None => screen.center() - egui::vec2(card_w / 2.0, card_h / 2.0),
            Some(r) => {
                if r.max.x + 16.0 + card_w < screen.max.x {
                    egui::pos2(r.max.x + 16.0, (r.center().y - card_h / 2.0).clamp(screen.min.y + 8.0, screen.max.y - card_h - 8.0))
                } else if r.min.x - 16.0 - card_w > screen.min.x {
                    egui::pos2(r.min.x - 16.0 - card_w, (r.center().y - card_h / 2.0).clamp(screen.min.y + 8.0, screen.max.y - card_h - 8.0))
                } else if r.min.y - card_h - 16.0 > screen.min.y {
                    egui::pos2((r.center().x - card_w / 2.0).clamp(8.0, screen.max.x - card_w - 8.0), r.min.y - card_h - 16.0)
                } else {
                    egui::pos2((r.center().x - card_w / 2.0).clamp(8.0, screen.max.x - card_w - 8.0), r.max.y + 16.0)
                }
            }
        };
        let pos = pos + egui::vec2(0.0, (1.0 - k) * 22.0);
        let n = TOUR.len();
        let mut action: Option<i32> = None; // -1 back, +1 next, 0 skip
        egui::Area::new(Id::new(("tour_card", step))).order(Order::Tooltip).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::none()
                .fill(th().window)
                .stroke(Stroke::new(1.0, th().line))
                .rounding(Rounding::same(16.0))
                .shadow(egui::epaint::Shadow { offset: egui::vec2(0.0, 8.0), blur: 28.0, spread: 0.0, color: Color32::from_black_alpha(60) })
                .inner_margin(egui::Margin::same(18.0))
                .show(ui, |ui| {
                    ui.set_width(card_w - 36.0);
                    ui.horizontal(|ui| {
                        // progress dots
                        for i in 0..n {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(if i == step { 18.0 } else { 7.0 }, 7.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, Rounding::same(4.0), if i <= step { th().accent } else { th().line });
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let s = self.t("tour_step").replace("{i}", &(step + 1).to_string()).replace("{n}", &n.to_string());
                            ui.colored_label(th().ink2, RichText::new(s).small());
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new(self.t(title)).color(th().ink).b().size(19.0));
                    ui.add_space(4.0);
                    self.rich_text(ui, self.t(body), th().ink2, 14.0);
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if step > 0 && ui.add(egui::Button::new(RichText::new(self.t("tour_back")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0))).clicked() {
                            action = Some(-1);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let last = step + 1 == n;
                            let label = if last { self.t("tour_finish") } else { self.t("tour_next") };
                            if ui.add(egui::Button::new(RichText::new(label).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(96.0, 30.0))).clicked() {
                                action = Some(1);
                            }
                            if !last && ui.add(egui::Button::new(RichText::new(self.t("tour_skip")).color(th().ink2)).frame(false)).clicked() {
                                action = Some(0);
                            }
                        });
                    });
                });
        });
        match action {
            Some(1) if step + 1 < n => self.go_tour(step + 1),
            Some(1) | Some(0) => self.end_tour(),
            Some(-1) => self.go_tour(step - 1),
            _ => {}
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.end_tour();
        }
        ctx.request_repaint();
    }

    // ------------------------------------------------------------- text with emoji

    /// Text with color emoji inline (Unicode or :shortcodes:).
    fn rich_text(&mut self, ui: &mut egui::Ui, text: &str, color: Color32, size: f32) {
        let segs = emoji::segment(text);
        let big = emoji::is_emoji_only(&segs);
        let esize = if big { 40.0 } else { size + 6.0 };
        let ctx = ui.ctx().clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for s in segs {
                match s {
                    Seg::Text(t) => {
                        ui.label(RichText::new(t).color(color).size(size));
                    }
                    Seg::Emoji(i) => {
                        let tex = self.emoji.tex(&ctx, i);
                        ui.add(egui::Image::new(SizedTexture::new(tex, egui::vec2(esize, esize))))
                            .on_hover_text(format!(":{}:", emoji::code(i)));
                    }
                }
            }
        });
    }
}

impl eframe::App for CyberApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        crate::dev::stop_all();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.drain_papers();
        if !self.brand_played && !self.hidden && self.started.elapsed().as_millis() > 300 {
            self.brand_played = true;
            self.play_sfx(crate::sounds::Sfx::Brand);
        }
        if !self.update_checked && self.settings.update_check && self.started.elapsed().as_secs() >= 10 {
            self.update_checked = true;
            self.updater.check(ctx.clone(), true);
        }
        self.anims.retain(|_, t| t.elapsed().as_secs_f32() < 2.0);
        self.background_and_tray(ctx);
        self.media.poll(ctx);
        self.poll_music();
        // theme / zoom changes take effect on the next frame
        if self.applied_theme != self.settings.theme {
            self.applied_theme = self.settings.theme.clone();
            theme::set(&self.settings.theme);
            install_theme(ctx);
        }
        let zoom = (self.settings.ui_scale / 100.0).clamp(0.7, 1.6);
        if (ctx.zoom_factor() - zoom).abs() > 0.001 && !ctx.input(|i| i.pointer.any_down()) {
            ctx.set_zoom_factor(zoom);
        }
        if self.startup_screens(ctx) {
            return;
        }
        if self.show_settings {
            self.settings_screen(ctx);
            self.doc_overlay(ctx);
            self.pdf_overlay(ctx);
            self.draw_toast(ctx);
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        } else if self.monitor.is_some() {
            self.monitor = None;
        }
        let ch = self.active_channel();
        if self.chat_visible() {
            self.ensure_open(&ch);
            let again = self.resync_again.map(|t| t.elapsed() > Duration::from_secs(4)).unwrap_or(false);
            if self.resync || again {
                self.resync_again = if self.resync { Some(Instant::now()) } else { None };
                self.resync = false;
                let since = self.views.get(&ch).and_then(|v| v.msgs.last()).map(|m| m.ts_ms).unwrap_or(0);
                let _ = self.handle.cmd_tx.send(Cmd::RequestSync { channel: ch.clone(), since_ms: since });
            }
        }

        // Files dropped onto the window are sent to the open room.
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            if let Some(p) = &f.path {
                self.send_path(p.clone());
                continue;
            }
            let bytes = match (&f.bytes, &f.path) {
                (Some(b), _) => Some(b.to_vec()),
                (None, Some(p)) => std::fs::read(p).ok(),
                _ => None,
            };
            let name = f.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| f.name.clone());
            if let Some(b) = bytes {
                self.send_file(name, b);
            }
        }

        self.rail(ctx);
        if self.show_subs {
            self.subs_column(ctx);
        } else {
            self.channel_column(ctx, &ch);
        }
        if !self.show_map && !self.show_city && !self.show_papers {
            self.member_panel(ctx, &ch);
        }
        self.bottom_bar(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(th().window).inner_margin(egui::Margin::same(16.0)))
            .show(ctx, |ui| {
                if self.show_downloads {
                    self.downloads_view(ui);
                } else if self.show_map {
                    self.map_view(ui);
                } else if self.show_city {
                    self.city_view(ui);
                } else if self.show_papers {
                    self.papers_view(ui);
                } else if self.show_home {
                    self.home_view(ui);
                } else if self.show_friends && self.dm_open.is_none() {
                    self.friends_home(ui);
                } else {
                    self.chat_view(ui, &ch);
                }
            });

        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("drop")));
            let r = ctx.screen_rect().shrink(24.0);
            painter.rect_filled(r, Rounding::same(18.0), th().peri.gamma_multiply(0.35));
            painter.rect_stroke(r, Rounding::same(18.0), Stroke::new(3.0, th().accent));
            painter.text(r.center(), Align2::CENTER_CENTER, self.t("drop_here"), egui::FontId::proportional(28.0), th().accent);
        }

        self.popups(ctx);
        self.recorder_overlay(ctx);
        self.doc_overlay(ctx);
        self.analyze_overlay(ctx);
        self.block_editor(ctx);
        self.profile_window(ctx);
        self.city_welcome_window(ctx);
        self.call_tick(ctx);
        if let Some((_, _, since)) = &self.pending_handle {
            if since.elapsed().as_secs() >= 40 {
                self.pending_handle = None;
                self.add_msg = self.t("handle_not_found").to_string();
            }
        }
        self.call_window(ctx);
        self.invite_window(ctx);
        self.add_friend_window(ctx);
        self.dev_window(ctx);
        self.pdf_overlay(ctx);
        self.draw_toast(ctx);
        self.draw_tour(ctx);

        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl CyberApp {
    // ------------------------------------------------------------- panels

    fn rail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("rail")
            .exact_width(72.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(th().rail).inner_margin(egui::Margin::symmetric(10.0, 12.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        let home = self.icons.get(ui.ctx(), "app", APP_ICON);
                        let r = rail_tile(ui, home, self.show_home && !self.show_map, None).on_hover_text(self.t("home_tip"));
                        if r.clicked() {
                            self.show_home = true;
                            self.show_friends = false;
                            self.show_map = false;
                            self.show_city = false;
                            self.show_papers = false;
                            self.show_subs = false;
                            self.show_downloads = false;
                        }
                        ui.add_space(8.0);
                        let fr_on = self.show_friends && !self.show_home && !self.show_map;
                        let online = self.friends.values().filter(|f| f.online).count();
                        let tex = self.icons.get(ui.ctx(), "friends", FRIENDS_TILE);
                        let badge = if online > 0 { Some(online) } else { None };
                        // add a friend: always one click away (pulses while the list is empty)
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::click());
                        {
                            let k = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.15);
                            let p = ui.painter();
                            if self.friends.is_empty() {
                                let t = self.started.elapsed().as_secs_f32();
                                let ph = (t * 0.7) % 1.0;
                                p.rect_stroke(r.expand(2.0 + ph * 6.0), Rounding::same(16.0 + ph * 4.0), Stroke::new(2.0, th().green.gamma_multiply(1.0 - ph)));
                                ui.ctx().request_repaint_after(Duration::from_millis(50));
                            }
                            p.rect_filled(r.expand(2.0 * k), Rounding::same(14.0), th().green);
                            plus_glyph(p, r.center(), 11.0, Color32::WHITE);
                        }
                        if resp.on_hover_text(self.t("addf_tip")).clicked() {
                            self.open_add_friend();
                        }
                        ui.add_space(8.0);
                        let fr = rail_tile(ui, tex, fr_on, badge).on_hover_text(self.t("friends_tip").replace("{n}", &online.to_string()));
                        self.anchor("friends_btn", fr.rect);
                        if fr.clicked() {
                            self.show_friends = true;
                            self.show_home = false;
                            self.show_map = false;
                            self.show_city = false;
                            self.show_papers = false;
                            self.show_subs = false;
                            self.show_downloads = false;
                        }
                        ui.add_space(8.0);
                        let tex = self.icons.get(ui.ctx(), "network", NETWORK_TILE);
                        let online = self.nodes.iter().filter(|n| n.online && n.name.is_some()).count();
                        let nm = rail_tile(ui, tex, self.show_map, (online > 0).then_some(online)).on_hover_text(self.t("map_tip"));
                        self.anchor("map_btn", nm.rect);
                        if nm.clicked() {
                            self.show_map = true;
                            self.show_city = false;
                            self.show_papers = false;
                            self.show_subs = false;
                            self.show_home = false;
                            self.show_friends = false;
                            self.show_downloads = false;
                        }
                        ui.add_space(8.0);
                        let tex = self.icons.get(ui.ctx(), "city_tile", CITY_TILE);
                        if rail_tile(ui, tex, self.show_city, None).on_hover_text(self.t("city_tip")).clicked() {
                            self.show_city = true;
                            self.show_papers = false;
                            self.show_subs = false;
                            self.show_map = false;
                            self.show_home = false;
                            self.show_friends = false;
                            self.show_downloads = false;
                        }
                        ui.add_space(8.0);
                        let tex = self.icons.get(ui.ctx(), "papers_tile", PAPERS_TILE);
                        if rail_tile(ui, tex, self.show_papers, None).on_hover_text(self.t("papers_tip")).clicked() {
                            self.show_papers = true;
                            self.show_city = false;
                            self.show_subs = false;
                            self.show_map = false;
                            self.show_home = false;
                            self.show_friends = false;
                            self.show_downloads = false;
                        }
                        if !self.settings.subs.is_empty() {
                            ui.add_space(8.0);
                            let tex = self.icons.get(ui.ctx(), "subs_tile", SUBS_TILE);
                            let on = self.show_subs && !self.show_city && !self.show_home && !self.show_friends && !self.show_map;
                            if rail_tile(ui, tex, on, Some(self.settings.subs.len())).on_hover_text(self.t("subs_tip")).clicked() {
                                self.show_subs = true;
                                self.show_city = false;
                                self.show_papers = false;
                                self.show_map = false;
                                self.show_home = false;
                                self.show_friends = false;
                                self.show_downloads = false;
                                if self.sub_sel.0.is_empty() || !self.settings.subs.contains(&self.sub_sel.0) {
                                    self.sub_sel = (self.settings.subs[0].clone(), "chat".into());
                                }
                            }
                        }
                        ui.add_space(8.0);
                        let (r, _) = ui.allocate_exact_size(egui::vec2(32.0, 2.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, Rounding::same(1.0), th().line);
                        ui.add_space(8.0);
                        let top = ui.cursor().min;
                        for (i, s) in SERVERS.iter().enumerate() {
                            let selected = !self.show_home && !self.show_friends && !self.show_map && !self.show_city && !self.show_subs && !self.show_papers && i == self.sel_server;
                            let tex = self.icons.get(ui.ctx(), s.slug, s.icon);
                            let in_voice_here = self.in_voice.as_deref().map(|r| r.starts_with(&format!("{}/", s.slug))).unwrap_or(false);
                            let r = rail_tile(ui, tex, selected, None).on_hover_text(s.name);
                            if in_voice_here {
                                let c = r.rect.right_bottom() - egui::vec2(6.0, 6.0);
                                ui.painter().circle_filled(c, 8.0, th().rail);
                                let spk = self.icons.get(ui.ctx(), "speaker", SYM_SPEAKER);
                                ui.painter().image(spk, Rect::from_center_size(c, egui::vec2(11.0, 11.0)), Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), th().green);
                            }
                            if r.clicked() {
                                self.sel_server = i;
                                self.sel_channel = 0;
                                self.show_home = false;
                                self.show_friends = false;
                                self.show_map = false;
                            self.show_city = false;
                            self.show_papers = false;
                            self.show_subs = false;
                                self.show_downloads = false;
                            }
                            ui.add_space(8.0);
                        }
                        let bottom = ui.cursor().min;
                        self.anchor("rail", Rect::from_min_max(egui::pos2(top.x - 4.0, top.y - 4.0), egui::pos2(top.x + 56.0, bottom.y)));
                    });
                });
            });
    }

    fn channel_column(&mut self, ctx: &egui::Context, _ch: &ChannelId) {
        let server = &SERVERS[self.sel_server];
        egui::SidePanel::left("channels")
            .exact_width(236.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(th().surface).inner_margin(egui::Margin::same(12.0)))
            .show(ctx, |ui| {
                self.me_panel(ui);
                if self.show_friends {
                    self.friends_column(ui);
                    return;
                }
                if self.show_city {
                    self.city_column(ui);
                    return;
                }
                ui.horizontal(|ui| {
                    let tex = self.icons.get(ui.ctx(), server.slug, server.icon);
                    ui.add(egui::Image::new(SizedTexture::new(tex, egui::vec2(28.0, 28.0))));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(server.name).color(th().ink).b().size(15.5));
                        let motto = self.td(&format!("motto_{}", server.slug));
                        let line = if motto.is_empty() { self.t("server_label") } else { motto };
                        ui.add(egui::Label::new(RichText::new(line).color(th().ink2).small()).truncate()).on_hover_text(line);
                        if server.slug == "atom-generator" && ui.link(RichText::new(self.t("atom_start")).small()).clicked() {
                            self.doc_open = Some(Doc::Atom);
                        }
                    });
                });
                ui.add_space(6.0);
                sep(ui);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    section_label(ui, self.t("text_channels"));
                    let hash = self.icons.get(ui.ctx(), "hash", SYM_HASH);
                    for (i, c) in server.channels.iter().enumerate() {
                        let selected = !self.show_home && !self.show_friends && i == self.sel_channel;
                        let label = self.ch_label(c);
                        if channel_row(ui, hash, &label, selected, false).on_hover_text(format!("#{c}")).clicked() {
                            self.sel_channel = i;
                            self.show_home = false;
                        }
                    }
                    ui.add_space(10.0);
                    let top = ui.cursor().min;
                    section_label(ui, self.t("voice_channels"));
                    let spk = self.icons.get(ui.ctx(), "speaker", SYM_SPEAKER);
                    let now = now_ms();
                    for v in server.voice.iter() {
                        let room = format!("{}/{}", server.slug, v);
                        let mine = self.in_voice.as_deref() == Some(room.as_str());
                        let label = self.ch_label(v);
                        let r = channel_row(ui, spk, &label, mine, true).on_hover_text(self.t("voice_join_tip"));
                        if r.clicked() && !mine {
                            let _ = self.handle.cmd_tx.send(Cmd::JoinVoice { channel: ChannelId::new(server.slug, *v) });
                        }
                        // who is in this room (from heartbeats) — like Discord's list under a voice channel
                        let mut people: Vec<(String, bool)> = self
                            .roster
                            .iter()
                            .filter(|(p, (_, r, seen))| r.as_deref() == Some(room.as_str()) && **p != self.self_id && now.saturating_sub(*seen) < 20_000)
                            .map(|(_, (n, _, _))| (n.clone(), self.talkers.get(n).map(|t| now.saturating_sub(*t) < 900).unwrap_or(false)))
                            .collect();
                        if mine {
                            people.insert(0, (self.self_name.clone(), self.mic_on && self.talkers.get(&self.self_name).map(|t| now.saturating_sub(*t) < 900).unwrap_or(false)));
                        }
                        for (name, talking) in people {
                            ui.horizontal(|ui| {
                                ui.add_space(26.0);
                                let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();
                                avatar(ui, &initial, 10.0, name_color(&name), if talking { Some(th().green) } else { None });
                                ui.label(RichText::new(&name).color(th().ink).size(13.5));
                            });
                        }
                    }
                    let bottom = ui.cursor().min;
                    self.anchor("voice", Rect::from_min_max(top - egui::vec2(4.0, 4.0), egui::pos2(top.x + ui.available_width() + 4.0, bottom.y + 2.0)));
                });
            });
    }

    /// "Voice connected" box above the user panel: room, mic toggle, hang up.
    fn voice_box(&mut self, ui: &mut egui::Ui) {
        if !voice::AUDIO_ENABLED {
            ui.colored_label(th().ink2, RichText::new(self.t("audio_off")).small().italics());
            ui.add_space(4.0);
        }
        let Some(room) = self.in_voice.clone() else { return };
        egui::Frame::none().fill(th().window).stroke(Stroke::new(1.0, th().line)).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(10.0, 8.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(self.t("voice_connected")).color(th().green_text).b().size(13.5));
                    let shown = match room.split_once('/') {
                        Some((g, c)) if g.starts_with("dm") || c.len() > 40 => self.friend_name(c),
                        Some((_, c)) => c.to_string(),
                        None => room.clone(),
                    };
                    ui.colored_label(th().ink2, RichText::new(shown).small());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let hang = self.icons.get(ui.ctx(), "hangup", SYM_HANGUP);
                    if icon_button(ui, hang, th().on_accent, th().red, 32.0).on_hover_text(self.t("leave_tip")).clicked() {
                        let _ = self.handle.cmd_tx.send(Cmd::LeaveVoice);
                    }
                    let (tex, fg, bg) = if self.mic_on {
                        (self.icons.get(ui.ctx(), "mic", SYM_MIC), th().on_accent, th().green)
                    } else {
                        (self.icons.get(ui.ctx(), "mic-off", SYM_MIC_OFF), th().ink2, th().card)
                    };
                    let tip = format!("{} — {}", self.t("mic_tip"), if self.mic_on { self.t("mic_on") } else { self.t("mic_off") });
                    if icon_button(ui, tex, fg, bg, 32.0).on_hover_text(tip).clicked() {
                        self.mic_on = !self.mic_on;
                        let _ = self.handle.cmd_tx.send(Cmd::SetMic(self.mic_on));
                    }
                });
            });
        });
    }

    fn member_panel(&mut self, ctx: &egui::Context, _ch: &ChannelId) {
        egui::SidePanel::right("members")
            .exact_width(210.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(th().surface).inner_margin(egui::Margin::same(12.0)))
            .show(ctx, |ui| {
                section_label(ui, self.t("members_h"));
                let now = now_ms();
                let me = format!("{} {}", self.self_name, self.t("you"));
                let my_role = self.role_of(&self.self_id);
                let spk = self.icons.get(ui.ctx(), "speaker", SYM_SPEAKER);
                if member_row(ui, &me, my_role, true, false, self.in_voice.is_some().then_some(spk)).clicked() {
                    self.profile_view = Some(self.self_id.clone());
                }
                if let Some(code) = self.user_code(&self.self_id.clone()) {
                    ui.horizontal(|ui| {
                        ui.add_space(38.0);
                        ui.label(RichText::new(code).color(th().ink2).size(10.5).monospace());
                    });
                }
                let rows: Vec<(String, Option<&'static str>, bool, bool, bool)> = self
                    .peers
                    .iter()
                    .filter(|(id, _)| **id != self.self_id)
                    .map(|(id, pv)| {
                        let name = pv.name.clone().unwrap_or_else(|| short(id));
                        let talking = self.talkers.get(&name).map(|t| now.saturating_sub(*t) < 900).unwrap_or(false);
                        let in_voice = self.roster.get(id).map(|(_, r, seen)| r.is_some() && now.saturating_sub(*seen) < 20_000).unwrap_or(false);
                        (name, self.role_of(id), pv.connected, talking, in_voice)
                    })
                    .collect();
                let ids: Vec<String> = self.peers.keys().filter(|id| **id != self.self_id).cloned().collect();
                for ((name, role, conn, talking, in_voice), id) in rows.into_iter().zip(ids) {
                    let st = self.status_of(&id);
                    let conn = conn && st != "offline";
                    let row = member_row(ui, &name, role, conn, talking, in_voice.then_some(spk));
                    if let Some(code) = self.user_code(&id) {
                        ui.horizontal(|ui| {
                            ui.add_space(38.0);
                            ui.label(RichText::new(code).color(th().ink2).size(10.5).monospace());
                        });
                    }
                    if row.clicked() {
                        self.profile_view = Some(id.clone());
                    }
                    if conn && !st.is_empty() && st != "online" {
                        ui.horizontal(|ui| {
                            ui.add_space(38.0);
                            ui.colored_label(status_color(&st).unwrap_or(th().ink2), RichText::new(self.status_label(&st)).size(11.0));
                        });
                    }
                }
            });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bar")
            .exact_height(30.0)
            .frame(egui::Frame::none().fill(th().surface).inner_margin(egui::Margin::symmetric(12.0, 5.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let tex = self.icons.get(ui.ctx(), "app", APP_ICON);
                    ui.add(egui::Image::new(SizedTexture::new(tex, egui::vec2(16.0, 16.0))));
                    let t = self.started.elapsed().as_secs_f32() - 0.6;
                    if self.settings.hack_fx && (0.0..crate::fx::TOTAL * 1.6).contains(&t) {
                        crate::fx::label(ui, "SocioCyberneering Inc.", t / 1.6, 0x5ce, egui::FontId::new(12.0, egui::FontFamily::Name("brand".into())), th().ink, th().accent);
                        ui.ctx().request_repaint();
                    } else {
                        docs::brand(ui, "SocioCyberneering Inc.", 12.0, th().ink);
                    }
                    ui.separator();
                    let hub = self.hub.as_deref().map(short).unwrap_or_else(|| "—".into());
                    ui.colored_label(th().ink2, RichText::new(format!("{}: {hub}", self.t("hub_label"))).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let crate::update::State::Available(r) = self.updater.get() {
                            if ui.add(egui::Button::new(RichText::new(self.t("upd_chip").replace("{v}", &r.version)).small().b().color(th().on_accent)).fill(th().green).rounding(Rounding::same(4.0))).clicked() {
                                self.show_settings = true;
                                self.set_tab = SetTab::About;
                                self.set_opened = Instant::now();
                            }
                            ui.separator();
                        }
                        if self.dev_on() {
                            let bots = crate::dev::KINDS.iter().filter(|k| crate::dev::running(k)).count();
                            let label = if bots > 0 { format!("DEV · {bots}") } else { "DEV".to_string() };
                            if ui.add(egui::Button::new(RichText::new(label).small().b().color(Color32::WHITE)).fill(Color32::from_rgb(0xc0, 0x1c, 0x28)).rounding(Rounding::same(4.0))).on_hover_text(self.t("devm_title")).clicked() {
                                self.show_dev = !self.show_dev;
                            }
                            ui.separator();
                        }
                        let active = self.transfers.iter().filter(|t| matches!(t.state, "downloading" | "waiting" | "verifying")).count();
                        let label = if active > 0 { format!("⬇ {} ({active})", self.t("downloads_btn")) } else { format!("⬇ {}", self.t("downloads_btn")) };
                        if ui.add(egui::Button::new(RichText::new(label).small().color(if self.show_downloads { th().accent } else { th().ink2 })).frame(false)).clicked() {
                            self.show_downloads = !self.show_downloads;
                        }
                        ui.separator();
                        let (txt, col) = if self.net.scanning {
                            (self.t("scanning_bar").replace("{n}", &self.net.known.to_string()), th().accent)
                        } else {
                            (
                                self.t("p2p_bar").replace("{c}", &self.net.connected.to_string()).replace("{k}", &self.net.known.to_string()),
                                th().green_text,
                            )
                        };
                        ui.colored_label(col, RichText::new(txt).small());
                    });
                });
            });
    }

    fn friends_column(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(self.t("friends")).color(th().ink).b().size(16.0));
        let online = self.friends.values().filter(|f| f.online).count();
        ui.colored_label(th().ink2, self.t("friends_count").replace("{o}", &online.to_string()).replace("{t}", &self.friends.len().to_string()));
        ui.add_space(6.0);
        let w = ui.available_width();
        if ui.add(egui::Button::new(RichText::new(self.t("addf_btn")).color(th().on_accent).b()).fill(th().green).rounding(Rounding::same(8.0)).min_size(egui::vec2(w, 34.0))).on_hover_text(self.t("addf_tip")).clicked() {
            self.open_add_friend();
        }
        ui.add_space(2.0);
        let add_sel = self.dm_open.is_none();
        if ui.selectable_label(add_sel, RichText::new(self.t("friends_page")).color(if add_sel { th().ink } else { th().accent })).clicked() {
            self.dm_open = None;
        }
        ui.add_space(4.0);
        sep(ui);
        if self.friends.is_empty() {
            ui.colored_label(th().ink2, self.t("no_friends"));
        }
        let mut list: Vec<(String, String, bool, String)> = self
            .friends
            .iter()
            .map(|(p, f)| {
                let st = self.status_of(p);
                (p.clone(), f.name.clone(), f.online && st != "offline", st)
            })
            .collect();
        list.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
        for (peer, name, on, st) in list {
            let selected = self.dm_open.as_deref() == Some(peer.as_str());
            ui.horizontal(|ui| {
                let dotc = if !on { th().line } else { status_color(&st).unwrap_or(th().green) };
                dot(ui, dotc);
                ui.vertical(|ui| {
                    if ui.selectable_label(selected, RichText::new(&name).color(if on { th().ink } else { th().ink2 })).clicked() {
                        self.dm_open = Some(peer.clone());
                    }
                    if on && !st.is_empty() && st != "online" {
                        ui.colored_label(th().ink2, RichText::new(self.status_label(&st)).size(11.0));
                    }
                });
            });
        }
    }

    // ------------------------------------------------------------- central views

    fn friends_home(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new(self.t("friends")).color(th().ink).b().size(24.0));
            ui.colored_label(th().ink2, self.t("friends_sub"));
            ui.add_space(12.0);
            card(ui, self.t("find_people"), |ui| {
                self.people_search(ui, false);
            });
            ui.add_space(10.0);
            self.invite_card(ui);
            ui.add_space(10.0);

            let r = card(ui, self.t("your_code"), |ui| {
                self.my_code_body(ui);
            });
            self.anchor("code", r);
            ui.add_space(10.0);

            card(ui, self.t("add_a_friend"), |ui| {
                self.add_friend_body(ui);
            });
            ui.add_space(10.0);
            self.network_card(ui, false);
        });
    }

    fn network_card(&mut self, ui: &mut egui::Ui, record_anchor: bool) {
        let n = self.net.clone();
        let r = card(ui, self.t("net_title"), |ui| {
            ui.horizontal(|ui| {
                dot(ui, if n.scanning { th().accent } else { th().green });
                let head = if n.scanning { self.t("net_scanning").to_string() } else { self.t("net_connected").replace("{n}", &n.connected.to_string()) };
                ui.label(RichText::new(head).color(th().ink).b());
            });
            ui.add_space(4.0);
            let reach = match n.reachable {
                Some(true) => self.t("reach_yes"),
                Some(false) => self.t("reach_no"),
                None => self.t("reach_unknown"),
            };
            ui.colored_label(
                th().ink2,
                self.t("net_stats").replace("{k}", &n.known.to_string()).replace("{r}", &n.relays.to_string()).replace("{f}", &n.relaying_for.to_string()),
            );
            ui.colored_label(th().ink2, reach);
            if !n.sources.is_empty() {
                ui.colored_label(th().ink2, self.t("found_via").replace("{s}", &n.sources));
            }
            if !self.status.is_empty() {
                ui.colored_label(th().accent, RichText::new(self.t("last").replace("{s}", &self.status)).small());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(self.t("scan_now")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0))).clicked() {
                    let _ = self.handle.cmd_tx.send(Cmd::Rescan);
                }
                if ui.add(egui::Button::new(RichText::new(self.t("map_open")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0))).clicked() {
                    self.show_map = true;
                            self.show_city = false;
                            self.show_papers = false;
                            self.show_subs = false;
                    self.show_home = false;
                    self.show_friends = false;
                }
            });
            ui.add_space(8.0);
            egui::CollapsingHeader::new(RichText::new(self.t("how_finds")).color(th().ink).b()).default_open(false).show(ui, |ui| {
                for k in ["how_lan", "how_saved", "how_friends", "how_pex", "how_dht", "how_nat"] {
                    bullet(ui, self.t(k));
                }
                ui.add_space(4.0);
                ui.colored_label(th().ink2, self.t("only_announced"));
                ui.add_space(8.0);
                ui.colored_label(th().ink2, self.t("connect_node"));
                ui.add(egui::TextEdit::singleline(&mut self.node_addr).font(egui::TextStyle::Monospace).hint_text("/ip4/…/udp/47100/quic-v1/p2p/12D3Koo…").desired_width(f32::INFINITY));
                if ui.add(egui::Button::new(RichText::new(self.t("connect")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0))).clicked() {
                    let _ = self.handle.cmd_tx.send(Cmd::AddNode(self.node_addr.trim().to_string()));
                }
            });
        });
        if record_anchor {
            self.anchor("network", r);
        }
    }

    fn home_view(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(self.t("welcome")).color(th().ink).b().size(26.0));
            ui.colored_label(th().ink2, self.t("tagline"));
            ui.add_space(14.0);
            self.guide_card(ui);
            ui.add_space(10.0);
            self.invite_card(ui);
            ui.add_space(10.0);
            self.network_card(ui, true);
            ui.add_space(10.0);
            card(ui, self.t("good_to_know"), |ui| {
                bullet(ui, self.t("gk1"));
                bullet(ui, self.t("gk2"));
                bullet(ui, self.t("gk3"));
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(self.t("open_welcome")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(160.0, 34.0))).clicked() {
                    self.show_home = false;
                    self.sel_server = 0;
                    self.sel_channel = 0;
                }
                if ui.add(egui::Button::new(RichText::new(self.t("show_tour")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0)).min_size(egui::vec2(160.0, 34.0))).clicked() {
                    self.go_tour(0);
                }
            });
            ui.add_space(12.0);
        });
    }

    /// Step-by-step guide: GNOME-style picture + caption, with arrows and dots.
    fn guide_card(&mut self, ui: &mut egui::Ui) {
        let n = GUIDE.len();
        let i = self.guide_step.min(n - 1);
        let (key, png, h, b) = GUIDE[i];
        let k = ease_out(self.guide_changed.elapsed().as_secs_f32() / 0.3);
        let mut go: Option<usize> = None;
        card(ui, self.t("guide_title"), |ui| {
            let w = ui.available_width().min(560.0);
            let tex = self.icons.get(ui.ctx(), key, png);
            let size = egui::vec2(w, w * 300.0 / 480.0);
            let (r, resp) = ui.allocate_exact_size(size, egui::Sense::click());
            let shift = (1.0 - k) * 24.0;
            ui.painter().image(tex, r.translate(egui::vec2(shift, 0.0)), Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE.gamma_multiply(k.max(0.05)));
            if resp.clicked() {
                go = Some((i + 1) % n);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}", i + 1)).color(th().on_accent).b().background_color(th().accent));
                ui.label(RichText::new(self.t(h)).color(th().ink).b().size(17.0));
            });
            ui.add_space(2.0);
            ui.label(RichText::new(self.t(b)).color(th().ink2));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(i > 0, egui::Button::new(RichText::new(self.t("guide_prev")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0))).clicked() {
                    go = Some(i - 1);
                }
                ui.add_space(8.0);
                for d in 0..n {
                    let (dr, dresp) = ui.allocate_exact_size(egui::vec2(if d == i { 20.0 } else { 8.0 }, 8.0), egui::Sense::click());
                    ui.painter().rect_filled(dr, Rounding::same(4.0), if d == i { th().accent } else { th().line });
                    if dresp.clicked() {
                        go = Some(d);
                    }
                }
                ui.add_space(8.0);
                if ui.add_enabled(i + 1 < n, egui::Button::new(RichText::new(self.t("guide_next")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0))).clicked() {
                    go = Some(i + 1);
                }
            });
        });
        if let Some(g) = go {
            self.guide_step = g;
            self.guide_changed = Instant::now();
        }
        if k < 1.0 {
            ui.ctx().request_repaint();
        }
    }

    fn chat_view(&mut self, ui: &mut egui::Ui, ch: &ChannelId) {
        ui.horizontal(|ui| {
            if ch.is_dm() {
                let online = self.friends.get(&ch.channel).map(|f| f.online).unwrap_or(false);
                dot(ui, if online { th().green } else { th().line });
                ui.label(RichText::new(format!("@  {}", self.friend_name(&ch.channel))).color(th().ink).b().size(17.0));
                ui.colored_label(th().ink2, format!("· {}", if online { self.t("dm_online") } else { self.t("dm_offline") }));
                if self.e2e_peers.contains(&ch.channel) {
                    ui.label(RichText::new(self.t("e2e_on")).color(th().green_text).small()).on_hover_text(self.t("e2e_on_tip"));
                } else {
                    ui.label(RichText::new(self.t("e2e_wait")).color(th().ink2).small()).on_hover_text(self.t("e2e_wait_tip"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let in_this = self.in_voice.as_deref().map(|r| r.ends_with(&ch.channel)).unwrap_or(false);
                    if !in_this {
                        let tex = self.icons.get(ui.ctx(), "headphones", SYM_HEADPHONES);
                        let btn = egui::Button::image_and_text(egui::Image::new(SizedTexture::new(tex, egui::vec2(16.0, 16.0))).tint(th().on_accent), RichText::new(self.t("call_btn")).color(th().on_accent).b())
                            .fill(th().green)
                            .rounding(Rounding::same(8.0));
                        if ui.add(btn).on_hover_text(self.t("call").replace("{name}", &self.friend_name(&ch.channel))).clicked() {
                            self.start_call(&ch.channel);
                        }
                    }
                });
            } else {
                let hash = self.icons.get(ui.ctx(), "hash", SYM_HASH);
                ui.add(egui::Image::new(SizedTexture::new(hash, egui::vec2(18.0, 18.0))).tint(th().ink2));
                ui.label(RichText::new(self.ch_label(&ch.channel)).color(th().ink).b().size(17.0)).on_hover_text(format!("#{}", ch.channel));
                let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
                if let Some(b) = ch.guild.strip_prefix("city.").and_then(cybercord_node::city::branch) {
                    ui.colored_label(th().ink2, format!("· {}", if ru { b.ru } else { b.en }));
                    if b.slug == "papers" && ui.small_button(self.t("papers_tip")).clicked() {
                        self.show_papers = true;
                    }
                    ui.menu_button(RichText::new(self.t("city_ready")).small(), |ui| {
                        ui.set_min_width(380.0);
                        for m in cybercord_node::city::ready_messages(b, ru) {
                            if ui.button(&m).clicked() {
                                self.input = m.clone();
                                self.focus_input = 3;
                                ui.close_menu();
                            }
                        }
                    });
                } else {
                    ui.colored_label(th().ink2, format!("· {}", SERVERS[self.sel_server].name));
                }
            }
        });
        if let Some(b) = ch.guild.strip_prefix("city.").and_then(cybercord_node::city::branch) {
            // the goal of the channel, shown on entering
            let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
            ui.add(egui::Label::new(RichText::new(format!("{}: {}", self.t("city_goal"), if ru { b.goal_ru } else { b.goal_en })).color(th().ink2).small()).wrap());
        }
        ui.add_space(4.0);
        sep(ui);

        let hints = sem::scan(&self.input);
        // the composer grows with its text (measured last frame), up to ~6 lines
        let mut input_h = 60.0 + (self.composer_rect.height() - self.composer_base).clamp(0.0, 100.0);
        if self.editing.as_ref().map(|e| e.0 == *ch).unwrap_or(false) {
            input_h += 24.0;
        }
        if !hints.is_empty() && self.settings.words_hints {
            input_h += 30.0;
        }
        if self.math_mode || crate::math::has_math(&self.input) {
            input_h += self.preview_h;
        }
        self.conversation_banner(ui, ch);
        let avail = ui.available_height() - input_h;
        // (message, what to show after edits); deleted messages are left out
        let msgs: Vec<(ChatEvent, cybercord_node::history::Resolved)> = self
            .views
            .get(ch)
            .map(|v| v.msgs.iter().map(|m| (m.clone(), cybercord_node::history::resolve(m, &v.edits))).filter(|(_, r)| !r.deleted).collect())
            .unwrap_or_default();
        if self.confirm_delete.as_ref().map(|(_, t)| t.elapsed() > Duration::from_secs(4)).unwrap_or(false) {
            self.confirm_delete = None;
        }
        let mut action: Option<(bool, String)> = None; // (delete?, id)
        let mut toggled: Option<(String, String, u32)> = None; // checkbox ticked in my message → edit
        let mut new_hover: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(avail).stick_to_bottom(true).show(ui, |ui| {
            if msgs.is_empty() {
                ui.add_space(8.0);
                ui.colored_label(th().ink2, self.t("no_messages"));
            }
            for (m, r) in &msgs {
                let mine = if m.from.is_empty() { m.name == self.self_name } else { m.from == self.self_id };
                let editable = !m.from.is_empty() && m.from == self.self_id;
                let editing_this = self.editing.as_ref().map(|e| e.1 == m.id).unwrap_or(false);
                let confirming = self.confirm_delete.as_ref().map(|c| c.0 == m.id).unwrap_or(false);
                let show_tools = editable && (self.hover_msg.as_deref() == Some(m.id.as_str()) || confirming);
                let bg = ui.painter().add(egui::Shape::Noop);
                let top = ui.cursor().min;
                ui.horizontal(|ui| {
                    ui.set_min_height(22.0);
                    ui.colored_label(th().ink2, RichText::new(clock(m.ts_ms)).small());
                    let nresp = ui.add(egui::Label::new(RichText::new(&m.name).color(if mine { th().accent } else { th().ink }).b()).sense(egui::Sense::click()));
                    if let Some(code) = self.user_code(&m.from) {
                        ui.label(RichText::new(format!("[{code}]")).color(th().ink2).size(10.5).monospace());
                    }
                    if nresp.clicked() && !m.from.is_empty() {
                        self.profile_view = Some(m.from.clone());
                    }
                    if r.rev > 1 {
                        // dated and indexed, as in general semantics: message₂ (date)
                        let mark = format!("{}{} · {}", self.t("edited_mark"), subscript(r.rev), short_date(r.ts_ms));
                        ui.label(RichText::new(mark).color(th().ink2).small()).on_hover_ui(|ui| {
                            ui.set_max_width(360.0);
                            ui.label(RichText::new(self.t("versions_h")).b());
                            for (rev, ts, body) in &r.versions {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(format!("{}{} · {}", self.t("version_n"), subscript(*rev), short_date(*ts))).color(th().ink2).small());
                                    ui.label(if body.is_empty() { "—".to_string() } else { body.chars().take(240).collect::<String>() });
                                });
                            }
                        });
                    }
                    if show_tools {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(6.0);
                            if confirming {
                                if ui.add(egui::Button::new(RichText::new(self.t("delete_confirm")).color(Color32::WHITE).b().small()).fill(th().red).rounding(Rounding::same(6.0))).clicked() {
                                    action = Some((true, m.id.clone()));
                                }
                            } else {
                                let trash = self.icons.get(ui.ctx(), "trash", SYM_TRASH);
                                if icon_button_k(ui, trash, th().ink2, th().window, 22.0, 0.72).on_hover_text(self.t("delete_msg")).clicked() {
                                    self.confirm_delete = Some((m.id.clone(), Instant::now()));
                                }
                            }
                            let pencil = self.icons.get(ui.ctx(), "pencil", SYM_PENCIL);
                            if icon_button_k(ui, pencil, if editing_this { th().accent } else { th().ink2 }, th().window, 22.0, 0.72).on_hover_text(self.t("edit_msg")).clicked() {
                                action = Some((false, m.id.clone()));
                            }
                            let lv = self.icons.get(ui.ctx(), "levels", SYM_LEVELS);
                            if icon_button_k(ui, lv, th().ink2, th().window, 22.0, 0.72).on_hover_text(self.t("sem_levels")).clicked() {
                                self.analyze = Some((m.name.clone(), r.body.clone(), r.ts_ms));
                            }
                        });
                    } else if self.hover_msg.as_deref() == Some(m.id.as_str()) && !r.body.trim().is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(6.0);
                            let lv = self.icons.get(ui.ctx(), "levels", SYM_LEVELS);
                            if icon_button_k(ui, lv, th().ink2, th().window, 22.0, 0.72).on_hover_text(self.t("sem_levels")).clicked() {
                                self.analyze = Some((m.name.clone(), r.body.clone(), r.ts_ms));
                            }
                        });
                    }
                });
                ui.indent(&m.id, |ui| {
                    if !r.body.is_empty() {
                        if let Some(nb) = self.body_view(ui, &m.id, &r.body, editable) {
                            toggled = Some((m.id.clone(), nb, r.rev + 1));
                        }
                    }
                    if let Some(a) = &m.attach {
                        self.media_widget(ui, a, ch);
                    }
                });
                let rect = Rect::from_min_max(top - egui::vec2(4.0, 2.0), egui::pos2(ui.max_rect().max.x, ui.cursor().min.y + 2.0));
                let hovered = ui.rect_contains_pointer(rect);
                if hovered {
                    new_hover = Some(m.id.clone());
                }
                if hovered || editing_this {
                    ui.painter().set(bg, egui::Shape::rect_filled(rect, Rounding::same(6.0), if editing_this { th().sel } else { th().card }));
                }
                ui.add_space(6.0);
            }
        });
        self.hover_msg = new_hover;
        if let Some((target, body, rev)) = toggled {
            let _ = self.handle.cmd_tx.send(Cmd::EditMessage { channel: ch.clone(), target, body: Some(body), rev });
        }
        match action {
            Some((true, id)) => self.delete_message(ch, &id),
            Some((false, id)) => self.start_edit(ch, &id),
            None => {}
        }

        ui.add_space(4.0);
        sep(ui);
        if self.vm.is_some() {
            let row = ui.horizontal(|ui| self.voice_message_bar(ui, ch));
            self.anchor("composer", row.response.rect.expand(2.0));
            return;
        }
        if let Some((ech, _, rev)) = self.editing.clone() {
            if ech != *ch {
                self.cancel_edit();
            } else {
                ui.horizontal(|ui| {
                    let pencil = self.icons.get(ui.ctx(), "pencil", SYM_PENCIL);
                    ui.add(egui::Image::new(SizedTexture::new(pencil, egui::vec2(14.0, 14.0))).tint(th().accent));
                    ui.label(RichText::new(format!("{}{}", self.t("editing_bar"), subscript(rev))).color(th().accent).b().small());
                    ui.colored_label(th().ink2, RichText::new(self.t("editing_keys")).small());
                    if ui.add(egui::Button::new(RichText::new(self.t("edit_cancel")).small()).frame(false)).clicked() {
                        self.cancel_edit();
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.cancel_edit();
                }
            }
        }
        if !hints.is_empty() && self.settings.words_hints {
            self.hint_strip(ui, &hints);
        }
        if self.math_mode || crate::math::has_math(&self.input) {
            let top = ui.cursor().min.y;
            self.math_preview(ui);
            self.preview_h = ui.cursor().min.y - top;
        }
        if let Some(rx) = &self.pick_rx {
            if let Ok(r) = rx.try_recv() {
                self.pick_rx = None;
                if let Some(p) = r {
                    self.send_path(p);
                }
            }
        }
        let row = ui.horizontal(|ui| {
            let btns = ui.horizontal(|ui| {
                ui.menu_button(RichText::new("＋").size(18.0).color(th().ink2), |ui| {
                    ui.set_min_width(230.0);
                    if ui.button(self.t("block_scale")).clicked() {
                        self.block_ed = Some(BlockEd::new_scale());
                        ui.close_menu();
                    }
                    if ui.button(self.t("block_chart")).clicked() {
                        self.block_ed = Some(BlockEd::new_chart());
                        ui.close_menu();
                    }
                    ui.separator();
                    for (key, tpl) in templates(self.lang) {
                        if ui.button(self.t(key)).clicked() {
                            if !self.input.is_empty() && !self.input.ends_with('\n') {
                                self.input.push('\n');
                            }
                            self.input.push_str(&tpl);
                            self.focus_input = 3;
                            ui.close_menu();
                        }
                    }
                })
                .response
                .on_hover_text(self.t("tpl_tip"));
                let e_idx = emoji::find_code("slight_smile").unwrap_or(0);
                let tex = self.emoji.tex(ui.ctx(), e_idx);
                let e = ui.add(egui::ImageButton::new(SizedTexture::new(tex, egui::vec2(22.0, 22.0))).frame(false)).on_hover_text(self.t("emoji_tip"));
                if e.clicked() {
                    self.popup = if self.popup == Popup::Emoji { Popup::None } else { Popup::Emoji };
                }
                let g = ui.add(egui::Button::new(RichText::new("GIF").b().size(12.0).color(th().accent)).fill(th().window).stroke(Stroke::new(1.5, th().accent)).rounding(Rounding::same(6.0)))
                    .on_hover_text(self.t("gif_tip"));
                if g.clicked() {
                    self.popup = if self.popup == Popup::Gifs { Popup::None } else { Popup::Gifs };
                }
                let img = self.icons.get(ui.ctx(), "image", SYM_IMAGE);
                if icon_button_k(ui, img, th().ink2, th().window, 30.0, 0.62).on_hover_text(self.t("img_tip")).clicked() && self.pick_rx.is_none() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.pick_rx = Some(rx);
                    let title = self.t("img_pick").to_string();
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let r = rfd::FileDialog::new().set_title(title).add_filter("images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"]).pick_file();
                        let _ = tx.send(r);
                        ctx.request_repaint();
                    });
                }
                let sig = self.icons.get(ui.ctx(), "sigma", SYM_SIGMA);
                let on = self.math_mode;
                if icon_button_k(ui, sig, if on { th().on_accent } else { th().ink2 }, if on { th().accent } else { th().window }, 30.0, 0.62).on_hover_text(self.t("math_tip")).clicked() {
                    self.math_mode = !self.math_mode;
                    if self.math_mode && !self.input.contains('$') {
                        self.input.push_str("$$  $$");
                    }
                    self.focus_input = 3;
                }
                // round "record a circle" button
                let (r, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
                let hovered = resp.hovered();
                ui.painter().circle_stroke(r.center(), 11.0, Stroke::new(2.0, if hovered { th().accent } else { th().ink2 }));
                ui.painter().circle_filled(r.center(), 5.0, if hovered { th().red } else { th().ink2 });
                if resp.on_hover_text(self.t("circle_tip")).clicked() {
                    self.start_recording();
                }
                // voice message
                let mic = self.icons.get(ui.ctx(), "mic", SYM_MIC);
                if icon_button(ui, mic, th().ink2, th().window, 30.0).on_hover_text(self.t("vm_tip")).clicked() {
                    self.start_voice_message();
                }
            });
            self.anchor("media_btns", btns.response.rect.expand(2.0));
            let hint = if ch.is_dm() {
                self.t("message_dm").replace("{name}", &self.friend_name(&ch.channel))
            } else {
                self.t("message_ch").replace("{ch}", &self.ch_label(&ch.channel))
            };
            // Enter sends, Shift+Enter starts a new line
            let input_id = Id::new("composer_input");
            let focused_before = ui.memory(|m| m.has_focus(input_id));
            let enter = focused_before && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            let w = ui.available_width() - 118.0;
            let scroll = egui::ScrollArea::vertical().id_salt("composer_scroll").max_height(132.0).min_scrolled_height(self.composer_rect.height().clamp(32.0, 132.0)).max_width(w).auto_shrink([false, true]).stick_to_bottom(true).show(ui, |ui| {
                egui::TextEdit::multiline(&mut self.input).id(input_id).hint_text(hint).desired_rows(1).desired_width(w - 12.0).min_size(egui::vec2(0.0, 32.0)).show(ui)
            });
            let out = scroll.inner;
            let resp = out.response.clone();
            self.composer_rect = Rect::from_min_size(scroll.inner_rect.min, egui::vec2(w, resp.rect.height().min(scroll.content_size.y).clamp(32.0, 132.0)));
            if !self.input.contains('\n') && self.input.chars().count() < 30 {
                self.composer_base = self.composer_base.min(self.composer_rect.height());
            }
            self.sel = out.cursor_range.and_then(|r| {
                let a = r.primary.ccursor.index.min(r.secondary.ccursor.index);
                let b = r.primary.ccursor.index.max(r.secondary.ccursor.index);
                (b > a && resp.has_focus()).then_some((a, b))
            }).or(if resp.has_focus() { None } else { self.sel });
            // a click on ✎ ends with "pointer released elsewhere", which drops
            // focus in that same frame, so ask again for a few frames
            if self.focus_input > 0 {
                self.focus_input -= 1;
                resp.request_focus();
                ui.ctx().request_repaint();
            }
            // ↑ in an empty composer edits my last message (like Discord)
            if resp.has_focus() && self.input.is_empty() && self.editing.is_none() && ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                let last = self.views.get(ch).and_then(|v| v.msgs.iter().rev().find(|m| m.from == self.self_id && !cybercord_node::history::resolve(m, &v.edits).deleted).map(|m| m.id.clone()));
                if let Some(id) = last {
                    self.start_edit(ch, &id);
                }
            }
            if ui.add(egui::Button::new(RichText::new(self.t("send")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(80.0, 32.0))).clicked() || enter {
                self.send_current();
                self.sel = None;
                resp.request_focus();
            }
        });
        self.anchor("composer", row.response.rect.expand(2.0));
        self.selection_tip(ui.ctx());
    }

    /// Floating tip over a selection in the composer: extensional devices.
    fn selection_tip(&mut self, ctx: &egui::Context) {
        let Some((a, b)) = self.sel else { return };
        let chars: Vec<char> = self.input.chars().collect();
        if b > chars.len() || a >= b {
            self.sel = None;
            return;
        }
        let picked: String = chars[a..b].iter().collect();
        let mut result: Option<String> = None;
        // where to look for indices already used: the draft and the open channel
        let mut context = self.input.clone();
        if let Some(v) = self.views.get(&self.active_channel()) {
            for m in v.msgs.iter().rev().take(80) {
                context.push(' ');
                context.push_str(&cybercord_node::history::resolve(m, &v.edits).body);
            }
        }
        let c = th();
        let lift = if self.settings.words_hints && !sem::scan(&self.input).is_empty() { 74.0 } else { 44.0 } + if self.index_pick { 34.0 } else { 0.0 };
        egui::Area::new(Id::new("sel_tip")).order(Order::Foreground).fixed_pos(self.composer_rect.left_top() + egui::vec2(0.0, -lift)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(c.window).rounding(Rounding::same(10.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let now = chrono_today();
                    let btn = |ui: &mut egui::Ui, label: String, tip: &str| ui.add(egui::Button::new(RichText::new(label).color(c.ink)).fill(c.card).rounding(Rounding::same(6.0))).on_hover_text(tip).clicked();
                    let core = picked.trim();
                    let next = sem::next_index(core, &context);
                    let (base, _) = sem::split_index(core);
                    if btn(ui, format!("{}{}", self.t("ext_index"), subscript(next)), self.t("ext_index_tip")) {
                        result = Some(picked.replacen(core, &sem::with_index(base, next), 1));
                    }
                    if btn(ui, "№".into(), self.t("ext_index_pick")) {
                        self.index_pick = !self.index_pick;
                    }
                    if btn(ui, format!("{}²⁰²⁶", self.t("ext_date")), self.t("ext_date_tip")) {
                        result = Some(sem::date(&picked, now.0, now.1, now.2));
                    }
                    if btn(ui, "«…»".into(), self.t("ext_quote_tip")) {
                        result = Some(sem::quote(&picked));
                    }
                    if btn(ui, self.t("ext_etc").to_string(), self.t("ext_etc_tip")) {
                        result = Some(format!("{picked} {}", self.t("ext_etc")));
                    }
                    if picked.trim().contains(' ') && btn(ui, self.t("ext_hyphen").to_string(), self.t("ext_hyphen_tip")) {
                        result = Some(sem::hyphen(&picked));
                    }
                    if btn(ui, self.t("ext_tome").to_string(), self.t("ext_tome_tip")) {
                        result = Some(format!("{picked} {}", self.t("ext_tome_text")));
                    }
                });
                if self.index_pick {
                    let core = picked.trim();
                    let (base, has) = sem::split_index(core);
                    ui.horizontal(|ui| {
                        for n in 1..=9u32 {
                            if ui.add(egui::Button::new(RichText::new(sem::with_index(base, n)).color(c.ink)).fill(c.card).rounding(Rounding::same(6.0))).clicked() {
                                result = Some(picked.replacen(core, &sem::with_index(base, n), 1));
                            }
                        }
                        if has.is_some() && ui.button(self.t("ext_index_none")).clicked() {
                            result = Some(picked.replacen(core, base, 1));
                        }
                    });
                }
            });
        });
        if let Some(r) = result {
            let mut s: String = chars[..a].iter().collect();
            s.push_str(&r);
            let caret = s.chars().count();
            s.extend(chars[b..].iter());
            self.input = s;
            self.sel = None;
            self.index_pick = false;
            self.focus_input = 3;
            // put the caret after the changed word (drops the old selection)
            let id = Id::new("composer_input");
            if let Some(mut st) = egui::TextEdit::load_state(ctx, id) {
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret))));
                st.store(ctx, id);
            }
        }
    }

    fn subscribe(&mut self, branch: &str, on: bool) {
        if on {
            if !self.settings.subs.iter().any(|b| b == branch) {
                self.settings.subs.push(branch.to_string());
            }
            for (c, _, _) in cybercord_node::city::TEXT {
                let _ = self.handle.cmd_tx.send(Cmd::Subscribe(ChannelId::new(cybercord_node::city::guild(branch), *c)));
            }
            self.city_welcome = Some(branch.to_string());
        } else {
            self.settings.subs.retain(|b| b != branch);
            if self.sub_sel.0 == branch {
                self.sub_sel = (String::new(), String::new());
                if self.settings.subs.is_empty() {
                    self.show_subs = false;
                }
            }
        }
        store::save_settings(&self.settings);
    }

    fn open_branch(&mut self, branch: &str, channel: &str) {
        self.sub_sel = (branch.to_string(), channel.to_string());
        self.show_subs = true;
        self.show_city = false;
        self.show_papers = false;
        self.show_map = false;
        self.show_home = false;
        self.show_friends = false;
        self.show_downloads = false;
    }

    /// Bottom of the side panel: "voice connected" box + me + settings.
    fn me_panel(&mut self, ui: &mut egui::Ui) {
        // bottom: "voice connected" box + user panel with settings (Discord-like)
        egui::TopBottomPanel::bottom("me").frame(egui::Frame::none().fill(th().surface)).show_separator_line(false).show_inside(ui, |ui| {
            self.voice_box(ui);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let initial = self.self_name.chars().next().unwrap_or('?').to_uppercase().to_string();
                avatar(ui, &initial, 16.0, th().accent, if self.mic_on && self.in_voice.is_some() { Some(th().green) } else { None });
                ui.vertical(|ui| {
                    ui.label(RichText::new(&self.self_name).color(th().ink).b());
                    let st = self.settings.status.clone();
                    let label = if st.is_empty() { format!("{}…", self.self_id.chars().take(7).collect::<String>()) } else { self.status_label(&st).to_string() };
                    let col = status_color(&st).unwrap_or(th().ink2);
                    ui.menu_button(RichText::new(format!("● {label}")).color(col).small(), |ui| {
                        for (sid, key) in crate::tray::STATUSES {
                            let text = RichText::new(format!("●  {}", self.t(key))).color(status_color(sid).unwrap_or(th().ink2));
                            if ui.selectable_label(st == *sid, text).clicked() {
                                self.set_status(sid);
                                ui.close_menu();
                            }
                        }
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let gear = self.icons.get(ui.ctx(), "gear", SYM_GEAR);
                    let g = icon_button(ui, gear, th().ink2, th().card, 34.0).on_hover_text(self.t("settings"));
                    self.anchor("settings_btn", g.rect);
                    if g.clicked() {
                        self.show_settings = true;
                        self.set_opened = Instant::now();
                    }
                    // add a friend / my code, next to the settings
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::click());
                    let k = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
                    ui.painter().rect_filled(r, Rounding::same(8.0), th().green.gamma_multiply(0.85 + 0.15 * k));
                    plus_glyph(ui.painter(), r.center(), 7.0, th().on_accent);
                    if resp.on_hover_text(self.t("addf_tip")).clicked() {
                        self.open_add_friend();
                    }
                });
            });
            ui.add_space(2.0);
        });
    }

    /// Side panel while the city map is open: districts → branches.
    fn city_column(&mut self, ui: &mut egui::Ui) {
        use cybercord_node::city::{BRANCHES, DISTRICTS};
        let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
        ui.label(RichText::new(self.t("city_h")).color(th().ink).b().size(15.5));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for d in DISTRICTS {
                let col = Color32::from_rgb(d.color.0, d.color.1, d.color.2);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, col);
                    section_label(ui, if ru { d.ru } else { d.en });
                });
                for b in BRANCHES.iter().filter(|b| b.district == d.slug) {
                    let subbed = self.settings.subs.iter().any(|s| s == b.slug);
                    let sel = self.city_sel.as_deref() == Some(b.slug);
                    let name = format!("{}{}", if ru { b.ru } else { b.en }, if subbed { "  ✓" } else { "" });
                    let txt = RichText::new(name).color(if sel { th().ink } else { th().ink2 });
                    if ui.selectable_label(sel, txt).clicked() {
                        self.city_sel = Some(b.slug.to_string());
                    }
                }
                ui.add_space(6.0);
            }
        });
    }

    /// Side panel in "subscriptions" mode: my branches with their channels.
    fn subs_column(&mut self, ctx: &egui::Context) {
        let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
        egui::SidePanel::left("subs").exact_width(236.0).resizable(false).frame(egui::Frame::none().fill(th().surface).inner_margin(egui::Margin::same(12.0))).show(ctx, |ui| {
            self.me_panel(ui);
            ui.label(RichText::new(self.t("subs_h")).color(th().ink).b().size(15.5));
            ui.add_space(6.0);
            let hash = self.icons.get(ui.ctx(), "hash", SYM_HASH);
            let spk = self.icons.get(ui.ctx(), "speaker", SYM_SPEAKER);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for slug in self.settings.subs.clone() {
                    let Some(b) = cybercord_node::city::branch(&slug) else { continue };
                    let col = cybercord_node::city::district(b.district).map(|d| Color32::from_rgb(d.color.0, d.color.1, d.color.2)).unwrap_or(th().accent);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, col);
                        section_label(ui, if ru { b.ru } else { b.en });
                    });
                    for (c, _, _) in cybercord_node::city::TEXT {
                        let sel = self.sub_sel == (slug.clone(), c.to_string());
                        let label = self.ch_label(c);
                        if channel_row(ui, hash, &label, sel, false).clicked() {
                            self.sub_sel = (slug.clone(), c.to_string());
                        }
                    }
                    let room = format!("{}/{}", cybercord_node::city::guild(&slug), cybercord_node::city::VOICE.0);
                    let mine = self.in_voice.as_deref() == Some(room.as_str());
                    let label = self.ch_label(cybercord_node::city::VOICE.0);
                    if channel_row(ui, spk, &label, mine, true).on_hover_text(self.t("voice_join_tip")).clicked() && !mine {
                        let _ = self.handle.cmd_tx.send(Cmd::JoinVoice { channel: ChannelId::new(cybercord_node::city::guild(&slug), cybercord_node::city::VOICE.0) });
                    }
                    ui.add_space(6.0);
                }
                if ui.link(self.t("city_open_map")).clicked() {
                    self.show_city = true;
                    self.show_papers = false;
                    self.show_subs = false;
                }
            });
        });
    }

    /// The SocioCyberneering city: districts in a ring around the centre,
    /// branches as nodes; search, a card with the goal, subscribe.
    fn city_view(&mut self, ui: &mut egui::Ui) {
        use cybercord_node::city::{BRANCHES, DISTRICTS};
        let c = th();
        let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.t("city_h")).color(c.ink).b().size(20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = self.t("city_search");
                ui.add(egui::TextEdit::singleline(&mut self.city_query).hint_text(hint).desired_width(260.0));
            });
        });
        ui.colored_label(c.ink2, RichText::new(self.t("city_sub")).small());
        ui.add_space(6.0);
        let q = self.city_query.trim().to_lowercase();
        let matches = |b: &cybercord_node::city::Branch| q.is_empty() || b.ru.to_lowercase().contains(&q) || b.en.to_lowercase().contains(&q) || b.goal_ru.to_lowercase().contains(&q) || b.goal_en.to_lowercase().contains(&q);
        let avail = ui.available_size();
        let card_w = 300.0;
        ui.horizontal_top(|ui| {
            // --- the map
            let size = (avail.x - card_w - 20.0).min(avail.y - 10.0).max(300.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
            let p = ui.painter_at(rect);
            let center = rect.center();
            let r_out = size / 2.0 - 8.0;
            let r_in = r_out * 0.30;
            p.circle_filled(center, r_out, c.surface);
            let n = DISTRICTS.len();
            let hover = resp.hover_pos();
            let mut hovered: Option<&'static str> = None;
            for (k, d) in DISTRICTS.iter().enumerate() {
                let a0 = std::f32::consts::TAU * k as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
                let a1 = std::f32::consts::TAU * (k + 1) as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
                let col = Color32::from_rgb(d.color.0, d.color.1, d.color.2);
                // district sector (ring segment)
                let mut pts = Vec::new();
                for i in 0..=24 {
                    let a = a0 + (a1 - a0) * i as f32 / 24.0;
                    pts.push(center + egui::vec2(a.cos(), a.sin()) * (r_out - 2.0));
                }
                for i in (0..=24).rev() {
                    let a = a0 + (a1 - a0) * i as f32 / 24.0;
                    pts.push(center + egui::vec2(a.cos(), a.sin()) * (r_in + 6.0));
                }
                p.add(egui::Shape::convex_polygon(pts.clone(), col.gamma_multiply(0.13), Stroke::new(1.5, c.window)));
                // label
                let am = (a0 + a1) / 2.0;
                let lp = center + egui::vec2(am.cos(), am.sin()) * (r_in + (r_out - r_in) * 0.90);
                let g = ui.fonts(|f| f.layout_no_wrap((if ru { d.ru } else { d.en }).to_string(), egui::FontId::new(11.5, egui::FontFamily::Name("bold".into())), col));
                let x = (lp.x - g.size().x / 2.0).clamp(rect.left() + 2.0, rect.right() - g.size().x - 2.0);
                p.galley(egui::pos2(x, lp.y - g.size().y / 2.0), g, col);
                // branches as nodes on two rings inside the sector
                let bs: Vec<&cybercord_node::city::Branch> = BRANCHES.iter().filter(|b| b.district == d.slug).collect();
                let m = bs.len().max(1);
                for (j, b) in bs.iter().enumerate() {
                    let a = a0 + (a1 - a0) * (j as f32 + 0.5) / m as f32;
                    let rr = r_in + (r_out - r_in) * [0.22, 0.47, 0.70][if m > 6 { j % 3 } else { 1 + j % 2 }];
                    let pos = center + egui::vec2(a.cos(), a.sin()) * rr;
                    let subbed = self.settings.subs.iter().any(|s| s == b.slug);
                    let selected = self.city_sel.as_deref() == Some(b.slug);
                    let hit = matches(b);
                    let rad = if selected { 11.0 } else { 8.0 };
                    let near = hover.map(|h| h.distance(pos) < 12.0).unwrap_or(false);
                    if near {
                        hovered = Some(b.slug);
                    }
                    p.line_segment([center + egui::vec2(a.cos(), a.sin()) * (r_in + 6.0), pos], Stroke::new(1.0, col.gamma_multiply(if hit { 0.5 } else { 0.15 })));
                    p.circle_filled(pos, rad + 2.0, c.window);
                    p.circle_filled(pos, rad, if hit { col } else { col.gamma_multiply(0.25) });
                    if subbed {
                        p.circle_stroke(pos, rad + 4.0, Stroke::new(2.0, c.green));
                    }
                    if selected || near || (!q.is_empty() && hit) {
                        p.text(pos + egui::vec2(0.0, rad + 9.0), Align2::CENTER_CENTER, if ru { b.ru } else { b.en }, egui::FontId::proportional(11.0), c.ink);
                    }
                }
            }
            // centre: SocioCyberneering, the First City
            p.circle_filled(center, r_in, Color32::from_rgb(0x0b, 0x12, 0x20));
            p.text(center - egui::vec2(0.0, r_in * 0.66), Align2::CENTER_CENTER, self.t("city_h"), egui::FontId::new((r_in * 0.12).clamp(9.0, 13.0), egui::FontFamily::Name("bold".into())), Color32::from_rgb(0x9f, 0xd8, 0xff));
            let tex = self.icons.get(ui.ctx(), "app256", APP_ICON_256);
            p.image(tex, Rect::from_center_size(center - egui::vec2(0.0, 8.0), egui::vec2(r_in * 0.9, r_in * 0.9)), Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            let mut job = egui::text::LayoutJob::default();
            job.append("SocioCyberneering Inc.", 0.0, egui::TextFormat { font_id: egui::FontId::new((r_in * 0.11).clamp(8.0, 12.0), egui::FontFamily::Name("brand".into())), color: Color32::WHITE, extra_letter_spacing: 1.2, ..Default::default() });
            let g = ui.fonts(|f| f.layout_job(job));
            p.galley(center + egui::vec2(-g.size().x / 2.0, r_in * 0.48), g, Color32::WHITE);
            if let Some(h) = hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    self.city_sel = Some(h.to_string());
                }
            }
            // --- the card / search results
            ui.vertical(|ui| {
                ui.set_width(card_w);
                if !q.is_empty() {
                    ui.label(RichText::new(self.t("city_found")).color(c.ink2).small().b());
                    for b in BRANCHES.iter().filter(|b| matches(b)).take(12) {
                        if ui.link(if ru { b.ru } else { b.en }).clicked() {
                            self.city_sel = Some(b.slug.to_string());
                        }
                    }
                    ui.separator();
                }
                let Some(b) = self.city_sel.as_deref().and_then(cybercord_node::city::branch) else {
                    ui.colored_label(c.ink2, self.t("city_pick"));
                    return;
                };
                let d = cybercord_node::city::district(b.district);
                let col = d.map(|d| Color32::from_rgb(d.color.0, d.color.1, d.color.2)).unwrap_or(c.accent);
                egui::Frame::none().fill(c.surface).stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                    ui.set_width(card_w - 28.0);
                    ui.label(RichText::new(d.map(|d| if ru { d.ru } else { d.en }).unwrap_or("")).color(col).small().b());
                    ui.label(RichText::new(if ru { b.ru } else { b.en }).color(c.ink).b().size(18.0));
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(if ru { b.goal_ru } else { b.goal_en }).color(c.ink)).wrap());
                    ui.add_space(8.0);
                    ui.label(RichText::new(self.t("city_channels")).color(c.ink2).small().b());
                    for (ch, _, _) in cybercord_node::city::TEXT {
                        ui.label(RichText::new(format!("# {}", self.ch_label(ch))).color(c.ink));
                    }
                    ui.label(RichText::new(format!("🔊 {}", self.ch_label(cybercord_node::city::VOICE.0))).color(c.ink));
                    ui.add_space(10.0);
                    let subbed = self.settings.subs.iter().any(|s| s == b.slug);
                    let slug = b.slug.to_string();
                    ui.horizontal(|ui| {
                        if subbed {
                            if ui.add(egui::Button::new(RichText::new(self.t("city_open")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                                self.open_branch(&slug, "chat");
                            }
                            if ui.button(self.t("city_unsub")).clicked() {
                                self.subscribe(&slug, false);
                            }
                        } else if ui.add(egui::Button::new(RichText::new(self.t("city_sub_btn")).color(c.on_accent).b()).fill(c.green).rounding(Rounding::same(8.0))).clicked() {
                            self.subscribe(&slug, true);
                        }
                    });
                });
            });
        });
    }

    /// After subscribing: what the branch is for, its channels, how to start.
    fn city_welcome_window(&mut self, ctx: &egui::Context) {
        let Some(slug) = self.city_welcome.clone() else { return };
        let Some(b) = cybercord_node::city::branch(&slug) else {
            self.city_welcome = None;
            return;
        };
        let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
        let c = th();
        let mut open = true;
        let mut go = false;
        egui::Window::new(if ru { b.ru } else { b.en }).collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).order(Order::Foreground).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(self.t("city_welcome")).color(c.green).b());
            ui.add_space(4.0);
            ui.label(RichText::new(self.t("city_goal")).color(c.ink2).small().b());
            ui.add(egui::Label::new(RichText::new(if ru { b.goal_ru } else { b.goal_en }).color(c.ink)).wrap());
            ui.add_space(6.0);
            ui.label(RichText::new(self.t("city_channels")).color(c.ink2).small().b());
            for (ch, _, _) in cybercord_node::city::TEXT {
                ui.label(format!("# {} — {}", self.ch_label(ch), self.td(&format!("city_ch_{ch}"))));
            }
            ui.label(format!("🔊 {}", self.ch_label(cybercord_node::city::VOICE.0)));
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(self.t("city_welcome_tip")).color(c.ink2).small()).wrap());
            ui.add_space(8.0);
            if ui.add(egui::Button::new(RichText::new(self.t("city_open")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                go = true;
            }
        });
        if go {
            self.open_branch(&slug, "chat");
            self.city_welcome = None;
        } else if !open {
            self.city_welcome = None;
        }
    }

    fn drain_papers(&mut self) {
        use crate::papers::Msg;
        while let Ok(m) = self.papers.rx.try_recv() {
            match m {
                Msg::Results(gen, r) => {
                    if gen != self.papers.gen {
                        continue;
                    }
                    self.papers_busy = false;
                    self.papers_expanded.clear();
                    match r {
                        Ok(v) => {
                            self.papers_err = None;
                            self.papers_res = v;
                        }
                        Err(e) => self.papers_err = Some(e),
                    }
                }
                Msg::Pdf(url, r) => {
                    let Some((paper, share)) = self.papers_pending.remove(&url) else { continue };
                    match r {
                        Ok(bytes) => {
                            let dir = cybercord_node::transfer::download_dir();
                            let _ = std::fs::create_dir_all(&dir);
                            let path = dir.join(paper.file_name());
                            if let Err(e) = std::fs::write(&path, &bytes) {
                                self.toast(format!("{}: {e}", path.display()));
                                continue;
                            }
                            if share {
                                self.show_papers = false;
                                self.input = paper.citation();
                                self.send_path(path.clone());
                            } else {
                                self.toast(self.t("papers_saved").replace("{p}", &path.display().to_string()));
                                self.pdf = Some(crate::pdfview::PdfView::open(paper.title.clone(), paper.file_name(), Arc::new(bytes)));
                            }
                        }
                        Err(e) => self.toast(format!("{}: {e}", self.t("papers_pdf_failed"))),
                    }
                }
            }
        }
    }

    fn papers_search(&mut self, ctx: &egui::Context) {
        let q = self.papers_q.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.papers_busy = true;
        self.papers_err = None;
        self.papers.run(q, self.papers_src, self.papers_oa, ctx.clone());
    }

    /// Scientific article search: OpenAlex + arXiv; read, download, share.
    fn papers_view(&mut self, ui: &mut egui::Ui) {
        use crate::papers::Source;
        let c = th();
        ui.label(RichText::new(self.t("papers_h")).color(c.ink).b().size(20.0));
        ui.colored_label(c.ink2, RichText::new(self.t("papers_sub")).small());
        ui.add_space(6.0);
        let mut go = false;
        ui.horizontal(|ui| {
            let hint = self.t("papers_hint");
            let r = ui.add(egui::TextEdit::singleline(&mut self.papers_q).hint_text(hint).desired_width(ui.available_width() - 130.0));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            if ui.add(egui::Button::new(RichText::new(self.t("papers_go")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(110.0, 28.0))).clicked() {
                go = true;
            }
        });
        ui.horizontal(|ui| {
            let before = (self.papers_src, self.papers_oa);
            ui.selectable_value(&mut self.papers_src, Source::OpenAlex, "OpenAlex").on_hover_text(self.t("papers_openalex_tip"));
            ui.selectable_value(&mut self.papers_src, Source::Arxiv, "arXiv").on_hover_text(self.t("papers_arxiv_tip"));
            ui.separator();
            let oa_label = self.t("papers_oa").to_string();
            ui.add_enabled_ui(self.papers_src == Source::OpenAlex, |ui| {
                ui.checkbox(&mut self.papers_oa, oa_label);
            });
            if before != (self.papers_src, self.papers_oa) && !self.papers_q.trim().is_empty() {
                go = true;
            }
            if self.papers_busy {
                ui.spinner();
            }
        });
        if go {
            self.papers_search(ui.ctx());
        }
        ui.add_space(4.0);
        if let Some(e) = &self.papers_err {
            ui.add(egui::Label::new(RichText::new(format!("{}: {e}", self.t("papers_failed"))).color(c.red)).wrap());
        }
        if self.papers_res.is_empty() && !self.papers_busy {
            ui.add_space(20.0);
            ui.colored_label(c.ink2, self.t("papers_empty"));
            ui.horizontal_wrapped(|ui| {
                for q in ["helium planet formation", "aquaponics nitrogen cycle", "general semantics", "resource based economy", "KiCad PCB"] {
                    if ui.small_button(q).clicked() {
                        self.papers_q = q.to_string();
                        self.papers_search(ui.ctx());
                    }
                }
            });
            return;
        }
        let res = self.papers_res.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, p) in res.iter().enumerate() {
                egui::Frame::none().fill(c.surface).stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(&p.title).color(c.ink).b().size(15.0)).wrap());
                    let mut meta = p.authors_short();
                    if let Some(y) = p.year {
                        meta.push_str(&format!(" · {y}"));
                    }
                    if !p.venue.is_empty() {
                        meta.push_str(&format!(" · {}", p.venue));
                    }
                    if let Some(n) = p.cited {
                        meta.push_str(&format!(" · {}", self.t("papers_cited").replace("{n}", &n.to_string())));
                    }
                    ui.add(egui::Label::new(RichText::new(meta).color(c.ink2).small()).wrap());
                    if !p.summary.is_empty() {
                        let open = self.papers_expanded.contains(&i);
                        let text = if open || p.summary.chars().count() <= 320 { p.summary.clone() } else { format!("{}…", p.summary.chars().take(320).collect::<String>()) };
                        ui.add(egui::Label::new(RichText::new(text).color(c.ink)).wrap());
                        if p.summary.chars().count() > 320 && ui.link(if open { self.t("papers_less") } else { self.t("papers_more") }).clicked() {
                            if open {
                                self.papers_expanded.remove(&i);
                            } else {
                                self.papers_expanded.insert(i);
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if let Some(url) = &p.pdf {
                            let busy = self.papers_pending.contains_key(url);
                            if ui.add_enabled(!busy, egui::Button::new(RichText::new(self.t("papers_read")).color(c.on_accent)).fill(c.accent)).clicked() {
                                self.papers_pending.insert(url.clone(), (p.clone(), false));
                                self.papers.fetch_pdf(url.clone(), ui.ctx().clone());
                            }
                            if ui.add_enabled(!busy, egui::Button::new(self.t("papers_send_pdf"))).clicked() {
                                self.papers_pending.insert(url.clone(), (p.clone(), true));
                                self.papers.fetch_pdf(url.clone(), ui.ctx().clone());
                            }
                            if busy {
                                ui.spinner();
                            }
                        } else {
                            ui.colored_label(c.ink2, RichText::new(self.t("papers_no_pdf")).small());
                        }
                        if ui.button(self.t("papers_link")).clicked() {
                            self.input = p.citation();
                            self.focus_input = 3;
                            self.show_papers = false;
                        }
                        if !p.page.is_empty() && ui.button(self.t("papers_page")).clicked() {
                            open_path(std::path::Path::new(&p.page));
                        }
                    });
                });
                ui.add_space(8.0);
            }
        });
    }

    /// Play an interface sound, if that group is on (and not the same sound
    /// a moment ago).
    fn play_sfx(&mut self, s: crate::sounds::Sfx) {
        use crate::sounds::Sfx::*;
        let st = &self.settings;
        let on = st.snd_on
            && match s {
                Brand => st.snd_start,
                Message => st.snd_msg,
                Mention => st.snd_mention,
                Sent => st.snd_sent,
                VoiceJoin | VoiceLeave | PeerJoin | PeerLeave => st.snd_voice,
                Ring | Ringback | CallStart | CallEnd => st.snd_call,
            };
        if !on {
            return;
        }
        if self.sfx_last.get(&s).map(|t| t.elapsed().as_millis() < 700).unwrap_or(false) {
            return;
        }
        self.sfx_last.insert(s, Instant::now());
        self.play_sfx_now(s);
    }

    fn play_sfx_now(&mut self, s: crate::sounds::Sfx) {
        self.sfx.retain(|p| !p.state.finished());
        if self.sfx.len() < 6 {
            if let Some(p) = crate::music::play(crate::sounds::track_at(s, self.settings.snd_vol)) {
                self.sfx.push(p);
            }
        }
    }

    /// "Your code": the connect code with copy buttons.
    fn my_handle(&self) -> String {
        format!("@{}", cybercord_node::types::handle(&self.self_name, &self.self_id))
    }

    fn my_code_body(&mut self, ui: &mut egui::Ui) {
        // the short handle first: one line, easy to send
        let h = self.my_handle();
        let c = th();
        egui::Frame::none().fill(c.accent.gamma_multiply(0.10)).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(self.t("handle_your")).color(c.ink2).small());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&h).color(c.accent).b().size(22.0).monospace());
                ui.add_space(8.0);
                if ui.add(egui::Button::new(RichText::new(self.t("handle_copy")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 30.0))).clicked() {
                    ui.output_mut(|o| o.copied_text = h.clone());
                    let m = self.t("code_copied").to_string();
                    self.toast(m);
                }
            });
            ui.add(egui::Label::new(RichText::new(self.t("handle_how")).color(c.ink2).small()).wrap());
        });
        ui.add_space(4.0);
        let label = self.t("handle_full").to_string();
        egui::CollapsingHeader::new(label).id_source("full_code").default_open(false).show(ui, |ui| {
            self.full_code_body(ui);
        });
    }

    /// The long connect code (ID + addresses) — works even when the DHT is not reachable.
    fn full_code_body(&mut self, ui: &mut egui::Ui) {
        let code = self.connect_code();
        ui.colored_label(th().ink2, self.t("your_code_body"));
        ui.add_space(6.0);
        let mut shown = if code.is_empty() { self.t("starting").to_string() } else { code.clone() };
        ui.add(egui::TextEdit::multiline(&mut shown).font(egui::TextStyle::Monospace).desired_rows(3).desired_width(f32::INFINITY));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new(self.t("copy_code")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0))).clicked() {
                ui.output_mut(|o| o.copied_text = code.clone());
                let m = self.t("code_copied").to_string();
                self.toast(m);
            }
            if ui.add(egui::Button::new(RichText::new(self.t("copy_id")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0))).on_hover_text(self.t("your_id_body")).clicked() {
                ui.output_mut(|o| o.copied_text = self.self_id.clone());
                let m = self.t("id_copied").to_string();
                self.toast(m);
            }
            ui.colored_label(th().ink2, RichText::new(format!("{}: {}", self.t("your_id"), short(&self.self_id))).monospace().small());
        });
    }

    /// "Add a friend": name + their code + Add.
    fn add_friend_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.colored_label(th().ink2, self.t("name"));
            let hint = self.t("name_hint");
            ui.add(egui::TextEdit::singleline(&mut self.add_name).hint_text(hint).desired_width(220.0));
        });
        ui.add_space(4.0);
        ui.colored_label(th().ink2, self.t("their_code"));
        ui.add(egui::TextEdit::multiline(&mut self.add_code).font(egui::TextStyle::Monospace).hint_text("@alisa-7kq2m9   ·   12D3Koo…   ·   /ip4/…/p2p/12D3Koo…").desired_rows(2).desired_width(f32::INFINITY));
        ui.add_space(6.0);
        if ui.add(egui::Button::new(RichText::new(self.t("add_btn")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0))).clicked() {
            let handle_tag = cybercord_node::types::parse_handle(&self.add_code);
            match parse_connect_code(&self.add_code) {
                _ if handle_tag.as_deref() == Some(cybercord_node::types::handle_tag(&self.self_id).as_str()) => self.add_msg = self.t("own_code").to_string(),
                None if handle_tag.is_some() => {
                    // a handle: look it up in the users' DHT, add when it answers
                    let q = self.add_code.trim().to_string();
                    self.pending_handle = Some((handle_tag.clone().unwrap_or_default(), self.add_name.trim().to_string(), Instant::now()));
                    self.search_q = q.clone();
                    self.search_res.clear();
                    let _ = self.handle.cmd_tx.send(Cmd::Search(q.clone()));
                    self.add_msg = self.t("handle_searching").replace("{h}", &q);
                }
                Some((peer_id, _)) if peer_id == self.self_id => self.add_msg = self.t("own_code").to_string(),
                Some((peer_id, addrs)) => {
                    let name = if self.add_name.trim().is_empty() { short(&peer_id) } else { self.add_name.trim().to_string() };
                    let _ = self.handle.cmd_tx.send(Cmd::AddFriend(Friend { peer_id: peer_id.clone(), name: name.clone(), addrs }));
                    self.friends.entry(peer_id.clone()).or_insert(FriendView { name: name.clone(), online: false });
                    self.add_msg = self.t("added").replace("{name}", &name);
                    self.add_code.clear();
                    self.add_name.clear();
                    self.dm_open = Some(peer_id);
                }
                None => self.add_msg = self.t("bad_code").to_string(),
            }
        }
        if !self.add_msg.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(th().accent, &self.add_msg);
        }
    }

    /// One window with everything for adding a friend: search by name,
    /// my code to send, their code to paste, and an invitation.
    fn add_friend_window(&mut self, ctx: &egui::Context) {
        if !self.show_add_friend {
            return;
        }
        let c = th();
        let mut open = true;
        egui::Window::new(self.t("addf_title")).open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).order(Order::Foreground).show(ctx, |ui| {
            ui.set_width(560.0);
            egui::ScrollArea::vertical().max_height(ctx.screen_rect().height() - 140.0).show(ui, |ui| {
                ui.label(RichText::new(self.t("addf_s1")).color(c.ink).b());
                self.people_search(ui, true);
                ui.add_space(10.0);
                ui.separator();
                ui.label(RichText::new(self.t("addf_s2")).color(c.ink).b());
                self.my_code_body(ui);
                ui.add_space(10.0);
                ui.separator();
                ui.label(RichText::new(self.t("addf_s3")).color(c.ink).b());
                self.add_friend_body(ui);
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.colored_label(c.ink2, self.t("addf_not_yet"));
                    if ui.add(egui::Button::new(RichText::new(self.t("invite_btn")).color(c.on_accent).b()).fill(c.green).rounding(Rounding::same(8.0))).clicked() {
                        self.show_invite = true;
                        self.show_add_friend = false;
                    }
                });
            });
        });
        if !open {
            self.show_add_friend = false;
        }
    }

    fn open_add_friend(&mut self) {
        self.show_add_friend = true;
        self.add_msg.clear();
    }

    // ---- inviting friends ----

    fn invite_card(&mut self, ui: &mut egui::Ui) {
        let c = th();
        egui::Frame::none().fill(c.green.gamma_multiply(0.12)).stroke(Stroke::new(1.0, c.green.gamma_multiply(0.5))).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 190.0).max(200.0));
                    ui.label(RichText::new(self.t("invite_h")).color(c.ink).b().size(16.0));
                    ui.add(egui::Label::new(RichText::new(self.t("invite_sub")).color(c.ink2)).wrap());
                });
                ui.vertical(|ui| {
                    if ui.add(egui::Button::new(RichText::new(self.t("invite_btn")).color(c.on_accent).b()).fill(c.green).rounding(Rounding::same(10.0)).min_size(egui::vec2(170.0, 34.0))).clicked() {
                        self.show_invite = true;
                    }
                    if ui.add(egui::Button::new(RichText::new(self.t("addf_btn")).color(c.ink).b()).fill(c.card).rounding(Rounding::same(10.0)).min_size(egui::vec2(170.0, 34.0))).clicked() {
                        self.open_add_friend();
                    }
                });
            });
        });
    }

    fn invite_window(&mut self, ctx: &egui::Context) {
        if !self.show_invite {
            return;
        }
        let c = th();
        let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
        let text = crate::invite::text(ru, &cybercord_node::types::handle(&self.self_name, &self.self_id));
        if self.invite_qr.is_none() {
            if let Some(img) = crate::invite::qr_image(crate::invite::DOWNLOAD, 6) {
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                self.invite_qr = Some(ctx.load_texture("invite_qr", ci, egui::TextureOptions::NEAREST));
            }
        }
        let mut open = true;
        egui::Window::new(self.t("invite_h")).open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).order(Order::Foreground).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(400.0);
                    ui.add(egui::Label::new(RichText::new(self.t("invite_how")).color(c.ink2)).wrap());
                    ui.add_space(6.0);
                    let mut shown = text.clone();
                    ui.add(egui::TextEdit::multiline(&mut shown).desired_rows(9).desired_width(f32::INFINITY));
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(egui::Button::new(RichText::new(self.t("invite_copy")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                            ui.output_mut(|o| o.copied_text = text.clone());
                            let m = self.t("invite_copied").to_string();
                            self.toast(m);
                        }
                        if ui.button(self.t("invite_copy_link")).clicked() {
                            ui.output_mut(|o| o.copied_text = crate::invite::DOWNLOAD.to_string());
                            let m = self.t("invite_copied").to_string();
                            self.toast(m);
                        }
                        if ui.button(self.t("invite_card")).on_hover_text(self.t("invite_card_tip")).clicked() {
                            self.save_invite_card();
                        }
                    });
                });
                ui.vertical_centered(|ui| {
                    if let Some(t) = &self.invite_qr {
                        ui.add(egui::Image::new(SizedTexture::new(t.id(), egui::vec2(200.0, 200.0))));
                    }
                    ui.label(RichText::new(self.t("invite_scan")).color(c.ink2).small());
                });
            });
        });
        if !open {
            self.show_invite = false;
        }
    }

    fn save_invite_card(&mut self) {
        let img = crate::invite::card(&self.my_handle(), self.t("invite_tagline"), self.t("motto_sce"), self.t("invite_find_me"), self.t("invite_scan_short"));
        let dir = cybercord_node::transfer::download_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("cybercord-invite.png");
        match img.save(&path) {
            Ok(()) => {
                let m = self.t("papers_saved").replace("{p}", &path.display().to_string());
                self.toast(m);
                open_path(&path);
            }
            Err(e) => self.toast(format!("{}: {e}", path.display())),
        }
    }

    // ---- calls ----

    fn start_call(&mut self, peer: &str) {
        if !matches!(self.call, CallState::Idle) {
            let m = self.t("call_busy_me").to_string();
            self.toast(m);
            return;
        }
        let id: u64 = rand::random();
        let _ = self.handle.cmd_tx.send(Cmd::Call { peer: peer.to_string(), id, kind: CallKind::Ring, round: 0 });
        let _ = self.handle.cmd_tx.send(Cmd::JoinVoice { channel: ChannelId::dm(peer) });
        self.call = CallState::Outgoing { peer: peer.to_string(), id, since: Instant::now(), last_ring: Instant::now(), round: 0 };
        self.call_tone_at = None;
    }

    fn accept_call(&mut self) {
        if let CallState::Incoming { peer, id, .. } = self.call.clone() {
            let _ = self.handle.cmd_tx.send(Cmd::Call { peer: peer.clone(), id, kind: CallKind::Accept, round: 0 });
            let _ = self.handle.cmd_tx.send(Cmd::JoinVoice { channel: ChannelId::dm(&peer) });
            self.begin_active(peer, id);
            self.dm_open = Some(self.call_peer().unwrap_or_default());
            self.show_friends = true;
            self.show_home = false;
            self.show_map = false;
            self.show_city = false;
            self.show_papers = false;
            self.show_subs = false;
        }
    }

    fn begin_active(&mut self, peer: String, id: u64) {
        self.call = CallState::Active { peer, id, since: Instant::now() };
        self.mic_on = true;
        let _ = self.handle.cmd_tx.send(Cmd::SetMic(true));
        self.play_sfx(crate::sounds::Sfx::CallStart);
    }

    fn decline_call(&mut self) {
        if let CallState::Incoming { peer, id, .. } = self.call.clone() {
            let _ = self.handle.cmd_tx.send(Cmd::Call { peer, id, kind: CallKind::Decline, round: 0 });
            self.call = CallState::Idle;
        }
    }

    fn hang_up(&mut self) {
        match std::mem::replace(&mut self.call, CallState::Idle) {
            CallState::Outgoing { peer, id, .. } => {
                let _ = self.handle.cmd_tx.send(Cmd::Call { peer, id, kind: CallKind::Cancel, round: 0 });
            }
            CallState::Active { peer, id, .. } => {
                let _ = self.handle.cmd_tx.send(Cmd::Call { peer, id, kind: CallKind::End, round: 0 });
            }
            CallState::Incoming { .. } | CallState::Idle => return,
        }
        let _ = self.handle.cmd_tx.send(Cmd::LeaveVoice);
        self.play_sfx(crate::sounds::Sfx::CallEnd);
    }

    fn call_peer(&self) -> Option<String> {
        match &self.call {
            CallState::Outgoing { peer, .. } | CallState::Incoming { peer, .. } | CallState::Active { peer, .. } => Some(peer.clone()),
            CallState::Idle => None,
        }
    }

    fn on_call(&mut self, peer: String, name: String, id: u64, kind: CallKind) {
        let who = if self.friends.contains_key(&peer) { self.friend_name(&peer) } else { name.clone() };
        match (kind, self.call.clone()) {
            (CallKind::Ring, CallState::Idle) => {
                self.call = CallState::Incoming { peer, name: who, id, since: Instant::now(), last: Instant::now() };
                self.call_tone_at = None;
                self.hidden = false;
            }
            (CallKind::Ring, CallState::Incoming { peer: p, id: i, name: n, since, .. }) if p == peer && i == id => {
                self.call = CallState::Incoming { peer: p, name: n, id: i, since, last: Instant::now() };
            }
            // both called each other at the same moment: that is an answer
            (CallKind::Ring, CallState::Outgoing { peer: p, id: mine, .. }) if p == peer => {
                let _ = self.handle.cmd_tx.send(Cmd::Call { peer: peer.clone(), id: mine, kind: CallKind::Accept, round: 0 });
                self.begin_active(peer, mine);
            }
            (CallKind::Ring, CallState::Active { peer: p, .. }) if p == peer => {}
            (CallKind::Ring, _) => {
                let _ = self.handle.cmd_tx.send(Cmd::Call { peer, id, kind: CallKind::Busy, round: 0 });
            }
            (CallKind::Accept, CallState::Outgoing { peer: p, id: i, .. }) if p == peer && i == id => self.begin_active(peer, id),
            (CallKind::Decline | CallKind::Busy, CallState::Outgoing { peer: p, id: i, .. }) if p == peer && i == id => {
                self.call = CallState::Idle;
                let _ = self.handle.cmd_tx.send(Cmd::LeaveVoice);
                self.play_sfx(crate::sounds::Sfx::CallEnd);
                let key = if kind == CallKind::Busy { "call_busy" } else { "call_declined" };
                let m = self.t(key).replace("{name}", &who);
                self.toast(m);
            }
            (CallKind::Cancel, CallState::Incoming { peer: p, id: i, .. }) if p == peer && i == id => {
                self.call = CallState::Idle;
                let m = self.t("call_missed").replace("{name}", &who);
                self.toast(m);
            }
            (CallKind::End | CallKind::Cancel, CallState::Active { peer: p, id: i, since }) if p == peer && i == id => {
                self.call = CallState::Idle;
                let _ = self.handle.cmd_tx.send(Cmd::LeaveVoice);
                self.play_sfx(crate::sounds::Sfx::CallEnd);
                let m = self.t("call_ended").replace("{name}", &who).replace("{t}", &mmss(since.elapsed().as_secs()));
                self.toast(m);
            }
            _ => {}
        }
    }

    /// Ring again, time out, play the ringing tones.
    fn call_tick(&mut self, ctx: &egui::Context) {
        use crate::sounds::{seconds, Sfx};
        match self.call.clone() {
            CallState::Idle => return,
            CallState::Outgoing { peer, id, since, last_ring, round } => {
                if since.elapsed().as_secs() >= 45 {
                    let who = self.friend_name(&peer);
                    self.hang_up();
                    let m = self.t("call_no_answer").replace("{name}", &who);
                    self.toast(m);
                    return;
                }
                if last_ring.elapsed().as_secs_f32() >= 3.0 {
                    let _ = self.handle.cmd_tx.send(Cmd::Call { peer: peer.clone(), id, kind: CallKind::Ring, round: round + 1 });
                    self.call = CallState::Outgoing { peer, id, since, last_ring: Instant::now(), round: round + 1 };
                }
                if self.call_tone_at.map(|t| t.elapsed().as_secs_f32() >= seconds(Sfx::Ringback)).unwrap_or(true) {
                    self.call_tone_at = Some(Instant::now());
                    self.play_sfx(Sfx::Ringback);
                }
            }
            CallState::Incoming { last, peer, .. } => {
                // the caller stopped ringing (gone, or the cancel was lost)
                if last.elapsed().as_secs() >= 12 {
                    let who = self.friend_name(&peer);
                    self.call = CallState::Idle;
                    let m = self.t("call_missed").replace("{name}", &who);
                    self.toast(m);
                    return;
                }
                if self.call_tone_at.map(|t| t.elapsed().as_secs_f32() >= seconds(Sfx::Ring)).unwrap_or(true) {
                    self.call_tone_at = Some(Instant::now());
                    self.play_sfx(Sfx::Ring);
                }
            }
            CallState::Active { .. } => {}
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    /// Incoming call (centre) or the call bar (top) while calling / talking.
    fn call_window(&mut self, ctx: &egui::Context) {
        let c = th();
        match self.call.clone() {
            CallState::Idle => {}
            CallState::Incoming { name, since, .. } => {
                let mut act = 0;
                egui::Area::new(egui::Id::new("call_in")).order(Order::Foreground).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, -40.0)).show(ctx, |ui| {
                    egui::Frame::none().fill(c.surface).stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(18.0)).inner_margin(egui::Margin::same(24.0)).shadow(egui::epaint::Shadow { offset: egui::vec2(0.0, 8.0), blur: 32.0, spread: 0.0, color: Color32::from_black_alpha(90) }).show(ui, |ui| {
                        ui.set_width(320.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(self.t("call_incoming")).color(c.ink2));
                            ui.add_space(10.0);
                            // calm pulsing rings around the avatar
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 140.0), egui::Sense::hover());
                            let t = since.elapsed().as_secs_f32();
                            let p = ui.painter();
                            for k in 0..3 {
                                let ph = ((t * 0.6 + k as f32 / 3.0) % 1.0) as f32;
                                p.circle_stroke(rect.center(), 34.0 + ph * 34.0, Stroke::new(2.0, c.green.gamma_multiply(1.0 - ph)));
                            }
                            let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();
                            p.circle_filled(rect.center(), 34.0, c.accent);
                            p.text(rect.center(), Align2::CENTER_CENTER, initial, egui::FontId::new(30.0, egui::FontFamily::Name("bold".into())), c.on_accent);
                            ui.add_space(6.0);
                            ui.label(RichText::new(&name).color(c.ink).b().size(20.0));
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                ui.add_space(20.0);
                                if ui.add(egui::Button::new(RichText::new(self.t("call_decline")).color(Color32::WHITE).b()).fill(c.red).rounding(Rounding::same(22.0)).min_size(egui::vec2(120.0, 44.0))).clicked() {
                                    act = 2;
                                }
                                ui.add_space(12.0);
                                if ui.add(egui::Button::new(RichText::new(self.t("call_accept")).color(Color32::WHITE).b()).fill(c.green).rounding(Rounding::same(22.0)).min_size(egui::vec2(120.0, 44.0))).clicked() {
                                    act = 1;
                                }
                            });
                        });
                    });
                });
                if act == 1 {
                    self.accept_call();
                } else if act == 2 {
                    self.decline_call();
                }
                ctx.request_repaint();
            }
            CallState::Outgoing { peer, since, .. } | CallState::Active { peer, since, .. } => {
                let active = matches!(self.call, CallState::Active { .. });
                let who = self.friend_name(&peer);
                let mut hang = false;
                let mut toggle_mic = false;
                egui::Area::new(egui::Id::new("call_bar")).order(Order::Foreground).anchor(Align2::CENTER_TOP, egui::vec2(0.0, 62.0)).show(ctx, |ui| {
                    egui::Frame::none().fill(if active { c.green } else { c.accent }).rounding(Rounding::same(22.0)).inner_margin(egui::Margin::symmetric(16.0, 8.0)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let text = if active {
                                self.t("call_with").replace("{name}", &who).replace("{t}", &mmss(since.elapsed().as_secs()))
                            } else {
                                let dots = ".".repeat(1 + (since.elapsed().as_millis() / 500 % 3) as usize);
                                format!("{}{dots}", self.t("call_calling").replace("{name}", &who))
                            };
                            ui.label(RichText::new(text).color(Color32::WHITE).b());
                            ui.add_space(10.0);
                            if active {
                                let label = if self.mic_on { self.t("mic_on") } else { self.t("mic_off") };
                                if ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(Color32::from_black_alpha(50)).rounding(Rounding::same(14.0))).clicked() {
                                    toggle_mic = true;
                                }
                            }
                            if ui.add(egui::Button::new(RichText::new(if active { self.t("call_hangup") } else { self.t("call_cancel") }).color(Color32::WHITE).b()).fill(c.red).rounding(Rounding::same(14.0))).clicked() {
                                hang = true;
                            }
                        });
                    });
                });
                if toggle_mic {
                    self.mic_on = !self.mic_on;
                    let _ = self.handle.cmd_tx.send(Cmd::SetMic(self.mic_on));
                }
                if hang {
                    self.hang_up();
                }
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }

    /// Settings → Voice and sound: interface sounds.
    fn sounds_section(&mut self, ui: &mut egui::Ui) {
        use crate::sounds::Sfx;
        let c = th();
        ui.label(RichText::new(self.t("sfx_h").to_uppercase()).color(c.ink2).size(12.0).b());
        let mut changed = false;
        ui.horizontal(|ui| {
            let (l_on, l_vol) = (self.t("sfx_on").to_string(), self.t("sfx_vol").to_string());
            changed |= ui.checkbox(&mut self.settings.snd_on, l_on).changed();
            ui.add_space(12.0);
            ui.label(RichText::new(l_vol).color(c.ink2));
            changed |= ui.add_enabled(self.settings.snd_on, egui::Slider::new(&mut self.settings.snd_vol, 0.05..=1.0).show_value(false)).changed();
        });
        let groups: [(&str, &[Sfx]); 6] = [
            ("sfx_g_msg", &[Sfx::Message]),
            ("sfx_g_mention", &[Sfx::Mention]),
            ("sfx_g_voice", &[Sfx::VoiceJoin, Sfx::VoiceLeave, Sfx::PeerJoin, Sfx::PeerLeave]),
            ("sfx_g_call", &[Sfx::Ring, Sfx::Ringback, Sfx::CallStart, Sfx::CallEnd]),
            ("sfx_g_start", &[Sfx::Brand]),
            ("sfx_g_sent", &[Sfx::Sent]),
        ];
        ui.add_enabled_ui(self.settings.snd_on, |ui| {
            egui::Grid::new("sfx_grid").num_columns(2).spacing(egui::vec2(16.0, 6.0)).show(ui, |ui| {
                for (key, list) in groups {
                    let flag = match key {
                        "sfx_g_msg" => &mut self.settings.snd_msg,
                        "sfx_g_mention" => &mut self.settings.snd_mention,
                        "sfx_g_voice" => &mut self.settings.snd_voice,
                        "sfx_g_call" => &mut self.settings.snd_call,
                        "sfx_g_start" => &mut self.settings.snd_start,
                        _ => &mut self.settings.snd_sent,
                    };
                    let label = crate::i18n::tr_dyn(self.lang, key).to_string();
                    changed |= ui.checkbox(flag, label).changed();
                    ui.horizontal(|ui| {
                        for s in list.iter() {
                            if ui.small_button(format!("▶ {}", crate::i18n::tr_dyn(self.lang, s.key()))).clicked() {
                                self.play_sfx_now(*s);
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
        if changed {
            store::save_settings(&self.settings);
        }
    }

    /// Settings → About: check, download, install, restart.
    fn update_section(&mut self, ui: &mut egui::Ui) {
        use crate::update::State;
        let c = th();
        ui.label(RichText::new(self.t("upd_h").to_uppercase()).color(c.ink2).size(12.0).b());
        let st = self.updater.get();
        ui.horizontal_wrapped(|ui| {
            let busy = matches!(st, State::Checking | State::Downloading(_) | State::Installing(_));
            if ui.add_enabled(!busy, egui::Button::new(self.t("upd_check"))).clicked() {
                self.updater.check(ui.ctx().clone(), false);
            }
            let mut on = self.settings.update_check;
            if ui.checkbox(&mut on, self.t("upd_auto")).changed() {
                self.settings.update_check = on;
                store::save_settings(&self.settings);
            }
            if ui.button(self.t("upd_backups")).clicked() {
                let d = crate::update::backups_dir();
                let _ = std::fs::create_dir_all(&d);
                open_path(&d);
            }
        });
        match st {
            State::Idle => {}
            State::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.colored_label(c.ink2, self.t("upd_checking"));
                });
            }
            State::UpToDate => {
                ui.colored_label(c.green_text, self.t("upd_latest").replace("{v}", VERSION));
            }
            State::Available(r) => {
                egui::Frame::none().fill(c.card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                    ui.label(RichText::new(self.t("upd_avail").replace("{v}", &r.version).replace("{c}", &crate::update::local_version())).color(c.ink).b());
                    let notes: String = r.notes.lines().filter(|l| !l.trim().is_empty()).take(8).collect::<Vec<_>>().join("\n");
                    if !notes.is_empty() {
                        ui.add(egui::Label::new(RichText::new(notes).color(c.ink2).small()).wrap());
                    }
                    ui.add(egui::Label::new(RichText::new(self.t("upd_safe")).color(c.ink2).small()).wrap());
                    ui.horizontal(|ui| {
                        if r.asset.is_some() && ui.add(egui::Button::new(RichText::new(self.t("upd_install")).color(c.on_accent).b()).fill(c.green).rounding(Rounding::same(8.0))).clicked() {
                            self.updater.install(ui.ctx().clone(), r.clone());
                        }
                        if ui.button(self.t("upd_page")).clicked() {
                            open_path(std::path::Path::new(&r.page));
                        }
                    });
                });
            }
            State::Downloading(_) => {
                let done = self.updater.done.load(std::sync::atomic::Ordering::Relaxed);
                let total = self.updater.total.load(std::sync::atomic::Ordering::Relaxed).max(1);
                ui.add(egui::ProgressBar::new(done as f32 / total as f32).text(format!("{} {} / {}", self.t("upd_downloading"), human_size(done), human_size(total))));
            }
            State::Installing(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.colored_label(c.ink2, self.t("upd_installing"));
                });
            }
            State::Ready(v, backup) => {
                ui.colored_label(c.green_text, self.t("upd_ready").replace("{v}", &v));
                ui.colored_label(c.ink2, RichText::new(format!("{}: {}", self.t("upd_backup_at"), backup.display())).small());
                if ui.add(egui::Button::new(RichText::new(self.t("upd_restart")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                    crate::dev::stop_all();
                    let _ = self.handle.cmd_tx.send(Cmd::Quit);
                    crate::update::restart();
                }
            }
            State::Failed(e) => {
                ui.add(egui::Label::new(RichText::new(format!("{}: {e}", self.t("upd_failed"))).color(c.red)).wrap());
                if ui.button(self.t("upd_page")).clicked() {
                    open_path(std::path::Path::new(crate::update::RELEASES_PAGE));
                }
            }
        }
    }

    fn dev_on(&self) -> bool {
        self.settings.dev || std::env::var("CYBERCORD_DEV").is_ok()
    }

    /// The room test bots join: mine, or the first voice room of this server.
    fn dev_room(&self) -> String {
        self.in_voice.clone().unwrap_or_else(|| {
            let s = &SERVERS[self.sel_server];
            format!("{}/{}", s.slug, s.voice.first().copied().unwrap_or("lounge"))
        })
    }

    /// Developer mode: test voice on one computer.
    fn dev_window(&mut self, ctx: &egui::Context) {
        if !self.show_dev {
            return;
        }
        if !self.dev_on() {
            self.show_dev = false;
            return;
        }
        let c = th();
        let room = self.dev_room();
        let mut open = true;
        egui::Window::new(self.t("devm_title")).open(&mut open).resizable(false).collapsible(true).default_pos(egui::pos2(360.0, 80.0)).order(Order::Foreground).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new(self.t("devm_voice_h")).color(c.ink).b().size(16.0));
            ui.add(egui::Label::new(RichText::new(self.t("devm_voice_info")).color(c.ink2)).wrap());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}: ", self.t("devm_room"))).color(c.ink2));
                let (g, ch) = room.split_once('/').unwrap_or((room.as_str(), ""));
                let label = format!("{} / {}", SERVERS.iter().find(|s| s.slug == g).map(|s| s.name).unwrap_or(g), self.ch_label(ch));
                ui.label(RichText::new(label).color(c.ink).b());
                if self.in_voice.is_none() && ui.button(self.t("devm_join")).clicked() {
                    let (g, ch) = room.split_once('/').unwrap_or((room.as_str(), ""));
                    let _ = self.handle.cmd_tx.send(Cmd::JoinVoice { channel: ChannelId::new(g, ch) });
                }
            });
            ui.add_space(6.0);
            let boot = crate::dev::loopback_addr(&self.listen_addrs);
            if boot.is_none() {
                ui.colored_label(c.red, self.t("devm_no_addr"));
            }
            for kind in crate::dev::KINDS {
                ui.horizontal(|ui| {
                    let on = crate::dev::running(kind);
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 5.0, if on { c.green } else { c.line });
                    ui.vertical(|ui| {
                        ui.set_width(390.0);
                        ui.label(RichText::new(self.td(&format!("devm_{kind}"))).color(c.ink).b());
                        ui.add(egui::Label::new(RichText::new(self.td(&format!("devm_{kind}_d"))).color(c.ink2).small()).wrap());
                    });
                    if on {
                        if ui.button(self.t("devm_stop")).clicked() {
                            crate::dev::stop(kind);
                        }
                    } else if ui.add_enabled(boot.is_some(), egui::Button::new(RichText::new(self.t("devm_start")).color(c.on_accent)).fill(c.accent)).clicked() {
                        let name = self.td(&format!("devm_{kind}_name"));
                        if let Err(e) = crate::dev::start(kind, &name, &room, boot.clone(), &self.settings) {
                            self.toast(format!("{}: {e}", self.t("devm_title")));
                        }
                    }
                });
                ui.add_space(4.0);
            }
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(self.t("devm_headphones")).color(c.accent)).wrap());
            ui.separator();
            // live counters
            ui.label(RichText::new(self.t("devm_stats")).color(c.ink2).small().b());
            match (&self.in_voice, self.voice_stats) {
                (Some(_), Some((s, r, b))) => {
                    ui.label(self.t("devm_stats_line").replace("{s}", &(s / 5).to_string()).replace("{r}", &(r / 5).to_string()).replace("{b}", &b.to_string()));
                }
                _ => {
                    ui.colored_label(c.ink2, self.t("devm_not_in_voice"));
                }
            }
            let now = now_ms();
            let talking: Vec<String> = self.talkers.iter().filter(|(_, t)| now.saturating_sub(**t) < 800).map(|(n, _)| n.clone()).collect();
            let in_room: Vec<String> = self.roster.values().filter(|(_, r, _)| r.as_deref() == Some(room.as_str())).map(|(n, _, _)| n.clone()).collect();
            ui.label(format!("{}: {}", self.t("devm_in_room"), if in_room.is_empty() { "—".to_string() } else { in_room.join(", ") }));
            ui.label(format!("{}: {}", self.t("devm_talking"), if talking.is_empty() { "—".to_string() } else { talking.join(", ") }));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(self.t("devm_stop_all")).clicked() {
                    crate::dev::stop_all();
                }
                if ui.button(self.t("devm_logs")).clicked() {
                    let d = cybercord_node::store::data_dir().join("dev");
                    let _ = std::fs::create_dir_all(&d);
                    open_path(&d);
                }
            });
        });
        if !open {
            self.show_dev = false;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    /// A user's index code, if they have one and codes are shown.
    fn user_code(&self, peer: &str) -> Option<String> {
        if !self.settings.show_codes || peer.is_empty() {
            return None;
        }
        self.profiles.get(peer).map(|p| p.code()).filter(|c| !c.is_empty())
    }

    fn avatar_texture(&mut self, ctx: &egui::Context, p: &Profile) -> Option<egui::TextureId> {
        use base64::Engine;
        if p.avatar.is_empty() {
            return None;
        }
        if let Some((rev, t)) = self.avatar_tex.get(&p.peer) {
            if *rev == p.rev {
                return Some(t.id());
            }
        }
        let bytes = base64::engine::general_purpose::STANDARD_NO_PAD.decode(&p.avatar).ok()?;
        let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let t = ctx.load_texture(format!("av-{}", p.peer), ci, egui::TextureOptions::LINEAR);
        let id = t.id();
        self.avatar_tex.insert(p.peer.clone(), (p.rev, t));
        Some(id)
    }

    /// Someone's profile with the index code decoded.
    fn profile_window(&mut self, ctx: &egui::Context) {
        let Some(peer) = self.profile_view.clone() else { return };
        let c = th();
        let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
        let p = self.profiles.get(&peer).cloned();
        let name = p.as_ref().map(|p| p.name.clone()).or_else(|| self.peers.get(&peer).and_then(|v| v.name.clone())).unwrap_or_else(|| short(&peer));
        let mut open = true;
        egui::Window::new(self.t("profile_h")).collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).order(Order::Foreground).default_width(460.0).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.horizontal(|ui| {
                let tex = p.as_ref().and_then(|p| self.avatar_texture(ui.ctx(), p));
                match tex {
                    Some(t) => {
                        ui.add(egui::Image::new(SizedTexture::new(t, egui::vec2(72.0, 72.0))).rounding(Rounding::same(36.0)));
                    }
                    None => avatar(ui, &name.chars().next().unwrap_or('?').to_uppercase().to_string(), 36.0, name_color(&name), None),
                }
                ui.vertical(|ui| {
                    // name, version index and date — the person is indexed and dated too
                    let idx = p.as_ref().map(|p| format!("{}{}", self.t("version_n"), subscript(p.rev.max(1)))).unwrap_or_default();
                    ui.label(RichText::new(&name).color(c.ink).b().size(19.0));
                    let h = format!("@{}", cybercord_node::types::handle(&name, &peer));
                    if ui.add(egui::Label::new(RichText::new(&h).color(c.accent).monospace()).sense(egui::Sense::click())).on_hover_text(self.t("handle_click_copy")).clicked() {
                        ui.output_mut(|o| o.copied_text = h.clone());
                    }
                    if let Some(p) = &p {
                        ui.label(RichText::new(format!("{idx} · {}", short_date(p.ts_ms))).color(c.ink2).small());
                        let code = p.code();
                        if !code.is_empty() {
                            ui.label(RichText::new(format!("[{code}]")).color(c.accent).monospace());
                        }
                    }
                    ui.label(RichText::new(short(&peer)).color(c.ink2).small().monospace());
                });
            });
            let Some(p) = p else {
                ui.add_space(8.0);
                ui.colored_label(c.ink2, self.t("profile_none"));
                return;
            };
            if !p.job.is_empty() || !p.tasks.is_empty() {
                ui.add_space(8.0);
                if !p.job.is_empty() {
                    ui.label(RichText::new(format!("{}: {}", self.t("profile_job"), p.job)).color(c.ink));
                }
                if !p.tasks.is_empty() {
                    ui.add(egui::Label::new(RichText::new(format!("{}: {}", self.t("profile_tasks"), p.tasks)).color(c.ink)).wrap());
                }
            }
            if !p.skills.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(self.t("profile_code_h")).color(c.ink2).small().b());
                for k in p.top_skills() {
                    let s = crate::blocks::Scale { title: format!("{} · {}", prof::short_code(&k.id), prof::name(&k.id, ru)), value: k.level as f64, max: 10.0, note: k.note.clone() };
                    crate::blocks::draw_scale(ui, &s, c.ink, c.ink2, c.accent, c.line);
                    ui.add_space(4.0);
                }
            }
            if !p.interests.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(self.t("profile_interests")).color(c.ink2).small().b());
                ui.horizontal_wrapped(|ui| {
                    for i in &p.interests {
                        egui::Frame::none().fill(c.card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(8.0, 3.0)).show(ui, |ui| {
                            ui.label(RichText::new(prof::name(i, ru)).color(c.ink).small());
                        });
                    }
                });
            }
            if !p.activity.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(self.t("profile_activity")).color(c.ink2).small().b());
                ui.add(egui::Label::new(RichText::new(&p.activity).color(c.ink)).wrap());
            }
            if peer != self.self_id && ui.button(self.t("profile_message")).clicked() {
                self.befriend(&peer, &name, true);
                self.profile_view = None;
            }
        });
        if !open {
            self.profile_view = None;
        }
    }

    /// Settings → Profile: edit my profile; the code is built from the skills.
    fn tab_profile(&mut self, ui: &mut egui::Ui) {
        use base64::Engine;
        let c = th();
        let ru = matches!(self.lang, Lang::Ru | Lang::Uk);
        if self.profile_draft.is_none() {
            self.profile_draft = Some(self.profiles.get(&self.self_id).cloned().unwrap_or_default());
        }
        // a picked avatar arrives from the file dialog thread
        if let Some(rx) = &self.avatar_rx {
            if let Ok(r) = rx.try_recv() {
                self.avatar_rx = None;
                if let Some(path) = r {
                    if let Some(b64) = std::fs::read(&path).ok().and_then(|b| image::load_from_memory(&b).ok()).map(|img| {
                        let img = img.resize_to_fill(128, 128, image::imageops::FilterType::Lanczos3).to_rgb8();
                        let mut out = Vec::new();
                        let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img);
                        base64::engine::general_purpose::STANDARD_NO_PAD.encode(out)
                    }) {
                        if let Some(d) = self.profile_draft.as_mut() {
                            d.avatar = b64;
                            d.rev = d.rev.wrapping_add(1000); // new texture
                        }
                    }
                }
            }
        }
        let mut d = self.profile_draft.take().unwrap_or_default();
        d.peer = self.self_id.clone();
        d.name = self.self_name.clone();
        ui.horizontal(|ui| {
            match self.avatar_texture(ui.ctx(), &d) {
                Some(t) => {
                    ui.add(egui::Image::new(SizedTexture::new(t, egui::vec2(72.0, 72.0))).rounding(Rounding::same(36.0)));
                }
                None => avatar(ui, &self.self_name.chars().next().unwrap_or('?').to_uppercase().to_string(), 36.0, c.accent, None),
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(&self.self_name).color(c.ink).b().size(18.0));
                let code = d.code();
                ui.label(RichText::new(if code.is_empty() { self.t("profile_code_empty").to_string() } else { format!("[{code}]") }).color(c.accent).monospace());
                ui.horizontal(|ui| {
                    if ui.button(self.t("profile_avatar")).clicked() && self.avatar_rx.is_none() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.avatar_rx = Some(rx);
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(rfd::FileDialog::new().add_filter("images", &["png", "jpg", "jpeg", "webp", "bmp", "gif"]).pick_file());
                            ctx.request_repaint();
                        });
                    }
                    if !d.avatar.is_empty() && ui.button(self.t("profile_avatar_remove")).clicked() {
                        d.avatar.clear();
                    }
                });
            });
        });
        ui.add_space(12.0);
        egui::Grid::new("prof_grid").num_columns(2).spacing(egui::vec2(12.0, 8.0)).show(ui, |ui| {
            ui.label(self.t("profile_job"));
            ui.add(egui::TextEdit::singleline(&mut d.job).hint_text(self.t("profile_job_hint")).desired_width(420.0));
            ui.end_row();
            ui.label(self.t("profile_tasks"));
            ui.add(egui::TextEdit::multiline(&mut d.tasks).hint_text(self.t("profile_tasks_hint")).desired_rows(2).desired_width(420.0));
            ui.end_row();
            ui.label(self.t("profile_activity"));
            ui.add(egui::TextEdit::multiline(&mut d.activity).hint_text(self.t("profile_activity_hint")).desired_rows(2).desired_width(420.0));
            ui.end_row();
        });
        // skills: 0–10 with a comment
        ui.add_space(12.0);
        ui.label(RichText::new(self.t("profile_skills").to_uppercase()).color(c.ink2).size(12.0).b());
        let mut remove: Option<usize> = None;
        for (k, sk) in d.skills.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} · {}", prof::short_code(&sk.id), prof::name(&sk.id, ru))).color(c.ink).b());
                ui.add(egui::Slider::new(&mut sk.level, 0..=10));
                ui.add(egui::TextEdit::singleline(&mut sk.note).hint_text(self.t("profile_skill_note")).desired_width(200.0));
                if ui.small_button("×").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            d.skills.remove(k);
        }
        ui.horizontal(|ui| {
            let hint = self.t("profile_skill_find");
            ui.add(egui::TextEdit::singleline(&mut self.skill_query).hint_text(hint).desired_width(260.0));
            let q = self.skill_query.trim().to_lowercase();
            if !q.is_empty() && !prof::CATALOG.iter().any(|e| e.ru.to_lowercase() == q || e.en.to_lowercase() == q) && ui.button(self.t("profile_skill_own")).clicked() {
                d.skills.push(Skill { id: self.skill_query.trim().to_string(), level: 5, note: String::new() });
                self.skill_query.clear();
            }
        });
        let q = self.skill_query.trim().to_lowercase();
        // the catalog: 100 options in districts; click to add as a skill
        egui::ScrollArea::vertical().id_salt("catalog").max_height(220.0).show(ui, |ui| {
            for (g, gru, gen) in prof::GROUPS {
                let items: Vec<&prof::Entry> = prof::CATALOG.iter().filter(|e| e.group == *g && (q.is_empty() || e.ru.to_lowercase().contains(&q) || e.en.to_lowercase().contains(&q) || e.code.to_lowercase() == q)).collect();
                if items.is_empty() {
                    continue;
                }
                ui.label(RichText::new(if ru { *gru } else { *gen }).color(c.ink2).small().b());
                ui.horizontal_wrapped(|ui| {
                    for e in items {
                        let has_skill = d.skills.iter().any(|s| s.id == e.code);
                        let has_int = d.interests.iter().any(|s| s == e.code);
                        let label = format!("{} {}", e.code, if ru { e.ru } else { e.en });
                        let r = ui.add(egui::Button::new(RichText::new(label).small().color(if has_skill { c.on_accent } else { c.ink })).fill(if has_skill { c.accent } else if has_int { c.sel } else { c.card }).rounding(Rounding::same(10.0)));
                        if r.clicked() {
                            if has_skill {
                                d.skills.retain(|s| s.id != e.code);
                            } else {
                                d.skills.push(Skill { id: e.code.to_string(), level: 5, note: String::new() });
                            }
                        }
                        if r.secondary_clicked() {
                            if has_int {
                                d.interests.retain(|s| s != e.code);
                            } else {
                                d.interests.push(e.code.to_string());
                            }
                        }
                        r.on_hover_text(self.t("profile_chip_tip"));
                    }
                });
            }
        });
        ui.add_space(4.0);
        ui.colored_label(c.ink2, RichText::new(self.t("profile_catalog_note")).small());
        if !d.interests.is_empty() {
            ui.label(RichText::new(format!("{}: {}", self.t("profile_interests"), d.interests.iter().map(|i| prof::name(i, ru)).collect::<Vec<_>>().join(", "))).color(c.ink).small());
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new(self.t("profile_save")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(160.0, 34.0))).clicked() {
                let mut p = d.clone();
                p.rev = self.profiles.get(&self.self_id).map(|o| o.rev).unwrap_or(0);
                let _ = self.handle.cmd_tx.send(Cmd::SetProfile(p));
                let m = self.t("profile_saved").to_string();
                self.toast(m);
            }
            let label = self.t("profile_show_codes");
            if ui.checkbox(&mut self.settings.show_codes, label).changed() {
                store::save_settings(&self.settings);
            }
        });
        self.profile_draft = Some(d);
    }

    /// The scale / chart editor: fields on the left, live preview, insert or save.
    fn block_editor(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.block_ed.take() else { return };
        let c = th();
        let mut open = true;
        let mut done: Option<String> = None;
        let title = if ed.chart_mode { self.t("block_chart") } else { self.t("block_scale") };
        egui::Window::new(title).collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).order(Order::Foreground).default_width(520.0).show(ctx, |ui| {
            ui.set_width(520.0);
            if ed.target.is_none() {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut ed.chart_mode, false, self.t("block_scale"));
                    ui.selectable_value(&mut ed.chart_mode, true, self.t("block_chart"));
                });
                ui.separator();
            }
            if !ed.chart_mode {
                let s = &mut ed.scale;
                egui::Grid::new("scale_ed").num_columns(2).spacing(egui::vec2(10.0, 8.0)).show(ui, |ui| {
                    ui.label(self.t("block_title"));
                    ui.add(egui::TextEdit::singleline(&mut s.title).hint_text(self.t("block_scale_hint")).desired_width(340.0));
                    ui.end_row();
                    ui.label(self.t("block_value"));
                    let max = s.max.max(1.0);
                    ui.add(egui::Slider::new(&mut s.value, 0.0..=max).step_by(if max <= 20.0 { 0.5 } else { 1.0 }));
                    ui.end_row();
                    ui.label(self.t("block_max"));
                    ui.horizontal(|ui| {
                        for m in [5.0, 10.0, 100.0] {
                            if ui.selectable_label((s.max - m).abs() < 1e-9, crate::blocks::fmt(m)).clicked() {
                                s.value = s.value / s.max * m;
                                s.max = m;
                            }
                        }
                        ui.add(egui::DragValue::new(&mut s.max).range(1.0..=1000.0));
                    });
                    ui.end_row();
                    ui.label(self.t("block_note"));
                    ui.add(egui::TextEdit::singleline(&mut s.note).desired_width(340.0));
                    ui.end_row();
                });
                s.value = s.value.clamp(0.0, s.max);
                ui.separator();
                crate::blocks::draw_scale(ui, &ed.scale, c.ink, c.ink2, c.accent, c.line);
            } else {
                let ch = &mut ed.chart;
                ui.horizontal(|ui| {
                    ui.label(self.t("block_title"));
                    ui.add(egui::TextEdit::singleline(&mut ch.title).desired_width(260.0));
                    ui.selectable_value(&mut ch.kind, crate::blocks::ChartKind::Bar, self.t("block_bar"));
                    ui.selectable_value(&mut ch.kind, crate::blocks::ChartKind::Line, self.t("block_line"));
                });
                ui.add_space(4.0);
                let mut remove: Option<usize> = None;
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for (k, (label, v)) in ch.rows.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(label).desired_width(200.0));
                            ui.add(egui::DragValue::new(v).speed(0.1));
                            if ui.small_button("×").clicked() {
                                remove = Some(k);
                            }
                        });
                    }
                });
                if let Some(k) = remove {
                    ch.rows.remove(k);
                }
                if ui.button(self.t("block_add_row")).clicked() {
                    let n = ch.rows.len() + 1;
                    ch.rows.push((format!("{n}"), 0.0));
                }
                ui.separator();
                crate::blocks::draw_chart(ui, &ed.chart, c.ink, c.ink2, c.accent, c.line);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let label = if ed.target.is_some() { self.t("block_save") } else { self.t("block_insert") };
                if ui.add(egui::Button::new(RichText::new(label).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                    done = Some(if ed.chart_mode { ed.chart.to_text() } else { ed.scale.to_text() });
                }
            });
        });
        match done {
            Some(text) => match ed.target.clone() {
                None => {
                    if !self.input.is_empty() && !self.input.ends_with('\n') {
                        self.input.push('\n');
                    }
                    self.input.push_str(&text);
                    self.focus_input = 3;
                }
                Some((id, start, len)) => {
                    let ch = self.active_channel();
                    if let Some(v) = self.views.get(&ch) {
                        if let Some(m) = v.msgs.iter().find(|m| m.id == id) {
                            let r = cybercord_node::history::resolve(m, &v.edits);
                            let mut lines: Vec<String> = r.body.lines().map(|l| l.to_string()).collect();
                            if start + len <= lines.len() {
                                lines.splice(start..start + len, text.lines().map(|l| l.to_string()));
                                let _ = self.handle.cmd_tx.send(Cmd::EditMessage { channel: ch, target: id, body: Some(lines.join("\n")), rev: r.rev + 1 });
                            }
                        }
                    }
                }
            },
            None if open => self.block_ed = Some(ed),
            None => {}
        }
    }

    /// Formula mode: snippets and a live preview of the draft.
    fn math_preview(&mut self, ui: &mut egui::Ui) {
        let c = th();
        egui::Frame::none().fill(c.surface).stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(10.0, 6.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if self.math_mode {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(self.t("math_snippets")).color(c.ink2).small());
                    for (label, snip) in [("a/b", "\\frac{a}{b}"), ("√x", "\\sqrt{x}"), ("xⁿ", "x^{n}"), ("xᵢ", "x_{i}"), ("∑", "\\sum_{i=1}^{n} "), ("∫", "\\int_{a}^{b} "), ("lim", "\\lim_{x \\to 0} "), ("α", "\\alpha "), ("π", "\\pi "), ("∞", "\\infty "), ("≤", "\\le "), ("≠", "\\neq "), ("→", "\\to "), ("( )", "\\left( x \\right)")] {
                        if ui.small_button(label).on_hover_text(snip).clicked() {
                            // insert before the closing $$ if there is one
                            match self.input.rfind("$$") {
                                Some(k) if self.input[..k].contains("$$") => self.input.insert_str(k, snip),
                                _ => self.input.push_str(snip),
                            }
                            self.focus_input = 3;
                        }
                    }
                });
            }
            if crate::math::has_math(&self.input) {
                ui.label(RichText::new(self.t("math_preview")).color(c.ink2).small());
                let draft = self.input.clone();
                egui::ScrollArea::vertical().id_salt("math_prev").max_height(140.0).show(ui, |ui| {
                    let _ = self.body_view(ui, "preview", &draft, false);
                });
            } else if self.math_mode {
                ui.label(RichText::new(self.t("math_help")).color(c.ink2).small());
            }
        });
    }

    /// Live hints under the composer: what in the draft may start a conflict.
    fn hint_strip(&mut self, ui: &mut egui::Ui, hints: &[sem::Finding]) {
        let c = th();
        ui.horizontal(|ui| {
            ui.set_min_height(26.0);
            let shown: Vec<&sem::Finding> = {
                let mut v: Vec<&sem::Finding> = Vec::new();
                for f in hints {
                    if !v.iter().any(|x| x.kind == f.kind) {
                        v.push(f);
                    }
                }
                v.into_iter().take(2).collect()
            };
            for f in shown {
                let col = if f.kind == Kind::Dogma { Color32::from_rgb(0x91, 0x41, 0xac) } else { Color32::from_rgb(0xe5, 0xa5, 0x0a) };
                let word = if f.text.is_empty() { String::new() } else { format!("«{}» ", f.text) };
                let label = format!("⚠ {word}{}", self.td(&format!("sem_{}_h", f.kind.key())));
                egui::Frame::none().fill(col.gamma_multiply(0.14)).rounding(Rounding::same(6.0)).inner_margin(egui::Margin::symmetric(8.0, 3.0)).show(ui, |ui| {
                    ui.label(RichText::new(label).color(c.ink).small());
                });
            }
        });
    }

    /// Banner over a conversation that seems to be heating up, or that shows
    /// dogmatic wordings. Nothing is blocked: it is a reminder to check facts.
    fn conversation_banner(&mut self, ui: &mut egui::Ui, ch: &ChannelId) {
        if !self.settings.words_hints {
            return;
        }
        if self.banner_hidden.get(ch).map(|t| t.elapsed() < Duration::from_secs(1800)).unwrap_or(false) {
            return;
        }
        let Some(view) = self.views.get(ch) else { return };
        let since = now_ms().saturating_sub(15 * 60 * 1000);
        let recent: Vec<(String, String)> = view
            .msgs
            .iter()
            .rev()
            .take(10)
            .filter(|m| m.ts_ms >= since)
            .map(|m| {
                let r = cybercord_node::history::resolve(m, &view.edits);
                (if m.from.is_empty() { m.name.clone() } else { m.from.clone() }, if r.deleted { String::new() } else { r.body })
            })
            .collect();
        let dogma: Option<(String, String, u64)> = view.msgs.iter().rev().take(30).find_map(|m| {
            let r = cybercord_node::history::resolve(m, &view.edits);
            (!r.deleted && sem::scan(&r.body).iter().any(|f| f.kind == Kind::Dogma)).then(|| (m.name.clone(), r.body, r.ts_ms))
        });
        let (hot, authors) = sem::heat(recent.iter().map(|(a, b)| (a.as_str(), b.as_str())));
        let heated = hot >= 3 && authors >= 2;
        if !heated && dogma.is_none() {
            return;
        }
        let c = th();
        let (col, text) = if heated { (Color32::from_rgb(0xe5, 0xa5, 0x0a), self.t("banner_heat")) } else { (Color32::from_rgb(0x91, 0x41, 0xac), self.t("banner_dogma")) };
        let mut hide = false;
        egui::Frame::none().fill(col.gamma_multiply(0.16)).stroke(Stroke::new(1.0, col.gamma_multiply(0.6))).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(10.0, 6.0)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(text).color(c.ink));
                if heated {
                    if ui.small_button(self.t("tpl_facts")).clicked() {
                        self.input.push_str(&templates(self.lang)[5].1);
                        self.focus_input = 3;
                    }
                    if ui.small_button(self.t("tpl_six")).clicked() {
                        self.input.push_str(&templates(self.lang)[3].1);
                        self.focus_input = 3;
                    }
                }
                if let Some(d) = &dogma {
                    if ui.small_button(self.t("sem_levels")).clicked() {
                        self.analyze = Some(d.clone());
                    }
                }
                if ui.small_button(self.t("banner_hide")).clicked() {
                    hide = true;
                }
            });
        });
        ui.add_space(4.0);
        if hide {
            self.banner_hidden.insert(ch.clone(), Instant::now());
        }
    }

    /// Message text with a small Markdown subset: tables, check lists, lists,
    /// **bold** lines. Returns a new body if the author ticked a checkbox.
    fn body_view(&mut self, ui: &mut egui::Ui, id: &str, body: &str, editable: bool) -> Option<String> {
        let c = th();
        let lines: Vec<&str> = body.lines().collect();
        let mut changed: Option<String> = None;
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i];
            let t = l.trim_start();
            if t.starts_with('|') {
                let start = i;
                while i < lines.len() && lines[i].trim_start().starts_with('|') {
                    i += 1;
                }
                let rows: Vec<Vec<String>> = lines[start..i]
                    .iter()
                    .filter(|r| !(r.contains('-') && r.chars().all(|ch| matches!(ch, '|' | '-' | ':' | ' '))))
                    .map(|r| r.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect())
                    .collect();
                egui::Frame::none().stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(6.0)).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                    egui::Grid::new((id, "tbl", start)).striped(true).spacing(egui::vec2(16.0, 4.0)).show(ui, |ui| {
                        for (ri, row) in rows.iter().enumerate() {
                            for cell in row {
                                let rt = RichText::new(cell.trim_matches('*')).color(c.ink);
                                ui.label(if ri == 0 { rt.b() } else { rt });
                            }
                            ui.end_row();
                        }
                    });
                });
                continue;
            }
            if t.starts_with("[chart") {
                if let Some((chart, used)) = crate::blocks::Chart::parse(&lines[i..]) {
                    let open = egui::Frame::none().stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                        crate::blocks::draw_chart(ui, &chart, c.ink, c.ink2, c.accent, c.line);
                        editable && ui.small_button(self.t("block_edit")).clicked()
                    }).inner;
                    if open {
                        self.block_ed = Some(BlockEd { chart_mode: true, chart, target: Some((id.to_string(), i, used)), ..BlockEd::new_chart() });
                    }
                    i += used;
                    continue;
                }
            }
            if let Some(scale) = crate::blocks::Scale::parse(t) {
                let open = egui::Frame::none().stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                    crate::blocks::draw_scale(ui, &scale, c.ink, c.ink2, c.accent, c.line);
                    editable && ui.small_button(self.t("block_edit")).clicked()
                }).inner;
                if open {
                    self.block_ed = Some(BlockEd { chart_mode: false, scale, target: Some((id.to_string(), i, 1)), ..BlockEd::new_scale() });
                }
                i += 1;
                continue;
            }
            if crate::math::has_math(l) {
                let pieces = crate::math::split(l);
                if let [crate::math::Piece::Block(f)] = pieces.as_slice() {
                    ui.add_space(4.0);
                    ui.vertical_centered(|ui| crate::math::show(ui, f, 19.0, c.ink));
                    ui.add_space(4.0);
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for p in pieces {
                            match p {
                                crate::math::Piece::Text(tx) => {
                                    ui.label(RichText::new(tx.replace("\\$", "$")).color(c.ink).size(14.0));
                                }
                                crate::math::Piece::Inline(f) | crate::math::Piece::Block(f) => {
                                    crate::math::show(ui, f, 15.0, c.ink);
                                }
                            }
                        }
                    });
                }
                i += 1;
                continue;
            }
            let check = t.strip_prefix("- [ ]").map(|r| (false, r)).or_else(|| t.strip_prefix("- [x]").or_else(|| t.strip_prefix("- [X]")).map(|r| (true, r)));
            if let Some((done, rest)) = check {
                let mut v = done;
                ui.horizontal(|ui| {
                    let r = ui.add_enabled(editable, egui::Checkbox::without_text(&mut v));
                    let text = RichText::new(rest.trim()).color(if done { c.ink2 } else { c.ink });
                    ui.label(if done { text.strikethrough() } else { text });
                    if r.changed() {
                        let mut new: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
                        let indent = &l[..l.len() - t.len()];
                        new[i] = format!("{indent}- [{}]{rest}", if v { "x" } else { " " });
                        changed = Some(new.join("\n"));
                    }
                });
            } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("•").color(c.accent));
                    self.rich_text(ui, rest, c.ink, 14.0);
                });
            } else if let Some(at) = self.anims.get(id).map(|a| a.elapsed().as_secs_f32()).filter(|t| *t < crate::fx::TOTAL && !t.is_nan()) {
                // "hacked text": noise resolves into the message, then a short glitch
                let bold = t.len() > 4 && t.starts_with("**") && t.ends_with("**");
                let font = if bold { egui::FontId::new(14.0, egui::FontFamily::Name("bold".into())) } else { egui::FontId::proportional(14.0) };
                crate::fx::label(ui, if bold { t.trim_matches('*') } else { l }, at, crate::fx::seed_of(id) ^ i as u64, font, c.ink, c.accent);
                ui.ctx().request_repaint();
            } else if t.len() > 4 && t.starts_with("**") && t.ends_with("**") {
                ui.label(RichText::new(t.trim_matches('*')).color(c.ink).b());
            } else if t.is_empty() {
                ui.add_space(4.0);
            } else {
                self.rich_text(ui, l, c.ink, 14.0);
            }
            i += 1;
        }
        changed
    }

    /// "Levels of abstraction" for one message (structural differential).
    fn analyze_overlay(&mut self, ctx: &egui::Context) {
        let Some((name, body, ts)) = self.analyze.clone() else { return };
        let c = th();
        let screen = ctx.screen_rect();
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Area::new(Id::new("an_dim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let r = ui.allocate_rect(screen, egui::Sense::click());
            ui.painter().rect_filled(screen, Rounding::ZERO, Color32::from_black_alpha(150));
            if r.clicked() {
                close = true;
            }
        });
        let w = (screen.width() - 48.0).clamp(320.0, 680.0);
        let h = (screen.height() - 56.0).min(720.0);
        let lines = sem::levels(&body);
        egui::Area::new(Id::new("an_win")).order(Order::Tooltip).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Frame::none().fill(c.window).rounding(Rounding::same(14.0)).stroke(Stroke::new(1.0, c.line)).shadow(egui::epaint::Shadow { offset: egui::vec2(0.0, 8.0), blur: 28.0, spread: 0.0, color: Color32::from_black_alpha(90) }).show(ui, |ui| {
                ui.set_width(w);
                ui.set_height(h);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.set_height(48.0);
                    ui.label(RichText::new(self.t("sem_levels")).color(c.ink).size(16.0).b());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        if ui.add(egui::Button::new(RichText::new(self.t("close")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(96.0, 30.0))).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add(egui::Separator::default().spacing(0.0));
                egui::ScrollArea::vertical().id_salt("an_scroll").auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::none().inner_margin(egui::Margin { left: 24.0, right: 24.0, top: 16.0, bottom: 20.0 }).show(ui, |ui| {
                        ui.set_width(w - 48.0);
                        let tex = self.icons.get(ui.ctx(), "doc_sd", DOC_SD);
                        ui.vertical_centered(|ui| {
                            ui.add(egui::Image::new(SizedTexture::new(tex, egui::vec2(w.min(560.0) - 48.0, (w.min(560.0) - 48.0) * 260.0 / 560.0))).rounding(Rounding::same(12.0)));
                        });
                        ui.add_space(8.0);
                        ui.add(egui::Label::new(RichText::new(self.t("sem_intro")).color(c.ink2)).wrap());
                        ui.add_space(10.0);
                        ui.label(RichText::new(format!("{name} · {}", short_date(ts))).color(c.ink2).small());
                        ui.add_space(4.0);
                        for line in &lines {
                            let (col, key) = match line.level {
                                Level::Description => (c.green, "lvl_desc"),
                                Level::Inference => (Color32::from_rgb(0xe6, 0x61, 0x00), "lvl_infer"),
                                Level::Evaluation => (c.red, "lvl_eval"),
                                Level::Unclear => (c.ink2, "lvl_unclear"),
                            };
                            egui::Frame::none().fill(col.gamma_multiply(0.10)).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(10.0, 6.0)).show(ui, |ui| {
                                ui.set_width(w - 70.0);
                                ui.horizontal(|ui| {
                                    egui::Frame::none().fill(col).rounding(Rounding::same(5.0)).inner_margin(egui::Margin::symmetric(6.0, 1.0)).show(ui, |ui| {
                                        ui.label(RichText::new(self.t(key)).color(Color32::WHITE).small().b());
                                    });
                                });
                                ui.add(egui::Label::new(RichText::new(&line.text).color(c.ink)).wrap());
                                for f in &line.findings {
                                    let word = if f.text.is_empty() { String::new() } else { format!("«{}» — ", f.text) };
                                    ui.add(egui::Label::new(RichText::new(format!("{}{}: {}", word, self.td(&format!("sem_{}_t", f.kind.key())), self.td(&format!("sem_{}_h", f.kind.key())))).color(c.ink2).small()).wrap());
                                }
                            });
                            ui.add_space(6.0);
                        }
                        ui.add_space(8.0);
                        ui.add(egui::Label::new(RichText::new(self.t("sem_disclaimer")).color(c.ink2).small().italics()).wrap());
                    });
                });
            });
        });
        if close {
            self.analyze = None;
        }
    }

    // ------------------------------------------------------------- media in chat

    fn media_widget(&mut self, ui: &mut egui::Ui, a: &Attachment, ch: &ChannelId) {
        if a.kind == MediaKind::Audio && store::has_blob(&a.hash) {
            self.music_widget(ui, a);
            return;
        }
        if a.kind == MediaKind::File {
            self.file_widget(ui, a, ch);
            return;
        }
        let time = ui.input(|i| i.time);
        enum Draw {
            Still(egui::TextureId, Vec2),
            Frame(egui::TextureId, Vec2, u32),
            Circle(Vec<egui::TextureId>, u32, Option<(u32, Vec<i16>)>),
            Waiting,
            Failed,
        }
        let draw = match self.media.get(&a.hash, a.kind) {
            Ok(Some(Shown::Still { tex, size })) => Draw::Still(tex.id(), *size),
            Ok(Some(Shown::Anim { frames, total_ms, size })) => {
                let mut t = ((time * 1000.0) as u64 % *total_ms as u64) as u32;
                let mut pick = (frames[0].0.id(), frames[0].1);
                for (tex, ms) in frames.iter() {
                    if t < *ms {
                        pick = (tex.id(), *ms);
                        break;
                    }
                    t -= ms;
                }
                Draw::Frame(pick.0, *size, pick.1)
            }
            Ok(Some(Shown::Circle { frames, fps, audio })) => Draw::Circle(frames.iter().map(|f| f.id()).collect(), *fps, audio.clone()),
            Ok(None) => Draw::Waiting,
            Err(()) => Draw::Failed,
        };
        let fit = |s: Vec2, max: Vec2| -> Vec2 {
            let k = (max.x / s.x).min(max.y / s.y).min(1.0);
            s * k
        };
        match draw {
            Draw::Still(tex, size) | Draw::Frame(tex, size, _) => {
                let sz = fit(size, egui::vec2(320.0, 260.0));
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(SizedTexture::new(tex, sz)).rounding(Rounding::same(10.0)));
                    if a.kind == MediaKind::Gif {
                        let saved = self.settings.gifs.contains(&a.hash);
                        let label = if saved { format!("★ {}", self.t("saved_gif")) } else { "☆".to_string() };
                        if ui.add(egui::Button::new(RichText::new(label).color(if saved { th().accent } else { th().ink2 })).frame(false)).on_hover_text(self.t("save_gif")).clicked() && !saved {
                            self.settings.gifs.insert(0, a.hash.clone());
                            store::save_settings(&self.settings);
                        }
                    }
                });
                if let Draw::Frame(_, _, ms) = draw {
                    ui.ctx().request_repaint_after(Duration::from_millis(ms.min(100) as u64));
                }
            }
            Draw::Circle(frames, fps, audio) => self.circle_widget(ui, a, &frames, fps, audio, time),
            Draw::Waiting => {
                if self.fetch_asked.insert(a.hash.clone()) {
                    let _ = self.handle.cmd_tx.send(Cmd::FetchBlob { channel: ch.clone(), hash: a.hash.clone() });
                }
                let text = match self.progress.get(&a.hash) {
                    Some((got, total)) => self.t("loading_media").replace("{p}", &((*got * 100) / (*total).max(1)).to_string()),
                    None => self.t("media_wait").to_string(),
                };
                let size = match a.kind {
                    MediaKind::Circle => egui::vec2(200.0, 200.0),
                    MediaKind::Audio => egui::vec2(400.0, 64.0),
                    _ => egui::vec2(220.0, 120.0),
                };
                let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                if a.kind == MediaKind::Circle {
                    ui.painter().circle_filled(r.center(), 100.0, th().card);
                } else {
                    ui.painter().rect_filled(r, Rounding::same(10.0), th().card);
                }
                ui.painter().text(r.center(), Align2::CENTER_CENTER, text, egui::FontId::proportional(13.0), th().ink2);
            }
            Draw::Failed => {
                ui.colored_label(th().ink2, format!("⚠ {}", a.name));
            }
        }
    }

    fn circle_widget(&mut self, ui: &mut egui::Ui, a: &Attachment, frames: &[egui::TextureId], fps: u32, audio: Option<(u32, Vec<i16>)>, time: f64) {
        let n = frames.len().max(1);
        let dur = n as f64 / fps.max(1) as f64;
        let playing = self.circle_sound.as_ref().filter(|(h, _, _)| *h == a.hash).map(|(_, t, _)| t.elapsed().as_secs_f64());
        let pos = match playing {
            Some(p) if p < dur => p,
            Some(_) => {
                self.circle_sound = None; // finished
                time % dur
            }
            None => time % dur, // muted autoplay loop, like Telegram
        };
        let idx = ((pos * fps as f64) as usize).min(n - 1);
        let (r, resp) = ui.allocate_exact_size(egui::vec2(208.0, 208.0), egui::Sense::click());
        let c = r.center();
        let p = ui.painter();
        p.circle_filled(c, 103.0, th().card);
        media::paint_circle(p, c, 98.0, frames[idx], false);
        media::progress_ring(p, c, 101.0, (pos / dur) as f32, if playing.is_some() { th().accent } else { th().on_accent.gamma_multiply(0.85) }, 3.0);
        let secs = (dur - pos).max(0.0) as u32;
        // say plainly whether there is sound and how to hear it (autoplay is muted)
        let hint = if playing.is_some() {
            "  🔊".to_string()
        } else if audio.is_some() {
            format!(" · {}", self.t("circle_sound_tap"))
        } else {
            format!(" · {}", self.t("circle_no_sound"))
        };
        let label = format!("{}:{:02}{hint}", secs / 60, secs % 60);
        let lw = ui.fonts(|f| f.layout_no_wrap(label.clone(), egui::FontId::proportional(12.0), Color32::WHITE).size().x) + 16.0;
        let lr = Rect::from_center_size(c + egui::vec2(0.0, 76.0), egui::vec2(lw.max(64.0), 20.0));
        p.rect_filled(lr, Rounding::same(10.0), Color32::from_black_alpha(110));
        p.text(lr.center(), Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), th().on_accent);
        let resp = resp.on_hover_text(self.t("circle_tap"));
        if resp.clicked() {
            if playing.is_some() {
                self.circle_sound = None;
            } else {
                let player = audio.and_then(|(rate, pcm)| voice_rec::play(rate, pcm));
                self.circle_sound = Some((a.hash.clone(), Instant::now(), player));
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1000 / fps.max(1) as u64));
    }

    // ------------------------------------------------------------- popups

    fn popups(&mut self, ctx: &egui::Context) {
        if self.popup == Popup::None {
            return;
        }
        let anchor = self.anchors.get("media_btns").copied().unwrap_or(Rect::from_min_size(Pos2::new(300.0, 500.0), Vec2::ZERO));
        let size = egui::vec2(360.0, 330.0);
        let pos = egui::pos2(anchor.min.x, anchor.min.y - size.y - 10.0);
        let k = ctx.animate_bool_with_time(Id::new(("popup", self.popup as u8)), true, 0.18);
        let area = egui::Area::new(Id::new(("popup_area", self.popup as u8)))
            .order(Order::Foreground)
            .fixed_pos(pos + egui::vec2(0.0, (1.0 - k) * 12.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(th().window)
                    .stroke(Stroke::new(1.0, th().line))
                    .rounding(Rounding::same(14.0))
                    .shadow(egui::epaint::Shadow { offset: egui::vec2(0.0, 6.0), blur: 22.0, spread: 0.0, color: Color32::from_black_alpha(50) })
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.set_width(size.x - 24.0);
                        ui.set_height(size.y - 24.0);
                        match self.popup {
                            Popup::Emoji => self.emoji_picker(ui),
                            Popup::Gifs => self.gif_panel(ui),
                            Popup::None => {}
                        }
                    });
            });
        self.popup_rect = area.response.rect;
        // Close when clicking elsewhere (but not on the buttons that toggle it).
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().map(|p| !self.popup_rect.contains(p) && !anchor.contains(p)).unwrap_or(false));
        if clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.popup = Popup::None;
        }
    }

    fn emoji_picker(&mut self, ui: &mut egui::Ui) {
        let hint = self.t("search");
        ui.add(egui::TextEdit::singleline(&mut self.emoji_query).hint_text(hint).desired_width(f32::INFINITY));
        ui.add_space(6.0);
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            for (ti, cat) in emoji::CATEGORIES.iter().enumerate() {
                let tex = self.emoji.tex(&ctx, emoji::cat_icon(cat));
                let sel = ti == self.emoji_tab && self.emoji_query.is_empty();
                let key: &'static str = match *cat {
                    "sce" => "cat_sce",
                    "smileys" => "cat_smileys",
                    "people" => "cat_people",
                    "hearts" => "cat_hearts",
                    "nature" => "cat_nature",
                    "food" => "cat_food",
                    "activity" => "cat_activity",
                    _ => "cat_symbols",
                };
                let b = ui.add(egui::ImageButton::new(SizedTexture::new(tex, egui::vec2(20.0, 20.0))).selected(sel)).on_hover_text(self.t(key));
                if b.clicked() {
                    self.emoji_tab = ti;
                    self.emoji_query.clear();
                }
            }
        });
        ui.add_space(4.0);
        let q = self.emoji_query.trim().to_lowercase();
        let cat = emoji::CATEGORIES[self.emoji_tab];
        let items: Vec<usize> = (0..emoji::count()).filter(|i| if q.is_empty() { emoji::cat(*i) == cat } else { emoji::code(*i).contains(&q) }).collect();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("emoji_grid").spacing(egui::vec2(4.0, 4.0)).show(ui, |ui| {
                for (n, i) in items.iter().enumerate() {
                    let tex = self.emoji.tex(&ctx, *i);
                    let b = ui.add(egui::ImageButton::new(SizedTexture::new(tex, egui::vec2(28.0, 28.0))).frame(false)).on_hover_text(match emoji::about(*i) {
                        Some(d) => format!(":{}: — {d}", emoji::code(*i)),
                        None => format!(":{}:", emoji::code(*i)),
                    });
                    if b.clicked() {
                        if !self.input.is_empty() && !self.input.ends_with(' ') {
                            self.input.push(' ');
                        }
                        self.input.push_str(&format!(":{}: ", emoji::code(*i)));
                    }
                    if (n + 1) % 9 == 0 {
                        ui.end_row();
                    }
                }
            });
        });
    }

    fn gif_panel(&mut self, ui: &mut egui::Ui) {
        // built-in stickers on general-semantics themes (stored like any received GIF)
        if self.builtin_gifs.is_empty() {
            self.builtin_gifs = BUILTIN_GIFS.iter().map(|b| store::save_blob(b)).collect();
        }
        ui.label(RichText::new(self.t("sce_gifs")).color(th().ink).b());
        ui.add_space(4.0);
        let time = ui.input(|i| i.time);
        let builtin = self.builtin_gifs.clone();
        ui.horizontal_wrapped(|ui| {
            for h in builtin {
                if let Ok(Some(Shown::Anim { frames, total_ms, .. })) = self.media.get(&h, MediaKind::Gif) {
                    let mut t = ((time * 1000.0) as u64 % *total_ms as u64) as u32;
                    let mut id = frames[0].0.id();
                    for (tex, ms) in frames.iter() {
                        if t < *ms {
                            id = tex.id();
                            break;
                        }
                        t -= ms;
                    }
                    if ui.add(egui::ImageButton::new(SizedTexture::new(id, egui::vec2(54.0, 54.0))).frame(false).rounding(Rounding::same(8.0))).clicked() {
                        if let Some(bytes) = store::load_blob(&h) {
                            self.send_file("sce.gif".into(), bytes);
                        }
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new(self.t("my_gifs")).color(th().ink).b());
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
            if self.settings.gifs.is_empty() {
                ui.colored_label(th().ink2, self.t("no_gifs"));
            }
            let gifs = self.settings.gifs.clone();
            ui.horizontal_wrapped(|ui| {
                for h in gifs {
                    let pick = match self.media.get(&h, MediaKind::Gif) {
                        Ok(Some(Shown::Anim { frames, total_ms, .. })) => {
                            let mut t = ((time * 1000.0) as u64 % *total_ms as u64) as u32;
                            let mut id = frames[0].0.id();
                            for (tex, ms) in frames.iter() {
                                if t < *ms {
                                    id = tex.id();
                                    break;
                                }
                                t -= ms;
                            }
                            Some(id)
                        }
                        _ => None,
                    };
                    if let Some(id) = pick {
                        if ui.add(egui::ImageButton::new(SizedTexture::new(id, egui::vec2(96.0, 96.0))).rounding(Rounding::same(8.0))).clicked() {
                            if let Some(bytes) = store::load_blob(&h) {
                                self.send_file("gif.gif".into(), bytes);
                            }
                        }
                    }
                }
            });
            ui.ctx().request_repaint_after(Duration::from_millis(80));
        });
        ui.add_space(6.0);
        sep(ui);
        ui.label(RichText::new(self.t("send_file")).color(th().ink).b());
        ui.horizontal(|ui| {
            let hint = self.t("path_hint");
            ui.add(egui::TextEdit::singleline(&mut self.file_path).hint_text(hint).desired_width(ui.available_width() - 90.0));
            if ui.add(egui::Button::new(RichText::new(self.t("send")).color(th().on_accent)).fill(th().accent).rounding(Rounding::same(8.0))).clicked() {
                let path = expand_home(self.file_path.trim());
                match std::fs::metadata(&path) {
                    Ok(_) => {
                        self.send_path(std::path::PathBuf::from(&path));
                        self.file_path.clear();
                    }
                    Err(e) => self.toast(format!("{path}: {e}")),
                }
            }
        });
        ui.colored_label(th().ink2, RichText::new(self.t("drop_hint")).small());
        ui.add_space(4.0);
        ui.colored_label(th().ink2, RichText::new(self.t("gif_note")).small().italics());
    }

    // ------------------------------------------------------------- video circles

    fn start_recording(&mut self) {
        self.popup = Popup::None;
        if !camera::supported() {
            self.rec_error = Some(format!("{}\n{}", self.t("rec_not_built"), self.t("rec_permission")));
            return;
        }
        self.rec = Some(camera::Recorder::start());
        self.rec_tex = None;
        if voice::AUDIO_ENABLED && !self.rec.as_ref().map(|r| r.has_sound()).unwrap_or(true) {
            let m = self.t("circle_mic_off").to_string();
            self.toast(m);
        }
    }

    fn recorder_overlay(&mut self, ctx: &egui::Context) {
        if self.rec.is_none() && self.rec_error.is_none() {
            return;
        }
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("rec_dim")));
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(140));
        let mut finish = false;
        let mut cancel = false;
        egui::Area::new(Id::new("rec_card")).order(Order::Tooltip).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Frame::none().fill(th().window).rounding(Rounding::same(20.0)).inner_margin(egui::Margin::same(22.0)).show(ui, |ui| {
                ui.set_width(340.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(self.t("rec_title")).color(th().ink).b().size(18.0));
                    ui.add_space(10.0);
                    if let Some(err) = self.rec_error.clone().or_else(|| self.rec.as_ref().and_then(|r| r.error())) {
                        let text = if err == "no-camera-build" {
                            format!("{}\n{}", self.t("rec_not_built"), self.t("rec_permission"))
                        } else if err.contains('\n') {
                            err
                        } else {
                            format!("{}\n{}", self.t("rec_no_camera").replace("{s}", &err), self.t("rec_permission"))
                        };
                        ui.colored_label(th().red, text);
                        ui.add_space(12.0);
                        if ui.add(egui::Button::new(RichText::new(self.t("close")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0))).clicked() {
                            cancel = true;
                        }
                        return;
                    }
                    let Some(rec) = &self.rec else { return };
                    let (r, _) = ui.allocate_exact_size(egui::vec2(270.0, 270.0), egui::Sense::hover());
                    let c = r.center();
                    let p = ui.painter();
                    p.circle_filled(c, 132.0, th().card);
                    if let Some(img) = rec.latest() {
                        match &mut self.rec_tex {
                            Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                            None => self.rec_tex = Some(ui.ctx().load_texture("rec_preview", img, egui::TextureOptions::LINEAR)),
                        }
                    }
                    if let Some(t) = &self.rec_tex {
                        media::paint_circle(p, c, 126.0, t.id(), false);
                    } else {
                        p.text(c, Align2::CENTER_CENTER, self.t("rec_starting"), egui::FontId::proportional(14.0), th().ink2);
                    }
                    let frac = rec.elapsed() / camera::MAX_SECS;
                    media::progress_ring(p, c, 132.0, frac, th().red, 4.0);
                    ui.add_space(8.0);
                    let blink = (ui.input(|i| i.time) * 2.0).sin() > 0.0;
                    ui.horizontal(|ui| {
                        ui.add_space(70.0);
                        dot(ui, if blink { th().red } else { th().window });
                        ui.colored_label(th().ink, self.t("rec_time").replace("{s}", &(rec.elapsed() as u32).to_string()));
                    });
                    ui.colored_label(th().ink2, RichText::new(if rec.has_sound() { self.t("rec_sound_on") } else { self.t("rec_sound_off") }).small());
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(30.0);
                        if ui.add(egui::Button::new(RichText::new(self.t("rec_cancel")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0)).min_size(egui::vec2(110.0, 34.0))).clicked() {
                            cancel = true;
                        }
                        if ui.add(egui::Button::new(RichText::new(self.t("rec_stop")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(150.0, 34.0))).clicked() {
                            finish = true;
                        }
                    });
                    if rec.auto_finished() {
                        finish = true;
                    }
                });
            });
        });
        if cancel {
            if let Some(r) = self.rec.take() {
                r.cancel();
            }
            self.rec_error = None;
            self.rec_tex = None;
        } else if finish {
            if let Some(r) = self.rec.take() {
                if let Some(clip) = r.finish() {
                    let dur_ms = clip.dur_ms();
                    let bytes = clip.encode();
                    let channel = self.active_channel();
                    let _ = self.handle.cmd_tx.send(Cmd::SendMedia {
                        channel,
                        body: String::new(),
                        kind: MediaKind::Circle,
                        name: "circle".into(),
                        bytes,
                        w: camera::SIZE,
                        h: camera::SIZE,
                        dur_ms,
                    });
                }
            }
            self.rec_tex = None;
        }
        ctx.request_repaint_after(Duration::from_millis(40));
    }

    // ------------------------------------------------------------- settings + toast

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = self.toast.clone() else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 4.0 {
            self.toast = None;
            return;
        }
        let alpha = ease_out((age / 0.25).min(1.0)) * (1.0 - ((age - 3.5) / 0.5).clamp(0.0, 1.0));
        let screen = ctx.screen_rect();
        egui::Area::new(Id::new("toast"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_BOTTOM, [0.0, -46.0 - (1.0 - alpha) * 10.0])
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(th().ink.gamma_multiply(0.92 * alpha))
                    .rounding(Rounding::same(18.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 9.0))
                    .show(ui, |ui| {
                        ui.set_max_width(screen.width() * 0.6);
                        ui.label(RichText::new(msg).color(th().window.gamma_multiply(alpha)));
                    });
            });
        ctx.request_repaint();
    }
}

fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            return format!("{home}/{rest}");
        }
    }
    p.to_string()
}

fn sep(ui: &mut egui::Ui) {
    let r = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(r, 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, th().line));
    ui.add_space(6.0);
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
}

/// A rounded card; returns its screen rectangle (for tour anchors).
fn card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) -> Rect {
    egui::Frame::none()
        .fill(th().surface)
        .stroke(Stroke::new(1.0, th().line))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(680.0));
            ui.label(RichText::new(title).color(th().ink).b().size(17.0));
            ui.add_space(8.0);
            body(ui);
        })
        .response
        .rect
}


fn bullet(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.colored_label(th().accent, "•");
        ui.vertical(|ui| {
            ui.colored_label(th().ink2, text);
        });
    });
    ui.add_space(4.0);
}

fn member_row(ui: &mut egui::Ui, name: &str, role: Option<&str>, connected: bool, talking: bool, voice_icon: Option<egui::TextureId>) -> egui::Response {
    let r = ui.horizontal(|ui| {
        let initial = name.chars().next().unwrap_or('?').to_uppercase().to_string();
        let col = if connected { name_color(name) } else { th().line };
        avatar(ui, &initial, 13.0, col, if talking { Some(th().green) } else { None });
        ui.label(RichText::new(name).color(if connected { th().ink } else { th().ink2 }));
        if let Some(tex) = voice_icon {
            ui.add(egui::Image::new(SizedTexture::new(tex, egui::vec2(13.0, 13.0))).tint(if talking { th().green } else { th().ink2 }));
        }
        if let Some(r) = role {
            ui.label(RichText::new(r).color(th().accent).small().b());
        }
    });
    // over the labels (which take clicks for text selection), so the row opens the profile
    ui.interact(r.response.rect, egui::Id::new(("member_row", name)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---- icons (GNOME-style tiles + symbolic icons, pre-rendered from SVG) ----
const APP_ICON: &[u8] = include_bytes!("../assets/icon-128.png");
const APP_ICON_256: &[u8] = include_bytes!("../assets/icon-256.png");
const FRIENDS_TILE: &[u8] = include_bytes!("../assets/servers/friends.png");
const SYM_SPEAKER: &[u8] = include_bytes!("../assets/symbolic/speaker.png");
const SYM_HASH: &[u8] = include_bytes!("../assets/symbolic/hash.png");
const SYM_MIC: &[u8] = include_bytes!("../assets/symbolic/mic.png");
const SYM_MIC_OFF: &[u8] = include_bytes!("../assets/symbolic/mic-off.png");
const SYM_HANGUP: &[u8] = include_bytes!("../assets/symbolic/hangup.png");
const SYM_PENCIL: &[u8] = include_bytes!("../assets/symbolic/pencil.png");
const SYM_TRASH: &[u8] = include_bytes!("../assets/symbolic/trash.png");
const BUILTIN_GIFS: [&[u8]; 5] = [
    include_bytes!("../assets/gifs/map-territory.gif"),
    include_bytes!("../assets/gifs/index.gif"),
    include_bytes!("../assets/gifs/pause.gif"),
    include_bytes!("../assets/gifs/check-facts.gif"),
    include_bytes!("../assets/gifs/bridge.gif"),
];
const SYM_IMAGE: &[u8] = include_bytes!("../assets/symbolic/image.png");
const SYM_SIGMA: &[u8] = include_bytes!("../assets/symbolic/sigma.png");
const CITY_TILE: &[u8] = include_bytes!("../assets/emoji/sce/sce_city.png");
const PAPERS_TILE: &[u8] = include_bytes!("../assets/emoji/sce/sce_science.png");
const SUBS_TILE: &[u8] = include_bytes!("../assets/emoji/sce/sce_nodes.png");
const SYM_LEVELS: &[u8] = include_bytes!("../assets/symbolic/levels.png");
const DOC_SD: &[u8] = include_bytes!("../assets/docs/structural-differential.png");

/// Today's local date (day, month, year).
fn chrono_today() -> (u32, u32, i32) {
    use chrono::Datelike;
    let t = chrono::Local::now();
    (t.day(), t.month(), t.year())
}

/// Ready-made blocks for the composer (problem solving, facts check …).
fn templates(lang: Lang) -> Vec<(&'static str, String)> {
    let ru = matches!(lang, Lang::Ru | Lang::Uk);
    let (d, m, y) = chrono_today();
    let today = format!("{d:02}.{m:02}.{y}");
    if ru {
        vec![
            ("tpl_check", "- [ ] \n- [ ] \n- [ ] ".to_string()),
            ("tpl_table", "| Что | Кто | Когда |\n|---|---|---|\n|  |  |  |".to_string()),
            ("tpl_proscons", "| Решение | Плюсы | Минусы |\n|---|---|---|\n| 1. |  |  |\n| 2. |  |  |".to_string()),
            ("tpl_six", format!("**Решение проблем — шесть шагов ({today})**\n1. Одна проблема — что именно, где, когда, с кем:\n2. Все возможные решения (хотя бы 5):\n- \n- \n3. Плюсы и минусы:\n| Решение | Плюсы | Минусы |\n|---|---|---|\n|  |  |  |\n4. Выбранное решение:\n5. План действий:\n- [ ] Шаг 1 — когда:\n- [ ] Шаг 2 — когда:\n6. Итоги: что получилось, что нет, что дальше:")),
            ("tpl_smart", "**Цель (SMART)**\n- Конкретная: \n- Измеримая: \n- Достижимая: \n- Значимая: \n- Срок: ".to_string()),
            ("tpl_facts", format!("**Сверка фактов ({today})**\n- Что я наблюдал (где, когда): \n- Что я предположил: \n- Как это проверить: \n- Что подтвердилось: ")),
        ]
    } else {
        vec![
            ("tpl_check", "- [ ] \n- [ ] \n- [ ] ".to_string()),
            ("tpl_table", "| What | Who | When |\n|---|---|---|\n|  |  |  |".to_string()),
            ("tpl_proscons", "| Option | Pros | Cons |\n|---|---|---|\n| 1. |  |  |\n| 2. |  |  |".to_string()),
            ("tpl_six", format!("**Problem solving — six steps ({today})**\n1. One problem — what exactly, where, when, with whom:\n2. All possible solutions (at least 5):\n- \n- \n3. Pros and cons:\n| Option | Pros | Cons |\n|---|---|---|\n|  |  |  |\n4. Chosen solution:\n5. Action plan:\n- [ ] Step 1 — when:\n- [ ] Step 2 — when:\n6. Review: what worked, what didn't, what next:")),
            ("tpl_smart", "**Goal (SMART)**\n- Specific: \n- Measurable: \n- Achievable: \n- Relevant: \n- Time-bound: ".to_string()),
            ("tpl_facts", format!("**Fact check ({today})**\n- What I observed (where, when): \n- What I assumed: \n- How to check it: \n- What was confirmed: ")),
        ]
    }
}
const SYM_GEAR: &[u8] = include_bytes!("../assets/symbolic/gear.png");
const SYM_HEADPHONES: &[u8] = include_bytes!("../assets/symbolic/headphones.png");
/// Wordless step-by-step pictures (docs/img/*.svg); captions come from i18n.
const GUIDE: &[(&str, &[u8], &str, &str)] = &[
    ("g1", include_bytes!("../assets/guide/guide-1-network.png"), "g1_h", "g1_b"),
    ("g2", include_bytes!("../assets/guide/guide-2-code.png"), "g2_h", "g2_b"),
    ("g3", include_bytes!("../assets/guide/guide-3-friend.png"), "g3_h", "g3_b"),
    ("g4", include_bytes!("../assets/guide/guide-4-chat.png"), "g4_h", "g4_b"),
    ("g5", include_bytes!("../assets/guide/guide-5-circle.png"), "g5_h", "g5_b"),
    ("g6", include_bytes!("../assets/guide/guide-6-voice.png"), "g6_h", "g6_b"),
];

#[derive(Default)]
struct Icons {
    map: HashMap<&'static str, egui::TextureHandle>,
}

impl Icons {
    fn get(&mut self, ctx: &egui::Context, key: &'static str, png: &'static [u8]) -> egui::TextureId {
        self.map
            .entry(key)
            .or_insert_with(|| {
                let img = image::load_from_memory(png).map(|i| i.to_rgba8()).unwrap_or_else(|_| image::RgbaImage::new(1, 1));
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                ctx.load_texture(key, ci, egui::TextureOptions::LINEAR)
            })
            .id()
    }
}

/// Real bold (Inter SemiBold) instead of egui's colour-only "strong".
trait Bold {
    fn b(self) -> RichText;
}
impl Bold for RichText {
    fn b(self) -> RichText {
        self.family(egui::FontFamily::Name("bold".into()))
    }
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).color(th().ink2).size(11.5).b());
    ui.add_space(2.0);
}

/// Server/home tile in the rail: rounded picture, Discord-like pill on the left when selected.
fn rail_tile(ui: &mut egui::Ui, tex: egui::TextureId, selected: bool, badge: Option<usize>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::click());
    let hovered = resp.hovered();
    let k = ui.ctx().animate_bool_with_time(resp.id, selected || hovered, 0.15);
    let p = ui.painter();
    if selected {
        p.rect_filled(Rect::from_center_size(r.center() + egui::vec2(-1.0, 0.0), r.size() + egui::vec2(8.0, 8.0)), Rounding::same(16.0), th().window);
    }
    let grow = 2.0 * k;
    p.image(tex, r.expand(grow), Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
    let pill_h = if selected { 34.0 } else if hovered { 16.0 } else { 0.0 };
    if pill_h > 0.0 {
        let x = r.min.x - 10.0;
        p.rect_filled(Rect::from_min_max(egui::pos2(x, r.center().y - pill_h / 2.0), egui::pos2(x + 4.0, r.center().y + pill_h / 2.0)), Rounding::same(2.0), th().ink);
    }
    if let Some(n) = badge {
        let c = r.right_bottom() - egui::vec2(4.0, 4.0);
        p.circle_filled(c, 9.0, th().rail);
        p.circle_filled(c, 7.5, th().green);
        p.text(c, Align2::CENTER_CENTER, n.min(99).to_string(), egui::FontId::new(10.0, egui::FontFamily::Name("bold".into())), th().on_accent);
    }
    resp
}

/// A rounded "+" drawn with two strokes (crisp at any scale, no font glyph).
fn plus_glyph(p: &egui::Painter, c: Pos2, half: f32, col: Color32) {
    let w = (half * 0.32).max(2.0);
    p.line_segment([c - egui::vec2(half, 0.0), c + egui::vec2(half, 0.0)], Stroke::new(w, col));
    p.line_segment([c - egui::vec2(0.0, half), c + egui::vec2(0.0, half)], Stroke::new(w, col));
}

/// One channel line with a symbolic icon (# or speaker).
fn channel_row(ui: &mut egui::Ui, icon: egui::TextureId, name: &str, selected: bool, voice: bool) -> egui::Response {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(r, Rounding::same(8.0), if voice { th().green.gamma_multiply(0.18) } else { th().sel });
    } else if resp.hovered() {
        p.rect_filled(r, Rounding::same(8.0), th().rail);
    }
    let col = if selected { th().ink } else { th().ink2 };
    let icon_col = if selected && voice { th().green_text } else { col };
    p.image(icon, Rect::from_center_size(egui::pos2(r.min.x + 16.0, r.center().y), egui::vec2(15.0, 15.0)), Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), icon_col);
    let font = if selected { egui::FontId::new(14.0, egui::FontFamily::Name("bold".into())) } else { egui::FontId::proportional(14.0) };
    p.text(egui::pos2(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, name, font, col);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Round initial avatar with an optional "talking" ring.
fn avatar(ui: &mut egui::Ui, initial: &str, r: f32, color: Color32, ring: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(r * 2.0 + 6.0, r * 2.0 + 6.0), egui::Sense::hover());
    let c = rect.center();
    if let Some(rc) = ring {
        ui.painter().circle_stroke(c, r + 2.5, Stroke::new(2.0, rc));
    }
    ui.painter().circle_filled(c, r, color);
    ui.painter().text(c, Align2::CENTER_CENTER, initial, egui::FontId::new(r * 0.95, egui::FontFamily::Name("bold".into())), th().on_accent);
}

/// Stable colour per name, from the GNOME palette.
fn name_color(name: &str) -> Color32 {
    const P: [Color32; 8] = [
        Color32::from_rgb(0x1C, 0x71, 0xD8),
        Color32::from_rgb(0x26, 0xA2, 0x69),
        Color32::from_rgb(0x91, 0x41, 0xAC),
        Color32::from_rgb(0xE6, 0x61, 0x00),
        Color32::from_rgb(0xC0, 0x1C, 0x28),
        Color32::from_rgb(0x21, 0x90, 0xA4),
        Color32::from_rgb(0x86, 0x5E, 0x3C),
        Color32::from_rgb(0x5D, 0x73, 0x85),
    ];
    let h = name.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    P[(h % 8) as usize]
}

fn icon_button(ui: &mut egui::Ui, tex: egui::TextureId, fg: Color32, bg: Color32, size: f32) -> egui::Response {
    icon_button_k(ui, tex, fg, bg, size, 0.52)
}

/// `icon_button` with the glyph at `k` × the button size.
fn icon_button_k(ui: &mut egui::Ui, tex: egui::TextureId, fg: Color32, bg: Color32, size: f32, k: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let bg = if resp.hovered() { bg.gamma_multiply(0.85) } else { bg };
    ui.painter().rect_filled(r, Rounding::same(size * 0.3), bg);
    let s = size * k;
    ui.painter().image(tex, Rect::from_center_size(r.center(), egui::vec2(s, s)), Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), fg);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn install_theme(ctx: &egui::Context) {
    let c = th();
    let mut v = if c.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = c.window;
    v.window_fill = c.window;
    v.window_stroke = Stroke::new(1.0, c.line);
    v.extreme_bg_color = c.card; // text inputs
    v.faint_bg_color = c.surface;
    v.code_bg_color = c.card;
    v.override_text_color = Some(c.ink);
    v.widgets.noninteractive.bg_fill = c.window;
    v.widgets.noninteractive.weak_bg_fill = c.window;
    v.widgets.noninteractive.bg_stroke.color = c.line;
    v.widgets.noninteractive.fg_stroke.color = c.ink;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.fg_stroke.color = c.ink;
        w.bg_stroke = Stroke::new(1.0, c.line);
    }
    v.widgets.inactive.bg_fill = c.card;
    v.widgets.inactive.weak_bg_fill = c.card;
    v.widgets.hovered.bg_fill = c.sel;
    v.widgets.hovered.weak_bg_fill = c.sel;
    v.widgets.active.bg_fill = c.accent.gamma_multiply(0.8);
    v.widgets.active.weak_bg_fill = c.sel;
    v.widgets.open.bg_fill = c.card;
    v.widgets.open.weak_bg_fill = c.card;
    v.selection.bg_fill = c.accent.gamma_multiply(if c.dark { 0.55 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0, c.accent);
    v.hyperlink_color = c.accent;
    v.popup_shadow.color = Color32::from_black_alpha(if c.dark { 120 } else { 40 });
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    use egui::{FontFamily as F, FontId, TextStyle as T};
    let body = if c.mono { F::Monospace } else { F::Proportional };
    style.text_styles = [
        (T::Heading, FontId::new(22.0, F::Name("bold".into()))),
        (T::Body, FontId::new(if c.mono { 13.5 } else { 14.5 }, body.clone())),
        (T::Button, FontId::new(if c.mono { 13.0 } else { 14.0 }, body)),
        (T::Small, FontId::new(12.0, F::Proportional)),
        (T::Monospace, FontId::new(13.0, F::Monospace)),
    ]
    .into();
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.slider_width = 260.0;
    ctx.set_style(style);
}

/// egui's fonts + a system CJK font as fallback, so Chinese/Japanese text
/// (UI or messages) renders. For a Chinese UI the Simplified face is preferred.
/// Inter (the face GNOME's Adwaita Sans is built on, OFL-1.1) embedded for UI
/// text, Inter SemiBold as the "bold" family, egui's fonts for symbols, and a
/// system CJK font as fallback.
fn install_fonts(ctx: &egui::Context, lang: Lang) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("inter".into(), egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.otf")));
    fonts.font_data.insert("inter-semibold".into(), egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.otf")));
    let base: Vec<String> = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "inter".into());
    let mut bold = vec!["inter-semibold".to_string()];
    bold.extend(base);
    fonts.families.insert(egui::FontFamily::Name("bold".into()), bold);
    // brand lettering: "SocioCyberneering Inc." is always Montserrat
    fonts.font_data.insert("montserrat".into(), egui::FontData::from_static(include_bytes!("../assets/fonts/Montserrat-Regular.ttf")));
    fonts.families.insert(egui::FontFamily::Name("brand".into()), vec!["montserrat".into(), "inter".into()]);
    let candidates = [
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\meiryo.ttc",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fd = egui::FontData::from_owned(bytes);
            // NotoSansCJK.ttc faces: 0 JP, 1 KR, 2 SC, 3 TC, 4 HK.
            if path.contains("NotoSansCJK") && lang == Lang::Zh {
                fd.index = 2;
            }
            fonts.font_data.insert("cjk".into(), fd);
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace, egui::FontFamily::Name("bold".into())] {
                fonts.families.entry(fam).or_default().push("cjk".into());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn load_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).ok()
}

pub fn run_gui(cfg: NetConfig, minimized: bool) -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([900.0, 560.0])
        .with_title("CyberCord — SocioCyberneering Inc.")
        .with_app_id("cybercord")
        .with_visible(!minimized)
        .with_drag_and_drop(true);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let native_options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("CyberCord", native_options, Box::new(move |cc| Ok(Box::new(CyberApp::new(cc, cfg).started_hidden(minimized)))))
}

// ---- music messages ----

enum MusicMsg {
    /// Waveform + length + tags, for drawing the message (samples are dropped).
    Info { hash: String, peaks: Vec<f32>, dur_ms: u32 },
    /// Fully decoded track, ready to play from `start` (0..1).
    Track { hash: String, track: Arc<music::Track>, start: f32 },
    Failed { hash: String },
    /// A file the user is sending: (label, length) worked out off the UI thread.
    Send { channel: ChannelId, name: String, bytes: Vec<u8>, dur_ms: u32 },
    SendFailed,
}

enum MusicInfo {
    Loading,
    Ready { peaks: Vec<f32>, dur_ms: u32 },
    Failed,
}

struct Music {
    info: HashMap<String, MusicInfo>,
    tx: std::sync::mpsc::Sender<MusicMsg>,
    rx: std::sync::mpsc::Receiver<MusicMsg>,
    /// The track playing now (one at a time).
    player: Option<(String, music::Player)>,
    /// A track being decoded for playback.
    preparing: Option<String>,
}

impl Default for Music {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self { info: HashMap::new(), tx, rx, player: None, preparing: None }
    }
}

const BARS: usize = 56;

impl Music {
    fn want_info(&mut self, hash: &str) {
        if self.info.contains_key(hash) {
            return;
        }
        self.info.insert(hash.to_string(), MusicInfo::Loading);
        let (tx, h) = (self.tx.clone(), hash.to_string());
        std::thread::spawn(move || {
            let msg = match store::load_blob(&h).ok_or_else(|| "missing".to_string()).and_then(|b| music::decode(&b)) {
                Ok(t) => MusicMsg::Info { peaks: t.peaks(BARS), dur_ms: t.dur_ms(), hash: h },
                Err(_) => MusicMsg::Failed { hash: h },
            };
            let _ = tx.send(msg);
        });
    }

    fn prepare(&mut self, hash: &str, start: f32) {
        self.preparing = Some(hash.to_string());
        let (tx, h) = (self.tx.clone(), hash.to_string());
        std::thread::spawn(move || {
            let msg = match store::load_blob(&h).ok_or_else(|| "missing".to_string()).and_then(|b| music::decode(&b)) {
                Ok(t) => MusicMsg::Track { hash: h, track: Arc::new(t), start },
                Err(_) => MusicMsg::Failed { hash: h },
            };
            let _ = tx.send(msg);
        });
    }

    /// State of `hash` if it is the current track: (playing, fraction, position ms).
    fn now(&self, hash: &str) -> Option<(bool, f32, u32)> {
        self.player.as_ref().filter(|(h, _)| h == hash).map(|(_, p)| (p.state.is_playing(), p.state.fraction(), p.state.position_ms()))
    }
}

impl CyberApp {
    fn poll_music(&mut self) {
        while let Ok(msg) = self.music.rx.try_recv() {
            match msg {
                MusicMsg::Info { hash, peaks, dur_ms } => {
                    self.music.info.insert(hash, MusicInfo::Ready { peaks, dur_ms });
                }
                MusicMsg::Track { hash, track, start } => {
                    if self.music.preparing.as_deref() != Some(hash.as_str()) {
                        continue; // the user picked another track meanwhile
                    }
                    self.music.preparing = None;
                    self.music.player = None; // stop the previous one first
                    match music::play(track) {
                        Some(p) => {
                            p.state.seek(start);
                            self.music.player = Some((hash, p));
                        }
                        None => {
                            let m = self.t("music_bad").to_string();
                            self.toast(m);
                        }
                    }
                }
                MusicMsg::Failed { hash } => {
                    if self.music.preparing.as_deref() == Some(hash.as_str()) {
                        self.music.preparing = None;
                    }
                    self.music.info.insert(hash, MusicInfo::Failed);
                }
                MusicMsg::Send { channel, name, bytes, dur_ms } => {
                    let _ = self.handle.cmd_tx.send(Cmd::SendMedia { channel, body: String::new(), kind: MediaKind::Audio, name, bytes, w: 0, h: 0, dur_ms });
                }
                MusicMsg::SendFailed => {
                    let m = self.t("music_bad").to_string();
                    self.toast(m);
                }
            }
        }
    }

    /// Play / pause / seek. Without a sound build, hand the file to the system player.
    fn music_action(&mut self, a: &Attachment, seek: Option<f32>) {
        if !music::CAN_PLAY {
            if let Some(bytes) = store::load_blob(&a.hash) {
                match music::open_externally(&a.hash, &bytes, &a.name) {
                    Ok(_) => {
                        let m = self.t("music_external").to_string();
                        self.toast(m);
                    }
                    Err(e) => self.toast(e),
                }
            }
            return;
        }
        if let Some((h, p)) = &self.music.player {
            if *h == a.hash {
                match seek {
                    Some(f) => {
                        p.state.seek(f);
                        p.state.set_playing(true);
                    }
                    None => p.state.set_playing(!p.state.is_playing()),
                }
                return;
            }
        }
        // another track (or none): pause the old one and decode this one
        if let Some((_, p)) = &self.music.player {
            p.state.set_playing(false);
        }
        self.music.prepare(&a.hash, seek.unwrap_or(0.0));
    }

    fn music_widget(&mut self, ui: &mut egui::Ui, a: &Attachment) {
        self.music.want_info(&a.hash);
        let (peaks, dur_ms, failed) = match self.music.info.get(&a.hash) {
            Some(MusicInfo::Ready { peaks, dur_ms }) => (Some(peaks.clone()), *dur_ms, false),
            Some(MusicInfo::Failed) => (None, a.dur_ms, true),
            _ => (None, a.dur_ms, false),
        };
        let now = self.music.now(&a.hash);
        let preparing = self.music.preparing.as_deref() == Some(a.hash.as_str());
        let playing = now.map(|n| n.0).unwrap_or(false);
        let frac = now.map(|n| n.1).unwrap_or(0.0);
        let time = ui.input(|i| i.time) as f32;
        let mut click_play = false;
        let mut seek: Option<f32> = None;
        let mut open = false;

        egui::Frame::none().fill(th().surface).stroke(Stroke::new(1.0, th().line)).rounding(Rounding::same(14.0)).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
            ui.set_width(400.0);
            ui.horizontal(|ui| {
                // round play / pause button
                let (r, resp) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::click());
                let p = ui.painter();
                let bg = if failed { th().line } else if resp.hovered() { th().accent_hover } else { th().accent };
                p.circle_filled(r.center(), 23.0, bg);
                let c = r.center();
                if preparing {
                    let a0 = time * 5.0;
                    let pts: Vec<Pos2> = (0..20).map(|i| {
                        let t = a0 + i as f32 * 0.22;
                        c + egui::vec2(t.cos(), t.sin()) * 11.0
                    }).collect();
                    p.add(egui::Shape::line(pts, Stroke::new(3.0, th().on_accent)));
                    ui.ctx().request_repaint();
                } else if playing {
                    p.rect_filled(Rect::from_center_size(c - egui::vec2(5.0, 0.0), egui::vec2(5.0, 16.0)), Rounding::same(1.5), th().on_accent);
                    p.rect_filled(Rect::from_center_size(c + egui::vec2(5.0, 0.0), egui::vec2(5.0, 16.0)), Rounding::same(1.5), th().on_accent);
                } else {
                    p.add(egui::Shape::convex_polygon(
                        vec![c + egui::vec2(-5.0, -9.0), c + egui::vec2(10.0, 0.0), c + egui::vec2(-5.0, 9.0)],
                        th().on_accent,
                        Stroke::NONE,
                    ));
                }
                let tip = if playing { self.t("music_pause") } else if music::CAN_PLAY { self.t("music_play") } else { self.t("music_open") };
                if resp.on_hover_text(tip).clicked() && !failed {
                    click_play = true;
                }
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    let (title, sub) = if a.name == voice_rec::VOICE_MSG_NAME {
                        (self.t("voice_msg").to_string(), format!("{} KB", a.size / 1024))
                    } else {
                        match a.name.split_once(" — ") {
                            Some((artist, t)) => (t.to_string(), artist.to_string()),
                            None => (a.name.clone(), format!("♪ {} KB", a.size / 1024)),
                        }
                    };
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(RichText::new(title).color(th().ink).b()).truncate());
                    });
                    ui.colored_label(th().ink2, RichText::new(sub).small());
                    // waveform
                    let (wr, wresp) = ui.allocate_exact_size(egui::vec2(276.0, 28.0), egui::Sense::click());
                    let n = BARS;
                    let bw = wr.width() / n as f32;
                    for i in 0..n {
                        let v = peaks.as_ref().map(|p| p[i]).unwrap_or(0.12 + 0.1 * ((i as f32 * 0.7).sin().abs()));
                        let h = (v * wr.height()).max(3.0);
                        let x = wr.min.x + i as f32 * bw + bw * 0.5;
                        let played = (i as f32 + 0.5) / n as f32 <= frac && now.is_some();
                        let col = if failed { th().line } else if played { th().accent } else { th().peri };
                        ui.painter().line_segment([egui::pos2(x, wr.center().y - h / 2.0), egui::pos2(x, wr.center().y + h / 2.0)], Stroke::new((bw - 1.6).max(1.5), col));
                    }
                    if wresp.hovered() && !failed {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if wresp.on_hover_text(self.t("music_seek")).clicked() && !failed {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            seek = Some(((pos.x - wr.min.x) / wr.width()).clamp(0.0, 1.0));
                        }
                    }
                    let pos_ms = now.map(|n| n.2).unwrap_or(0);
                    let fmt = |ms: u32| format!("{}:{:02}", ms / 60_000, ms / 1000 % 60);
                    let label = if failed {
                        self.t("music_bad").to_string()
                    } else if preparing {
                        self.t("music_loading").to_string()
                    } else if now.is_some() {
                        format!("{} / {}", fmt(pos_ms), fmt(dur_ms))
                    } else {
                        fmt(dur_ms)
                    };
                    ui.colored_label(th().ink2, RichText::new(label).small());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if ui.add(egui::Button::new(RichText::new("↗").color(th().ink2).size(15.0)).fill(th().card).rounding(Rounding::same(8.0))).on_hover_text(self.t("music_open")).clicked() {
                        open = true;
                    }
                });
            });
        });
        if playing {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if self.autoplay && peaks.is_some() {
            self.autoplay = false;
            seek = Some(0.35);
        }
        if click_play {
            self.music_action(a, None);
        } else if let Some(f) = seek {
            self.music_action(a, Some(f));
        } else if open {
            if let Some(bytes) = store::load_blob(&a.hash) {
                if let Err(e) = music::open_externally(&a.hash, &bytes, &a.name) {
                    self.toast(e);
                }
            }
        }
    }
}

// ---- voice messages + Settings → Sound ----

impl CyberApp {
    fn start_voice_message(&mut self) {
        self.popup = Popup::None;
        if !voice::AUDIO_ENABLED {
            let m = self.t("no_sound_build").to_string();
            self.toast(m);
            return;
        }
        self.music.player = None; // don't record the music
        match voice_rec::MicRecorder::start() {
            Some(rec) => self.vm = Some((rec, Instant::now(), Vec::new())),
            None => {
                let m = self.t("vm_no_mic").to_string();
                self.toast(m);
            }
        }
    }

    /// The composer while a voice message is being recorded.
    fn voice_message_bar(&mut self, ui: &mut egui::Ui, ch: &ChannelId) {
        let Some((rec, started, levels)) = &mut self.vm else { return };
        let secs = started.elapsed().as_secs_f32();
        if self.vm_last_level.elapsed() > Duration::from_millis(70) {
            self.vm_last_level = Instant::now();
            levels.push(rec.level());
            if levels.len() > 400 {
                levels.remove(0);
            }
        }
        let levels = levels.clone();
        let time = ui.input(|i| i.time) as f32;
        let mut send = ui.input(|i| i.key_pressed(egui::Key::Enter)) || secs >= voice_rec::VOICE_MSG_MAX_SECS as f32;
        let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

        let pulse = 0.5 + 0.5 * (time * 4.0).sin();
        let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 32.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 5.0 + 2.0 * pulse, th().red.gamma_multiply(0.6 + 0.4 * pulse));
        let s = secs as u32;
        ui.label(RichText::new(format!("{}:{:02}", s / 60, s % 60)).color(th().ink).b().size(15.0));
        ui.add_space(6.0);
        // live waveform of the last few seconds
        let w = (ui.available_width() - 240.0).max(80.0);
        let (wr, _) = ui.allocate_exact_size(egui::vec2(w, 32.0), egui::Sense::hover());
        let bars = (w / 4.0) as usize;
        let start = levels.len().saturating_sub(bars);
        for (i, v) in levels[start..].iter().enumerate() {
            let h = (v.sqrt() * 30.0).max(2.0);
            let x = wr.min.x + i as f32 * 4.0 + 1.5;
            ui.painter().line_segment([egui::pos2(x, wr.center().y - h / 2.0), egui::pos2(x, wr.center().y + h / 2.0)], Stroke::new(2.5, th().accent));
        }
        if levels.len() < 3 {
            ui.painter().text(wr.left_center(), Align2::LEFT_CENTER, self.t("vm_recording"), egui::FontId::proportional(13.0), th().ink2);
        }
        if ui.add(egui::Button::new(RichText::new(self.t("vm_cancel")).color(th().ink)).fill(th().card).rounding(Rounding::same(8.0)).min_size(egui::vec2(90.0, 32.0))).clicked() {
            cancel = true;
        }
        if ui.add(egui::Button::new(RichText::new(self.t("vm_send")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(110.0, 32.0))).clicked() {
            send = true;
        }
        ui.ctx().request_repaint_after(Duration::from_millis(50));

        if cancel {
            self.vm = None;
        } else if send {
            if let Some((rec, _, _)) = self.vm.take() {
                match voice_rec::finish_voice_message(rec) {
                    Some((bytes, dur_ms)) => {
                        let _ = self.handle.cmd_tx.send(Cmd::SendMedia {
                            channel: ch.clone(),
                            body: String::new(),
                            kind: MediaKind::Audio,
                            name: voice_rec::VOICE_MSG_NAME.into(),
                            bytes,
                            w: 0,
                            h: 0,
                            dur_ms,
                        });
                    }
                    None => {
                        let m = self.t("vm_short").to_string();
                        self.toast(m);
                    }
                }
            }
        }
    }
}

// ---- full-screen settings (Discord-like) ----

#[derive(Clone, Copy, PartialEq)]
enum SetTab {
    Profile,
    Appearance,
    Voice,
    Language,
    Help,
    About,
    Privacy,
    Background,
}

impl CyberApp {
    fn settings_screen(&mut self, ctx: &egui::Context) {
        let c = th();
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_settings();
            return;
        }
        let k = ease_out(self.set_opened.elapsed().as_secs_f32() / 0.25);
        if k < 1.0 {
            ctx.request_repaint();
        }
        let tabs: [(SetTab, &'static str); 8] = [
            (SetTab::Profile, "set_profile"),
            (SetTab::Appearance, "set_appearance"),
            (SetTab::Voice, "set_voice"),
            (SetTab::Language, "language"),
            (SetTab::Privacy, "set_privacy"),
            (SetTab::Background, "set_background"),
            (SetTab::Help, "set_help"),
            (SetTab::About, "about"),
        ];
        // left: section list, right-aligned like Discord
        egui::SidePanel::left("set_nav")
            .exact_width((ctx.screen_rect().width() * 0.28).clamp(220.0, 360.0))
            .resizable(false)
            .frame(egui::Frame::none().fill(c.surface).inner_margin(egui::Margin { left: 16.0, right: 12.0, top: 48.0, bottom: 16.0 }))
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    ui.set_max_width(210.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.label(RichText::new(self.t("settings_title").to_uppercase()).color(c.ink2).size(11.5).b());
                        ui.add_space(6.0);
                        for (tab, key) in tabs {
                            let sel = self.set_tab == tab;
                            let w = ui.available_width();
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 34.0), egui::Sense::click());
                            if sel {
                                ui.painter().rect_filled(r, Rounding::same(6.0), c.sel);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(r, Rounding::same(6.0), c.card);
                            }
                            let font = if sel { egui::FontId::new(14.5, egui::FontFamily::Name("bold".into())) } else { egui::FontId::proportional(14.5) };
                            ui.painter().text(egui::pos2(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, self.t(key), font, if sel { c.ink } else { c.ink2 });
                            if resp.clicked() && !sel {
                                self.set_tab = tab;
                                self.set_opened = Instant::now();
                                self.monitor = None;
                            }
                            ui.add_space(2.0);
                        }
                        ui.add_space(10.0);
                        sep(ui);
                        ui.horizontal(|ui| {
                            let initial = self.self_name.chars().next().unwrap_or('?').to_uppercase().to_string();
                            avatar(ui, &initial, 15.0, c.accent, None);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&self.self_name).color(c.ink).b());
                                ui.colored_label(c.ink2, RichText::new(short(&self.self_id)).small());
                            });
                        });
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(c.window).inner_margin(egui::Margin { left: 40.0, right: 24.0, top: 40.0, bottom: 16.0 }))
            .show(ctx, |ui| {
                // close button (top right)
                let close_r = Rect::from_min_size(egui::pos2(ui.max_rect().max.x - 48.0, ui.max_rect().min.y - 8.0), egui::vec2(40.0, 56.0));
                let resp = ui.interact(close_r, Id::new("set_close"), egui::Sense::click());
                let cc = close_r.center_top() + egui::vec2(0.0, 18.0);
                ui.painter().circle_stroke(cc, 17.0, Stroke::new(2.0, if resp.hovered() { c.ink } else { c.ink2 }));
                let x = 5.5;
                for (a, b) in [(egui::vec2(-x, -x), egui::vec2(x, x)), (egui::vec2(-x, x), egui::vec2(x, -x))] {
                    ui.painter().line_segment([cc + a, cc + b], Stroke::new(2.0, if resp.hovered() { c.ink } else { c.ink2 }));
                }
                ui.painter().text(cc + egui::vec2(0.0, 30.0), Align2::CENTER_CENTER, self.t("close_esc"), egui::FontId::new(11.0, egui::FontFamily::Name("bold".into())), c.ink2);
                if resp.clicked() {
                    self.close_settings();
                    return;
                }
                let title = tabs.iter().find(|(t, _)| *t == self.set_tab).map(|(_, k)| *k).unwrap_or("settings_title");
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_space((1.0 - k) * 16.0);
                    ui.set_max_width((ui.available_width() - 56.0).min(760.0));
                    ui.label(RichText::new(self.t(title)).color(c.ink).b().size(22.0));
                    ui.add_space(18.0);
                    match self.set_tab {
                        SetTab::Appearance => self.tab_appearance(ui),
                        SetTab::Voice => self.tab_voice(ui),
                        SetTab::Language => self.tab_language(ui),
                        SetTab::Help => self.tab_help(ui),
                        SetTab::About => self.tab_about(ui),
                        SetTab::Background => self.tab_background(ui),
                        SetTab::Profile => self.tab_profile(ui),
                        SetTab::Privacy => {
                            let label = self.t("listed_opt");
                            if ui.checkbox(&mut self.settings.listed, label).changed() {
                                store::save_settings(&self.settings);
                                let _ = self.handle.cmd_tx.send(Cmd::SetListed(self.settings.listed));
                            }
                            ui.add_space(6.0);
                            ui.colored_label(th().ink2, self.t("listed_note"));
                            ui.add_space(18.0);
                            let label = self.t("global_opt");
                            if ui.checkbox(&mut self.settings.global, label).changed() {
                                store::save_settings(&self.settings);
                            }
                            ui.add_space(6.0);
                            ui.colored_label(th().ink2, self.t("global_note"));
                            ui.colored_label(th().ink2, RichText::new(self.t("restart_note")).small().italics());
                            ui.add_space(18.0);
                            let label = self.t("keeper_opt");
                            if ui.checkbox(&mut self.settings.relay_mail, label).changed() {
                                store::save_settings(&self.settings);
                                let _ = self.handle.cmd_tx.send(Cmd::SetKeeper(self.settings.relay_mail));
                            }
                            ui.add_space(6.0);
                            ui.add(egui::Label::new(RichText::new(self.t("keeper_note")).color(th().ink2)).wrap());
                            ui.colored_label(th().ink2, RichText::new(self.t("keeper_stat").replace("{n}", &self.mail_kept.0.to_string()).replace("{u}", &self.mail_kept.1.to_string())).small());
                            ui.add_space(18.0);
                            ui.label(RichText::new(self.t("e2e_h")).color(th().ink).b());
                            ui.add(egui::Label::new(RichText::new(self.t("e2e_note")).color(th().ink2)).wrap());
                        }
                    }
                    ui.add_space(30.0);
                });
            });
    }

    fn close_settings(&mut self) {
        self.show_settings = false;
        self.monitor = None;
        store::save_settings(&self.settings);
    }

    fn tab_appearance(&mut self, ui: &mut egui::Ui) {
        let c = th();
        let label = self.t("fx_opt");
        if ui.checkbox(&mut self.settings.hack_fx, label).changed() {
            store::save_settings(&self.settings);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(self.t("intro_opt"));
            let cur = if self.settings.intro_mode.is_empty() { "first".to_string() } else { self.settings.intro_mode.clone() };
            for (id, key) in [("always", "intro_always"), ("first", "intro_first"), ("never", "intro_never")] {
                if ui.selectable_label(cur == id, self.t(key)).clicked() {
                    self.settings.intro_mode = id.into();
                    store::save_settings(&self.settings);
                }
            }
            if ui.button(self.t("intro_watch")).clicked() {
                self.intro = crate::intro::Intro::start();
                self.show_settings = false;
            }
        });
        let label = self.t("splash_opt");
        if ui.checkbox(&mut self.settings.splash, label).changed() {
            store::save_settings(&self.settings);
        }
        ui.add_space(14.0);
        ui.label(RichText::new(self.t("theme_h").to_uppercase()).color(c.ink2).size(12.0).b());
        ui.add_space(8.0);
        let cols = ((ui.available_width() + 14.0) / 214.0).floor().max(1.0) as usize;
        let mut picked: Option<&'static str> = None;
        egui::Grid::new("themes").spacing(egui::vec2(14.0, 14.0)).show(ui, |ui| {
            for (i, t) in theme::THEMES.iter().enumerate() {
                let active = self.settings.theme == t.id || (self.settings.theme.is_empty() && i == 0);
                if theme_card(ui, &t.pal, self.t(t.name_key), active).clicked() {
                    picked = Some(t.id);
                }
                if (i + 1) % cols == 0 {
                    ui.end_row();
                }
            }
        });
        if let Some(id) = picked {
            self.settings.theme = id.to_string();
            store::save_settings(&self.settings);
        }
        ui.add_space(8.0);
        ui.colored_label(c.ink2, RichText::new(self.t("theme_note")).small());
        ui.add_space(22.0);
        ui.label(RichText::new(self.t("ui_scale").to_uppercase()).color(c.ink2).size(12.0).b());
        ui.add_space(6.0);
        let mut v = self.settings.ui_scale;
        let r = ui.add(egui::Slider::new(&mut v, 80.0..=150.0).step_by(5.0).fixed_decimals(0).suffix(" %"));
        if r.changed() {
            self.settings.ui_scale = v;
        }
        if r.drag_stopped() || r.lost_focus() {
            store::save_settings(&self.settings);
        }
    }

    fn tab_language(&mut self, ui: &mut egui::Ui) {
        let label = self.t("ch_translate_opt");
        if ui.checkbox(&mut self.settings.ch_translate, label).changed() {
            store::save_settings(&self.settings);
        }
        ui.colored_label(th().ink2, RichText::new(self.t("ch_translate_note")).small());
        ui.add_space(14.0);
        let sys = i18n::system();
        let auto_label = self.t("lang_auto").replace("{lang}", sys.native());
        let current = if self.settings.lang.is_empty() || self.settings.lang == "auto" { auto_label.clone() } else { self.lang.native().to_string() };
        let mut choice: Option<String> = None;
        egui::ComboBox::from_id_salt("lang_combo").selected_text(current).width(340.0).show_ui(ui, |ui| {
            if ui.selectable_label(self.settings.lang.is_empty() || self.settings.lang == "auto", &auto_label).clicked() {
                choice = Some("auto".into());
            }
            for l in i18n::ALL {
                if ui.selectable_label(self.settings.lang == l.code(), l.native()).clicked() {
                    choice = Some(l.code().into());
                }
            }
        });
        if let Some(cn) = choice {
            self.settings.lang = cn;
            store::save_settings(&self.settings);
            let new = i18n::resolve(&self.settings.lang);
            if new != self.lang {
                self.lang = new;
                install_fonts(ui.ctx(), new);
            }
        }
    }

    fn tab_help(&mut self, ui: &mut egui::Ui) {
        if ui.add(egui::Button::new(RichText::new(self.t("show_tour")).color(th().on_accent).b()).fill(th().accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(200.0, 36.0))).clicked() {
            self.close_settings();
            self.go_tour(0);
            return;
        }
        ui.add_space(20.0);
        self.words_section(ui);
        ui.add_space(22.0);
        self.guide_card(ui);
    }

    fn tab_about(&mut self, ui: &mut egui::Ui) {
        let c = th();
        ui.horizontal(|ui| {
            let tex = self.icons.get(ui.ctx(), "app", APP_ICON);
            ui.add(egui::Image::new(SizedTexture::new(tex, egui::vec2(72.0, 72.0))));
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("CyberCord {VERSION}")).color(c.ink).b().size(20.0));
                docs::brand(ui, "SocioCyberneering Inc.", 15.0, c.ink);
                ui.colored_label(c.ink2, "Communication Cooperations");
            });
        });
        ui.add_space(12.0);
        ui.colored_label(c.ink, self.t("about_body").replace("{v}", VERSION));
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add(egui::Button::new(RichText::new(self.t("sce_doc")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0)).min_size(egui::vec2(220.0, 34.0))).clicked() {
                self.doc_open = Some(Doc::Sce);
            }
            if ui.add(egui::Button::new(RichText::new(self.t("intro_watch_long")).color(c.ink)).fill(c.card).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 34.0))).clicked() {
                self.intro = crate::intro::Intro::start();
                self.show_settings = false;
            }
            for (key, file) in [("pdf_venus", "venus-project.pdf"), ("pdf_gs", "general-semantics.pdf")] {
                if ui.add(egui::Button::new(RichText::new(format!("PDF · {}", self.t(key))).color(c.ink)).fill(c.card).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 34.0))).clicked() {
                    self.open_builtin_pdf(file);
                }
            }
        });
        ui.add_space(10.0);
        if ui.add(egui::Button::new(RichText::new(self.t("invite_btn")).color(c.on_accent).b()).fill(c.green).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 34.0))).clicked() {
            self.show_invite = true;
            self.show_settings = false;
        }
        ui.add_space(14.0);
        self.update_section(ui);
        ui.add_space(14.0);
        ui.label(RichText::new(self.t("devm_title").to_uppercase()).color(c.ink2).size(12.0).b());
        ui.horizontal(|ui| {
            let mut on = self.settings.dev;
            if ui.checkbox(&mut on, self.t("devm_toggle")).changed() {
                self.settings.dev = on;
                if !on {
                    crate::dev::stop_all();
                    self.show_dev = false;
                }
                store::save_settings(&self.settings);
            }
            if self.dev_on() && ui.button(self.t("devm_open")).clicked() {
                self.show_dev = true;
                self.show_settings = false;
            }
        });
        ui.add(egui::Label::new(RichText::new(self.t("devm_about")).color(c.ink2).small()).wrap());
        ui.add_space(10.0);
        ui.colored_label(c.ink2, RichText::new(self.t("twemoji_credit")).small());
        ui.colored_label(c.ink2, RichText::new(self.t("fonts_credit")).small());
    }

    fn tab_voice(&mut self, ui: &mut egui::Ui) {
        let c = th();
        if !voice::AUDIO_ENABLED {
            ui.colored_label(c.red, self.t("no_sound_build"));
            return;
        }
        if self.dev_lists.is_none() {
            self.dev_lists = Some((voice::list_outputs(), voice::list_inputs()));
        }
        self.sounds_section(ui);
        ui.add_space(14.0);
        let (outs, ins) = self.dev_lists.clone().unwrap_or_default();
        let default = self.t("dev_default").to_string();
        let shown = |v: &str| if v.is_empty() { default.clone() } else { v.to_string() };
        let mut dev_changed = false;
        let mut levels_changed = false;
        let col_w = ((ui.available_width() - 24.0) / 2.0).max(200.0);

        ui.horizontal_top(|ui| {
            // input column
            ui.vertical(|ui| {
                ui.set_width(col_w);
                ui.label(RichText::new(self.t("in_dev").to_uppercase()).color(c.ink2).size(12.0).b());
                ui.add_space(4.0);
                let mut sel = self.settings.in_dev.clone();
                egui::ComboBox::from_id_salt("in_dev").selected_text(shown(&sel)).width(col_w - 8.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut sel, String::new(), &default);
                    for d in &ins {
                        ui.selectable_value(&mut sel, d.clone(), d);
                    }
                });
                if sel != self.settings.in_dev {
                    self.settings.in_dev = sel;
                    dev_changed = true;
                }
                ui.add_space(14.0);
                ui.label(RichText::new(self.t("in_vol").to_uppercase()).color(c.ink2).size(12.0).b());
                ui.spacing_mut().slider_width = col_w - 80.0;
                if ui.add(egui::Slider::new(&mut self.settings.in_vol, 0.0..=200.0).step_by(1.0).fixed_decimals(0).suffix(" %")).changed() {
                    levels_changed = true;
                }
            });
            ui.add_space(24.0);
            // output column
            ui.vertical(|ui| {
                ui.set_width(col_w);
                ui.label(RichText::new(self.t("out_dev").to_uppercase()).color(c.ink2).size(12.0).b());
                ui.add_space(4.0);
                let mut sel = self.settings.out_dev.clone();
                egui::ComboBox::from_id_salt("out_dev").selected_text(shown(&sel)).width(col_w - 8.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut sel, String::new(), &default);
                    for d in &outs {
                        ui.selectable_value(&mut sel, d.clone(), d);
                    }
                });
                if sel != self.settings.out_dev {
                    self.settings.out_dev = sel;
                    dev_changed = true;
                }
                ui.add_space(14.0);
                ui.label(RichText::new(self.t("out_vol").to_uppercase()).color(c.ink2).size(12.0).b());
                ui.spacing_mut().slider_width = col_w - 80.0;
                if ui.add(egui::Slider::new(&mut self.settings.out_vol, 0.0..=200.0).step_by(1.0).fixed_decimals(0).suffix(" %")).changed() {
                    levels_changed = true;
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new(format!("♪  {}", self.t("test_sound"))).color(c.ink)).fill(c.card).rounding(Rounding::same(8.0))).clicked() {
                let (rate, pcm) = voice_rec::chime();
                self.test_player = voice_rec::play(rate, pcm).map(|p| (p, Instant::now()));
            }
            if ui.add(egui::Button::new(RichText::new(self.t("dev_refresh")).color(c.ink2)).frame(false)).clicked() {
                self.dev_lists = None;
            }
        });
        ui.colored_label(c.ink2, RichText::new(self.t("dev_note")).small());

        // ---- mic test with live waveform ----
        ui.add_space(22.0);
        sep(ui);
        ui.add_space(6.0);
        ui.label(RichText::new(self.t("mic_check_h").to_uppercase()).color(c.ink2).size(12.0).b());
        ui.add_space(2.0);
        ui.colored_label(c.ink2, self.t("mic_check_hint"));
        ui.add_space(8.0);
        let testing = self.monitor.is_some();
        ui.horizontal(|ui| {
            let (label, fill) = if testing { (self.t("mic_check_stop"), c.red) } else { (self.t("mic_check_btn"), c.accent) };
            if ui.add(egui::Button::new(RichText::new(label).color(c.on_accent).b()).fill(fill).rounding(Rounding::same(8.0)).min_size(egui::vec2(150.0, 36.0))).clicked() {
                if testing {
                    self.monitor = None;
                } else {
                    self.mic_heard = 0.0;
                    self.mic_test_msg = None;
                    self.music.player = None;
                    self.monitor = voice_rec::Monitor::start();
                    self.set_opened = Instant::now() - Duration::from_secs(1);
                    if self.monitor.is_none() {
                        self.mic_test_msg = Some("vm_no_mic");
                    }
                }
            }
        });
        ui.add_space(10.0);
        let peaks = self.monitor.as_ref().map(|m| m.recent()).unwrap_or_default();
        let level = peaks.iter().rev().take(5).fold(0f32, |m, v| m.max(*v));
        self.mic_heard = self.mic_heard.max(peaks.iter().fold(0f32, |m, v| m.max(*v)));
        let th_now = voice::gate_threshold();
        waveform(ui, &peaks, th_now, testing);
        if testing {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
            // nothing heard after 4 s → say so
            if self.set_opened.elapsed() > Duration::from_secs(5) && self.mic_heard < 0.01 {
                self.mic_test_msg = Some("mic_silent");
            } else if self.mic_heard >= 0.01 {
                self.mic_test_msg = None;
            }
        }
        if let Some(k) = self.mic_test_msg {
            ui.colored_label(c.red, self.t(k));
        }

        // ---- input sensitivity ----
        ui.add_space(22.0);
        ui.label(RichText::new(self.t("sens_h").to_uppercase()).color(c.ink2).size(12.0).b());
        ui.add_space(4.0);
        let auto_label = self.t("sens_auto");
        if ui.checkbox(&mut self.settings.sens_auto, auto_label).changed() {
            levels_changed = true;
        }
        ui.add_space(6.0);
        if let Some(db) = sensitivity_bar(ui, level, if self.settings.sens_auto { None } else { Some(self.settings.sens_db) }, th_now) {
            self.settings.sens_db = db;
            levels_changed = true;
        }
        ui.colored_label(c.ink2, RichText::new(self.t("sens_hint")).small());

        if levels_changed {
            self.settings.apply_levels();
        }
        if dev_changed {
            store::save_settings(&self.settings);
            voice::set_devices(Some(self.settings.in_dev.clone()), Some(self.settings.out_dev.clone()));
            self.music.player = None;
            if self.monitor.is_some() {
                self.monitor = voice_rec::Monitor::start();
            }
            if let Some((g, ch)) = self.in_voice.as_deref().and_then(|r| r.split_once('/')) {
                let _ = self.handle.cmd_tx.send(Cmd::JoinVoice { channel: ChannelId::new(g, ch) });
                if self.mic_on {
                    let _ = self.handle.cmd_tx.send(Cmd::SetMic(true));
                }
            }
        }
        if self.test_player.as_ref().map(|(_, t)| t.elapsed() > Duration::from_secs(7)).unwrap_or(false) {
            self.test_player = None;
        }
    }
}

/// A clickable mini preview of a theme.
fn theme_card(ui: &mut egui::Ui, p: &theme::Palette, name: &str, active: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(200.0, 158.0), egui::Sense::click());
    let cur = th();
    let painter = ui.painter();
    let prev = Rect::from_min_size(r.min, egui::vec2(200.0, 118.0));
    let ring = if active { cur.accent } else if resp.hovered() { cur.ink2 } else { cur.line };
    painter.rect_filled(prev, Rounding::same(10.0), p.window);
    // rail with server dots
    let rail = Rect::from_min_size(prev.min, egui::vec2(26.0, prev.height()));
    painter.rect_filled(rail, Rounding { nw: 10.0, sw: 10.0, ne: 0.0, se: 0.0 }, p.rail);
    for (i, col) in [p.accent, p.green, p.red, p.peri].iter().enumerate() {
        painter.circle_filled(egui::pos2(rail.center().x, prev.min.y + 16.0 + i as f32 * 22.0), 7.5, *col);
    }
    // channel column
    let side = Rect::from_min_size(egui::pos2(rail.max.x, prev.min.y), egui::vec2(52.0, prev.height()));
    painter.rect_filled(side, Rounding::ZERO, p.surface);
    for i in 0..5 {
        let y = prev.min.y + 14.0 + i as f32 * 16.0;
        let w = [34.0, 28.0, 38.0, 24.0, 30.0][i];
        if i == 1 {
            painter.rect_filled(Rect::from_min_size(egui::pos2(side.min.x + 4.0, y - 5.0), egui::vec2(44.0, 13.0)), Rounding::same(3.0), p.sel);
        }
        painter.rect_filled(Rect::from_min_size(egui::pos2(side.min.x + 8.0, y - 2.0), egui::vec2(w, 5.0)), Rounding::same(2.5), if i == 1 { p.ink } else { p.ink2 });
    }
    // chat
    let chat_x = side.max.x + 10.0;
    for i in 0..3 {
        let y = prev.min.y + 16.0 + i as f32 * 28.0;
        painter.circle_filled(egui::pos2(chat_x + 6.0, y + 4.0), 6.0, [p.accent, p.green, p.peri][i]);
        painter.rect_filled(Rect::from_min_size(egui::pos2(chat_x + 16.0, y - 1.0), egui::vec2(36.0, 5.0)), Rounding::same(2.5), p.ink);
        painter.rect_filled(Rect::from_min_size(egui::pos2(chat_x + 16.0, y + 8.0), egui::vec2([80.0, 62.0, 70.0][i], 5.0)), Rounding::same(2.5), p.ink2);
    }
    // composer + send button
    let comp = Rect::from_min_size(egui::pos2(chat_x, prev.max.y - 22.0), egui::vec2(prev.max.x - chat_x - 8.0, 14.0));
    painter.rect_filled(comp, Rounding::same(4.0), p.card);
    painter.rect_filled(Rect::from_min_size(egui::pos2(comp.max.x - 26.0, comp.min.y + 2.0), egui::vec2(24.0, 10.0)), Rounding::same(3.0), p.accent);
    if p.mono {
        painter.text(egui::pos2(chat_x + 2.0, prev.max.y - 36.0), Align2::LEFT_CENTER, "$ _", egui::FontId::monospace(10.0), p.accent);
    }
    painter.rect_stroke(prev, Rounding::same(10.0), Stroke::new(if active { 2.5 } else { 1.0 }, ring));
    if active {
        let cc = prev.right_top() + egui::vec2(-14.0, 14.0);
        painter.circle_filled(cc, 9.0, cur.accent);
        painter.line_segment([cc + egui::vec2(-4.0, 0.0), cc + egui::vec2(-1.0, 3.0)], Stroke::new(2.0, cur.on_accent));
        painter.line_segment([cc + egui::vec2(-1.0, 3.0), cc + egui::vec2(4.5, -3.5)], Stroke::new(2.0, cur.on_accent));
    }
    let font = if active { egui::FontId::new(14.0, egui::FontFamily::Name("bold".into())) } else { egui::FontId::proportional(14.0) };
    painter.text(egui::pos2(r.min.x + 2.0, prev.max.y + 18.0), Align2::LEFT_CENTER, name, font, if active { cur.ink } else { cur.ink2 });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Scrolling mirrored waveform of the last few seconds of microphone loudness.
/// Bars above the sensitivity threshold are green (would be sent).
fn waveform(ui: &mut egui::Ui, peaks: &[f32], threshold: f32, live: bool) {
    let c = th();
    let w = ui.available_width().min(760.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 110.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(r, Rounding::same(12.0), c.card);
    let inner = r.shrink2(egui::vec2(12.0, 10.0));
    let mid = inner.center().y;
    painter.line_segment([egui::pos2(inner.min.x, mid), egui::pos2(inner.max.x, mid)], Stroke::new(1.0, c.line));
    // threshold lines
    let amp = |v: f32| (v.sqrt() * inner.height() / 2.0).min(inner.height() / 2.0);
    let ty = amp(threshold);
    for y in [mid - ty, mid + ty] {
        let mut x = inner.min.x;
        while x < inner.max.x {
            painter.line_segment([egui::pos2(x, y), egui::pos2((x + 6.0).min(inner.max.x), y)], Stroke::new(1.0, c.ink2.gamma_multiply(0.6)));
            x += 11.0;
        }
    }
    let bar = 4.0;
    let n = (inner.width() / bar) as usize;
    // 400 buckets of 10 ms → n bars, newest on the right
    let src = peaks.len();
    for i in 0..n {
        let idx = (i as f32 / n as f32 * 400.0) as usize;
        let from_end = 400usize.saturating_sub(idx);
        if from_end > src {
            continue;
        }
        let j = src - from_end;
        let a = (j.saturating_sub(1)..(j + 1).min(src)).fold(0f32, |m, k| m.max(peaks[k]));
        let h = amp(a).max(1.0);
        let x = inner.min.x + i as f32 * bar + bar / 2.0;
        let col = if a >= threshold { c.green } else { c.ink2.gamma_multiply(0.7) };
        painter.line_segment([egui::pos2(x, mid - h), egui::pos2(x, mid + h)], Stroke::new(bar - 1.2, col));
    }
    if !live && peaks.is_empty() {
        painter.text(r.center(), Align2::CENTER_CENTER, "—", egui::FontId::proportional(18.0), c.ink2);
    }
}

/// Discord-style sensitivity bar: live level behind a draggable threshold
/// marker (dBFS, -100..0). Returns the new threshold if the user dragged it.
fn sensitivity_bar(ui: &mut egui::Ui, level: f32, manual_db: Option<f32>, auto_threshold: f32) -> Option<f32> {
    let c = th();
    let w = ui.available_width().min(760.0);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), if manual_db.is_some() { egui::Sense::click_and_drag() } else { egui::Sense::hover() });
    let track = Rect::from_center_size(r.center(), egui::vec2(r.width() - 16.0, 10.0));
    let to_x = |db: f32| track.min.x + ((db + 100.0) / 100.0).clamp(0.0, 1.0) * track.width();
    let th_db = manual_db.unwrap_or_else(|| voice::lin_to_db(auto_threshold));
    let lvl_db = voice::lin_to_db(level);
    let painter = ui.painter();
    painter.rect_filled(track, Rounding::same(5.0), c.card);
    // level fill: amber below the threshold, green above
    let lx = to_x(lvl_db);
    let tx = to_x(th_db);
    if lx > track.min.x {
        let amber = Color32::from_rgb(0xF0, 0xB2, 0x32);
        painter.rect_filled(Rect::from_min_max(track.min, egui::pos2(lx.min(tx), track.max.y)), Rounding::same(5.0), amber);
        if lx > tx {
            painter.rect_filled(Rect::from_min_max(egui::pos2(tx, track.min.y), egui::pos2(lx, track.max.y)), Rounding::same(5.0), c.green);
        }
    }
    // marker
    let enabled = manual_db.is_some();
    let knob = Rect::from_center_size(egui::pos2(tx, r.center().y), egui::vec2(10.0, 24.0));
    painter.rect_filled(knob, Rounding::same(3.0), if enabled { c.ink } else { c.ink2.gamma_multiply(0.6) });
    painter.text(egui::pos2(track.max.x, r.min.y - 2.0), Align2::RIGHT_BOTTOM, format!("{th_db:.0} dB"), egui::FontId::proportional(11.0), c.ink2);
    if enabled && (resp.dragged() || resp.clicked()) {
        if let Some(pos) = resp.interact_pointer_pos() {
            let db = ((pos.x - track.min.x) / track.width() * 100.0 - 100.0).clamp(-100.0, 0.0);
            return Some(db.round());
        }
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    None
}

// ---- network map + people search ----

#[derive(Default)]
struct MapState {
    /// Smoothed position of each node, relative to the map centre (in points).
    pos: HashMap<String, Vec2>,
    selected: Option<String>,
}

fn hash01(s: &str, salt: u32) -> f32 {
    let h = s.bytes().fold(2166136261u32 ^ salt, |a, b| (a ^ b as u32).wrapping_mul(16777619));
    (h % 10_000) as f32 / 10_000.0
}

const NETWORK_TILE: &[u8] = include_bytes!("../assets/servers/network.png");

impl CyberApp {
    fn ago(&self, ms: u64) -> String {
        let d = now_ms().saturating_sub(ms) / 1000;
        if d < 3600 {
            self.t("ago_min").replace("{n}", &(d / 60).max(1).to_string())
        } else if d < 86_400 {
            self.t("ago_h").replace("{n}", &(d / 3600).to_string())
        } else {
            self.t("ago_d").replace("{n}", &(d / 86_400).to_string())
        }
    }

    fn src_label(&self, src: &str) -> &'static str {
        match src {
            "lan" => self.t("src_lan"),
            "saved" => self.t("src_saved"),
            "friend" => self.t("src_friend"),
            "node" => self.t("src_node"),
            "dht" => self.t("src_dht"),
            "identify" => self.t("src_identify"),
            "search" => self.t("src_search"),
            "global" => self.t("src_global"),
            _ => self.t("src_exchange"),
        }
    }

    fn link_label(&self, link: &str) -> &'static str {
        match link {
            "lan" => self.t("link_lan"),
            "relay" => self.t("link_relay"),
            _ => self.t("link_direct"),
        }
    }

    /// Add as friend (if needed) and open the conversation.
    fn befriend(&mut self, peer: &str, name: &str, open_dm: bool) {
        if !self.friends.contains_key(peer) {
            let _ = self.handle.cmd_tx.send(Cmd::AddFriend(Friend { peer_id: peer.to_string(), name: name.to_string(), addrs: Vec::new() }));
            self.friends.insert(peer.to_string(), FriendView { name: name.to_string(), online: false });
            let m = self.t("added").replace("{name}", name);
            self.toast(m);
        }
        if open_dm {
            self.dm_open = Some(peer.to_string());
            self.show_friends = true;
            self.show_home = false;
            self.show_map = false;
                            self.show_city = false;
                            self.show_papers = false;
                            self.show_subs = false;
        }
    }

    /// Search box + results; used on the Friends page and the map.
    fn people_search(&mut self, ui: &mut egui::Ui, compact: bool) {
        let c = th();
        let hint = self.t("find_hint");
        let resp = ui.add(egui::TextEdit::singleline(&mut self.search_q).hint_text(format!("🔍  {hint}")).desired_width(f32::INFINITY));
        if resp.changed() {
            self.search_res.clear();
            let _ = self.handle.cmd_tx.send(Cmd::Search(self.search_q.clone()));
        }
        if self.search_q.trim().is_empty() {
            return;
        }
        ui.add_space(6.0);
        if self.search_res.is_empty() {
            ui.colored_label(c.ink2, RichText::new(self.t("find_none")).small());
            return;
        }
        let res = self.search_res.clone();
        let max = if compact { 6 } else { 20 };
        for u in res.iter().take(max) {
            ui.horizontal(|ui| {
                let initial = u.name.chars().next().unwrap_or('?').to_uppercase().to_string();
                avatar(ui, &initial, 15.0, name_color(&u.name), u.online.then_some(c.green));
                ui.vertical(|ui| {
                    ui.label(RichText::new(&u.name).color(c.ink).b());
                    let online = u.online && u.status != "offline";
                    let status = if online && !u.status.is_empty() { self.status_label(&u.status).to_string() } else if online { self.t("lg_online").to_string() } else { self.t("last_seen").replace("{t}", &self.ago(u.last_seen_ms)) };
                    let via = if u.via == "dht" { format!(" · {}", self.t("find_dht")) } else { String::new() };
                    ui.colored_label(c.ink2, RichText::new(format!("{status} · @{}{via}", cybercord_node::types::handle(&u.name, &u.peer))).small());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.friends.contains_key(&u.peer) {
                        if ui.add(egui::Button::new(RichText::new(self.t("node_msg")).color(c.on_accent)).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                            self.befriend(&u.peer, &u.name, true);
                        }
                    } else {
                        if ui.add(egui::Button::new(RichText::new(self.t("node_msg")).color(c.ink)).fill(c.card).rounding(Rounding::same(8.0))).clicked() {
                            self.befriend(&u.peer, &u.name, true);
                        }
                        if ui.add(egui::Button::new(RichText::new(self.t("node_add")).color(c.on_accent).b()).fill(c.green).rounding(Rounding::same(8.0))).clicked() {
                            self.befriend(&u.peer, &u.name, false);
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }
    }

    fn map_view(&mut self, ui: &mut egui::Ui) {
        let c = th();
        let time = ui.input(|i| i.time) as f32;
        let nodes = self.nodes.clone();
        let online = nodes.iter().filter(|n| n.online && n.name.is_some()).count();
        let connected = nodes.iter().filter(|n| n.connected).count();
        // header
        ui.horizontal(|ui| {
            let tex = self.icons.get(ui.ctx(), "network", NETWORK_TILE);
            ui.add(egui::Image::new(SizedTexture::new(tex, egui::vec2(30.0, 30.0))));
            ui.vertical(|ui| {
                ui.label(RichText::new(self.t("map_title")).color(c.ink).b().size(19.0));
                ui.colored_label(c.ink2, RichText::new(self.t("map_sub")).small());
            });
        });
        ui.add_space(6.0);
        let side_w = 300.0;
        let avail = ui.available_size();
        ui.horizontal_top(|ui| {
            // ---------- canvas ----------
            let size = egui::vec2((avail.x - side_w - 12.0).max(300.0), avail.y.max(300.0));
            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, Rounding::same(14.0), c.surface);
            let center = rect.center();
            let m = rect.width().min(rect.height());
            // orbit guides
            for (k, a) in [(0.27, 0.5), (0.37, 0.35), (0.45, 0.25)] {
                painter.circle_stroke(center, m * k, Stroke::new(1.0, c.line.gamma_multiply(a + 0.3)));
            }
            // radar waves from you (the app is looking for nodes)
            for i in 0..3 {
                let ph = ((time * 0.45 + i as f32 / 3.0) % 1.0) as f32;
                painter.circle_stroke(center, 22.0 + ph * m * 0.48, Stroke::new(2.0, c.accent.gamma_multiply((1.0 - ph) * 0.35)));
            }
            // positions (stable angle per node, slow drift)
            let shown: Vec<&cybercord_node::types::NodeInfo> = nodes.iter().take(160).collect();
            // Evenly spread each orbit, ordered by a stable per-node angle (no overlaps).
            let ring_of = |n: &cybercord_node::types::NodeInfo| if n.connected { 0 } else if n.online { 1 } else { 2 };
            let mut slot: HashMap<String, (usize, usize)> = HashMap::new();
            for ring in 0..3 {
                let mut ids: Vec<&str> = shown.iter().filter(|n| ring_of(n) == ring).map(|n| n.peer.as_str()).collect();
                ids.sort_by(|a, b| hash01(a, 1).total_cmp(&hash01(b, 1)));
                let total = ids.len();
                for (i, id) in ids.into_iter().enumerate() {
                    slot.insert(id.to_string(), (i, total));
                }
            }
            let mut placed: Vec<(String, Pos2, f32)> = Vec::new();
            for n in &shown {
                let ring = [0.27, 0.37, 0.45][ring_of(n)];
                let (i, total) = slot.get(&n.peer).copied().unwrap_or((0, 1));
                let ang = std::f32::consts::TAU * (i as f32 / total.max(1) as f32) + ring_of(n) as f32 * 0.5 + time * 0.015;
                let jitter = if ring_of(n) == 2 { (hash01(&n.peer, 7) - 0.5) * 0.04 } else { 0.0 };
                let target = egui::vec2(ang.cos(), ang.sin()) * m * (ring + jitter);
                let p = self.map.pos.entry(n.peer.clone()).or_insert(target * 1.3);
                *p += (target - *p) * 0.08;
                let r = if n.connected { 15.0 } else if n.online { 10.0 } else { 6.5 };
                placed.push((n.peer.clone(), center + *p, r));
            }
            // edges
            for (n, (_, pos, _)) in shown.iter().zip(placed.iter()) {
                if n.connected {
                    let col = match n.link {
                        "lan" => c.green,
                        "relay" => Color32::from_rgb(0x91, 0x41, 0xAC),
                        _ => c.accent,
                    };
                    if n.link == "relay" {
                        let d = *pos - center;
                        let len = d.length();
                        let mut t = 0.0;
                        while t < len {
                            let a = center + d * (t / len);
                            let b = center + d * ((t + 7.0).min(len) / len);
                            painter.line_segment([a, b], Stroke::new(2.0, col));
                            t += 12.0;
                        }
                    } else {
                        painter.line_segment([center, *pos], Stroke::new(2.2, col.gamma_multiply(0.8)));
                    }
                    // a little packet travelling along the link: the connection is alive
                    let ph = (time * 0.6 + hash01(&n.peer, 3)) % 1.0;
                    painter.circle_filled(center + (*pos - center) * ph, 2.5, col);
                }
            }
            // hover / click
            let pointer = resp.hover_pos();
            let hovered = pointer.and_then(|pp| placed.iter().filter(|(_, p, r)| p.distance(pp) <= r + 5.0).min_by(|a, b| a.1.distance(pp).total_cmp(&b.1.distance(pp))).map(|(id, _, _)| id.clone()));
            if resp.clicked() {
                self.map.selected = hovered.clone();
            }
            // nodes
            for (n, (id, pos, r)) in shown.iter().zip(placed.iter()) {
                let name = n.name.clone().unwrap_or_default();
                let base = if n.name.is_some() { name_color(&name) } else { c.ink2 };
                let fill = if n.connected { base } else if n.online { base.gamma_multiply(0.6) } else { c.line };
                let sel = self.map.selected.as_deref() == Some(id.as_str());
                let hov = hovered.as_deref() == Some(id.as_str());
                if sel || hov {
                    painter.circle_stroke(*pos, r + 5.0, Stroke::new(2.0, c.accent));
                }
                if n.relays_us || n.we_relay {
                    painter.circle_stroke(*pos, r + 2.5, Stroke::new(1.5, Color32::from_rgb(0x91, 0x41, 0xAC)));
                }
                painter.circle_filled(*pos, *r, fill);
                if self.friends.contains_key(id) {
                    painter.circle_filled(*pos + egui::vec2(r * 0.75, -r * 0.75), 4.0, c.green);
                }
                if *r >= 10.0 {
                    let initial = name.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
                    painter.text(*pos, Align2::CENTER_CENTER, initial, egui::FontId::new(r * 0.95, egui::FontFamily::Name("bold".into())), c.on_accent);
                }
                if n.connected || sel || hov {
                    let label = if name.is_empty() { short(id) } else { name.clone() };
                    painter.text(*pos + egui::vec2(0.0, r + 9.0), Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), c.ink);
                }
            }
            // you
            painter.circle_filled(center, 22.0, c.accent);
            painter.circle_stroke(center, 25.0, Stroke::new(2.0, c.accent.gamma_multiply(0.5)));
            let me = self.self_name.chars().next().unwrap_or('?').to_uppercase().to_string();
            painter.text(center, Align2::CENTER_CENTER, me, egui::FontId::new(20.0, egui::FontFamily::Name("bold".into())), c.on_accent);
            painter.text(center + egui::vec2(0.0, 36.0), Align2::CENTER_CENTER, self.t("lg_you"), egui::FontId::new(12.0, egui::FontFamily::Name("bold".into())), c.ink);
            if shown.is_empty() {
                painter.text(center + egui::vec2(0.0, 64.0), Align2::CENTER_CENTER, self.t("map_empty"), egui::FontId::proportional(14.0), c.ink2);
            }
            if let Some(h) = &hovered {
                if let Some(n) = nodes.iter().find(|n| &n.peer == h) {
                    let mut tip = n.name.clone().unwrap_or_else(|| self.t("node_unknown").to_string());
                    if n.connected {
                        tip.push_str(&format!("\n{}", self.link_label(n.link)));
                    }
                    tip.push_str(&format!("\n{}", self.t("found_via").replace("{s}", self.src_label(n.source))));
                    resp.clone().on_hover_text_at_pointer(tip);
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(33));

            // ---------- side column ----------
            ui.vertical(|ui| {
                ui.set_width(side_w);
                egui::ScrollArea::vertical().id_salt("map_side").max_height(avail.y).show(ui, |ui| {
                    card(ui, self.t("map_title"), |ui| {
                        ui.label(
                            RichText::new(self.t("map_people").replace("{o}", &online.to_string()).replace("{k}", &nodes.len().to_string()).replace("{c}", &connected.to_string()))
                                .color(c.ink),
                        );
                        ui.add_space(6.0);
                        let legend = [
                            (c.accent, self.t("lg_you"), false),
                            (c.green, self.t("link_lan"), true),
                            (c.accent, self.t("link_direct"), true),
                            (Color32::from_rgb(0x91, 0x41, 0xAC), self.t("link_relay"), true),
                            (c.line, self.t("lg_known"), false),
                        ];
                        for (col, label, line) in legend {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 14.0), egui::Sense::hover());
                                if line {
                                    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(2.5, col));
                                } else {
                                    ui.painter().circle_filled(r.center(), 6.0, col);
                                }
                                ui.colored_label(c.ink2, RichText::new(label).small());
                            });
                        }
                        ui.add_space(6.0);
                        if ui.add(egui::Button::new(RichText::new(self.t("scan_now")).color(c.on_accent).b()).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                            let _ = self.handle.cmd_tx.send(Cmd::Rescan);
                        }
                    });
                    ui.add_space(10.0);
                    if let Some(sel) = self.map.selected.clone() {
                        if let Some(n) = nodes.iter().find(|n| n.peer == sel).cloned() {
                            let title = n.name.clone().unwrap_or_else(|| self.t("node_unknown").to_string());
                            card(ui, &title, |ui| {
                                ui.colored_label(c.ink2, RichText::new(short(&n.peer)).monospace().small());
                                let status = if n.connected {
                                    format!("{} — {}", self.t("lg_conn"), self.link_label(n.link))
                                } else if n.online {
                                    self.t("lg_online").to_string()
                                } else {
                                    self.t("last_seen").replace("{t}", &self.ago(n.last_seen_ms))
                                };
                                ui.colored_label(c.ink, status);
                                if !n.status.is_empty() {
                                    ui.colored_label(status_color(&n.status).unwrap_or(c.ink2), format!("● {}", self.status_label(&n.status)));
                                }
                                ui.colored_label(c.ink2, self.t("found_via").replace("{s}", self.src_label(n.source)));
                                if n.relays_us {
                                    ui.colored_label(c.ink2, self.t("relays_us"));
                                }
                                if n.we_relay {
                                    ui.colored_label(c.ink2, self.t("we_relay"));
                                }
                                ui.add_space(6.0);
                                if let Some(name) = &n.name {
                                    ui.horizontal(|ui| {
                                        if self.friends.contains_key(&n.peer) {
                                            ui.colored_label(c.green_text, format!("✔ {}", self.t("node_friend")));
                                        } else if ui.add(egui::Button::new(RichText::new(self.t("node_add")).color(c.on_accent).b()).fill(c.green).rounding(Rounding::same(8.0))).clicked() {
                                            self.befriend(&n.peer, name, false);
                                        }
                                        if ui.add(egui::Button::new(RichText::new(self.t("node_msg")).color(c.on_accent)).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                                            self.befriend(&n.peer, name, true);
                                        }
                                    });
                                }
                            });
                            ui.add_space(10.0);
                        }
                    }
                    card(ui, self.t("find_people"), |ui| {
                        self.people_search(ui, true);
                    });
                });
            });
        });
    }
}

/// A call between two friends (signals over the DM topic, sound over the DM voice room).
#[derive(Clone)]
enum CallState {
    Idle,
    Outgoing { peer: String, id: u64, since: Instant, last_ring: Instant, round: u32 },
    Incoming { peer: String, name: String, id: u64, since: Instant, last: Instant },
    Active { peer: String, id: u64, since: Instant },
}

fn mmss(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

// ---- background (tray) + desktop notifications ----

pub struct NotifyState {
    pub enabled: bool,
    pub my_name: String,
    /// Window visible and focused (no notifications then).
    pub active: bool,
    pub lang: Lang,
}

/// Forward network events to the UI, wake it, and raise desktop notifications
/// for direct messages and mentions while the window is not in front.
fn spawn_event_pump(rx: std::sync::mpsc::Receiver<UiEvent>, tx: std::sync::mpsc::Sender<UiEvent>, ctx: egui::Context, st: Arc<std::sync::Mutex<NotifyState>>) {
    std::thread::Builder::new()
        .name("event-pump".into())
        .spawn(move || {
            let started = now_ms();
            let mut seen: HashSet<String> = HashSet::new();
            let mut rung: HashSet<u64> = HashSet::new();
            while let Ok(ev) = rx.recv() {
                if let UiEvent::Call { name, id, kind: CallKind::Ring, .. } = &ev {
                    if rung.insert(*id) {
                        let lang = st.lock().map(|s| s.lang).unwrap_or(Lang::En);
                        notify_desktop(i18n::tr_dyn(lang, "call_incoming"), name);
                        crate::tray::bring_to_front(&ctx);
                    }
                }
                if let UiEvent::Chat(m) = &ev {
                    let (enabled, me, active, lang) = st.lock().map(|s| (s.enabled, s.my_name.clone(), s.active, s.lang)).unwrap_or((false, String::new(), true, Lang::En));
                    // only fresh messages (not history arriving by sync), once each
                    let fresh = m.ts_ms + 120_000 > now_ms() && m.ts_ms + 5_000 > started;
                    if enabled && !active && fresh && m.edits.is_none() && m.name != me && seen.insert(m.id.clone()) {
                        let mention = me.chars().count() >= 2 && m.body.to_lowercase().contains(&me.to_lowercase());
                        if m.channel.is_dm() || mention {
                            let title = if m.channel.is_dm() { m.name.clone() } else { {
                                let t = i18n::tr_dyn(lang, &format!("ch_{}", m.channel.channel));
                                format!("{} · #{}", m.name, if t.is_empty() { m.channel.channel.as_str() } else { t })
                            } };
                            let mut body: String = emoji::shortcodes_to_unicode(&m.body).chars().take(160).collect();
                            if body.is_empty() && m.attach.is_some() {
                                body = "📎".into();
                            }
                            notify_desktop(&title, &body);
                        }
                    }
                    let _ = lang;
                }
                if tx.send(ev).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        })
        .ok();
}

fn notify_desktop(title: &str, body: &str) {
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new().appname("CyberCord").summary(&title).body(&body).icon("cybercord").show();
    });
}

/// Applies tray actions right away (network, notifications, settings file) and
/// passes them on to the UI, which may not be drawing while hidden.
fn spawn_tray_router(
    rx: std::sync::mpsc::Receiver<crate::tray::TrayCmd>,
    ui: std::sync::mpsc::Sender<crate::tray::TrayCmd>,
    net: tokio::sync::mpsc::UnboundedSender<Cmd>,
    notify: Arc<std::sync::Mutex<NotifyState>>,
    quitting: Arc<std::sync::atomic::AtomicBool>,
) {
    use crate::tray::{TrayCmd, STATUSES};
    std::thread::Builder::new()
        .name("tray-router".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                let mut st = store::load_settings();
                match &cmd {
                    TrayCmd::SetScan(on) => {
                        st.scan = *on;
                        let _ = net.send(Cmd::SetScan(*on));
                    }
                    TrayCmd::SetStatus(i) => {
                        st.status = STATUSES.get(*i).map(|s| s.0.to_string()).unwrap_or_default();
                        let _ = net.send(Cmd::SetStatus(st.status.clone()));
                    }
                    TrayCmd::SetNotify(on) => {
                        st.notify = *on;
                        if let Ok(mut n) = notify.lock() {
                            n.enabled = *on;
                        }
                    }
                    TrayCmd::Quit => {
                        quitting.store(true, std::sync::atomic::Ordering::SeqCst);
                        crate::dev::stop_all();
                        let _ = net.send(Cmd::Quit);
                        // A hidden window runs no frames (Windows), so the normal close may
                        // never happen: give the node a moment to finish, then exit. All data
                        // is already on disk (messages are appended as they arrive).
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_millis(1500));
                            std::process::exit(0);
                        });
                    }
                    TrayCmd::Open => {}
                }
                if !matches!(cmd, TrayCmd::Open | TrayCmd::Quit) {
                    store::save_settings(&st);
                }
                if ui.send(cmd).is_err() {
                    return;
                }
            }
        })
        .ok();
}

fn tray_state(s: &Settings, lang: Lang, online: usize, connected: usize) -> crate::tray::TrayState {
    use crate::tray::{status_index, STATUSES};
    let t = |k: &'static str| i18n::tr(lang, k).to_string();
    crate::tray::TrayState {
        scan: s.scan,
        status: status_index(&s.status),
        notify: s.notify,
        tooltip: t("tray_tip").replace("{o}", &online.to_string()).replace("{c}", &connected.to_string()),
        l_open: t("tray_open"),
        l_scan: t("tray_scan"),
        l_status: t("tray_status"),
        l_notify: t("tray_notify"),
        l_quit: t("tray_quit"),
        l_statuses: STATUSES.iter().map(|(_, k)| t(k)).collect(),
    }
}

/// Colour of a status dot.
fn status_color(id: &str) -> Option<Color32> {
    Some(match id {
        "online" => Color32::from_rgb(0x2E, 0xC2, 0x7E),
        "offline" => Color32::from_rgb(0x8A, 0x8D, 0x93),
        "work" => Color32::from_rgb(0x35, 0x84, 0xE4),
        "sleep" => Color32::from_rgb(0x91, 0x41, 0xAC),
        "busy" => Color32::from_rgb(0xE0, 0x1B, 0x24),
        "learning" => Color32::from_rgb(0xE5, 0xA5, 0x0A),
        "relax" => Color32::from_rgb(0x21, 0x90, 0xA4),
        "play" => Color32::from_rgb(0xE6, 0x61, 0x00),
        "read" => Color32::from_rgb(0x86, 0x5E, 0x3C),
        _ => return None,
    })
}

impl CyberApp {
    fn started_hidden(mut self, hidden: bool) -> Self {
        let mode = self.settings.intro_mode.as_str();
        // the intro plays once by default; "always" / "never" in Settings → Appearance
        let play = !hidden && std::env::var("CYBERCORD_START").is_err() && (mode == "always" || ((mode.is_empty() || mode == "first") && !self.settings.intro_seen));
        if play {
            self.intro = crate::intro::Intro::start();
        }
        if !hidden && self.settings.splash && std::env::var("CYBERCORD_START").map(|v| !v.contains("nosplash")).unwrap_or(true) {
            self.splash = Some(Instant::now());
        }
        if std::env::var("CYBERCORD_START").map(|v| v.contains("intro")).unwrap_or(false) {
            self.intro = crate::intro::Intro::start();
        }
        if hidden {
            self.brand_played = true;
            if self.tray.is_some() {
                self.hidden = true;
                if let Ok(mut n) = self.notify.lock() {
                    n.active = false;
                }
            } else {
                // no tray to come back from: show minimized instead
                self.pending_minimize = true;
            }
        }
        self
    }

    fn status_label(&self, id: &str) -> &'static str {
        crate::tray::STATUSES.iter().find(|(s, _)| *s == id).map(|(_, k)| self.t(k)).unwrap_or("")
    }

    /// Status of another user, from the latest node snapshot.
    fn status_of(&self, peer: &str) -> String {
        self.nodes.iter().find(|n| n.peer == peer).map(|n| n.status.clone()).unwrap_or_default()
    }

    fn set_status(&mut self, id: &str) {
        self.settings.status = id.to_string();
        store::save_settings(&self.settings);
        let _ = self.handle.cmd_tx.send(Cmd::SetStatus(id.to_string()));
        self.refresh_tray(true);
    }

    fn refresh_tray(&mut self, force: bool) {
        let online = self.nodes.iter().filter(|n| n.online && n.name.is_some()).count();
        let connected = self.nodes.iter().filter(|n| n.connected).count();
        let sig = format!("{}{}{}{}{online}{connected}{:?}", self.settings.scan, self.settings.status, self.settings.notify, self.lang.code(), self.lang);
        if force || sig != self.tray_sig {
            self.tray_sig = sig;
            if let Some(t) = &self.tray {
                t.update(&tray_state(&self.settings, self.lang, online, connected));
            }
        }
    }

    fn background_and_tray(&mut self, ctx: &egui::Context) {
        use crate::tray::{TrayCmd, STATUSES};
        if self.pending_minimize {
            self.pending_minimize = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if let Some(at) = self.demo_close_at {
            if Instant::now() >= at {
                self.demo_close_at = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
        // actions from the tray (already applied to network/settings by the router)
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCmd::Open => self.hidden = false,
                TrayCmd::SetScan(on) => self.settings.scan = on,
                TrayCmd::SetStatus(i) => self.settings.status = STATUSES.get(i).map(|s| s.0.to_string()).unwrap_or_default(),
                TrayCmd::SetNotify(on) => self.settings.notify = on,
                TrayCmd::Quit => {}
            }
            self.refresh_tray(true);
        }
        let quitting = self.quitting.load(std::sync::atomic::Ordering::SeqCst);
        if ctx.input(|i| i.viewport().close_requested()) && !quitting && self.settings.background {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.hidden = true;
                if !self.bg_hint_shown && self.settings.notify {
                    self.bg_hint_shown = true;
                    notify_desktop(self.t("notif_bg_title"), self.t("notif_bg_body"));
                }
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        } else if ctx.input(|i| i.viewport().close_requested()) {
            let _ = self.handle.cmd_tx.send(Cmd::Quit);
        }
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if let Ok(mut n) = self.notify.lock() {
            n.active = !self.hidden && focused && !minimized;
            n.enabled = self.settings.notify;
            n.lang = self.lang;
        }
        self.refresh_tray(false);
    }

    /// Settings → Background & tray.
    fn tab_background(&mut self, ui: &mut egui::Ui) {
        let c = th();
        ui.colored_label(c.ink2, self.t("bg_note"));
        ui.add_space(14.0);
        let l = self.t("bg_close_tray");
        if ui.checkbox(&mut self.settings.background, l).changed() {
            store::save_settings(&self.settings);
        }
        if self.tray.is_none() {
            ui.colored_label(Color32::from_rgb(0xE5, 0xA5, 0x0A), RichText::new(self.t("bg_no_tray")).small());
        }
        ui.add_space(6.0);
        let l = self.t("bg_autostart");
        if ui.checkbox(&mut self.settings.autostart, l).changed() {
            store::save_settings(&self.settings);
            if let Err(e) = set_autostart(self.settings.autostart, &self.self_name) {
                self.toast(e);
            }
        }
        ui.add_space(6.0);
        let l = self.t("tray_scan");
        if ui.checkbox(&mut self.settings.scan, l).changed() {
            store::save_settings(&self.settings);
            let _ = self.handle.cmd_tx.send(Cmd::SetScan(self.settings.scan));
            self.refresh_tray(true);
        }
        ui.add_space(6.0);
        let l = self.t("tray_notify");
        if ui.checkbox(&mut self.settings.notify, l).changed() {
            store::save_settings(&self.settings);
            self.refresh_tray(true);
        }
        ui.add_space(16.0);
        ui.label(RichText::new(self.t("set_status").to_uppercase()).color(c.ink2).size(12.0).b());
        ui.add_space(4.0);
        self.status_picker(ui, "status_tab");
    }

    fn status_picker(&mut self, ui: &mut egui::Ui, id: &str) {
        let cur = self.settings.status.clone();
        let mut pick: Option<&'static str> = None;
        egui::ComboBox::from_id_salt(id).selected_text(self.status_label(&cur)).width(260.0).show_ui(ui, |ui| {
            for (sid, key) in crate::tray::STATUSES {
                let label = self.t(key);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    if let Some(col) = status_color(sid) {
                        ui.painter().circle_filled(r.center(), 5.0, col);
                    }
                    if ui.selectable_label(cur == *sid, label).clicked() {
                        pick = Some(sid);
                    }
                });
            }
        });
        if let Some(p) = pick {
            self.set_status(p);
        }
    }
}

/// Start with the system: XDG autostart entry on Linux, Run key on Windows.
fn set_autostart(on: bool, name: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    {
        let dir = std::env::var("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")).join("autostart");
        let file = dir.join("cybercord.desktop");
        if on {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=CyberCord\nComment=Serverless peer-to-peer chat (runs in the tray)\nExec=\"{}\" --name \"{}\" --minimized\nIcon=cybercord\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
                exe.display(),
                name.replace('"', "")
            );
            std::fs::write(&file, entry).map_err(|e| e.to_string())
        } else {
            let _ = std::fs::remove_file(&file);
            Ok(())
        }
    }
    #[cfg(windows)]
    {
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let status = if on {
            let cmd = format!("\"{}\" --name \"{}\" --minimized", exe.display(), name.replace('"', ""));
            std::process::Command::new("reg").args(["add", key, "/v", "CyberCord", "/t", "REG_SZ", "/d", &cmd, "/f"]).status()
        } else {
            std::process::Command::new("reg").args(["delete", key, "/v", "CyberCord", "/f"]).status()
        };
        status.map_err(|e| e.to_string()).map(|_| ())
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (on, name, exe);
        Err("autostart is not supported on this system".into())
    }
}

// ---- torrent-like files: chat card + downloads page ----

fn human_size(b: u64) -> String {
    let f = b as f64;
    if f >= 1073741824.0 {
        format!("{:.2} GB", f / 1073741824.0)
    } else if f >= 1048576.0 {
        format!("{:.1} MB", f / 1048576.0)
    } else if f >= 1024.0 {
        format!("{:.0} KB", f / 1024.0)
    } else {
        format!("{b} B")
    }
}

/// Open a file or folder with the system's default app.
fn open_path(p: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

impl CyberApp {
    fn transfer_status(&self, t: &cybercord_node::transfer::TransferInfo) -> String {
        let pct = if t.size == 0 { 100 } else { t.have_bytes * 100 / t.size };
        match t.state {
            "waiting" => self.t("file_waiting").to_string(),
            "downloading" => self.t("file_downloading").replace("{p}", &pct.to_string()).replace("{s}", &human_size(t.speed_bps)).replace("{n}", &t.peers.to_string()),
            "paused" => self.t("file_paused").replace("{p}", &pct.to_string()),
            "verifying" => self.t("file_verifying").to_string(),
            "done" => self.t("file_done").to_string(),
            "seeding" => self.t("file_seeding").to_string(),
            _ => t.state.to_string(),
        }
    }

    /// Buttons for a transfer (shared by the chat card and the downloads page).
    fn transfer_buttons(&mut self, ui: &mut egui::Ui, t: &cybercord_node::transfer::TransferInfo) {
        let c = th();
        match t.state {
            "downloading" | "waiting" => {
                if ui.add(egui::Button::new(RichText::new(self.t("file_pause")).color(c.ink)).fill(c.card).rounding(Rounding::same(8.0))).clicked() {
                    let _ = self.handle.cmd_tx.send(Cmd::PauseDownload { hash: t.hash.clone(), paused: true });
                }
            }
            "paused" => {
                if ui.add(egui::Button::new(RichText::new(self.t("file_resume")).color(c.on_accent)).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                    let _ = self.handle.cmd_tx.send(Cmd::PauseDownload { hash: t.hash.clone(), paused: false });
                }
            }
            "done" | "seeding" => {
                if let Some(p) = &t.path {
                    let path = std::path::PathBuf::from(p);
                    if path.extension().map(|e| e.eq_ignore_ascii_case("pdf")).unwrap_or(false) && ui.add(egui::Button::new(RichText::new(self.t("pdf_read")).color(c.on_accent)).fill(c.green).rounding(Rounding::same(8.0))).clicked() {
                        self.open_pdf_file(&path);
                    }
                    if ui.add(egui::Button::new(RichText::new(self.t("file_open")).color(c.on_accent)).fill(c.accent).rounding(Rounding::same(8.0))).clicked() {
                        open_path(&path);
                    }
                    if ui.add(egui::Button::new(RichText::new(self.t("file_folder")).color(c.ink)).fill(c.card).rounding(Rounding::same(8.0))).clicked() {
                        if let Some(dir) = path.parent() {
                            open_path(dir);
                        }
                    }
                }
            }
            _ => {}
        }
        if matches!(t.state, "downloading" | "waiting" | "paused") && ui.add(egui::Button::new(RichText::new(self.t("file_cancel")).color(c.ink2)).frame(false)).clicked() {
            let _ = self.handle.cmd_tx.send(Cmd::CancelDownload(t.hash.clone()));
        }
    }

    fn file_widget(&mut self, ui: &mut egui::Ui, a: &Attachment, ch: &ChannelId) {
        let c = th();
        let t = self.transfers.iter().find(|t| t.hash == a.hash).cloned();
        egui::Frame::none().fill(c.surface).stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(14.0)).inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                // document icon with the extension
                let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 48.0), egui::Sense::hover());
                let p = ui.painter();
                p.rect_filled(r, Rounding::same(6.0), c.accent.gamma_multiply(0.85));
                p.add(egui::Shape::convex_polygon(vec![r.right_top() - egui::vec2(12.0, 0.0), r.right_top(), r.right_top() + egui::vec2(0.0, 12.0)], c.surface, Stroke::NONE));
                let ext: String = a.name.rsplit_once('.').map(|(_, e)| e.to_uppercase()).unwrap_or_default().chars().take(4).collect();
                p.text(r.center() + egui::vec2(0.0, 6.0), Align2::CENTER_CENTER, ext, egui::FontId::new(10.5, egui::FontFamily::Name("bold".into())), c.on_accent);
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(RichText::new(&a.name).color(c.ink).b()).truncate());
                    let status = match &t {
                        Some(t) => format!("{} · {}", human_size(a.size), self.transfer_status(t)),
                        None => human_size(a.size),
                    };
                    ui.colored_label(c.ink2, RichText::new(status).small());
                    if let Some(t) = &t {
                        if matches!(t.state, "downloading" | "paused" | "waiting" | "verifying") {
                            let frac = if t.size == 0 { 1.0 } else { t.have_bytes as f32 / t.size as f32 };
                            ui.add(egui::ProgressBar::new(frac).desired_width(300.0).desired_height(6.0).fill(if t.state == "paused" { c.ink2 } else { c.accent }));
                        }
                    }
                    ui.horizontal(|ui| match &t {
                        Some(t) => self.transfer_buttons(ui, t),
                        None => {
                            let auto = self.autoplay && a.size > 100_000_000;
                            if auto {
                                self.autoplay = false;
                            }
                            if ui.add(egui::Button::new(RichText::new(format!("⬇  {}", self.t("file_get"))).color(c.on_accent).b()).fill(c.green).rounding(Rounding::same(8.0))).on_hover_text(self.t("file_get_tip")).clicked() || auto {
                                let _ = self.handle.cmd_tx.send(Cmd::Download { hash: a.hash.clone(), name: a.name.clone(), size: a.size, channel: ch.clone() });
                            }
                        }
                    });
                });
            });
        });
    }

    fn downloads_view(&mut self, ui: &mut egui::Ui) {
        let c = th();
        ui.label(RichText::new(self.t("downloads_title")).color(c.ink).b().size(22.0));
        ui.colored_label(c.ink2, RichText::new(self.t("downloads_note").replace("{dir}", &cybercord_node::transfer::download_dir().display().to_string())).small());
        ui.add_space(12.0);
        let list = self.transfers.clone();
        if list.is_empty() {
            ui.colored_label(c.ink2, self.t("downloads_empty"));
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for t in &list {
                egui::Frame::none().fill(c.surface).stroke(Stroke::new(1.0, c.line)).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width().min(760.0));
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&t.name).color(c.ink).b());
                            ui.colored_label(c.ink2, RichText::new(format!("{} / {} · {}", human_size(t.have_bytes), human_size(t.size), self.transfer_status(t))).small());
                            if t.state != "seeding" && t.state != "done" {
                                let frac = if t.size == 0 { 1.0 } else { t.have_bytes as f32 / t.size as f32 };
                                ui.add(egui::ProgressBar::new(frac).desired_width(420.0).desired_height(6.0).fill(if t.state == "paused" { c.ink2 } else { c.accent }));
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.transfer_buttons(ui, t);
                        });
                    });
                });
                ui.add_space(8.0);
            }
        });
        ui.ctx().request_repaint_after(Duration::from_millis(500));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod autostart_tests {
    #[test]
    fn writes_and_removes_the_xdg_entry() {
        let dir = std::env::temp_dir().join(format!("cc-auto-{}", std::process::id()));
        // SAFETY: test-local environment
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        super::set_autostart(true, "Тест").unwrap();
        let f = dir.join("autostart/cybercord.desktop");
        let t = std::fs::read_to_string(&f).unwrap();
        assert!(t.contains("--minimized") && t.contains("--name \"Тест\"") && t.starts_with("[Desktop Entry]"), "{t}");
        super::set_autostart(false, "Тест").unwrap();
        assert!(!f.exists());
    }
}
