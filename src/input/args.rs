//! Command-line arguments and configuration.

use crate::filtering::Filter;
use crate::loaders::TraceFormat;
use clap::Parser;
use std::collections::HashSet;
use std::thread::available_parallelism;

/// TurboPlot is a blazingly fast waveform renderer made for visualizing huge traces.
///
/// Arguments for loading and displaying traces.
/// These can be provided via CLI or modified through the file manager UI.
#[derive(Parser, Clone)]
#[command(about, version)]
pub struct Args {
    /// Data file paths.
    #[arg(required = false, num_args = 0..)]
    pub paths: Vec<String>,

    /// Trace sampling rate in MS/s. Default to 125MS/s
    #[arg(long, short, default_value_t = 125.0f32)]
    pub sampling_rate: f32,

    /// Specify a digital filter.
    #[arg(long, requires("cutoff_freq"), value_enum)]
    pub filter: Option<Filter>,

    /// Cutoff frequency in kHz if a filter has been specified.
    #[arg(long, requires("filter"), default_value_t = 1000.0f32)]
    pub cutoff_freq: f32,

    /// Trace file format. If not specified, TurboPlot will guess from file extension.
    #[arg(long, short)]
    pub format: Option<TraceFormat>,

    /// When loading a CSV file, how many lines must be skipped before reading the values.
    #[arg(long, default_value_t = 0)]
    pub skip_lines: usize,

    /// When loading a CSV file, this is the index of the column storing the trace values. Index
    /// starts at zero.
    #[arg(long, default_value_t = 0)]
    pub column: usize,

    /// Number of GPU rendering threads to spawn.
    #[arg(long, short, default_value_t = 1)]
    pub gpu: usize,

    /// Number of CPU rendering threads to spawn. If not specified, TurboPlot will spawn as many
    /// threads as the CPU can run simultaneously.
    #[arg(long, short)]
    pub cpu: Option<usize>,

    /// For Tektronix WFM FastFrame files, select which frames to load.
    /// Format: comma-separated indices or ranges, e.g. "1-3,6,7-8,12".
    /// If not specified, all frames are loaded.
    #[arg(long)]
    pub frames: Option<String>,
}

impl Args {
    /// Returns the number of CPU threads to use, resolving the default if not specified.
    pub fn cpu_threads(&self) -> usize {
        self.cpu.unwrap_or_else(|| {
            available_parallelism()
                .map(|x| x.get())
                .unwrap_or_else(|_| {
                    println!("Warning: failed to query available parallelism.");
                    1
                })
        })
    }

    /// Parses the `--frames` argument into a set of frame indices.
    /// Returns `None` if `--frames` was not specified (meaning all frames).
    pub fn frame_selection(&self) -> Option<HashSet<usize>> {
        let spec = self.frames.as_ref()?;
        let mut set = HashSet::new();
        for part in spec.split(',') {
            let part = part.trim();
            if let Some((start, end)) = part.split_once('-') {
                let start: usize = start
                    .trim()
                    .parse()
                    .unwrap_or_else(|_| panic!("Invalid frame range start: '{}'", start.trim()));
                let end: usize = end
                    .trim()
                    .parse()
                    .unwrap_or_else(|_| panic!("Invalid frame range end: '{}'", end.trim()));
                assert!(start <= end, "Invalid frame range: {}-{}", start, end);
                set.extend(start..=end);
            } else {
                let idx: usize = part
                    .parse()
                    .unwrap_or_else(|_| panic!("Invalid frame index: '{}'", part));
                set.insert(idx);
            }
        }
        Some(set)
    }
}
