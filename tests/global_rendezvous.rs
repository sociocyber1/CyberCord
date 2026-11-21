//! Two CyberCord nodes that know nothing about each other — no addresses, no
//! codes, no LAN discovery — find each other through the BitTorrent Mainline
//! DHT used as a public meeting point. Here the DHT is a local test network of
//! 10 nodes; in real use it is the worldwide BitTorrent DHT.

use std::time::{Duration, Instant};

use cybercord_node::net::{self, NetConfig, UiEvent};

#[test]
fn nodes_meet_through_the_mainline_dht() {
    let testnet = mainline::Testnet::builder(10).build().expect("local DHT testnet");
    let dir = std::env::temp_dir().join(format!("cc-rv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: set before any other thread of this test reads the environment.
    unsafe {
        std::env::set_var("CYBERCORD_MAINLINE_BOOTSTRAP", testnet.bootstrap.join(","));
        std::env::set_var("CYBERCORD_ALLOW_LOOPBACK", "1");
        std::env::set_var("CYBERCORD_DATA_DIR", &dir);
    }
    let node = |name: &str, port: u16| {
        net::spawn(NetConfig {
            name: name.into(),
            identity: dir.join(format!("{name}.key")),
            bootstrap: Vec::new(), // nothing: no addresses, no codes
            no_mdns: true,         // and no local discovery
            tcp_port: port,
            quic_port: port,
            listed: true,
            global: true,
            scan: true,
            status: String::new(),
        })
    };
    let a = node("ann", 47811);
    let b = node("ben", 47812);

    let mut ids = (String::new(), String::new());
    let start = Instant::now();
    let mut met = false;
    while start.elapsed() < Duration::from_secs(90) && !met {
        for (h, mine) in [(&a, 0), (&b, 1)] {
            while let Ok(ev) = h.event_rx.try_recv() {
                match ev {
                    UiEvent::Ready { peer_id, .. } => {
                        if mine == 0 { ids.0 = peer_id } else { ids.1 = peer_id }
                    }
                    UiEvent::Peer { id, connected: true, .. } => {
                        let other = if mine == 0 { &ids.1 } else { &ids.0 };
                        if !other.is_empty() && &id == other {
                            met = true;
                        }
                    }
                    UiEvent::Status(s) => eprintln!("[{mine}] {s}"),
                    _ => {}
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = a.cmd_tx.send(net::Cmd::Quit);
    let _ = b.cmd_tx.send(net::Cmd::Quit);
    assert!(met, "the two nodes did not find each other through the DHT within 90 s");
    eprintln!("met through the DHT in {:.1} s", start.elapsed().as_secs_f32());
}
