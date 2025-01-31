use bevy::prelude::*;
use bevy_args::{
    Deserialize,
    Parser,
    Serialize,
};


// TODO: clean this up, add better docs
#[derive(
    Clone,
    Debug,
    Resource,
    Serialize,
    Deserialize,
    Parser,
)]
#[command(about = "bevy_r_place", version, long_about = None)]
pub struct BevyPlaceConfig {
    #[arg(long, default_value = "0.0.0.0", help = "the address to bind to")]
    pub address: String,

    #[arg(long, default_value = "4201", help = "the quic port to bind to")]
    pub quic_port: u16,

    #[arg(long, default_value = "4202", help = "the tcp port to bind to")]
    pub tcp_port: u16,

    #[arg(long, default_value = "4203", help = "the ws port to bind to")]
    pub ws_port: u16,

    #[arg(long, default_value = "/", help = "the ws path to bind to")]
    pub ws_path: String,

    #[arg(long, default_value = "4204", help = "the wss port to bind to")]
    pub wss_port: u16,

    #[arg(long, default_value = "/", help = "the wss path to bind to")]
    pub wss_path: String,

    #[arg(long, default_value = None, help = "the webrtc pem certificate path")]
    pub webrtc_pem_certificate_path: Option<String>,

    #[arg(long, default_value = "4205", help = "the webrtc port to bind to")]
    pub webrtc_port: u16,

    #[arg(long, default_value = "4206", help = "the health check port to bind to")]
    pub health_check_port: u16,

    #[arg(long, default_value = "false", help = "whether or not this node is a bootstrap node")]
    pub bootstrap: bool,

    #[arg(long, default_value = "", help = "singular cli argument of bootstrap_nodes")]
    pub network: String,

    // TODO: de-duplicate defaults across clap and Default impl
    #[arg(
        long = "bootstrap-nodes",
        help = "e.g. /ip4/127.0.0.1/tcp/4201",
        default_values_t = vec![
            "/ip4/127.0.0.1/udp/4201/quic-v1".to_string(),
            "/ip4/127.0.0.1/tcp/4202".to_string(),
            "/ip4/127.0.0.1/tcp/4203/ws".to_string(),
            "/ip4/127.0.0.1/tcp/4204/wss".to_string(),
            "/ip4/127.0.0.1/udp/4205/webrtc-direct/certhash/uEiCMKpbQeJQuNNZSWyljeixDlNYLFllcZDX5LGGwwxTcmQ".to_string(),
        ],
    )]
    pub bootstrap_nodes: Vec<String>,

    #[arg(long, default_value = "false", help = "run headless")]
    pub headless: bool,

    #[arg(long, default_value = "false", help = "short parameter for overriding bootstrap_nodes with mainnet")]
    pub mainnet: bool,

    #[arg(long, default_value = None, help = "store backup snapshots to s3 bucket")]
    pub artifact_s3_bucket: Option<String>,

    #[arg(long, default_value = None, help = "store backup snapshots to disk")]
    pub artifact_folder: Option<String>,

    #[arg(long, default_value = None, help = "snapshot interval in seconds")]
    pub snapshot_interval_seconds: Option<u64>,

    // TODO: support seamless bootstrap node reboots via anchor/pseudo-bootstrap nodes
    #[arg(long, default_value = "false", help = "restore latest snapshot from provided artifact source, for network cold starts")]
    pub restore_latest_snapshot: bool,

    #[arg(long, default_value = None, help = "path to certificate chain file")]
    pub certificate_chain_path: Option<String>,

    #[arg(long, default_value = None, help = "path to private key file")]
    pub private_key_path: Option<String>,
}

impl Default for BevyPlaceConfig {
    fn default() -> Self {
        Self {
            address: "0.0.0.0".to_string(),
            quic_port: 4201,
            tcp_port: 4202,
            ws_port: 4203,
            ws_path: "/".to_string(),
            wss_port: 4204,
            wss_path: "/".to_string(),
            webrtc_pem_certificate_path: None,
            webrtc_port: 4205,
            health_check_port: 4206,
            bootstrap: false,
            network: "".to_string(),
            bootstrap_nodes: vec![
                "/ip4/127.0.0.1/udp/4201/quic-v1".to_string(),
                "/ip4/127.0.0.1/tcp/4202".to_string(),
                "/ip4/127.0.0.1/tcp/4203/ws".to_string(),
                "/ip4/127.0.0.1/tcp/4204/wss".to_string(),
                // note, default won't work without a valid cert hash
                "/ip4/127.0.0.1/udp/4205/webrtc-direct/certhash/uEiCMKpbQeJQuNNZSWyljeixDlNYLFllcZDX5LGGwwxTcmQ".to_string(),
            ],
            headless: false,
            mainnet: false,
            artifact_s3_bucket: None,
            artifact_folder: None,
            snapshot_interval_seconds: None,
            restore_latest_snapshot: false,
            certificate_chain_path: None,
            private_key_path: None,
        }
    }
}
