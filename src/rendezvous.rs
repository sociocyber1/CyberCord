//! Global rendezvous without servers of our own: BitTorrent's Mainline DHT
//! (millions of ordinary computers) is used as a public meeting point.
//!
//! * Nodes that are reachable from the internet (public IP or a router port
//!   opened by UPnP) announce "CyberCord lives at my-ip:port" under one fixed
//!   key (info-hash).
//! * Every node looks that key up and dials what it gets back — these become
//!   its first entry points; from there peer exchange and Kademlia take over.
//!
//! Nothing is scanned: we only contact addresses that announced themselves.
//! Dials are capped per round, rounds are every 15 minutes.

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The rendezvous key: 20 bytes derived from a fixed CyberCord string.
pub fn info_hash() -> [u8; 20] {
    let h = blake3::hash(b"cybercord/rendezvous/v1");
    let mut out = [0u8; 20];
    out.copy_from_slice(&h.as_bytes()[..20]);
    out
}

/// Shared switches the network engine updates while running.
#[derive(Clone, Default)]
pub struct Control {
    /// Announce ourselves (only when others can reach us).
    pub announce: Arc<AtomicBool>,
    /// Our QUIC (UDP) port to announce.
    pub port: Arc<AtomicU16>,
    /// Ask for an extra lookup round now.
    pub poke: Arc<AtomicBool>,
    /// Stop the thread.
    pub stop: Arc<AtomicBool>,
}

/// Start the rendezvous thread. Found addresses go to `found`.
/// `$CYBERCORD_MAINLINE_BOOTSTRAP` (host:port,…) replaces the default DHT
/// bootstrap nodes — used for local test networks.
pub fn spawn(ctl: Control, found: tokio::sync::mpsc::UnboundedSender<Vec<SocketAddrV4>>) {
    std::thread::Builder::new()
        .name("rendezvous".into())
        .spawn(move || {
            #[allow(deprecated)]
            run(ctl, found)
        })
        .ok();
}

#[allow(deprecated)]
fn run(ctl: Control, found: tokio::sync::mpsc::UnboundedSender<Vec<SocketAddrV4>>) {
    let mut builder = mainline::Dht::builder();
    if let Ok(b) = std::env::var("CYBERCORD_MAINLINE_BOOTSTRAP") {
        let list: Vec<String> = b.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        builder.bootstrap(&list);
        builder.bind_address(std::net::Ipv4Addr::LOCALHOST);
    }
    let dht = match builder.build() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("rendezvous: cannot start the DHT client: {e}");
            return;
        }
    };
    let Ok(key) = mainline::Id::from_bytes(info_hash()) else { return };
    let mut wait_s = 0u64;
    let mut empty_backoff = 10u64;
    let mut announced: (bool, u16) = (false, 0);
    loop {
        if ctl.stop.load(Ordering::Relaxed) {
            return;
        }
        let state = (ctl.announce.load(Ordering::Relaxed), ctl.port.load(Ordering::Relaxed));
        // (Re-)announce as soon as we become reachable or our port changes.
        if state.0 && state.1 != 0 && state != announced {
            if dht.announce_peer(key, Some(state.1)).is_ok() {
                announced = state;
            }
        }
        if wait_s == 0 || ctl.poke.swap(false, Ordering::Relaxed) {
            if state.0 && state.1 != 0 {
                let _ = dht.announce_peer(key, Some(state.1)); // refresh (records expire)
            }
            let mut all: Vec<SocketAddrV4> = Vec::new();
            for batch in dht.get_peers(key) {
                for a in batch {
                    if !all.contains(&a) {
                        all.push(a);
                    }
                }
                if all.len() >= 64 {
                    break;
                }
            }
            if all.is_empty() {
                // Nobody yet: look again soon, then less and less often (≤ 5 min).
                wait_s = empty_backoff;
                empty_backoff = (empty_backoff * 2).min(300);
            } else {
                if found.send(all).is_err() {
                    return; // engine gone
                }
                empty_backoff = 10;
                wait_s = 15 * 60;
            }
        }
        std::thread::sleep(Duration::from_secs(1));
        wait_s = wait_s.saturating_sub(1);
    }
}
