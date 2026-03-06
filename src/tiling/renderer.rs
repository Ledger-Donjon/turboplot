//! Tile renderer: picks tiles from the shared tiling and renders them using a [`Renderer`].

use super::tile::{Tile, TileProperties, TileSize, TileStatus, Tiling};
use crate::{
    renderer::Renderer,
    util::{Fixed, FixedVec2},
};
use std::sync::{Arc, Condvar, Mutex};

pub struct TilingRenderer {
    renderer: Box<dyn Renderer>,
    shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
    traces: Arc<Vec<Arc<Vec<f32>>>>,
}

impl TilingRenderer {
    pub fn new(
        shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
        traces: Arc<Vec<Arc<Vec<f32>>>>,
        renderer: Box<dyn Renderer>,
    ) -> Self {
        Self {
            renderer,
            shared_tiling,
            traces,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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

    pub fn has_pending(&self) -> bool {
        self.shared_tiling.0.lock().unwrap().has_pending()
    }

    pub fn render_next_tile(&mut self) {
        let Some(properties) = self.shared_tiling.0.lock().unwrap().take_job() else {
            return;
        };
        let data = self.render_tile(
            properties.id,
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
        id: u32,
        index: i32,
        offset: Fixed,
        scale: FixedVec2,
        size: TileSize,
    ) -> Vec<u32> {
        let trace: &Vec<f32> = &self.traces[id as usize];
        let trace_len = trace.len() as i32;
        let i_start = (index as f32 * size.w as f32 * scale.x.to_num::<f32>()).floor() as i32;
        let i_end = ((index + 1) as f32 * size.w as f32 * scale.x.to_num::<f32>()).floor() as i32;

        if (i_start >= trace_len) || (i_start < 0) {
            return vec![0; size.area() as usize];
        }

        let trace_chunk = &trace[i_start as usize..(i_end + 1).min(trace_len) as usize];

        // We need at least 2 points to have one segment.
        if trace_chunk.len() < 2 {
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

/// Pre-computed parameters for rendering a single tile.
pub struct TileRenderParams<'a> {
    pub chunk_samples: u32,
    pub trace_chunk: &'a [f32],
    pub w: u32,
    pub h: u32,
    pub offset: f32,
    pub scale_y: f32,
}

/// Extracts the trace slice and rendering parameters for a tile.
/// Returns `None` when the tile falls outside the trace or contains fewer than
/// 2 samples (no segment to render).
pub fn prepare_tile_render<'a>(
    trace: &'a [f32],
    properties: &TileProperties,
) -> Option<TileRenderParams<'a>> {
    let trace_len = trace.len() as i32;
    let w = properties.size.w;
    let h = properties.size.h;
    let scale_x = properties.scale.x.to_num::<f32>();

    let i_start = (properties.index as f32 * w as f32 * scale_x).floor() as i32;
    let i_end = ((properties.index + 1) as f32 * w as f32 * scale_x).floor() as i32;

    if (i_start >= trace_len) || (i_start < 0) {
        return None;
    }

    let trace_chunk = &trace[i_start as usize..(i_end + 1).min(trace_len) as usize];
    if trace_chunk.len() < 2 {
        return None;
    }

    Some(TileRenderParams {
        chunk_samples: (w as f32 * scale_x) as u32,
        trace_chunk,
        w,
        h,
        offset: properties.offset.to_num::<f32>(),
        scale_y: properties.scale.y.to_num::<f32>(),
    })
}
