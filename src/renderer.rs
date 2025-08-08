use eframe::wgpu::{
    self, Backends, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferUsages, ComputePipeline,
    Device, Instance, InstanceDescriptor, MapMode, Queue, ShaderStages,
};
use std::{
    num::NonZeroU64,
    sync::{Arc, Mutex},
};

/// Maximum number of f32 trace points that can be sent to the GPU at once.
const RENDERER_MAX_TRACE_SIZE: usize = 8 * 1024 * 1024 * 4;
/// Maximum number of u32 pixels that can be calculated by the compute shader.
const RENDERER_MAX_PIXELS: usize = 524288;
/// Workgroup size defined in the shader.
const RENDERER_WORKGROUP_SIZE: usize = 64;

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

    pub fn render(&self, chunk_samples: u32, trace: &[f32], w: u32, h: u32, scale_y: f32) {
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
        compute_pass.dispatch_workgroups(workgroup_count as u32, 1, 1);
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
        };
        self.queue
            .write_buffer(&self.params_buffer, 0, bytemuck::cast_slice(&[params]));
        self.queue.submit([command_buffer]);
    }
}

/// Uniquely identifies a tile that can be rendered.
/// If any of this structure values changes, the corresponding render of this tile will be
/// different as well.
#[derive(Clone, Copy, PartialEq)]
pub struct TileProperties {
    /// Rendering X-axis scale.
    /// This is the number of samples for each pixel column.
    pub scale_x: f32,
    /// Rendering Y-axis scale.
    pub scale_y: f32,
    /// Index of the first sample in the trace for this tile.
    pub index: i32,
    /// Height in pixels of the tile.
    pub height: u32,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Camera {
    pub scale_x: f32,
    pub scale_y: f32,
    pub shift_x: f32,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            scale_x: 1000.0,
            shift_x: 0.0,
            scale_y: 1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TileStatus {
    NotRendered,
    Rendered,
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
}

pub struct Tiling {
    pub tiles: Vec<Tile>,
    height: u32,
}

impl Tiling {
    pub fn new() -> Self {
        Self {
            tiles: Vec::new(),
            height: 64,
        }
    }

    pub fn get(&mut self, properties: TileProperties) -> Tile {
        if let Some(tile) = self.tiles.iter().find(|x| x.properties == properties) {
            return tile.clone();
        }
        let tile = Tile::new(properties);
        self.tiles.push(tile.clone());
        return tile;
    }

    pub fn set_height(&mut self, height: u32) {
        if height != self.height {
            self.height = height;
            self.tiles.clear();
        }
    }
}

pub struct TilingRenderer {
    shared_tiling: Arc<Mutex<Tiling>>,
    trace: Vec<f32>,
    gpu_renderer: GpuRenderer,
    tile_width: u32,
}

impl TilingRenderer {
    pub fn new(shared_tiling: Arc<Mutex<Tiling>>, trace: Vec<f32>) -> Self {
        Self {
            shared_tiling,
            trace,
            gpu_renderer: GpuRenderer::new(),
            tile_width: 64,
        }
    }

    pub fn render_next_tile(&mut self) {
        let (height, Some(properties)) = ({
            let tiling = self.shared_tiling.lock().unwrap();
            if let Some(tile) = tiling
                .tiles
                .iter()
                .find(|x| x.status == TileStatus::NotRendered)
            {
                (tiling.height, Some(tile.properties.clone()))
            } else {
                (tiling.height, None)
            }
        }) else {
            return;
        };

        // We have a tile to be rendered
        let data = self.render_tile(
            properties.index,
            properties.scale_x,
            properties.scale_y,
            height,
        );
        // Save the result
        {
            let mut tiling = self.shared_tiling.lock().unwrap();
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
    }

    /// Renders the tile starting a sample `index` for the given scales `scale_x` and `scale_y`.
    pub fn render_tile(&mut self, index: i32, scale_x: f32, scale_y: f32, tile_height: u32) -> Vec<u32> {
        let trace_len = self.trace.len() as i32;
        let tile_w = self.tile_width;
        let i_start = (index as f32 * tile_w as f32 * scale_x).floor() as i32;
        let i_end = ((index + 1) as f32 * tile_w as f32 * scale_x).floor() as i32;

        if (i_start >= trace_len) || (i_start < 0) {
            let mut result = Vec::new();
            result.resize((self.tile_width * tile_height) as usize, 0);
            return result;
        }

        let trace_chunk = &self.trace[i_start as usize..i_end.min(trace_len) as usize];
        let mut result: Vec<u32> = Vec::new();
        if trace_chunk.len() == 0 {
            return result;
        }

        self.gpu_renderer
            .render((tile_w as f32 * scale_x) as u32, &trace_chunk, tile_w as u32, tile_height, scale_y);
        result.resize((tile_w * tile_height) as usize, 0);
        self.gpu_renderer.read_result(&mut result);
        result
    }
}
