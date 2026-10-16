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
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Url};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_STRM_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const PREFIX_BYTES: usize = 8192;

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
    #[error(transparent)]
    Db(#[from] komga_db::Error),
    #[error("failed to analyze resolved STRM content: {0}")]
    Analysis(String),
}

impl From<StrmError> for ApiError {
    fn from(error: StrmError) -> Self {
        match error {
            StrmError::NotStrm | StrmError::EmptyPointer | StrmError::InvalidUrl(_) => {
                Self::bad_request(error.to_string())
            }
            StrmError::Io(_) | StrmError::Http(_) | StrmError::RemoteStatus(_) => {
                Self::bad_gateway(error.to_string())
            }
            StrmError::TooLarge => Self::bad_gateway(error.to_string()),
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
    state.config.config_dir.join("strm-cache")
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

fn file_name_from_url(url: &Url, media_type: &str, fallback: &str) -> String {
    let name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback)
        .replace(['/', '\\'], "_");
    if Path::new(&name).extension().is_some() {
        return name;
    }
    let extension = detect::media_type_to_extension(media_type).unwrap_or(".bin");
    format!("{name}{extension}")
}

fn resolved_cache_path(state: &AppState, book: &Book) -> Option<PathBuf> {
    if !book.is_strm() {
        return None;
    }
    let source = StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)
        .ok()??;
    let path = PathBuf::from(source.cache_path);
    path.is_file().then_some(path)
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
    if let Err(error) = std::fs::remove_file(&source.cache_path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                book_id,
                path = %source.cache_path,
                %error,
                "failed to remove resolved STRM cache file"
            );
        }
    }
    Ok(())
}

async fn analyze_resolved(
    state: &AppState,
    book: &Book,
    source_url: &str,
    strm_last_modified: time::OffsetDateTime,
    path: &Path,
    file_name: &str,
) -> Result<StrmSource, StrmError> {
    let (actions, sources) = book::analyze_and_persist_at(state, book, path)
        .map_err(|e| StrmError::Analysis(e.to_string()))?;
    let media = MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| StrmError::Analysis("book has no media row".to_string()))?;
    let media_type = media.media_type.clone();
    let file_size = std::fs::metadata(path)?.len() as i64;
    StrmSourceDao::new(state.kmrs_db.clone()).upsert(
        &book.id,
        source_url,
        strm_last_modified,
        &path.to_string_lossy(),
        file_name,
        media_type.as_deref(),
        file_size,
    )?;

    if actions.contains(&book::BookAction::GenerateThumbnail) {
        book::generate_thumbnail_and_persist_at(state, book, false, path)
            .map_err(|e| StrmError::Analysis(e.to_string()))?;
    }
    if actions.contains(&book::BookAction::RefreshMetadata) {
        metadata::refresh_book_metadata_with_sources(
            state,
            book,
            &BookMetadataPatchCapability::all(),
            Some(&sources),
        )
        .map_err(|e| StrmError::Analysis(e.to_string()))?;
        let _ = state
            .task_emitter
            .refresh_series_metadata(&book.series_id, komga_core::task::DEFAULT_PRIORITY);
    }

    StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)?
        .ok_or_else(|| StrmError::Analysis("resolved source row disappeared".to_string()))
}

/// Resolve a `.strm` pointer on first reader access. Cached content is reused until the
/// pointer URL or modification time changes.
pub async fn resolve_strm(state: &AppState, book: &Book) -> Result<StrmSource, StrmError> {
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

    let existing = StrmSourceDao::new(state.kmrs_db.clone()).get(&book.id)?;
    if let Some(existing) = existing.as_ref() {
        if existing.source_url == source_url_string
            && time_codec::truncate_to_millis(existing.strm_last_modified)
                == time_codec::truncate_to_millis(strm_last_modified)
            && Path::new(&existing.cache_path).is_file()
        {
            let media = MediaDao::new(state.db.clone()).find_by_id(&book.id)?;
            if media
                .as_ref()
                .is_some_and(|media| media.status != MediaStatus::Ready)
            {
                return analyze_resolved(
                    state,
                    book,
                    &existing.source_url,
                    existing.strm_last_modified,
                    Path::new(&existing.cache_path),
                    &existing.file_name,
                )
                .await;
            }
            return Ok(existing.clone());
        }
    }

    tokio::fs::create_dir_all(cache_dir(state)).await?;
    let partial = cache_dir(state).join(format!("{}.{}.partial", book.id, uuid::Uuid::new_v4()));
    let response = http_client().get(source_url.clone()).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(StrmError::RemoteStatus(status.as_u16()));
    }
    let final_url = response.url().clone();
    let header_media_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
        .filter(|value| !value.is_empty());

    let mut file = tokio::fs::File::create(&partial).await?;
    let mut response = response;
    let mut written = 0u64;
    while let Some(chunk) = response.chunk().await? {
        written = written.saturating_add(chunk.len() as u64);
        if written > MAX_STRM_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(StrmError::TooLarge);
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    let mut prefix = vec![0u8; PREFIX_BYTES.min(written as usize)];
    if !prefix.is_empty() {
        let mut prefix_file = tokio::fs::File::open(&partial).await?;
        prefix_file.read_exact(&mut prefix).await?;
    }
    let detected_media_type = detect::detect_media_type(&prefix);
    let media_type = header_media_type
        .filter(|value| value != detect::APPLICATION_OCTET_STREAM)
        .unwrap_or(detected_media_type);
    let file_name = file_name_from_url(&final_url, &media_type, &book.name);
    let extension = Path::new(&file_name)
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_else(|| ".bin".to_string());
    let final_path = cache_dir(state).join(format!("{}{extension}", book.id));

    if final_path.exists() {
        tokio::fs::remove_file(&final_path).await?;
    }
    tokio::fs::rename(&partial, &final_path).await?;

    let resolved = analyze_resolved(
        state,
        book,
        &source_url_string,
        strm_last_modified,
        &final_path,
        &file_name,
    )
    .await?;
    if let Some(previous) = existing {
        if previous.cache_path != resolved.cache_path {
            let _ = tokio::fs::remove_file(previous.cache_path).await;
        }
    }
    Ok(resolved)
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
}
