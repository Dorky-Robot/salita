use image::{ImageBuffer, RgbImage};
use imagepipe::Pipeline;
use std::path::Path;

use crate::error::AppError;

/// Generate a thumbnail for a standard image (JPEG, PNG, etc.) from bytes.
pub fn generate_image_thumbnail(
    bytes: &[u8],
    max_w: u32,
    max_h: u32,
) -> Result<Vec<u8>, AppError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| AppError::Internal(format!("Failed to decode image: {e}")))?;
    let img = img.resize(max_w, max_h, image::imageops::FilterType::Lanczos3);
    encode_jpeg(&img.to_rgb8(), 80)
}

/// Generate a thumbnail for a RAW file from its path on disk.
/// imagepipe requires a file path so we read directly from the filesystem.
pub fn generate_raw_thumbnail(
    path: &Path,
    max_w: u32,
    max_h: u32,
) -> Result<Vec<u8>, AppError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| AppError::Internal("Invalid file path".into()))?;

    let mut pipeline = Pipeline::new_from_file(path_str)
        .map_err(|e| AppError::Internal(format!("Failed to open RAW: {e}")))?;

    let srgb = pipeline
        .output_8bit(None)
        .map_err(|e| AppError::Internal(format!("Pipeline error: {e}")))?;

    let img: RgbImage =
        ImageBuffer::from_raw(srgb.width as u32, srgb.height as u32, srgb.data)
            .ok_or_else(|| AppError::Internal("Failed to create image buffer".into()))?;

    let img = resize_if_needed(img, max_w, max_h);
    encode_jpeg(&img, 80)
}

/// Encode an RgbImage as JPEG bytes with the given quality.
pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>, AppError> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    image::ImageEncoder::write_image(
        encoder,
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )
    .map_err(|e| AppError::Internal(format!("JPEG encoding error: {e}")))?;
    Ok(buf.into_inner())
}

/// Generate a mid-res preview (1600px max, JPEG quality 82).
/// Good enough for fullscreen viewing, ~150-250KB.
pub fn generate_image_preview(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| AppError::Internal(format!("Failed to decode image: {e}")))?;
    let img = img.resize(1600, 1600, image::imageops::FilterType::Lanczos3);
    encode_jpeg(&img.to_rgb8(), 82)
}

/// Generate a mid-res preview for a RAW file.
pub fn generate_raw_preview(path: &Path) -> Result<Vec<u8>, AppError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| AppError::Internal("Invalid file path".into()))?;

    let mut pipeline = Pipeline::new_from_file(path_str)
        .map_err(|e| AppError::Internal(format!("Failed to open RAW: {e}")))?;

    let srgb = pipeline
        .output_8bit(None)
        .map_err(|e| AppError::Internal(format!("Pipeline error: {e}")))?;

    let img: RgbImage =
        ImageBuffer::from_raw(srgb.width as u32, srgb.height as u32, srgb.data)
            .ok_or_else(|| AppError::Internal("Failed to create image buffer".into()))?;

    let img = resize_if_needed(img, 1600, 1600);
    encode_jpeg(&img, 82)
}

fn resize_if_needed(img: RgbImage, max_w: u32, max_h: u32) -> RgbImage {
    if max_w == 0 && max_h == 0 {
        return img;
    }

    let (w, h) = (img.width(), img.height());
    let target_w = if max_w > 0 { max_w } else { w };
    let target_h = if max_h > 0 { max_h } else { h };

    if w <= target_w && h <= target_h {
        return img;
    }

    let scale = f64::min(target_w as f64 / w as f64, target_h as f64 / h as f64);
    let new_w = (w as f64 * scale) as u32;
    let new_h = (h as f64 * scale) as u32;

    let dynamic = image::DynamicImage::ImageRgb8(img);
    dynamic
        .resize(new_w, new_h, image::imageops::FilterType::Lanczos3)
        .to_rgb8()
}
