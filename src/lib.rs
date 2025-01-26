pub mod chunk_crdt;
pub mod color_picker;
pub mod headless;
pub mod network;
pub mod viewer;
pub mod window_icon;

pub use libp2p::PeerId;


pub mod prelude {
    pub use crate::chunk_crdt::{
        ChunkedCanvas,
        WORLD_HEIGHT,
        WORLD_WIDTH,
    };
    pub use crate::headless::HeadlessPlugin;
    pub use crate::network::{
        build_node,
        inbound_pixel_update_system,
        run_swarm_task,
        BevyPlaceNode,
        BevyPlaceNodeConfig,
        BevyPlaceNodeHandle,
        PixelUpdateMsg,
    };
    pub use crate::viewer::ViewerPlugin;
}
