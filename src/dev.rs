//! Developer mode: check voice on one computer.
//!
//! Test nodes are copies of this program started with their own data folder
//! (`<data>/dev/<kind>`), so they have their own identity, and connected
//! straight to this node over 127.0.0.1 (no LAN announce, no global DHT):
//!
//! * echo  — a voice bot that repeats what it hears after 1.5 s: you hear
//!   yourself through the whole path (microphone → network → another node →
//!   back → speakers);
//! * tone  — a bot that plays a short "beep-beep-beeep" in a loop;
//! * music — a bot that plays the intro soundtrack;
//! * window — a second CyberCord window as another user.
//!
//! Bots open no audio device, so the microphone stays free. All test nodes
//! stop when CyberCord quits (bots also stop by themselves: their input pipe
//! closes).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use cybercord_node::store::{self, Settings};

pub const KINDS: [&str; 4] = ["echo", "tone", "music", "window"];

static PROCS: Mutex<Vec<(&'static str, Child)>> = Mutex::new(Vec::new());

/// The data folder of a test node.
pub fn dir(kind: &str) -> PathBuf {
    store::data_dir().join("dev").join(kind)
}

/// Is a test node of this kind running? (Reaps the ones that exited.)
pub fn running(kind: &str) -> bool {
    let Ok(mut p) = PROCS.lock() else { return false };
    p.retain_mut(|(_, c)| matches!(c.try_wait(), Ok(None)));
    p.iter().any(|(k, _)| *k == kind)
}

/// `/ip4/127.0.0.1/tcp/<port>/p2p/<id>` from one of this node's listen addresses.
pub fn loopback_addr(listen: &[String]) -> Option<String> {
    for a in listen {
        let parts: Vec<&str> = a.split('/').collect();
        let tcp = parts.iter().position(|p| *p == "tcp").and_then(|i| parts.get(i + 1));
        let id = parts.iter().position(|p| *p == "p2p").and_then(|i| parts.get(i + 1));
        if let (Some(port), Some(id)) = (tcp, id) {
            return Some(format!("/ip4/127.0.0.1/tcp/{port}/p2p/{id}"));
        }
    }
    None
}

/// Start a test node. `room` is the voice room for bots (guild/channel).
pub fn start(kind: &'static str, name: &str, room: &str, bootstrap: Option<String>, base: &Settings) -> Result<(), String> {
    if running(kind) {
        return Ok(());
    }
    let d = dir(kind);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    // settings for the test node: quiet, private, no intro / tour
    let mut s = base.clone();
    s.intro_seen = true;
    s.splash = false;
    s.tour_done = true;
    s.listed = false;
    s.global = false;
    s.autostart = false;
    s.notify = false;
    s.subs.clear();
    s.dev = false;
    if let Ok(json) = serde_json::to_string_pretty(&s) {
        let _ = std::fs::write(d.join("settings.json"), json);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.env("CYBERCORD_DATA_DIR", &d)
        .env("CYBERCORD_ALLOW_LOOPBACK", "1")
        .env("CYBERCORD_NO_GLOBAL", "1")
        .env_remove("CYBERCORD_START")
        .env_remove("CYBERCORD_DEV")
        .arg("--no-mdns")
        .arg("--name")
        .arg(name);
    if let Some(b) = bootstrap {
        cmd.arg("--bootstrap").arg(b);
    }
    let log = std::fs::File::create(d.join("node.log")).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    if kind == "window" {
        // the window exits when this pipe closes (CyberCord quit or crashed)
        cmd.env("CYBERCORD_START", "notour,nosplash").env("CYBERCORD_DEV_CHILD", "1").stdin(Stdio::piped());
    } else {
        // the bot keeps reading this pipe; when CyberCord exits it closes and the bot stops
        cmd.arg("--headless").arg("--voice-bot").arg(kind).arg("--voice-room").arg(room).arg("--channel").arg(room).stdin(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
    }
    cmd.stdout(Stdio::from(log)).stderr(Stdio::from(log2));
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    if let Ok(mut p) = PROCS.lock() {
        p.push((kind, child));
    }
    Ok(())
}

pub fn stop(kind: &str) {
    if let Ok(mut p) = PROCS.lock() {
        for (k, c) in p.iter_mut() {
            if *k == kind {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        p.retain(|(k, _)| *k != kind);
    }
}

pub fn stop_all() {
    for k in KINDS {
        stop(k);
    }
}
