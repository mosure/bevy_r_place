use std::{
    fs::File,
    io::{
        BufWriter,
        Write,
    },
    path::{Path, PathBuf},
    time::Duration,
};

use bevy::{
    prelude::*,
    time::common_conditions::on_timer,
};

use crate::{
    args::BevyPlaceConfig,
    chunk_crdt::ChunkedCanvas,
    network::PixelUpdateMsg,
    time::epoch_time_seconds,
};


#[derive(Default)]
pub struct SnapshotPlugin {
    pub artifact_directory: Option<PathBuf>,
    pub pixel_artifact_stream_chunk_mb: Option<u64>,
    pub snapshot_interval: Option<Duration>,
}

impl Plugin for SnapshotPlugin {
    fn build(&self, app: &mut App) {
        if let Some(snapshot_interval) = self.snapshot_interval {
            info!("snapshot interval: {:?}", snapshot_interval);

            app.insert_resource(PixelArtifactStream {
                directory: self.artifact_directory
                    .as_ref()
                    .map(|path| path.join("stream")),
                current_file: None,
                current_file_size: 0,
                max_file_size: self.pixel_artifact_stream_chunk_mb
                    .map(|mb| mb * 1024 * 1024),
            });

            app
                .add_systems(
                    Update,
                    (
                        snapshot_interval_system.run_if(
                            on_timer(snapshot_interval)
                        ),
                        flush_pixel_artifact_stream.run_if(
                            on_timer(std::time::Duration::from_secs(120)),
                        ),
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
    if let Some(artifact_folder) = config.artifact_directory.as_ref() {
        let snapshot_folder = Path::new(artifact_folder)
            .join("snapshot");

        if !std::fs::exists(&snapshot_folder).ok().unwrap_or(false) {
            info!("creating snapshot folder {:?}", snapshot_folder);
            std::fs::create_dir_all(&snapshot_folder).ok();
        }

        let timestamp = epoch_time_seconds();
        let path = snapshot_folder.join(format!("{timestamp}.png"));

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


#[derive(Resource, Default)]
pub struct PixelArtifactStream {
    pub directory: Option<PathBuf>,
    pub current_file: Option<BufWriter<File>>,
    pub current_file_size: u64,
    pub max_file_size: Option<u64>,
}

impl PixelArtifactStream {
    fn open_new_chunk(&mut self) -> std::io::Result<()> {
        if let Some(ref mut writer) = self.current_file {
            writer.flush()?;
        }

        let directory = self.directory.as_ref().unwrap();

        if !std::fs::exists(directory).ok().unwrap_or(false) {
            info!("creating stream folder {:?}", directory);
            std::fs::create_dir_all(directory).ok();
        }

        let filename = format!("{}.brp", epoch_time_seconds());
        let file_path = directory.join(filename);
        let file = File::create(file_path)?;
        self.current_file = Some(BufWriter::new(file));
        self.current_file_size = 0;
        Ok(())
    }

    pub fn write_msg(&mut self, msg: &PixelUpdateMsg) -> std::io::Result<()> {
        if self.directory.is_none() {
            return Ok(());
        }

        if self.max_file_size.is_none() {
            return Ok(());
        }

        if self.current_file.is_none() {
            self.open_new_chunk()?;
        }

        let msg_size = bincode2::serialized_size(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let total_size_needed = msg_size + 4;

        if self.current_file_size + total_size_needed > self.max_file_size.unwrap() {
            if let Some(mut current) = self.current_file.take() {
                current.flush()?;
            }
            self.open_new_chunk()?;
        }

        if let Some(ref mut writer) = self.current_file {
            let size_bytes = (msg_size as u32).to_be_bytes();
            writer.write_all(&size_bytes)?;

            bincode2::serialize_into(writer, msg)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            self.current_file_size += total_size_needed;
        }

        Ok(())
    }
}


fn flush_pixel_artifact_stream(
    mut stream: ResMut<PixelArtifactStream>,
) {
    if let Some(ref mut writer) = stream.current_file {
        writer.flush().ok();
    }
}
