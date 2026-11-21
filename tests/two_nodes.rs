//! Automated test: two peers in one process, over loopback TCP, deliver a chat
//! message with no mDNS, no servers and no human. This is the "test without people"
//! answer in its purest form — `cargo test` is the whole network.

use std::time::Duration;

use cybercord_node::{build_swarm, subscribe_chat, ChatMessage, CyberBehaviourEvent};
use futures::StreamExt;
use libp2p::identity::Keypair;
use libp2p::{gossipsub, Multiaddr, Swarm};
use libp2p::swarm::SwarmEvent;
use cybercord_node::CyberBehaviour;

/// Drive a swarm until it reports its first listen address.
async fn first_listen_addr(swarm: &mut Swarm<CyberBehaviour>) -> Multiaddr {
    loop {
        if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
            return address;
        }
    }
}

#[tokio::test]
async fn two_peers_exchange_a_message() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    // Two independent identities / nodes.
    let mut alice = build_swarm(Keypair::generate_ed25519()).expect("build alice");
    let mut bob = build_swarm(Keypair::generate_ed25519()).expect("build bob");
    let bob_id = *bob.local_peer_id();

    let topic = subscribe_chat(&mut alice).expect("alice subscribe");
    let _ = subscribe_chat(&mut bob).expect("bob subscribe");

    // Alice listens on a loopback TCP port the OS picks.
    alice.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
    let alice_addr = first_listen_addr(&mut alice).await;
    let alice_id = *alice.local_peer_id();
    let dial_addr: Multiaddr = format!("{alice_addr}/p2p/{alice_id}").parse().unwrap();

    // Bob dials Alice.
    bob.dial(dial_addr).expect("bob dials alice");

    // Now drive both swarms until Bob receives Alice's message. Alice re-publishes
    // on a timer because gossip only delivers once Bob's subscription is known to
    // her (a few round-trips after the connection opens).
    let outcome = tokio::time::timeout(Duration::from_secs(30), async {
        let mut publish_tick = tokio::time::interval(Duration::from_millis(250));
        let mut connected = false;
        loop {
            tokio::select! {
                _ = publish_tick.tick() => {
                    if connected {
                        let msg = ChatMessage::now("alice", "hello from the test");
                        // Ignore InsufficientPeers; we simply retry next tick.
                        let _ = alice.behaviour_mut().gossipsub.publish(topic.clone(), msg.encode());
                    }
                }
                ev = alice.select_next_some() => {
                    if let SwarmEvent::ConnectionEstablished { peer_id, .. } = ev {
                        if peer_id == bob_id { connected = true; }
                    }
                }
                ev = bob.select_next_some() => {
                    if let SwarmEvent::Behaviour(CyberBehaviourEvent::Gossipsub(
                        gossipsub::Event::Message { message, .. })) = ev
                    {
                        if let Some(m) = ChatMessage::decode(&message.data) {
                            return (m.name, m.body);
                        }
                    }
                }
            }
        }
    })
    .await;

    match outcome {
        Ok((name, body)) => {
            assert_eq!(name, "alice");
            assert_eq!(body, "hello from the test");
            // Sanity: the ids really are two distinct peers.
            assert_ne!(alice_id, bob_id);
        }
        Err(_) => panic!("bob did not receive alice's message within 30s"),
    }
}
