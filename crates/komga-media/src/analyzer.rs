//! Book analysis state machine, ported from `BookAnalyzer.kt` and the mediacontainer
//! extractors (`ZipExtractor` / `RarExtractor` / `PdfExtractor` / `EpubExtractor`).
//!
//! The `analyze` state machine never fails outwardly: every outcome becomes a `Media` with
//! status READY / ERROR (with an ERR_ comment) / UNSUPPORTED, exactly like the Java side.

use crate::error::{MediaError, Result};
use crate::zip as zip_utils;
use crate::{container, detect, hash, image, pdf};
use container::PageContent;
use komga_core::dto::progression::{R2Location, R2Locator};
use komga_core::model::media::{BookPage, Media, MediaFile, MediaFileSubType, MediaStatus};
use komga_core::natural_sort;
use komga_core::search::MediaProfile;
use komga_core::time_codec;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;

/// `org.gotson.komga.domain.model.MediaExtensionEpub` (EXTENSION_CLASS for EPUB media)
pub const EPUB_EXTENSION_CLASS: &str = "org.gotson.komga.domain.model.MediaExtensionEpub";

pub struct Analyzer {
    page_hashing: u32,
    thumbnail_max_edge: u32,
    letter_count_threshold: usize,
    /// Probed kepubify executable; plain EPUBs are converted on the fly to extract real kobo
    /// span positions (`EpubExtractor.computePositions`).
    kepubify_path: Option<std::path::PathBuf>,
}

pub struct Analysis {
    pub media: Media,
    pub epub_extension: Option<MediaExtensionEpub>,
}

/// `MediaExtensionEpub.kt`. All fields are always serialized (Jackson default inclusion),
/// including null `href`s inside `EpubTocEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaExtensionEpub {
    #[serde(default)]
    pub toc: Vec<EpubTocEntry>,
    #[serde(default)]
    pub landmarks: Vec<EpubTocEntry>,
    #[serde(default)]
    pub page_list: Vec<EpubTocEntry>,
    #[serde(default)]
    pub is_fixed_layout: bool,
    #[serde(default)]
    pub positions: Vec<R2Locator>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpubTocEntry {
    pub title: String,
    // Jackson's default inclusion keeps the null in the stored extension JSON
    pub href: Option<String>,
    #[serde(default)]
    pub children: Vec<EpubTocEntry>,
}

/// gzip+JSON encoding of the extension, for `MEDIA.EXTENSION_VALUE_BLOB`
/// (`ObjectMapper.serializeJsonGz`).
pub fn encode_epub_extension_gz(extension: &MediaExtensionEpub) -> Result<Vec<u8>> {
    use std::io::Write;
    let json = serde_json::to_vec(extension).expect("extension serialization cannot fail");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&json)
        .map_err(|e| MediaError::Other(e.into()))?;
    encoder.finish().map_err(|e| MediaError::Other(e.into()))
}

pub struct GeneratedThumbnail {
    pub bytes: Vec<u8>,
    pub media_type: String,
    pub width: i32,
    pub height: i32,
    pub file_size: i64,
}

impl Analyzer {
    pub fn new(
        page_hashing: u32,
        thumbnail_max_edge: u32,
        letter_count_threshold: usize,
        kepubify_path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            page_hashing,
            thumbnail_max_edge,
            letter_count_threshold,
            kepubify_path,
        }
    }

    /// `BookAnalyzer.analyze`: errors are reported as Media, never as Err
    pub fn analyze(&self, book_path: &Path, analyze_dimensions: bool) -> Analysis {
        match self.analyze_internal(book_path, analyze_dimensions) {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("Error while analyzing book {}: {e}", book_path.display());
                Analysis {
                    media: error_media(&e),
                    epub_extension: None,
                }
            }
        }
    }

    fn analyze_internal(&self, book_path: &Path, analyze_dimensions: bool) -> Result<Analysis> {
        let detected = detect_book_media_type(book_path)?;
        let mut media_type = match container::media_profile(Some(&detected)) {
            Some(_) => detected,
            None => {
                return Ok(Analysis {
                    media: media(
                        MediaStatus::Unsupported,
                        Some(detected),
                        Some("ERR_1001".into()),
                    ),
                    epub_extension: None,
                })
            }
        };

        if is_epub_extension(book_path)
            && container::media_profile(Some(&media_type)) != Some(MediaProfile::Epub)
        {
            if is_epub_file(book_path) {
                media_type = detect::APPLICATION_EPUB.to_string();
            } else {
                tracing::warn!(
                    "Epub file is malformed, file is probably broken: {}",
                    book_path.display()
                );
                return Ok(Analysis {
                    media: media(
                        MediaStatus::Error,
                        Some(media_type),
                        Some("ERR_1032".into()),
                    ),
                    epub_extension: None,
                });
            }
        }

        match container::media_profile(Some(&media_type)) {
            Some(MediaProfile::Divina) => {
                let media = self.analyze_divina(book_path, &media_type, analyze_dimensions);
                Ok(Analysis {
                    media: Media {
                        media_type: Some(media_type),
                        ..media
                    },
                    epub_extension: None,
                })
            }
            Some(MediaProfile::Pdf) => {
                let media = self.analyze_pdf(book_path, analyze_dimensions)?;
                Ok(Analysis {
                    media: Media {
                        media_type: Some(media_type),
                        ..media
                    },
                    epub_extension: None,
                })
            }
            Some(MediaProfile::Epub) => {
                let (media, extension) = self.analyze_epub(book_path, analyze_dimensions)?;
                Ok(Analysis {
                    media: Media {
                        media_type: Some(media_type),
                        ..media
                    },
                    epub_extension: Some(extension),
                })
            }
            // media_profile returned Some above, so one of the profiles must match
            None => unreachable!(),
        }
    }

    fn analyze_divina(
        &self,
        book_path: &Path,
        media_type: &str,
        analyze_dimensions: bool,
    ) -> Media {
        let entries = match get_divina_entries(book_path, media_type, analyze_dimensions) {
            Ok(e) => e,
            Err(MediaError::Unsupported { code, .. }) => {
                return media(MediaStatus::Unsupported, None, code)
            }
            Err(e) => {
                tracing::error!("Error while analyzing book {}: {e}", book_path.display());
                return media(MediaStatus::Error, None, Some("ERR_1008".into()));
            }
        };

        let (pages, others): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| {
            e.media_type
                .as_deref()
                .map(detect::is_image)
                .unwrap_or(false)
        });

        let error_summary = {
            let names: Vec<&str> = others
                .iter()
                .filter(|e| {
                    e.media_type
                        .as_deref()
                        .map(|s| s.trim().is_empty())
                        .unwrap_or(true)
                })
                .map(|e| e.name.as_str())
                .collect();
            if names.is_empty() {
                None
            } else {
                Some(format!("ERR_1007 [{}]", names.join(", ")))
            }
        };

        if pages.is_empty() {
            tracing::warn!("Book {} does not contain any pages", book_path.display());
            return media(MediaStatus::Error, None, Some("ERR_1006".into()));
        }

        let files = others
            .iter()
            .map(|e| MediaFile {
                file_name: e.name.clone(),
                media_type: e.media_type.clone(),
                sub_type: None,
                file_size: e.file_size,
            })
            .collect();

        Media {
            status: MediaStatus::Ready,
            page_count: pages.len() as i32,
            pages: pages
                .into_iter()
                .map(|e| BookPage {
                    file_name: e.name,
                    media_type: e.media_type.expect("pages are images"),
                    width: e.dimension.map(|d| d.0),
                    height: e.dimension.map(|d| d.1),
                    file_hash: String::new(),
                    file_size: e.file_size,
                })
                .collect(),
            files,
            comment: error_summary,
            ..media(MediaStatus::Ready, None, None)
        }
    }

    fn analyze_pdf(&self, book_path: &Path, analyze_dimensions: bool) -> Result<Media> {
        let pages = get_pdf_pages(book_path, analyze_dimensions)?;
        Ok(Media {
            status: MediaStatus::Ready,
            page_count: pages.len() as i32,
            pages,
            ..media(MediaStatus::Ready, None, None)
        })
    }

    fn analyze_epub(
        &self,
        book_path: &Path,
        analyze_dimensions: bool,
    ) -> Result<(Media, MediaExtensionEpub)> {
        let mut pkg = open_epub(book_path)?;

        let all_resources = get_resources(&mut pkg);
        let (resources, missing): (Vec<_>, Vec<_>) = all_resources
            .into_iter()
            .partition(|r| r.file_size.is_some());
        let is_kepub = is_kepub(&mut pkg, &resources);

        let mut errors: Vec<String> = vec![];
        let toc = match get_toc(&mut pkg) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Error while getting EPUB TOC: {e}");
                errors.push("ERR_1035".into());
                vec![]
            }
        };
        let landmarks = match get_landmarks(&mut pkg) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Error while getting EPUB Landmarks: {e}");
                errors.push("ERR_1036".into());
                vec![]
            }
        };
        let page_list = match get_page_list(&mut pkg) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Error while getting EPUB page list: {e}");
                errors.push("ERR_1037".into());
                vec![]
            }
        };
        let divina_pages = match get_divina_pages(self, &mut pkg, analyze_dimensions) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Error while getting EPUB Divina pages: {e}");
                errors.push("ERR_1038".into());
                vec![]
            }
        };

        let is_fixed_layout = !divina_pages.is_empty() || is_fixed_layout(&pkg);

        let positions = match compute_positions(
            &mut pkg,
            &resources,
            is_fixed_layout,
            is_kepub,
            book_path,
            self.kepubify_path.as_deref(),
        ) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Error while getting EPUB positions: {e}");
                errors.push("ERR_1039".into());
                vec![]
            }
        };

        let missing_summary = if missing.is_empty() {
            None
        } else {
            Some(format!(
                "ERR_1033 [{}]",
                missing
                    .iter()
                    .map(|m| m.file_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        };
        let comment = {
            let mut parts = errors;
            if let Some(s) = missing_summary {
                parts.push(s);
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        };

        let divina_compatible = !divina_pages.is_empty();
        let page_count = if divina_compatible {
            divina_pages.len() as i32
        } else {
            compute_page_count(&mut pkg)
        };

        let extension = MediaExtensionEpub {
            toc,
            landmarks,
            page_list,
            is_fixed_layout,
            positions,
        };
        let media = Media {
            status: MediaStatus::Ready,
            page_count,
            pages: divina_pages,
            files: resources,
            epub_divina_compatible: divina_compatible,
            epub_is_kepub: is_kepub,
            comment,
            ..media(MediaStatus::Ready, None, None)
        };
        Ok((media, extension))
    }

    /// `BookAnalyzer.generateThumbnail`
    pub fn generate_thumbnail(
        &self,
        book_path: &Path,
        media: &Media,
    ) -> Result<GeneratedThumbnail> {
        if media.status != MediaStatus::Ready {
            tracing::warn!(
                "Book media is not ready, cannot generate thumbnail. Book: {}",
                book_path.display()
            );
            return Err(MediaError::NotReady);
        }
        let poster = self
            .get_poster(book_path, media)
            .ok_or_else(|| MediaError::Conversion("no thumbnail could be found".into()))?;
        let bytes = image::resize(
            &poster.bytes,
            image::ImageType::Jpeg,
            self.thumbnail_max_edge,
        )?;
        let (w, h) = image::get_dimension(&bytes).unwrap_or((0, 0));
        Ok(GeneratedThumbnail {
            media_type: detect::IMAGE_JPEG.to_string(),
            file_size: bytes.len() as i64,
            width: w as i32,
            height: h as i32,
            bytes,
        })
    }

    /// `BookAnalyzer.getPoster`
    pub fn get_poster(&self, book_path: &Path, media: &Media) -> Option<PageContent> {
        match container::media_profile(media.media_type.as_deref()) {
            Some(MediaProfile::Divina) => {
                let page = media.pages.first()?;
                let bytes = container::get_page_content(book_path, media, 1).ok()?;
                Some(PageContent {
                    bytes,
                    media_type: page.media_type.clone(),
                })
            }
            Some(MediaProfile::Pdf) => {
                let bytes = pdf::get_page_content_as_image(book_path, 1).ok()?;
                Some(PageContent {
                    bytes,
                    media_type: detect::IMAGE_JPEG.to_string(),
                })
            }
            Some(MediaProfile::Epub) => get_cover(book_path).or_else(|| {
                if media.epub_divina_compatible {
                    let page = media.pages.first()?;
                    let bytes = container::get_page_content(book_path, media, 1).ok()?;
                    Some(PageContent {
                        bytes,
                        media_type: page.media_type.clone(),
                    })
                } else {
                    None
                }
            }),
            None => None,
        }
    }

    /// `BookAnalyzer.hashPages`: hashes the first and last `page_hashing` pages whose hash is blank
    pub fn hash_pages(&self, book_path: &Path, media: &Media) -> Result<Media> {
        let page_count = media.page_count as usize;
        let mut hashed = media.clone();
        for index in 0..media.pages.len() {
            let page = &media.pages[index];
            if page.file_hash.trim().is_empty()
                && (index < self.page_hashing as usize
                    || index >= page_count.saturating_sub(self.page_hashing as usize))
            {
                let content = container::get_page_content(book_path, media, index + 1)?;
                hashed.pages[index].file_hash = self.hash_page(page, &content)?;
            }
        }
        Ok(hashed)
    }

    /// `BookAnalyzer.hashPage`: JPEG pages are decoded and re-encoded first (EXIF removal),
    /// everything else is hashed as-is
    /// `BookAnalyzer.hashPage`: JPEG pages are decoded and re-encoded first (EXIF removal),
    /// everything else is hashed as-is
    pub fn hash_page(&self, page: &BookPage, content: &[u8]) -> Result<String> {
        if page.media_type == detect::IMAGE_JPEG {
            let img_reader = ::image::ImageReader::new(std::io::Cursor::new(content))
                .with_guessed_format()
                .map_err(|e| {
                    MediaError::Conversion(format!("could not read jpeg page for hashing: {e}"))
                })?;
            let img = img_reader.decode().map_err(|e| {
                MediaError::Conversion(format!("could not decode jpeg page for hashing: {e}"))
            })?;
            let bytes = image::encode_jpeg(&img)?;
            return Ok(hash::compute_hash_bytes(&bytes));
        }
        Ok(hash::compute_hash_bytes(content))
    }
}

fn media(status: MediaStatus, media_type: Option<String>, comment: Option<String>) -> Media {
    Media {
        book_id: String::new(),
        status,
        media_type,
        comment,
        page_count: 0,
        pages: vec![],
        files: vec![],
        extension_class: None,
        extension_value: None,
        epub_divina_compatible: false,
        epub_is_kepub: false,
        created_date: time_codec::now_utc(),
        last_modified_date: time_codec::now_utc(),
    }
}

fn error_media(e: &MediaError) -> Media {
    let code = match e {
        MediaError::NoSuchFile(_) => "ERR_1018",
        MediaError::Other(err)
            if err
                .downcast_ref::<std::io::Error>()
                .map(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
                .unwrap_or(false) =>
        {
            "ERR_1000"
        }
        _ => "ERR_1005",
    };
    media(MediaStatus::Error, None, Some(code.to_string()))
}

fn is_epub_extension(book_path: &Path) -> bool {
    book_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("epub"))
        .unwrap_or(false)
}

/// `EpubExtractor.isEpub`
fn is_epub_file(book_path: &Path) -> bool {
    zip_utils::get_entry_bytes(book_path, "mimetype")
        .map(|b| b.trim_ascii() == b"application/epub+zip")
        .unwrap_or(false)
}

/// `ContentDetector.detectMediaType(Path)`: sniffs the head; for zip containers the `mimetype`
/// entry is inspected with random access (Tika's ZipContainerDetector behavior) without
/// buffering the whole book.
fn detect_book_media_type(book_path: &Path) -> Result<String> {
    let mut file = open_book_file(book_path)?;
    let mut head = vec![0u8; 65536];
    let n = read_full(&mut file, &mut head)?;
    head.truncate(n);
    if head.starts_with(b"PK\x03\x04") {
        if let Ok(mut archive) = zip::ZipArchive::new(open_book_file(book_path)?) {
            if let Ok(mut entry) = archive.by_name("mimetype") {
                let mut content = String::new();
                if entry.read_to_string(&mut content).is_ok() {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        return Ok(trimmed.to_string());
                    }
                }
            }
        }
    }
    Ok(detect::detect_media_type(&head))
}

fn open_book_file(book_path: &Path) -> Result<std::fs::File> {
    std::fs::File::open(book_path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => MediaError::NoSuchFile(book_path.display().to_string()),
        _ => MediaError::Other(e.into()),
    })
}

/// roxmltree rejects DTDs by default; jsoup (the Kotlin parser) tolerates them (NCX has one)
fn parse_xml(content: &str) -> std::result::Result<roxmltree::Document<'_>, roxmltree::Error> {
    roxmltree::Document::parse_with_options(
        content,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
}

fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader
            .read(&mut buf[filled..])
            .map_err(|e| MediaError::Other(e.into()))?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

// region divina entries

/// `MediaContainerEntry.kt`
pub(crate) struct ContainerEntry {
    pub(crate) name: String,
    media_type: Option<String>,
    pub(crate) dimension: Option<(i32, i32)>,
    file_size: Option<i64>,
}

fn get_divina_entries(
    book_path: &Path,
    media_type: &str,
    analyze_dimensions: bool,
) -> Result<Vec<ContainerEntry>> {
    match media_type {
        detect::APPLICATION_ZIP => get_zip_entries(book_path, analyze_dimensions),
        "application/x-rar-compressed" | detect::APPLICATION_RAR_4 | detect::APPLICATION_RAR_5 => {
            get_rar_entries(book_path, analyze_dimensions)
        }
        // Kotlin returns UNSUPPORTED with no comment when no extractor matches
        other => Err(MediaError::unsupported(format!(
            "no divina extractor for media type {other}"
        ))),
    }
}

/// `ZipExtractor.getEntries`
fn get_zip_entries(book_path: &Path, analyze_dimensions: bool) -> Result<Vec<ContainerEntry>> {
    let file = open_book_file(book_path)?;
    // an unopenable archive is a generic getEntries failure (ERR_1008), not a coded UNSUPPORTED
    let archive = zip::ZipArchive::new(file)
        .map_err(|e| MediaError::Other(anyhow::anyhow!("could not open zip archive: {e}")))?;
    zip_entries_from(archive, analyze_dimensions)
}

/// Entry loop of `get_zip_entries`, split from file opening so tests can drive it with
/// instrumented readers.
pub(crate) fn zip_entries_from<R: std::io::Read + std::io::Seek>(
    mut archive: zip::ZipArchive<R>,
    analyze_dimensions: bool,
) -> Result<Vec<ContainerEntry>> {
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| MediaError::Other(anyhow::anyhow!("could not read zip entry: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let file_size = entry.size() as i64;

        // sniff the first 64 KiB; a read error leaves the entry without a media type
        // (Kotlin logs and keeps it with a null mediaType, which feeds ERR_1007)
        let mut head = vec![0u8; 65536.min(file_size.max(0) as usize)];
        let head = match read_full(&mut entry, &mut head) {
            Ok(n) => {
                head.truncate(n);
                head
            }
            Err(e) => {
                tracing::warn!("Could not analyze entry: {name}: {e}");
                entries.push(ContainerEntry {
                    name,
                    media_type: None,
                    dimension: None,
                    file_size: Some(file_size),
                });
                continue;
            }
        };
        let media_type = detect::detect_media_type(&head);
        let dimension = if analyze_dimensions && detect::is_image(&media_type) {
            // dimensions live in the image header for the common formats, which the sniffed
            // head already covers; only headers spanning the window (e.g. JPEG with a huge
            // EXIF) justify reading the whole entry — on network mounts every byte costs
            let dimension = image::get_dimension(&head).or_else(|| {
                let mut bytes = head.clone();
                if (file_size as usize) > head.len() && entry.read_to_end(&mut bytes).is_err() {
                    bytes = head.clone();
                }
                image::get_dimension(&bytes)
            });
            dimension.map(|(w, h)| (w as i32, h as i32))
        } else {
            None
        };
        entries.push(ContainerEntry {
            name,
            media_type: Some(media_type),
            dimension,
            file_size: Some(file_size),
        });
    }
    entries.sort_by(|a, b| natural_sort::compare(&a.name, &b.name));
    Ok(entries)
}

/// `RarExtractor.getEntries`. junrar extracts solid archives sequentially just fine;
/// the `unrar` crate is used for the same reason (libarchive refuses solid archives).
fn get_rar_entries(book_path: &Path, analyze_dimensions: bool) -> Result<Vec<ContainerEntry>> {
    if unrar::Archive::new(book_path).is_multipart() {
        return Err(MediaError::unsupported_coded(
            "Multi-Volume RAR archives are not supported",
            "ERR_1004",
        ));
    }
    let mut archive = unrar::Archive::new(book_path)
        .open_for_processing()
        .map_err(map_unrar_error)?;
    let mut entries = Vec::new();
    loop {
        let header = match archive.read_header() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(e) => return Err(map_unrar_error(e)),
        };
        let e = header.entry();
        let name = e.filename.to_string_lossy().to_string();
        if e.is_directory() {
            archive = header
                .skip()
                .map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
            continue;
        }
        let unpacked_size = e.unpacked_size as i64;
        let (bytes, next) = match header.read() {
            Ok(ok) => ok,
            Err(e) => {
                // junrar lists the remaining entries with a null mediaType; unrar cannot
                // recover mid-archive, so they are lost here (broken archives only)
                tracing::warn!("Could not analyze entry: {name}: {e}");
                entries.push(ContainerEntry {
                    name,
                    media_type: None,
                    dimension: None,
                    file_size: Some(unpacked_size),
                });
                break;
            }
        };
        archive = next;
        let media_type = detect::detect_media_type(&bytes[..bytes.len().min(65536)]);
        let dimension = if analyze_dimensions && detect::is_image(&media_type) {
            image::get_dimension(&bytes).map(|(w, h)| (w as i32, h as i32))
        } else {
            None
        };
        entries.push(ContainerEntry {
            name,
            media_type: Some(media_type),
            dimension,
            file_size: Some(unpacked_size),
        });
    }
    entries.sort_by(|a, b| natural_sort::compare(&a.name, &b.name));
    Ok(entries)
}

fn map_unrar_error(e: unrar::error::UnrarError) -> MediaError {
    let s = e.to_string();
    if s.to_lowercase().contains("password") {
        MediaError::unsupported_coded("Encrypted RAR archives are not supported", "ERR_1002")
    } else {
        // broken archive: a generic getEntries failure (ERR_1008)
        MediaError::Other(anyhow::anyhow!("could not read rar archive: {s}"))
    }
}

// endregion

// region pdf pages

/// `PdfExtractor.getPages`: page name is the 1-based index; dimensions come from the crop box
fn get_pdf_pages(book_path: &Path, analyze_dimensions: bool) -> Result<Vec<BookPage>> {
    let pdfium = bind_pdfium()?;
    let document = pdfium.load_pdf_from_file(book_path, None).map_err(|e| {
        if !book_path.exists() {
            MediaError::NoSuchFile(book_path.display().to_string())
        } else {
            MediaError::unsupported(format!("could not open pdf document: {e}"))
        }
    })?;
    let mut pages = vec![];
    for index in 0..document.pages().len() {
        let page = document
            .pages()
            .get(index)
            .map_err(|e| MediaError::unsupported(format!("could not get pdf page {index}: {e}")))?;
        let dimension = if analyze_dimensions {
            let (w, h) = crop_box_size(&page);
            Some((w.round() as i32, h.round() as i32))
        } else {
            None
        };
        pages.push(BookPage {
            file_name: (index + 1).to_string(),
            media_type: String::new(),
            width: dimension.map(|d| d.0),
            height: dimension.map(|d| d.1),
            file_hash: String::new(),
            file_size: None,
        });
    }
    Ok(pages)
}

/// Mirrors `pdf.rs`'s library lookup (kept private there): env override, executable dir, system
fn bind_pdfium() -> Result<pdfium_render::prelude::Pdfium> {
    use pdfium_render::prelude::Pdfium;
    let mut last_err = None;
    let mut candidates = vec![];
    if let Ok(custom) = std::env::var("KOMGA_PDFIUM_PATH") {
        candidates.push(std::path::PathBuf::from(custom));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(Pdfium::pdfium_platform_library_name()));
        }
    }
    for path in candidates {
        if path.exists() {
            match Pdfium::bind_to_library(&path) {
                Ok(bindings) => return Ok(Pdfium::new(bindings)),
                Err(e) => last_err = Some(e.to_string()),
            }
        }
    }
    Pdfium::bind_to_system_library()
        .map(Pdfium::new)
        .map_err(|e| {
            MediaError::unsupported(format!(
                "libpdfium is not available: {}",
                last_err.unwrap_or_else(|| e.to_string())
            ))
        })
}

/// PDFBox `page.cropBox` semantics: falls back to the media box
fn crop_box_size(page: &pdfium_render::prelude::PdfPage<'_>) -> (f32, f32) {
    let boundaries = page.boundaries();
    match boundaries.crop().or_else(|_| boundaries.media()) {
        Ok(b) => (b.bounds.width().value, b.bounds.height().value),
        Err(_) => (page.width().value, page.height().value),
    }
}

// endregion

// region epub package

struct EpubPackage {
    archive: zip::ZipArchive<std::fs::File>,
    opf_content: String,
    opf_dir: Option<String>,
    /// manifest items in document order (Kotlin's LinkedHashMap)
    manifest: Vec<ManifestItem>,
    manifest_by_id: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq)]
struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    properties: BTreeSet<String>,
}

struct ZipEntryMeta {
    name: String,
    size: i64,
    compressed_size: i64,
}

impl EpubPackage {
    fn manifest_item(&self, id: &str) -> Option<&ManifestItem> {
        self.manifest_by_id.get(id).map(|&i| &self.manifest[i])
    }

    fn entry_metas(&mut self) -> Vec<ZipEntryMeta> {
        let mut out = vec![];
        for i in 0..self.archive.len() {
            if let Ok(e) = self.archive.by_index(i) {
                out.push(ZipEntryMeta {
                    name: e.name().to_string(),
                    size: e.size() as i64,
                    compressed_size: e.compressed_size() as i64,
                });
            }
        }
        out
    }

    fn read_entry_string(&mut self, name: &str) -> Option<String> {
        let mut entry = self.archive.by_name(name).ok()?;
        let mut content = String::new();
        entry.read_to_string(&mut content).ok()?;
        Some(content)
    }

    fn read_entry_bytes(&mut self, name: &str) -> Option<Vec<u8>> {
        let mut entry = self.archive.by_name(name).ok()?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).ok()?;
        Some(buf)
    }

    fn read_entry_head(&mut self, name: &str, max: usize) -> Option<Vec<u8>> {
        let mut entry = self.archive.by_name(name).ok()?;
        let mut buf = vec![0u8; max.min(entry.size() as usize)];
        let n = read_full(&mut entry, &mut buf).ok()?;
        buf.truncate(n);
        Some(buf)
    }
}

/// `Path.epub {}`: opens the zip, locates the OPF via META-INF/container.xml, parses the manifest
fn open_epub(book_path: &Path) -> Result<EpubPackage> {
    let file = open_book_file(book_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| MediaError::unsupported(format!("could not open epub archive: {e}")))?;

    let opf_path = {
        let container = archive
            .by_name("META-INF/container.xml")
            .ok()
            .and_then(|mut e| {
                let mut s = String::new();
                e.read_to_string(&mut s).ok().map(|_| s)
            })
            .ok_or_else(|| {
                MediaError::unsupported("META-INF/container.xml does not contain rootfile tag")
            })?;
        let doc = parse_xml(&container)
            .map_err(|e| MediaError::unsupported(format!("could not parse container.xml: {e}")))?;
        doc.descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "rootfile")
            .and_then(|n| n.attribute("full-path"))
            .map(|s| s.to_string())
            .ok_or_else(|| {
                MediaError::unsupported("META-INF/container.xml does not contain rootfile tag")
            })?
    };

    let opf_content = {
        let mut entry = archive
            .by_name(&opf_path)
            .map_err(|_| MediaError::unsupported("Could not open OPF resource"))?;
        let mut content = String::new();
        entry
            .read_to_string(&mut content)
            .map_err(|_| MediaError::unsupported("Could not open OPF resource"))?;
        content
    };

    let (manifest, manifest_by_id) = parse_manifest(&opf_content)?;
    let opf_dir = parent_dir(&opf_path);
    Ok(EpubPackage {
        archive,
        opf_content,
        opf_dir,
        manifest,
        manifest_by_id,
    })
}

/// `Document.getManifest()`: id → ManifestItem, in document order
fn parse_manifest(opf_content: &str) -> Result<(Vec<ManifestItem>, HashMap<String, usize>)> {
    let doc = parse_xml(opf_content)
        .map_err(|e| MediaError::unsupported(format!("Could not open OPF resource: {e}")))?;
    let mut manifest = vec![];
    let mut by_id = HashMap::new();
    for item in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "manifest")
        .flat_map(|m| {
            m.children()
                .filter(|c| c.is_element() && c.tag_name().name() == "item")
        })
    {
        let id = item.attribute("id").unwrap_or_default().to_string();
        let properties = item
            .attribute("properties")
            .map(|p| {
                p.split(' ')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        by_id.insert(id.clone(), manifest.len());
        manifest.push(ManifestItem {
            id,
            href: item.attribute("href").unwrap_or_default().to_string(),
            media_type: item.attribute("media-type").unwrap_or_default().to_string(),
            properties,
        });
    }
    Ok((manifest, by_id))
}

fn spine_idrefs(opf_content: &str) -> Vec<String> {
    parse_xml(opf_content)
        .ok()
        .and_then(|doc| {
            doc.descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "spine")
                .map(|s| {
                    s.children()
                        .filter(|c| c.is_element() && c.tag_name().name() == "itemref")
                        .map(|c| c.attribute("idref").unwrap_or_default().to_string())
                        .collect()
                })
        })
        .unwrap_or_default()
}

/// `EpubExtractor.getResources`
fn get_resources(pkg: &mut EpubPackage) -> Vec<MediaFile> {
    let idrefs = spine_idrefs(&pkg.opf_content);
    let spine_items: Vec<&ManifestItem> = idrefs
        .iter()
        .filter_map(|idref| pkg.manifest_item(idref))
        .collect();
    let pages: Vec<MediaFile> = spine_items
        .iter()
        .map(|item| MediaFile {
            file_name: normalize_href(pkg.opf_dir.as_deref(), &percent_decode(&item.href)),
            media_type: Some(item.media_type.clone()),
            sub_type: Some(MediaFileSubType::EpubPage),
            file_size: None,
        })
        .collect();
    let assets: Vec<MediaFile> = pkg
        .manifest
        .iter()
        .filter(|item| !spine_items.contains(item))
        .map(|item| MediaFile {
            file_name: normalize_href(pkg.opf_dir.as_deref(), &percent_decode(&item.href)),
            media_type: Some(item.media_type.clone()),
            sub_type: Some(MediaFileSubType::EpubAsset),
            file_size: None,
        })
        .collect();
    let sizes: BTreeMap<String, i64> = pkg
        .entry_metas()
        .into_iter()
        .map(|m| (m.name, m.size))
        .collect();
    pages
        .into_iter()
        .chain(assets)
        .map(|mut r| {
            r.file_size = sizes.get(&r.file_name).copied();
            r
        })
        .collect()
}

/// `EpubExtractor.getDivinaPages`
fn get_divina_pages(
    analyzer: &Analyzer,
    pkg: &mut EpubPackage,
    analyze_dimensions: bool,
) -> Result<Vec<BookPage>> {
    let idrefs = spine_idrefs(&pkg.opf_content);
    let spine_paths: Vec<String> = idrefs
        .iter()
        .filter_map(|idref| {
            pkg.manifest_item(idref)
                .map(|item| normalize_href(pkg.opf_dir.as_deref(), &item.href))
        })
        .collect();
    let entry_names: BTreeSet<String> = pkg.entry_metas().into_iter().map(|m| m.name).collect();
    let page_count = entry_names
        .iter()
        .filter(|n| spine_paths.contains(n))
        .count();

    let mut pages_with_images: Vec<Vec<String>> = vec![];
    for idref in &idrefs {
        let Some(item) = pkg.manifest_item(idref) else {
            continue;
        };
        let page_path = normalize_href(pkg.opf_dir.as_deref(), &item.href);
        if item.media_type.to_lowercase().starts_with("image") {
            pages_with_images.push(vec![normalize_zip_path(&page_path)]);
            continue;
        }
        let Some(content) = pkg.read_entry_string(&page_path) else {
            pages_with_images.push(vec![]);
            continue;
        };
        let doc = parse_xml(&content).map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
        // a page with text over the threshold makes the whole book not divina compatible
        if body_text_len(&doc) > analyzer.letter_count_threshold {
            return Ok(vec![]);
        }
        let mut images: Vec<String> = vec![];
        for img in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "img")
        {
            if let Some(src) = img.attribute("src") {
                images.push(resolve_relative(&page_path, src));
            }
        }
        for svg in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "svg")
        {
            for img in svg
                .children()
                .filter(|c| c.is_element() && c.tag_name().name() == "image")
            {
                if let Some(href) = img
                    .attributes()
                    .find(|a| a.name() == "href")
                    .map(|a| a.value())
                {
                    images.push(resolve_relative(&page_path, href));
                }
            }
        }
        pages_with_images.push(images);
    }

    if pages_with_images.len() != page_count {
        tracing::info!(
            "Epub Divina detection failed: book has {} pages with images, but {page_count} total pages",
            pages_with_images.len()
        );
        return Ok(vec![]);
    }
    // unique image path per page only (KCC repeats the same image within a page)
    let mut images_path: Vec<String> = vec![];
    for images in &pages_with_images {
        let mut seen: Vec<String> = vec![];
        for img in images {
            if !seen.contains(img) {
                seen.push(img.clone());
            }
        }
        images_path.extend(seen);
    }
    if images_path.len() != page_count {
        tracing::info!(
            "Epub Divina detection failed: book has {} detected images, but {page_count} total pages",
            images_path.len()
        );
        return Ok(vec![]);
    }

    let mut divina_pages: Vec<BookPage> = vec![];
    for image_path in &images_path {
        let Some(media_type) = pkg
            .manifest
            .iter()
            .find(|item| normalize_href(pkg.opf_dir.as_deref(), &item.href) == *image_path)
            .map(|item| item.media_type.clone())
        else {
            return Ok(vec![]);
        };
        if !detect::is_image(&media_type) {
            return Ok(vec![]);
        }
        let metas = pkg.entry_metas();
        let Some(meta) = metas.iter().find(|m| m.name == *image_path) else {
            // Kotlin NPEs here, which the caller reports as ERR_1038
            return Err(MediaError::EntryNotFound(image_path.clone()));
        };
        let dimension = if analyze_dimensions {
            // same head-first strategy as the zip path: full read only when the header
            // spans the sniff window
            let head = pkg
                .read_entry_head(image_path, 65536)
                .ok_or_else(|| MediaError::EntryNotFound(image_path.clone()))?;
            image::get_dimension(&head)
                .or_else(|| {
                    pkg.read_entry_bytes(image_path)
                        .and_then(|bytes| image::get_dimension(&bytes))
                })
                .map(|(w, h)| (w as i32, h as i32))
        } else {
            None
        };
        divina_pages.push(BookPage {
            file_name: image_path.clone(),
            media_type,
            width: dimension.map(|d| d.0),
            height: dimension.map(|d| d.1),
            file_hash: String::new(),
            file_size: Some(meta.size),
        });
    }
    if divina_pages.len() != page_count {
        tracing::info!(
            "Epub Divina detection failed: book has {} detected divina pages, but {page_count} total pages",
            divina_pages.len()
        );
        return Ok(vec![]);
    }
    Ok(divina_pages)
}

/// jsoup `body().text()`: all text under the first body element, whitespace-normalized,
/// counted in UTF-16 code units like a Java String
fn body_text_len(doc: &roxmltree::Document) -> usize {
    let Some(body) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "body")
    else {
        return 0;
    };
    let text: String = body
        .descendants()
        .filter(|n| n.is_text())
        .map(|n| n.text().unwrap_or(""))
        .collect();
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.encode_utf16().count()
}

/// `EpubExtractor.isKepub`: any spine page containing an element with class koboSpan
fn is_kepub(pkg: &mut EpubPackage, resources: &[MediaFile]) -> bool {
    let selector = scraper::Selector::parse(".koboSpan").expect("valid selector");
    for file in resources
        .iter()
        .filter(|r| r.sub_type == Some(MediaFileSubType::EpubPage))
    {
        let Some(content) = pkg.read_entry_string(&file.file_name) else {
            continue;
        };
        let doc = scraper::Html::parse_document(&content);
        if doc.select(&selector).next().is_some() {
            return true;
        }
    }
    false
}

/// `EpubExtractor.computePageCount`: 1 page per 1024 bytes of COMPRESSED data per spine entry
fn compute_page_count(pkg: &mut EpubPackage) -> i32 {
    let spine_paths: Vec<String> = spine_idrefs(&pkg.opf_content)
        .iter()
        .filter_map(|idref| {
            pkg.manifest_item(idref)
                .map(|item| normalize_href(pkg.opf_dir.as_deref(), &item.href))
        })
        .collect();
    pkg.entry_metas()
        .into_iter()
        .filter(|m| spine_paths.contains(&m.name))
        .map(|m| (m.compressed_size as f64 / 1024.0).ceil() as i64)
        .sum::<i64>() as i32
}

/// `EpubExtractor.isFixedLayout`
fn is_fixed_layout(pkg: &EpubPackage) -> bool {
    let Ok(doc) = parse_xml(&pkg.opf_content) else {
        return false;
    };
    let meta = |name: &str, value: &str| {
        doc.descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "metadata")
            .flat_map(|m| m.children())
            .find(|c| {
                c.is_element() && c.tag_name().name() == "meta" && c.attribute(name) == Some(value)
            })
            .map(|c| c.attribute("content").map(|s| s.to_string()))
    };
    let rendition = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "metadata")
        .flat_map(|m| m.children())
        .find(|c| {
            c.is_element()
                && c.tag_name().name() == "meta"
                && c.attribute("property") == Some("rendition:layout")
        })
        .map(|c| c.text().unwrap_or(""));
    if rendition == Some("pre-paginated") {
        return true;
    }
    meta("name", "fixed-layout").flatten().as_deref() == Some("true")
}

/// `EpubExtractor.computePositions`
fn compute_positions(
    pkg: &mut EpubPackage,
    resources: &[MediaFile],
    is_fixed_layout: bool,
    is_kepub: bool,
    book_path: &Path,
    kepubify_path: Option<&Path>,
) -> Result<Vec<R2Locator>> {
    let reading_order: Vec<&MediaFile> = resources
        .iter()
        .filter(|r| r.sub_type == Some(MediaFileSubType::EpubPage))
        .collect();

    let kobo_positions: HashMap<String, Vec<(String, f32)>> = if is_fixed_layout {
        HashMap::new()
    } else if is_kepub {
        compute_positions_from_kobo_span(&reading_order, &mut |name| pkg.read_entry_string(name))?
    } else if let Some(kepubify_path) = kepubify_path {
        positions_via_kepubify(kepubify_path, book_path, &reading_order).unwrap_or_else(|| {
            tracing::warn!(
                "Could not convert to Kepub to compute positions: {}",
                book_path.display()
            );
            HashMap::new()
        })
    } else {
        HashMap::new()
    };

    let mut start_position = 1i32;
    let mut positions: Vec<R2Locator> = vec![];
    if is_fixed_layout {
        for file in &reading_order {
            positions.push(R2Locator {
                href: file.file_name.clone(),
                type_: file
                    .media_type
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".into()),
                title: None,
                locations: Some(R2Location {
                    fragments: vec![],
                    progression: Some(0.0),
                    position: Some(start_position),
                    total_progression: None,
                }),
                text: None,
                kobo_span: Some("kobo.1.1".into()),
            });
            start_position += 1;
        }
    } else {
        for file in &reading_order {
            let position_count =
                ((file.file_size.unwrap_or(0) as f64 / 1024.0).ceil() as i32).max(1);
            for p in 0..position_count {
                let progression = p as f32 / position_count as f32;
                let kobo_span = if position_count == 1 || p == 0 {
                    Some("kobo.1.1".to_string())
                } else {
                    kobo_positions.get(&file.file_name).and_then(|entries| {
                        entries
                            .iter()
                            .min_by(|a, b| {
                                (progression - a.1)
                                    .abs()
                                    .partial_cmp(&(progression - b.1).abs())
                                    .expect("progressions are finite")
                            })
                            .map(|e| e.0.clone())
                    })
                };
                positions.push(R2Locator {
                    href: file.file_name.clone(),
                    type_: file
                        .media_type
                        .clone()
                        .unwrap_or_else(|| "application/octet-stream".into()),
                    title: None,
                    locations: Some(R2Location {
                        fragments: vec![],
                        progression: Some(progression),
                        position: Some(start_position),
                        total_progression: None,
                    }),
                    text: None,
                    kobo_span,
                });
                start_position += 1;
            }
        }
    }

    let total = positions.len() as f32;
    Ok(positions
        .into_iter()
        .map(|mut l| {
            if let Some(loc) = &mut l.locations {
                loc.total_progression = loc.position.map(|p| p as f32 / total);
            }
            l
        })
        .collect())
}

/// `EpubExtractor`: plain EPUBs are converted to a temporary KEPUB so positions can be read
/// from real kobo spans; the converted file is deleted right after.
fn positions_via_kepubify(
    kepubify_path: &Path,
    book_path: &Path,
    reading_order: &[&MediaFile],
) -> Option<HashMap<String, Vec<(String, f32)>>> {
    // the output name is derived from the source file stem, so same-named EPUBs converted
    // concurrently would clobber each other in the shared temp dir — isolate per call
    let tmp = tempfile::tempdir().ok()?;
    let kepub = crate::kepubify::convert(kepubify_path, book_path, Some(tmp.path()))?;
    let result = std::fs::File::open(&kepub)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .and_then(|mut archive| {
            compute_positions_from_kobo_span(reading_order, &mut |name| {
                let mut entry = archive.by_name(name).ok()?;
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).ok()?;
                Some(String::from_utf8_lossy(&buf).into_owned())
            })
            .ok()
        });
    result
}

/// `EpubExtractor.computePositionsFromKoboSpan`: koboSpan id → progression per resource.
/// Byte offsets approximate jsoup's UTF-16 `sourceRange().endPos()` (identical for ASCII).
fn compute_positions_from_kobo_span(
    reading_order: &[&MediaFile],
    supplier: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<HashMap<String, Vec<(String, f32)>>> {
    let mut map = HashMap::new();
    for file in reading_order {
        let entries = supplier(&file.file_name)
            .map(|html| scan_kobo_spans(&html, file.file_size.unwrap_or(0)))
            .unwrap_or_default();
        map.insert(file.file_name.clone(), entries);
    }
    Ok(map)
}

fn scan_kobo_spans(html: &str, file_size: i64) -> Vec<(String, f32)> {
    let hay = html.as_bytes();
    let mut out = vec![];
    let mut search_from = 0;
    while let Some(start) = find_subslice(hay, b"<span", search_from) {
        let Some(tag_end) = find_subslice(hay, b">", start) else {
            break;
        };
        let tag = &html[start..=tag_end];
        let is_kobo_span = extract_attr(tag, "class")
            .map(|c| c.split_whitespace().any(|c| c == "koboSpan"))
            .unwrap_or(false);
        if is_kobo_span {
            if let Some(id) = extract_attr(tag, "id") {
                if !id.is_empty() {
                    let end_pos = find_subslice(hay, b"</span>", tag_end)
                        .map(|i| i + "</span>".len())
                        .unwrap_or(tag_end + 1);
                    out.push((id, end_pos as f32 / file_size.max(1) as f32));
                }
            }
        }
        search_from = tag_end + 1;
    }
    out
}

fn find_subslice(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

/// Attribute extraction from a raw HTML/XML tag string; handles single and double quotes
fn extract_attr(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(i) = tag.find(&needle) {
            let rest = &tag[i + needle.len()..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

// endregion

// region epub navigation

struct ResourceContent {
    path: String,
    content: String,
}

/// `EpubPackage.getNavResource()`
fn get_nav_resource(pkg: &mut EpubPackage) -> Option<ResourceContent> {
    let nav = pkg
        .manifest
        .iter()
        .find(|item| item.properties.contains("nav"))?
        .clone();
    let href = normalize_href(pkg.opf_dir.as_deref(), &nav.href);
    let content = pkg.read_entry_string(&href)?;
    Some(ResourceContent {
        path: href,
        content,
    })
}

/// `EpubPackage.getNcxResource()`
fn get_ncx_resource(pkg: &mut EpubPackage) -> Option<ResourceContent> {
    const NCX_IDS: [&str; 3] = ["toc", "ncx", "ncxtoc"];
    let ncx = pkg
        .manifest
        .iter()
        .find(|item| item.media_type == "application/x-dtbncx+xml")
        .or_else(|| {
            pkg.manifest
                .iter()
                .find(|item| NCX_IDS.contains(&item.id.as_str()))
        })?
        .clone();
    let href = normalize_href(pkg.opf_dir.as_deref(), &ncx.href);
    let content = pkg.read_entry_string(&href)?;
    Some(ResourceContent {
        path: href,
        content,
    })
}

/// `EpubExtractor.getToc`
fn get_toc(pkg: &mut EpubPackage) -> Result<Vec<EpubTocEntry>> {
    if let Some(nav) = get_nav_resource(pkg) {
        let entries = process_nav(&nav.content, parent_dir(&nav.path), "toc");
        if !entries.is_empty() {
            return Ok(entries);
        }
    }
    if let Some(ncx) = get_ncx_resource(pkg) {
        return Ok(process_ncx(
            &ncx.content,
            parent_dir(&ncx.path),
            "navMap",
            "navPoint",
        ));
    }
    Ok(vec![])
}

/// `EpubExtractor.getPageList`
fn get_page_list(pkg: &mut EpubPackage) -> Result<Vec<EpubTocEntry>> {
    if let Some(nav) = get_nav_resource(pkg) {
        let entries = process_nav(&nav.content, parent_dir(&nav.path), "page-list");
        if !entries.is_empty() {
            return Ok(entries);
        }
    }
    if let Some(ncx) = get_ncx_resource(pkg) {
        return Ok(process_ncx(
            &ncx.content,
            parent_dir(&ncx.path),
            "pageList",
            "pageTarget",
        ));
    }
    Ok(vec![])
}

/// `EpubExtractor.getLandmarks`
fn get_landmarks(pkg: &mut EpubPackage) -> Result<Vec<EpubTocEntry>> {
    if let Some(nav) = get_nav_resource(pkg) {
        let entries = process_nav(&nav.content, parent_dir(&nav.path), "landmarks");
        if !entries.is_empty() {
            return Ok(entries);
        }
    }
    Ok(process_opf_guide(pkg))
}

/// `processNav`
fn process_nav(content: &str, nav_dir: Option<String>, nav_type: &str) -> Vec<EpubTocEntry> {
    let Ok(doc) = parse_xml(content) else {
        return vec![];
    };
    let Some(nav) = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "nav")
        .find(|n| {
            n.attributes()
                .any(|a| a.name().ends_with("type") && a.value() == nav_type)
        })
    else {
        return vec![];
    };
    let Some(ol) = nav
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "ol")
    else {
        return vec![];
    };
    ol.children()
        .filter(|c| c.is_element() && c.tag_name().name() == "li")
        .filter_map(|li| nav_li_to_toc_entry(&li, nav_dir.as_deref()))
        .collect()
}

fn nav_li_to_toc_entry(
    li: &roxmltree::Node<'_, '_>,
    nav_dir: Option<&str>,
) -> Option<EpubTocEntry> {
    let title = li
        .children()
        .find(|c| c.is_element() && (c.tag_name().name() == "a" || c.tag_name().name() == "span"))
        .map(|c| text_content(&c))?;
    let href = li
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "a")
        .and_then(|a| a.attribute("href"))
        .map(|h| normalize_href(nav_dir, &percent_decode(h)));
    let children = li
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "ol")
        .map(|ol| {
            ol.children()
                .filter(|c| c.is_element() && c.tag_name().name() == "li")
                .filter_map(|li2| nav_li_to_toc_entry(&li2, nav_dir))
                .collect()
        })
        .unwrap_or_default();
    Some(EpubTocEntry {
        title,
        href,
        children,
    })
}

/// `processNcx`
fn process_ncx(
    content: &str,
    ncx_dir: Option<String>,
    level1: &str,
    level2: &str,
) -> Vec<EpubTocEntry> {
    let Ok(doc) = parse_xml(content) else {
        return vec![];
    };
    let mut out = vec![];
    for map in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == level1)
    {
        for el in map
            .children()
            .filter(|c| c.is_element() && c.tag_name().name() == level2)
        {
            if let Some(entry) = ncx_el_to_toc_entry(&el, level2, ncx_dir.as_deref()) {
                out.push(entry);
            }
        }
    }
    out
}

fn ncx_el_to_toc_entry(
    el: &roxmltree::Node<'_, '_>,
    level2: &str,
    ncx_dir: Option<&str>,
) -> Option<EpubTocEntry> {
    let title = el
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "navLabel")
        .and_then(|l| {
            l.children()
                .find(|c| c.is_element() && c.tag_name().name() == "text")
        })
        .map(|t| text_content(&t))?;
    let href = el
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "content")
        .and_then(|c| c.attribute("src"))
        .map(|s| normalize_href(ncx_dir, &percent_decode(s)));
    let children = el
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == level2)
        .filter_map(|c| ncx_el_to_toc_entry(&c, level2, ncx_dir))
        .collect();
    Some(EpubTocEntry {
        title,
        href,
        children,
    })
}

/// `processOpfGuide`
fn process_opf_guide(pkg: &EpubPackage) -> Vec<EpubTocEntry> {
    let Ok(doc) = parse_xml(&pkg.opf_content) else {
        return vec![];
    };
    let Some(guide) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "guide")
    else {
        return vec![];
    };
    guide
        .descendants()
        .filter(|c| c.is_element() && c.tag_name().name() == "reference")
        .map(|r| EpubTocEntry {
            title: r.attribute("title").unwrap_or_default().to_string(),
            href: r
                .attribute("href")
                .filter(|h| !h.is_empty())
                .map(|h| normalize_href(pkg.opf_dir.as_deref(), &percent_decode(h))),
            children: vec![],
        })
        .collect()
}

/// jsoup `element.text()`: descendant text nodes, whitespace-normalized
fn text_content(node: &roxmltree::Node<'_, '_>) -> String {
    let text: String = node
        .descendants()
        .filter(|n| n.is_text())
        .map(|n| n.text().unwrap_or(""))
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// endregion

// region epub cover

/// `EpubExtractor.getCover`: EPUB 3 `cover-image` property → EPUB 2 `meta[name=cover]` →
/// `id="cover-image"`
fn get_cover(book_path: &Path) -> Option<PageContent> {
    let mut pkg = open_epub(book_path).ok()?;
    let cover_item = pkg
        .manifest
        .iter()
        .find(|item| item.properties.contains("cover-image"))
        .or_else(|| {
            let doc = parse_xml(&pkg.opf_content).ok()?;
            let meta_cover = doc
                .descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "metadata")
                .flat_map(|m| m.children())
                .find(|c| {
                    c.is_element()
                        && c.tag_name().name() == "meta"
                        && c.attribute("name") == Some("cover")
                })
                .and_then(|m| m.attribute("content"))
                .filter(|c| !c.is_empty());
            meta_cover.and_then(|id| pkg.manifest_item(id))
        })
        .or_else(|| pkg.manifest.iter().find(|item| item.id == "cover-image"));
    let item = cover_item?.clone();
    let cover_path = normalize_href(pkg.opf_dir.as_deref(), &percent_decode(&item.href));
    let bytes = pkg.read_entry_bytes(&cover_path)?;
    Some(PageContent {
        bytes,
        media_type: item.media_type,
    })
}

// endregion

// region path helpers

/// `Opf.kt#normalizeHref`: resolves `href` against `opf_dir`, keeping the fragment
fn normalize_href(opf_dir: Option<&str>, href: &str) -> String {
    let (base, anchor) = match href.rfind('#') {
        Some(i) => (&href[..i], &href[i + 1..]),
        None => (href, ""),
    };
    let resolved = match opf_dir {
        Some(dir) => normalize_zip_path(&join_path(dir, base)),
        // Kotlin does not normalize when opfDir is null (root-level OPF)
        None => base.to_string(),
    };
    if anchor.is_empty() {
        resolved
    } else {
        format!("{resolved}#{anchor}")
    }
}

/// Java `Path.resolve`: an absolute `base` wins
fn join_path(dir: &str, base: &str) -> String {
    if base.starts_with('/') || dir.is_empty() {
        base.to_string()
    } else {
        format!("{dir}/{base}")
    }
}

/// Java `Path.normalize` over forward-slash zip paths
fn normalize_zip_path(path: &str) -> String {
    let mut segments: Vec<&str> = vec![];
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if matches!(segments.last(), Some(s) if *s != "..") {
                    segments.pop();
                } else {
                    segments.push(seg);
                }
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}

/// `(Path(pagePath).parent ?: Path("")).resolve(src).normalize()` in Kotlin terms
fn resolve_relative(page_path: &str, src: &str) -> String {
    let base = match page_path.rfind('/') {
        Some(i) => &page_path[..i + 1],
        None => "",
    };
    if src.starts_with('/') {
        normalize_zip_path(src)
    } else {
        normalize_zip_path(&format!("{base}{src}"))
    }
}

/// Java `Path.getParent`: None for a single-element path
fn parent_dir(path: &str) -> Option<String> {
    match path.rfind('/') {
        Some(i) if i > 0 => Some(path[..i].to_string()),
        _ => None,
    }
}

/// `URLDecoder.decode(s, UTF_8)`: `%XX` → byte, `+` → space; invalid sequences are kept as-is
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::media::BookPage;

    fn fixtures() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources")
    }

    fn analyzer() -> Analyzer {
        Analyzer::new(3, 300, 15, None)
    }

    fn make_png(w: u32, h: u32) -> Vec<u8> {
        let img = ::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(
            w,
            h,
            ::image::Rgba([10, 200, 30, 255]),
        ));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, ::image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = ::image::DynamicImage::ImageRgb8(::image::RgbImage::from_pixel(
            w,
            h,
            ::image::Rgb([200, 30, 10]),
        ));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, ::image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kmrs-analyzer-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // region analyze: divina

    #[test]
    fn analyze_zip_ready() {
        let media = analyzer()
            .analyze(&fixtures().join("archives/zip.zip"), false)
            .media;
        assert_eq!(media.status, MediaStatus::Ready);
        assert_eq!(media.media_type.as_deref(), Some(detect::APPLICATION_ZIP));
        assert_eq!(media.page_count, 1);
        assert_eq!(media.pages[0].file_name, "komga.png");
        assert_eq!(media.pages[0].media_type, detect::IMAGE_PNG);
        assert_eq!(media.pages[0].width, None);
        assert_eq!(media.pages[0].file_size, Some(3108));
        assert_eq!(media.comment, None);
    }

    #[test]
    fn analyze_zip_with_dimensions() {
        let media = analyzer()
            .analyze(&fixtures().join("archives/zip.zip"), true)
            .media;
        assert_eq!(media.pages[0].width, Some(48));
        assert_eq!(media.pages[0].height, Some(48));
    }

    #[test]
    fn analyze_zip_with_dimensions_beyond_sniff_head() {
        let dir = tmpdir("dims-beyond-head");
        let book = dir.join("big-exif.zip");
        // an APP1 segment just large enough to push the SOF past the 64 KiB sniff window:
        // dimensions must come from the fallback full read
        let jpeg = make_jpeg(48, 32);
        let app1_len: u16 = u16::MAX - 2; // segment length includes its own 2 bytes
        let mut padded = vec![0xFF, 0xD8, 0xFF, 0xE1];
        padded.extend_from_slice(&app1_len.to_be_bytes());
        padded.extend(std::iter::repeat_n(0u8, app1_len as usize - 2));
        padded.extend_from_slice(&jpeg[2..]);
        write_zip(&book, &[("p1.jpg", &padded)]);

        let media = analyzer().analyze(&book, true).media;
        assert_eq!(media.status, MediaStatus::Ready);
        assert_eq!(media.pages[0].width, Some(48));
        assert_eq!(media.pages[0].height, Some(32));
    }

    /// Guards the head-first dimension reads: analyzing a big entry must not pull the whole
    /// entry through the reader (kmworks/kmrs#40 — full reads collapse on network mounts)
    #[test]
    fn analyze_zip_dimensions_do_not_read_full_entries() {
        use std::cell::Cell;
        use std::io::{Cursor, Seek, SeekFrom};
        use std::rc::Rc;

        struct CountingCursor {
            inner: Cursor<Vec<u8>>,
            bytes: Rc<Cell<usize>>,
        }
        impl Read for CountingCursor {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.inner.read(buf)?;
                self.bytes.set(self.bytes.get() + n);
                Ok(n)
            }
        }
        impl Seek for CountingCursor {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
                self.inner.seek(pos)
            }
        }

        // noise compresses badly, keeping the entry well over the 64 KiB sniff window
        let mut img = ::image::RgbImage::new(800, 600);
        let mut state = 0x9e3779b97f4a7c15u64;
        for px in img.pixels_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let v = (state >> 33) as u8;
            *px = ::image::Rgb([v, v, v]);
        }
        let mut jpeg = Cursor::new(Vec::new());
        ::image::DynamicImage::ImageRgb8(img)
            .write_to(&mut jpeg, ::image::ImageFormat::Jpeg)
            .unwrap();
        let jpeg = jpeg.into_inner();
        assert!(jpeg.len() > 65536, "entry must exceed the sniff window");

        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut zip_buf);
            writer
                .start_file("p1.jpg", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, &jpeg).unwrap();
            writer.finish().unwrap();
        }
        let zip_bytes = zip_buf.into_inner();

        let read_bytes = Rc::new(Cell::new(0usize));
        let archive = zip::ZipArchive::new(CountingCursor {
            inner: Cursor::new(zip_bytes),
            bytes: read_bytes.clone(),
        })
        .unwrap();
        let entries = zip_entries_from(archive, true).unwrap();
        assert_eq!(entries[0].dimension, Some((800, 600)));
        assert!(
            read_bytes.get() < jpeg.len(),
            "dimension analysis pulled {} bytes for a {} byte entry",
            read_bytes.get(),
            jpeg.len()
        );
    }

    #[test]
    fn analyze_zip_compression_variants() {
        for name in [
            "zip-copy.zip",
            "zip-bzip2.zip",
            "zip-lzma.zip",
            "zip-ppmd.zip",
            "zip-deflate64.zip",
        ] {
            let media = analyzer()
                .analyze(&fixtures().join("archives").join(name), false)
                .media;
            assert_eq!(media.status, MediaStatus::Ready, "{name}");
            assert_eq!(media.page_count, 1, "{name}");
        }
    }

    #[test]
    fn analyze_zip_encrypted_is_error() {
        let media = analyzer()
            .analyze(&fixtures().join("archives/zip-encrypted.zip"), false)
            .media;
        assert_eq!(media.status, MediaStatus::Error);
        assert_eq!(media.media_type.as_deref(), Some(detect::APPLICATION_ZIP));
    }

    #[test]
    fn analyze_zip_without_images_is_err_1006() {
        let dir = tmpdir("err1006");
        let book = dir.join("text-only.zip");
        write_zip(&book, &[("readme.txt", b"hello world")]);
        let media = analyzer().analyze(&book, false).media;
        assert_eq!(media.status, MediaStatus::Error);
        assert_eq!(media.comment.as_deref(), Some("ERR_1006"));
    }

    #[test]
    fn analyze_zip_with_unreadable_entry_is_err_1007() {
        let dir = tmpdir("err1007");
        let book = dir.join("mixed.zip");
        let png = make_png(48, 48);
        write_zip(&book, &[("good.png", &png), ("bad.png", &png)]);

        // patch bad.png's compressed data: the read then fails on CRC / inflation
        let mut bytes = std::fs::read(&book).unwrap();
        let name = b"bad.png";
        let pos = bytes
            .windows(name.len())
            .position(|w| w == name)
            .expect("bad.png local header");
        let header = pos - 30;
        assert!(bytes[header..pos].starts_with(b"PK\x03\x04"));
        let extra_len = u16::from_le_bytes([bytes[header + 28], bytes[header + 29]]) as usize;
        let data_start = pos + name.len() + extra_len;
        for b in &mut bytes[data_start..data_start + 16] {
            *b ^= 0xFF;
        }
        std::fs::write(&book, &bytes).unwrap();

        let media = analyzer().analyze(&book, true).media;
        assert_eq!(media.status, MediaStatus::Ready);
        assert_eq!(media.page_count, 1);
        assert_eq!(media.pages[0].file_name, "good.png");
        assert_eq!(media.pages[0].width, Some(48));
        assert_eq!(media.comment.as_deref(), Some("ERR_1007 [bad.png]"));
        let bad = media
            .files
            .iter()
            .find(|f| f.file_name == "bad.png")
            .unwrap();
        assert_eq!(bad.media_type, None);
    }

    #[test]
    fn analyze_rar_ready() {
        for (name, pages, first_page) in
            [("rar4.rar", 3, "komga-1.png"), ("rar5.rar", 1, "komga.png")]
        {
            let media = analyzer()
                .analyze(&fixtures().join("archives").join(name), false)
                .media;
            assert_eq!(media.status, MediaStatus::Ready, "{name}");
            assert_eq!(media.page_count, pages, "{name}");
            assert!(media
                .media_type
                .as_deref()
                .unwrap()
                .starts_with("application/x-rar-compressed"));
            assert_eq!(media.pages[0].file_name, first_page, "{name}");
        }
    }

    #[test]
    fn analyze_rar_solid_ready() {
        for name in ["rar4-solid.rar", "rar5-solid.rar"] {
            let media = analyzer()
                .analyze(&fixtures().join("archives").join(name), false)
                .media;
            assert_eq!(media.status, MediaStatus::Ready, "{name}");
            assert_eq!(media.page_count, 3, "{name}");
        }
    }

    #[test]
    fn analyze_rar_encrypted_is_err_1002() {
        for name in ["rar4-encrypted.rar", "rar5-encrypted.rar"] {
            let media = analyzer()
                .analyze(&fixtures().join("archives").join(name), false)
                .media;
            assert_eq!(media.status, MediaStatus::Unsupported, "{name}");
            assert_eq!(media.comment.as_deref(), Some("ERR_1002"), "{name}");
        }
    }

    #[test]
    fn analyze_7z_is_err_1001() {
        let media = analyzer()
            .analyze(&fixtures().join("archives/7zip.7z"), false)
            .media;
        assert_eq!(media.status, MediaStatus::Unsupported);
        assert_eq!(media.comment.as_deref(), Some("ERR_1001"));
        assert_eq!(
            media.media_type.as_deref(),
            Some("application/x-7z-compressed")
        );
    }

    #[test]
    fn analyze_image_file_is_err_1001() {
        let dir = tmpdir("err1001");
        let book = dir.join("not-a-book.png");
        std::fs::write(&book, make_png(48, 48)).unwrap();
        let media = analyzer().analyze(&book, false).media;
        assert_eq!(media.status, MediaStatus::Unsupported);
        assert_eq!(media.comment.as_deref(), Some("ERR_1001"));
        assert_eq!(media.media_type.as_deref(), Some(detect::IMAGE_PNG));
    }

    #[test]
    fn analyze_missing_file_is_err_1018() {
        let media = analyzer()
            .analyze(&fixtures().join("archives/does-not-exist.zip"), false)
            .media;
        assert_eq!(media.status, MediaStatus::Error);
        assert_eq!(media.comment.as_deref(), Some("ERR_1018"));
    }

    // endregion

    // region analyze: epub

    #[test]
    fn analyze_fake_epub_is_err_1032() {
        let media = analyzer()
            .analyze(&fixtures().join("archives/zip-as-epub.epub"), false)
            .media;
        assert_eq!(media.status, MediaStatus::Error);
        assert_eq!(media.comment.as_deref(), Some("ERR_1032"));
        assert_eq!(media.media_type.as_deref(), Some(detect::APPLICATION_ZIP));
        assert!(media.pages.is_empty());
    }

    #[test]
    fn analyze_epub3_ready_divina() {
        let analysis = analyzer().analyze(&fixtures().join("archives/epub3.epub"), false);
        let media = &analysis.media;
        assert_eq!(media.status, MediaStatus::Ready);
        assert_eq!(media.media_type.as_deref(), Some(detect::APPLICATION_EPUB));
        assert!(media.epub_divina_compatible);
        assert!(!media.epub_is_kepub);
        assert_eq!(media.page_count, 2);
        assert_eq!(media.pages[0].file_name, "cover.jpeg");
        assert_eq!(media.pages[0].media_type, detect::IMAGE_JPEG);
        assert_eq!(media.pages[0].file_size, Some(1638));
        assert_eq!(media.pages[1].file_name, "0_0.png");
        assert_eq!(media.pages[1].media_type, detect::IMAGE_PNG);
        assert_eq!(media.comment, None);
        assert_eq!(media.files.len(), 7);

        let ext = analysis.epub_extension.as_ref().expect("epub extension");
        assert!(ext.is_fixed_layout);
        assert_eq!(ext.toc.len(), 1);
        assert_eq!(ext.toc[0].title, "Page 1");
        assert_eq!(ext.toc[0].href.as_deref(), Some("page_1.xhtml"));
        assert_eq!(ext.landmarks.len(), 1);
        assert_eq!(ext.landmarks[0].title, "Cover");
        assert_eq!(ext.landmarks[0].href.as_deref(), Some("titlepage.xhtml"));
        assert!(ext.page_list.is_empty());

        assert_eq!(ext.positions.len(), 2);
        let p0 = &ext.positions[0];
        assert_eq!(p0.href, "titlepage.xhtml");
        assert_eq!(p0.type_, "application/xhtml+xml");
        assert_eq!(p0.kobo_span.as_deref(), Some("kobo.1.1"));
        let loc0 = p0.locations.as_ref().unwrap();
        assert_eq!(loc0.progression, Some(0.0));
        assert_eq!(loc0.position, Some(1));
        assert_eq!(loc0.total_progression, Some(0.5));
        let p1 = &ext.positions[1];
        assert_eq!(p1.href, "page_1.xhtml");
        assert_eq!(p1.locations.as_ref().unwrap().position, Some(2));
        assert_eq!(p1.locations.as_ref().unwrap().total_progression, Some(1.0));
    }

    #[test]
    fn analyze_epub_text_book_positions() {
        let analysis = analyzer().analyze(
            &fixtures().join("epub/The Incomplete Theft - Ralph Burke.epub"),
            false,
        );
        let media = &analysis.media;
        assert_eq!(media.status, MediaStatus::Ready);
        assert!(!media.epub_divina_compatible);
        assert!(!media.epub_is_kepub);
        assert_eq!(media.page_count, 14);
        assert_eq!(media.files.len(), 8);
        assert_eq!(media.comment, None);

        let ext = analysis.epub_extension.as_ref().unwrap();
        assert!(!ext.is_fixed_layout);
        assert_eq!(ext.toc.len(), 1);
        assert_eq!(ext.toc[0].title, "The Incomplete Theft");
        assert_eq!(
            ext.toc[0].href.as_deref(),
            Some("OEBPS/@public@vhost@g@gutenberg@html@files@65659@65659-h@65659-h-0.htm_split_001.html")
        );
        assert_eq!(ext.landmarks.len(), 1);
        assert_eq!(ext.landmarks[0].title, "Cover");

        let positions = &ext.positions;
        assert_eq!(positions.len(), 35);
        let at = |i: usize| positions[i].locations.as_ref().unwrap();
        assert_eq!(positions[0].href, "titlepage.xhtml");
        assert_eq!(at(0).position, Some(1));
        assert_eq!(positions[0].kobo_span.as_deref(), Some("kobo.1.1"));
        assert!((at(0).total_progression.unwrap() - 1.0 / 35.0).abs() < 1e-6);

        assert!(positions[1].href.ends_with("split_000.html"));
        assert_eq!(at(1).position, Some(2));
        assert_eq!(at(1).progression, Some(0.0));
        assert_eq!(at(2).position, Some(3));
        assert_eq!(at(2).progression, Some(0.5));
        assert_eq!(positions[2].kobo_span, None);

        assert!(positions[3].href.ends_with("split_001.html"));
        assert_eq!(at(3).position, Some(4));

        assert!(positions[4].href.ends_with("split_002.html"));
        assert_eq!(at(4).position, Some(5));
        assert_eq!(at(4).progression, Some(0.0));
        assert_eq!(at(34).position, Some(35));
        assert!((at(34).progression.unwrap() - 30.0 / 31.0).abs() < 1e-6);
        assert_eq!(at(34).total_progression, Some(1.0));
    }

    // region analyze: epub positions via kepubify

    fn executable_script(path: &Path, content: &str) {
        std::fs::write(path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn kepub_fixture_paths(
        dir: &Path,
    ) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let source = dir.join("plain-book.epub");
        let converted = dir.join("converted.epub");
        let script = dir.join("kepubify");
        (source, converted, script)
    }

    /// Builds a plain EPUB and its would-be kepubify output (same page with kobo spans).
    /// The plain page is 2440 bytes: 3 positions at progressions 0, 1/3, 2/3. In the converted
    /// page the kobo.9.9 span ends at byte 1303 (progression 1303/2440 ≈ 0.53), the nearest
    /// span for both p1 and p2.
    fn write_plain_and_converted_epubs(source: &Path, converted: &Path) {
        let text1 = "lorem ".repeat(200);
        let text2 = "ipsum ".repeat(200);
        let container = br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
        let opf = br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="id">
  <metadata><dc:identifier xmlns:dc="http://purl.org/dc/elements/1.1/">id</dc:identifier></metadata>
  <manifest><item id="p1" href="page1.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="p1"/></spine>
</package>"#;
        let plain_page = format!("<html><body><p>{text1}</p><p>{text2}</p></body></html>");
        assert_eq!(
            plain_page.len(),
            2440,
            "position math depends on the page size"
        );
        let converted_page = format!(
            "<html><body><span class=\"koboSpan\" id=\"kobo.1.1\"></span><p>{text1}<span class=\"koboSpan\" id=\"kobo.9.9\"></span></p><p>{text2}<span class=\"koboSpan\" id=\"kobo.9.10\"></span></p></body></html>"
        );
        write_zip(
            source,
            &[
                ("mimetype", b"application/epub+zip".as_slice()),
                ("META-INF/container.xml", container.as_slice()),
                ("content.opf", opf.as_slice()),
                ("page1.xhtml", plain_page.as_bytes()),
            ],
        );
        write_zip(
            converted,
            &[
                ("mimetype", b"application/epub+zip".as_slice()),
                ("META-INF/container.xml", container.as_slice()),
                ("content.opf", opf.as_slice()),
                ("page1.xhtml", converted_page.as_bytes()),
            ],
        );
    }

    #[test]
    fn analyze_epub_positions_from_kepubify_conversion() {
        let dir = tmpdir("kepub-positions");
        let (source, converted, script) = kepub_fixture_paths(&dir);
        write_plain_and_converted_epubs(&source, &converted);
        executable_script(
            &script,
            &format!("#!/bin/sh\ncp \"{}\" \"$3\"\n", converted.display()),
        );

        let analysis = Analyzer::new(3, 300, 15, Some(script.clone())).analyze(&source, false);
        let media = &analysis.media;
        assert_eq!(media.status, MediaStatus::Ready);
        assert!(!media.epub_is_kepub);

        let ext = analysis.epub_extension.as_ref().unwrap();
        assert!(!ext.is_fixed_layout);
        let positions = &ext.positions;
        // 2440 bytes -> 3 positions; p0 is hardcoded, p1/p2 map to spans of the converted file
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[0].kobo_span.as_deref(), Some("kobo.1.1"));
        assert_eq!(positions[1].kobo_span.as_deref(), Some("kobo.9.9"));
        assert_eq!(positions[2].kobo_span.as_deref(), Some("kobo.9.9"));
        // conversion runs in a per-call temp dir, nothing leaks into the shared one
        assert!(!std::env::temp_dir().join("plain-book.kepub.epub").exists());
    }

    #[test]
    fn analyze_epub_positions_kepubify_failure_degrades() {
        let dir = tmpdir("kepub-positions-fail");
        let (source, converted, script) = kepub_fixture_paths(&dir);
        write_plain_and_converted_epubs(&source, &converted);
        executable_script(&script, "#!/bin/sh\nexit 1\n");

        let analysis = Analyzer::new(3, 300, 15, Some(script.clone())).analyze(&source, false);
        assert_eq!(analysis.media.status, MediaStatus::Ready);
        let ext = analysis.epub_extension.as_ref().unwrap();
        let positions = &ext.positions;
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[0].kobo_span.as_deref(), Some("kobo.1.1"));
        assert_eq!(positions[1].kobo_span, None);
        assert_eq!(positions[2].kobo_span, None);
    }

    // endregion

    #[test]
    fn analyze_pdf_ready() {
        if !pdf::pdf_available() {
            eprintln!("libpdfium not available, skipping");
            return;
        }
        let media = analyzer()
            .analyze(&fixtures().join("pdf/komga.pdf"), true)
            .media;
        assert_eq!(media.status, MediaStatus::Ready);
        assert_eq!(media.media_type.as_deref(), Some(detect::APPLICATION_PDF));
        assert!(media.page_count > 0);
        assert_eq!(media.pages[0].file_name, "1");
        assert!(media.pages[0].width.is_some());
    }

    // endregion

    // region thumbnails and posters

    #[test]
    fn generate_thumbnail_from_zip() {
        let a = analyzer();
        let media = a.analyze(&fixtures().join("archives/zip.zip"), false).media;
        let thumb = a
            .generate_thumbnail(&fixtures().join("archives/zip.zip"), &media)
            .unwrap();
        assert_eq!(thumb.media_type, detect::IMAGE_JPEG);
        // source is 48x48: no upscale
        assert_eq!((thumb.width, thumb.height), (48, 48));
        assert_eq!(thumb.file_size as usize, thumb.bytes.len());
        assert_eq!(&thumb.bytes[0..3], b"\xFF\xD8\xFF");
    }

    #[test]
    fn generate_thumbnail_not_ready() {
        let media = media(MediaStatus::Unknown, None, None);
        assert!(matches!(
            analyzer().generate_thumbnail(&fixtures().join("archives/zip.zip"), &media),
            Err(MediaError::NotReady)
        ));
    }

    #[test]
    fn get_poster_zip_first_page() {
        let a = analyzer();
        let media = a.analyze(&fixtures().join("archives/zip.zip"), false).media;
        let poster = a
            .get_poster(&fixtures().join("archives/zip.zip"), &media)
            .unwrap();
        assert_eq!(poster.media_type, detect::IMAGE_PNG);
        assert_eq!(&poster.bytes[0..4], b"\x89PNG");
    }

    #[test]
    fn get_poster_epub_cover() {
        let a = analyzer();
        let media = a
            .analyze(&fixtures().join("archives/epub3.epub"), false)
            .media;
        let poster = a
            .get_poster(&fixtures().join("archives/epub3.epub"), &media)
            .unwrap();
        assert_eq!(poster.media_type, detect::IMAGE_JPEG);
        assert_eq!(&poster.bytes[0..3], b"\xFF\xD8\xFF");
    }

    // endregion

    // region page hashing

    #[test]
    fn hash_pages_first_and_last_three() {
        let dir = tmpdir("hashpages");
        let book = dir.join("book.zip");
        let jpeg = make_jpeg(48, 48);
        let png = make_png(48, 48);
        let mut entries: Vec<(String, Vec<u8>)> = vec![];
        entries.push(("p00.jpg".to_string(), jpeg.clone()));
        for i in 1..11 {
            entries.push((format!("p{i:02}.png"), png.clone()));
        }
        entries.push(("p11.jpg".to_string(), jpeg.clone()));
        let refs: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        write_zip(&book, &refs);

        let a = analyzer();
        let media = a.analyze(&book, false).media;
        assert_eq!(media.page_count, 12);
        let hashed = a.hash_pages(&book, &media).unwrap();

        let hashed_indexes: Vec<usize> = hashed
            .pages
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.file_hash.is_empty())
            .map(|(i, _)| i)
            .collect();
        assert_eq!(hashed_indexes, vec![0, 1, 2, 9, 10, 11]);

        // JPEG pages are hashed after decode + re-encode
        let img_reader = ::image::ImageReader::new(std::io::Cursor::new(&jpeg))
            .with_guessed_format()
            .unwrap();
        let expected_jpeg_hash =
            hash::compute_hash_bytes(&image::encode_jpeg(&img_reader.decode().unwrap()).unwrap());
        assert_eq!(hashed.pages[0].file_hash, expected_jpeg_hash);
        assert_eq!(hashed.pages[11].file_hash, expected_jpeg_hash);
        // PNG pages are hashed on raw bytes
        assert_eq!(hashed.pages[1].file_hash, hash::compute_hash_bytes(&png));
    }

    #[test]
    fn hash_page_jpeg_reencodes() {
        let a = analyzer();
        let jpeg = make_jpeg(48, 48);
        let page = BookPage {
            file_name: "p1.jpg".into(),
            media_type: detect::IMAGE_JPEG.into(),
            width: None,
            height: None,
            file_hash: String::new(),
            file_size: None,
        };
        let h = a.hash_page(&page, &jpeg).unwrap();
        let img_reader = ::image::ImageReader::new(std::io::Cursor::new(&jpeg))
            .with_guessed_format()
            .unwrap();
        let expected =
            hash::compute_hash_bytes(&image::encode_jpeg(&img_reader.decode().unwrap()).unwrap());
        assert_eq!(h, expected);
    }

    // endregion

    // region navigation parsing

    #[test]
    fn ncx_toc_parsing() {
        let content = std::fs::read_to_string(fixtures().join("epub/toc.ncx")).unwrap();
        let toc = process_ncx(&content, None, "navMap", "navPoint");
        assert!(toc.len() >= 3);
        assert_eq!(toc[0].title, "COVER");
        assert_eq!(
            toc[0].href.as_deref(),
            Some("Text/Mart_9780553897852_epub_cvi_r1.htm#b02-cvi")
        );
        assert_eq!(toc[1].title, "BRAN");
        assert_eq!(toc[2].title, "APPENDIX");
        assert!(!toc[2].children.is_empty());
        assert_eq!(toc[2].children[0].title, "THE KINGS AND THEIR COURTS");
        assert_eq!(
            toc[2].children[0].href.as_deref(),
            Some("Text/Mart_9780553897852_epub_app_r1.htm#apps01.00")
        );
    }

    #[test]
    fn nav_toc_landmarks_pagelist_parsing() {
        let content = std::fs::read_to_string(fixtures().join("epub/nav.xhtml")).unwrap();

        let toc = process_nav(&content, None, "toc");
        assert_eq!(toc.len(), 7);
        assert_eq!(toc[0].title, "Cover");
        assert_eq!(toc[0].href.as_deref(), Some("cover.xhtml"));
        assert_eq!(toc[4].title, "An unlinked heading");
        assert_eq!(toc[4].href, None);
        assert_eq!(toc[5].title, "Introduction");
        assert_eq!(toc[5].children.len(), 4);
        assert_eq!(toc[5].children[0].title, "Spring");
        assert_eq!(
            toc[5].children[0].href.as_deref(),
            Some("chapter 001.xhtml")
        );
        assert_eq!(
            toc[5].children[1].href.as_deref(),
            Some("chapter 027.xhtml")
        );
        assert_eq!(
            toc[5].children[2].href.as_deref(),
            Some("chapter053.xhtml#what:why")
        );

        let landmarks = process_nav(&content, None, "landmarks");
        assert_eq!(landmarks.len(), 2);
        assert_eq!(landmarks[0].title, "Begin Reading");
        assert_eq!(landmarks[0].href.as_deref(), Some("cover.xhtml#coverimage"));

        let page_list = process_nav(&content, None, "page-list");
        assert_eq!(page_list.len(), 8);
        assert_eq!(page_list[0].title, "Cover Page");
        assert_eq!(page_list[0].href.as_deref(), Some("xhtml/cover.xhtml"));
        assert_eq!(
            page_list[1].href.as_deref(),
            Some("xhtml/title.xhtml#pg_iii")
        );
        assert_eq!(page_list[7].title, "iv");
    }

    #[test]
    fn scan_kobo_spans_offsets() {
        let html = r#"<html><body><p><span class="koboSpan" id="kobo.1.1"></span>some text<span id="kobo.2.1" class="koboSpan other"></span></p></body></html>"#;
        let spans = scan_kobo_spans(html, html.len() as i64);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].0, "kobo.1.1");
        assert_eq!(spans[1].0, "kobo.2.1");
        assert!(spans[0].1 > 0.0 && spans[0].1 < spans[1].1 && spans[1].1 <= 1.0);
    }

    // endregion

    // region helpers

    #[test]
    fn href_normalization() {
        assert_eq!(
            normalize_href(Some("OEBPS"), "Text/ch1.xhtml"),
            "OEBPS/Text/ch1.xhtml"
        );
        assert_eq!(normalize_href(None, "Text/ch1.xhtml"), "Text/ch1.xhtml");
        assert_eq!(normalize_href(Some("OEBPS"), "../ch1.xhtml"), "ch1.xhtml");
        assert_eq!(
            normalize_href(Some("OEBPS"), "ch1.xhtml#frag"),
            "OEBPS/ch1.xhtml#frag"
        );
        assert_eq!(normalize_href(None, "ch1.xhtml#frag"), "ch1.xhtml#frag");
    }

    #[test]
    fn zip_path_normalization() {
        assert_eq!(normalize_zip_path("a/./b/../c"), "a/c");
        assert_eq!(normalize_zip_path("a//b"), "a/b");
        assert_eq!(normalize_zip_path("../a/b"), "../a/b");
        assert_eq!(normalize_zip_path("a/b/"), "a/b");
        assert_eq!(
            resolve_relative("OEBPS/page.xhtml", "img/p1.png"),
            "OEBPS/img/p1.png"
        );
        assert_eq!(
            resolve_relative("page.xhtml", "../img/p1.png"),
            "../img/p1.png"
        );
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("chapter%20027.xhtml"), "chapter 027.xhtml");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("%E3%83%9A"), "ペ");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn extension_gzip_json_roundtrip() {
        let analysis = analyzer().analyze(&fixtures().join("archives/epub3.epub"), false);
        let ext = analysis.epub_extension.as_ref().unwrap();
        let gz = encode_epub_extension_gz(ext).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(gz.as_slice());
        let mut json = String::new();
        std::io::Read::read_to_string(&mut decoder, &mut json).unwrap();
        assert!(json.contains("\"isFixedLayout\":true"));
        assert!(json.contains("\"pageList\":[]"));
        assert!(json.contains("\"koboSpan\":\"kobo.1.1\""));
        let decoded: MediaExtensionEpub = serde_json::from_str(&json).unwrap();
        assert_eq!(&decoded, ext);
    }

    // endregion
}
