//! Rendering backends: GPU compute shaders (via wgpu) and CPU fallback.

mod cpu;
mod gpu;

pub use cpu::CpuRenderer;
pub use gpu::AsyncGpuRenderer;

/// Maximum number of f32 trace segments that can be sent to the GPU at once.
pub const RENDERER_MAX_TRACE_SIZE: usize = 8 * 1024 * 1024 * 4;

pub trait Renderer: Send {
    fn render(
        &self,
        chunk_samples: u32,
        trace: &[f32],
        w: u32,
        h: u32,
        offset: f32,
        scale_y: f32,
    ) -> Vec<u32>;
}
