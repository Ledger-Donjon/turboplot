use crate::{
    camera::Camera,
    renderer::RENDERER_MAX_TRACE_SIZE,
    tiling::{ColorScale, TileProperties, TileStatus, Tiling, TilingRenderer},
    util::{U64F24, generate_checkboard},
};
use eframe::{App, egui};
use egui::{
    Color32, Painter, Pos2, Rect, Response, Sense, Stroke, TextureHandle, TextureOptions, Ui, pos2,
};
use muscat::util::read_array1_from_npy_file;
use ndarray::Array1;
use npyz::NpyFile;
use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    sync::{Arc, Mutex},
    thread,
};
mod camera;
mod renderer;
mod tiling;
mod util;

const TILE_WIDTH: u32 = 64;

struct Viewer {
    camera: Camera,
    tile_scale_x: U64F24,
    shared_tiling: Arc<Mutex<Tiling>>,
    color: Color32,
    /// Defines how to calculate pixel colors depending on the density data calculated by the GPU.
    color_scale: ColorScale,
    /// Used to detect changes in color_scale so we can discard the texture cache.
    previous_color_scale: ColorScale,
    textures: HashMap<TileProperties, TextureHandle>,
    /// The texture used to draw the background checkboard.
    /// This texture is not loaded from a file but generated during initialization.
    texture_checkboard: TextureHandle,
}

impl Viewer {
    pub fn new(ctx: &egui::Context, shared_tiling: Arc<Mutex<Tiling>>) -> Self {
        let color_scale = ColorScale {
            minimum: 0.02,
            power: 1.0,
            opacity: 1.0,
        };
        let camera = Camera::new();
        Self {
            camera,
            tile_scale_x: camera.scale_x,
            shared_tiling,
            color: Color32::WHITE,
            color_scale,
            previous_color_scale: color_scale,
            textures: HashMap::default(),
            texture_checkboard: generate_checkboard(ctx, 64),
        }
    }

    /// Toolbar widgets rendering.
    pub fn ui_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.color_edit_button_srgba(&mut self.color);
            ui.label("Minimum:");
            let drag_minimum_gray = egui::DragValue::new(&mut self.color_scale.minimum)
                .range(0.0..=1.0)
                .speed(0.005);
            ui.add(drag_minimum_gray);
            ui.label("Power:");
            let drag_power = egui::DragValue::new(&mut self.color_scale.power)
                .range(0.1..=4.0)
                .speed(0.005);
            ui.add(drag_power);
            ui.label("Opacity:");
            let drag_opacity = egui::DragValue::new(&mut self.color_scale.opacity)
                .range(0.0..=20.0)
                .speed(0.005);
            ui.add(drag_opacity);
        });
    }

    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut Ui) {
        self.ui_toolbar(ui);

        // If color scale changes all textures become invalid. We destroy the textures cache.
        // The GPU rendering of the tiles remains valid, we just need to recreate the images from
        // the density data.
        if self.color_scale != self.previous_color_scale {
            self.textures.clear();
            self.previous_color_scale = self.color_scale;
        }

        let size = ui.available_size();
        let image_height = size.y as usize;
        self.shared_tiling
            .lock()
            .unwrap()
            .set_height(image_height as u32);

        let (response, painter) = ui.allocate_painter(size, Sense::drag());
        let zoom_delta = ui.input(|i| i.smooth_scroll_delta)[1];
        let zooming = zoom_delta != 0.0;

        if zoom_delta != 0.0 {
            if ui.input(|i| i.modifiers.alt) {
                let factor = U64F24::from_num(1.0 + zoom_delta * 0.01);
                self.camera.scale_y = (self.camera.scale_y * factor).max(U64F24::from_num(1));
            } else {
                let factor = U64F24::from_num(1.5f32.powf(-zoom_delta / 40.0));
                self.camera.scale_x =
                    (self.camera.scale_x * factor)
                        .max(U64F24::from_num(1))
                        .min(U64F24::from_num(
                            RENDERER_MAX_TRACE_SIZE / TILE_WIDTH as usize,
                        ));
            }
            //self.shared_tiling.lock().unwrap().tiles.clear();
        }
        if response.drag_delta()[0] != 0.0 {
            self.camera.shift_x -= response.drag_delta()[0] * self.camera.scale_x.to_num::<f32>();
        }

        self.paint_checkboard(&response, &painter);
        self.paint_trace(ctx, &response, &painter, self.tile_scale_x, false);
        let mut complete = true;
        if !zooming {
            complete = self.paint_trace(ctx, &response, &painter, self.camera.scale_x, true);
            if complete {
                self.tile_scale_x = self.camera.scale_x;
            }
        }

        let x = self.w2sx(&response, 0.0);
        painter.line(
            vec![Pos2::new(x, 10.0), Pos2::new(x, 100.0)],
            Stroke::new(1.0, Color32::RED),
        );

        if !complete {
            ctx.request_repaint();
        }
    }

    /// Paint the trace for a given scale tiles.
    ///
    /// This method is usually called twice:
    /// - first to render previously generated tiles with a new scaling, as a dirty preview,
    /// - second to render tiles rendered with the matching scale, as the final render.
    ///
    /// If `request` is true, missing tiles will be requested to the rendering thread. This is the
    /// case for the final render, not the preview.
    fn paint_trace(
        &mut self,
        ctx: &egui::Context,
        response: &Response,
        painter: &Painter,
        scale: U64F24,
        request: bool,
    ) -> bool {
        let width = response.rect.width();
        let height = response.rect.height();
        let world_tile_width =
            (U64F24::from_num(TILE_WIDTH) * scale / self.camera.scale_x).to_num::<f32>();
        let shift_x = self.camera.shift_x / self.camera.scale_x.to_num::<f32>();
        let tile_start = ((width / -2.0 + shift_x) / world_tile_width).floor();
        let tile_end = ((width / 2.0 + shift_x) / world_tile_width).ceil();
        let mut tile_indexes: Vec<_> = (tile_start as i32..tile_end as i32).collect();
        tile_indexes.sort_by_key(|&a| {
            let middle = (tile_start + tile_end) / 2.0;
            let da = (a as f32 - middle).abs();
            da as i32
        });

        let mut complete = true;
        for tile_i in tile_indexes {
            let tile = {
                let mut tiling = self.shared_tiling.lock().unwrap();
                tiling.get(
                    TileProperties {
                        scale_x: scale,
                        scale_y: self.camera.scale_y,
                        index: tile_i,
                        size: (TILE_WIDTH, height as u32),
                    },
                    request,
                )
            };
            let Some(tile) = tile else {
                continue;
            };
            if tile.status == TileStatus::Rendered {
                let tex = self.textures.entry(tile.properties).or_insert_with(|| {
                    let image = tile.generate_image(self.camera.scale_x, self.color_scale);
                    ctx.load_texture("tile", image, TextureOptions::default())
                });
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let tile_x = (tile_i as f32 * world_tile_width) - shift_x + width / 2.0;
                let rect = Rect {
                    min: pos2(tile_x, response.rect.min.y),
                    max: pos2(tile_x + world_tile_width, response.rect.min.y + height),
                };
                painter.image(tex.into(), rect, uv, self.color);
            } else {
                complete = false;
            }
        }
        complete
    }

    /// Draw a checkboard on all the surface of the given painter.
    fn paint_checkboard(&self, response: &Response, painter: &Painter) {
        let width = response.rect.width();
        let height = response.rect.height();
        let nx = width / self.texture_checkboard.size()[0] as f32;
        let ny = height / self.texture_checkboard.size()[1] as f32;
        painter.image(
            (&self.texture_checkboard).into(),
            response.rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(nx, ny)),
            Color32::from_gray(10),
        );
    }

    fn w2sx(&self, response: &Response, x: f64) -> f32 {
        response.rect.width() / 2.0
            + (x as f32 - self.camera.shift_x) / self.camera.scale_x.to_num::<f32>()
    }
}

impl App for Viewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_pixels_per_point(1.0);
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui(ctx, ui);
        });
    }
}

fn main() -> eframe::Result {
    let file = File::open("ae61d1d0.npy").unwrap();
    let buf_reader = BufReader::new(file);
    let npy = NpyFile::new(buf_reader).unwrap();
    let trace: Array1<i8> = read_array1_from_npy_file(npy);
    let mut trace: Vec<f32> = trace.iter().map(|x| *x as f32).collect();
    let app = trace.clone();
    for _ in 0..30 {
        trace.extend_from_slice(&app);
    }
    println!("Trace length: {}", trace.len());

    let shared_tiling = Arc::new(Mutex::new(Tiling::new()));
    let mut renderer = TilingRenderer::new(shared_tiling.clone(), TILE_WIDTH, trace.clone());

    thread::spawn(move || {
        loop {
            renderer.render_next_tile();
        }
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        ..Default::default()
    };

    eframe::run_native(
        "Wavetracer",
        options,
        Box::new(|_cc| Ok(Box::new(Viewer::new(&_cc.egui_ctx, shared_tiling)))),
    )
}
