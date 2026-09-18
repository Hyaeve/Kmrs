//! Image operations, ported from `ImageAnalyzer.kt` and `ImageConverter.kt`.
//!
//! Byte-level output differs from ImageIO (different JPEG encoder); komga already accepts that
//! deviation for thumbnails and page hashing (see project plan §7-1).

use crate::detect;
use crate::error::{MediaError, Result};
use image::{DynamicImage, ImageFormat, ImageReader, Rgb, RgbImage};
use std::io::Cursor;

/// `ImageType.kt`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Png,
    Jpeg,
}

impl ImageType {
    pub fn media_type(self) -> &'static str {
        match self {
            ImageType::Png => detect::IMAGE_PNG,
            ImageType::Jpeg => detect::IMAGE_JPEG,
        }
    }

    fn image_format(self) -> ImageFormat {
        match self {
            ImageType::Png => ImageFormat::Png,
            ImageType::Jpeg => ImageFormat::Jpeg,
        }
    }
}

/// `ImageAnalyzer.getDimension`
pub fn get_dimension(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok()
}

fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| MediaError::Conversion(format!("could not read image: {e}")))?
        .decode()
        .map_err(|e| MediaError::Conversion(format!("could not decode image: {e}")))
}

fn encode(image: &DynamicImage, format: ImageType) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, format.image_format())
        .map_err(|e| MediaError::Conversion(format!("could not encode image: {e}")))?;
    Ok(out.into_inner())
}

/// JPEG encoder shared with the PDF renderer
pub fn encode_jpeg(image: &DynamicImage) -> Result<Vec<u8>> {
    encode(image, ImageType::Jpeg)
}

/// `ImageConverter.resizeImageToByteArray`: fit within `size` px on the longest edge, keeping the
/// aspect ratio. Returns the original bytes when the source is already smaller and the target
/// format matches (komga's skip-resize shortcut).
pub fn resize(bytes: &[u8], format: ImageType, size: u32) -> Result<Vec<u8>> {
    let longest_edge = get_dimension(bytes).map(|(w, h)| {
        let media_type = detect::detect_media_type(bytes);
        let longest_edge = w.max(h);
        if media_type == format.media_type() && longest_edge <= size {
            return None;
        }
        Some(longest_edge)
    });

    match longest_edge {
        Some(None) => Ok(bytes.to_vec()),
        Some(Some(longest_edge)) => {
            let resize_to = longest_edge.min(size);
            let image = decode(bytes)?;
            let resized = image.resize(resize_to, resize_to, image::imageops::FilterType::Lanczos3);
            encode_for_format(resized, format)
        }
        // dimensions unknown: resize blind to the requested size, like Thumbnailator does
        None => {
            let image = decode(bytes)?;
            let resized = image.resize(size, size, image::imageops::FilterType::Lanczos3);
            encode_for_format(resized, format)
        }
    }
}

/// `ImageConverter.convertImage`: re-encode to the target format.
/// JPEG does not support transparency; alpha is composited onto a white background first,
/// matching the Java behavior.
pub fn convert(bytes: &[u8], format: ImageType) -> Result<Vec<u8>> {
    let image = decode(bytes)?;
    encode_for_format(image, format)
}

fn encode_for_format(image: DynamicImage, format: ImageType) -> Result<Vec<u8>> {
    match format {
        ImageType::Jpeg if image.color().has_alpha() => {
            // SrcOver onto white, like drawImage(image, 0, 0, Color.WHITE, null)
            let rgba = image.to_rgba8();
            let (w, h) = rgba.dimensions();
            let mut rgb = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
            for (x, y, p) in rgba.enumerate_pixels() {
                let alpha = p[3] as f32 / 255.0;
                let dst = rgb.get_pixel_mut(x, y);
                for c in 0..3 {
                    dst[c] = (p[c] as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
                }
            }
            encode(&DynamicImage::ImageRgb8(rgb), format)
        }
        _ => encode(&image, format),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../komga/komga/src/test/resources")
    }

    fn zip_entry_bytes(name: &str) -> Vec<u8> {
        let zip_path = fixtures().join("archives/zip.zip");
        let file = std::fs::File::open(zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut entry = archive.by_name(name).unwrap();
        let mut buf = vec![];
        std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
        buf
    }

    fn make_png(w: u32, h: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            w,
            h,
            image::Rgba([10, 200, 30, 128]),
        ));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
        let image =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 10])));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[test]
    fn dimension_from_fixture_image() {
        let bytes = zip_entry_bytes("komga.png");
        assert_eq!(get_dimension(&bytes), Some((48, 48)));
        assert_eq!(detect::detect_media_type(&bytes), detect::IMAGE_PNG);
    }

    #[test]
    fn dimension_unknown_format() {
        assert_eq!(get_dimension(b"not an image"), None);
    }

    #[test]
    fn resize_shrinks_longest_edge() {
        let png = make_png(800, 400);
        let out = resize(&png, ImageType::Jpeg, 300).unwrap();
        assert_eq!(get_dimension(&out), Some((300, 150)));
        assert_eq!(detect::detect_media_type(&out), detect::IMAGE_JPEG);
    }

    #[test]
    fn resize_never_upscales() {
        let png = make_png(200, 100);
        let out = resize(&png, ImageType::Jpeg, 300).unwrap();
        assert_eq!(get_dimension(&out), Some((200, 100)));
    }

    #[test]
    fn resize_skips_when_same_format_and_smaller() {
        let bytes = make_jpeg(48, 48);
        let out = resize(&bytes, ImageType::Jpeg, 900).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn convert_png_with_alpha_to_jpeg() {
        let png = make_png(40, 40);
        let out = convert(&png, ImageType::Jpeg).unwrap();
        assert_eq!(detect::detect_media_type(&out), detect::IMAGE_JPEG);
        let (r, g, b) = {
            let img = decode(&out).unwrap().to_rgb8();
            let p = img.get_pixel(0, 0).0;
            (p[0], p[1], p[2])
        };
        // semi-transparent (10, 200, 30, 128) over white is close to (132, 227, 142)
        assert!((100..=165).contains(&r) && (210..=245).contains(&g) && (110..=175).contains(&b));
    }

    #[test]
    fn convert_jpeg_to_png() {
        let bytes = make_jpeg(48, 48);
        let out = convert(&bytes, ImageType::Png).unwrap();
        assert_eq!(detect::detect_media_type(&out), detect::IMAGE_PNG);
    }

    #[test]
    fn convert_rejects_non_image() {
        assert!(matches!(
            convert(b"junk", ImageType::Jpeg),
            Err(MediaError::Conversion(_))
        ));
    }
}
