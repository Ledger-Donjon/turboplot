use crate::{
    renderer::Renderer,
    util::{Fixed, FixedVec2},
};
use egui::{Color32, ColorImage, epaint::Hsva, lerp};
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
        self.tiles.iter().any(|t| t.status != TileStatus::Rendered)
    }

    /// Finds and returns a pending rendering, and tag it has being currently rendered.
    /// If no pending job is available, `None` is returned.
    pub fn take_job(&mut self) -> Option<TileProperties> {
        if let Some(tile) = self
            .tiles
            .iter_mut()
            .find(|t| t.status == TileStatus::NotRendered)
        {
            tile.status = TileStatus::Rendering;
            Some(tile.properties)
        } else {
            None
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

    pub fn generate_image(&self, color_scale: ColorScale) -> ColorImage {
        let size = self.properties.size;
        let mut image = ColorImage::new([size.w as usize, size.h as usize], Color32::BLACK);
        let sx = 1.0 / self.properties.scale.x.to_num::<f32>();
        for x in 0..(size.w as i32) {
            for y in 0..size.h as i32 {
                let offset = x * size.h as i32 + y;
                let density = self.data[offset as usize];
                let a = if density == 0 {
                    0.0
                } else {
                    ((density as f32) * sx).powf(color_scale.power) * color_scale.opacity
                };
                let color = if a > 0.0 {
                    color_scale.gradient.apply(a.clamp(0.0, 1.0))
                } else {
                    Color32::BLACK
                };
                image.pixels[(y * size.w as i32 + x) as usize] = color;
            }
        }
        image
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TileStatus {
    /// The tile must be rendered.
    NotRendered,
    /// A renderer is currently generating the tile.
    Rendering,
    /// The tile has been rendered.
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
    /// Vertical offset.
    /// This value is added to the trace samples during GPU rendering.
    pub offset: Fixed,
    /// Index of the first sample in the trace for this tile.
    pub index: i32,
    /// Width and Height of the tile.
    pub size: TileSize,
}

pub struct TilingRenderer<'a> {
    renderer: Box<dyn Renderer>,
    shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
    trace: &'a Vec<f32>,
}

impl<'a> TilingRenderer<'a> {
    pub fn new(
        shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
        trace: &'a Vec<f32>,
        renderer: Box<dyn Renderer>,
    ) -> Self {
        Self {
            renderer,
            shared_tiling,
            trace,
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
        let Some(properties) = self.shared_tiling.0.lock().unwrap().take_job() else {
            return;
        };
        let data = self.render_tile(
            properties.index,
            properties.offset,
            properties.scale,
            properties.size,
        );
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
    fn render_tile(
        &mut self,
        index: i32,
        offset: Fixed,
        scale: FixedVec2,
        size: TileSize,
    ) -> Vec<u32> {
        let trace_len = self.trace.len() as i32;
        let i_start = (index as f32 * size.w as f32 * scale.x.to_num::<f32>()).floor() as i32;
        let i_end = ((index + 1) as f32 * size.w as f32 * scale.x.to_num::<f32>()).floor() as i32;

        if (i_start >= trace_len) || (i_start < 0) {
            return vec![0; size.area() as usize];
        }

        let trace_chunk = &self.trace[i_start as usize..(i_end + 1).min(trace_len) as usize];
        if trace_chunk.is_empty() {
            return vec![0; size.area() as usize];
        }

        self.renderer.render(
            (size.w as f32 * scale.x.to_num::<f32>()) as u32,
            trace_chunk,
            size.w,
            size.h,
            offset.to_num::<f32>(),
            scale.y.to_num::<f32>(),
        )
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum Gradient {
    SingleColor { min: f32, end: Color32 },
    BiColor { start: Color32, end: Color32 },
    Rainbow,
}

impl Gradient {
    pub fn apply(&self, x: f32) -> Color32 {
        debug_assert!((0.0..=1.0).contains(&x));
        match self {
            Gradient::SingleColor { min, end } => {
                let t = x * (1.0 - min) + min;
                Color32::BLACK.lerp_to_gamma(*end, t)
            }
            Gradient::BiColor { start, end } => start.lerp_to_gamma(*end, x),
            Gradient::Rainbow => Hsva::new(lerp(4.0 / 6.0..=0.0, x), 1.0, 1.0, 1.0).into(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Gradient::SingleColor { .. } => "Single color",
            Gradient::BiColor { .. } => "Gradient",
            Gradient::Rainbow => "Rainbow",
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
pub struct ColorScale {
    pub power: f32,
    pub opacity: f32,
    pub gradient: Gradient,
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
