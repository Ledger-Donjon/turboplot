#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use clap::Parser;
    use turboplot_lib::filtering::Filter;
    use turboplot_lib::input::Args;
    use turboplot_lib::loaders::TraceFormat;

    /// TurboPlot is a blazingly fast waveform renderer made for visualizing huge traces.
    #[derive(Parser)]
    #[command(about, version)]
    struct CliArgs {
        /// Data file paths.
        #[arg(required = false, num_args = 0..)]
        paths: Vec<String>,

        /// Trace sampling rate in MS/s. Default to 125MS/s
        #[arg(long, short, default_value_t = 125.0f32)]
        sampling_rate: f32,

        /// Specify a digital filter.
        #[arg(long, requires("cutoff_freq"), value_parser = clap::value_parser!(Filter))]
        filter: Option<Filter>,

        /// Cutoff frequency in kHz if a filter has been specified.
        #[arg(long, requires("filter"), default_value_t = 1000.0f32)]
        cutoff_freq: f32,

        /// Trace file format. If not specified, TurboPlot will guess from file extension.
        #[arg(long, short, value_parser = clap::value_parser!(TraceFormat))]
        format: Option<TraceFormat>,

        /// When loading a CSV file, how many lines must be skipped before reading the values.
        #[arg(long, default_value_t = 0)]
        skip_lines: usize,

        /// When loading a CSV file, this is the index of the column storing the trace values.
        /// Index starts at zero.
        #[arg(long, default_value_t = 0)]
        column: usize,

        /// Number of GPU rendering threads to spawn.
        #[arg(long, short, default_value_t = 1)]
        gpu: usize,

        /// Number of CPU rendering threads to spawn. If not specified, TurboPlot will spawn as
        /// many threads as the CPU can run simultaneously.
        #[arg(long, short)]
        cpu: Option<usize>,

        /// For files that contain multiple traces, select which traces to load.
        /// Format: comma-separated indices or ranges, e.g. "1-3,6,7-8,12".
        /// If not specified, all frames are loaded.
        #[arg(long)]
        frames: Option<String>,
    }

    impl From<CliArgs> for Args {
        fn from(cli: CliArgs) -> Self {
            Self {
                paths: cli.paths,
                sampling_rate: cli.sampling_rate,
                filter: cli.filter,
                cutoff_freq: cli.cutoff_freq,
                format: cli.format,
                skip_lines: cli.skip_lines,
                column: cli.column,
                gpu: cli.gpu,
                cpu: cli.cpu,
                frames: cli.frames,
            }
        }
    }

    turboplot_lib::run_native(CliArgs::parse().into());
}

#[cfg(target_arch = "wasm32")]
fn main() {}
