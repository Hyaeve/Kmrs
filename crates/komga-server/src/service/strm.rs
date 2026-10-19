//! On-demand resolution for kmrs STRM books. The pointer file remains the main-library
//! `BOOK.URL`; downloaded content and its path live only in `kmrs.sqlite` and the cache dir.

use crate::error::ApiError;
use crate::service::{book, metadata};
use crate::state::AppState;
use komga_core::dto::url_to_file_path;
use komga_core::model::book::Book;
use komga_core::model::media::MediaStatus;
use komga_core::task::BookMetadataPatchCapability;
use komga_core::time_codec;
use komga_db::dao::media::MediaDao;
use komga_db::dao::strm_source::{StrmSource, StrmSourceDao};
use komga_media::detect;
use komga_media::image::ImageType;
use reqwest::{Client, Url};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use super::strm_stream::{self, StrmStreamError};

const MAX_STRM_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const STRM_CACHE_ROOT: &str = "/data/strm-cache";

#[derive(Debug, thiserror::Error)]
pub enum StrmError {
    #[error("book is not a STRM pointer")]
    NotStrm,
    #[error("STRM pointer is empty")]
    EmptyPointer,
    #[error("invalid STRM URL: {0}")]
    InvalidUrl(String),
    #[error("failed to read STRM pointer: {0}")]
    Io(#[from] std::io::Error),
    #[error("remote request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("remote server returned HTTP {0}")]
    RemoteStatus(u16),
    #[error("remote content exceeds the {MAX_STRM_BYTES} byte limit")]
    TooLarge,
    #[error("STRM Range streaming is not supported for '{0}'")]
    UnsupportedMedia(String),
    #[error(transparent)]
    Db(#[from] komga_db::Error),
    #[error("failed to analyze resolved STRM content: {0}")]
    Analysis(String),
    #[error(transparent)]
    Stream(#[from] StrmStreamError),
}

impl From<StrmError> for ApiError {
    fn from(error: StrmError) -> Self {
        match error {
            StrmError::NotStrm | StrmError::EmptyPointer | StrmError::InvalidUrl(_) => {
                Self::bad_request(error.to_string())
            }
            StrmError::Io(_)
            | StrmError::Http(_)
            | StrmError::RemoteStatus(_)
            | StrmError::Stream(_) => Self::bad_gateway(error.to_string()),
            StrmError::TooLarge | StrmError::UnsupportedMedia(_) => {
                Self::bad_gateway(error.to_string())
            }
            StrmError::Db(_) | StrmError::Analysis(_) => Self::Internal(error.to_string()),
        }
    }
}

fn http_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(format!("kmrs/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("valid reqwest client")
    })
}

fn cache_dir(state: &AppState) -> PathBuf {
    if cfg!(test) {
        state.config.config_dir.join("strm-cache")
    } else {
        PathBuf::from(STRM_CACHE_ROOT)
    }
}

fn source_path(book: &Book) -> PathBuf {
    PathBuf::from(url_to_file_path(&book.url))
}

fn read_source_url(content: &str) -> Result<Url, StrmError> {
    let value = content
        .lines()
        .map(str::trim)
        .map(|line| line.trim_start_matches('\u{feff}'))
        .find(|line| !line.is_empty())
        .ok_or(StrmError::EmptyPointer)?;
    let url = Url::parse(value).map_err(|e| StrmError::InvalidUrl(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(StrmError::InvalidUrl(format!(
            "unsupported scheme '{}'",
            url.scheme()
        )));
    }
    Ok(url)
}

fn sanitize_file_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .trim_end_matches([' ', '.'])
        .to_string()
}

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}

fn file_name_from_url(url: &Url, media_type: &str, fallback: &str) -> String {
    let candidate = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback);
    let fallback = sanitize_file_component(fallback);
    let fallback = if fallback.is_empty() {
        "strm-download".to_string()
    } else {
        fallback
    };
    let mut name = sanitize_file_component(candidate);
    if name.is_empty() || matches!(name.as_str(), "." | "..") {
        name = fallback;
    }
    if is_windows_reserved_name(&name) {
        name.insert(0, '_');
    }
    if Path::new(&name).extension().is_none() {
        let extension = detect::media_type_to_extension(media_type).unwrap_or(".bin");
        name.push_str(extension);
    }
    name
}

fn cache_path_uses_file_name(path: &Path, file_name: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == file_name)
}

fn cache_path_is_within(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

fn remove_cache_file(book_id: &str, path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                book_id,
                path = %path.display(),
                %error,
                "failed to remove resolved STRM cache file"
            );
        }
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

fn resolved_cache_path(state: &AppState, book: &Book) -> Option<PathBuf> {
    if !book.is_strm() {
        return None;
    }
    let source = StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)
        .ok()??;
    if source.cache_path.is_empty() {
        return None;
    }
    let path = PathBuf::from(source.cache_path);
    (path.is_file()
        && cache_path_is_within(&cache_dir(state), &path)
        && cache_path_uses_file_name(&path, &source.file_name))
    .then_some(path)
}

/// Path used by services that may run after resolution. Unresolved STRM books fall back to
/// their pointer path; automatic STRM tasks are filtered before they reach those services.
pub fn resolved_or_source_path(state: &AppState, book: &Book) -> PathBuf {
    resolved_cache_path(state, book).unwrap_or_else(|| source_path(book))
}

pub fn path_for_book(state: &AppState, book: &Book) -> Result<PathBuf, ApiError> {
    if !book.is_strm() {
        return Ok(source_path(book));
    }
    resolved_cache_path(state, book).ok_or_else(|| {
        ApiError::conflict("STRM book has not been resolved yet; open it from the reader first")
    })
}

pub fn delete_resolution(state: &AppState, book_id: &str) -> komga_db::Result<()> {
    let Some(source) = StrmSourceDao::new(state.kmrs_db.clone()).delete(book_id)? else {
        return Ok(());
    };
    remove_cache_file(book_id, Path::new(&source.cache_path));
    Ok(())
}

fn stream_media_type(book: &Book) -> Result<String, StrmError> {
    let pointer_name = source_path(book)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or(StrmError::NotStrm)?;
    let extension =
        komga_media::scanner::strm_entity_extension(&pointer_name).ok_or(StrmError::NotStrm)?;
    match extension.as_str() {
        "zip" | "cbz" => Ok(detect::APPLICATION_ZIP.to_string()),
        "epub" => Ok(detect::APPLICATION_EPUB.to_string()),
        "pdf" => Ok(detect::APPLICATION_PDF.to_string()),
        "cbr" | "rar" => Ok("application/x-rar-compressed".to_string()),
        other => Err(StrmError::UnsupportedMedia(other.to_string())),
    }
}

async fn generate_stream_thumbnail(
    state: &AppState,
    book: &Book,
    source: &StrmSource,
) -> Result<(), StrmError> {
    let media = MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| StrmError::Analysis("book has no media row".to_string()))?;
    let page_name = media.pages.first().map(|page| page.file_name.clone());
    let source = source.clone();
    let analyzer = book::analyzer_for(state);
    let max_edge = state.settings.get().thumbnail_size.max_edge();
    let generated = tokio::task::spawn_blocking(move || {
        if source.media_type.as_deref() == Some(detect::APPLICATION_EPUB) {
            return strm_stream::generate_epub_thumbnail(&source, &analyzer);
        }
        if source.media_type.as_deref() == Some(detect::APPLICATION_PDF) {
            let bytes = strm_stream::render_pdf_page(&source, 1)?;
            let bytes = komga_media::image::resize(&bytes, ImageType::Jpeg, max_edge)
                .map_err(|error| StrmStreamError::Media(error.to_string()))?;
            let (width, height) = komga_media::image::get_dimension(&bytes).unwrap_or((0, 0));
            return Ok(komga_media::analyzer::GeneratedThumbnail {
                file_size: bytes.len() as i64,
                bytes,
                media_type: detect::IMAGE_JPEG.to_string(),
                width: width as i32,
                height: height as i32,
            });
        }
        let entry_name =
            page_name.ok_or_else(|| StrmStreamError::Media("book has no first page".into()))?;
        let bytes = strm_stream::read_zip_entry(&source, &entry_name)?;
        let bytes = komga_media::image::resize(&bytes, ImageType::Jpeg, max_edge)
            .map_err(|error| StrmStreamError::Media(error.to_string()))?;
        let (width, height) = komga_media::image::get_dimension(&bytes).unwrap_or((0, 0));
        Ok(komga_media::analyzer::GeneratedThumbnail {
            file_size: bytes.len() as i64,
            bytes,
            media_type: detect::IMAGE_JPEG.to_string(),
            width: width as i32,
            height: height as i32,
        })
    })
    .await
    .map_err(|error| StrmError::Analysis(error.to_string()))??;
    book::persist_generated_thumbnail_bytes(state, book, generated.bytes);
    Ok(())
}

/// Resolve a `.strm` pointer on reader access. ZIP content remains remote; the service
/// probes byte-range support and later fetches only the blocks needed for analysis/pages.
async fn resolve_strm_inner(
    state: &AppState,
    book: &Book,
    force: bool,
) -> Result<StrmSource, StrmError> {
    if !book.is_strm() {
        return Err(StrmError::NotStrm);
    }
    let pointer = source_path(book);
    let pointer_meta = tokio::fs::metadata(&pointer).await?;
    let strm_last_modified: time::OffsetDateTime = pointer_meta
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        .into();
    let content = tokio::fs::read_to_string(&pointer).await?;
    let source_url = read_source_url(&content)?;
    let source_url_string = source_url.to_string();
    let media_type = stream_media_type(book)?;
    let source_dao = StrmSourceDao::new(state.kmrs_db.clone());
    if !force {
        if let Some(existing) = source_dao.get(&book.id)? {
            let media = MediaDao::new(state.db.clone()).find_by_id(&book.id)?;
            if existing.source_url == source_url_string
                && existing.cache_path.is_empty()
                && time_codec::truncate_to_millis(existing.strm_last_modified)
                    == time_codec::truncate_to_millis(strm_last_modified)
                && media.is_some_and(|media| {
                    media.status == MediaStatus::Ready
                        || (media.status == MediaStatus::Unsupported
                            && existing.media_type.as_deref()
                                == Some("application/x-rar-compressed"))
                })
            {
                let last_accessed_at = time_codec::now_utc();
                source_dao.touch(&book.id, last_accessed_at)?;
                return Ok(StrmSource {
                    last_accessed_at,
                    ..existing
                });
            }
        }
    }

    let probe = strm_stream::probe_remote(source_url).await?;
    if media_type != "application/x-rar-compressed" && !probe.range_supported {
        return Err(StrmStreamError::RangeUnsupported.into());
    }
    if probe.content_length > MAX_STRM_BYTES {
        return Err(StrmError::TooLarge);
    }
    let now = time_codec::now_utc();
    let file_name = file_name_from_url(&probe.resolved_url, &media_type, &book.name);
    let source = StrmSource {
        book_id: book.id.clone(),
        source_url: source_url_string,
        strm_last_modified,
        cache_path: String::new(),
        file_name,
        media_type: Some(media_type.clone()),
        file_size: i64::try_from(probe.content_length).unwrap_or(i64::MAX),
        created_date: now,
        last_modified_date: now,
        cached_at: now,
        last_accessed_at: now,
    };

    let analysis = if source.media_type.as_deref() == Some("application/x-rar-compressed") {
        let mut media = MediaDao::new(state.db.clone())
            .find_by_id(&book.id)?
            .ok_or_else(|| StrmError::Analysis("book has no media row".to_string()))?;
        media.status = MediaStatus::Unsupported;
        media.media_type = source.media_type.clone();
        media.comment = Some("ERR_1001".to_string());
        media.page_count = 0;
        media.pages.clear();
        media.files.clear();
        komga_media::analyzer::Analysis {
            media,
            epub_extension: None,
            kepub_file_size: None,
            metadata_sources: Default::default(),
        }
    } else {
        let analyzer = book::analyzer_for(state);
        let source_for_analysis = source.clone();
        tokio::task::spawn_blocking(move || {
            if source_for_analysis.media_type.as_deref() == Some(detect::APPLICATION_EPUB) {
                strm_stream::analyze_epub(&source_for_analysis, &analyzer)
            } else if source_for_analysis.media_type.as_deref() == Some(detect::APPLICATION_PDF) {
                strm_stream::analyze_pdf(&source_for_analysis, &analyzer)
            } else {
                strm_stream::analyze_zip(&source_for_analysis, &analyzer)
            }
        })
        .await
        .map_err(|error| StrmError::Analysis(error.to_string()))??
    };
    let (actions, metadata_sources) = book::persist_analysis(state, book, analysis)
        .map_err(|error| StrmError::Analysis(error.to_string()))?;

    let previous = source_dao.get(&book.id)?;
    source_dao.upsert(
        &source.book_id,
        &source.source_url,
        source.strm_last_modified,
        source.cached_at,
        source.last_accessed_at,
        "",
        &source.file_name,
        source.media_type.as_deref(),
        source.file_size,
    )?;

    if actions.contains(&book::BookAction::GenerateThumbnail) {
        generate_stream_thumbnail(state, book, &source).await?;
    }
    if actions.contains(&book::BookAction::RefreshMetadata) {
        metadata::refresh_book_metadata_with_sources(
            state,
            book,
            &BookMetadataPatchCapability::all(),
            Some(&metadata_sources),
        )
        .map_err(|error| StrmError::Analysis(error.to_string()))?;
        let _ = state
            .task_emitter
            .refresh_series_metadata(&book.series_id, komga_core::task::DEFAULT_PRIORITY);
    }
    if let Some(previous) = previous {
        if !previous.cache_path.is_empty() {
            remove_cache_file(&previous.book_id, Path::new(&previous.cache_path));
        }
    }

    source_dao
        .get(&book.id)?
        .ok_or_else(|| StrmError::Analysis("resolved source row disappeared".to_string()))
}

pub async fn resolve_strm(state: &AppState, book: &Book) -> Result<StrmSource, StrmError> {
    resolve_strm_inner(state, book, false).await
}

pub async fn analyze_strm(state: &AppState, book: &Book) -> Result<StrmSource, StrmError> {
    resolve_strm_inner(state, book, true).await
}

pub async fn remote_response(
    state: &AppState,
    book: &Book,
) -> Result<(reqwest::Response, StrmSource), StrmError> {
    remote_response_range(state, book, None).await
}

pub async fn remote_response_range(
    state: &AppState,
    book: &Book,
    range: Option<&str>,
) -> Result<(reqwest::Response, StrmSource), StrmError> {
    let source = StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)?
        .ok_or(StrmError::NotStrm)?;
    let mut request = http_client().get(source.source_url.clone());
    if let Some(range) = range {
        request = request.header(reqwest::header::RANGE, range);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(StrmError::RemoteStatus(response.status().as_u16()));
    }
    Ok((response, source))
}

pub async fn download_bytes(state: &AppState, book: &Book) -> Result<Vec<u8>, StrmError> {
    let (response, _) = remote_response(state, book).await?;
    Ok(response.bytes().await?.to_vec())
}

pub async fn extract_page_content(
    state: &AppState,
    book: &Book,
    media: &komga_core::model::media::Media,
    page_number: usize,
    convert_to: Option<ImageType>,
    resize_to: Option<u32>,
) -> Result<komga_media::container::PageContent, StrmError> {
    if media.status != MediaStatus::Ready
        || page_number == 0
        || page_number > media.page_count as usize
    {
        return Err(StrmError::Analysis("STRM page is not ready".to_string()));
    }
    let source = StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)?
        .ok_or(StrmError::NotStrm)?;
    if !source.cache_path.is_empty() {
        return Err(StrmError::Analysis(
            "legacy STRM cache path cannot be used by the streaming page reader".to_string(),
        ));
    }
    let entry_name = media.pages[page_number - 1].file_name.clone();
    let media_type = media.pages[page_number - 1].media_type.clone();
    let book_name = book.name.clone();
    tokio::task::spawn_blocking(move || {
        let bytes = strm_stream::read_zip_entry(&source, &entry_name)
            .map_err(|error| StrmError::Analysis(error.to_string()))?;
        komga_media::container::process_page_content(
            &book_name,
            page_number,
            bytes,
            &media_type,
            convert_to,
            resize_to,
        )
        .map_err(|error| StrmError::Analysis(error.to_string()))
    })
    .await
    .map_err(|error| StrmError::Analysis(error.to_string()))?
}

pub async fn extract_pdf_page_content(
    state: &AppState,
    book: &Book,
    page_number: usize,
    convert_to: Option<ImageType>,
    resize_to: Option<u32>,
) -> Result<komga_media::container::PageContent, StrmError> {
    let source = StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)?
        .ok_or(StrmError::NotStrm)?;
    let book_name = book.name.clone();
    tokio::task::spawn_blocking(move || {
        let bytes = strm_stream::render_pdf_page(&source, page_number)
            .map_err(|error| StrmError::Analysis(error.to_string()))?;
        komga_media::container::process_page_content(
            &book_name,
            page_number,
            bytes,
            detect::IMAGE_JPEG,
            convert_to,
            resize_to,
        )
        .map_err(|error| StrmError::Analysis(error.to_string()))
    })
    .await
    .map_err(|error| StrmError::Analysis(error.to_string()))?
}

pub async fn extract_raw_pdf_page(
    state: &AppState,
    book: &Book,
    page_number: usize,
) -> Result<komga_media::container::PageContent, StrmError> {
    let source = StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)?
        .ok_or(StrmError::NotStrm)?;
    let bytes =
        tokio::task::spawn_blocking(move || strm_stream::extract_pdf_page(&source, page_number))
            .await
            .map_err(|error| StrmError::Analysis(error.to_string()))??;
    Ok(komga_media::container::PageContent {
        bytes,
        media_type: detect::APPLICATION_PDF.to_string(),
    })
}

pub async fn extract_epub_resource(
    state: &AppState,
    book: &Book,
    resource_name: &str,
) -> Result<Vec<u8>, StrmError> {
    let source = StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)?
        .ok_or(StrmError::NotStrm)?;
    let resource_name = resource_name.to_string();
    tokio::task::spawn_blocking(move || strm_stream::read_zip_entry(&source, &resource_name))
        .await
        .map_err(|error| StrmError::Analysis(error.to_string()))?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_non_empty_http_url() {
        let url = read_source_url("\n  https://example.test/book.cbz  \nignored").unwrap();
        assert_eq!(url.as_str(), "https://example.test/book.cbz");
        assert!(matches!(
            read_source_url("file:///tmp/book.cbz"),
            Err(StrmError::InvalidUrl(_))
        ));
        assert!(matches!(
            read_source_url("\n \n"),
            Err(StrmError::EmptyPointer)
        ));
    }

    #[test]
    fn remote_file_names_are_preserved() {
        let url = Url::parse("https://example.test/library/My-Book.cbz").unwrap();
        let file_name = file_name_from_url(&url, detect::APPLICATION_ZIP, "fallback");
        assert_eq!(file_name, "My-Book.cbz");
    }

    #[test]
    fn streaming_maps_supported_pointer_extensions() {
        let mut book = crate::service::series::tests::sample_book("s1", "l1", "book");
        book.url = "file:/library/book.cbz.strm".into();
        assert_eq!(stream_media_type(&book).unwrap(), detect::APPLICATION_ZIP);

        book.url = "file:/library/book.epub.strm".into();
        assert_eq!(stream_media_type(&book).unwrap(), detect::APPLICATION_EPUB);

        book.url = "file:/library/book.pdf.strm".into();
        assert_eq!(stream_media_type(&book).unwrap(), detect::APPLICATION_PDF);

        book.url = "file:/library/book.cbr.strm".into();
        assert_eq!(
            stream_media_type(&book).unwrap(),
            "application/x-rar-compressed"
        );
    }
}
