//! Application configuration.

use crate::filtering::Filter;
use crate::loaders::TraceFormat;
use std::collections::HashSet;

/// TurboPlot is a blazingly fast waveform renderer made for visualizing huge traces.
///
/// Arguments for loading and displaying traces.
/// These can be provided via CLI or modified through the file manager UI.
#[derive(Clone)]
pub struct Args {
    /// Data file paths.
    pub paths: Vec<String>,

    /// Trace sampling rate in MS/s. Default to 125MS/s.
    pub sampling_rate: f32,

    /// Specify a digital filter.
    pub filter: Option<Filter>,

    /// Cutoff frequency in kHz if a filter has been specified.
    pub cutoff_freq: f32,

    /// Trace file format. If not specified, TurboPlot will guess from file extension.
    pub format: Option<TraceFormat>,

    /// When loading a CSV file, how many lines must be skipped before reading the values.
    pub skip_lines: usize,

    /// When loading a CSV file, this is the index of the column storing the trace values. Index
    /// starts at zero.
    pub column: usize,

    /// Number of GPU rendering threads to spawn.
    pub gpu: usize,

    /// Number of CPU rendering threads to spawn. If not specified, TurboPlot will spawn as many
    /// threads as the CPU can run simultaneously.
    pub cpu: Option<usize>,

    /// For files that contain multiple traces, select which traces to load.
    /// Format: comma-separated indices or ranges, e.g. "1-3,6,7-8,12".
    /// If not specified, all frames are loaded.
    pub frames: Option<String>,
}

impl Args {
    /// Returns the number of CPU threads to use, resolving the default if not specified.
    pub fn cpu_threads(&self) -> usize {
        self.cpu.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|x| x.get())
                .unwrap_or(1)
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

impl Default for Args {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            sampling_rate: 125.0,
            filter: None,
            cutoff_freq: 1000.0,
            format: None,
            skip_lines: 0,
            column: 0,
            gpu: 1,
            cpu: None,
            frames: None,
        }
    }
}

/// Parse URL query parameters into [`Args`] and an optional auto-fetch URL.
#[cfg(target_arch = "wasm32")]
pub fn parse_url_params() -> (Args, Option<String>) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return (Args::default(), None),
    };

    let search = match window.location().search() {
        Ok(s) => s,
        Err(_) => return (Args::default(), None),
    };

    let params = match web_sys::UrlSearchParams::new_with_str(&search) {
        Ok(p) => p,
        Err(_) => return (Args::default(), None),
    };

    let mut args = Args::default();

    if let Some(v) = params.get("sampling_rate") {
        if let Ok(sr) = v.parse::<f32>() {
            args.sampling_rate = sr;
        }
    }

    if let Some(v) = params.get("format") {
        if let Ok(f) = v.parse::<TraceFormat>() {
            args.format = Some(f);
        }
    }

    if let Some(v) = params.get("filter") {
        if let Ok(f) = v.parse::<Filter>() {
            args.filter = Some(f);
        }
    }

    if let Some(v) = params.get("cutoff_freq") {
        if let Ok(cf) = v.parse::<f32>() {
            args.cutoff_freq = cf;
        }
    }

    if let Some(v) = params.get("skip_lines") {
        if let Ok(sl) = v.parse::<usize>() {
            args.skip_lines = sl;
        }
    }

    if let Some(v) = params.get("column") {
        if let Ok(c) = v.parse::<usize>() {
            args.column = c;
        }
    }

    if let Some(v) = params.get("frames") {
        args.frames = Some(v);
    }

    let auto_url = params.get("url");
    (args, auto_url)
}
