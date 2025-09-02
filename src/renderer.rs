use eframe::wgpu::{
    self, Backends, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferUsages, ComputePipeline,
    Device, Instance, InstanceDescriptor, MapMode, Queue, ShaderStages,
};
use std::num::NonZeroU64;

/// Maximum number of f32 trace points that can be sent to the GPU at once.
pub const RENDERER_MAX_TRACE_SIZE: usize = 8 * 1024 * 1024 * 4;
/// Maximum number of u32 pixels that can be calculated by the compute shader.
const RENDERER_MAX_PIXELS: usize = 524288;
/// Workgroup size defined in the shader.
const RENDERER_WORKGROUP_SIZE: usize = 64;

pub trait Renderer {
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

pub struct GpuRenderer {
    /// Connection to the compute device.
    device: Device,
    /// Processing queue.
    queue: Queue,
    /// Buffer storing trace data, written by the CPU and copied to the GPU in `input_buffer`.
    download_input_buffer: Buffer,
    /// Buffer storing trace data, accessed by the compute shader.
    input_buffer: Buffer,
    /// Compute shader result buffer.
    output_buffer: Buffer,
    /// Result buffer copied from GPU to CPU.
    download_output_buffer: Buffer,
    /// Buffer for the shader parameters
    params_buffer: Buffer,
    /// Compute pipeline
    pipeline: ComputePipeline,
    /// Shader data binding
    bind_group: BindGroup,
}

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

impl GpuRenderer {
    pub fn new() -> Self {
        let instance = Instance::new(&InstanceDescriptor::default());
        let mut adapters: Vec<_> = instance.enumerate_adapters(Backends::PRIMARY);
        // There can be multiple adapters, we don't want to select a Cpu adapter if a Gpu one is
        // available. We sort them and select the best.
        adapters.sort_by_key(|x| match x.get_info().device_type {
            wgpu::DeviceType::Other => 4,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::VirtualGpu => 3,
            wgpu::DeviceType::Cpu => 2,
        });
        let adapter = adapters[0].clone();
        println!("Running on Adapter: {:#?}", adapter.get_info());

        // Check that the adapter support compute shaders
        let downlevel_capabilities = adapter.get_downlevel_capabilities();
        if !downlevel_capabilities
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            panic!("Adapter does not support compute shaders");
        }

        // Create the device and processing queue.
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
            },
            None,
        ))
        .expect("Failed to create device");

        let trace_buffer_size = (RENDERER_MAX_TRACE_SIZE * 4) as u64;
        let pixel_buffer_size = (RENDERER_MAX_PIXELS * 4) as u64;

        let download_input_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("download_input_buffer"),
            size: trace_buffer_size,
            usage: BufferUsages::MAP_WRITE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let input_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("input_buffer"),
            size: trace_buffer_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let output_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("output_buffer"),
            size: pixel_buffer_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let download_output_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("download_output_buffer"),
            size: pixel_buffer_size,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let params_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("params_buffer"),
            size: size_of::<Params>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Load the compute shader
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        // Create the compute pipeline
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("bind_group_layout"),
            entries: &[
                // Input buffer
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        // This is the size of a single element in the buffer.
                        min_binding_size: Some(NonZeroU64::new(4).unwrap()),
                        has_dynamic_offset: false,
                    },
                    count: None,
                },
                // Output buffer
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: false },
                        // This is the size of a single element in the buffer.
                        min_binding_size: Some(NonZeroU64::new(4).unwrap()),
                        has_dynamic_offset: false,
                    },
                    count: None,
                },
                // Rendering parameters (trace length, chunck size...)
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(
                            NonZeroU64::new(size_of::<Params>() as u64).unwrap(),
                        ),
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
            module: &shader,
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

        Self {
            device,
            queue,
            download_input_buffer,
            input_buffer,
            output_buffer,
            download_output_buffer,
            params_buffer,
            pipeline,
            bind_group,
        }
    }

    /// Wait for the GPU to finish work that has been submitted.
    fn wait(&self) {
        self.device.poll(wgpu::Maintain::Wait);
    }

    /// Load trace data in the download input buffer.
    fn load_trace(&self, trace: &[f32]) {
        assert!(trace.len() <= RENDERER_MAX_TRACE_SIZE);
        let slice = self.download_input_buffer.slice(..);
        slice.map_async(MapMode::Write, |_| {});
        self.wait();
        let mut data = slice.get_mapped_range_mut();
        let data_f32 = bytemuck::cast_slice_mut(&mut data);
        data_f32[0..trace.len()].copy_from_slice(trace);
        drop(data);
        self.download_input_buffer.unmap();
    }

    /// Copy result buffer
    pub fn read_result(&self, dst: &mut [u32]) {
        let buffer_slice = self.download_output_buffer.slice(..);
        buffer_slice.map_async(MapMode::Read, |_| {});
        self.wait();
        let data = buffer_slice.get_mapped_range();
        let data_u32 = bytemuck::cast_slice(&data);
        dst.copy_from_slice(&data_u32[0..dst.len()]);
        drop(data);
        self.download_output_buffer.unmap();
    }
}

impl Renderer for GpuRenderer {
    fn render(
        &self,
        chunk_samples: u32,
        trace: &[f32],
        w: u32,
        h: u32,
        offset: f32,
        scale_y: f32,
    ) -> Vec<u32> {
        self.load_trace(trace);

        // The command encoder allows us to record commands that we will later submit to the GPU.
        let mut commands = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        commands.copy_buffer_to_buffer(
            &self.download_input_buffer,
            0,
            &self.input_buffer,
            0,
            (trace.len() * 4) as u64,
        );

        let mut compute_pass = commands.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);

        let pixel_count = w * h;
        let workgroup_count = pixel_count.div_ceil(RENDERER_WORKGROUP_SIZE as u32);
        compute_pass.dispatch_workgroups(workgroup_count, 1, 1);
        drop(compute_pass); // Get back access to commands encoder

        commands.copy_buffer_to_buffer(
            &self.output_buffer,
            0,
            &self.download_output_buffer,
            0,
            (pixel_count * 4) as u64,
        );

        let command_buffer = commands.finish();
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
        self.queue.submit([command_buffer]);

        let mut result = vec![0; (w * h) as usize];
        self.read_result(&mut result);
        result
    }
}

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
        let mut result = vec![0; (w * h) as usize];
        for i in 0..trace.len() - 1 {
            let x = ((i as u32 * w) / chunk_samples).min(w - 1);
            let p0 = trace[i] + offset;
            let p1 = trace[i + 1] + offset;
            let y0 = (h as i32 / 2) + (p0 * scale_y) as i32;
            let y1 = (h as i32 / 2) + (p1 * scale_y) as i32;
            for y in y0.min(y1)..=y0.max(y1) {
                result[(x as i32 * h as i32 + y) as usize] += 1
            }
        }
        result
    }
}
