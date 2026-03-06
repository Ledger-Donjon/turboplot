//! CPU density renderer using a difference-array algorithm.

use super::Renderer;
use std::ops::Sub;

pub struct CpuRenderer {}

impl CpuRenderer {
    pub fn new() -> Self {
        Self {}
    }
}

impl Renderer for CpuRenderer {
    fn render(
        &self,
        chunk_samples: u32,
        trace: &[f32],
        w: u32,
        h: u32,
        offset: f32,
        scale_y: f32,
    ) -> Vec<u32> {
        debug_assert!(trace.len() >= 2);
        let mut result = vec![0; (w * h) as usize];
        // Implementation using difference array for fast density calculation.
        // Optimization suggested by ProgramCrafter in:
        // https://github.com/Ledger-Donjon/turboplot/issues/3.
        let h_mid = (h as f32) / 2.0;
        // Difference array is created once and reused for each pixel column.
        let mut diff = vec![0i32; h as usize + 1];
        for x in 0..w {
            for v in diff.iter_mut() {
                *v = 0;
            }
            let i_start = trace
                .len()
                .sub(2)
                .min((chunk_samples as usize * x as usize) / w as usize);
            let i_end = trace
                .len()
                .sub(1)
                .min((chunk_samples as usize * (x as usize + 1)) / w as usize);
            for i in i_start..i_end {
                let y0 = h_mid - ((trace[i] + offset) * scale_y);
                let y1 = h_mid - ((trace[i + 1] + offset) * scale_y);
                let (y0, y1) = (y0.min(y1).ceil() as i32, y0.max(y1).floor() as i32);
                let y0 = y0.clamp(0, (h - 1) as i32) as usize;
                let y1 = y1.clamp(0, (h - 1) as i32) as usize;
                diff[y0] += 1;
                diff[y1 + 1] -= 1;
            }

            let mut density = 0i32;
            for y in 0..h {
                density += diff[y as usize];
                debug_assert!(density >= 0);
                result[(x as i32 * h as i32 + y as i32) as usize] = density as u32;
            }
        }
        result
    }
}
