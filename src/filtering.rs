use sci_rs::signal::filter::{
    design::{BesselThomsonNorm, DigitalFilter, FilterBandType, FilterType},
    sosfiltfilt_dyn,
};

/// Returns the display name for a given `FilterType`.
///
/// # Arguments
///
/// * `filter_type` - A `FilterType` enum variant representing the type of filter.
///
/// # Returns
///
/// * A string slice representing the name of the filter type.
pub fn filter_type_name<'a>(filter_type: FilterType) -> &'a str {
    match filter_type {
        FilterType::Butterworth => "Butterworth",
        FilterType::ChebyshevI => "Chebyshev I",
        FilterType::ChebyshevII => "Chebyshev II",
        FilterType::CauerElliptic => "Cauer Elliptic",
        FilterType::BesselThomson(BesselThomsonNorm::Delay) => "Bessel Thomson",
        FilterType::BesselThomson(BesselThomsonNorm::Phase) => "Bessel Thomson",
        FilterType::BesselThomson(BesselThomsonNorm::Mag) => "Bessel Thomson",
    }
}

/// Returns the display name for a given `FilterBandType`.
///
/// # Arguments
///
/// * `filter_band_type` - A `FilterBandType` enum variant representing the filter band type.
///
/// # Returns
///
/// * A string slice representing the name of the filter band type.
pub fn filter_band_type_name<'a>(filter_band_type: FilterBandType) -> &'a str {
    match filter_band_type {
        FilterBandType::Lowpass => "Low pass",
        FilterBandType::Highpass => "High pass",
        FilterBandType::Bandpass => "Band pass",
        FilterBandType::Bandstop => "Band stop",
    }
}

/// Define an interface to apply filter on traces.
pub trait Filtering {
    fn apply_filter(&mut self, filter: DigitalFilter<f32>);
}

/// Extends Vec<f32> to support digital filters.
impl Filtering for Vec<f32> {
    fn apply_filter(&mut self, filter: DigitalFilter<f32>) {
        let DigitalFilter::Sos(sos) = filter else {
            panic!("Not SOS filter")
        };
        let filtered: Vec<f32> = sosfiltfilt_dyn(self.iter(), &sos.sos);
        *self = filtered;
    }
}
