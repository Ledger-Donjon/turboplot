mod csv;
mod npy;
mod tek_wfm;

pub use csv::load_csv;
pub use npy::load_npy;
pub use tek_wfm::load_tek_wfm;

use std::path::Path;

/// Possible trace formats that TurboPlot is able to load.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TraceFormat {
    Numpy,
    Csv,
    TekWfm,
}

impl std::fmt::Display for TraceFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceFormat::Numpy => write!(f, "numpy"),
            TraceFormat::Csv => write!(f, "csv"),
            TraceFormat::TekWfm => write!(f, "tek-wfm"),
        }
    }
}

impl std::str::FromStr for TraceFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "numpy" | "npy" => Ok(TraceFormat::Numpy),
            "csv" => Ok(TraceFormat::Csv),
            "tek-wfm" | "wfm" => Ok(TraceFormat::TekWfm),
            _ => Err(format!("unknown format: {s}")),
        }
    }
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
