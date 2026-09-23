//! Profile-based dispatch for page/file extraction, ported from `BookAnalyzer.kt`
//! (`getPageContent` / `getPageContentRaw` / `getFileContent`) and `BookLifecycle.getBookPage`.
//!
//! Page numbers are 1-based, as in komga's REST API.

use crate::error::{MediaError, Result};
use crate::image::ImageType;
use crate::{detect, image, pdf, rar, zip};
use komga_core::model::media::{Media, MediaStatus};
use komga_core::search::MediaProfile;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct PageContent {
    pub bytes: Vec<u8>,
    pub media_type: String,
}

/// `MediaType.fromMediaType(mediaType)?.profile`
pub fn media_profile(media_type: Option<&str>) -> Option<MediaProfile> {
    Some(match media_type? {
        detect::APPLICATION_ZIP
        | "application/x-rar-compressed"
        | detect::APPLICATION_RAR_4
        | detect::APPLICATION_RAR_5 => MediaProfile::Divina,
        detect::APPLICATION_EPUB => MediaProfile::Epub,
        detect::APPLICATION_PDF => MediaProfile::Pdf,
        _ => return None,
    })
}

/// `BookAnalyzer.getPageContent`: the page bytes straight from the container.
/// PDF pages are rendered to JPEG; EPUB pages only exist for divina-compatible books.
pub fn get_page_content(book_path: &Path, media: &Media, number: usize) -> Result<Vec<u8>> {
    if media.status != MediaStatus::Ready {
        return Err(MediaError::NotReady);
    }
    if number > media.page_count as usize || number == 0 {
        return Err(MediaError::PageOutOfBounds(number));
    }

    match media_profile(media.media_type.as_deref()) {
        Some(MediaProfile::Divina) => {
            get_divina_entry(book_path, media, &media.pages[number - 1].file_name)
        }
        Some(MediaProfile::Pdf) => pdf::get_page_content_as_image(book_path, number),
        Some(MediaProfile::Epub) => {
            if media.epub_divina_compatible {
                zip::get_entry_bytes(book_path, &media.pages[number - 1].file_name)
            } else {
                Err(MediaError::unsupported(
                    "Epub profile does not support getting page content",
                ))
            }
        }
        None => Err(MediaError::NotReady),
    }
}

/// Batch variant of `get_page_content` for the page-hashing path: reads the requested
/// pages (1-based, output follows `numbers`) with a single container open per book instead
/// of one open per page — network mounts charge per open. A page outside `[1, page_count]`
/// fails like the single-page function; any missing entry fails the whole batch.
pub fn get_pages_content(
    book_path: &Path,
    media: &Media,
    numbers: &[usize],
) -> Result<Vec<Vec<u8>>> {
    if media.status != MediaStatus::Ready {
        return Err(MediaError::NotReady);
    }
    if numbers.is_empty() {
        return Ok(vec![]);
    }
    if let Some(&n) = numbers
        .iter()
        .find(|&&n| n == 0 || n > media.page_count as usize)
    {
        return Err(MediaError::PageOutOfBounds(n));
    }
    let names: Vec<&str> = numbers
        .iter()
        .map(|&n| media.pages[n - 1].file_name.as_str())
        .collect();
    match media_profile(media.media_type.as_deref()) {
        Some(MediaProfile::Divina) => match media.media_type.as_deref() {
            Some(detect::APPLICATION_ZIP) | Some(detect::APPLICATION_EPUB) => {
                zip::get_entries_bytes(book_path, &names)
            }
            Some("application/x-rar-compressed")
            | Some(detect::APPLICATION_RAR_4)
            | Some(detect::APPLICATION_RAR_5) => rar::get_entries_bytes(book_path, &names),
            Some(other) => Err(MediaError::unsupported(format!(
                "no divina extractor for media type {other}"
            ))),
            None => Err(MediaError::NotReady),
        },
        Some(MediaProfile::Pdf) => pdf::get_pages_content_as_images(book_path, numbers),
        Some(MediaProfile::Epub) => {
            if media.epub_divina_compatible {
                zip::get_entries_bytes(book_path, &names)
            } else {
                Err(MediaError::unsupported(
                    "Epub profile does not support getting page content",
                ))
            }
        }
        None => Err(MediaError::NotReady),
    }
}

/// A book container opened once, with per-page reads on the same handle. Used where
/// candidates must be consumed lazily in priority order with early termination and
/// per-page fault tolerance (e.g. the barcode scan), as opposed to the eager
/// `get_pages_content` which materializes every requested page up front.
///
/// ZIP / EPUB-divina / PDF reads are lazy: a page is only touched when `read_page` is
/// called, so a hit stops the container reads immediately. RAR cannot seek and its
/// priority candidates sit at the end of the archive, so they are collected in one
/// sequential pass at open with per-entry outcomes (a missing entry fails only its own
/// slot); decoding still stops at the first hit.
pub struct PagesReader {
    kind: PagesReaderKind,
    page_count: usize,
}

enum PagesReaderKind {
    Zip {
        entries: zip::ZipEntries,
        /// 1-based page number -> entry name (only the requested pages)
        names: HashMap<usize, String>,
    },
    Rar {
        /// per-requested-page outcome; a slot is consumed by its first `read_page`
        outcomes: HashMap<usize, Result<Vec<u8>>>,
    },
    Pdf {
        pages: pdf::PdfPages,
    },
}

impl PagesReader {
    /// Opens the container once for the given 1-based page numbers (same bounds/status
    /// checks as `get_page_content`). For RAR the candidates are collected in a single
    /// sequential pass here; ZIP/EPUB/PDF stay lazy.
    pub fn open(book_path: &Path, media: &Media, numbers: &[usize]) -> Result<Self> {
        if media.status != MediaStatus::Ready {
            return Err(MediaError::NotReady);
        }
        if let Some(&n) = numbers
            .iter()
            .find(|&&n| n == 0 || n > media.page_count as usize)
        {
            return Err(MediaError::PageOutOfBounds(n));
        }
        let page_count = media.page_count as usize;
        match media_profile(media.media_type.as_deref()) {
            Some(MediaProfile::Divina) => match media.media_type.as_deref() {
                Some(detect::APPLICATION_ZIP) | Some(detect::APPLICATION_EPUB) => {
                    Self::zip(book_path, media, numbers, page_count)
                }
                Some("application/x-rar-compressed")
                | Some(detect::APPLICATION_RAR_4)
                | Some(detect::APPLICATION_RAR_5) => {
                    let names: Vec<&str> = numbers
                        .iter()
                        .map(|&n| media.pages[n - 1].file_name.as_str())
                        .collect();
                    let outcomes = rar::get_entries_bytes_tolerant(book_path, &names)?;
                    Ok(Self {
                        kind: PagesReaderKind::Rar {
                            outcomes: numbers.iter().copied().zip(outcomes).collect(),
                        },
                        page_count,
                    })
                }
                Some(other) => Err(MediaError::unsupported(format!(
                    "no divina extractor for media type {other}"
                ))),
                None => Err(MediaError::NotReady),
            },
            Some(MediaProfile::Pdf) => Ok(Self {
                kind: PagesReaderKind::Pdf {
                    pages: pdf::PdfPages::open(book_path)?,
                },
                page_count,
            }),
            Some(MediaProfile::Epub) => {
                if media.epub_divina_compatible {
                    Self::zip(book_path, media, numbers, page_count)
                } else {
                    Err(MediaError::unsupported(
                        "Epub profile does not support getting page content",
                    ))
                }
            }
            None => Err(MediaError::NotReady),
        }
    }

    fn zip(book_path: &Path, media: &Media, numbers: &[usize], page_count: usize) -> Result<Self> {
        let names: HashMap<usize, String> = numbers
            .iter()
            .map(|&n| (n, media.pages[n - 1].file_name.clone()))
            .collect();
        Ok(Self {
            kind: PagesReaderKind::Zip {
                entries: zip::ZipEntries::open(book_path)?,
                names,
            },
            page_count,
        })
    }

    /// Reads one page on the held container; only this page is materialized (ZIP/EPUB/PDF).
    /// RAR slots are consumed by their first read, so each requested page must be read at
    /// most once — the barcode scan does.
    pub fn read_page(&mut self, number: usize) -> Result<Vec<u8>> {
        if number == 0 || number > self.page_count {
            return Err(MediaError::PageOutOfBounds(number));
        }
        match &mut self.kind {
            PagesReaderKind::Zip { entries, names } => {
                let name = names.get(&number).ok_or_else(|| {
                    MediaError::Other(anyhow::anyhow!("page {number} was not requested at open"))
                })?;
                entries.read(name)
            }
            PagesReaderKind::Rar { outcomes } => outcomes.remove(&number).ok_or_else(|| {
                MediaError::Other(anyhow::anyhow!(
                    "page {number} was not requested at open or was already read"
                ))
            })?,
            PagesReaderKind::Pdf { pages } => pages.render(number),
        }
    }
}

/// `BookAnalyzer.getPageContentRaw`: the raw page; only PDF supports it (single-page document).
pub fn get_page_content_raw(book_path: &Path, media: &Media, number: usize) -> Result<PageContent> {
    if media_profile(media.media_type.as_deref()) != Some(MediaProfile::Pdf) {
        return Err(MediaError::unsupported(
            "Extractor does not support raw extraction of pages",
        ));
    }
    if media.status != MediaStatus::Ready {
        return Err(MediaError::NotReady);
    }
    if number > media.page_count as usize || number == 0 {
        return Err(MediaError::PageOutOfBounds(number));
    }
    Ok(PageContent {
        bytes: pdf::get_page_content_as_pdf(book_path, number)?,
        media_type: detect::APPLICATION_PDF.to_string(),
    })
}

/// `BookAnalyzer.getFileContent`: an arbitrary file from the container (EPUB resources).
pub fn get_file_content(book_path: &Path, media: &Media, file_name: &str) -> Result<Vec<u8>> {
    if media.status != MediaStatus::Ready {
        return Err(MediaError::NotReady);
    }
    match media_profile(media.media_type.as_deref()) {
        Some(MediaProfile::Divina) => get_divina_entry(book_path, media, file_name),
        Some(MediaProfile::Epub) => zip::get_entry_bytes(book_path, file_name),
        _ => Err(MediaError::unsupported(
            "Extractor does not support extraction of files",
        )),
    }
}

fn get_divina_entry(book_path: &Path, media: &Media, file_name: &str) -> Result<Vec<u8>> {
    match media.media_type.as_deref() {
        Some(detect::APPLICATION_ZIP) | Some(detect::APPLICATION_EPUB) => {
            zip::get_entry_bytes(book_path, file_name)
        }
        Some("application/x-rar-compressed")
        | Some(detect::APPLICATION_RAR_4)
        | Some(detect::APPLICATION_RAR_5) => rar::get_entry_bytes(book_path, file_name),
        Some(other) => Err(MediaError::unsupported(format!(
            "no divina extractor for media type {other}"
        ))),
        None => Err(MediaError::NotReady),
    }
}

/// Image media types decodable by the `image` crate: komga's JXL/HEIF/JPEG2000 readers have no
/// equivalent yet (plan §7-4), so conversion from those formats fails like an unsupported reader.
const READABLE_IMAGE_TYPES: &[&str] = &[
    detect::IMAGE_JPEG,
    detect::IMAGE_PNG,
    detect::IMAGE_GIF,
    detect::IMAGE_WEBP,
    detect::IMAGE_TIFF,
    detect::IMAGE_BMP,
];

/// `BookLifecycle.getBookPage`: container extraction plus optional resize/convert.
pub fn get_book_page(
    book_path: &Path,
    book_name: &str,
    media: &Media,
    number: usize,
    convert_to: Option<ImageType>,
    resize_to: Option<u32>,
) -> Result<PageContent> {
    let page_content = get_page_content(book_path, media, number)?;
    let page_media_type = if media_profile(media.media_type.as_deref()) == Some(MediaProfile::Pdf) {
        detect::IMAGE_JPEG
    } else {
        media.pages[number - 1].media_type.as_str()
    };

    if let Some(resize_to) = resize_to {
        let bytes = image::resize(&page_content, ImageType::Jpeg, resize_to).map_err(|e| {
            MediaError::Conversion(format!(
                "Resize page #{number} of book {book_name} to {resize_to}: failed: {e}"
            ))
        })?;
        return Ok(PageContent {
            bytes,
            media_type: ImageType::Jpeg.media_type().to_string(),
        });
    }

    if let Some(convert_to) = convert_to {
        let msg = format!(
            "Convert page #{number} of book {book_name} from {page_media_type} to {}",
            convert_to.media_type()
        );
        if !READABLE_IMAGE_TYPES.contains(&page_media_type) {
            return Err(MediaError::Conversion(format!(
                "{msg}: unsupported read format {page_media_type}"
            )));
        }
        if page_media_type == convert_to.media_type() {
            return Ok(PageContent {
                bytes: page_content,
                media_type: page_media_type.to_string(),
            });
        }
        let bytes = image::convert(&page_content, convert_to)
            .map_err(|e| MediaError::Conversion(format!("{msg}: conversion failed: {e}")))?;
        return Ok(PageContent {
            bytes,
            media_type: convert_to.media_type().to_string(),
        });
    }

    Ok(PageContent {
        bytes: page_content,
        media_type: page_media_type.to_string(),
    })
}

/// `BookAnalyzer.getPdfPagesDynamic`: synthetic pages with render-scaled dimensions,
/// used by the pages endpoint for PDF books.
pub fn get_pdf_pages_dynamic(media: &Media) -> Result<Vec<komga_core::model::media::BookPage>> {
    if media_profile(media.media_type.as_deref()) != Some(MediaProfile::Pdf) {
        return Err(MediaError::unsupported(
            "Cannot get synthetic pages for non-PDF media",
        ));
    }
    Ok(media
        .pages
        .iter()
        .map(|page| komga_core::model::media::BookPage {
            media_type: detect::IMAGE_JPEG.to_string(),
            width: page
                .width
                .zip(page.height)
                .map(|(w, h)| pdf::scale_dimension(w, h).0),
            height: page
                .width
                .zip(page.height)
                .map(|(w, h)| pdf::scale_dimension(w, h).1),
            ..page.clone()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::media::BookPage;
    use komga_core::time_codec::now_utc;

    fn fixtures() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources")
    }

    fn media(media_type: &str, pages: Vec<BookPage>) -> Media {
        Media {
            book_id: "b1".into(),
            status: MediaStatus::Ready,
            media_type: Some(media_type.into()),
            comment: None,
            page_count: pages.len() as i32,
            pages,
            files: vec![],
            extension_class: None,
            extension_value: None,
            epub_divina_compatible: false,
            epub_is_kepub: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn page(file_name: &str, media_type: &str) -> BookPage {
        BookPage {
            file_name: file_name.into(),
            media_type: media_type.into(),
            width: None,
            height: None,
            file_hash: String::new(),
            file_size: None,
        }
    }

    #[test]
    fn profile_mapping() {
        assert_eq!(
            media_profile(Some(detect::APPLICATION_ZIP)),
            Some(MediaProfile::Divina)
        );
        assert_eq!(
            media_profile(Some(detect::APPLICATION_RAR_4)),
            Some(MediaProfile::Divina)
        );
        assert_eq!(
            media_profile(Some(detect::APPLICATION_RAR_5)),
            Some(MediaProfile::Divina)
        );
        assert_eq!(
            media_profile(Some("application/x-rar-compressed")),
            Some(MediaProfile::Divina)
        );
        assert_eq!(
            media_profile(Some(detect::APPLICATION_EPUB)),
            Some(MediaProfile::Epub)
        );
        assert_eq!(
            media_profile(Some(detect::APPLICATION_PDF)),
            Some(MediaProfile::Pdf)
        );
        assert_eq!(media_profile(Some("text/plain")), None);
        assert_eq!(media_profile(None), None);
    }

    #[test]
    fn zip_book_page_content() {
        let book = fixtures().join("archives/zip.zip");
        let media = media(
            detect::APPLICATION_ZIP,
            vec![page("komga.png", detect::IMAGE_PNG)],
        );
        let bytes = get_page_content(&book, &media, 1).unwrap();
        assert_eq!(&bytes[0..4], b"\x89PNG");

        let content = get_book_page(&book, "zip", &media, 1, None, None).unwrap();
        assert_eq!(content.media_type, detect::IMAGE_PNG);
    }

    #[test]
    fn rar_book_page_content() {
        let book = fixtures().join("archives/rar4.rar");
        let media = media(
            detect::APPLICATION_RAR_4,
            vec![page("komga-1.png", detect::IMAGE_PNG)],
        );
        let bytes = get_page_content(&book, &media, 1).unwrap();
        assert_eq!(&bytes[0..4], b"\x89PNG");
    }

    #[test]
    fn get_pages_content_batch_matches_individual() {
        let zip_book = fixtures().join("archives/zip.zip");
        let zip_media = media(
            detect::APPLICATION_ZIP,
            vec![page("komga.png", detect::IMAGE_PNG)],
        );
        assert_eq!(
            get_pages_content(&zip_book, &zip_media, &[1]).unwrap(),
            vec![get_page_content(&zip_book, &zip_media, 1).unwrap()]
        );

        // rar batch keeps the requested order, even when it differs from archive order
        let rar_book = fixtures().join("archives/rar4.rar");
        let rar_media = media(
            detect::APPLICATION_RAR_4,
            vec![
                page("komga-1.png", detect::IMAGE_PNG),
                page("komga-2.png", detect::IMAGE_PNG),
                page("komga-3.png", detect::IMAGE_PNG),
            ],
        );
        let batch = get_pages_content(&rar_book, &rar_media, &[1, 3, 2]).unwrap();
        assert_eq!(batch.len(), 3);
        for (n, bytes) in [1usize, 3, 2].iter().zip(batch.iter()) {
            assert_eq!(*bytes, get_page_content(&rar_book, &rar_media, *n).unwrap());
        }

        // bounds and status behave like the single-page function
        assert!(matches!(
            get_pages_content(&rar_book, &rar_media, &[0]),
            Err(MediaError::PageOutOfBounds(0))
        ));
        assert!(matches!(
            get_pages_content(&rar_book, &rar_media, &[4]),
            Err(MediaError::PageOutOfBounds(4))
        ));
        let mut not_ready = zip_media.clone();
        not_ready.status = MediaStatus::Unknown;
        assert!(matches!(
            get_pages_content(&zip_book, &not_ready, &[1]),
            Err(MediaError::NotReady)
        ));
        assert!(get_pages_content(&rar_book, &rar_media, &[])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn pages_reader_lazy_zip_fault_tolerant() {
        let book = fixtures().join("archives/zip.zip");
        // media lists one entry the archive does not have: only that page fails, the
        // other is still readable from the same open handle
        let media = media(
            detect::APPLICATION_ZIP,
            vec![
                page("komga.png", detect::IMAGE_PNG),
                page("nope.png", detect::IMAGE_PNG),
            ],
        );
        let mut reader = PagesReader::open(&book, &media, &[1, 2]).unwrap();
        assert_eq!(&reader.read_page(1).unwrap()[0..4], b"\x89PNG");
        assert!(matches!(
            reader.read_page(2),
            Err(MediaError::EntryNotFound(_))
        ));
        // bounds behave like the single-page functions
        assert!(matches!(
            PagesReader::open(&book, &media, &[3]),
            Err(MediaError::PageOutOfBounds(3))
        ));
        // a valid page that was not requested at open is a caller error, not PageOutOfBounds
        let mut partial = PagesReader::open(&book, &media, &[1]).unwrap();
        assert!(matches!(partial.read_page(2), Err(MediaError::Other(_))));
    }

    #[test]
    fn pages_reader_rar_order_and_fault_tolerance() {
        let book = fixtures().join("archives/rar4.rar");
        let media = media(
            detect::APPLICATION_RAR_4,
            vec![
                page("komga-1.png", detect::IMAGE_PNG),
                page("komga-2.png", detect::IMAGE_PNG),
                page("komga-3.png", detect::IMAGE_PNG),
                page("nope.png", detect::IMAGE_PNG),
            ],
        );
        let mut reader = PagesReader::open(&book, &media, &[1, 3, 2, 4]).unwrap();
        for n in [1usize, 3, 2] {
            assert_eq!(
                reader.read_page(n).unwrap(),
                get_page_content(&book, &media, n).unwrap(),
                "page {n}"
            );
        }
        // a missing entry fails only its own slot
        assert!(matches!(
            reader.read_page(4),
            Err(MediaError::EntryNotFound(_))
        ));
    }

    #[test]
    fn epub_not_divina_compatible_has_no_pages() {
        let book = fixtures().join("archives/epub3.epub");
        let mut media = media(
            detect::APPLICATION_EPUB,
            vec![page("page_1.xhtml", "application/xhtml+xml")],
        );
        media.epub_divina_compatible = false;
        assert!(matches!(
            get_page_content(&book, &media, 1),
            Err(MediaError::Unsupported { .. })
        ));
    }

    #[test]
    fn epub_file_content() {
        let book = fixtures().join("archives/epub3.epub");
        let media = media(detect::APPLICATION_EPUB, vec![]);
        let bytes = get_file_content(&book, &media, "content.opf").unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("<package"));
        assert!(matches!(
            get_file_content(&book, &media, "nope.xml"),
            Err(MediaError::EntryNotFound(_))
        ));
    }

    #[test]
    fn out_of_bounds_and_not_ready() {
        let book = fixtures().join("archives/zip.zip");
        let media = media(
            detect::APPLICATION_ZIP,
            vec![page("komga.png", detect::IMAGE_PNG)],
        );
        assert!(matches!(
            get_page_content(&book, &media, 2),
            Err(MediaError::PageOutOfBounds(2))
        ));
        assert!(matches!(
            get_page_content(&book, &media, 0),
            Err(MediaError::PageOutOfBounds(0))
        ));

        let mut not_ready = media.clone();
        not_ready.status = MediaStatus::Unknown;
        assert!(matches!(
            get_page_content(&book, &not_ready, 1),
            Err(MediaError::NotReady)
        ));
    }

    #[test]
    fn convert_to_jpeg() {
        let book = fixtures().join("archives/zip.zip");
        let media = media(
            detect::APPLICATION_ZIP,
            vec![page("komga.png", detect::IMAGE_PNG)],
        );
        let content = get_book_page(&book, "zip", &media, 1, Some(ImageType::Jpeg), None).unwrap();
        assert_eq!(content.media_type, detect::IMAGE_JPEG);
        assert_eq!(&content.bytes[0..3], b"\xFF\xD8\xFF");
    }

    #[test]
    fn resize_to_300() {
        let book = fixtures().join("archives/zip.zip");
        let media = media(
            detect::APPLICATION_ZIP,
            vec![page("komga.png", detect::IMAGE_PNG)],
        );
        let content = get_book_page(&book, "zip", &media, 1, None, Some(300)).unwrap();
        assert_eq!(content.media_type, detect::IMAGE_JPEG);
        // source is 48x48: no upscale
        assert_eq!(image::get_dimension(&content.bytes), Some((48, 48)));
    }

    #[test]
    fn raw_extraction_only_for_pdf() {
        let book = fixtures().join("archives/zip.zip");
        let media = media(
            detect::APPLICATION_ZIP,
            vec![page("komga.png", detect::IMAGE_PNG)],
        );
        assert!(matches!(
            get_page_content_raw(&book, &media, 1),
            Err(MediaError::Unsupported { .. })
        ));
    }
}
