//! `IsbnBarcodeProvider.kt`: EAN-13 barcode detection on book pages.
//!
//! Pages are tried from the end of the book backwards, then from the start (ISBN barcodes live
//! on back covers, then front covers); the first valid ISBN wins.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use komga_core::model::library::Library;
use komga_core::model::media::Media;
use komga_core::search::MediaProfile;
use komga_core::task::BookMetadataPatchCapability;
use rxing::{
    common::HybridBinarizer, BarcodeFormat, BinaryBitmap, DecodeHintValue, DecodeHints,
    MultiFormatReader, RGBLuminanceSource,
};

use crate::container;
use crate::metadata::patch::{
    isbn_validate, BookMetadataPatch, BookMetadataProvider, MetadataPatchTarget, MetadataProvider,
};

const PAGES_LAST: usize = 3;
const PAGES_FIRST: usize = 3;

pub struct IsbnBarcodeProvider {
    capabilities: BTreeSet<BookMetadataPatchCapability>,
}

impl IsbnBarcodeProvider {
    pub fn new() -> Self {
        Self {
            capabilities: [BookMetadataPatchCapability::Isbn].into_iter().collect(),
        }
    }
}

impl Default for IsbnBarcodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataProvider for IsbnBarcodeProvider {
    /// `shouldLibraryHandlePatch`: only BOOK, gated by the library's `importBarcodeIsbn`
    fn should_library_handle_patch(&self, library: &Library, target: MetadataPatchTarget) -> bool {
        matches!(target, MetadataPatchTarget::Book) && library.import_barcode_isbn
    }
}

impl BookMetadataProvider for IsbnBarcodeProvider {
    fn capabilities(&self) -> &BTreeSet<BookMetadataPatchCapability> {
        &self.capabilities
    }

    /// `getBookMetadataFromBook`: the first valid ISBN barcode found
    fn get_book_metadata_from_book(
        &self,
        book_path: &Path,
        media: &Media,
    ) -> Option<BookMetadataPatch> {
        if container::media_profile(media.media_type.as_deref()) == Some(MediaProfile::Epub) {
            return None;
        }

        let page_count = media.page_count.max(0) as usize;
        let pages_to_try = (1..=page_count)
            .rev()
            .take(PAGES_LAST)
            .chain((1..=page_count).take(PAGES_FIRST))
            .fold(Vec::new(), |mut pages, page| {
                // `distinct()` keeps the first occurrence, so last pages win over first pages
                if !pages.contains(&page) {
                    pages.push(page);
                }
                pages
            });

        for page in pages_to_try {
            match try_page(book_path, media, page) {
                Ok(Some(patch)) => return Some(patch),
                Ok(None) => {}
                Err(e) => tracing::error!("Error while processing page: {e}"),
            }
        }
        None
    }
}

fn try_page(
    book_path: &Path,
    media: &Media,
    page: usize,
) -> crate::Result<Option<BookMetadataPatch>> {
    let bytes = container::get_page_content(book_path, media, page)?;
    let Some(text) = decode_ean13(&bytes) else {
        return Ok(None);
    };
    match isbn_validate(&text) {
        Some(isbn) => Ok(Some(BookMetadataPatch {
            isbn: Some(isbn),
            ..Default::default()
        })),
        None => {
            tracing::debug!("Page {page} contains barcode which is invalid ISBN: '{text}'");
            Ok(None)
        }
    }
}

/// ZXing EAN_13 + TRY_HARDER on the decoded page pixels
fn decode_ean13(bytes: &[u8]) -> Option<String> {
    let rgb = image::load_from_memory(bytes).ok()?.to_rgb8();
    let (width, height) = (rgb.width() as usize, rgb.height() as usize);
    let pixels: Vec<u32> = rgb
        .chunks_exact(3)
        .map(|c| ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32)
        .collect();
    let source = RGBLuminanceSource::new_with_width_height_pixels(width, height, &pixels).ok()?;
    let mut bitmap = BinaryBitmap::new(HybridBinarizer::new(source));
    let mut reader = MultiFormatReader::default();
    reader.set_hints(
        &DecodeHints::default()
            .with(DecodeHintValue::PossibleFormats(HashSet::from([
                BarcodeFormat::EAN_13,
            ])))
            .with(DecodeHintValue::TryHarder(true)),
    );
    reader
        .decode_with_state(&mut bitmap)
        .ok()
        .map(|result| result.getText().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::media::{BookPage, MediaStatus};
    use komga_core::time_codec::now_utc;
    use std::io::Write;

    fn fixtures() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources")
    }

    fn zip_media(file_names: &[(&str, &str)]) -> Media {
        Media {
            book_id: "b1".into(),
            status: MediaStatus::Ready,
            media_type: Some(crate::detect::APPLICATION_ZIP.into()),
            comment: None,
            page_count: file_names.len() as i32,
            pages: file_names
                .iter()
                .map(|(name, media_type)| BookPage {
                    file_name: name.to_string(),
                    media_type: media_type.to_string(),
                    width: None,
                    height: None,
                    file_hash: String::new(),
                    file_size: None,
                })
                .collect(),
            files: vec![],
            extension_class: None,
            extension_value: None,
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn make_zip(dir: &Path, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = dir.join("book.cbz");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    fn library(import_barcode_isbn: bool) -> Library {
        Library {
            id: "l1".into(),
            name: "lib".into(),
            root: "file:/data/".into(),
            import_comicinfo_book: false,
            import_comicinfo_series: false,
            import_comicinfo_collection: false,
            import_comicinfo_readlist: false,
            import_comicinfo_series_append_volume: false,
            import_epub_book: false,
            import_epub_series: false,
            import_mylar_series: false,
            import_local_artwork: false,
            import_barcode_isbn,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: komga_core::model::library::ScanInterval::Disabled,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: komga_core::model::library::SeriesCover::First,
            hash_files: false,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: false,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn fixture_page_has_isbn() {
        let page = fixtures().join("barcode/page_384.jpg");
        let bytes = std::fs::read(&page).unwrap();
        assert_eq!(
            decode_ean13(&bytes)
                .and_then(|t| isbn_validate(&t))
                .as_deref(),
            Some("9782811632397")
        );
    }

    #[test]
    fn fixture_page_without_barcode() {
        let page = fixtures().join("barcode/komga.png");
        let bytes = std::fs::read(&page).unwrap();
        assert!(decode_ean13(&bytes).is_none());
    }

    #[test]
    fn book_with_barcode_page_returns_patch() {
        let dir = tempfile::tempdir().unwrap();
        let jpg = std::fs::read(fixtures().join("barcode/page_384.jpg")).unwrap();
        let book = make_zip(dir.path(), &[("p1.jpg", &jpg)]);
        let media = zip_media(&[("p1.jpg", "image/jpeg")]);

        let patch = IsbnBarcodeProvider::new()
            .get_book_metadata_from_book(&book, &media)
            .unwrap();
        assert_eq!(patch.isbn.as_deref(), Some("9782811632397"));
    }

    #[test]
    fn book_without_barcode_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let png = std::fs::read(fixtures().join("barcode/komga.png")).unwrap();
        let book = make_zip(dir.path(), &[("p1.png", &png)]);
        let media = zip_media(&[("p1.png", "image/png")]);

        assert!(IsbnBarcodeProvider::new()
            .get_book_metadata_from_book(&book, &media)
            .is_none());
    }

    #[test]
    fn epub_profile_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let jpg = std::fs::read(fixtures().join("barcode/page_384.jpg")).unwrap();
        let book = make_zip(dir.path(), &[("p1.jpg", &jpg)]);
        let mut media = zip_media(&[("p1.jpg", "image/jpeg")]);
        media.media_type = Some(crate::detect::APPLICATION_EPUB.into());

        assert!(IsbnBarcodeProvider::new()
            .get_book_metadata_from_book(&book, &media)
            .is_none());
    }

    #[test]
    fn last_pages_are_tried_before_first_pages() {
        let dir = tempfile::tempdir().unwrap();
        let jpg = std::fs::read(fixtures().join("barcode/page_384.jpg")).unwrap();
        let png = std::fs::read(fixtures().join("barcode/komga.png")).unwrap();
        // barcode on the last page: must be found even though earlier pages have none
        let book = make_zip(
            dir.path(),
            &[
                ("p1.png", &png),
                ("p2.png", &png),
                ("p3.png", &png),
                ("p4.png", &png),
                ("p5.png", &png),
                ("p6.png", &png),
                ("p7.png", &png),
                ("p8.jpg", &jpg),
            ],
        );
        let media = zip_media(&[
            ("p1.png", "image/png"),
            ("p2.png", "image/png"),
            ("p3.png", "image/png"),
            ("p4.png", "image/png"),
            ("p5.png", "image/png"),
            ("p6.png", "image/png"),
            ("p7.png", "image/png"),
            ("p8.jpg", "image/jpeg"),
        ]);

        let patch = IsbnBarcodeProvider::new()
            .get_book_metadata_from_book(&book, &media)
            .unwrap();
        assert_eq!(patch.isbn.as_deref(), Some("9782811632397"));
    }

    #[test]
    fn library_gate() {
        let provider = IsbnBarcodeProvider::new();
        let mut library = library(false);
        assert!(!provider.should_library_handle_patch(&library, MetadataPatchTarget::Book));
        library.import_barcode_isbn = true;
        assert!(provider.should_library_handle_patch(&library, MetadataPatchTarget::Book));
        assert!(!provider.should_library_handle_patch(&library, MetadataPatchTarget::Series));
        assert!(!provider.should_library_handle_patch(&library, MetadataPatchTarget::ReadList));
        assert!(!provider.should_library_handle_patch(&library, MetadataPatchTarget::Collection));
        assert!(provider
            .capabilities()
            .contains(&BookMetadataPatchCapability::Isbn));
        assert_eq!(provider.capabilities().len(), 1);
    }
}
