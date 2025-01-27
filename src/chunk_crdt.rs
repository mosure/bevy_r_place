use std::collections::HashMap;

use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{
            Extent3d,
            TextureDimension,
            TextureFormat,
        },
    },
};
use serde::{Serialize, Deserialize};


pub const CHUNK_SIZE: u32 = 64;
pub const WORLD_WIDTH: u32 = 1024;
pub const WORLD_HEIGHT: u32 = 1024;
pub const CHUNKS_X: u32 = WORLD_WIDTH / CHUNK_SIZE;
pub const CHUNKS_Y: u32 = WORLD_HEIGHT / CHUNK_SIZE;


#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub timestamp: u64,
    pub owner: [u8; 32],
}

impl Default for Pixel {
    fn default() -> Self {
        Pixel {
            r: 0,
            g: 0,
            b: 0,
            timestamp: 0,
            owner: [0;32],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub pixels: Vec<Pixel>,
}

impl Chunk {
    pub fn new() -> Self {
        Self {
            pixels: vec![Pixel::default(); (CHUNK_SIZE * CHUNK_SIZE) as usize],
        }
    }

    /// last-writer-wins update
    pub fn update_pixel(&mut self, x: u32, y: u32, new_pixel: Pixel) -> bool {
        if x >= CHUNK_SIZE || y >= CHUNK_SIZE {
            warn!("out of bounds chunk update: ({}, {})", x, y);
            return false;
        }
        let idx = (y * CHUNK_SIZE + x) as usize;
        let old_pixel = &mut self.pixels[idx];
        if new_pixel.timestamp > old_pixel.timestamp {
            *old_pixel = new_pixel;
            true
        } else {
            warn!("timestamp not greater than old pixel: {} <= {}", new_pixel.timestamp, old_pixel.timestamp);
            false
        }
    }
}

// TODO: convert to an asset
#[derive(Resource)]
pub struct ChunkedCanvas {
    pub chunks: HashMap<(u32, u32), Chunk>,
}

impl ChunkedCanvas {
    pub fn new() -> Self {
        let mut map = HashMap::new();
        for cx in 0..CHUNKS_X {
            for cy in 0..CHUNKS_Y {
                map.insert((cx, cy), Chunk::new());
            }
        }
        Self { chunks: map }
    }

    pub fn set_pixel(&mut self, wx: u32, wy: u32, pixel: Pixel) -> bool {
        if wx >= WORLD_WIDTH || wy >= WORLD_HEIGHT {
            warn!("out of bounds pixel update: ({}, {})", wx, wy);
            return false;
        }

        let cx = wx / CHUNK_SIZE;
        let cy = wy / CHUNK_SIZE;
        let lx = wx % CHUNK_SIZE;
        let ly = wy % CHUNK_SIZE;

        if let Some(chunk) = self.chunks.get_mut(&(cx, cy)) {
            chunk.update_pixel(lx, ly, pixel)
        } else {
            warn!("chunk not found: ({}, {})", cx, cy);

            false
        }
    }

    pub fn to_image(&self) -> Image {
        let mut raw_data = vec![0_u8; (WORLD_WIDTH * WORLD_HEIGHT * 4) as usize];

        for ((cx, cy), chunk) in &self.chunks {
            for y in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let pixel = &chunk.pixels[(y * CHUNK_SIZE + x) as usize];
                    let wx = cx * CHUNK_SIZE + x;
                    let wy = cy * CHUNK_SIZE + y;

                    let index = ((wy * WORLD_WIDTH + wx) * 4) as usize;
                    raw_data[index + 0] = pixel.r;
                    raw_data[index + 1] = pixel.g;
                    raw_data[index + 2] = pixel.b;
                    raw_data[index + 3] = 255;
                }
            }
        }

        Image::new_fill(
            Extent3d {
                width: WORLD_WIDTH,
                height: WORLD_HEIGHT,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &raw_data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkSnapshot {
    pub cx: u32,
    pub cy: u32,
    pub chunk: Chunk,
}

impl ChunkSnapshot {
    pub fn from_chunk(cx: u32, cy: u32, chunk: &Chunk) -> Self {
        Self {
            cx,
            cy,
            chunk: chunk.clone(),
        }
    }
}


// TODO: design a better canvas transfer method, support chunking to arbitrary world sizes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasRequest {
    pub chunk_x: u32,
    pub chunk_y: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasResponse {
    pub chunk: ChunkSnapshot,
}
