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
    StreamProtocol,
    swarm::Swarm,
    request_response::ResponseChannel,
};
use serde::{Serialize, Deserialize};

use crate::chunk_crdt::{
    Pixel,
    CanvasResponse,
    ChunkedCanvas,
};


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
    pub external_addr: Option<IpAddr>,
    pub bootstrap_peers: Vec<Multiaddr>,
    pub kademlia_protocol: StreamProtocol,
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
            external_addr: None,
            bootstrap_peers: vec![],
            kademlia_protocol: StreamProtocol::new("/ipfs/kad/1.0.0"),
        }
    }
}


#[derive(Resource, Debug)]
pub struct BevyPlaceNodeHandle {
    pub inbound_rx: Receiver<PixelUpdateMsg>,
    pub outbound_tx: Sender<PixelUpdateMsg>,
    pub inbound_canvas_rx: Receiver<ChunkedCanvas>,
    pub canvas_request_rx: Receiver<ResponseChannel<CanvasResponse>>,
    pub canvas_response_tx: Sender<(ResponseChannel<CanvasResponse>, ChunkedCanvas)>,
    pub sub_rx: Receiver<libp2p::PeerId>,
    pub chunk_topic: gossipsub::IdentTopic,
    pub peer_topic: gossipsub::IdentTopic,
    pub pixel_topic: gossipsub::IdentTopic,
}

pub struct BevyPlaceNode {
    pub swarm: Swarm<native::BevyPlaceBehavior>,
    pub inbound_tx: Sender<PixelUpdateMsg>,
    pub outbound_rx: Receiver<PixelUpdateMsg>,
    pub inbound_canvas_tx: Sender<ChunkedCanvas>,
    pub canvas_response_rx: Receiver<(ResponseChannel<CanvasResponse>, ChunkedCanvas)>,
    pub canvas_request_tx: Sender<ResponseChannel<CanvasResponse>>,
    pub sub_tx: Sender<libp2p::PeerId>,
    pub chunk_topic: gossipsub::IdentTopic,
    pub peer_topic: gossipsub::IdentTopic,
    pub pixel_topic: gossipsub::IdentTopic,
    pub config: BevyPlaceNodeConfig,
}



#[cfg(feature = "native")]
mod native {
    use std::{
        collections::hash_map::DefaultHasher,
        error::Error,
        hash::{Hash, Hasher},
        time::{Duration, SystemTime, UNIX_EPOCH},
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
        request_response,
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

    use crate::chunk_crdt::{
        CanvasRequest,
        CanvasResponse,
        ChunkedCanvas,
        codec::Codec,
        CHUNK_SIZE,
        WORLD_HEIGHT,
        WORLD_WIDTH,
    };
    use super::{
        BevyPlaceNode,
        BevyPlaceNodeConfig,
        BevyPlaceNodeHandle,
        Pixel,
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
        pub request_response: request_response::Behaviour<Codec<CanvasRequest, CanvasResponse>>,
    }

    fn current_timestamp_as_vec() -> Vec<u8> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let secs = now.as_secs();
        secs.to_be_bytes().to_vec()
    }

    pub fn build_node(
        config: BevyPlaceNodeConfig,
    ) -> Result<(BevyPlaceNode, BevyPlaceNodeHandle), Box<dyn Error>>  {
        let (inbound_tx, inbound_rx) = unbounded::<PixelUpdateMsg>();
        let (outbound_tx, outbound_rx) = unbounded::<PixelUpdateMsg>();
        let (sub_tx, sub_rx) = unbounded::<PeerId>();

        let (inbound_canvas_tx, inbound_canvas_rx) = unbounded::<ChunkedCanvas>();
        let (canvas_request_tx, canvas_request_rx) = unbounded::<request_response::ResponseChannel<CanvasResponse>>();
        let (canvas_response_tx, canvas_response_rx) = unbounded::<(request_response::ResponseChannel<CanvasResponse>, ChunkedCanvas)>();

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

                let kad_config = kad::Config::new(config.kademlia_protocol.clone());
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

                // TODO: only request visible chunks (lowering max response size requirement)
                // TODO: use a custom codec copy for now, until https://github.com/libp2p/rust-libp2p/pull/5830 releases
                let max_response_size = (std::mem::size_of::<Pixel>() * WORLD_WIDTH as usize * WORLD_HEIGHT as usize
                    + (WORLD_WIDTH / CHUNK_SIZE * WORLD_HEIGHT / CHUNK_SIZE * 8) as usize) * 2;
                println!("expected max response size: {}", max_response_size);

                let canvas_codec = Codec::<CanvasRequest, CanvasResponse>::default()
                    .set_response_size_maximum(max_response_size as u64);
                let request_response = request_response::Behaviour::with_codec(
                    canvas_codec,
                    std::iter::once((
                        StreamProtocol::new("/bevy-r-place-chunks/1"),
                        request_response::ProtocolSupport::Full,
                    )),
                    request_response::Config::default().with_request_timeout(Duration::from_secs(30)),
                );

                Ok(BevyPlaceBehavior {
                    gossipsub,
                    identify,
                    kademlia,
                    limits,
                    mdns,
                    relay,
                    request_response,
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

        // let transport = {
        //     let webrtc = webrtc::tokio::Transport::new(local_key.clone(), certificate);
        //     let quic = quic::tokio::Transport::new(quic::Config::new(&local_key));

        //     let mapped = webrtc.or_transport(quic).map(|fut, _| match fut {
        //         Either::Right((local_peer_id, conn)) => (local_peer_id, StreamMuxerBox::new(conn)),
        //         Either::Left((local_peer_id, conn)) => (local_peer_id, StreamMuxerBox::new(conn)),
        //     });

        //     dns::TokioDnsConfig::system(mapped)?.boxed()
        // };

        let chunk_topic = gossipsub::IdentTopic::new(&config.chunk_topic);
        swarm.behaviour_mut().gossipsub.subscribe(&chunk_topic)?;

        let peer_topic = gossipsub::IdentTopic::new(&config.peer_topic);
        swarm.behaviour_mut().gossipsub.subscribe(&peer_topic)?;

        let pixel_topic = gossipsub::IdentTopic::new(&config.pixel_topic);
        swarm.behaviour_mut().gossipsub.subscribe(&pixel_topic)?;


        Ok((
            BevyPlaceNode {
                inbound_tx,
                outbound_rx,
                canvas_request_tx,
                canvas_response_rx,
                inbound_canvas_tx,
                sub_tx,
                swarm,
                chunk_topic: chunk_topic.clone(),
                peer_topic: peer_topic.clone(),
                pixel_topic: pixel_topic.clone(),
                config,
            },
            BevyPlaceNodeHandle {
                inbound_rx,
                outbound_tx,
                canvas_request_rx,
                canvas_response_tx,
                inbound_canvas_rx,
                sub_rx,
                chunk_topic,
                peer_topic,
                pixel_topic,
            },
        ))
    }

    pub async fn run_swarm_task(mut node: BevyPlaceNode) {
        let chunk_hash = node.chunk_topic.hash();
        let peer_hash = node.peer_topic.hash();
        let pixel_hash = node.pixel_topic.hash();

        let mut initialized = node.config.bootstrap_peers.is_empty();

        loop {
            select! {
                Ok(msg) = node.outbound_rx.recv() => {
                    if let Ok(json) = serde_json::to_vec(&msg) {
                        let publish = node.swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(node.pixel_topic.clone(), json);

                        match publish {
                            Ok(_) => {},
                            Err(e) => error!("failed to publish pixel update: {e:?}"),
                        }
                    }
                }
                Ok((channel, canvas)) = node.canvas_response_rx.recv() => {
                    let publish = node.swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(
                            channel,
                            CanvasResponse {
                                canvas,
                            },
                        );

                    match publish {
                        Ok(_) => info!("sent canvas history to peer"),
                        Err(_) => error!("failed to send canvas history"),
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
                            if message.topic == pixel_hash {
                                if let Ok(update) = serde_json::from_slice::<PixelUpdateMsg>(&message.data) {
                                    node.inbound_tx
                                        .send(update)
                                        .await
                                        .expect("failed to send inbound message");
                                }
                            } else if message.topic == chunk_hash {
                                info!("peer {:?} has canvas history", message.source);

                                if !initialized {
                                    let request_id = node.swarm
                                        .behaviour_mut()
                                        .request_response
                                        .send_request(
                                            &message.source.unwrap(),
                                            CanvasRequest::default(),
                                        );

                                    info!("sent canvas history request: {request_id:?}");
                                }
                            } else if message.topic == peer_hash {
                                info!("received peer message...");
                            } else {
                                warn!("unknown topic: {}", message.topic);
                            }
                        }
                        SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                            gossipsub::Event::Subscribed { peer_id, topic }
                        )) => {
                            debug!("peer {peer_id} subscribed to topic {topic}");
                            node.sub_tx
                                .send(peer_id)
                                .await
                                .expect("failed to send subscription");

                            if initialized && topic == chunk_hash {
                                info!("notifying peer {peer_id} that canvas history is available");
                                node.swarm
                                    .behaviour_mut()
                                    .gossipsub
                                    .publish(node.chunk_topic.clone(), current_timestamp_as_vec())
                                    .expect("failed to notify initialized canvas is available");
                            }
                        }
                        SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                            gossipsub::Event::Unsubscribed { peer_id, topic }
                        )) => {
                            debug!("peer {peer_id} unsubscribed from topic {topic}");
                        }
                        SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                            gossipsub::Event::GossipsubNotSupported { peer_id }
                        )) => {
                            warn!("peer {peer_id} does not support Gossipsub");
                        }
                        SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Gossipsub(
                            gossipsub::Event::SlowPeer { peer_id, failed_messages }
                        )) => {
                            debug!("peer {peer_id} is slow: {failed_messages:?}");
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
                                        info!("dial error for {peer_id}@{addr}: {e:?}");
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
                            if let Some(external_ip) = node.config.external_addr {
                                let external_address = address
                                    .replace(0, |_| Some(external_ip.into()))
                                    .expect("address.len > 1 and we always return `Some`");

                                node.swarm.add_external_address(external_address);
                            }

                            let p2p_address = address.with(Protocol::P2p(*node.swarm.local_peer_id()));
                            info!("listening on {p2p_address}");
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            info!("connected to {peer_id}");
                        }
                        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                            debug!("disconnected from {peer_id}: {cause:?}");
                        }
                        SwarmEvent::IncomingConnection { .. } => {
                            debug!("incoming connection");
                        }
                        SwarmEvent::IncomingConnectionError { .. } => {
                            debug!("incoming connection error");
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                            debug!("outgoing connection error: {:?}: {error}", peer_id);
                        }
                        SwarmEvent::ExpiredListenAddr { address, .. } => {
                            debug!("expired listen address {address}");
                        }
                        SwarmEvent::ListenerClosed { addresses, .. } => {
                            debug!("listener closed on {addresses:?}");
                        }
                        SwarmEvent::ListenerError { error, .. } => {
                            debug!("listener error: {error}");
                        }
                        SwarmEvent::Dialing { peer_id, .. } => {
                            debug!("dialing {:?}", peer_id);
                        }
                        SwarmEvent::NewExternalAddrCandidate { address } => {
                            debug!("new external address candidate {address}");
                        }
                        SwarmEvent::ExternalAddrConfirmed { address } => {
                            debug!("external address confirmed {address}");
                        }
                        SwarmEvent::ExternalAddrExpired { address } => {
                            debug!("external address expired {address}");
                        }
                        SwarmEvent::NewExternalAddrOfPeer { peer_id, address } => {
                            debug!("new external address of {peer_id}: {address}");
                        }
                        SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::Identify(e)) => {
                            debug!("BevyPlaceBehaviorEvent::Identify {:?}", e);

                            if let identify::Event::Error { peer_id, error, connection_id } = e {
                                match error {
                                    libp2p::swarm::StreamUpgradeError::Timeout => {
                                        // When a browser tab closes, we don't get a swarm event
                                        // maybe there's a way to get this with TransportEvent
                                        // but for now remove the peer from routing table if there's an Identify timeout
                                        node.swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
                                        info!("removed {peer_id} from the routing table, connection id: {connection_id}.");
                                    }
                                    _ => {
                                        debug!("{error}");
                                    }
                                }
                            } else if let identify::Event::Received {
                                peer_id: _,
                                info: identify::Info {
                                    listen_addrs: _,
                                    protocols: _,
                                    observed_addr,
                                    ..
                                },
                                connection_id: _,
                            } = e
                            {
                                debug!("identify::Event::Received observed_addr: {}", observed_addr);
                            }
                        }
                        SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::RequestResponse(
                            request_response::Event::Message { message, .. },
                        )) => match message {
                            request_response::Message::Request { channel, .. } => {
                                info!("received canvas history request");
                                if initialized {
                                    node.canvas_request_tx
                                        .send(channel)
                                        .await
                                        .expect("failed to send canvas request");
                                } else {
                                    warn!("received canvas history request but the canvas is not initialized");
                                }
                            }
                            request_response::Message::Response { response, .. } => {
                                info!("received canvas history");
                                node.inbound_canvas_tx
                                    .send(response.canvas)
                                    .await
                                    .expect("failed to send canvas response");

                                initialized = true;
                            }
                        },
                        SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::RequestResponse(
                            request_response::Event::OutboundFailure {
                                peer, connection_id, request_id, error
                            },
                        )) => {
                            error!(
                                "request_response::Event::OutboundFailure: {:?}, {:?}, {:?}, {:?}",
                                peer, connection_id, request_id, error
                            );
                        }
                        SwarmEvent::Behaviour(BevyPlaceBehaviorEvent::RequestResponse(
                            request_response::Event::InboundFailure {
                                peer, connection_id, request_id, error
                            },
                        )) => {
                            error!(
                                "request_response::Event::InboundFailure: {:?}, {:?}, {:?}, {:?}",
                                peer, connection_id, request_id, error
                            );
                        }
                        _ => {
                            debug!("unknown swarm event");
                        }
                    }
                }
            }
        }
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



fn inbound_canvas_system(
    mut world_canvas: ResMut<ChunkedCanvas>,
    net: Res<BevyPlaceNodeHandle>,
) {
    while let Ok(canvas) = net.inbound_canvas_rx.try_recv() {
        *world_canvas = canvas;
    }
}


fn inbound_canvas_request_system(
    canvas: Res<ChunkedCanvas>,
    net: Res<BevyPlaceNodeHandle>,
) {
    while let Ok(channel) = net.canvas_request_rx.try_recv() {
        net.canvas_response_tx
            .send_blocking((channel, canvas.clone()))
            .ok();
    }
}


fn inbound_pixel_update_system(
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
            info!(
                "inbound pixel from network at ({},{}), with color({},{},{}).",
                update.x, update.y, update.r, update.g, update.b
            );
        } else {
            info!("ignored older or out-of-bounds update from network at ({},{}).", update.x, update.y);
        }

        // TODO: manage periodic chunk storing
    }
}


#[derive(Default)]
pub struct SwarmPlugin;
impl Plugin for SwarmPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_systems(
                Update,
                (
                    inbound_canvas_system,
                    inbound_canvas_request_system,
                    inbound_pixel_update_system,
                ),
            );
    }
}
