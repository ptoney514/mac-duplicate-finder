//! CLIP image preprocessing: shortest side to 224 (bicubic-ish), center
//! crop, per-channel normalization, CHW layout.

use image::DynamicImage;

/// CLIP ViT-B/32 input side length.
pub const CLIP_SIDE: u32 = 224;

/// OpenAI CLIP normalization constants.
pub const CLIP_MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
pub const CLIP_STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];

/// Converts an image into the model's input tensor data: 3x224x224 f32,
/// channel-major (CHW), normalized with [`CLIP_MEAN`]/[`CLIP_STD`].
pub fn clip_pixels(img: &DynamicImage) -> Vec<f32> {
    let (w, h) = (img.width().max(1), img.height().max(1));
    let scale = CLIP_SIDE as f32 / w.min(h) as f32;
    let nw = ((w as f32 * scale).round() as u32).max(CLIP_SIDE);
    let nh = ((h as f32 * scale).round() as u32).max(CLIP_SIDE);
    let resized = img.resize_exact(nw, nh, image::imageops::FilterType::CatmullRom);
    let cropped = resized
        .crop_imm(
            (nw - CLIP_SIDE) / 2,
            (nh - CLIP_SIDE) / 2,
            CLIP_SIDE,
            CLIP_SIDE,
        )
        .to_rgb8();

    let plane = (CLIP_SIDE * CLIP_SIDE) as usize;
    let mut out = vec![0.0f32; 3 * plane];
    for (i, pixel) in cropped.pixels().enumerate() {
        for c in 0..3 {
            out[c * plane + i] = (pixel[c] as f32 / 255.0 - CLIP_MEAN[c]) / CLIP_STD[c];
        }
    }
    out
}
