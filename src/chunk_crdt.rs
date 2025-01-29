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

use crate::time::epoch_time_seconds;


pub const CHUNK_SIZE: u32 = 64;
pub const WORLD_WIDTH: u32 = 1024;
pub const WORLD_HEIGHT: u32 = 1024;
pub const CHUNKS_X: u32 = WORLD_WIDTH / CHUNK_SIZE;
pub const CHUNKS_Y: u32 = WORLD_HEIGHT / CHUNK_SIZE;


#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub timestamp: u64,
    pub owner: [u8; 32],
}


#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub pixels: Vec<Pixel>,
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            pixels: vec![Pixel::default(); (CHUNK_SIZE * CHUNK_SIZE) as usize],
        }
    }
}

impl Chunk {
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
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkedCanvas {
    pub chunks: HashMap<(u32, u32), Chunk>,
}

impl ChunkedCanvas {
    pub fn new() -> Self {
        let mut map = HashMap::new();
        for cx in 0..CHUNKS_X {
            for cy in 0..CHUNKS_Y {
                map.insert((cx, cy), Chunk::default());
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
                    raw_data[index] = pixel.r;
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
    // pub chunk_x: u32,
    // pub chunk_y: u32,
    pub timestamp: u64,
}

impl Default for CanvasRequest {
    fn default() -> Self {
        Self {
            timestamp: epoch_time_seconds(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasResponse {
    // pub chunk: ChunkSnapshot,
    pub canvas: ChunkedCanvas,
}


pub mod codec {
    use std::{collections::TryReserveError, convert::Infallible, io, marker::PhantomData};

    use async_trait::async_trait;
    use cbor4ii::core::error::DecodeError;
    use futures::prelude::*;
    use libp2p::{
        request_response,
        swarm::StreamProtocol,
    };
    use serde::{de::DeserializeOwned, Serialize};

    pub struct Codec<Req, Resp> {
        /// Max request size in bytes.
        request_size_maximum: u64,
        /// Max response size in bytes.
        response_size_maximum: u64,
        phantom: PhantomData<(Req, Resp)>,
    }

    impl<Req, Resp> Default for Codec<Req, Resp> {
        fn default() -> Self {
            Codec {
                request_size_maximum: 1024 * 1024,
                response_size_maximum: 10 * 1024 * 1024,
                phantom: PhantomData,
            }
        }
    }

    impl<Req, Resp> Clone for Codec<Req, Resp> {
        fn clone(&self) -> Self {
            Self {
                request_size_maximum: self.request_size_maximum,
                response_size_maximum: self.response_size_maximum,
                phantom: PhantomData,
            }
        }
    }

    impl<Req, Resp> Codec<Req, Resp> {
        /// Sets the limit for request size in bytes.
        pub fn set_request_size_maximum(mut self, request_size_maximum: u64) -> Self {
            self.request_size_maximum = request_size_maximum;
            self
        }

        /// Sets the limit for response size in bytes.
        pub fn set_response_size_maximum(mut self, response_size_maximum: u64) -> Self {
            self.response_size_maximum = response_size_maximum;
            self
        }
    }

    #[async_trait]
    impl<Req, Resp> request_response::Codec for Codec<Req, Resp>
    where
        Req: Send + Serialize + DeserializeOwned,
        Resp: Send + Serialize + DeserializeOwned,
    {
        type Protocol = StreamProtocol;
        type Request = Req;
        type Response = Resp;

        async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Req>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut vec = Vec::new();

            io.take(self.request_size_maximum)
                .read_to_end(&mut vec)
                .await?;

            cbor4ii::serde::from_slice(vec.as_slice()).map_err(decode_into_io_error)
        }

        async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Resp>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut vec = Vec::new();

            io.take(self.response_size_maximum)
                .read_to_end(&mut vec)
                .await?;

            cbor4ii::serde::from_slice(vec.as_slice()).map_err(decode_into_io_error)
        }

        async fn write_request<T>(
            &mut self,
            _: &Self::Protocol,
            io: &mut T,
            req: Self::Request,
        ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send,
        {
            let data: Vec<u8> =
                cbor4ii::serde::to_vec(Vec::new(), &req).map_err(encode_into_io_error)?;

            io.write_all(data.as_ref()).await?;

            Ok(())
        }

        async fn write_response<T>(
            &mut self,
            _: &Self::Protocol,
            io: &mut T,
            resp: Self::Response,
        ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send,
        {
            let data: Vec<u8> =
                cbor4ii::serde::to_vec(Vec::new(), &resp).map_err(encode_into_io_error)?;

            io.write_all(data.as_ref()).await?;

            Ok(())
        }
    }

    fn decode_into_io_error(err: cbor4ii::serde::DecodeError<Infallible>) -> io::Error {
        match err {
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            cbor4ii::serde::DecodeError::Core(DecodeError::Read(e)) => {
                io::Error::new(io::ErrorKind::Other, e)
            }
            cbor4ii::serde::DecodeError::Core(e @ DecodeError::Unsupported { .. }) => {
                io::Error::new(io::ErrorKind::Unsupported, e)
            }
            cbor4ii::serde::DecodeError::Core(e @ DecodeError::Eof { .. }) => {
                io::Error::new(io::ErrorKind::UnexpectedEof, e)
            }
            cbor4ii::serde::DecodeError::Core(e) => io::Error::new(io::ErrorKind::InvalidData, e),
            cbor4ii::serde::DecodeError::Custom(e) => {
                io::Error::new(io::ErrorKind::Other, e.to_string())
            }
        }
    }

    fn encode_into_io_error(err: cbor4ii::serde::EncodeError<TryReserveError>) -> io::Error {
        io::Error::new(io::ErrorKind::Other, err)
    }
}

