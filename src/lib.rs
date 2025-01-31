pub mod args;
pub mod chunk_crdt;
pub mod color_picker;
pub mod headless;
pub mod network;
pub mod snapshot;
pub mod time;

#[cfg(feature = "viewer")]
pub mod viewer;

#[cfg(all(feature = "native", feature = "viewer"))]
pub mod window_icon;

pub use libp2p::PeerId;


pub mod prelude {
    pub use crate::args::BevyPlaceConfig;
    pub use crate::chunk_crdt::{
        ChunkedCanvas,
        WORLD_HEIGHT,
        WORLD_WIDTH,
    };
    pub use crate::headless::HeadlessPlugin;
    pub use crate::network::{
        build_node,
        run_swarm_task,
        BevyPlaceNode,
        BevyPlaceNodeConfig,
        BevyPlaceNodeHandle,
        PixelUpdateMsg,
        SwarmPlugin,
    };
    pub use crate::snapshot::SnapshotPlugin;

    #[cfg(feature = "viewer")]
    pub use crate::viewer::ViewerPlugin;
}
