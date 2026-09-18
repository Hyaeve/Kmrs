//! Media type sniffing: the subset of Tika 3.3.2 (`tika-mimetypes.xml`) detection that komga
//! relies on, ported from `ContentDetector.kt`.
//!
//! Detection order follows Tika's magic priority: container formats (epub via zip mimetype
//! entry, rar versions) are checked before generic zip, images before the octet-stream fallback.

use crate::error::{MediaError, Result};
use std::io::Read;

pub const APPLICATION_ZIP: &str = "application/zip";
pub const APPLICATION_EPUB: &str = "application/epub+zip";
pub const APPLICATION_RAR_4: &str = "application/x-rar-compressed; version=4";
pub const APPLICATION_RAR_5: &str = "application/x-rar-compressed; version=5";
pub const APPLICATION_7Z: &str = "application/x-7z-compressed";
pub const APPLICATION_PDF: &str = "application/pdf";
pub const APPLICATION_OCTET_STREAM: &str = "application/octet-stream";
pub const IMAGE_JPEG: &str = "image/jpeg";
pub const IMAGE_PNG: &str = "image/png";
pub const IMAGE_GIF: &str = "image/gif";
pub const IMAGE_WEBP: &str = "image/webp";
pub const IMAGE_TIFF: &str = "image/tiff";
pub const IMAGE_BMP: &str = "image/bmp";
pub const IMAGE_JXL: &str = "image/jxl";
pub const IMAGE_HEIF: &str = "image/heif";
pub const IMAGE_HEIC: &str = "image/heic";
pub const IMAGE_AVIF: &str = "image/avif";
pub const IMAGE_JP2: &str = "image/jp2";

/// `ContentDetector.detectMediaType(InputStream)`: detects from content only.
pub fn detect_media_type(bytes: &[u8]) -> String {
    if bytes.len() >= 4 && &bytes[0..4] == b"PK\x03\x04" {
        return detect_zip_container(bytes);
    }
    if bytes.starts_with(b"Rar!\x1A\x07\x00") {
        return APPLICATION_RAR_4.to_string();
    }
    if bytes.starts_with(b"Rar!\x1A\x07\x01\x00") {
        return APPLICATION_RAR_5.to_string();
    }
    if bytes.starts_with(b"%PDF-") {
        return APPLICATION_PDF.to_string();
    }
    if bytes.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return APPLICATION_7Z.to_string();
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return IMAGE_JPEG.to_string();
    }
    if bytes.starts_with(b"\x89PNG\x0D\x0A\x1A\x0A") {
        return IMAGE_PNG.to_string();
    }
    if bytes.starts_with(b"GIF8") {
        return IMAGE_GIF.to_string();
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return IMAGE_WEBP.to_string();
    }
    if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        return IMAGE_TIFF.to_string();
    }
    if bytes.starts_with(b"BM") {
        return IMAGE_BMP.to_string();
    }
    // JXL codestream and container signatures
    if bytes.starts_with(b"\xFF\x0A") || bytes.starts_with(b"\x00\x00\x00\x0CJXL \x0D\x0A\x87\x0A")
    {
        return IMAGE_JXL.to_string();
    }
    // JPEG 2000 signature box
    if bytes.starts_with(b"\x00\x00\x00\x0CjP  \x0D\x0A\x87\x0A") {
        return IMAGE_JP2.to_string();
    }
    if let Some(heif) = detect_heif(bytes) {
        return heif.to_string();
    }
    APPLICATION_OCTET_STREAM.to_string()
}

/// ISO-BMFF `ftyp` brands: heif/heic/avif family
fn detect_heif(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return None;
    }
    let brand = &bytes[8..12];
    Some(match brand {
        b"mif1" | b"msf1" => IMAGE_HEIF,
        b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"hevm" | b"hevs" => IMAGE_HEIC,
        b"avif" | b"avis" => IMAGE_AVIF,
        _ => return None,
    })
}

/// Tika's ZipContainerDetector: a zip whose `mimetype` entry names a container format gets that
/// format; anything else stays a plain zip.
fn detect_zip_container(bytes: &[u8]) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let Ok(mut archive) = zip::ZipArchive::new(cursor) else {
        return APPLICATION_ZIP.to_string();
    };
    let Ok(mut entry) = archive.by_name("mimetype") else {
        return APPLICATION_ZIP.to_string();
    };
    let mut content = String::new();
    if entry.read_to_string(&mut content).is_err() {
        return APPLICATION_ZIP.to_string();
    }
    let content = content.trim();
    if content.is_empty() {
        APPLICATION_ZIP.to_string()
    } else {
        content.to_string()
    }
}

/// `ContentDetector.isImage`
pub fn is_image(media_type: &str) -> bool {
    media_type.starts_with("image/")
}

/// `ContentDetector.mediaTypeToExtension`: Tika's default extension for the type
/// (first glob pattern), including the leading dot.
pub fn media_type_to_extension(media_type: &str) -> Option<&'static str> {
    // parameters are ignored, like Tika's MimeTypes.forName
    let base = media_type.split(';').next()?.trim();
    Some(match base {
        APPLICATION_ZIP => ".zip",
        APPLICATION_EPUB => ".epub",
        "application/x-rar-compressed" => ".rar",
        APPLICATION_PDF => ".pdf",
        APPLICATION_7Z => ".7z",
        IMAGE_JPEG => ".jpg",
        IMAGE_PNG => ".png",
        IMAGE_GIF => ".gif",
        IMAGE_WEBP => ".webp",
        IMAGE_TIFF => ".tiff",
        IMAGE_BMP => ".bmp",
        IMAGE_JXL => ".jxl",
        IMAGE_HEIF => ".heif",
        IMAGE_HEIC => ".heic",
        IMAGE_AVIF => ".avif",
        IMAGE_JP2 => ".jp2",
        _ => return None,
    })
}

/// `getMediaTypeOrDefault`: parse the media type, falling back to octet-stream
pub fn media_type_or_default(media_type: Option<&str>) -> String {
    match media_type {
        Some(s) if is_valid_media_type(s) => s.to_string(),
        _ => APPLICATION_OCTET_STREAM.to_string(),
    }
}

/// Spring's `MediaType.parseMediaType` accepts `type/subtype` with optional `;` parameters
fn is_valid_media_type(s: &str) -> bool {
    let Some((type_, rest)) = s.split_once('/') else {
        return false;
    };
    let subtype = rest.split(';').next().unwrap_or("");
    let is_token = |t: &str| {
        !t.is_empty()
            && t.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(
                        b,
                        b'!' | b'#'
                            | b'$'
                            | b'%'
                            | b'&'
                            | b'\''
                            | b'*'
                            | b'+'
                            | b'-'
                            | b'.'
                            | b'^'
                            | b'_'
                            | b'`'
                            | b'|'
                            | b'~'
                    )
            })
    };
    is_token(type_) && is_token(subtype)
}

/// Convenience for callers that detect from a reader's head; reads up to `limit` bytes.
pub fn detect_media_type_reader(reader: &mut impl Read, limit: usize) -> Result<String> {
    let mut buf = vec![0u8; limit];
    let mut filled = 0;
    loop {
        let n = reader
            .read(&mut buf[filled..])
            .map_err(|e| MediaError::Other(e.into()))?;
        if n == 0 {
            break;
        }
        filled += n;
        if filled == limit {
            break;
        }
    }
    buf.truncate(filled);
    Ok(detect_media_type(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(hex: &str) -> String {
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        detect_media_type(&bytes)
    }

    #[test]
    fn containers() {
        assert_eq!(detect("526172211A0700"), APPLICATION_RAR_4);
        assert_eq!(detect("526172211A070100"), APPLICATION_RAR_5);
        assert_eq!(detect_media_type(b"%PDF-1.7\n"), APPLICATION_PDF);
        assert_eq!(detect("377ABCAF271C"), APPLICATION_7Z);
        assert_eq!(detect_media_type(b"PK\x03\x04garbage"), APPLICATION_ZIP);
    }

    #[test]
    fn images() {
        assert_eq!(detect("FFD8FFE0"), IMAGE_JPEG);
        assert_eq!(detect("89504E470D0A1A0A"), IMAGE_PNG);
        assert_eq!(detect_media_type(b"GIF89a"), IMAGE_GIF);
        assert_eq!(detect_media_type(b"RIFF\x00\x00\x00\x00WEBP"), IMAGE_WEBP);
        assert_eq!(detect_media_type(b"II*\x00"), IMAGE_TIFF);
        assert_eq!(detect_media_type(b"MM\x00*"), IMAGE_TIFF);
        assert_eq!(detect_media_type(b"BM6"), IMAGE_BMP);
        assert_eq!(detect("FF0A"), IMAGE_JXL);
        assert_eq!(detect("0000000C4A584C200D0A870A"), IMAGE_JXL);
        assert_eq!(detect("0000000C6A5020200D0A870A"), IMAGE_JP2);
    }

    #[test]
    fn heif_brands() {
        let ftyp = |brand: &[u8; 4]| {
            let mut v = b"\x00\x00\x00\x18ftyp".to_vec();
            v.extend_from_slice(brand);
            v.extend_from_slice(b"\x00\x00\x00\x00");
            v
        };
        assert_eq!(detect_media_type(&ftyp(b"mif1")), IMAGE_HEIF);
        assert_eq!(detect_media_type(&ftyp(b"msf1")), IMAGE_HEIF);
        assert_eq!(detect_media_type(&ftyp(b"heic")), IMAGE_HEIC);
        assert_eq!(detect_media_type(&ftyp(b"heix")), IMAGE_HEIC);
        assert_eq!(detect_media_type(&ftyp(b"avif")), IMAGE_AVIF);
        assert_eq!(detect_media_type(&ftyp(b"avis")), IMAGE_AVIF);
        assert_eq!(detect_media_type(&ftyp(b"jP2 ")), APPLICATION_OCTET_STREAM);
    }

    #[test]
    fn unknown_is_octet_stream() {
        assert_eq!(detect_media_type(b""), APPLICATION_OCTET_STREAM);
        assert_eq!(detect_media_type(b"\x01\x02\x03"), APPLICATION_OCTET_STREAM);
    }

    #[test]
    fn epub_detection_from_fixture() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../komga/komga/src/test/resources");
        let epub = std::fs::read(dir.join("archives/epub3.epub")).unwrap();
        assert_eq!(detect_media_type(&epub), APPLICATION_EPUB);
        let zip = std::fs::read(dir.join("archives/zip.zip")).unwrap();
        assert_eq!(detect_media_type(&zip), APPLICATION_ZIP);
        let rar4 = std::fs::read(dir.join("archives/rar4.rar")).unwrap();
        assert_eq!(detect_media_type(&rar4), APPLICATION_RAR_4);
        let rar5 = std::fs::read(dir.join("archives/rar5.rar")).unwrap();
        assert_eq!(detect_media_type(&rar5), APPLICATION_RAR_5);
        let pdf = std::fs::read(dir.join("pdf/komga.pdf")).unwrap();
        assert_eq!(detect_media_type(&pdf), APPLICATION_PDF);
    }

    #[test]
    fn extensions() {
        assert_eq!(media_type_to_extension("image/jpeg"), Some(".jpg"));
        assert_eq!(media_type_to_extension("image/png"), Some(".png"));
        assert_eq!(media_type_to_extension(APPLICATION_RAR_4), Some(".rar"));
        assert_eq!(media_type_to_extension(APPLICATION_RAR_5), Some(".rar"));
        assert_eq!(
            media_type_to_extension("application/epub+zip"),
            Some(".epub")
        );
        assert_eq!(media_type_to_extension("application/unknown"), None);
    }

    #[test]
    fn media_type_validation() {
        assert_eq!(media_type_or_default(Some("image/jpeg")), "image/jpeg");
        assert_eq!(
            media_type_or_default(Some(APPLICATION_RAR_4)),
            APPLICATION_RAR_4
        );
        assert_eq!(
            media_type_or_default(Some("not a type")),
            APPLICATION_OCTET_STREAM
        );
        assert_eq!(media_type_or_default(None), APPLICATION_OCTET_STREAM);
        assert_eq!(
            media_type_or_default(Some("text/")),
            APPLICATION_OCTET_STREAM
        );
    }
}
