use muscat::util::read_array1_from_npy_file;
use npyz::{DType, NpyFile};
use std::{io::BufRead, path::Path};

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

/// Load a 1D numpy file.
/// Data type is automatically casted to `f32`.
pub fn load_npy<R: BufRead>(reader: R) -> Vec<f32> {
    let npy = NpyFile::new(reader).expect("Failed to parse numpy file");
    println!("Numpy data type: {}", npy.dtype().descr());

    let DType::Plain(dtype) = npy.dtype().clone() else {
        panic!("Invalid numpy data type")
    };

    match (dtype.type_char(), dtype.num_bytes()) {
        (npyz::TypeChar::Int, Some(1)) => read_array1_from_npy_file(npy)
            .into_iter()
            .map(|x: i8| x as f32)
            .collect(),
        (npyz::TypeChar::Int, Some(2)) => read_array1_from_npy_file(npy)
            .into_iter()
            .map(|x: i16| x as f32)
            .collect(),
        (npyz::TypeChar::Int, Some(4)) => read_array1_from_npy_file(npy)
            .into_iter()
            .map(|x: i32| x as f32)
            .collect(),
        (npyz::TypeChar::Uint, Some(1)) => read_array1_from_npy_file(npy)
            .into_iter()
            .map(|x: u8| x as f32)
            .collect(),
        (npyz::TypeChar::Uint, Some(2)) => read_array1_from_npy_file(npy)
            .into_iter()
            .map(|x: u16| x as f32)
            .collect(),
        (npyz::TypeChar::Uint, Some(4)) => read_array1_from_npy_file(npy)
            .into_iter()
            .map(|x: u32| x as f32)
            .collect(),
        (npyz::TypeChar::Float, Some(4)) => read_array1_from_npy_file(npy).into_iter().collect(),
        (npyz::TypeChar::Float, Some(8)) => read_array1_from_npy_file(npy)
            .into_iter()
            .map(|x: f64| x as f32)
            .collect(),
        _ => panic!("Unsupported data type"),
    }
}

/// Loads a CSV file.
///
/// `skip` indicates how many lines must be skipped before starting to read the values.
/// `column` is the column number (starting from 0) containing the values.
pub fn load_csv<R: BufRead>(reader: R, skip: usize, column: usize) -> Vec<f32> {
    reader
        .lines()
        .skip(skip)
        .map(|l| {
            let line = l.unwrap();
            let value = line.split(",").nth(column).unwrap();
            value.parse::<f32>().unwrap()
        })
        .collect()
}
