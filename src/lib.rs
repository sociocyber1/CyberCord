//! CyberCord M1 prototype — the serverless core, as a library so both the
//! `cybercord-node` binary and the integration tests share exactly one network stack.
//!
//! Scope of M1 (see the concept document, §"Roadmap"): identity, QUIC/TCP transport,
//! mDNS + explicit-dial discovery, and gossip chat. There is deliberately **no**
//! address scanning here — peers are learned only from mDNS, an explicit dial, or
//! from other peers (gossip), never by probing strangers.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use libp2p::identity::Keypair;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{gossipsub, identify, mdns, ping, PeerId, Swarm};
use serde::{Deserialize, Serialize};

/// Protocol id advertised over `identify` — this is SCOOP/1 from the concept doc.
pub const PROTOCOL: &str = "/cybercord/scoop/1";

/// The single global gossip topic used by the M1 prototype. A real guild/channel
/// maps to its own topic; one is enough to prove the transport works.
pub const CHAT_TOPIC: &str = "cybercord/global";

/// A chat message as it travels over gossip. The libp2p layer already signs every
/// published message with the sender's key (`MessageAuthenticity::Signed`), so the
/// `PeerId` of the author is authenticated by the network; this struct is just the
/// human payload on top.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Display name the sender chose (not authenticated — cosmetic only).
    pub name: String,
    /// The message body.
    pub body: String,
    /// Unix milliseconds at the sender.
    pub ts_ms: u64,
}

impl ChatMessage {
    pub fn now(name: impl Into<String>, body: impl Into<String>) -> Self {
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self { name: name.into(), body: body.into(), ts_ms }
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("ChatMessage serialises")
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// The node's network behaviour: gossip for chat, mDNS for local discovery,
/// identify to learn peers' addresses/protocols, ping to keep links warm.
#[derive(NetworkBehaviour)]
pub struct CyberBehaviour {
    /// Use other users' nodes as relays when we are behind NAT (Circuit Relay v2 client).
    pub relay_client: libp2p::relay::client::Behaviour,
    /// Every CyberCord node can relay for others — no special servers.
    pub relay: libp2p::relay::Behaviour,
    /// Hole punching: upgrade a relayed connection to a direct one.
    pub dcutr: libp2p::dcutr::Behaviour,
    /// Kademlia DHT: find any user by their ID through other users.
    pub kad: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
    /// Ask the home router to open our port (UPnP) so others can reach us.
    pub upnp: libp2p::upnp::tokio::Behaviour,
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub identify: identify::Behaviour,
    pub ping: ping::Behaviour,
    /// Torrent-like file pieces, node to node (`/cybercord/files/1`).
    pub files: libp2p::request_response::cbor::Behaviour<transfer::FileReq, transfer::FileResp>,
}

/// Kademlia protocol name for the CyberCord DHT (separate from the public IPFS DHT).
pub const KAD_PROTOCOL: &str = "/cybercord/kad/1.0.0";
/// Circuit Relay v2 "hop" protocol — present in identify when a peer can relay.
pub const RELAY_HOP_PROTOCOL: &str = "/libp2p/circuit/relay/0.2.0/hop";

/// Build a fully configured swarm from a keypair. The caller decides what to
/// listen on and what (if anything) to dial — this function only assembles the
/// stack, so the binary and the tests get identical behaviour.
pub fn build_swarm(keypair: Keypair) -> anyhow::Result<Swarm<CyberBehaviour>> {
    let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_quic()
        .with_relay_client(libp2p::noise::Config::new, libp2p::yamux::Config::default)?
        .with_behaviour(|key, relay_client| {
            let pid = key.public().to_peer_id();
            // Deduplicate messages by content hash so a message that arrives over
            // two paths is shown once.
            let message_id_fn = |message: &gossipsub::Message| {
                let mut hasher = DefaultHasher::new();
                message.data.hash(&mut hasher);
                gossipsub::MessageId::from(hasher.finish().to_string())
            };

            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(1))
                .validation_mode(gossipsub::ValidationMode::Strict)
                // Media (GIFs, images, video circles) travels as chunks of up to ~256 KB.
                .max_transmit_size(1024 * 1024)
                .message_id_fn(message_id_fn)
                .build()
                .map_err(|e| anyhow::anyhow!("gossipsub config: {e}"))?;

            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )
            .map_err(|e| anyhow::anyhow!("gossipsub init: {e}"))?;

            let mdns = mdns::tokio::Behaviour::new(
                mdns::Config::default(),
                key.public().to_peer_id(),
            )?;

            let identify = identify::Behaviour::new(
                identify::Config::new(PROTOCOL.to_string(), key.public())
                    .with_push_listen_addr_updates(true),
            );

            let ping = ping::Behaviour::new(ping::Config::new());

            // Relay limits: generous enough to carry chat/voice until hole punching
            // upgrades the link to a direct one.
            let relay = libp2p::relay::Behaviour::new(
                pid,
                libp2p::relay::Config {
                    max_circuit_duration: Duration::from_secs(30 * 60),
                    max_circuit_bytes: 64 * 1024 * 1024,
                    ..Default::default()
                },
            );
            let dcutr = libp2p::dcutr::Behaviour::new(pid);

            let mut kcfg = libp2p::kad::Config::new(libp2p::StreamProtocol::new(KAD_PROTOCOL));
            kcfg.set_query_timeout(Duration::from_secs(30));
            let mut kad = libp2p::kad::Behaviour::with_config(pid, libp2p::kad::store::MemoryStore::new(pid), kcfg);
            kad.set_mode(Some(libp2p::kad::Mode::Server));

            let upnp = libp2p::upnp::tokio::Behaviour::default();

            let files = libp2p::request_response::cbor::Behaviour::new(
                [(libp2p::StreamProtocol::new("/cybercord/files/1"), libp2p::request_response::ProtocolSupport::Full)],
                libp2p::request_response::Config::default().with_request_timeout(Duration::from_secs(30)).with_max_concurrent_streams(128),
            );
            Ok(CyberBehaviour { relay_client, relay, dcutr, kad, upnp, gossipsub, mdns, identify, ping, files })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(120)))
        .build();

    Ok(swarm)
}

/// Subscribe a freshly built swarm to the global chat topic.
pub fn subscribe_chat(swarm: &mut Swarm<CyberBehaviour>) -> anyhow::Result<gossipsub::IdentTopic> {
    let topic = gossipsub::IdentTopic::new(CHAT_TOPIC);
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
    Ok(topic)
}

/// Short, human-friendly form of a peer id for log lines.
pub fn short_peer(peer: &PeerId) -> String {
    let s = peer.to_base58();
    let n = s.len();
    if n <= 10 {
        s
    } else {
        format!("{}…{}", &s[..6], &s[n - 4..])
    }
}

// ---- CyberCord v0.2 modules (GUI desktop app on top of the M1 core) ----
pub mod types;
pub mod servers;
pub mod store;
pub mod net;
pub mod voice;
pub mod rendezvous;
pub mod transfer;
pub mod words;
