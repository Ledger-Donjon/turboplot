//! Async GPU renderer — unified implementation, no platform cfg.
//!
//! Two usage patterns from the same code:
//! - **Non-blocking:** [`AsyncGpuRenderer::submit`] + [`AsyncGpuRenderer::poll_result`]
//!   (for the web main-loop)
//! - **Blocking:** [`Renderer::render`] (for native background threads)
//!
//! The Device & Queue come from eframe's RenderState (created by eframe for
//! the UI, reused here for compute).

use super::Renderer;
use eframe::wgpu::{
    self, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
    Buffer, BufferBindingType, BufferDescriptor, BufferUsages, ComputePipeline, Device, MapMode,
    Queue, ShaderStages,
};
use std::num::NonZeroU64;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

/// Maximum number of u32 pixels that can be calculated by the compute shader.
const RENDERER_MAX_PIXELS: usize = 524288;
/// Workgroup size defined in the shader.
const RENDERER_WORKGROUP_SIZE: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    /// Number of points in the trace buffer the shader must render.
    chunk_samples: u32,
    /// The real number of available samples in the trace. Most of the time it is equal to
    /// chunk_samples, excepted on the last rendered tile.
    trace_samples: u32,
    pixel_count: u32,
    /// Rendered chunk width.
    w: u32,
    /// Rendered chunk height.
    h: u32,
    /// Y-axis scaling coefficient.
    scale_y: f32,
    /// Y offset.
    /// This value is added to the trace samples before rendering.
    offset: f32,
}

struct PendingGpuWork {
    pixel_count: usize,
    map_ready: Arc<AtomicBool>,
}

pub struct AsyncGpuRenderer {
    /// Connection to the compute device.
    device: Device,
    /// Processing queue.
    queue: Queue,
    /// Buffer storing trace data, accessed by the compute shader.
    input_buffer: Buffer,
    /// Compute shader result buffer.
    output_buffer: Buffer,
    /// Result buffer copied from GPU to CPU.
    download_output_buffer: Buffer,
    /// Buffer for the shader parameters.
    params_buffer: Buffer,
    /// Compute pipeline.
    pipeline: ComputePipeline,
    /// Shader data binding.
    bind_group: BindGroup,
    /// Tracks in-flight GPU work for the non-blocking submit/poll path.
    pending: Mutex<Option<PendingGpuWork>>,
}

impl AsyncGpuRenderer {
    /// Creates a renderer that reuses eframe's wgpu device.
    /// Returns `None` when the adapter does not support compute shaders.
    pub fn from_render_state(rs: &eframe::egui_wgpu::RenderState) -> Option<Self> {
        let downlevel = rs.adapter.get_downlevel_capabilities();
        if !downlevel
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return None;
        }
        Some(Self::with_device_queue(rs.device.clone(), rs.queue.clone()))
    }

    fn with_device_queue(device: Device, queue: Queue) -> Self {
        let trace_buffer_size = (super::RENDERER_MAX_TRACE_SIZE * 4) as u64;
        let pixel_buffer_size = (RENDERER_MAX_PIXELS * 4) as u64;

        let input_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("gpu_input"),
            size: trace_buffer_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let output_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("gpu_output"),
            size: pixel_buffer_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let download_output_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("gpu_download_output"),
            size: pixel_buffer_size,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let params_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("gpu_params"),
            size: size_of::<Params>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("../shader.wgsl"));

        let (_layout, pipeline, bind_group) =
            create_compute_pipeline(&device, &shader, &input_buffer, &output_buffer, &params_buffer);

        Self {
            device,
            queue,
            input_buffer,
            output_buffer,
            download_output_buffer,
            params_buffer,
            pipeline,
            bind_group,
            pending: Mutex::new(None),
        }
    }

    /// Returns `true` when a GPU render is in flight.
    pub fn is_busy(&self) -> bool {
        self.pending.lock().unwrap().is_some()
    }

    // -- internal helpers used by both blocking and non-blocking paths --------

    fn submit_work(
        &self,
        chunk_samples: u32,
        trace: &[f32],
        w: u32,
        h: u32,
        offset: f32,
        scale_y: f32,
    ) {
        debug_assert!(trace.len() >= 2);
        debug_assert!(trace.len() <= super::RENDERER_MAX_TRACE_SIZE);
        debug_assert!(!self.is_busy());

        self.queue
            .write_buffer(&self.input_buffer, 0, bytemuck::cast_slice(trace));

        let pixel_count = w * h;
        let params = Params {
            chunk_samples,
            trace_samples: trace.len() as u32,
            pixel_count,
            w,
            h,
            scale_y,
            offset,
        };
        self.queue
            .write_buffer(&self.params_buffer, 0, bytemuck::cast_slice(&[params]));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(pixel_count.div_ceil(RENDERER_WORKGROUP_SIZE as u32), 1, 1);
        }

        encoder.copy_buffer_to_buffer(
            &self.output_buffer,
            0,
            &self.download_output_buffer,
            0,
            (pixel_count * 4) as u64,
        );

        self.queue.submit([encoder.finish()]);

        let ready = Arc::new(AtomicBool::new(false));
        let flag = ready.clone();
        self.download_output_buffer
            .slice(..)
            .map_async(MapMode::Read, move |_| {
                flag.store(true, Ordering::SeqCst);
            });

        *self.pending.lock().unwrap() = Some(PendingGpuWork {
            pixel_count: pixel_count as usize,
            map_ready: ready,
        });
    }

    fn try_collect(&self) -> Option<Vec<u32>> {
        let pixel_count = {
            let pending = self.pending.lock().unwrap();
            let p = pending.as_ref()?;
            if !p.map_ready.load(Ordering::SeqCst) {
                return None;
            }
            p.pixel_count
        };

        let slice = self.download_output_buffer.slice(..);
        let data = slice.get_mapped_range();
        let data_u32: &[u32] = bytemuck::cast_slice(&data);
        let result = data_u32[..pixel_count].to_vec();
        drop(data);
        self.download_output_buffer.unmap();

        *self.pending.lock().unwrap() = None;
        Some(result)
    }

    // -- public non-blocking API (used by the web main-loop) -----------------

    /// Uploads trace data, dispatches the compute shader and starts an async
    /// read-back.  Returns immediately.
    pub fn submit(
        &self,
        chunk_samples: u32,
        trace: &[f32],
        w: u32,
        h: u32,
        offset: f32,
        scale_y: f32,
    ) {
        self.submit_work(chunk_samples, trace, w, h, offset, scale_y);
    }

    /// Non-blocking check: drives the GPU event queue forward and returns the
    /// rendered tile data when ready.
    pub fn poll_result(&self) -> Option<Vec<u32>> {
        let _ = self.device.poll(wgpu::PollType::Poll);
        self.try_collect()
    }
}

/// Blocking interface — used by native background threads via
/// `Box<dyn Renderer>`.  On web this would never return because
/// `device.poll(Wait)` is a no-op and the callback can only fire from the
/// browser event loop, but web never calls this path.
impl Renderer for AsyncGpuRenderer {
    fn render(
        &self,
        chunk_samples: u32,
        trace: &[f32],
        w: u32,
        h: u32,
        offset: f32,
        scale_y: f32,
    ) -> Vec<u32> {
        self.submit_work(chunk_samples, trace, w, h, offset, scale_y);
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        self.try_collect()
            .expect("GPU work should be complete after blocking poll")
    }
}

// ---------------------------------------------------------------------------
// Compute pipeline setup
// ---------------------------------------------------------------------------

fn create_compute_pipeline(
    device: &Device,
    shader: &wgpu::ShaderModule,
    input_buffer: &Buffer,
    output_buffer: &Buffer,
    params_buffer: &Buffer,
) -> (wgpu::BindGroupLayout, ComputePipeline, BindGroup) {
    let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("bind_group_layout"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    min_binding_size: Some(NonZeroU64::new(4).unwrap()),
                    has_dynamic_offset: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: false },
                    min_binding_size: Some(NonZeroU64::new(4).unwrap()),
                    has_dynamic_offset: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(NonZeroU64::new(size_of::<Params>() as u64).unwrap()),
                },
                count: None,
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: shader,
        entry_point: Some("render"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bind_group"),
        layout: &bind_group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: input_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: output_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: params_buffer.as_entire_binding(),
            },
        ],
    });

    (bind_group_layout, pipeline, bind_group)
}
