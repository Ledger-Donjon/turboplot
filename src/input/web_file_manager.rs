//! Web file manager GUI: drag-and-drop or URL input with a settings panel.
//! Fetches trace files over HTTP when a URL is provided.

use super::loading::{self, FetchSlot, LoadResult, collect_frames, load_from_bytes, parse_traces};
use super::Args;
use crate::filtering::Filter;
use crate::loaders::{TraceFormat, guess_format};
use egui::{Color32, ComboBox, DragValue, DroppedFile, RichText, Stroke, TextEdit};
use std::sync::{Arc, Mutex};

enum WebState {
    Input,
    Fetching,
}

/// Web file manager with drag-and-drop and URL fetch support.
pub struct WebFileManager {
    args: Args,
    frames_text: String,
    url_text: String,
    error: Option<String>,
    state: WebState,
    fetch_result: FetchSlot,
}

impl WebFileManager {
    pub fn new(args: Args) -> Self {
        let frames_text = args.frames.clone().unwrap_or_default();
        Self {
            args,
            frames_text,
            url_text: String::new(),
            error: None,
            state: WebState::Input,
            fetch_result: Arc::new(Mutex::new(None)),
        }
    }

    pub fn update(&mut self, ctx: &egui::Context) -> LoadResult {
        match self.state {
            WebState::Input => self.update_input(ctx),
            WebState::Fetching => self.update_fetching(ctx),
        }
    }

    fn update_input(&mut self, ctx: &egui::Context) -> LoadResult {
        egui::SidePanel::right("settings_panel")
            .min_width(220.0)
            .show(ctx, |ui| {
                self.settings_ui(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.drop_zone_ui(ui);
        });

        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            return self.load_dropped_files(&dropped);
        }

        LoadResult::Pending
    }

    fn update_fetching(&mut self, ctx: &egui::Context) -> LoadResult {
        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(rect.height() / 2.0 - 30.0);
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Fetching trace...")
                                .size(18.0)
                                .color(Color32::GRAY),
                        );
                    });
                });
            });
        });

        let fetched = self.fetch_result.lock().unwrap().take();
        if let Some(result) = fetched {
            self.sync_frames_arg();
            match result {
                Ok((bytes, name)) => match load_from_bytes(&bytes, &name, &self.args) {
                    Ok((labels, traces)) => {
                        return LoadResult::Loaded {
                            labels,
                            traces,
                            args: self.args.clone(),
                        };
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.state = WebState::Input;
                    }
                },
                Err(e) => {
                    self.error = Some(e);
                    self.state = WebState::Input;
                }
            }
        }

        ctx.request_repaint();
        LoadResult::Pending
    }

    fn sync_frames_arg(&mut self) {
        let trimmed = self.frames_text.trim();
        self.args.frames = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }

    fn start_fetch(&mut self, url: String) {
        self.state = WebState::Fetching;
        self.error = None;
        let slot = self.fetch_result.clone();
        *slot.lock().unwrap() = None;
        loading::spawn_fetch(url, slot);
    }

    /// Shared settings sidebar (no CPU/GPU thread controls on web).
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.heading("Load Settings");
        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.label("Sampling Rate:");
            ui.add(
                DragValue::new(&mut self.args.sampling_rate)
                    .suffix(" MS/s")
                    .range(1.0..=1000e9)
                    .speed(25.0),
            );
        });

        ui.add_space(15.0);
        ui.separator();
        ui.add_space(10.0);

        ui.heading("Filter");
        ui.add_space(5.0);

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
                ui.selectable_value(&mut self.args.filter, Some(Filter::LowPass), "Low-pass");
                ui.selectable_value(&mut self.args.filter, Some(Filter::HighPass), "High-pass");
                ui.selectable_value(&mut self.args.filter, Some(Filter::BandPass), "Band-pass");
                ui.selectable_value(&mut self.args.filter, Some(Filter::Notch), "Notch");
            });

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
    }

    fn drop_zone_ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_stroke(
            rect.shrink(16.0),
            12.0,
            Stroke::new(2.0, Color32::GRAY),
            egui::StrokeKind::Inside,
        );

        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(rect.height() / 2.0 - 60.0);
                    ui.label(
                        RichText::new("Drop .npy, .csv or .wfm files here")
                            .size(22.0)
                            .color(Color32::GRAY),
                    );

                    ui.add_space(16.0);
                    ui.label(
                        RichText::new("or paste a URL")
                            .size(16.0)
                            .color(Color32::GRAY),
                    );
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        let available = ui.available_width();
                        let field_width = (available - 60.0).max(200.0).min(500.0);
                        let offset = (available - field_width - 60.0) / 2.0;
                        ui.add_space(offset.max(0.0));

                        let response = ui.add(
                            TextEdit::singleline(&mut self.url_text)
                                .hint_text("https://example.com/trace.npy")
                                .desired_width(field_width),
                        );

                        let enter_pressed = response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        if (ui.button("Load").clicked() || enter_pressed)
                            && !self.url_text.trim().is_empty()
                        {
                            let url = self.url_text.trim().to_string();
                            self.start_fetch(url);
                        }
                    });

                    if let Some(ref err) = self.error {
                        ui.add_space(12.0);
                        ui.label(RichText::new(err).color(Color32::RED));
                    }
                });
            });
        });
    }

    fn load_dropped_files(&mut self, files: &[DroppedFile]) -> LoadResult {
        self.sync_frames_arg();
        let mut labels = Vec::new();
        let mut traces = Vec::new();

        for file in files {
            let name = &file.name;
            let Some(format) = self.args.format.or_else(|| guess_format(name)) else {
                self.error = Some(format!("Unrecognized file extension: {name}"));
                continue;
            };

            if let Some(ref bytes) = file.bytes {
                let cursor = std::io::Cursor::new(bytes.as_ref());
                let frames = parse_traces(cursor, format, name, &self.args);
                collect_frames(name, frames, &self.args, &mut labels, &mut traces);
            } else {
                self.error = Some("No data in dropped file".into());
            }
        }

        if traces.is_empty() {
            return LoadResult::Pending;
        }

        self.error = None;
        LoadResult::Loaded {
            labels,
            traces,
            args: self.args.clone(),
        }
    }
}
