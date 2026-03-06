use muscat::util::read_array1_from_npy_file;
use npyz::{DType, NpyFile};
use std::io::BufRead;

/// Load a numpy file as one or more traces.
///
/// Supports 1D arrays (single trace) and 2D arrays (one trace per row).
/// Data type is automatically cast to `f32`.
pub fn load_npy<R: BufRead>(reader: R, path: &str) -> Vec<Vec<f32>> {
    let npy = NpyFile::new(reader).expect("Failed to parse numpy file");
    let shape = npy.shape().to_vec();
    let dtype_descr = npy.dtype().descr();

    let DType::Plain(dtype) = npy.dtype().clone() else {
        panic!("Invalid numpy data type")
    };

    let flat: Vec<f32> = match (dtype.type_char(), dtype.num_bytes()) {
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
    };

    match shape.len() {
        1 => {
            println!("{}: NumPy {}, {} pts", path, dtype_descr, flat.len());
            vec![flat]
        }
        2 => {
            let n_traces = shape[0] as usize;
            let pts = shape[1] as usize;
            println!(
                "{}: NumPy {}, {} trace(s), {} pts/trace",
                path, dtype_descr, n_traces, pts
            );
            flat.chunks_exact(pts).map(|c| c.to_vec()).collect()
        }
        _ => panic!("Unsupported numpy array dimension: {:?}", shape),
    }
}
