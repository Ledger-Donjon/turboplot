use sci_rs::signal::filter::design::{
    BesselThomsonNorm, DigitalFilter, FilterBandType, FilterOutputType, FilterType, iirfilter_dyn,
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
fn filter_type_name<'a>(filter_type: FilterType) -> &'a str {
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
fn filter_band_type_name<'a>(filter_band_type: FilterBandType) -> &'a str {
    match filter_band_type {
        FilterBandType::Lowpass => "Low pass",
        FilterBandType::Highpass => "High pass",
        FilterBandType::Bandpass => "Band pass",
        FilterBandType::Bandstop => "Band stop",
    }
}

/// A struct that encapsulates filter design parameters and state for a filter design dialog.
///
/// `FilterDesigner` provides a way to configure and manage settings for designing digital filters,
/// including filter type, band type, order, frequency specifications, and dialog state.
/// It also stores the last error encountered during filter design for user feedback.
pub struct FilterDesigner {
    filter_band_type: FilterBandType,
    filter_type: FilterType,
    filter_order: u32,
    filter_f1: f32,
    filter_f2: f32,
    filter_pass: f32,
    filter_stop: f32,
    is_open: bool,
    last_error: Option<String>,
}

impl FilterDesigner {
    pub fn new() -> Self {
        Self {
            filter_band_type: FilterBandType::Lowpass,
            filter_type: FilterType::Butterworth,
            filter_order: 4,
            filter_f1: 0.0,
            filter_f2: 0.0,
            filter_pass: 0.5,
            filter_stop: 60.0,
            is_open: false,
            last_error: None,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Displays a modal dialog for designing a digital filter
    ///
    /// Note: filters coefficients are normalized using the sampling rate, so the frequency values
    /// should be in MHz.
    ///
    /// # Arguments
    ///
    /// * `ctx` - The egui context.
    /// * `fs` - The sampling rate in MHz.
    ///
    /// # Returns
    ///
    /// * An `Option` containing the resulting `DigitalFilter<f32>` if the user clicks "Apply filter",
    ///   or `None` if the user clicks "Cancel" or closes the modal.
    pub fn ui_design_filter(&mut self, ctx: &egui::Context, fs: f32) -> Option<DigitalFilter<f32>> {
        if !self.is_open {
            return None;
        }
        let mut result = None;

        let modal = egui::Modal::new(egui::Id::new("Create filter"));
        modal.show(ctx, |ui| {
            ui.heading("Filter Designer");
            ui.add_space(16.0);
            ui.label(format!("Sampling rate:  {} MHz", fs));
            ui.add_space(8.0);

            egui::Grid::new("filter_grid").show(ui, |ui| {
                ui.label("Filter type:");
                egui::ComboBox::from_id_salt(egui::Id::new("filter_band_type"))
                    .selected_text(filter_band_type_name(self.filter_band_type))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.filter_band_type,
                            FilterBandType::Lowpass,
                            "Low pass",
                        );
                        ui.selectable_value(
                            &mut self.filter_band_type,
                            FilterBandType::Highpass,
                            "High pass",
                        );
                        ui.selectable_value(
                            &mut self.filter_band_type,
                            FilterBandType::Bandpass,
                            "Band pass",
                        );
                        ui.selectable_value(
                            &mut self.filter_band_type,
                            FilterBandType::Bandstop,
                            "Band stop",
                        );
                    });
                egui::ComboBox::from_id_salt(egui::Id::new("filter_type"))
                    .selected_text(filter_type_name(self.filter_type))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.filter_type,
                            FilterType::Butterworth,
                            "Butterworth",
                        );
                        ui.selectable_value(
                            &mut self.filter_type,
                            FilterType::ChebyshevI,
                            "Chebyshev I",
                        );
                        ui.selectable_value(
                            &mut self.filter_type,
                            FilterType::ChebyshevII,
                            "Chebyshev II",
                        );
                        ui.selectable_value(
                            &mut self.filter_type,
                            FilterType::CauerElliptic,
                            "Cauer Elliptic",
                        );
                        ui.selectable_value(
                            &mut self.filter_type,
                            FilterType::BesselThomson(BesselThomsonNorm::Delay),
                            "Bessel Thomson",
                        );
                    });
                ui.end_row();

                ui.label("Order:");
                ui.add(
                    egui::DragValue::new(&mut self.filter_order)
                        .range(0..=16)
                        .speed(0.05),
                );
                ui.end_row();

                ui.label("F1:");
                ui.add(
                    egui::DragValue::new(&mut self.filter_f1)
                        .range(0.0..=fs / 2.0f32)
                        .speed(1.0)
                        .suffix(" MHz"),
                );
                ui.label("F2:");
                ui.add(
                    egui::DragValue::new(&mut self.filter_f2)
                        .range(0.0..=fs / 2.0f32)
                        .speed(1.0)
                        .suffix(" MHz"),
                );
                ui.end_row();

                ui.label("Pass:");
                ui.add(
                    egui::DragValue::new(&mut self.filter_pass)
                        .range(0.0..=1.0)
                        .speed(0.005)
                        .suffix(" dB"),
                );
                ui.label("Stop:");
                ui.add(
                    egui::DragValue::new(&mut self.filter_stop)
                        .range(0.0..=100.0)
                        .speed(0.2)
                        .suffix(" dB"),
                );
                ui.end_row();
            });

            if let Some(err) = &self.last_error {
                ui.add_space(6.0);
                ui.colored_label(egui::Color32::RED, err);
            }

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            egui::Sides::new().show(
                ui,
                |_ui| {},
                |ui| {
                    if ui
                        .button(egui::RichText::new(" Cancel ").color(egui::Color32::RED))
                        .clicked()
                    {
                        self.last_error = None;
                        self.is_open = false;
                    }
                    if ui
                        .button(egui::RichText::new(" Apply filter ").color(egui::Color32::GREEN))
                        .clicked()
                    {
                        match self.build_filter(fs) {
                            Ok(f) => {
                                self.last_error = None;
                                result = Some(f);
                                self.is_open = false;
                            }
                            Err(msg) => {
                                self.last_error = Some(msg.to_string());
                            }
                        }
                    }
                },
            );
        });

        result
    }

    /// Verifies and builds a digital filter based on the current filter design parameters.
    ///
    /// # Arguments
    ///
    /// * `fs` - The sampling rate in MHz.
    ///
    /// # Returns
    ///
    /// * A `Result` containing the resulting `DigitalFilter<f32>` if the filter is valid,
    ///   or an error message if the filter is invalid.
    fn build_filter<'a>(&self, fs: f32) -> Result<DigitalFilter<f32>, &'a str> {
        if self.filter_order == 0 {
            return Err("Order must be >= 1");
        }

        // Nyquist frequency verification.
        let wn: Vec<f32> = match self.filter_band_type {
            FilterBandType::Lowpass => {
                if !(self.filter_f1 > 0.0 && self.filter_f1 < fs / 2.0) {
                    return Err("F1 must be in ]0, fs/2[ interval");
                }
                vec![self.filter_f1]
            }
            FilterBandType::Highpass => {
                if !(self.filter_f1 > 0.0 && self.filter_f1 < fs / 2.0) {
                    return Err("F1 must be in ]0, fs/2[ interval");
                }
                vec![self.filter_f1]
            }
            FilterBandType::Bandpass | FilterBandType::Bandstop => {
                let f0 = self.filter_f1.min(self.filter_f2);
                let f1 = self.filter_f1.max(self.filter_f2);
                if !(f0 > 0.0 && f1 < fs / 2.0 && f0 < f1) {
                    return Err("F1 and F2 must be in ]0, fs/2[ interval");
                }
                vec![f0, f1]
            }
        };

        // Pass and stop ripple verification depending on the filter type.
        match self.filter_type {
            FilterType::ChebyshevI => {
                if self.filter_pass <= 0.0 {
                    return Err("Pass ripple must be > 0 dB for Chebyshev I");
                }
            }
            FilterType::ChebyshevII => {
                if self.filter_stop <= 0.0 {
                    return Err("Stop attenuation must be > 0 dB for Chebyshev II");
                }
            }
            FilterType::CauerElliptic => {
                if self.filter_pass <= 0.0 || self.filter_stop <= 0.0 {
                    return Err("Pass ripple and Stop attenuation must be > 0 dB for Elliptic");
                }
            }
            FilterType::Butterworth | FilterType::BesselThomson(_) => {}
        };

        // iirfilter takes Wn and fs in the same units; we keep MHz across UI and fs.
        Ok(iirfilter_dyn::<f32>(
            self.filter_order as usize,
            wn,
            Some(self.filter_pass),
            Some(self.filter_stop),
            Some(self.filter_band_type),
            Some(self.filter_type),
            Some(false),
            Some(FilterOutputType::Sos),
            Some(fs),
        ))
    }
}
