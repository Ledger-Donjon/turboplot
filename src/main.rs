use crate::{
    filtering::Filtering,
    input::{Args, FileManager, FileManagerResult},
    loaders::{TraceFormat, guess_format, load_csv, load_npy},
    multi_viewer::MultiViewer,
};
use biquad::ToHertz;
use clap::Parser;
use eframe::egui;
use egui::Vec2;
use std::{fs::File, io::BufReader, sync::Arc};

mod camera;
mod filtering;
mod input;
mod loaders;
mod multi_viewer;
mod renderer;
mod sync_features;
mod tek_wfm;
mod tiling;
mod util;
mod viewer;

/// Application state: selecting files, viewing traces, or closing.
enum AppState {
    /// File selection state with the file manager.
    Selection(Box<FileManager>),
    /// Viewing state with the multi-viewer.
    Viewing(MultiViewer),
    /// Application is closing.
    Closing,
}

/// Main application wrapper that handles file selection and viewing states.
struct TurboPlotApp {
    state: AppState,
}

impl TurboPlotApp {
    fn new(ctx: &egui::Context, args: Args) -> Self {
        let state = if args.paths.is_empty() {
            // No files provided, show file manager
            AppState::Selection(Box::new(FileManager::new(args)))
        } else {
            // Files were provided via command line, load and go to viewing
            match Self::load_and_create_viewer(ctx, &args) {
                Some(viewer) => AppState::Viewing(viewer),
                None => {
                    // Failed to load, show file manager
                    AppState::Selection(Box::new(FileManager::new(args)))
                }
            }
        };

        Self { state }
    }

    /// Loads traces from args and creates a MultiViewer if successful.
    fn load_and_create_viewer(ctx: &egui::Context, args: &Args) -> Option<MultiViewer> {
        let (labels, traces) = Self::load_traces(args);
        if traces.is_empty() {
            return None;
        }

        println!(
            "Using {} GPU threads and {} CPU threads.",
            args.gpu,
            args.cpu_threads()
        );

        Some(MultiViewer::new(
            ctx,
            labels,
            Arc::new(traces),
            args.sampling_rate,
            args.gpu,
            args.cpu_threads(),
        ))
    }

    /// Loads traces from the given args. Returns (labels, traces) where labels
    /// may differ from the input paths when a single file produces multiple
    /// traces we call frames (e.g. multi-frame WFM or 2D numpy files).
    fn load_traces(args: &Args) -> (Vec<String>, Vec<Arc<Vec<f32>>>) {
        let mut labels = Vec::new();
        let mut traces = Vec::new();
        for path in &args.paths {
            let Some(format) = args.format.or_else(|| guess_format(path)) else {
                println!("Unrecognized file extension: {}", path);
                continue;
            };

            let file = match File::open(path) {
                Ok(f) => f,
                Err(e) => {
                    println!("Failed to open file {}: {}", path, e);
                    continue;
                }
            };
            let buf_reader = BufReader::new(file);

            // All loaders return Vec<Vec<f32>> (one or more traces per file)
            let mut frames = match format {
                TraceFormat::TekWfm => tek_wfm::load_tek_wfm(buf_reader, path),
                TraceFormat::Numpy => load_npy(buf_reader, path),
                TraceFormat::Csv => vec![load_csv(buf_reader, args.skip_lines, args.column)],
            };

            let n = frames.len();
            let selection = args.frame_selection();
            for (i, mut frame) in frames.drain(..).enumerate() {
                if let Some(ref sel) = selection {
                    if !sel.contains(&i) {
                        continue;
                    }
                }
                if let Some(filter) = args.filter {
                    frame.apply_filter(
                        filter,
                        args.sampling_rate.mhz(),
                        args.cutoff_freq.khz(),
                    );
                }
                if n > 1 {
                    labels.push(format!("{} [frame {}]", path, i));
                } else {
                    labels.push(path.clone());
                }
                traces.push(Arc::new(frame));
            }
        }
        (labels, traces)
    }
}

impl eframe::App for TurboPlotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match &mut self.state {
            AppState::Selection(file_manager) => match file_manager.update(ctx) {
                FileManagerResult::Selected(args) => {
                    // Load traces and transition to viewing state
                    if let Some(viewer) = Self::load_and_create_viewer(ctx, &args) {
                        self.state = AppState::Viewing(viewer);
                    }
                }
                FileManagerResult::Cancelled => {
                    // Transition to closing state
                    self.state = AppState::Closing;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                FileManagerResult::Pending => {}
            },
            AppState::Viewing(viewer) => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default().outer_margin(0.0))
                    .show(ctx, |ui| {
                        viewer.update(ctx, ui);
                    });
            }
            AppState::Closing => {
                // Do nothing, app is closing
            }
        }
    }
}

fn main() {
    let args = Args::parse();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        window_builder: Some(Box::new(|w| w.with_inner_size(Vec2::new(1280.0, 512.0)))),
        ..Default::default()
    };

    eframe::run_native(
        "TurboPlot",
        options,
        Box::new(move |cc| Ok(Box::new(TurboPlotApp::new(&cc.egui_ctx, args)))),
    )
    .unwrap();
}
