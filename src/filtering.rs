use biquad::{Biquad, Coefficients, DirectForm1, Hertz, Q_BUTTERWORTH_F32, Type};
use serde::Serialize;

#[derive(clap::ValueEnum, Copy, Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Digital filters supported by TurboPlot.
pub enum Filter {
    /// Low-pass filter
    LowPass,
    /// High-pass filter
    HighPass,
    /// Band-Pass filter
    BandPass,
    /// Notch filter
    Notch,
}

/// Converts CLI filters into biquad ones.
impl From<Filter> for Type<f32> {
    fn from(value: Filter) -> Self {
        match value {
            Filter::LowPass => Type::LowPass,
            Filter::HighPass => Type::HighPass,
            Filter::BandPass => Type::BandPass,
            Filter::Notch => Type::Notch,
        }
    }
}

/// Define an interface to apply filter on traces.
pub trait Filtering {
    fn apply_filter(&mut self, filter: Filter, fs: Hertz<f32>, f0: Hertz<f32>);
}

/// Extends Vec<f32> to support digital filters.
impl Filtering for Vec<f32> {
    fn apply_filter(&mut self, filter: Filter, fs: Hertz<f32>, f0: Hertz<f32>) {
        let coeffs =
            Coefficients::<f32>::from_params(filter.into(), fs, f0, Q_BUTTERWORTH_F32).unwrap();
        let mut biquad = DirectForm1::<f32>::new(coeffs);

        for x in self.iter_mut() {
            *x = biquad.run(*x);
        }
    }
}
