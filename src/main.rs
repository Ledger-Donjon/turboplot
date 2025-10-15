use crate::{
    loaders::{TraceFormat, guess_format, load_csv, load_npy},
    renderer::{CpuRenderer, GpuRenderer, Renderer},
    tiling::{Tiling, TilingRenderer},
    viewer::{SyncOptions, Viewer},
};
use clap::Parser;
use eframe::{App, egui};
use egui::{Rect, Vec2, pos2};
use std::{
    fs::File,
    io::BufReader,
    sync::{Arc, Condvar, Mutex},
    thread::{self, available_parallelism},
};

mod camera;
mod loaders;
mod renderer;
mod tiling;
mod util;
mod viewer;

/// TurboPlot is a blazingly fast waveform renderer made for visualizing huge traces.
#[derive(Parser)]
struct Args {
    /// Data file paths.
    #[arg(required = true, num_args = 1..)]
    paths: Vec<String>,
    /// Trace file format. If not specified, TurboPlot will guess from file extension.
    #[arg(long, short)]
    format: Option<TraceFormat>,
    /// When loading a CSV file, how many lines must be skipped before reading the values.
    #[arg(long, default_value_t = 0)]
    skip_lines: usize,
    /// When loading a CSV file, this is the index of the column storing the trace values. Index
    /// starts at zero.
    #[arg(long, default_value_t = 0)]
    column: usize,
    /// Number of GPU rendering threads to spawn.
    #[arg(long, short, default_value_t = 1)]
    gpu: usize,
    /// Number of CPU rendering threads to spawn. If not specified, TurboPlot will spawn as many
    /// thread as the CPU can run simultaneously.
    #[arg(long, short)]
    cpu: Option<usize>,
}

fn main() {
    let args = Args::parse();

    let mut traces = Vec::new();
    for path in args.paths {
        let Some(format) = args.format.or_else(|| guess_format(&path)) else {
            println!("Unrecognized file extension. Please specify trace format.");
            return;
        };

        let file = File::open(&path).expect("Failed to open file");
        let buf_reader = BufReader::new(file);

        traces.push(match format {
            TraceFormat::Numpy => load_npy(buf_reader),
            TraceFormat::Csv => load_csv(buf_reader, args.skip_lines, args.column),
        });
    }

    let shared_tiling = Arc::new((Mutex::new(Tiling::new()), Condvar::new()));
    let traces = Arc::new(traces);

    for _ in 0..args.gpu {
        let shared_tiling_clone = shared_tiling.clone();
        let trace_clone = traces.clone();
        thread::spawn(move || {
            let renderer: Box<dyn Renderer> = Box::new(GpuRenderer::new());
            TilingRenderer::new(shared_tiling_clone, &trace_clone, renderer).render_loop();
        });
    }

    let cpu_count =
        args.cpu.unwrap_or(
            available_parallelism()
                .map(|x| x.get())
                .unwrap_or_else(|_| {
                    println!("Warning: failed to query available parallelism.");
                    1
                }),
        );

    println!(
        "Using {} GPU threads and {} CPU threads.",
        args.gpu, cpu_count
    );

    for _ in 0..cpu_count {
        let shared_tiling_clone = shared_tiling.clone();
        let trace_clone = traces.clone();
        thread::spawn(move || {
            let renderer: Box<dyn Renderer> = Box::new(CpuRenderer::new());
            TilingRenderer::new(shared_tiling_clone, &trace_clone, renderer).render_loop();
        });
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        window_builder: Some(Box::new(|w| w.with_inner_size(Vec2::new(1280.0, 512.0)))),
        ..Default::default()
    };

    eframe::run_native(
        "TurboPlot",
        options,
        Box::new(|_cc| {
            Ok(Box::new(MultiViewer::new(
                &_cc.egui_ctx,
                shared_tiling,
                &traces,
            )))
        }),
    )
    .unwrap();
}

struct MultiViewer<'a> {
    viewers: Vec<Viewer<'a>>,
    sync: SyncOptions,
}

impl<'a> MultiViewer<'a> {
    pub fn new(
        ctx: &egui::Context,
        shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
        traces: &'a [Vec<f32>],
    ) -> Self {
        Self {
            viewers: traces
                .iter()
                .enumerate()
                .map(|(i, t)| Viewer::new(i as u32, ctx, shared_tiling.clone(), t))
                .collect(),
            sync: SyncOptions::new(),
        }
    }

    /// Copy settings from viewer number `index` to others.
    fn sync(&mut self, index: usize) {
        let source_camera = *self.viewers[index].get_camera();
        for viewer in self
            .viewers
            .iter_mut()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, viewer)| viewer)
        {
            let mut camera = *viewer.get_camera();
            if self.sync.shift_x {
                camera.shift.x = source_camera.shift.x;
            }
            if self.sync.shift_y {
                camera.shift.y = source_camera.shift.y;
            }
            if self.sync.scale_x {
                camera.scale.x = source_camera.scale.x;
            }
            if self.sync.scale_y {
                camera.scale.y = source_camera.scale.y;
            }
            viewer.set_camera(camera);
        }
    }
}

impl<'a> App for MultiViewer<'a> {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().outer_margin(0.0))
            .show(ctx, |ui| {
                let size = ui.available_size();
                let n = self.viewers.len();
                let h = size.y / n as f32;

                // Calculate the viewport for each viewer.
                // We need viewports for both update and paint.
                let viewports: Vec<_> = (0..self.viewers.len())
                    .map(|i| {
                        Rect::from_min_max(
                            pos2(0.0, i as f32 * h),
                            pos2(size.x, (i + 1) as f32 * h),
                        )
                    })
                    .collect();

                // Call update of each viewer, don't do the painting yet because we might change
                // viewer settings afterwards for synchronization.
                let status: Vec<_> = self
                    .viewers
                    .iter_mut()
                    .zip(viewports.iter())
                    .map(|(viewer, viewport)| viewer.update(ctx, ui, *viewport))
                    .collect();

                // If some viewer changes and synchronization is performed, we use this flag to
                // prevent other viewers to request tiles while dragging or zooming is not finished
                // yet.
                let mut allow_tile_requests_for_all = true;

                if self.sync.any() {
                    // Check if a viewer has changing camera settings
                    if let Some((sync_index, status)) =
                        status.iter().enumerate().find(|(_, status)| {
                            status.dragging_x || status.dragging_y || status.zooming
                        })
                    {
                        // dragging_x is not used here, it is ok to request for tiles when dragging
                        // along X-axis. Since the scale does not change, only missing tiles on the
                        // left or right will be requested, which is not heavy.
                        allow_tile_requests_for_all &= !status.zooming && !status.dragging_y;
                        // Viewer number sync_index has changed, we must copy settings to others.
                        self.sync(sync_index);
                    }
                }

                // Paint all toolbars first: if we detect that synchronization is turned on we have
                // to perform sync before painting waveforms.
                let mut sync_index = None;
                for (index, (viewer, viewport)) in
                    self.viewers.iter_mut().zip(viewports.iter()).enumerate()
                {
                    let prev_sync = self.sync;
                    viewer.paint_toolbar(
                        ctx,
                        if n > 1 { Some(&mut self.sync) } else { None },
                        *viewport,
                    );

                    if (!prev_sync & self.sync).any() {
                        // One option has been enabled.
                        sync_index = Some(index);
                    }
                }

                if let Some(sync_index) = sync_index {
                    self.sync(sync_index)
                }

                // Now that all viewers have been updated and synchronized, we can paint them.
                for ((viewer, viewport), status) in self
                    .viewers
                    .iter_mut()
                    .zip(viewports.iter())
                    .zip(status.iter())
                {
                    let allow_tile_requests = !status.zooming && !status.dragging_y;
                    viewer.paint_waveform(
                        ctx,
                        ui,
                        *viewport,
                        allow_tile_requests && allow_tile_requests_for_all,
                    );
                }
            });
    }
}
