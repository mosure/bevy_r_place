use std::{
    error::Error,
    net::{
        IpAddr,
        Ipv4Addr,
    },
};

use async_channel::{Sender, Receiver};
use bevy::prelude::*;
use libp2p::{
    gossipsub,
    Multiaddr,
    swarm::Swarm,
};
use serde::{Serialize, Deserialize};

use crate::chunk_crdt::{Pixel, ChunkedCanvas};


pub const PORT_WEBRTC: u16 = 9090;
pub const PORT_QUIC: u16 = 9091;


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
    pub chunk_topic: String,
    pub peer_topic: String,
    pub pixel_topic: String,
    pub quic_port: u16,
    pub tcp_port: u16,
    pub webrtc_port: u16,
    pub addr: IpAddr,
    pub bootstrap_peers: Vec<Multiaddr>,
}

impl Default for BevyPlaceNodeConfig {
    fn default() -> Self {
        Self {
            chunk_topic: "bevy_r_place_mainnet_chunks".to_string(),
            peer_topic: "bevy_r_place_mainnet_peers".to_string(),
            pixel_topic: "bevy_r_place_mainnet_pixels".to_string(),
            quic_port: 0,
            tcp_port: 0,
            webrtc_port: 0,
            addr: Ipv4Addr::UNSPECIFIED.into(),
            bootstrap_peers: vec![],
        }
    }
}


#[derive(Resource, Debug)]
pub struct BevyPlaceNodeHandle {
    pub inbound_rx: Receiver<PixelUpdateMsg>,
    pub outbound_tx: Sender<PixelUpdateMsg>,
    pub sub_rx: Receiver<libp2p::PeerId>,
    pub pixel_topic: gossipsub::IdentTopic,
}

pub struct BevyPlaceNode {
    pub swarm: Swarm<native::BevyPlaceBehavior>,
    pub inbound_tx: Sender<PixelUpdateMsg>,
    pub outbound_rx: Receiver<PixelUpdateMsg>,
    pub sub_tx: Sender<libp2p::PeerId>,
    pub pixel_topic: gossipsub::IdentTopic,
}



#[cfg(feature = "native")]
mod native {
    use std::{
        collections::hash_map::DefaultHasher,
        error::Error,
        hash::{Hash, Hasher},
        time::Duration,
    };

    use async_channel::unbounded;
    use bevy::prelude::*;
    use libp2p::{
        futures::StreamExt,
        gossipsub,
        identify,
        kad,
        mdns,
        memory_connection_limits,
        multiaddr::Protocol,
        Multiaddr,
        PeerId,
        relay,
        StreamProtocol,
        swarm::{NetworkBehaviour, SwarmEvent},
        SwarmBuilder,
        tcp,
        noise,
        yamux,
    };
    // TODO: copy https://github.com/libp2p/universal-connectivity/blob/main/rust-peer/src/main.rs#L311
    use libp2p_webrtc as webrtc;
    use libp2p_webrtc::tokio::Certificate;
    use tokio::{
        io,
        select,
    };

    use super::{
        BevyPlaceNode,
        BevyPlaceNodeConfig,
        BevyPlaceNodeHandle,
        PixelUpdateMsg,
    };


    #[derive(NetworkBehaviour)]
    pub struct BevyPlaceBehavior {
        pub gossipsub: gossipsub::Behaviour,
        pub identify: identify::Behaviour,
        pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
        pub limits: memory_connection_limits::Behaviour,
        pub mdns: mdns::tokio::Behaviour,
        pub relay: relay::Behaviour,
    }

    pub async fn run_swarm_task(mut node: BevyPlaceNode) {
        loop {
            select! {
                Ok(msg) = node.outbound_rx.recv() => {
                    if let Ok(json) = serde_json::to_vec(&msg) {
                        let _ = node.swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(node.pixel_topic.clone(), json);
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
                let local_peer_id = key.public().to_peer_id();

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

                let identify = identify::Behaviour::new(
                    identify::Config::new("/ipfs/0.1.0".into(), key.public())
                        .with_interval(Duration::from_secs(60)),
                );

                let kad_config = kad::Config::new(
                    StreamProtocol::new("/ipfs/kad/1.0.0"),
                );
                let kad_store = kad::store::MemoryStore::new(local_peer_id);
                let kademlia = kad::Behaviour::with_config(
                    local_peer_id,
                    kad_store,
                    kad_config,
                );

                let limits = memory_connection_limits::Behaviour::with_max_percentage(0.9);

                let mdns = mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    key.public().to_peer_id(),
                )?;

                let relay = relay::Behaviour::new(
                    local_peer_id,
                    relay::Config {
                        max_reservations_per_peer: 100,
                        max_circuits_per_peer: 100,
                        ..Default::default()
                    },
                );

                Ok(BevyPlaceBehavior {
                    gossipsub,
                    identify,
                    kademlia,
                    limits,
                    mdns,
                    relay,
                })
            })?
            .build();

        // TODO: configurable address
        let tcp_addr = Multiaddr::from(config.addr)
            .with(Protocol::Tcp(config.tcp_port));
        swarm.listen_on(tcp_addr)?;

        let quic_addr = Multiaddr::from(config.addr)
            .with(Protocol::Udp(config.quic_port))
            .with(Protocol::QuicV1);
        swarm.listen_on(quic_addr)?;

        // TODO: add the webrtc transport - https://github.com/libp2p/universal-connectivity/blob/main/rust-peer/src/main.rs#L347
        // let webrtc_addr = Multiaddr::from(config.addr)
        //     .with(Protocol::Udp(config.webrtc_port))
        //     .with(Protocol::WebRTCDirect);
        // swarm.listen_on(webrtc_addr)?;

        let pixel_topic = gossipsub::IdentTopic::new(config.pixel_topic);
        swarm.behaviour_mut().gossipsub.subscribe(&pixel_topic)?;

        Ok((
            BevyPlaceNode {
                inbound_tx,
                outbound_rx,
                sub_tx,
                swarm,
                pixel_topic: pixel_topic.clone(),
            },
            BevyPlaceNodeHandle {
                inbound_rx,
                outbound_tx,
                sub_rx,
                pixel_topic,
            },
        ))
    }
}



mod web {

}




pub async fn run_swarm_task(node: BevyPlaceNode) {
    #[cfg(feature = "native")]
    native::run_swarm_task(node).await;
}


pub fn build_node(
    config: BevyPlaceNodeConfig,
) -> Result<(BevyPlaceNode, BevyPlaceNodeHandle), Box<dyn Error>>  {
    #[cfg(feature = "native")]
    native::build_node(config)
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
