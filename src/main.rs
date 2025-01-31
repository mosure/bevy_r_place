use std::error::Error;

use bevy::prelude::*;
use bevy_args::{
    parse_args,
    BevyArgsPlugin,
};
use bevy_r_place::prelude::*;



async fn run_app_async() -> Result<(), Box<dyn Error>> {
    let args = parse_args::<BevyPlaceConfig>();
    log(&format!("args: {:?}", args));

    #[cfg(feature = "aws")]
    {
        let runtime_handle = tokio::runtime::Handle::current();
        runtime_handle.spawn(aws::http_health_check(args.clone()));
    }

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
                "/dns4/bevy-r-place.mosure.dev/tcp/4203/ws".parse()?,
                "/dns4/bevy-r-place.mosure.dev/tcp/4204/wss".parse()?,
                "/dns4/raw.bevy-r-place.mosure.dev/udp/4201/quic-v1".parse()?,
                "/dns4/raw.bevy-r-place.mosure.dev/tcp/4202".parse()?,
                "/dns4/raw.bevy-r-place.mosure.dev/udp/4205/webrtc-direct/certhash/uEiCMKpbQeJQuNNZSWyljeixDlNYLFllcZDX5LGGwwxTcmQ".parse()?,
            ]
        }

        bootstrap_nodes
    };

    let certificate = if let Some(path) = args.certificate_chain_path.as_ref() {
        std::fs::read(path)?.into()
    } else {
        None
    };

    let private_key = if let Some(path) = args.private_key_path.as_ref() {
        std::fs::read(path)?.into()
    } else {
        None
    };

    #[cfg(feature = "aws")]
    let webrtc_pem_certificate = aws::webrtc_pem_certificate().await;
    #[cfg(not(feature = "aws"))]
    let webrtc_pem_certificate = None;

    let (node, node_handle) = build_node(
        BevyPlaceNodeConfig {
            addr: args.address.parse()?,
            bootstrap_peers,
            quic_port: args.quic_port,
            tcp_port: args.tcp_port,
            ws_port: args.ws_port,
            ws_path: args.ws_path,
            wss_port: args.wss_port,
            wss_path: args.wss_path,
            certificate,
            private_key,
            webrtc_port: args.webrtc_port,
            webrtc_pem_certificate,
            webrtc_pem_certificate_path: args.webrtc_pem_certificate_path,
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

    #[cfg(feature = "native")]
    app.add_plugins(SnapshotPlugin {
        artifact_directory: args.artifact_directory.clone().map(std::path::PathBuf::from),
        pixel_artifact_stream_chunk_mb: args.pixel_artifact_stream_chunk_mb,
        snapshot_interval: args.snapshot_interval_seconds.map(std::time::Duration::from_secs),
    });

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



#[cfg(feature = "aws")]
pub mod aws {
    use aws_config::{self, BehaviorVersion, Region};
    use aws_sdk_secretsmanager;
    use axum::{
        routing::get,
        http::StatusCode,
        Router,
    };

    pub async fn webrtc_pem_certificate() -> Option<String> {
        let secret_name = "/bevy_r_place/certs/webrtc_pem";
        let region = Region::new("us-west-2");

        let config = aws_config::defaults(BehaviorVersion::v2024_03_28())
            .region(region)
            .load()
            .await;

        let asm = aws_sdk_secretsmanager::Client::new(&config);

        let response = asm
            .get_secret_value()
            .secret_id(secret_name)
            .send()
            .await;

        if let Err(err) = response {
            super::log(&format!("failed to get secret: {:?}", err));
            None
        } else {
            response.unwrap().secret_string().map(String::from)
        }
    }

    pub async fn http_health_check(
        config: super::BevyPlaceConfig,
    ) {
        super::log(&format!("starting health check server on {}:{}", config.address, config.health_check_port));

        let app = Router::new()
            .route("/health", get(|| async {
                (StatusCode::OK, "ok")
            }));

        let addr = format!("{}:{}", config.address, config.health_check_port);
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    }
}
