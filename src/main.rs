use crate::{
    camera::Camera,
    renderer::RENDERER_MAX_TRACE_SIZE,
    tiling::{ColorScale, TileProperties, TileStatus, Tiling, TilingRenderer},
    util::{Fixed, FixedVec2, generate_checkboard},
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
    /// Current camera settings.
    current_camera: Camera,
    /// Camera settings before last zoom started.
    /// This is necessary to render old tiles using the new camera settings as a preview image when
    /// the new tiles have not finished rendering.
    preview_camera: Camera,
    /// Rendering tiles shared between the user interface and the GPU tiles renderer.
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
            current_camera: camera,
            preview_camera: camera,
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
                let factor = Fixed::from_num(1.0 + zoom_delta * 0.01);
                self.current_camera.scale.y =
                    (self.current_camera.scale.y * factor).max(Fixed::from_num(1));
            } else {
                let factor = Fixed::from_num(1.5f32.powf(-zoom_delta / 40.0));
                self.current_camera.scale.x = (self.current_camera.scale.x * factor)
                    .max(Fixed::from_num(1))
                    .min(Fixed::from_num(
                        RENDERER_MAX_TRACE_SIZE / TILE_WIDTH as usize,
                    ));
            }
            //self.shared_tiling.lock().unwrap().tiles.clear();
        }
        if response.drag_delta()[0] != 0.0 {
            self.current_camera.shift.x -=
                Fixed::from_num(response.drag_delta()[0]) * self.current_camera.scale.x;
        }

        self.paint_checkboard(&response, &painter);
        self.paint_trace(ctx, &response, &painter, self.preview_camera.scale, false);
        let mut complete = true;
        if !zooming {
            complete = self.paint_trace(ctx, &response, &painter, self.current_camera.scale, true);
            if complete {
                self.preview_camera = self.current_camera;
            }
        }

        let x = self
            .current_camera
            .world_to_screen_x(&response.rect, Fixed::from_num(0));
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
        scale: FixedVec2,
        request: bool,
    ) -> bool {
        let width = response.rect.width();
        let height = response.rect.height();
        let world_tile_width = Fixed::from_num(TILE_WIDTH) * scale.x / self.current_camera.scale.x;
        let shift_x = self.current_camera.shift.x / self.current_camera.scale.x;
        let tile_start = ((Fixed::from_num(width / -2.0) + shift_x) / world_tile_width)
            .floor()
            .to_num::<i32>();
        let tile_end = ((Fixed::from_num(width / 2.0) + shift_x) / world_tile_width)
            .ceil()
            .to_num::<i32>();
        let mut tile_indexes: Vec<_> = (tile_start..tile_end).collect();
        tile_indexes.sort_by_key(|&a| {
            let middle = (tile_start + tile_end) / 2;
            (a - middle).abs();
        });

        let mut complete = true;
        for tile_i in tile_indexes {
            let tile = {
                let mut tiling = self.shared_tiling.lock().unwrap();
                tiling.get(
                    TileProperties {
                        scale,
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
                    let image = tile.generate_image(self.current_camera.scale.x, self.color_scale);
                    ctx.load_texture("tile", image, TextureOptions::default())
                });
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let tile_x = (Fixed::from_num(tile_i) * world_tile_width) - shift_x
                    + Fixed::from_num(width / 2.0);
                let rect = Rect {
                    min: pos2(tile_x.to_num::<f32>(), response.rect.min.y),
                    max: pos2(
                        (tile_x + world_tile_width).to_num::<f32>(),
                        response.rect.min.y + height,
                    ),
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
