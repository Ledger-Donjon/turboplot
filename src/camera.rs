use crate::util::{Fixed, FixedVec2};
use egui::Rect;

#[derive(Clone, Copy, PartialEq)]
pub struct Camera {
    /// Scaling.
    /// For X-axis, the number represents the number of samples per pixel column.
    pub scale: FixedVec2,
    pub shift: FixedVec2,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            scale: FixedVec2 {
                x: Fixed::from_num(1000),
                y: Fixed::from_num(1),
            },
            shift: FixedVec2::default(),
        }
    }

    pub fn _world_to_screen_x(&self, viewport: &Rect, x: Fixed) -> f32 {
        (Fixed::from_num(viewport.width() / 2.0) + (x - self.shift.x) / self.scale.x)
            .to_num::<f32>()
    }

    pub fn _screen_to_world_x(&self, viewport: &Rect, x: f32) -> Fixed {
        self.scale.x * Fixed::from_num(x - viewport.width() / 2.0) + self.shift.x
    }
}
