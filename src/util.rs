use egui::{Color32, ColorImage, TextureHandle, TextureOptions, TextureWrapMode};
use fixed::{FixedI64, types::extra::U24};

/// Fixed floating point number used by the viewer.
pub type Fixed = FixedI64<U24>;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default)]
pub struct FixedVec2 {
    pub x: Fixed,
    pub y: Fixed,
}

pub fn generate_checkboard(ctx: &egui::Context, size: usize) -> TextureHandle {
    debug_assert!(size.is_multiple_of(2));
    let mut image = ColorImage::new([size, size], Color32::BLACK);
    let half = size / 2;
    for i in 0..(size * size) {
        if (i % size < half) ^ (i % (size * size) < (half * size)) {
            image.pixels[i] = Color32::WHITE
        };
    }
    ctx.load_texture(
        "checkboard",
        image,
        TextureOptions {
            wrap_mode: TextureWrapMode::Repeat,
            mipmap_mode: None,
            ..Default::default()
        },
    )
}

pub fn format_number_unit(n: usize) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1000000 {
        format!("{:.1}k", (n as f32) / 1000.0)
    } else if n < 1000000000 {
        format!("{:.1}M", (n as f32) / 1e6)
    } else {
        format!("{:.1}G", (n as f32) / 1e9)
    }
}
