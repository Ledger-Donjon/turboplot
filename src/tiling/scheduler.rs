//! Tile scheduler — two platform-specific implementations behind a type alias.
//!
//! Both expose the same public API:
//!   `new(traces, gpu_threads, cpu_threads, render_state) -> Self`
//!   `shared_tiling() -> Arc<(Mutex<Tiling>, Condvar)>`
//!   `process_pending(&mut self)`

use super::renderer::TilingRenderer;
use super::tile::Tiling;
use crate::renderer::{AsyncGpuRenderer, CpuRenderer};
use std::sync::{Arc, Condvar, Mutex};

/// Compile-time alias that selects the right scheduler for the target platform.
#[cfg(not(target_arch = "wasm32"))]
pub type TileScheduler = NativeScheduler;
#[cfg(target_arch = "wasm32")]
pub type TileScheduler = WebScheduler;

// ---------------------------------------------------------------------------
// Native
// ---------------------------------------------------------------------------

/// Native tile scheduler — spawns background threads at construction.
///
/// GPU and CPU rendering threads run independently; [`process_pending`] is a
/// no-op because the threads handle everything.
#[cfg(not(target_arch = "wasm32"))]
pub struct NativeScheduler {
    shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeScheduler {
    pub fn new(
        traces: Arc<Vec<Arc<Vec<f32>>>>,
        gpu_threads: usize,
        cpu_threads: usize,
        render_state: Option<&eframe::egui_wgpu::RenderState>,
    ) -> Self {
        use std::thread;

        let shared_tiling = Arc::new((Mutex::new(Tiling::new()), Condvar::new()));

        for _ in 0..gpu_threads {
            if let Some(gpu) = render_state.and_then(AsyncGpuRenderer::from_render_state) {
                let st = shared_tiling.clone();
                let tr = traces.clone();
                thread::spawn(move || {
                    TilingRenderer::new(st, tr, Box::new(gpu)).render_loop();
                });
            }
        }

        for _ in 0..cpu_threads {
            let st = shared_tiling.clone();
            let tr = traces.clone();
            thread::spawn(move || {
                TilingRenderer::new(st, tr, Box::new(CpuRenderer::new())).render_loop();
            });
        }

        Self { shared_tiling }
    }

    /// Returns a clone of the shared tiling Arc, needed by viewers to request
    /// and read tiles.
    pub fn shared_tiling(&self) -> Arc<(Mutex<Tiling>, Condvar)> {
        self.shared_tiling.clone()
    }

    /// No-op on native — background threads handle tile rendering.
    pub fn process_pending(&mut self) {}
}

// ---------------------------------------------------------------------------
// Web
// ---------------------------------------------------------------------------

/// Web tile scheduler — drives CPU and GPU renderers from the main loop.
///
/// [`process_pending`] must be called each frame to poll the async GPU
/// renderer and run a batch of synchronous CPU tile renders.
#[cfg(target_arch = "wasm32")]
use super::renderer::prepare_tile_render;
#[cfg(target_arch = "wasm32")]
use super::tile::{Tile, TileProperties, TileStatus};

#[cfg(target_arch = "wasm32")]
pub struct WebScheduler {
    shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
    traces: Arc<Vec<Arc<Vec<f32>>>>,
    cpu_renderer: TilingRenderer,
    gpu: Option<AsyncGpuRenderer>,
    pending_gpu_tile: Option<TileProperties>,
}

#[cfg(target_arch = "wasm32")]
impl WebScheduler {
    pub fn new(
        traces: Arc<Vec<Arc<Vec<f32>>>>,
        _gpu_threads: usize,
        _cpu_threads: usize,
        render_state: Option<&eframe::egui_wgpu::RenderState>,
    ) -> Self {
        let shared_tiling = Arc::new((Mutex::new(Tiling::new()), Condvar::new()));

        let cpu_renderer = TilingRenderer::new(
            shared_tiling.clone(),
            traces.clone(),
            Box::new(CpuRenderer::new()),
        );
        let gpu = render_state.and_then(AsyncGpuRenderer::from_render_state);

        Self {
            shared_tiling,
            traces,
            cpu_renderer,
            gpu,
            pending_gpu_tile: None,
        }
    }

    /// Returns a clone of the shared tiling Arc, needed by viewers to request
    /// and read tiles.
    pub fn shared_tiling(&self) -> Arc<(Mutex<Tiling>, Condvar)> {
        self.shared_tiling.clone()
    }

    /// Drives tile rendering forward: polls the async GPU renderer and runs a
    /// batch of synchronous CPU tile renders.
    pub fn process_pending(&mut self) {
        self.process_gpu();

        for _ in 0..32 {
            if !self.cpu_renderer.has_pending() {
                break;
            }
            self.cpu_renderer.render_next_tile();
        }
    }

    /// Collects completed GPU results and submits new work when the GPU is idle.
    fn process_gpu(&mut self) {
        let Some(gpu) = &self.gpu else { return };

        // Collect a completed result.
        if self.pending_gpu_tile.is_some() {
            if let Some(data) = gpu.poll_result() {
                let properties = self.pending_gpu_tile.take().unwrap();
                let mut tiling = self.shared_tiling.0.lock().unwrap();
                if let Some(tile) =
                    tiling.tiles.iter_mut().find(|x| x.properties == properties)
                {
                    tile.data = data;
                    tile.status = TileStatus::Rendered;
                } else {
                    tiling.tiles.push(Tile {
                        status: TileStatus::Rendered,
                        properties,
                        data,
                    });
                }
            }
        }

        // Submit new work when idle.
        if !gpu.is_busy() {
            let job = self.shared_tiling.0.lock().unwrap().take_job();
            if let Some(properties) = job {
                let trace = &self.traces[properties.id as usize];
                if let Some(params) = prepare_tile_render(trace, &properties) {
                    gpu.submit(
                        params.chunk_samples,
                        params.trace_chunk,
                        params.w,
                        params.h,
                        params.offset,
                        params.scale_y,
                    );
                    self.pending_gpu_tile = Some(properties);
                } else {
                    let mut tiling = self.shared_tiling.0.lock().unwrap();
                    if let Some(tile) =
                        tiling.tiles.iter_mut().find(|x| x.properties == properties)
                    {
                        tile.data = vec![0; properties.size.area() as usize];
                        tile.status = TileStatus::Rendered;
                    }
                }
            }
        }
    }
}
