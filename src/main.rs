use crate::{
    filtering::Filtering,
    loaders::{TraceFormat, guess_format, load_csv, load_npy},
    multi_viewer::MultiViewer,
    renderer::{CpuRenderer, GpuRenderer, Renderer},
    tiling::{Tiling, TilingRenderer},
};
use biquad::ToHertz;
use clap::Parser;
use eframe::egui;
use egui::Vec2;
use std::{
    fs::File,
    io::BufReader,
    sync::{Arc, Condvar, Mutex},
    thread::{self, available_parallelism},
};

mod camera;
mod filtering;
mod loaders;
mod multi_viewer;
mod renderer;
mod sync_features;
mod tiling;
mod util;
mod viewer;

/// TurboPlot is a blazingly fast waveform renderer made for visualizing huge traces.
#[derive(Parser)]
struct Args {
    /// Data file paths.
    #[arg(required = true, num_args = 1..)]
    paths: Vec<String>,
    /// Trace sampling rate in MS/s. Default to 100MS/s
    #[arg(long, short, default_value_t = 100.0f32)]
    sampling_rate: f32,
    /// Specify a digital filter.
    #[arg(long, requires("cutoff_freq"), value_enum)]
    filter: Option<filtering::Filter>,
    /// Cutoff frequency in kHz if a filter has been specified.
    #[arg(long, requires("filter"))]
    cutoff_freq: Option<f32>,
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
    for path in &args.paths {
        let Some(format) = args.format.or_else(|| guess_format(path)) else {
            println!("Unrecognized file extension. Please specify trace format.");
            return;
        };

        let file = File::open(path).expect("Failed to open file");
        let buf_reader = BufReader::new(file);

        let mut trace = match format {
            TraceFormat::Numpy => load_npy(buf_reader),
            TraceFormat::Csv => load_csv(buf_reader, args.skip_lines, args.column),
        };

        if let Some(filter) = args.filter {
            trace.apply_filter(
                filter,
                args.sampling_rate.mhz(),
                args.cutoff_freq.unwrap().khz(),
            )
        }

        traces.push(trace);
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
                &args.paths,
                &traces,
                args.sampling_rate,
            )))
        }),
    )
    .unwrap();
}
