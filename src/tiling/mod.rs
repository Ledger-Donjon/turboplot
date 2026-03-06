//! Tile-based rendering system: data model, renderers, and platform-specific schedulers.

mod renderer;
mod scheduler;
mod tile;

pub use renderer::{TileRenderParams, TilingRenderer, prepare_tile_render};
pub use scheduler::TileScheduler;
pub use tile::{
    ColorScale, Gradient, Tile, TileProperties, TileSize, TileStatus, Tiling,
};
