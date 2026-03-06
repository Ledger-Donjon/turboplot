//! File manager GUI for selecting trace files.

use super::Args;
use crate::filtering::Filter;
use crate::loaders::TraceFormat;
use egui::{ComboBox, DragValue, TextEdit};
use egui_file_dialog::FileDialog;

/// Result of the file manager update.
pub enum FileManagerResult {
    /// No files selected yet, continue showing the dialog.
    Pending,
    /// Files were selected successfully, args contains the paths and settings.
    Selected(Args),
    /// Dialog was cancelled, close the app.
    Cancelled,
}

/// File manager GUI for selecting trace files.
pub struct FileManager {
    /// The file dialog.
    file_dialog: FileDialog,
    /// Arguments (editable by the user).
    args: Args,
    /// Text buffer for the frames input field.
    frames_text: String,
}

impl FileManager {
    /// Creates a new file manager with the given initial arguments.
    pub fn new(args: Args) -> Self {
        let mut file_dialog = FileDialog::new();
        file_dialog.pick_multiple();
        let frames_text = args.frames.clone().unwrap_or_default();
        Self {
            file_dialog,
            args,
            frames_text,
        }
    }

    /// Updates the file manager UI and returns the result.
    pub fn update(&mut self, ctx: &egui::Context) -> FileManagerResult {
        let state = self.file_dialog.state().clone();

        // Check if dialog was closed/cancelled
        match state {
            egui_file_dialog::DialogState::Cancelled | egui_file_dialog::DialogState::Closed => {
                return FileManagerResult::Cancelled;
            }
            _ => {}
        }

        // Update the dialog with a custom right panel for configuration
        self.file_dialog
            .update_with_right_panel_ui(ctx, &mut |ui, _dialog| {
                ui.add_space(10.0);
                ui.heading("Load Settings");
                ui.add_space(5.0);

                // Sampling rate
                ui.horizontal(|ui| {
                    ui.label("Sampling Rate:");
                    ui.add(
                        DragValue::new(&mut self.args.sampling_rate)
                            .suffix(" MS/s")
                            .range(1.0..=1000e9)
                            .speed(25.0),
                    );
                });

                // For CPU, we need a mutable value to edit
                let mut cpu_value = self.args.cpu.unwrap_or(self.args.cpu_threads());
                ui.horizontal(|ui| {
                    ui.label("CPU Threads:");
                    if ui
                        .add(DragValue::new(&mut cpu_value).range(1..=self.args.cpu_threads()))
                        .changed()
                    {
                        self.args.cpu = Some(cpu_value);
                    }
                })
                .response
                .on_hover_text(format!(
                    "Number of CPU rendering threads (max: {})",
                    self.args.cpu_threads()
                ));

                ui.add_space(5.0);

                ui.horizontal(|ui| {
                    ui.label("GPU Threads:");
                    ui.add(DragValue::new(&mut self.args.gpu).range(0..=16));
                })
                .response
                .on_hover_text("Number of GPU rendering threads (0 to disable GPU rendering)");

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                // Filter section
                ui.heading("Filter");
                ui.add_space(5.0);

                // Filter type selection
                let filter_label = match self.args.filter {
                    None => "None",
                    Some(Filter::LowPass) => "Low-pass",
                    Some(Filter::HighPass) => "High-pass",
                    Some(Filter::BandPass) => "Band-pass",
                    Some(Filter::Notch) => "Notch",
                };

                ComboBox::from_id_salt("filter_combo")
                    .selected_text(filter_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.args.filter, None, "None");
                        ui.selectable_value(
                            &mut self.args.filter,
                            Some(Filter::LowPass),
                            "Low-pass",
                        );
                        ui.selectable_value(
                            &mut self.args.filter,
                            Some(Filter::HighPass),
                            "High-pass",
                        );
                        ui.selectable_value(
                            &mut self.args.filter,
                            Some(Filter::BandPass),
                            "Band-pass",
                        );
                        ui.selectable_value(&mut self.args.filter, Some(Filter::Notch), "Notch");
                    });

                // Cutoff frequency (only show if filter is enabled)
                if self.args.filter.is_some() {
                    ui.add_space(5.0);
                    ui.label("Cutoff Frequency:");
                    ui.add(
                        DragValue::new(&mut self.args.cutoff_freq)
                            .suffix(" kHz")
                            .range(0.001..=1000e6)
                            .speed(10.0),
                    );
                }

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                // Format section
                ui.heading("File Format");
                ui.add_space(5.0);

                let format_label = match self.args.format {
                    None => "Auto",
                    Some(TraceFormat::Csv) => "CSV",
                    Some(TraceFormat::Numpy) => "NPY",
                    Some(TraceFormat::TekWfm) => "Tek WFM",
                };

                ComboBox::from_id_salt("format_combo")
                    .selected_text(format_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.args.format, None, "Auto");
                        ui.selectable_value(&mut self.args.format, Some(TraceFormat::Csv), "CSV");
                        ui.selectable_value(&mut self.args.format, Some(TraceFormat::Numpy), "NPY");
                        ui.selectable_value(
                            &mut self.args.format,
                            Some(TraceFormat::TekWfm),
                            "Tek WFM",
                        );
                    });

                // CSV-specific options (show if format is CSV or Auto)
                if matches!(self.args.format, None | Some(TraceFormat::Csv)) {
                    ui.add_space(10.0);
                    ui.label("CSV Options:");
                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        ui.label("Column:");
                        ui.add(DragValue::new(&mut self.args.column).range(0..=1000));
                    })
                    .response
                    .on_hover_text("Index of the column containing trace values starting from 0");

                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        ui.label("Skip lines:");
                        ui.add(DragValue::new(&mut self.args.skip_lines).range(0..=10000));
                    })
                    .response
                    .on_hover_text("Number of header lines to skip before reading values");
                }

                // Multi-trace options (show for formats that can contain multiple traces)
                if matches!(
                    self.args.format,
                    None | Some(TraceFormat::TekWfm) | Some(TraceFormat::Numpy)
                ) {
                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        ui.label("Traces indices:");
                        ui.add(
                            TextEdit::singleline(&mut self.frames_text)
                                .hint_text("all or 0-3,6,7-8,12")
                                .desired_width(120.0),
                        );
                    })
                    .response
                    .on_hover_text(
                        "Comma-separated indices or ranges, e.g. \"0-3,6,7-8,12\". Leave empty to load all traces.",
                    );
                }
            });

        if let Some(paths) = self.file_dialog.take_picked_multiple() {
            let paths_str: Vec<String> = paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();

            if !paths_str.is_empty() {
                let mut args = self.args.clone();
                args.paths = paths_str;
                let trimmed = self.frames_text.trim();
                args.frames = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
                return FileManagerResult::Selected(args);
            }
        }

        FileManagerResult::Pending
    }
}
