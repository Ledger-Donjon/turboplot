use std::io::BufRead;

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
