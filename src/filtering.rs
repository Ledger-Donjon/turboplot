use biquad::{Biquad, Coefficients, DirectForm1, Hertz, Q_BUTTERWORTH_F32, Type};
use serde::Serialize;

#[derive(Copy, Clone, Debug, Serialize, PartialEq, Eq)]
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

impl std::fmt::Display for Filter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Filter::LowPass => write!(f, "low-pass"),
            Filter::HighPass => write!(f, "high-pass"),
            Filter::BandPass => write!(f, "band-pass"),
            Filter::Notch => write!(f, "notch"),
        }
    }
}

impl std::str::FromStr for Filter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low-pass" => Ok(Filter::LowPass),
            "high-pass" => Ok(Filter::HighPass),
            "band-pass" => Ok(Filter::BandPass),
            "notch" => Ok(Filter::Notch),
            _ => Err(format!("unknown filter: {s}")),
        }
    }
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
