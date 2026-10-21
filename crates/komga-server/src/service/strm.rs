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
const STRM_CACHE_IDLE_HOURS: i64 = 3;
const STRM_CACHE_MAX_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const STRM_CACHE_ROOT: &str = "/data/strm-cache";
const STRM_CACHE_TOUCH_INTERVAL_SECONDS: i64 = 60;

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

struct ResolvedCache<'a> {
    source_url: &'a str,
    strm_last_modified: time::OffsetDateTime,
    cached_at: time::OffsetDateTime,
    last_accessed_at: time::OffsetDateTime,
    path: &'a Path,
    file_name: &'a str,
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

fn book_cache_dir(state: &AppState, book_id: &str) -> PathBuf {
    cache_dir(state).join(book_id)
}

fn cache_is_fresh(last_accessed_at: time::OffsetDateTime, now: time::OffsetDateTime) -> bool {
    now - last_accessed_at < time::Duration::hours(STRM_CACHE_IDLE_HOURS)
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

fn remove_cache_file(state: &AppState, book_id: &str, path: &Path) {
    if !cache_path_is_within(&cache_dir(state), path) {
        tracing::warn!(
            book_id,
            path = %path.display(),
            "refusing to remove STRM cache outside the managed root"
        );
        return;
    }
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

fn cached_file_size(state: &AppState, source: &StrmSource) -> u64 {
    if !cache_path_is_within(&cache_dir(state), Path::new(&source.cache_path)) {
        return 0;
    }
    std::fs::metadata(&source.cache_path)
        .map(|metadata| metadata.len())
        .unwrap_or_else(|_| u64::try_from(source.file_size).unwrap_or(0))
}

fn enforce_cache_limit(
    state: &AppState,
    keep_book_id: Option<&str>,
    max_bytes: u64,
) -> Result<(), StrmError> {
    let dao = StrmSourceDao::new(state.kmrs_db.clone());
    let sources = dao
        .find_all_by_lru()?
        .into_iter()
        .filter(|source| {
            cache_path_is_within(&cache_dir(state), Path::new(&source.cache_path))
                && Path::new(&source.cache_path).is_file()
        })
        .collect::<Vec<_>>();
    let mut total_bytes = sources
        .iter()
        .map(|source| cached_file_size(state, source))
        .sum::<u64>();
    if total_bytes <= max_bytes {
        return Ok(());
    }

    for source in sources {
        if keep_book_id == Some(source.book_id.as_str()) {
            continue;
        }
        let removed_bytes = cached_file_size(state, &source);
        remove_cache_file(state, &source.book_id, Path::new(&source.cache_path));
        dao.delete(&source.book_id)?;
        total_bytes = total_bytes.saturating_sub(removed_bytes);
        if total_bytes <= max_bytes {
            break;
        }
    }
    Ok(())
}

fn resolved_cache_path(state: &AppState, book: &Book) -> Option<PathBuf> {
    if !book.is_strm() {
        return None;
    }
    let source = StrmSourceDao::new(state.kmrs_db.clone())
        .get(&book.id)
        .ok()??;
    let path = PathBuf::from(source.cache_path);
    (path.is_file()
        && cache_path_is_within(&cache_dir(state), &path)
        && cache_path_uses_file_name(&path, &source.file_name)
        && cache_is_fresh(source.last_accessed_at, time_codec::now_utc()))
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
    remove_cache_file(state, book_id, Path::new(&source.cache_path));
    Ok(())
}

pub fn cleanup_cache(state: &AppState) -> Result<usize, StrmError> {
    let dao = StrmSourceDao::new(state.kmrs_db.clone());
    let now = time_codec::now_utc();
    let mut removed = 0;
    for source in dao.find_all_by_lru()? {
        let path = Path::new(&source.cache_path);
        let expired = !cache_is_fresh(source.last_accessed_at, now);
        let invalid = !cache_path_is_within(&cache_dir(state), path) || !path.is_file();
        if expired || invalid {
            remove_cache_file(state, &source.book_id, path);
            dao.delete(&source.book_id)?;
            if expired {
                removed += 1;
            }
        }
    }
    enforce_cache_limit(state, None, STRM_CACHE_MAX_BYTES)?;
    Ok(removed)
}

async fn analyze_resolved(
    state: &AppState,
    book: &Book,
    cache: ResolvedCache<'_>,
) -> Result<StrmSource, StrmError> {
    let (actions, sources) = book::analyze_and_persist_at(state, book, cache.path)
        .map_err(|e| StrmError::Analysis(e.to_string()))?;
    let media = MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| StrmError::Analysis("book has no media row".to_string()))?;
    let media_type = media.media_type.clone();
    let file_size = std::fs::metadata(cache.path)?.len() as i64;
    StrmSourceDao::new(state.kmrs_db.clone()).upsert(
        &book.id,
        cache.source_url,
        cache.strm_last_modified,
        cache.cached_at,
        cache.last_accessed_at,
        &cache.path.to_string_lossy(),
        cache.file_name,
        media_type.as_deref(),
        file_size,
    )?;

    if actions.contains(&book::BookAction::GenerateThumbnail) {
        book::generate_thumbnail_and_persist_at(state, book, false, cache.path)
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

/// Resolve a `.strm` pointer on reader access. Cached content is reused while the pointer
/// URL and modification time remain unchanged and the cache was accessed within three hours.
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

    let source_dao = StrmSourceDao::new(state.kmrs_db.clone());
    let existing = source_dao.get(&book.id)?;
    if let Some(existing) = existing.as_ref() {
        if existing.source_url == source_url_string
            && time_codec::truncate_to_millis(existing.strm_last_modified)
                == time_codec::truncate_to_millis(strm_last_modified)
            && Path::new(&existing.cache_path).is_file()
            && cache_path_is_within(&cache_dir(state), Path::new(&existing.cache_path))
            && cache_path_uses_file_name(Path::new(&existing.cache_path), &existing.file_name)
            && cache_is_fresh(existing.last_accessed_at, time_codec::now_utc())
        {
            let media = MediaDao::new(state.db.clone()).find_by_id(&book.id)?;
            let last_accessed_at = time_codec::now_utc();
            if media
                .as_ref()
                .is_some_and(|media| media.status != MediaStatus::Ready)
            {
                return analyze_resolved(
                    state,
                    book,
                    ResolvedCache {
                        source_url: &existing.source_url,
                        strm_last_modified: existing.strm_last_modified,
                        cached_at: existing.cached_at,
                        last_accessed_at,
                        path: Path::new(&existing.cache_path),
                        file_name: &existing.file_name,
                    },
                )
                .await;
            }
            if last_accessed_at - existing.last_accessed_at
                >= time::Duration::seconds(STRM_CACHE_TOUCH_INTERVAL_SECONDS)
            {
                source_dao.touch(&book.id, last_accessed_at)?;
            } else {
                return Ok(existing.clone());
            }
            return Ok(StrmSource {
                last_accessed_at,
                ..existing.clone()
            });
        }
    }

    let book_cache_dir = book_cache_dir(state, &book.id);
    tokio::fs::create_dir_all(&book_cache_dir).await?;
    let partial = book_cache_dir.join(format!("{}.partial", uuid::Uuid::new_v4()));
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
    let final_path = book_cache_dir.join(&file_name);

    if final_path.exists() {
        tokio::fs::remove_file(&final_path).await?;
    }
    tokio::fs::rename(&partial, &final_path).await?;
    let cached_at = time_codec::now_utc();

    let resolved = analyze_resolved(
        state,
        book,
        ResolvedCache {
            source_url: &source_url_string,
            strm_last_modified,
            cached_at,
            last_accessed_at: cached_at,
            path: &final_path,
            file_name: &file_name,
        },
    )
    .await?;
    if let Some(previous) = existing {
        if previous.cache_path != resolved.cache_path {
            remove_cache_file(state, &previous.book_id, Path::new(&previous.cache_path));
        }
    }
    enforce_cache_limit(state, Some(&book.id), STRM_CACHE_MAX_BYTES)?;
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

    #[test]
    fn cache_expires_after_three_idle_hours() {
        let last_accessed_at = time_codec::parse_datetime_utc("2026-10-17 00:00:00").unwrap();
        assert!(cache_is_fresh(
            last_accessed_at,
            last_accessed_at + time::Duration::hours(2) + time::Duration::minutes(59)
        ));
        assert!(!cache_is_fresh(
            last_accessed_at,
            last_accessed_at + time::Duration::hours(3)
        ));
    }

    #[test]
    fn cache_paths_preserve_the_remote_file_name() {
        let url = Url::parse("https://example.test/library/My-Book.cbz").unwrap();
        let file_name = file_name_from_url(&url, detect::APPLICATION_ZIP, "fallback");
        assert_eq!(file_name, "My-Book.cbz");
        assert!(cache_path_uses_file_name(
            Path::new("/cache/b1/My-Book.cbz"),
            &file_name
        ));
        assert!(!cache_path_uses_file_name(
            Path::new("/cache/b1.cbz"),
            &file_name
        ));
    }

    #[test]
    fn production_cache_root_uses_the_data_mount() {
        assert_eq!(STRM_CACHE_ROOT, "/data/strm-cache");
        assert!(cache_path_is_within(
            Path::new(STRM_CACHE_ROOT),
            Path::new("/data/strm-cache/b1/book.cbz")
        ));
        assert!(!cache_path_is_within(
            Path::new(STRM_CACHE_ROOT),
            Path::new("/config/strm-cache/b1/book.cbz")
        ));
    }

    #[test]
    fn cache_limit_evicts_least_recently_used_entries() {
        let mut state = crate::service::series::tests::test_state();
        let config_dir = tempfile::tempdir().unwrap();
        state.config = std::sync::Arc::new(crate::config::ServerConfig {
            config_dir: config_dir.path().to_path_buf(),
            ..(*state.config).clone()
        });

        let now = time_codec::now_utc();
        let dao = StrmSourceDao::new(state.kmrs_db.clone());
        for (book_id, accessed_hours_ago) in [("b1", 3), ("b2", 2), ("b3", 1)] {
            let path = cache_dir(&state).join(book_id).join("book.cbz");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"123456").unwrap();
            dao.upsert(
                book_id,
                "https://example.test/book.cbz",
                now,
                now,
                now - time::Duration::hours(accessed_hours_ago),
                &path.to_string_lossy(),
                "book.cbz",
                Some(detect::APPLICATION_ZIP),
                6,
            )
            .unwrap();
        }

        enforce_cache_limit(&state, Some("b3"), 12).unwrap();

        assert!(dao.get("b1").unwrap().is_none());
        assert!(!cache_dir(&state).join("b1").exists());
        assert!(dao.get("b2").unwrap().is_some());
        assert!(dao.get("b3").unwrap().is_some());
    }

    #[test]
    fn cleanup_removes_idle_cache_entries() {
        let mut state = crate::service::series::tests::test_state();
        let config_dir = tempfile::tempdir().unwrap();
        state.config = std::sync::Arc::new(crate::config::ServerConfig {
            config_dir: config_dir.path().to_path_buf(),
            ..(*state.config).clone()
        });

        let now = time_codec::now_utc();
        let dao = StrmSourceDao::new(state.kmrs_db.clone());
        for (book_id, accessed_hours_ago) in [("b1", 4), ("b2", 2)] {
            let path = cache_dir(&state).join(book_id).join("book.cbz");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"123456").unwrap();
            dao.upsert(
                book_id,
                "https://example.test/book.cbz",
                now,
                now,
                now - time::Duration::hours(accessed_hours_ago),
                &path.to_string_lossy(),
                "book.cbz",
                Some(detect::APPLICATION_ZIP),
                6,
            )
            .unwrap();
        }

        assert_eq!(cleanup_cache(&state).unwrap(), 1);
        assert!(dao.get("b1").unwrap().is_none());
        assert!(!cache_dir(&state).join("b1").exists());
        assert!(dao.get("b2").unwrap().is_some());
    }
}
