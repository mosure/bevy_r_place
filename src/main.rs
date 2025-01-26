use std::error::Error;

use bevy::prelude::*;
use bevy_r_place::prelude::*;


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (node, node_handle) = build_node(BevyPlaceNodeConfig::default()).expect("failed to build node");

    let runtime_handle = tokio::runtime::Handle::current();
    runtime_handle.spawn(run_swarm_task(node));

    let mut app = App::new();

    // TODO: add toggle for headless (bootstrap) mode
    app.add_plugins(ViewerPlugin);

    app.insert_resource(ChunkedCanvas::new());
    app.insert_resource(node_handle);

    app.add_systems(
        Update,
        (
            inbound_pixel_update_system,
        )
    );

    app.run();

    Ok(())
}
