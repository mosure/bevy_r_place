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

#[cfg(feature = "aws")]
use image::ImageEncoder;

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


pub async fn recover_latest_snapshot(
    config: &BevyPlaceConfig,
) -> Result<ChunkedCanvas, ()> {
    if !config.bootstrap {
        return Ok(ChunkedCanvas::new());
    }

    if let Some(artifact_folder) = config.artifact_directory.as_ref() {
        let snapshot_folder = Path::new(artifact_folder).join("snapshot");
        if snapshot_folder.exists() {
            let entries = match std::fs::read_dir(&snapshot_folder) {
                Ok(e) => e,
                Err(e) => {
                    error!("Failed to read snapshot folder {:?}: {:?}", snapshot_folder, e);
                    return Ok(ChunkedCanvas::new());
                },
            };
            let mut latest_file: Option<PathBuf> = None;
            let mut latest_ts: u64 = 0;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if ext == "png" {
                            if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                                if let Ok(ts) = filename.parse::<u64>() {
                                    if ts > latest_ts {
                                        latest_ts = ts;
                                        latest_file = Some(path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(file_path) = latest_file {
                info!("Found local snapshot: {:?}", file_path);
                match image::open(&file_path) {
                    Ok(dyn_img) => {
                        let rgba8 = dyn_img.to_rgba8();
                        let raw_bytes = rgba8.into_raw();
                        return Ok(ChunkedCanvas::from_bytes(&raw_bytes));
                    },
                    Err(e) => {
                        error!("Failed to open local snapshot file {:?}: {:?}", file_path, e);
                        return Ok(ChunkedCanvas::new());
                    }
                }
            } else {
                error!("No local snapshot file found in {:?}", snapshot_folder);
            }
        } else {
            error!("Snapshot folder {:?} does not exist", snapshot_folder);
        }
    }

    #[cfg(feature = "aws")]
    if let Some(bucket) = config.artifact_s3_bucket.clone() {
        let region = aws_sdk_s3::config::Region::new("us-west-2");
        let aws_config = aws_config::defaults(aws_sdk_s3::config::BehaviorVersion::v2024_03_28())
            .region(region)
            .load()
            .await;
        let client = aws_sdk_s3::Client::new(&aws_config);

        let list_resp = match client
            .list_objects_v2()
            .bucket(&bucket)
            .prefix("snapshot/")
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                error!("Failed to list objects in bucket {}: {:?}", bucket, e);
                return Ok(ChunkedCanvas::new());
            },
        };

        let objects = list_resp.contents.unwrap_or_default();

        let mut latest_key: Option<String> = None;
        let mut latest_ts: u64 = 0;

        for obj in objects {
            if let Some(key) = obj.key {
                if key.starts_with("snapshot/") && key.ends_with(".png") {
                    let ts_str = &key["snapshot/".len() .. key.len() - ".png".len()];
                    if let Ok(ts) = ts_str.parse::<u64>() {
                        if ts > latest_ts {
                            latest_ts = ts;
                            latest_key = Some(key);
                        }
                    }
                }
            }
        }

        let key = match latest_key {
            Some(k) => k,
            None => {
                error!("No valid snapshot found in bucket {}", bucket);
                return Ok(ChunkedCanvas::new());
            },
        };

        info!("Latest snapshot key found: {}", key);

        let get_resp = match client
            .get_object()
            .bucket(&bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                error!("Failed to get object {}: {:?}", key, e);
                return Ok(ChunkedCanvas::new());
            },
        };

        let data = match get_resp.body.collect().await {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to collect object body: {:?}", e);
                return Ok(ChunkedCanvas::new());
            },
        };
        let bytes = data.into_bytes();

        let dyn_img = match image::load_from_memory(&bytes) {
            Ok(img) => img,
            Err(e) => {
                error!("Failed to decode PNG image from S3: {:?}", e);
                return Ok(ChunkedCanvas::new());
            },
        };

        let rgba8 = dyn_img.to_rgba8();
        let raw_bytes = rgba8.into_raw();
        return Ok(ChunkedCanvas::from_bytes(&raw_bytes));
    }

    Ok(ChunkedCanvas::new())
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
    let image = world_canvas.to_image();
    let timestamp = epoch_time_seconds();

    // TODO: implement S3 artifact output
    if let Some(artifact_folder) = config.artifact_directory.as_ref() {
        let snapshot_folder = Path::new(artifact_folder)
            .join("snapshot");

        if !std::fs::exists(&snapshot_folder).ok().unwrap_or(false) {
            info!("creating snapshot folder {:?}", snapshot_folder);
            std::fs::create_dir_all(&snapshot_folder).ok();
        }

        let path = snapshot_folder.join(format!("{timestamp}.png"));

        image::save_buffer(
            &path,
            &image.data,
            image.width(),
            image.height(),
            image::ColorType::Rgba8,
        ).ok();

        info!("snapshot saved to {:?}", std::fs::canonicalize(path).ok());
    }

    #[cfg(feature = "aws")]
    if let Some(bucket) = config.artifact_s3_bucket.clone() {
        let s3_key = format!("snapshot/{}.png", timestamp);

        let mut png_data: Vec<u8> = Vec::new();
        {
            let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
            encoder
                .write_image(
                    &image.data,
                    image.width(),
                    image.height(),
                    image::ColorType::Rgba8.into(),
                )
                .expect("failed to encode PNG");
        }

        tokio::spawn(async move {
            let region = aws_config::Region::new("us-west-2");
            let config = aws_config::defaults(aws_sdk_s3::config::BehaviorVersion::v2024_03_28())
                .region(region)
                .load()
                .await;

            let client = aws_sdk_s3::Client::new(&config);

            match client
                .put_object()
                .bucket(&bucket)
                .key(s3_key.clone())
                .body(aws_sdk_s3::primitives::ByteStream::from(png_data))
                .send()
                .await
            {
                Ok(_) => info!("snapshot uploaded to S3 bucket {} with key {}", bucket, s3_key),
                Err(e) => error!("failed to upload snapshot to S3: {:?}", e),
            }
        });
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
