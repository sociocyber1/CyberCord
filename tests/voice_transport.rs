//! Verifies the voice *network path* without any audio hardware: two peers join
//! a voice topic, one publishes a 20 ms PCM frame, the other receives and decodes
//! it. This is the part that must work regardless of the `voice` feature; the
//! cpal mic/speaker glue sits on top of exactly this transport.

use std::time::Duration;

use cybercord_node::types::{ChannelId, VoiceFrame, Wire};
use cybercord_node::{build_swarm, CyberBehaviour, CyberBehaviourEvent};
use futures::StreamExt;
use libp2p::gossipsub::{self, IdentTopic};
use libp2p::identity::Keypair;
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, Swarm};

async fn first_listen_addr(swarm: &mut Swarm<CyberBehaviour>) -> Multiaddr {
    loop {
        if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
            return address;
        }
    }
}

#[tokio::test]
async fn voice_frame_crosses_the_network() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    let mut alice = build_swarm(Keypair::generate_ed25519()).unwrap();
    let mut bob = build_swarm(Keypair::generate_ed25519()).unwrap();
    let bob_id = *bob.local_peer_id();

    let room = ChannelId::new("sce", "round-table");
    let topic = IdentTopic::new(room.voice_topic());
    alice.behaviour_mut().gossipsub.subscribe(&topic).unwrap();
    bob.behaviour_mut().gossipsub.subscribe(&topic).unwrap();

    alice.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
    let addr = first_listen_addr(&mut alice).await;
    let alice_id = *alice.local_peer_id();
    bob.dial(format!("{addr}/p2p/{alice_id}").parse::<Multiaddr>().unwrap()).unwrap();

    // A 20 ms mono frame (a ramp, so we can check the bytes survived intact).
    let pcm: Vec<i16> = (0..960).map(|i| (i as i16).wrapping_mul(17)).collect();
    let frame = VoiceFrame { room: room.clone(), name: "alice".into(), seq: 7, pcm: pcm.clone() };

    let got = tokio::time::timeout(Duration::from_secs(30), async {
        let mut tick = tokio::time::interval(Duration::from_millis(200));
        let mut connected = false;
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if connected {
                        let _ = alice.behaviour_mut().gossipsub
                            .publish(topic.clone(), Wire::Voice(frame.clone()).encode());
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
                        if let Some(Wire::Voice(vf)) = Wire::decode(&message.data) {
                            return vf;
                        }
                    }
                }
            }
        }
    })
    .await
    .expect("bob received the voice frame");

    assert_eq!(got.seq, 7);
    assert_eq!(got.name, "alice");
    assert_eq!(got.pcm, pcm, "PCM samples must survive the round trip intact");
}
