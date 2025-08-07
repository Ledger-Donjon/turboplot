use crate::{
    camera::Camera,
    renderer::{TileProperties, TileStatus, Tiling, TilingRenderer},
};
use eframe::{App, egui};
use egui::{
    Color32, ColorImage, Pos2, Rect, Response, Sense, Stroke, TextureFilter, TextureHandle,
    TextureOptions, Ui, load::SizedTexture, pos2,
};
use muscat::util::read_array1_from_npy_file;
use ndarray::Array1;
use npyz::NpyFile;
use std::{
    fs::File,
    io::BufReader,
    sync::{Arc, Mutex},
    thread,
};
mod camera;
mod renderer;

struct Viewer {
    camera: Camera,
    shared_tiling: Arc<Mutex<Tiling>>,
    image: ColorImage,
    minimum_gray: f32,
    power: f32,
    opacity: f32,
    texture: TextureHandle,
}

impl Viewer {
    pub fn new(ctx: &egui::Context, shared_tiling: Arc<Mutex<Tiling>>) -> Self {
        let dummy_image = ColorImage::new([64, 64], Color32::BLACK);
        Self {
            camera: Camera::new(),
            shared_tiling,
            image: dummy_image.clone(),
            minimum_gray: 0.02,
            power: 1.0,
            opacity: 1.0,
            texture: ctx.load_texture(
                "waveform",
                dummy_image,
                TextureOptions {
                    magnification: TextureFilter::Nearest,
                    ..Default::default()
                },
            ),
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Minimum gray:");
            let drag_minimum_gray = egui::DragValue::new(&mut self.minimum_gray)
                .range(0.0..=1.0)
                .speed(0.005);
            ui.add(drag_minimum_gray);
            ui.label("Power:");
            let drag_power = egui::DragValue::new(&mut self.power)
                .range(0.1..=4.0)
                .speed(0.005);
            ui.add(drag_power);
            ui.label("Opacity:");
            let drag_opacity = egui::DragValue::new(&mut self.opacity)
                .range(0.0..=20.0)
                .speed(0.005);
            ui.add(drag_opacity);
        });

        let size = ui.available_size();
        let image_width = size.x as usize;
        let image_height = size.y as usize;

        if self.image.size != [image_width, image_height] {
            println!("Allocating image {} x {}", image_width, image_height);
            self.image = ColorImage::new([image_width, image_height], Color32::BLACK);
            self.shared_tiling
                .lock()
                .unwrap()
                .set_height(image_height as u32);
        }

        let (response, painter) = ui.allocate_painter(size, Sense::drag());
        let zoom_delta = ui.input(|i| i.raw_scroll_delta)[1];

        if zoom_delta != 0.0 {
            if ui.input(|i| i.modifiers.ctrl) {
                let factor = 1.0 + (zoom_delta * 0.01);
                self.camera.scale_y = (self.camera.scale_y * factor).max(0.1);
            } else {
                let factor = 1.0 + (-zoom_delta * 0.01);
                self.camera.scale_x = (self.camera.scale_x * factor).max(1.0);
            }
            self.shared_tiling.lock().unwrap().tiles.clear();
        }
        if response.drag_delta()[0] != 0.0 {
            self.camera.shift_x -= response.drag_delta()[0] * self.camera.scale_x;
        }

        // Render tiles
        let chunk_width: i32 = 64;
        let image_width = self.image.width();
        let shift_x = self.camera.shift_x / self.camera.scale_x;
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
                    height: image_height as u32,
                })
            };
            let tile_x =
                ((tile_i * chunk_width as i32) - shift_x as i32 + (image_width as i32 / 2)) as i32;
            if data.status == TileStatus::Rendered {
                self.paste_tile(data.data, 64, image_height as i32, tile_x);
            } else {
                partial = true;
                self.paste_dummies(64, tile_x);
            }
        }

        let texture_options = TextureOptions {
            magnification: TextureFilter::Nearest,
            ..Default::default()
        };
        self.texture.set(self.image.clone(), texture_options);

        let sized_texture = SizedTexture::from_handle(&self.texture);
        painter.image(
            sized_texture.id,
            response.rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );

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
        response.rect.width() / 2.0 + (x as f32 - self.camera.shift_x) / self.camera.scale_x
    }

    pub fn paste_tile(&mut self, tile: Vec<u32>, tile_width: i32, tile_height: i32, x: i32) {
        let image_width = self.image.width();
        // Copy time is usually < 1ms
        for x2 in x..(x + tile_width as i32) {
            if (0..image_width as i32).contains(&x2) {
                for y in 0..tile_height as i32 {
                    let offset = (x2 - x) * tile_height as i32 + y;
                    let density = tile[offset as usize];
                    let a = if density == 0 {
                        0.0
                    } else {
                        self.minimum_gray
                            + ((density as f32).powf(self.power)
                                * self.opacity
                                * 0.005
                                * (1000.0 / self.camera.scale_x))
                    };
                    let c = (a * 255.0) as u8;
                    self.image.pixels[(y * image_width as i32 + x2) as usize] =
                        Color32::from_gray(c);
                }
            }
        }
    }

    pub fn paste_dummies(&mut self, tile_width: i32, x: i32) {
        let height = self.image.height() as i32;
        let image_width = self.image.width() as i32;
        let gray = Color32::from_gray(20);
        for x2 in (x.max(0))..((x + tile_width).min(image_width - 1)) {
            for y in (0..height) {
                let c = ((x2 % 50) < 25) ^ ((y % 50) < 25);
                self.image.pixels[((y * image_width) + x2) as usize] =
                    if c { Color32::BLACK } else { gray };
            }
        }
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
