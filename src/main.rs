use crate::{
    camera::Camera,
    tiling::{ColorScale, TileProperties, TileStatus, Tiling, TilingRenderer},
};
use eframe::{App, egui};
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, TextureHandle, TextureOptions, Ui, pos2};
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
mod scale;
mod tiling;

struct Viewer {
    camera: Camera,
    shared_tiling: Arc<Mutex<Tiling>>,
    color: Color32,
    /// Defines how to calculate pixel colors depending on the density data calculated by the GPU.
    color_scale: ColorScale,
    /// Used to detect changes in color_scale so we can discard the texture cache.
    previous_color_scale: ColorScale,
    textures: HashMap<TileProperties, TextureHandle>,
}

impl Viewer {
    pub fn new(ctx: &egui::Context, shared_tiling: Arc<Mutex<Tiling>>) -> Self {
        let color_scale = ColorScale {
            minimum: 0.02,
            power: 1.0,
            opacity: 1.0,
        };
        Self {
            camera: Camera::new(),
            shared_tiling,
            color: Color32::WHITE,
            color_scale,
            previous_color_scale: color_scale,
            textures: HashMap::default(),
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut Ui) {
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

        // If color scale changes all textures become invalid. We destroy the textures cache.
        // The GPU rendering of the tiles remains valid, we just need to recreate the images from
        // the density data.
        if self.color_scale != self.previous_color_scale {
            self.textures.clear();
            self.previous_color_scale = self.color_scale;
        }

        let size = ui.available_size();
        let image_width = size.x as usize;
        let image_height = size.y as usize;

        let (response, painter) = ui.allocate_painter(size, Sense::drag());
        let zoom_delta = ui.input(|i| i.raw_scroll_delta)[1];

        if zoom_delta != 0.0 {
            let factor = 1.5f32.powf(-zoom_delta / 40.0);
            if ui.input(|i| i.modifiers.ctrl) {
                let factor = 1.0 + (zoom_delta * 0.01);
                self.camera.scale_y = (self.camera.scale_y * factor).max(0.1.into());
            } else {
                self.camera.scale_x = (self.camera.scale_x * factor).max(1.0.into());
                println!("SCALE {}", f32::from(self.camera.scale_x));
            }
            self.shared_tiling.lock().unwrap().tiles.clear();
        }
        if response.drag_delta()[0] != 0.0 {
            self.camera.shift_x -= response.drag_delta()[0] * f32::from(self.camera.scale_x);
        }

        // Render tiles
        let chunk_width: i32 = 64;
        let shift_x = self.camera.shift_x / f32::from(self.camera.scale_x);
        let tile_start = (((image_width as f32 / -2.0) + shift_x) / chunk_width as f32).floor();
        let tile_end = (((image_width as f32 / 2.0) + shift_x) / chunk_width as f32).ceil();
        let mut tile_indexes: Vec<_> = (tile_start as i32..tile_end as i32).collect();
        tile_indexes.sort_by_key(|&a| {
            let middle = ((tile_start as f32) + (tile_end as f32)) / 2.0;
            let da = (a as f32 - middle).abs();
            da as i32
        });

        let mut partial = false;

        for tile_i in tile_indexes {
            let data = {
                let mut tiling = self.shared_tiling.lock().unwrap();
                tiling.get(TileProperties {
                    scale_x: self.camera.scale_x,
                    scale_y: self.camera.scale_y,
                    index: tile_i,
                    size: (64, image_height as u32),
                })
            };
            let tile_x =
                ((tile_i * chunk_width as i32) - shift_x as i32 + (image_width as i32 / 2)) as i32;
            if data.status == TileStatus::Rendered {
                let tex = self.textures.entry(data.properties).or_insert_with(|| {
                    println!("Generating tile texture");
                    let image = data.generate_image(self.camera.scale_x, self.color_scale);
                    ctx.load_texture("tile", image, TextureOptions::default())
                });
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let rect = Rect {
                    min: pos2(tile_x as f32, 0.0),
                    max: pos2((tile_x + 64) as f32, image_height as f32),
                };
                painter.image(tex.into(), rect, uv, self.color);
            } else {
                partial = true;
                //self.paste_dummies(64, tile_x);
            }
        }

        let x = self.w2sx(&response, 0.0);
        painter.line(
            vec![Pos2::new(x, 10.0), Pos2::new(x, 100.0)],
            Stroke::new(1.0, Color32::RED),
        );

        if partial {
            ctx.request_repaint();
        }
    }

    fn w2sx(&self, response: &Response, x: f64) -> f32 {
        response.rect.width() / 2.0
            + (x as f32 - self.camera.shift_x) / f32::from(self.camera.scale_x)
    }
}

impl App for Viewer {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
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
    /*for i in 0..31 {
        trace.extend_from_slice(&app);
    }*/
    println!("Trace length: {}", trace.len());

    let shared_tiling = Arc::new(Mutex::new(Tiling::new()));
    let mut renderer = TilingRenderer::new(shared_tiling.clone(), trace.clone());

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
