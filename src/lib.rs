//! TurboPlot — a blazingly fast waveform renderer for visualizing huge traces.
//!
//! This crate provides both a native desktop application and a WebAssembly build
//! that runs in the browser.  The core rendering pipeline (GPU compute shaders
//! via wgpu + CPU fallback), the tile-based progressive display, and the egui UI
//! are shared across platforms.
//!
//! # Entry points
//!
//! - **Native:** [`run_native`] — launched from the binary crate (`main.rs`)
//!   with CLI-parsed [`input::Args`].
//! - **Web:** [`start`] — a `#[wasm_bindgen(start)]` function that boots the
//!   eframe WebRunner with default settings.

pub mod camera;
pub mod filtering;
pub mod input;
pub mod loaders;
pub mod multi_viewer;
pub mod renderer;
pub mod sync_features;
pub mod tiling;
pub mod util;
pub mod viewer;

use input::{Args, LoadResult};
use multi_viewer::MultiViewer;

use eframe::egui;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use input::FileManager;
#[cfg(target_arch = "wasm32")]
use input::WebFileManager;

/// Application state: selecting files, loading from URL, or viewing traces.
enum AppState {
    #[cfg(not(target_arch = "wasm32"))]
    Selection(FileManager),
    #[cfg(target_arch = "wasm32")]
    Selection(WebFileManager),
    #[cfg(target_arch = "wasm32")]
    Loading {
        fetch_result: input::loading::FetchSlot,
        args: Args,
    },
    Viewing(MultiViewer),
}

/// Main application wrapper that handles file selection and viewing states.
pub struct TurboPlotApp {
    state: AppState,
    wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
}

impl TurboPlotApp {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        _ctx: &egui::Context,
        args: Args,
        wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    ) -> Self {
        Self {
            state: AppState::Selection(FileManager::new(args)),
            wgpu_render_state,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn new(
        _ctx: &egui::Context,
        args: Args,
        auto_fetch_url: Option<String>,
        wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    ) -> Self {
        let state = if let Some(url) = auto_fetch_url {
            let slot = input::loading::new_fetch_slot();
            input::loading::spawn_fetch(url, slot.clone());
            AppState::Loading {
                fetch_result: slot,
                args,
            }
        } else {
            AppState::Selection(WebFileManager::new(args))
        };
        Self {
            state,
            wgpu_render_state,
        }
    }

    fn transition_to_viewing(
        &mut self,
        ctx: &egui::Context,
        labels: Vec<String>,
        traces: Vec<Arc<Vec<f32>>>,
        args: &Args,
    ) {
        let viewer = MultiViewer::new(
            ctx,
            labels,
            Arc::new(traces),
            args.sampling_rate,
            args.gpu,
            args.cpu_threads(),
            self.wgpu_render_state.as_ref(),
        );
        self.state = AppState::Viewing(viewer);
    }
}

impl eframe::App for TurboPlotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_arch = "wasm32")]
        if let Some(transition) = self.poll_loading(ctx) {
            match transition {
                LoadingTransition::Loaded(labels, traces, args) => {
                    self.transition_to_viewing(ctx, labels, traces, &args);
                }
                LoadingTransition::Failed(args) => {
                    self.state = AppState::Selection(WebFileManager::new(args));
                }
            }
            return;
        }

        match &mut self.state {
            AppState::Selection(mgr) => {
                if let LoadResult::Loaded {
                    labels,
                    traces,
                    args,
                } = mgr.update(ctx)
                {
                    self.transition_to_viewing(ctx, labels, traces, &args);
                }
            }

            #[cfg(target_arch = "wasm32")]
            AppState::Loading { .. } => unreachable!(),

            AppState::Viewing(viewer) => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default().outer_margin(0.0))
                    .show(ctx, |ui| {
                        viewer.update(ctx, ui);
                    });
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
enum LoadingTransition {
    Loaded(Vec<String>, Vec<Arc<Vec<f32>>>, Args),
    Failed(Args),
}

/// Handle the Loading state outside the main match to avoid borrow conflicts.
#[cfg(target_arch = "wasm32")]
impl TurboPlotApp {
    fn poll_loading(&mut self, ctx: &egui::Context) -> Option<LoadingTransition> {
        let (fetch_result, args) = match &self.state {
            AppState::Loading { fetch_result, args } => (fetch_result.clone(), args.clone()),
            _ => return None,
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(rect.height() / 2.0 - 30.0);
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Loading trace...")
                                .size(18.0)
                                .color(egui::Color32::GRAY),
                        );
                    });
                });
            });
        });

        let fetched = fetch_result.lock().unwrap().take();
        match fetched {
            Some(Ok((bytes, name))) => {
                match input::loading::load_from_bytes(&bytes, &name, &args) {
                    Ok((labels, traces)) => Some(LoadingTransition::Loaded(labels, traces, args)),
                    Err(e) => {
                        web_sys::console::error_1(
                            &format!("Failed to parse trace: {e}").into(),
                        );
                        Some(LoadingTransition::Failed(args))
                    }
                }
            }
            Some(Err(e)) => {
                web_sys::console::error_1(&format!("Failed to fetch trace: {e}").into());
                Some(LoadingTransition::Failed(args))
            }
            None => {
                ctx.request_repaint();
                None
            }
        }
    }
}

/// Launches the native desktop application with the given configuration.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native(args: Args) {
    use egui::Vec2;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        window_builder: Some(Box::new(|w| w.with_inner_size(Vec2::new(1280.0, 512.0)))),
        ..Default::default()
    };

    eframe::run_native(
        "TurboPlot",
        options,
        Box::new(move |cc| {
            Ok(Box::new(TurboPlotApp::new(
                &cc.egui_ctx,
                args,
                cc.wgpu_render_state.clone(),
            )))
        }),
    )
    .unwrap();
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// WASM entry point.
///
/// Parses URL query parameters to configure the viewer. If a `url` parameter
/// is present, the trace is fetched automatically and the input GUI is skipped.
///
/// Supported query parameters:
/// - `url` — trace file URL (triggers auto-load)
/// - `sampling_rate` — in MS/s (default 125)
/// - `format` — `npy`, `csv`, or `tek-wfm`
/// - `filter` — `low-pass`, `high-pass`, `band-pass`, or `notch`
/// - `cutoff_freq` — filter cutoff in kHz (default 1000)
/// - `skip_lines` — CSV header lines to skip
/// - `column` — CSV column index
/// - `frames` — frame selection, e.g. `0-3,6,7-8,12`
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let (args, auto_url) = input::parse_url_params();

    wasm_bindgen_futures::spawn_local(async move {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");
        let canvas = document
            .get_element_by_id("turboplot_canvas")
            .expect("No canvas element with id 'turboplot_canvas'")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("Element is not a canvas");

        let runner = eframe::WebRunner::new();
        runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |cc| {
                    Ok(Box::new(TurboPlotApp::new(
                        &cc.egui_ctx,
                        args,
                        auto_url,
                        cc.wgpu_render_state.clone(),
                    )))
                }),
            )
            .await
            .expect("Failed to start eframe");
    });

    Ok(())
}
