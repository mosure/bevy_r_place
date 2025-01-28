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


#[derive(
    Debug,
    Resource,
    Serialize,
    Deserialize,
    Parser,
)]
#[command(about = "bevy_r_place", version, long_about = None)]
pub struct BevyPlaceConfig {
    #[arg(long, default_value = "false", help = "is this node a bootstrap node?")]
    pub bootstrap_node: bool,

    #[arg(
        long = "bootstrap-nodes",
        help = "e.g. /ip4/127.0.0.1/tcp/4001",
        default_values_t = vec![
            "/ip4/127.0.0.1/tcp/4001".to_string(),
        ],
    )]
    pub bootstrap_nodes: Vec<String>,
}

impl Default for BevyPlaceConfig {
    fn default() -> Self {
        Self {
            bootstrap_node: false,
            bootstrap_nodes: vec![
                "/ip4/127.0.0.1/tcp/4001".to_string(),
            ],
        }
    }
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args::<BevyPlaceConfig>();

    let bootstrap_peers = if args.bootstrap_node {
        println!("not connecting to any bootstrap peers!");
        vec![]
    } else {
        // TODO: add proper bootstrap peer
        vec![
            "/ip4/127.0.0.1".parse().unwrap(),
        ]
    };

    let (node, node_handle) = build_node(
        BevyPlaceNodeConfig {
            bootstrap_peers,
            ..default()
        }
    ).await.expect("failed to build node");

    let runtime_handle = tokio::runtime::Handle::current();
    runtime_handle.spawn(run_swarm_task(node));

    let mut app = App::new();

    app.add_plugins(ViewerPlugin);
    app.add_plugins(BevyArgsPlugin::<BevyPlaceConfig>::default());

    app.insert_resource(ChunkedCanvas::new());
    app.insert_resource(node_handle);

    app.add_plugins(SwarmPlugin);

    app.run();

    Ok(())
}
