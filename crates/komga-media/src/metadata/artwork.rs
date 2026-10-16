//! `LocalArtworkProvider.kt` consumption side: local artwork thumbnail discovery.
//!
//! Book artwork is any image next to the book named `<bookBaseName>(-<digits>)?.<ext>`;
//! series artwork is any image in the series folder named one of
//! `cover|default|folder|poster|series.<ext>`. Files are listed in directory-stream order and
//! the first match is marked selected.

use std::path::Path;

use komga_core::model::thumbnail::Dimension;

use crate::{detect, image, scanner};

const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpeg", "jpg", "tbn", "webp", "gif"];
const SUPPORTED_SERIES_FILES: &[&str] = &["cover", "default", "folder", "poster", "series"];

/// A discovered artwork file; ids, type, and the owner reference are filled by the service layer.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalArtworkDraft {
    pub url: String,
    pub selected: bool,
    pub file_size: i64,
    pub media_type: String,
    pub dimension: Dimension,
}

/// `getBookThumbnails`
pub fn get_book_thumbnails(book_path: &Path) -> Vec<LocalArtworkDraft> {
    let Some(parent) = book_path.parent() else {
        return vec![];
    };
    let base_name = book_path
        .file_name()
        .map(|n| kotlin_name_without_extension(&n.to_string_lossy()))
        .unwrap_or_default();
    let pattern = regex::RegexBuilder::new(&format!("^{}(-\\d+)?$", regex::escape(&base_name)))
        .case_insensitive(true)
        .build()
        .expect("escaped base name builds a valid regex");

    collect(parent, |stem, _| pattern.is_match(stem))
}

/// `getSeriesThumbnails`: empty for oneshot series (their cover is the book's own)
pub fn get_series_thumbnails(series_path: &Path, oneshot: bool) -> Vec<LocalArtworkDraft> {
    if oneshot {
        return vec![];
    }
    collect(series_path, |stem, _| {
        SUPPORTED_SERIES_FILES.contains(&stem.to_lowercase().as_str())
    })
}

fn collect(dir: &Path, name_matches: impl Fn(&str, &Path) -> bool) -> Vec<LocalArtworkDraft> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name_matches(&kotlin_name_without_extension(&name), &entry.path())
        })
        .filter(|entry| {
            let extension = entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            SUPPORTED_EXTENSIONS.contains(&extension.as_str())
        })
        .filter_map(|entry| {
            let bytes = std::fs::read(entry.path()).ok()?;
            let media_type = detect::detect_media_type(&bytes);
            detect::is_image(&media_type).then_some((entry, bytes, media_type))
        })
        .enumerate()
        .map(|(index, (entry, bytes, media_type))| LocalArtworkDraft {
            url: scanner::path_to_url(&entry.path()),
            selected: index == 0,
            file_size: bytes.len() as i64,
            dimension: image::get_dimension(&bytes)
                .map(|(w, h)| Dimension {
                    width: w as i32,
                    height: h as i32,
                })
                .unwrap_or_default(),
            media_type,
        })
        .collect()
}

/// `kotlin.io.path.nameWithoutExtension`: name up to the last `.` (or the whole name);
/// unlike Rust's `file_stem`, a leading dot counts as an extension separator (".cbz" → "").
fn kotlin_name_without_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(0) => String::new(),
        Some(i) => name[..i].to_string(),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1x1 red PNG
    fn png_bytes() -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        ::image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut cursor, ::image::ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }

    fn jpeg_bytes() -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        ::image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut cursor, ::image::ImageFormat::Jpeg)
            .unwrap();
        cursor.into_inner()
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    #[test]
    fn book_thumbnails_match_basename_and_dash_number() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("series");
        std::fs::create_dir_all(&root).unwrap();
        let png = png_bytes();
        let jpg = jpeg_bytes();

        write(&root, "book(e).cbz", b"PK\x03\x04");
        for name in [
            "bOOk(e).jpeg",
            "Book(e).tbn",
            "book(e).PNG",
            "book(e)-1.jpeg",
            "book(e)-2.tbn",
            "book(e)-23.png",
            "book(e)-111.jpeg",
            "book(e)-123.webp",
        ] {
            write(&root, name, &png);
        }
        // gif is a supported extension: komga's mocked detector excluded it, the real
        // magic-byte detector includes it when the content is an image
        write(&root, "book(e).gif", &png);
        // matches the pattern but is not a supported extension
        write(&root, "book(e).avif", &png);
        write(&root, "book(e).jxl", &png);
        // supported extension but the pattern does not match
        write(&root, "book12(e).jpeg", &png);
        write(&root, "cover.png", &png);
        write(&root, "other.jpeg", &png);
        write(&root, "book.webp", &png);
        // matches the pattern but is not an image
        write(&root, "book(e)-999.png", b"not an image");

        let drafts = get_book_thumbnails(&root.join("book(e).cbz"));
        let names: Vec<String> = drafts
            .iter()
            .map(|d| {
                komga_core::dto::url_to_file_path(&d.url)
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(drafts.len(), 9, "{names:?}");
        assert_eq!(drafts.iter().filter(|d| d.selected).count(), 1);
        assert!(names.contains(&"bOOk(e).jpeg".to_string()));
        assert!(names.contains(&"book(e)-123.webp".to_string()));
        assert!(names.contains(&"book(e).gif".to_string()));
        assert!(!names.contains(&"book(e)-999.png".to_string()));
        assert!(drafts.iter().all(|d| d.file_size == png.len() as i64));
        assert!(drafts.iter().all(|d| d.dimension.width == 1));
        let _ = jpg;
    }

    #[test]
    fn series_thumbnails_match_supported_names_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("series");
        std::fs::create_dir_all(&root).unwrap();
        let png = png_bytes();

        for name in [
            "CoVeR.jpeg",
            "DefauLt.tbn",
            "POSter.PNG",
            "FoLDer.jpeg",
            "serIES.TBN",
            "serIes.WebP",
        ] {
            write(&root, name, &png);
        }
        // gif is a supported extension: included when the content is an image
        write(&root, "cover.gif", &png);
        write(&root, "artwork.jpg", &png);
        write(&root, "other.jpeg", &png);
        write(&root, "cover.avif", &png);
        write(&root, "series.jxl", &png);
        write(&root, "series.png", b"not an image");

        let drafts = get_series_thumbnails(&root, false);
        assert_eq!(drafts.len(), 7);
        assert_eq!(drafts.iter().filter(|d| d.selected).count(), 1);
        assert!(drafts.iter().any(|d| d.url.ends_with("CoVeR.jpeg")));
        assert!(drafts.iter().any(|d| d.url.ends_with("serIes.WebP")));
        assert!(drafts.iter().any(|d| d.url.ends_with("cover.gif")));
        assert!(!drafts.iter().any(|d| d.url.ends_with("series.png")));

        assert!(get_series_thumbnails(&root, true).is_empty());
    }

    #[test]
    fn name_without_extension_matches_kotlin() {
        assert_eq!(kotlin_name_without_extension("foo.bar.cbz"), "foo.bar");
        assert_eq!(kotlin_name_without_extension(".cbz"), "");
        assert_eq!(kotlin_name_without_extension("foo"), "foo");
        assert_eq!(kotlin_name_without_extension("v01-2.png"), "v01-2");
    }
}
