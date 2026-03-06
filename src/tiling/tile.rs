//! Tile data model: tiling library, tile status, properties, sizes, and color scales.

use crate::util::{Fixed, FixedVec2};
use egui::{Color32, ColorImage, epaint::Hsva, lerp};

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
        let mut image = ColorImage::filled([size.w as usize, size.h as usize], Color32::BLACK);
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
    /// ID of the viewer using the tile.
    pub id: u32,
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

/// Defines the size of a tile.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct TileSize {
    pub(crate) w: u32,
    pub(crate) h: u32,
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
