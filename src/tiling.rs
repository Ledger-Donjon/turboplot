use std::sync::{Arc, Mutex};
use crate::renderer::GpuRenderer;

/// A library of tiles and their current rendering status and result.
/// 
/// This structure is shared between the viewer, which asks for tiles and use them, and a tile
/// rendered which receives and fulfill rendering requests.
pub struct Tiling {
    pub tiles: Vec<Tile>,
    height: u32,
}

impl Tiling {
    pub fn new() -> Self {
        Self {
            tiles: Vec::new(),
            height: 64,
        }
    }

    pub fn get(&mut self, properties: TileProperties) -> Tile {
        if let Some(tile) = self.tiles.iter().find(|x| x.properties == properties) {
            return tile.clone();
        }
        let tile = Tile::new(properties);
        self.tiles.push(tile.clone());
        return tile;
    }

    pub fn set_height(&mut self, height: u32) {
        if height != self.height {
            self.height = height;
            self.tiles.clear();
        }
    }
}

#[derive(Clone)]
pub struct Tile {
    pub status: TileStatus,
    pub properties: TileProperties,
    pub data: Vec<u32>,
}

impl Tile {
    pub fn new(properties: TileProperties) -> Self {
        Self {
            status: TileStatus::NotRendered,
            properties,
            data: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TileStatus {
    NotRendered,
    Rendered,
}

/// Uniquely identifies a tile that can be rendered.
/// If any of this structure values changes, the corresponding render of this tile will be
/// different as well.
#[derive(Clone, Copy, PartialEq)]
pub struct TileProperties {
    /// Rendering X-axis scale.
    /// This is the number of samples for each pixel column.
    pub scale_x: f32,
    /// Rendering Y-axis scale.
    pub scale_y: f32,
    /// Index of the first sample in the trace for this tile.
    pub index: i32,
    /// Height in pixels of the tile.
    pub height: u32,
}


pub struct TilingRenderer {
    shared_tiling: Arc<Mutex<Tiling>>,
    trace: Vec<f32>,
    gpu_renderer: GpuRenderer,
    tile_width: u32,
}

impl TilingRenderer {
    pub fn new(shared_tiling: Arc<Mutex<Tiling>>, trace: Vec<f32>) -> Self {
        Self {
            shared_tiling,
            trace,
            gpu_renderer: GpuRenderer::new(),
            tile_width: 64,
        }
    }

    pub fn render_next_tile(&mut self) {
        let (height, Some(properties)) = ({
            let tiling = self.shared_tiling.lock().unwrap();
            if let Some(tile) = tiling
                .tiles
                .iter()
                .find(|x| x.status == TileStatus::NotRendered)
            {
                (tiling.height, Some(tile.properties.clone()))
            } else {
                (tiling.height, None)
            }
        }) else {
            return;
        };

        // We have a tile to be rendered
        let data = self.render_tile(
            properties.index,
            properties.scale_x,
            properties.scale_y,
            height,
        );
        // Save the result
        {
            let mut tiling = self.shared_tiling.lock().unwrap();
            if let Some(tile) = tiling.tiles.iter_mut().find(|x| x.properties == properties) {
                tile.data = data;
                tile.status = TileStatus::Rendered;
            } else {
                // Tile not found, it probably has been deleted during rendering. Save as new tile
                // anyway.
                tiling.tiles.push(Tile {
                    status: TileStatus::Rendered,
                    properties,
                    data,
                });
            }
        }
    }

    /// Renders the tile starting a sample `index` for the given scales `scale_x` and `scale_y`.
    pub fn render_tile(&mut self, index: i32, scale_x: f32, scale_y: f32, tile_height: u32) -> Vec<u32> {
        let trace_len = self.trace.len() as i32;
        let tile_w = self.tile_width;
        let i_start = (index as f32 * tile_w as f32 * scale_x).floor() as i32;
        let i_end = ((index + 1) as f32 * tile_w as f32 * scale_x).floor() as i32;

        if (i_start >= trace_len) || (i_start < 0) {
            let mut result = Vec::new();
            result.resize((self.tile_width * tile_height) as usize, 0);
            return result;
        }

        let trace_chunk = &self.trace[i_start as usize..i_end.min(trace_len) as usize];
        let mut result: Vec<u32> = Vec::new();
        if trace_chunk.len() == 0 {
            return result;
        }

        self.gpu_renderer
            .render((tile_w as f32 * scale_x) as u32, &trace_chunk, tile_w as u32, tile_height, scale_y);
        result.resize((tile_w * tile_height) as usize, 0);
        self.gpu_renderer.read_result(&mut result);
        result
    }
}