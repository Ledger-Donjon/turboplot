use crate::{
    camera::Camera,
    renderer::RENDERER_MAX_TRACE_SIZE,
    tiling::{ColorScale, TileProperties, TileSize, TileStatus, Tiling, TilingRenderer},
    util::{Fixed, generate_checkboard},
};
use eframe::{App, egui};
use egui::{
    Color32, Painter, Rect, Response, Sense, Spinner, TextureHandle, TextureOptions, Ui, pos2,
};
use muscat::util::read_array1_from_npy_file;
use ndarray::Array1;
use npyz::NpyFile;
use std::{
    collections::HashMap,
    env,
    fs::File,
    io::BufReader,
    sync::{Arc, Condvar, Mutex},
    thread,
};
mod camera;
mod renderer;
mod tiling;
mod util;

/// Defines the width of the tiles rendered by the GPU.
/// A smaller value will raise the number of required tiles to fill the screen, the number of GPU
/// calls will increase and therefore the overall rendering might be slower due to this overhead.
/// A higher value can lead to unsufficient GPU memory to store a trace slice for rendering a tile,
/// and therefore the minimum zoom level may be very limited.
/// The current value seems to be a good compromise.
const TILE_WIDTH: u32 = 64;

struct Viewer {
    /// Current camera settings.
    camera: Camera,
    /// Rendering tiles shared between the user interface and the GPU tiles renderer.
    shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
    /// Trace rendering color.
    color: Color32,
    /// Defines how to calculate pixel colors depending on the density data calculated by the GPU.
    color_scale: ColorScale,
    /// Used to detect changes in color_scale so we can discard the texture cache.
    previous_color_scale: ColorScale,
    /// Textures created from the tiles rendered by the GPU, after the color scale has been
    /// applied. This is kind of a cache to avoid creating the textures at each egui rendering.
    /// If the color scale changes, the texture cache is discarded.
    textures: HashMap<TileProperties, TextureHandle>,
    /// The texture used to draw the background checkboard.
    /// This texture is not loaded from a file but generated during initialization.
    texture_checkboard: TextureHandle,
}

impl Viewer {
    pub const UV: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

    pub fn new(ctx: &egui::Context, shared_tiling: Arc<(Mutex<Tiling>, Condvar)>) -> Self {
        let color_scale = ColorScale {
            minimum: 0.1,
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
            {
                let tiling = self.shared_tiling.0.lock().unwrap();
                if tiling.has_pending() {
                    ui.add(Spinner::new().color(self.color).size(10.0));
                }
                ui.label(format!(
                    "{} tiles / {} textures",
                    tiling.tiles.len().to_string(),
                    self.textures.len()
                ));
            }
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
        let (response, painter) = ui.allocate_painter(size, Sense::drag());
        let zoom_delta = ui.input(|i| i.smooth_scroll_delta)[1];
        let zooming = zoom_delta != 0.0;

        if zooming {
            if ui.input(|i| i.modifiers.alt) {
                // Change in Y scaling
                let factor = Fixed::from_num(1.1f32.powf(zoom_delta / 40.0));
                self.camera.scale.y = (self.camera.scale.y * factor).max(Fixed::from_num(0.001));
            } else {
                // Change in X scaling
                let factor = Fixed::from_num(1.5f32.powf(-zoom_delta / 40.0));
                self.camera.scale.x =
                    (self.camera.scale.x * factor)
                        .max(Fixed::from_num(1))
                        .min(Fixed::from_num(
                            RENDERER_MAX_TRACE_SIZE / TILE_WIDTH as usize,
                        ));
            }
        }

        let mut dragging_y = false;
        if ui.input(|i| i.modifiers.alt) {
            if response.drag_delta()[1] != 0.0 {
                self.camera.shift.y +=
                    Fixed::from_num(response.drag_delta()[1]) / self.camera.scale.y;
                dragging_y = true;
            }
        } else if response.drag_delta()[0] != 0.0 {
            self.camera.shift.x -= Fixed::from_num(response.drag_delta()[0]) * self.camera.scale.x;
        }

        // Draw a background checkboard to show zones that are not rendered yet.
        self.paint_checkboard(&response, &painter);

        // New tiles are requested when moving the camera has finished. While we are zooming or
        // changing Y offset, we use previous tiles to render a preview.
        if !zooming && !dragging_y {
            // Calculate the set of tiles which must be rendered to cover all the current screen with
            // the current camera scale and offsets.
            let required = self.compute_viewport_tiles(response.rect);

            let mut complete = true;
            for tile in required {
                if self
                    .shared_tiling
                    .0
                    .lock()
                    .unwrap()
                    .get(tile, true)
                    .unwrap()
                    .status
                    != TileStatus::Rendered
                {
                    complete = false;
                }
            }

            if complete {
                // All the tiles required to render the trace perfectly with current camera
                // settings have been rendered by the GPU. We can therefore discard all other
                // previous tiles which were used for the preview.
                self.shared_tiling.0.lock().unwrap().tiles.retain(|t| {
                    (t.properties.scale == self.camera.scale)
                        && (t.properties.offset == self.camera.shift.y)
                });
            } else {
                // Some tiles have not been rendered yet, and maybe have been added to the pool.
                // Wake-up the rendering thread if it was sleeping.
                self.shared_tiling.1.notify_one();
            }
        }

        self.paint_tiles(ctx, &painter, response.rect);

        if self.shared_tiling.0.lock().unwrap().has_pending() {
            // TODO: it would be better to request repaint only when the GPU renderer has finished
            // rendering a tile. This would reduce CPU usage but requires extra thread
            // synchronization mechanisms.
            ctx.request_repaint();
        }
    }

    /// Paint all the tiles that are available in the tiling set. This includes tiles rendered with
    /// both previous and new camera settings.
    ///
    /// Because tiles are stored in a Vec, those which were requested first are rendered first.
    /// This way the preview is always behind the final rendering.
    fn paint_tiles(&mut self, ctx: &egui::Context, painter: &Painter, rect: Rect) {
        // We cannot iterate the vec of tiles while rendering because of the borrow checker (mutex
        // locking vs call to mutable paint method or texture set update). So we collect all the
        // tiles to be rendered first.
        // Note that we clone only the properties; we avoid cloning the tiles images.
        let properties: Vec<_> = self
            .shared_tiling
            .0
            .lock()
            .unwrap()
            .tiles
            .iter()
            .map(|t| t.properties)
            .collect();

        for p in properties {
            let Some(tile) = self.shared_tiling.0.lock().unwrap().get(p, false) else {
                continue;
            };
            if tile.status != TileStatus::Rendered {
                continue;
            }
            let tex = self
                .textures
                .entry(p)
                .or_insert_with(|| {
                    let image = tile.generate_image(self.camera.scale.x, self.color_scale);
                    ctx.load_texture("tile", image, TextureOptions::NEAREST)
                })
                .clone();
            self.paint_tile(painter, rect, tile.properties, &tex);
        }
    }

    /// Paint a particular tile in the viewport.
    ///
    /// The tile scale and offset can be different from the current camera settings. A homothecy is
    /// applied to draw the tile texture at the correct position.
    fn paint_tile(
        &mut self,
        painter: &Painter,
        viewport: Rect,
        properties: TileProperties,
        tex: &TextureHandle,
    ) {
        let world_tile_width =
            Fixed::from_num(TILE_WIDTH) * properties.scale.x / self.camera.scale.x;
        let shift_x = self.camera.shift.x / self.camera.scale.x;

        let mul_y = (self.camera.scale.y / properties.scale.y).to_num::<f32>();
        let offset_y =
            ((self.camera.shift.y - properties.offset) * self.camera.scale.y).to_num::<f32>();
        let y_mid = viewport.center().y;
        let y0 = y_mid - viewport.height() * mul_y * 0.5 + offset_y;
        let y1 = y_mid + viewport.height() * mul_y * 0.5 + offset_y;
        let tile_x = (Fixed::from_num(properties.index) * world_tile_width) - shift_x
            + Fixed::from_num(viewport.width() / 2.0);
        let rect = Rect {
            min: pos2(tile_x.to_num::<f32>(), y0),
            max: pos2((tile_x + world_tile_width).to_num::<f32>(), y1),
        };
        painter.image(tex.into(), rect, Self::UV, self.color);
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

    /// Calculates the set of tiles required to render the trace at full resolution in the viewport
    /// with current camera settings.
    ///
    /// Tiles are sorted by distance from the screen center, so the center will be rendered first
    /// and the edges last.
    fn compute_viewport_tiles(&self, viewport: Rect) -> Vec<TileProperties> {
        let width_half = Fixed::from_num(viewport.width() / 2.0);
        let tile_width = Fixed::from_num(TILE_WIDTH);
        let dx = self.camera.shift.x / self.camera.scale.x;
        let start = ((-width_half + dx) / tile_width).floor().to_num::<i32>();
        let end = ((width_half + dx) / tile_width).ceil().to_num::<i32>();
        let mut tile_indexes: Vec<_> = (start..end).collect();
        tile_indexes.sort_by_key(|&a| (a - (start + end) / 2).abs());
        tile_indexes
            .iter()
            .map(|&index| TileProperties {
                scale: self.camera.scale,
                index,
                offset: self.camera.shift.y,
                size: TileSize::new(TILE_WIDTH, viewport.height() as u32),
            })
            .collect()
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

fn main() {
    let args: Vec<_> = env::args().collect();

    if args.len() < 2 {
        println!("Please specify a path to a numpy file.");
        return;
    }

    if args.len() > 2 {
        println!("Too many arguments.");
        return;
    }

    let file = File::open(&args[1]).expect("Failed to open file");
    let buf_reader = BufReader::new(file);
    let npy = NpyFile::new(buf_reader).expect("Failed to parse numpy file");
    let trace: Array1<i8> = read_array1_from_npy_file(npy);
    let mut trace: Vec<f32> = trace.iter().map(|x| *x as f32).collect();
    let app = trace.clone();
    //for _ in 0..30 {
    //    trace.extend_from_slice(&app);
    //}
    println!("Trace length: {}", trace.len());

    let shared_tiling = Arc::new((Mutex::new(Tiling::new()), Condvar::new()));
    let mut renderer = TilingRenderer::new(shared_tiling.clone(), trace.clone());

    thread::spawn(move || renderer.render_loop());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        ..Default::default()
    };

    eframe::run_native(
        "TurboPlot",
        options,
        Box::new(|_cc| Ok(Box::new(Viewer::new(&_cc.egui_ctx, shared_tiling)))),
    )
    .unwrap();
}
