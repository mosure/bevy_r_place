use std::error::Error;

use bevy::prelude::*;
use bevy_args::{
    parse_args,
    BevyArgsPlugin,
    Deserialize,
    Parser,
    Serialize,
};
use bevy_r_place::prelude::*;


// TODO: clean this up, add better docs
#[derive(
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

    #[arg(long, default_value = "false", help = "whether or not this node is a bootstrap node")]
    pub bootstrap: bool,

    #[arg(long, default_value = "", help = "singular cli argument of bootstrap_nodes")]
    pub network: String,

    #[arg(
        long = "bootstrap-nodes",
        help = "e.g. /ip4/127.0.0.1/tcp/4201",
        default_values_t = vec![
            "/ip4/127.0.0.1/udp/4201/quic-v1".to_string(),
            "/ip4/127.0.0.1/tcp/4202".to_string(),
            "/ip4/127.0.0.1/tcp/4203/ws".to_string(),
            "/ip4/127.0.0.1/tcp/4204/wss".to_string(),
        ],
    )]
    pub bootstrap_nodes: Vec<String>,

    #[arg(long, default_value = "false", help = "run headless")]
    pub headless: bool,

    #[arg(long, default_value = "false", help = "short parameter for overriding bootstrap_nodes with mainnet")]
    pub mainnet: bool,

    #[arg(long, default_value = "false", help = "serialize pixel updates to disk")]
    pub artifact_s3_bucket: Option<String>,

    #[arg(long, default_value = "false", help = "path to certificate chain file")]
    pub certificate_chain_path: Option<String>,

    #[arg(long, default_value = "false", help = "path to private key file")]
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
            bootstrap: false,
            network: "".to_string(),
            bootstrap_nodes: vec![
                "/ip4/127.0.0.1/udp/4201/quic-v1".to_string(),
                "/ip4/127.0.0.1/tcp/4202".to_string(),
                "/ip4/127.0.0.1/tcp/4203/ws".to_string(),
                "/ip4/127.0.0.1/tcp/4204/wss".to_string(),
            ],
            headless: false,
            mainnet: false,
            artifact_s3_bucket: None,
            certificate_chain_path: None,
            private_key_path: None,
        }
    }
}


async fn run_app_async() -> Result<(), Box<dyn Error>> {
    let args = parse_args::<BevyPlaceConfig>();
    log(&format!("args: {:?}", args));

    let bootstrap_peers = if args.bootstrap {
        log("not connecting to any bootstrap peers!");
        vec![]
    } else {
        let mut bootstrap_nodes = args.bootstrap_nodes
            .iter()
            .map(|node| node.parse().expect("failed to parse bootstrap node"))
            .collect::<Vec<_>>();

        if !args.network.is_empty() {
            bootstrap_nodes.push(args.network.parse().expect("failed to parse bootstrap node"));
        }

        if args.mainnet {
            bootstrap_nodes = vec![
                "/dns4/bevy_r_place.mosure.dev/udp/4201/quic-v1".parse()?,
                "/dns4/bevy_r_place.mosure.dev/tcp/4202".parse()?,
                "/dns4/bevy_r_place.mosure.dev/tcp/4203/ws".parse()?,
                "/dns4/bevy_r_place.mosure.dev/tcp/4204/wss".parse()?,
            ]
        }

        bootstrap_nodes
    };

    let (node, node_handle) = build_node(
        BevyPlaceNodeConfig {
            addr: args.address.parse()?,
            bootstrap_peers,
            quic_port: args.quic_port,
            tcp_port: args.tcp_port,
            ws_port: args.ws_port,
            ws_path: args.ws_path,
            ..default()
        }
    ).await.expect("failed to build node");

    #[cfg(feature = "native")]
    {
        let runtime_handle = tokio::runtime::Handle::current();
        runtime_handle.spawn(run_swarm_task(node));
    }

    #[cfg(feature = "web")]
    {
        wasm_bindgen_futures::spawn_local(async {
            run_swarm_task(node).await;
            log("swarm task finished :c");
        });
    }

    let mut app = App::new();

    let headless = args.headless;

    #[cfg(not(feature = "viewer"))]
    let headless = headless || true;

    if headless {
        app.add_plugins(HeadlessPlugin);
    } else {
        #[cfg(feature = "viewer")]
        app.add_plugins(ViewerPlugin);
    }

    app.add_plugins(BevyArgsPlugin::<BevyPlaceConfig>::default());

    app.insert_resource(ChunkedCanvas::new());
    app.insert_resource(node_handle);

    app.add_plugins(SwarmPlugin);

    app.run();

    Ok(())
}


#[cfg(feature = "native")]
fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(run_app_async()).expect("failed to run app");
}

#[cfg(feature = "web")]
fn main() {
    #[cfg(debug_assertions)]
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
    }

    wasm_bindgen_futures::spawn_local(async {
        run_app_async().await.expect("failed to run app");
    });
}


pub fn log(_msg: &str) {
    #[cfg(debug_assertions)]
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::console::log_1(&_msg.into());
    }
    #[cfg(debug_assertions)]
    #[cfg(not(target_arch = "wasm32"))]
    {
        println!("{}", _msg);
    }
}
