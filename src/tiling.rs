use crate::{
    renderer::GpuRenderer,
    util::{Fixed, FixedVec2},
};
use egui::{Color32, ColorImage};
use std::sync::{Arc, Condvar, Mutex};

/// A library of tiles and their current rendering status and result.
///
/// This structure is shared between the viewer, which asks for tiles and use them, and a tile
/// rendered which receives and fulfill rendering requests.
pub struct Tiling {
    pub tiles: Vec<Tile>,
}

impl Tiling {
    pub fn new() -> Self {
        Self { tiles: Vec::new() }
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

    /// Returns true if there is at least one tile which is not rendered.
    pub fn has_pending(&self) -> bool {
        self.next_pending().is_some()
    }

    /// Get properties of next tile to be rendered. Returns `None` if there are no pending jobs.
    pub fn next_pending(&self) -> Option<TileProperties> {
        self.tiles
            .iter()
            .find(|t| t.status == TileStatus::NotRendered)
            .map(|t| t.properties)
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

    pub fn generate_image(&self, scale_x: Fixed, color_scale: ColorScale) -> ColorImage {
        let size = self.properties.size;
        let mut image = ColorImage::new([size.w as usize, size.h as usize], Color32::BLACK);
        for x in 0..(size.w as i32) {
            for y in 0..size.h as i32 {
                let offset = x * size.h as i32 + y;
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
                image.pixels[(y * size.w as i32 + x) as usize] = Color32::from_gray(c);
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
    /// Rendering scale.
    /// For x-axis, this is the number of samples for each pixel column.
    pub scale: FixedVec2,
    /// Index of the first sample in the trace for this tile.
    pub index: i32,
    /// Width and Height of the tile.
    pub size: TileSize,
}

pub struct TilingRenderer {
    shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
    trace: Vec<f32>,
    gpu_renderer: GpuRenderer,
}

impl TilingRenderer {
    pub fn new(shared_tiling: Arc<(Mutex<Tiling>, Condvar)>, trace: Vec<f32>) -> Self {
        Self {
            shared_tiling,
            trace,
            gpu_renderer: GpuRenderer::new(),
        }
    }

    pub fn render_loop(&mut self) {
        loop {
            self.render_next_tile();
            {
                let (tiling, condvar) = &*self.shared_tiling;
                let guard = tiling.lock().unwrap();
                let _guard = condvar.wait_while(guard, |t| !t.has_pending()).unwrap();
            }
        }
    }

    fn render_next_tile(&mut self) {
        let Some(properties) = self.shared_tiling.0.lock().unwrap().next_pending() else {
            return;
        };
        let data = self.render_tile(properties.index, properties.scale, properties.size);
        // Save the result
        let (tiling, _) = &*self.shared_tiling;
        let mut tiling = tiling.lock().unwrap();
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

    /// Renders the tile starting a sample `index` for the given scales `scale_x` and `scale_y`.
    fn render_tile(&mut self, index: i32, scale: FixedVec2, size: TileSize) -> Vec<u32> {
        let trace_len = self.trace.len() as i32;
        let i_start = (index as f32 * size.w as f32 * scale.x.to_num::<f32>()).floor() as i32;
        let i_end = ((index + 1) as f32 * size.w as f32 * scale.x.to_num::<f32>()).floor() as i32;

        if (i_start >= trace_len) || (i_start < 0) {
            return vec![0; size.area() as usize];
        }

        let trace_chunk = &self.trace[i_start as usize..i_end.min(trace_len) as usize];
        let mut result: Vec<u32> = Vec::new();
        if trace_chunk.is_empty() {
            return result;
        }

        self.gpu_renderer.render(
            (size.w as f32 * scale.x.to_num::<f32>()) as u32,
            trace_chunk,
            size.w,
            size.h,
            scale.y.to_num::<f32>(),
        );
        result.resize(size.area() as usize, 0);
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

/// Defines the size of a tile.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct TileSize {
    w: u32,
    h: u32,
}

impl TileSize {
    pub fn new(w: u32, h: u32) -> Self {
        Self { w, h }
    }

    /// Returns width multiplied by height.
    /// Panics in case of overflow.
    pub fn area(&self) -> u32 {
        self.w.checked_mul(self.h).unwrap()
    }
}
