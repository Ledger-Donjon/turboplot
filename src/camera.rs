use crate::scale::Scale;
use egui::Rect;

#[derive(Clone, Copy, PartialEq)]
pub struct Camera {
    pub scale_x: Scale,
    pub scale_y: Scale,
    pub shift_x: f32,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            scale_x: 1000.0.into(),
            shift_x: 0.0,
            scale_y: 1.0.into(),
        }
    }

    pub fn world_to_screen_x(&self, viewport: &Rect, x: f64) -> f32 {
        viewport.width() / 2.0 + (x as f32 - self.shift_x) / f32::from(self.scale_x)
    }

    pub fn screen_to_world_x(&self, viewport: &Rect, x: f32) -> f64 {
        (f32::from(self.scale_x) * (x - viewport.width() / 2.0) + self.shift_x) as f64
    }
}
