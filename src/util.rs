use egui::{Color32, ColorImage, TextureHandle, TextureOptions, TextureWrapMode};

pub fn generate_checkboard(ctx: &egui::Context, size: usize) -> TextureHandle {
    debug_assert!(size % 2 == 0);
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
