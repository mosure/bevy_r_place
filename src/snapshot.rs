use std::time::Duration;

use bevy::{
    prelude::*,
    time::common_conditions::on_timer,
};

use crate::{
    args::BevyPlaceConfig,
    chunk_crdt::ChunkedCanvas,
    time::epoch_time_seconds,
};


#[derive(Default)]
pub struct SnapshotPlugin {
    pub snapshot_interval: Option<Duration>,
}

impl Plugin for SnapshotPlugin {
    fn build(&self, app: &mut App) {
        if let Some(snapshot_interval) = self.snapshot_interval {
            info!("snapshot interval: {:?}", snapshot_interval);

            app
                .add_systems(
                    Update,
                    snapshot_interval_system.run_if(
                        on_timer(snapshot_interval)
                    )
                );
        }
    }
}


fn snapshot_interval_system(
    config: Res<BevyPlaceConfig>,
    world_canvas: Res<ChunkedCanvas>,
    mut previous_canvas_snapshot: Local<ChunkedCanvas>,
) {
    if *world_canvas == *previous_canvas_snapshot {
        debug!("skipping snapshot, no changes.");
        return;
    }

    canvas_snapshot(&world_canvas, &config);
    *previous_canvas_snapshot = world_canvas.clone();
}


pub fn canvas_snapshot(
    world_canvas: &ChunkedCanvas,
    config: &BevyPlaceConfig,
) {
    // TODO: implement S3 artifact output
    if let Some(artifact_folder) = config.artifact_folder.as_ref() {
        if !std::fs::exists(artifact_folder).ok().unwrap_or(false) {
            info!("creating artifact folder {:?}", artifact_folder);
            std::fs::create_dir(artifact_folder).ok();
        }

        let timestamp = epoch_time_seconds();
        let path = std::path::Path::new(artifact_folder)
            .join(format!("{timestamp}.png"));

        let image = world_canvas.to_image();
        image::save_buffer(
            &path,
            &image.data,
            image.width(),
            image.height(),
            image::ColorType::Rgba8,
        ).ok();

        info!("snapshot saved to {:?}", std::fs::canonicalize(path).ok());
    }
}
