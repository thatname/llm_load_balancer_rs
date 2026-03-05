use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageEncoder, ImageFormat};
use serde_json::{Map, Value};
use tracing::debug;

/// Converts a WebP base64 data URI to JPEG base64 data URI
/// Returns the new data URI with JPEG format
fn convert_webp_to_jpeg(webp_data_uri: &str) -> Result<String> {
    // Extract the base64 data from the data URI
    let base64_data = webp_data_uri
        .strip_prefix("data:image/webp;base64,")
        .context("Invalid WebP data URI format")?;

    // Decode the base64 data using base64 crate
    let webp_bytes = STANDARD
        .decode(base64_data)
        .context("Failed to decode base64 WebP data")?;

    // Decode the WebP image using image crate
    let image = image::ImageReader::new(std::io::Cursor::new(&webp_bytes))
        .with_guessed_format()
        .context("Failed to guess image format")?
        .decode()
        .context("Failed to decode WebP image")?;

    // Convert to RGB (JPEG doesn't support alpha channel)
    let rgb_image = image.to_rgb8();

    // Encode as JPEG using image crate's ImageEncoder
    let mut jpeg_bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg_bytes);
    encoder
        .encode_image(&rgb_image)
        .context("Failed to encode JPEG image")?;

    // Encode as base64 using base64 crate
    let jpeg_base64 = STANDARD.encode(&jpeg_bytes);

    Ok(format!("data:image/jpeg;base64,{}", jpeg_base64))
}

/// Recursively processes a JSON value, converting WebP images to JPEG
pub fn convert_webp_images_to_jpeg(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut new_map = Map::new();
            for (key, val) in map {
                let converted = convert_webp_images_to_jpeg(val);
                new_map.insert(key.clone(), converted);
            }
            Value::Object(new_map)
        }
        Value::Array(arr) => {
            let new_arr: Vec<Value> = arr
                .iter()
                .map(|item| convert_webp_images_to_jpeg(item))
                .collect();
            Value::Array(new_arr)
        }
        Value::String(s) => {
            // Check if this is a WebP data URI
            if s.starts_with("data:image/webp;base64,") {
                match convert_webp_to_jpeg(s) {
                    Ok(jpeg_uri) => {
                        debug!("Converted WebP to JPEG");
                        Value::String(jpeg_uri)
                    }
                    Err(e) => {
                        debug!("Failed to convert WebP to JPEG: {}", e);
                        value.clone()
                    }
                }
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_webp_to_jpeg() {
        // This is a minimal valid WebP file in base64
        let webp_uri = "data:image/webp;base64,UklGRi4AAABXRUJQVlA4IEoAAADQAQCdASoBAAEAAUAmJYgCdAEO/hOCqGf+2P/bP9s/2f/aP9o/2T/aP9g/2D/YH9f/1f/V/9P/0//a/9r/2v/a/9n/2v/a/9f/1f/V/9X/1P/V/9T/0//a/9r/2v/a/9n/2v/a/9f/1f/V/9X/1P/V/9T/0//a/9r/2v/a/9n/2v/a/9f/1f/V/9X/1P/V/9T/0==";
        let result = convert_webp_to_jpeg(webp_uri);
        assert!(result.is_ok());
        let jpeg_uri = result.unwrap();
        assert!(jpeg_uri.starts_with("data:image/jpeg;base64,"));
    }
}