
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
    error::Error,
};

use async_channel::{unbounded, Sender, Receiver};
use bevy::prelude::*;
use libp2p::{
    futures::StreamExt,
    gossipsub,
    PeerId,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    SwarmBuilder,
    noise,
    yamux,
};
use serde::{Serialize, Deserialize};
use tokio::{
    io,
    select,
    time::sleep,
};

#[cfg(feature = "native")]
use libp2p::{
    mdns,
    tcp,
};

use crate::chunk_crdt::{Pixel, ChunkedCanvas};


#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PixelUpdateMsg {
    pub x: u32,
    pub y: u32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub timestamp: u64,
    pub owner: [u8; 32],
}


#[derive(Clone, Debug)]
pub struct BevyPlaceNodeConfig {
    pub topic: String,
    pub port: u16,
    pub addr: String,
}

impl Default for BevyPlaceNodeConfig {
    fn default() -> Self {
        Self {
            topic: "bevy_r_place_mainnet".to_string(),
            port: 0,
            addr: "".to_string(),
        }
    }
}


#[derive(NetworkBehaviour)]
#[behaviour(out_event = "BevyPlaceBehaviorEvent")]
pub struct BevyPlaceBehavior {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
}

// The enum of possible events our behaviour emits to the swarm.
// The derive macro automatically generates code to unify sub-behaviour events here.
#[allow(clippy::large_enum_variant)]
pub enum BevyPlaceBehaviorEvent {
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
}

impl From<gossipsub::Event> for BevyPlaceBehaviorEvent {
    fn from(ev: gossipsub::Event) -> Self {
        BevyPlaceBehaviorEvent::Gossipsub(ev)
    }
}
impl From<mdns::Event> for BevyPlaceBehaviorEvent {
    fn from(ev: mdns::Event) -> Self {
        BevyPlaceBehaviorEvent::Mdns(ev)
    }
}


#[derive(Resource, Debug)]
pub struct BevyPlaceNodeHandle {
    pub inbound_rx: Receiver<PixelUpdateMsg>,
    pub outbound_tx: Sender<PixelUpdateMsg>,
    pub sub_rx: Receiver<libp2p::PeerId>,
    pub topic: gossipsub::IdentTopic,
}

pub struct BevyPlaceNode {
    pub swarm: Swarm<BevyPlaceBehavior>,
    pub inbound_tx: Sender<PixelUpdateMsg>,
    pub outbound_rx: Receiver<PixelUpdateMsg>,
    pub sub_tx: Sender<libp2p::PeerId>,
    pub topic: gossipsub::IdentTopic,
}

impl BevyPlaceNodeHandle {
    pub async fn wait_for_subscription(
        &self,
        timeout_duration: Duration,
    ) -> Result<(), String> {
        select! {
            received = self.sub_rx.recv() => {
                match received {
                    Ok(_peer_id) => Ok(()),
                    Err(e) => Err(format!("Error receiving from channel: {e}")),
                }
            }
            _ = sleep(timeout_duration) => {
                Err("Timed out waiting for subscription.".to_string())
            }
        }
    }
}


pub async fn run_swarm_task(mut node: BevyPlaceNode) {
    loop {
        select! {
            Ok(msg) = node.outbound_rx.recv() => {
                if let Ok(json) = serde_json::to_vec(&msg) {
                    let _ = node.swarm
                        .behaviour_mut()
                        .gossipsub
                        .publish(node.topic.clone(), json);
                }
            }
            event = node.swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                        gossipsub::Event::Message {
                            propagation_source: _,
                            message_id: _,
                            message,
                        }
                    )) => {
                        if let Ok(update) = serde_json::from_slice::<PixelUpdateMsg>(&message.data) {
                            node.inbound_tx
                                .send(update)
                                .await
                                .expect("Failed to send inbound message");
                        }
                    }
                    SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                        gossipsub::Event::Subscribed { peer_id, topic }
                    )) => {
                        debug!("Peer {peer_id} subscribed to topic {topic}");
                        node.sub_tx
                            .send(peer_id)
                            .await
                            .expect("Failed to send subscription");
                    }
                    SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                        gossipsub::Event::Unsubscribed { peer_id, topic }
                    )) => {
                        debug!("Peer {peer_id} unsubscribed from topic {topic}");
                    }
                    SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                        gossipsub::Event::GossipsubNotSupported { peer_id }
                    )) => {
                        warn!("Peer {peer_id} does not support Gossipsub");
                    }
                    SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                        gossipsub::Event::SlowPeer { peer_id, failed_messages }
                    )) => {
                        debug!("Peer {peer_id} is slow: {failed_messages:?}");
                    }
                    SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Mdns(
                        libp2p::mdns::Event::Discovered(list)
                    )) => {
                        for (peer_id, addr) in list {
                            debug!("mDNS discovered {peer_id} at {addr}");
                            node.swarm
                                .behaviour_mut()
                                .gossipsub
                                .add_explicit_peer(&peer_id);

                            if peer_id != *node.swarm.local_peer_id() {
                                if let Err(e) = node.swarm.dial(addr.clone()) {
                                    info!("Dial error for {peer_id}@{addr}: {e:?}");
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Mdns(
                        libp2p::mdns::Event::Expired(list)
                    )) => {
                        for (peer_id, _addr) in list {
                            debug!("mDNS peer expired {peer_id}");
                            node.swarm
                                .behaviour_mut()
                                .gossipsub
                                .remove_explicit_peer(&peer_id);
                        }
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        debug!("Now listening on {address}");
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        info!("Connected to {peer_id}");
                    }
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        debug!("Disconnected from {peer_id}: {cause:?}");
                    }
                    SwarmEvent::IncomingConnection { .. } => {
                        debug!("Incoming connection");
                    }
                    SwarmEvent::IncomingConnectionError { .. } => {
                        debug!("Incoming connection error");
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        debug!("Outgoing connection error: {:?}: {error}", peer_id);
                    }
                    SwarmEvent::ExpiredListenAddr { address, .. } => {
                        debug!("Expired listen address {address}");
                    }
                    SwarmEvent::ListenerClosed { addresses, .. } => {
                        debug!("Listener closed on {addresses:?}");
                    }
                    SwarmEvent::ListenerError { error, .. } => {
                        debug!("Listener error: {error}");
                    }
                    SwarmEvent::Dialing { peer_id, .. } => {
                        debug!("Dialing {:?}", peer_id);
                    }
                    SwarmEvent::NewExternalAddrCandidate { address } => {
                        debug!("New external address candidate {address}");
                    }
                    SwarmEvent::ExternalAddrConfirmed { address } => {
                        debug!("External address confirmed {address}");
                    }
                    SwarmEvent::ExternalAddrExpired { address } => {
                        debug!("External address expired {address}");
                    }
                    SwarmEvent::NewExternalAddrOfPeer { peer_id, address } => {
                        debug!("New external address of {peer_id}: {address}");
                    }
                    _ => {
                        debug!("unknown swarm event");
                    }
                }
            }
        }
    }
}


pub fn build_node(
    config: BevyPlaceNodeConfig,
) -> Result<(BevyPlaceNode, BevyPlaceNodeHandle), Box<dyn Error>>  {
    let (inbound_tx, inbound_rx) = unbounded::<PixelUpdateMsg>();
    let (outbound_tx, outbound_rx) = unbounded::<PixelUpdateMsg>();
    let (sub_tx, sub_rx) = unbounded::<PeerId>();

    let mut swarm = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| {
            let message_id_fn = |message: &gossipsub::Message| {
                let mut hasher = DefaultHasher::new();
                message.data.hash(&mut hasher);
                gossipsub::MessageId::from(hasher.finish().to_string())
            };

            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(10))
                .validation_mode(gossipsub::ValidationMode::Strict)
                .message_id_fn(message_id_fn)
                .build()
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let mdns = mdns::tokio::Behaviour::new(
                mdns::Config::default(),
                key.public().to_peer_id(),
            )?;

            Ok(BevyPlaceBehavior { gossipsub, mdns })
        })?
        .build();

    // TODO: configurable address
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;

    let topic = gossipsub::IdentTopic::new(config.topic);
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    Ok((
        BevyPlaceNode {
            inbound_tx,
            outbound_rx,
            sub_tx,
            swarm,
            topic: topic.clone(),
        },
        BevyPlaceNodeHandle {
            inbound_rx,
            outbound_tx,
            sub_rx,
            topic,
        },
    ))
}



pub fn inbound_pixel_update_system(
    mut canvas: ResMut<ChunkedCanvas>,
    net: Res<BevyPlaceNodeHandle>,
) {
    while let Ok(update) = net.inbound_rx.try_recv() {
        let pixel = Pixel {
            r: update.r,
            g: update.g,
            b: update.b,
            timestamp: update.timestamp,
            owner: update.owner,
        };

        let updated = canvas.set_pixel(update.x, update.y, pixel);
        if updated {
            println!("Inbound pixel from network at ({},{}). Overwrote with color({},{},{}).",
                update.x, update.y, update.r, update.g, update.b);
        } else {
            println!("Ignored older or out-of-bounds update from network at ({},{}).", update.x, update.y);
        }

        // TODO: manage periodic IPFS chunk storing
    }
}
