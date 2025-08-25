use crate::{renderer::GpuRenderer, util::U64F24};
use egui::{Color32, ColorImage};
use std::sync::{Arc, Mutex};

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

    pub fn get(&mut self, properties: TileProperties, request: bool) -> Option<Tile> {
        if let Some(tile) = self.tiles.iter().find(|x| x.properties == properties) {
            return Some(tile.clone());
        }
        if request {
            let tile = Tile::new(properties);
            self.tiles.push(tile.clone());
            Some(tile)
        } else {
            None
        }
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

    pub fn generate_image(&self, scale_x: U64F24, color_scale: ColorScale) -> ColorImage {
        let size = self.properties.size;
        let mut image = ColorImage::new([size.0 as usize, size.1 as usize], Color32::BLACK);
        for x in 0..(size.0 as i32) {
            for y in 0..size.1 as i32 {
                let offset = x * size.1 as i32 + y;
                let density = self.data[offset as usize];
                let a = if density == 0 {
                    0.0
                } else {
                    color_scale.minimum
                        + ((density as f32).powf(color_scale.power)
                            * color_scale.opacity
                            * 0.005
                            * (1000.0 / scale_x.to_num::<f32>()))
                };
                let c = (a * 255.0) as u8;
                image.pixels[(y * size.0 as i32 + x) as usize] = Color32::from_gray(c);
            }
        }
        image
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
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct TileProperties {
    /// Rendering X-axis scale.
    /// This is the number of samples for each pixel column.
    pub scale_x: U64F24,
    /// Rendering Y-axis scale.
    pub scale_y: U64F24,
    /// Index of the first sample in the trace for this tile.
    pub index: i32,
    /// Width and Height of the tile.
    pub size: (u32, u32),
}

pub struct TilingRenderer {
    shared_tiling: Arc<Mutex<Tiling>>,
    trace: Vec<f32>,
    gpu_renderer: GpuRenderer,
    tile_width: u32,
}

impl TilingRenderer {
    pub fn new(shared_tiling: Arc<Mutex<Tiling>>, tile_width: u32, trace: Vec<f32>) -> Self {
        Self {
            shared_tiling,
            trace,
            gpu_renderer: GpuRenderer::new(),
            tile_width,
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
    pub fn render_tile(
        &mut self,
        index: i32,
        scale_x: U64F24,
        scale_y: U64F24,
        tile_height: u32,
    ) -> Vec<u32> {
        let trace_len = self.trace.len() as i32;
        let tile_w = self.tile_width;
        let i_start = (index as f32 * tile_w as f32 * scale_x.to_num::<f32>()).floor() as i32;
        let i_end = ((index + 1) as f32 * tile_w as f32 * scale_x.to_num::<f32>()).floor() as i32;

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

        self.gpu_renderer.render(
            (tile_w as f32 * scale_x.to_num::<f32>()) as u32,
            &trace_chunk,
            tile_w as u32,
            tile_height,
            scale_y.to_num::<f32>(),
        );
        result.resize((tile_w * tile_height) as usize, 0);
        self.gpu_renderer.read_result(&mut result);
        result
    }
}

#[derive(Copy, Clone, PartialEq)]
pub struct ColorScale {
    pub minimum: f32,
    pub power: f32,
    pub opacity: f32,
}
