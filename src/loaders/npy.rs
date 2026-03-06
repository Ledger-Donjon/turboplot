use npyz::{DType, NpyFile};
use std::io::BufRead;

/// Reads all elements from an NpyFile into a Vec.
fn npy_to_vec<T: npyz::Deserialize, R: std::io::Read>(npy: NpyFile<R>) -> Vec<T> {
    npy.into_vec::<T>().unwrap()
}

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
        (npyz::TypeChar::Int, Some(1)) => {
            npy_to_vec::<i8, _>(npy).into_iter().map(|x| x as f32).collect()
        }
        (npyz::TypeChar::Int, Some(2)) => {
            npy_to_vec::<i16, _>(npy).into_iter().map(|x| x as f32).collect()
        }
        (npyz::TypeChar::Int, Some(4)) => {
            npy_to_vec::<i32, _>(npy).into_iter().map(|x| x as f32).collect()
        }
        (npyz::TypeChar::Uint, Some(1)) => {
            npy_to_vec::<u8, _>(npy).into_iter().map(|x| x as f32).collect()
        }
        (npyz::TypeChar::Uint, Some(2)) => {
            npy_to_vec::<u16, _>(npy).into_iter().map(|x| x as f32).collect()
        }
        (npyz::TypeChar::Uint, Some(4)) => {
            npy_to_vec::<u32, _>(npy).into_iter().map(|x| x as f32).collect()
        }
        (npyz::TypeChar::Float, Some(4)) => npy_to_vec::<f32, _>(npy),
        (npyz::TypeChar::Float, Some(8)) => {
            npy_to_vec::<f64, _>(npy).into_iter().map(|x| x as f32).collect()
        }
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
