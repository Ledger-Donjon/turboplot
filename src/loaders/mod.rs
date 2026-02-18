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
