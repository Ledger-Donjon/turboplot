use crate::util::U64F24;
use egui::Rect;

#[derive(Clone, Copy, PartialEq)]
pub struct Camera {
    pub scale_x: U64F24,
    pub scale_y: U64F24,
    pub shift_x: f32,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            scale_x: U64F24::from_num(1000),
            shift_x: 0.0,
            scale_y: U64F24::from_num(1),
        }
    }

    pub fn world_to_screen_x(&self, viewport: &Rect, x: f64) -> f32 {
        viewport.width() / 2.0 + (x as f32 - self.shift_x) / self.scale_x.to_num::<f32>()
    }

    pub fn screen_to_world_x(&self, viewport: &Rect, x: f32) -> f64 {
        (self.scale_x.to_num::<f32>() * (x - viewport.width() / 2.0) + self.shift_x) as f64
    }
}
