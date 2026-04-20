use crate::loaders::NpyLayout;
use muscat::util::read_array1_from_npy_file;
use npyz::{DType, NpyFile};
use std::io::BufRead;

/// Heuristic threshold on the inner dimension for auto-detection of a
/// column-wise 2D Numpy array. Anything with `cols <= N` is likely a
/// column-wise dump (e.g. `(time, voltage)` pairs from an oscilloscope) rather
/// than a stack of tiny traces.
const AUTO_COLUMNS_MAX_COLS: usize = 10;

/// Load a numpy file as one or more traces.
///
/// Supports 1D arrays (single trace) and 2D arrays. For 2D arrays, `layout`
/// controls how the shape is interpreted:
///
/// - [`NpyLayout::Auto`]: arrays with few columns and many rows are treated as
///   column-wise; all others as row-wise.
/// - [`NpyLayout::Columns`]: shape `(pts, n_traces)`; each column is a trace.
///   The array is transposed internally so the downstream behavior is
///   identical to [`NpyLayout::Rows`].
/// - [`NpyLayout::Rows`]: shape `(n_traces, pts)`; each row is a trace.
///
/// In every multi-trace case, `--frames` selects which traces to keep.
///
/// Data type is automatically cast to `f32`.
pub fn load_npy<R: BufRead>(reader: R, path: &str, layout: NpyLayout) -> Vec<Vec<f32>> {
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
            let rows = shape[0] as usize;
            let cols = shape[1] as usize;
            assert!(rows > 0 && cols > 0, "Empty 2D numpy array: {:?}", shape);
            assert_eq!(
                flat.len(),
                rows * cols,
                "Unexpected flat buffer size for shape {:?}",
                shape
            );

            // Single-row and single-column arrays collapse to one trace
            // regardless of layout.
            if cols == 1 {
                println!(
                    "{}: NumPy {}, 1 trace of {} pts (2D with single column)",
                    path, dtype_descr, rows
                );
                return vec![flat];
            }
            if rows == 1 {
                println!(
                    "{}: NumPy {}, 1 trace of {} pts (2D with single row)",
                    path, dtype_descr, cols
                );
                return vec![flat];
            }

            let resolved = match layout {
                NpyLayout::Auto => {
                    if cols <= AUTO_COLUMNS_MAX_COLS && rows > cols {
                        NpyLayout::Columns
                    } else {
                        NpyLayout::Rows
                    }
                }
                other => other,
            };

            let auto_note = if matches!(layout, NpyLayout::Auto) {
                " [auto]"
            } else {
                ""
            };

            match resolved {
                NpyLayout::Columns => {
                    // Transpose: each column becomes a trace. The result is
                    // `cols` traces of `rows` points each, which is exactly
                    // the shape `Rows` would produce for a `(cols, rows)`
                    // array, so the rest of the app treats both layouts
                    // uniformly.
                    println!(
                        "{}: NumPy {}, shape ({}, {}), column-wise{}: {} trace(s) of {} pts \
                         (transposed)",
                        path, dtype_descr, rows, cols, auto_note, cols, rows
                    );
                    let mut traces: Vec<Vec<f32>> =
                        (0..cols).map(|_| Vec::with_capacity(rows)).collect();
                    for row in flat.chunks_exact(cols) {
                        for (c, value) in row.iter().enumerate() {
                            traces[c].push(*value);
                        }
                    }
                    traces
                }
                NpyLayout::Rows => {
                    println!(
                        "{}: NumPy {}, shape ({}, {}), row-wise{}: {} trace(s) of {} pts",
                        path, dtype_descr, rows, cols, auto_note, rows, cols
                    );
                    flat.chunks_exact(cols).map(|c| c.to_vec()).collect()
                }
                NpyLayout::Auto => unreachable!("auto already resolved above"),
            }
        }
        _ => panic!("Unsupported numpy array dimension: {:?}", shape),
    }
}
