mod csv;
mod npy;
mod tek_wfm;

pub use csv::load_csv;
pub use npy::load_npy;
pub use tek_wfm::load_tek_wfm;

use std::path::Path;

/// Possible trace formats that TurboPlot is able to load.
#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum TraceFormat {
    Numpy,
    Csv,
    TekWfm,
}

/// How a 2D Numpy array should be interpreted.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, clap::ValueEnum)]
pub enum NpyLayout {
    /// Guess from the array shape: arrays with few columns and many rows are
    /// treated as column-wise (e.g. oscilloscope `(time, voltage)` dumps),
    /// everything else as row-wise.
    #[default]
    Auto,
    /// Shape `(pts, cols)`: one trace per column. Use `--column` to select
    /// which column to display.
    Columns,
    /// Shape `(n_traces, pts)`: one trace per row. Use `--frames` to select
    /// which traces to display.
    Rows,
}

/// Guess trace file format from its path extension.
pub fn guess_format(path: &str) -> Option<TraceFormat> {
    match Path::new(path)
        .extension()?
        .to_str()?
        .to_lowercase()
        .as_str()
    {
        "npy" => Some(TraceFormat::Numpy),
        "csv" => Some(TraceFormat::Csv),
        "wfm" => Some(TraceFormat::TekWfm),
        _ => None,
    }
}
