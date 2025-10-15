use crate::{
    camera::Camera,
    renderer::RENDERER_MAX_TRACE_SIZE,
    sync_features::SyncFeatures,
    tiling::{ColorScale, Gradient, TileProperties, TileSize, TileStatus, Tiling},
    util::{Fixed, format_f64_unit, format_number_unit, generate_checkboard},
};
use egui::{
    Align, Align2, Color32, DragValue, FontFamily, Key, Painter, PointerButton, Popup,
    PopupCloseBehavior, Rect, Sense, Shape, Stroke, TextFormat, TextureHandle, TextureOptions, Ui,
    pos2, text::LayoutJob, vec2,
};
use std::{
    collections::HashMap,
    ops::Add,
    sync::{Arc, Condvar, Mutex},
};

/// Defines the width of the tiles rendered by the GPU.
/// A smaller value will raise the number of required tiles to fill the screen, the number of GPU
/// calls will increase and therefore the overall rendering might be slower due to this overhead.
/// A higher value can lead to unsufficient GPU memory to store a trace slice for rendering a tile,
/// and therefore the minimum zoom level may be very limited.
/// The current value seems to be a good compromise.
const TILE_WIDTH: u32 = 64;

/// Minimum zoom level that can be rendered by the GPU.
const MIN_SCALE_X: usize = (RENDERER_MAX_TRACE_SIZE - 1) / TILE_WIDTH as usize;

/// Defines the zoom limit between antialiased lines display and density rendering.
const LINES_RENDERING_SCALE_LIMIT: f32 = 5.0;

pub struct Viewer<'a> {
    /// Viewer identifier used to distinguish tiles in the shared tiling in case there are multiple
    /// viewers.
    id: u32,
    /// The trace being displayed.
    trace: &'a Vec<f32>,
    /// Current camera settings.
    camera: Camera,
    /// Rendering tiles shared between the user interface and the GPU tiles renderer.
    shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
    /// Current tool for mouse left button
    tool: Tool,
    /// Tool usage step.
    tool_step: u8,
    /// Time selected by the tool.
    tool_times: Vec<Fixed>,
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
    /// Trace min and max values.
    /// Used for autoscaling.
    trace_min_max: [f32; 2],
    /// When true, the viewer will change scale and offset so the trace fits the screen.
    autoscale_request: bool,
    /// Trace sampling rate in MS/s
    sampling_rate: f32,
}

impl<'a> Viewer<'a> {
    pub const UV: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

    pub fn new(
        id: u32,
        ctx: &egui::Context,
        shared_tiling: Arc<(Mutex<Tiling>, Condvar)>,
        trace: &'a Vec<f32>,
    ) -> Self {
        let trace_min_max = [
            trace
                .iter()
                .cloned()
                .min_by(f32::total_cmp)
                .expect("Trace has NaN sample"),
            trace
                .iter()
                .cloned()
                .max_by(f32::total_cmp)
                .expect("Trace has NaN sample"),
        ];
        let color_scale = ColorScale {
            power: 1.0,
            opacity: 10.0,
            gradient: Gradient::SingleColor {
                min: 0.1,
                end: Color32::WHITE,
            },
        };
        Self {
            id,
            trace,
            camera: Camera::new(),
            shared_tiling,
            tool: Tool::Move,
            tool_step: 0,
            tool_times: Vec::new(),
            color_scale,
            previous_color_scale: color_scale,
            textures: HashMap::default(),
            texture_checkboard: generate_checkboard(ctx, 64),
            trace_min_max,
            autoscale_request: true,
            sampling_rate: 100.0,
        }
    }

    pub fn get_camera(&self) -> &Camera {
        &self.camera
    }

    pub fn set_camera(&mut self, camera: Camera) {
        if self.camera != camera {
            self.camera = camera;
        }
    }

    /// Toolbar widgets rendering.
    pub fn ui_toolbar(&mut self, ui: &mut Ui, sync_options: Option<&mut SyncFeatures>) {
        ui.horizontal(|ui| {
            ui.label(format!("Trace: {}S", format_number_unit(self.trace.len())));

            ui.label("@");
            let drag = DragValue::new(&mut self.sampling_rate)
                .suffix(" MS/s")
                .range(1.0..=1000e9)
                .speed(25.0);
            ui.add(drag);

            egui::ComboBox::from_id_salt("display")
                .selected_text(self.color_scale.gradient.name())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.color_scale.gradient,
                        Gradient::SingleColor {
                            min: 0.1,
                            end: Color32::WHITE,
                        },
                        "Single color",
                    );
                    ui.selectable_value(
                        &mut self.color_scale.gradient,
                        Gradient::BiColor {
                            start: Color32::BLUE,
                            end: Color32::GREEN,
                        },
                        "Gradient",
                    );
                    ui.selectable_value(
                        &mut self.color_scale.gradient,
                        Gradient::Rainbow,
                        "Rainbow",
                    );
                });

            match &mut self.color_scale.gradient {
                Gradient::SingleColor { min, end } => {
                    let drag = egui::DragValue::new(min).range(0.0..=1.0).speed(0.001);
                    ui.add(drag);
                    ui.color_edit_button_srgba(end);
                }
                Gradient::BiColor { start, end } => {
                    ui.color_edit_button_srgba(start);
                    ui.color_edit_button_srgba(end);
                }
                Gradient::Rainbow => {}
            };

            ui.label("Power:");
            let drag_power = egui::DragValue::new(&mut self.color_scale.power)
                .range(0.1..=4.0)
                .speed(0.005);
            ui.add(drag_power);
            ui.label("Opacity:");
            let drag_opacity = egui::DragValue::new(&mut self.color_scale.opacity)
                .range(0.01..=100.0)
                .speed(0.05);
            ui.add(drag_opacity);
            self.autoscale_request |= ui.button("Auto").clicked();

            // Tool selection
            let previous_tool = self.tool;
            egui::ComboBox::from_id_salt("tool")
                .selected_text(self.tool.name())
                .show_ui(ui, |ui| {
                    for x in [Tool::Move, Tool::Range, Tool::Count] {
                        ui.selectable_value(&mut self.tool, x, x.name());
                    }
                });
            if self.tool != previous_tool {
                self.tool_times.clear();
                self.tool_step = 0;
            }

            if let Some(options) = sync_options {
                let response = ui.button("Sync");
                Popup::menu(&response)
                    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.horizontal(|ui| {
                            if ui.button("All").clicked() {
                                options.set_all(true);
                            }
                            if ui.button("None").clicked() {
                                options.set_all(false);
                            };
                        });
                        ui.checkbox(&mut options.shift_x, "Shift X");
                        ui.checkbox(&mut options.shift_y, "Shift Y");
                        ui.checkbox(&mut options.scale_x, "Scale X");
                        ui.checkbox(&mut options.scale_y, "Scale Y");
                    });
            }
        });
    }

    /// Update viewer from mouse interaction.
    /// This is done before any painting.
    ///
    /// `viewport` is the allocating window region for the current viewer. This is the whole window
    /// when only one trace is open.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        ui: &mut Ui,
        viewport: Rect,
    ) -> ViewerUpdateStatus {
        let (stable_dt, mut left_pressed, key_left, key_right, scroll_delta, pos, modifiers) = ctx
            .input(|i| {
                (
                    i.stable_dt,
                    i.pointer.button_pressed(PointerButton::Primary),
                    i.key_down(Key::ArrowLeft),
                    i.key_down(Key::ArrowRight),
                    i.smooth_scroll_delta[1],
                    i.pointer.latest_pos(),
                    i.modifiers,
                )
            });

        let response = ui.allocate_rect(viewport, Sense::drag());

        // use hovered to disable interaction when cursor is on another widget (toolbar or other
        // viewer for instance).
        let hovered = response.hovered();
        left_pressed &= hovered;

        // All egui UI can be scaled up and down, like a page in a web browser.
        // However we don't want the trace rendering to scale up, so there are some gymnastics with
        // ppp during the painting.
        let ppp = ctx.pixels_per_point();

        let zooming = (scroll_delta != 0.0) & hovered;
        if zooming {
            if modifiers.alt {
                // Change in Y scaling
                let factor = Fixed::from_num(1.1f32.powf(scroll_delta / 40.0));
                self.camera.scale.y = (self.camera.scale.y * factor).max(Fixed::from_num(0.001));
            } else if pos.is_some() {
                // Change in X scaling
                let factor = Fixed::from_num(1.5f32.powf(-scroll_delta / 40.0));
                let s1 = self.camera.scale.x;
                let s2 = (s1 * factor).clamp(Fixed::from_num(0.01), Fixed::from_num(MIN_SCALE_X));
                let k = Fixed::from_num(pos.unwrap().x - response.rect.width() / 2.0);
                self.camera.shift.x = s1 * k + self.camera.shift.x - s2 * k;
                self.camera.scale.x = s2;
            }
        }

        if key_left && !key_right {
            self.camera.shift.x -= self.camera.scale.x * Fixed::from_num(1000.0 * stable_dt);
            ctx.request_repaint();
        }
        if key_right && !key_left {
            self.camera.shift.x += self.camera.scale.x * Fixed::from_num(1000.0 * stable_dt);
            ctx.request_repaint();
        }

        let mut dragging_y = false;
        let mut dragging_x = false;
        if response.dragged_by(PointerButton::Secondary)
            || (response.dragged_by(PointerButton::Primary) && self.tool == Tool::Move)
        {
            if ui.input(|i| i.modifiers.alt) {
                if response.drag_delta()[1] != 0.0 {
                    self.camera.shift.y -=
                        Fixed::from_num(response.drag_delta()[1] * ppp) / self.camera.scale.y;
                    dragging_y = true;
                }
            } else if response.drag_delta()[0] != 0.0 {
                self.camera.shift.x -=
                    Fixed::from_num(response.drag_delta()[0] * ppp) * self.camera.scale.x;
                dragging_x = true;
            }
        }

        let world_x =
            self.camera
                .screen_to_world_x(&viewport, ppp, pos.map(|p| p.x).unwrap_or(0.0));

        // Tool management
        match self.tool {
            Tool::Move => {}
            Tool::Range => match self.tool_step {
                0 => {
                    if left_pressed {
                        self.tool_times = vec![world_x, world_x];
                        self.tool_step = 1;
                    }
                }
                1 => {
                    self.tool_times[1] = world_x;
                    if left_pressed {
                        self.tool_step = 2;
                    }
                }
                2 => {
                    if left_pressed {
                        self.tool_times.clear();
                        self.tool_step = 0;
                    }
                }
                _ => panic!(),
            },
            Tool::Count => match self.tool_step {
                0 => {
                    if left_pressed {
                        self.tool_times = vec![world_x, world_x];
                        self.tool_step = 1;
                    }
                }
                1 => {
                    self.tool_times[1] = world_x;
                    if left_pressed {
                        self.tool_times.push(world_x);
                        self.tool_step = 2;
                    }
                }
                2 => {
                    self.tool_times[2] = world_x;
                    if left_pressed {
                        self.tool_step = 3;
                    }
                }
                3 => {
                    if left_pressed {
                        self.tool_times.clear();
                        self.tool_step = 0;
                    }
                }
                _ => panic!(),
            },
        }

        if self.autoscale_request {
            self.autoscale_request = false;
            let trace_len = Fixed::from_num(self.trace.len());
            self.camera.scale.x = (trace_len / Fixed::from_num(viewport.width() * ppp))
                .min(Fixed::from_num(MIN_SCALE_X));
            self.camera.shift.x = trace_len / 2;
            self.camera.scale.y = Fixed::from_num(
                ((viewport.height() * ppp) * 0.75)
                    / (self.trace_min_max[1] - self.trace_min_max[0]),
            );
            self.camera.shift.y =
                -Fixed::from_num(self.trace_min_max[0].midpoint(self.trace_min_max[1]));
        }

        ViewerUpdateStatus {
            zooming,
            dragging_x,
            dragging_y,
        }
    }

    pub fn paint_toolbar(
        &mut self,
        ctx: &egui::Context,
        sync: Option<&mut SyncFeatures>,
        viewport: Rect,
    ) {
        egui::Window::new(format!("toolbar{}", self.id))
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::LEFT_TOP, vec2(0.0, viewport.top()))
            .frame(
                egui::Frame::default()
                    .fill(Color32::from_rgba_unmultiplied(30, 30, 30, 200))
                    .inner_margin(8.0)
                    .outer_margin(8.0)
                    .corner_radius(4.0),
            )
            .min_width(viewport.width() - 32.0)
            .show(ctx, |ui| {
                ui.set_width(viewport.width() - 32.0);
                self.ui_toolbar(ui, sync)
            });
    }

    pub fn paint_waveform(
        &mut self,
        ctx: &egui::Context,
        ui: &mut Ui,
        viewport: Rect,
        allow_tile_requests: bool,
    ) {
        let painter = ui.painter().with_clip_rect(viewport);

        // All egui UI can be scaled up and down, like a page in a web browser.
        // However we don't want the trace rendering to scale up, so there are some gymnastics with
        // ppp during the painting.
        let ppp = ctx.pixels_per_point();

        let mode = if self.camera.scale.x < LINES_RENDERING_SCALE_LIMIT {
            RenderMode::Lines
        } else {
            RenderMode::Density
        };

        match mode {
            RenderMode::Density => {
                // If color scale changes all textures become invalid. We destroy the textures
                // cache. The GPU rendering of the tiles remains valid, we just need to recreate
                // the images from the density data.
                if (self.color_scale != self.previous_color_scale) && (mode == RenderMode::Density)
                {
                    self.textures.clear();
                    self.previous_color_scale = self.color_scale;
                }
                // New tiles are requested when moving the camera has finished. While we are zooming or
                // changing Y offset, we use previous tiles to render a preview.
                if allow_tile_requests {
                    // Calculate the set of tiles which must be rendered to cover all the current screen with
                    // the current camera scale and offsets.
                    let required = self.compute_viewport_tiles(viewport * ppp);

                    let mut complete = true;
                    for tile in required {
                        complete &= self
                            .shared_tiling
                            .0
                            .lock()
                            .unwrap()
                            .get(tile, true)
                            .unwrap()
                            .status
                            == TileStatus::Rendered;
                    }

                    if complete {
                        // All the tiles required to render the trace perfectly with current camera
                        // settings have been rendered by the GPU. We can therefore discard all other
                        // previous tiles which were used for the preview.
                        let mut tiling = self.shared_tiling.0.lock().unwrap();
                        tiling.tiles.retain(|t| {
                            ((t.properties.scale == self.camera.scale)
                                && (t.properties.offset == self.camera.shift.y))
                                // Don't remove tiles from other viewers!
                                || (t.properties.id != self.id)
                        });
                        // We also discard textures that are not used anymore.
                        self.textures
                            .retain(|k, _| tiling.tiles.iter().any(|t| t.properties == *k));
                    } else {
                        // Some tiles have not been rendered yet, and maybe have been added to the pool.
                        // Wake-up the rendering thread if it was sleeping.
                        self.shared_tiling.1.notify_one();
                    }
                }

                // Draw a background checkboard to show zones that are not rendered yet.
                self.paint_checkboard(&viewport, &painter);

                self.paint_tiles(ctx, ppp, &painter, viewport);

                if self.shared_tiling.0.lock().unwrap().has_pending() {
                    // TODO: it would be better to request repaint only when the GPU renderer has finished
                    // rendering a tile. This would reduce CPU usage but requires extra thread
                    // synchronization mechanisms.
                    ctx.request_repaint();
                }
            }
            RenderMode::Lines => {
                self.paint_black_background(&painter, viewport);
                self.paint_waveform_as_lines(ppp, &painter, &viewport);
            }
        }

        self.paint_tool(ppp, &painter, &viewport);
    }

    /// Paint the waveform as lines using egui painter. This is more suited for high zoom values
    /// and benefits from lines antialiasing.
    fn paint_waveform_as_lines(&self, ppp: f32, painter: &Painter, viewport: &Rect) {
        let t0 = self
            .camera
            .screen_to_world_x(viewport, ppp, 0.0)
            .floor()
            .to_num::<isize>()
            .clamp(0, self.trace.len() as isize) as usize;
        let t1 = self
            .camera
            .screen_to_world_x(viewport, ppp, viewport.max.x)
            .ceil()
            .to_num::<isize>()
            .add(1)
            .clamp(0, self.trace.len() as isize) as usize;
        let points = (t0..t1)
            .map(|t| {
                let x = self
                    .camera
                    .world_to_screen_x(viewport, ppp, Fixed::from_num(t));
                let y = viewport.center().y
                    - (self.trace[t] + self.camera.shift.y.to_num::<f32>())
                        * self.camera.scale.y.to_num::<f32>()
                        / ppp;
                pos2(x, y)
            })
            .collect();
        let color = match self.color_scale.gradient {
            Gradient::SingleColor { min: _, end } => end,
            Gradient::BiColor { start, end: _ } => start,
            Gradient::Rainbow => Color32::RED,
        };
        painter.line(points, Stroke::new(1.0, color));
    }

    /// Paint all the tiles that are available in the tiling set. This includes tiles rendered with
    /// both previous and new camera settings.
    ///
    /// Because tiles are stored in a Vec, those which were requested first are rendered first.
    /// This way the preview is always behind the final rendering.
    fn paint_tiles(&mut self, ctx: &egui::Context, ppp: f32, painter: &Painter, rect: Rect) {
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
            .filter(|p| p.id == self.id)
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
                    let image = tile.generate_image(self.color_scale);
                    ctx.load_texture("tile", image, TextureOptions::NEAREST)
                })
                .clone();
            self.paint_tile(painter, ppp, rect, tile.properties, &tex);
        }
    }

    /// Paint a particular tile in the viewport.
    ///
    /// The tile scale and offset can be different from the current camera settings. A homothecy is
    /// applied to draw the tile texture at the correct position.
    fn paint_tile(
        &mut self,
        painter: &Painter,
        ppp: f32,
        viewport: Rect,
        properties: TileProperties,
        tex: &TextureHandle,
    ) {
        let world_tile_width =
            Fixed::from_num(TILE_WIDTH) * properties.scale.x / self.camera.scale.x;
        let shift_x = self.camera.shift.x / self.camera.scale.x;

        let mul_y = (self.camera.scale.y / properties.scale.y).to_num::<f32>();
        let offset_y =
            ((properties.offset - self.camera.shift.y) * self.camera.scale.y).to_num::<f32>() / ppp;
        let y_mid = viewport.center().y;
        let y0 = y_mid - viewport.height() * mul_y * 0.5 + offset_y;
        let y1 = y_mid + viewport.height() * mul_y * 0.5 + offset_y;
        let tile_x = (Fixed::from_num(properties.index) * world_tile_width) - shift_x
            + Fixed::from_num(viewport.width() * ppp / 2.0);
        let rect = Rect {
            min: pos2(viewport.min.x + tile_x.to_num::<f32>() / ppp, y0),
            max: pos2(
                viewport.min.x + (tile_x + world_tile_width).to_num::<f32>() / ppp,
                y1,
            ),
        };
        painter.image(tex.into(), rect, Self::UV, Color32::WHITE);
    }

    /// Draw a black rectangle on all the surface of the given painter.
    fn paint_black_background(&self, painter: &Painter, viewport: Rect) {
        painter.rect_filled(viewport, 0.0, Color32::BLACK);
    }

    /// Draw a checkboard on all the surface of the given painter.
    fn paint_checkboard(&self, viewport: &Rect, painter: &Painter) {
        let width = viewport.width();
        let height = viewport.height();
        let nx = width / self.texture_checkboard.size()[0] as f32;
        let ny = height / self.texture_checkboard.size()[1] as f32;
        painter.image(
            (&self.texture_checkboard).into(),
            *viewport,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(nx, ny)),
            Color32::from_gray(10),
        );
    }

    /// Paint bars, ranges and labels from the selected tool.
    fn paint_tool(&self, ppp: f32, painter: &Painter, viewport: &Rect) {
        if self.tool_times.len() < 2 {
            return;
        }

        // Font used to draw column numbers for counting tool.
        let font_id = egui::FontId::new(12.0, FontFamily::Proportional);

        let (t0, t1) = (self.tool_times[0], self.tool_times[1]);
        let (t0, t1) = (t0.min(t1), t0.max(t1)); // No negative range
        let dt = t1 - t0;
        let x0 = self.camera.world_to_screen_x(viewport, ppp, t0);
        let x1 = self.camera.world_to_screen_x(viewport, ppp, t1);
        let y_top = 80.5; // Base line for displaying ranges at the top.
        let y_bot = viewport.max.y - 40.0; // Base line for counter at the bottop.
        let dy = 30.0; // Distance in Y of secondary range.

        match self.tool {
            Tool::Move => {}
            Tool::Range => {
                self.paint_bar(painter, viewport, x0);
                self.paint_bar(painter, viewport, x1);
                self.paint_time_range(ppp, painter, viewport, y_top, t0, t1);
            }
            Tool::Count => {
                self.paint_bar(painter, viewport, x0);
                self.paint_bar(painter, viewport, x1);
                if self.tool_step >= 2 {
                    let t2 = self.tool_times[2];
                    if t2 > t1 {
                        // First counting mode: count by dt step.
                        let mut t = t0 + dt;
                        let mut index = 1;
                        // prev_x used to center number label.
                        let mut prev_x = self.camera.world_to_screen_x(viewport, ppp, t0);
                        // We don't want to draw beyond viewport right edge.
                        // TODO: ideally, do the same for left edge.
                        let right_pos =
                            self.camera.screen_to_world_x(viewport, ppp, viewport.max.x);
                        while t < right_pos.min(t2 + dt) {
                            let x = self.camera.world_to_screen_x(viewport, ppp, t);
                            // Don't paint right bar twice.
                            if index > 1 {
                                self.paint_bar(painter, viewport, x);
                            }
                            painter.text(
                                pos2((x + prev_x) / 2.0, y_bot),
                                Align2::CENTER_CENTER,
                                index.to_string(),
                                font_id.clone(),
                                Color32::WHITE,
                            );
                            t += dt;
                            index += 1;
                            prev_x = x;
                        }
                        self.paint_time_range(ppp, painter, viewport, y_top, t0, t - dt);
                        self.paint_time_range(ppp, painter, viewport, y_top + dy, t0, t1);
                    } else {
                        // Second counting mode: divide the range.
                        if (t2 - t0) > 0 {
                            let count = (dt / (t2 - t0)).round().to_num::<usize>();
                            // 2048 as upper limit to prevent crashes or lags.
                            // This should be high enough anyway: the tool is difficult to use when
                            // this high.
                            if (count > 1) && (count <= 2048) {
                                // prev_x used to center number label.
                                let mut prev_x = self.camera.world_to_screen_x(viewport, ppp, t0);
                                for i in 0..count {
                                    let t =
                                        (dt * Fixed::from_num(i + 1)) / Fixed::from_num(count) + t0;
                                    let x = self.camera.world_to_screen_x(viewport, ppp, t);
                                    // Right bar was already painted.
                                    if i < count {
                                        self.paint_bar(painter, viewport, x);
                                    }
                                    painter.text(
                                        pos2((x + prev_x) / 2.0, y_bot),
                                        Align2::CENTER_CENTER,
                                        (i + 1).to_string(),
                                        font_id.clone(),
                                        Color32::WHITE,
                                    );
                                    prev_x = x;
                                }
                                // If count is 1, no need to paint twice the same time range.
                                if count > 1 {
                                    self.paint_time_range(
                                        ppp,
                                        painter,
                                        viewport,
                                        y_top + dy,
                                        t0,
                                        t0 + dt / Fixed::from_num(count),
                                    );
                                }
                            }
                        }
                        self.paint_time_range(ppp, painter, viewport, y_top, t0, t1);
                    }
                } else {
                    self.paint_time_range(ppp, painter, viewport, y_top, t0, t1);
                }
            }
        }
    }

    /// Paint a time range to display the duration and number of sample between to times.
    fn paint_time_range(
        &self,
        ppp: f32,
        painter: &Painter,
        viewport: &Rect,
        y: f32,
        t0: Fixed,
        t1: Fixed,
    ) {
        let font_id = egui::FontId::new(12.0, FontFamily::Proportional);
        let (t0, t1) = (t0.min(t1), t0.max(t1)); // No negative range
        let dt = t1 - t0;
        let duration = dt.to_num::<f64>() / (self.sampling_rate * 1e6) as f64;
        let x0 = self.camera.world_to_screen_x(viewport, ppp, t0);
        let x1 = self.camera.world_to_screen_x(viewport, ppp, t1);

        let dx = 5.0; // Arrow size on X axis
        let dy = 3.0; // Arrow radius on Y axis

        let mut job = LayoutJob {
            halign: Align::Center,
            ..Default::default()
        };
        job.append(
            &format!("{}s\n{} samples", format_f64_unit(duration), dt.ceil()),
            0.0,
            TextFormat {
                font_id: font_id.clone(),
                color: Color32::WHITE,
                ..Default::default()
            },
        );
        let galley = painter.layout_job(job);
        let rect = galley
            .rect
            .translate(vec2(x0.midpoint(x1), 0.0))
            .expand(4.0);
        painter.galley(
            pos2((x0 + x1) / 2.0, y - rect.height() / 2.0),
            galley.clone(),
            Color32::BLUE,
        );

        // Hide arrows smoothly when text is larger than range.
        let arrows_opacity = ((rect.min.x - x0) * 0.04).clamp(0.0, 0.75);
        let stroke = Stroke::new(1.0, Color32::WHITE.gamma_multiply(arrows_opacity));

        painter.line(vec![pos2(x0, y), pos2(rect.min.x, y)], stroke);
        painter.line(vec![pos2(rect.max.x, y), pos2(x1, y)], stroke);
        painter.line(
            vec![pos2(x0 + dx, y - dy), pos2(x0, y), pos2(x0 + dx, y + dy)],
            stroke,
        );
        painter.line(
            vec![pos2(x1 - dx, y - dy), pos2(x1, y), pos2(x1 - dx, y + dy)],
            stroke,
        );
    }

    /// Paint a vertical dashed line.
    fn paint_bar(&self, painter: &Painter, viewport: &Rect, x: f32) {
        painter.add(Shape::dashed_line(
            &[pos2(x, viewport.min.y), pos2(x, viewport.max.y)],
            Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.5)),
            4.0,
            4.0,
        ));
        painter.add(Shape::dashed_line_with_offset(
            &[pos2(x, viewport.min.y), pos2(x, viewport.max.y)],
            Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.5)),
            &[4.0],
            &[4.0],
            4.0,
        ));
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
                id: self.id,
                scale: self.camera.scale,
                index,
                offset: self.camera.shift.y,
                size: TileSize::new(TILE_WIDTH, viewport.height() as u32),
            })
            .collect()
    }
}

/// Returned by [`Viewer::update`], used for synchronization between different viewers and also to
/// allow or prevent tiles requests when camera settings are still being changed.
pub struct ViewerUpdateStatus {
    pub zooming: bool,
    pub dragging_x: bool,
    pub dragging_y: bool,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Tool {
    /// Pan the view.
    Move,
    /// Select time range.
    Range,
    /// Utility to count intervals using a time range and time indication.
    Count,
}

impl Tool {
    pub fn name(&self) -> &str {
        match self {
            Tool::Move => "Move",
            Tool::Range => "Range",
            Tool::Count => "Count",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    Density,
    Lines,
}
