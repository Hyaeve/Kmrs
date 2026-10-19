//! Writes kmrs-managed metadata and artwork back into STRM library folders.

use crate::service::{book, series};
use crate::state::AppState;
use komga_core::dto::url_to_file_path;
use komga_core::model::book::{Book, BookMetadata};
use komga_core::model::series::{Series, SeriesMetadata, SeriesStatus};
use komga_db::dao::book::{BookDao, BookMetadataDao};
use komga_db::dao::library::LibraryDao;
use komga_db::dao::library_setting::LibrarySettingDao;
use komga_db::dao::series::{SeriesDao, SeriesMetadataDao};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const SERIES_JSON: &str = "series.json";
const COMICINFO_SUFFIX: &str = ".ComicInfo.xml";
const SERIES_COVER_STEMS: [&str; 5] = ["cover", "default", "folder", "poster", "series"];
const IMAGE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "webp", "gif", "tbn"];

#[derive(Debug, thiserror::Error)]
pub enum StrmMetadataError {
    #[error(transparent)]
    Db(#[from] komga_db::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("failed to serialize ComicInfo sidecar: {0}")]
    ComicInfo(String),
    #[error("library, series or book not found")]
    NotFound,
    #[error("resolved sidecar path escapes the library root")]
    PathEscape,
}

type Result<T> = std::result::Result<T, StrmMetadataError>;

fn write_lock() -> &'static Mutex<()> {
    static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

fn metadata_write_enabled(state: &AppState, library_id: &str) -> Result<bool> {
    let settings = LibrarySettingDao::new(state.kmrs_db.clone()).get(library_id)?;
    Ok(settings.write_metadata)
}

fn library_root(state: &AppState, library_id: &str) -> Result<PathBuf> {
    let library = LibraryDao::new(state.db.clone())
        .find_by_id(library_id)?
        .ok_or(StrmMetadataError::NotFound)?;
    Ok(PathBuf::from(url_to_file_path(&library.root)))
}

fn ensure_contained(root: &Path, path: &Path) -> Result<()> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(StrmMetadataError::PathEscape)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(StrmMetadataError::PathEscape);
    };
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("metadata");
    let temp = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temp, bytes)?;
    if let Err(error) = std::fs::rename(&temp, path) {
        if matches!(
            error.kind(),
            ErrorKind::AlreadyExists | ErrorKind::PermissionDenied
        ) {
            let _ = std::fs::remove_file(path);
            std::fs::rename(&temp, path)?;
        } else {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
    }
    Ok(())
}

fn series_directory(series: &Series, books: &[Book]) -> Result<PathBuf> {
    if series.oneshot {
        let book = books.first().ok_or(StrmMetadataError::NotFound)?;
        let pointer = PathBuf::from(url_to_file_path(&book.url));
        return pointer
            .parent()
            .map(Path::to_path_buf)
            .ok_or(StrmMetadataError::NotFound);
    }
    Ok(PathBuf::from(url_to_file_path(&series.url)))
}

fn existing_series_cover(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                return false;
            };
            let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
                return false;
            };
            SERIES_COVER_STEMS
                .iter()
                .any(|candidate| stem.eq_ignore_ascii_case(candidate))
                && IMAGE_EXTENSIONS
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn media_extension(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "jpg",
    }
}

fn age_rating(age: Option<i32>) -> &'static str {
    match age.unwrap_or(0) {
        i32::MIN..=8 => "All",
        9..=11 => "9+",
        12..=14 => "12+",
        15..=16 => "15+",
        17 => "17+",
        _ => "Adult",
    }
}

fn preserved_value(object: &Map<String, Value>, key: &str, fallback: Value) -> Value {
    object.get(key).cloned().unwrap_or(fallback)
}

fn merge_series_json(
    existing: Option<&[u8]>,
    metadata: &SeriesMetadata,
    book_count: i32,
) -> Result<Vec<u8>> {
    let mut root = match existing {
        Some(bytes) if !bytes.is_empty() => {
            serde_json::from_slice::<Value>(bytes).unwrap_or_else(|_| Value::Object(Map::new()))
        }
        _ => Value::Object(Map::new()),
    };
    if !root.is_object() {
        root = Value::Object(Map::new());
    }
    let root_object = root.as_object_mut().expect("checked object");
    let metadata_value = root_object
        .entry("metadata")
        .or_insert_with(|| Value::Object(Map::new()));
    if !metadata_value.is_object() {
        *metadata_value = Value::Object(Map::new());
    }
    let object = metadata_value.as_object_mut().expect("checked object");

    object.insert(
        "type".into(),
        preserved_value(object, "type", json!("comic")),
    );
    object.insert(
        "publisher".into(),
        Value::String(metadata.publisher.clone()),
    );
    object.insert("name".into(), Value::String(metadata.title.clone()));
    object.insert(
        "comicid".into(),
        preserved_value(object, "comicid", json!("")),
    );
    object.insert(
        "year".into(),
        preserved_value(
            object,
            "year",
            json!(time::OffsetDateTime::now_utc().year()),
        ),
    );
    object.insert(
        "description_formatted".into(),
        Value::String(metadata.summary.clone()),
    );
    object.insert(
        "booktype".into(),
        preserved_value(object, "booktype", json!("Print")),
    );
    object.insert(
        "age_rating".into(),
        Value::String(age_rating(metadata.age_rating).to_string()),
    );
    object.insert(
        "comic_image".into(),
        preserved_value(object, "comic_image", json!("cover.jpg")),
    );
    object.insert(
        "total_issues".into(),
        json!(metadata.total_book_count.unwrap_or(book_count)),
    );
    object.insert(
        "publication_run".into(),
        preserved_value(object, "publication_run", json!("")),
    );
    object.insert(
        "status".into(),
        Value::String(
            if metadata.status == SeriesStatus::Ended {
                "Ended"
            } else {
                "Continuing"
            }
            .to_string(),
        ),
    );
    Ok(serde_json::to_vec_pretty(&root)?)
}

#[derive(Serialize)]
#[serde(rename = "ComicInfo")]
struct ComicInfoSidecar<'a> {
    #[serde(rename = "Title")]
    title: &'a str,
    #[serde(rename = "Series")]
    series: &'a str,
    #[serde(rename = "Number")]
    number: &'a str,
    #[serde(rename = "Summary", skip_serializing_if = "str::is_empty")]
    summary: &'a str,
    #[serde(rename = "Year", skip_serializing_if = "Option::is_none")]
    year: Option<i32>,
    #[serde(rename = "Month", skip_serializing_if = "Option::is_none")]
    month: Option<i8>,
    #[serde(rename = "Day", skip_serializing_if = "Option::is_none")]
    day: Option<u8>,
    #[serde(rename = "Writer", skip_serializing_if = "str::is_empty")]
    writer: &'a str,
    #[serde(rename = "Publisher", skip_serializing_if = "str::is_empty")]
    publisher: &'a str,
    #[serde(rename = "Tags", skip_serializing_if = "str::is_empty")]
    tags: &'a str,
    #[serde(rename = "ISBN", skip_serializing_if = "str::is_empty")]
    isbn: &'a str,
}

fn comicinfo_xml(book: &BookMetadata, series: &SeriesMetadata) -> Result<Vec<u8>> {
    let writers = book
        .authors
        .iter()
        .map(|author| author.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let tags = book.tags.join(", ");
    let sidecar = ComicInfoSidecar {
        title: &book.title,
        series: &series.title,
        number: &book.number,
        summary: &book.summary,
        year: book.release_date.map(|date| date.year()),
        month: book.release_date.map(|date| date.month() as i8),
        day: book.release_date.map(|date| date.day()),
        writer: &writers,
        publisher: &series.publisher,
        tags: &tags,
        isbn: &book.isbn,
    };
    let xml = quick_xml::se::to_string(&sidecar)
        .map_err(|error| StrmMetadataError::ComicInfo(error.to_string()))?;
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}\n").into_bytes())
}

fn comicinfo_sidecar_path(book: &Book) -> Result<PathBuf> {
    let pointer = PathBuf::from(url_to_file_path(&book.url));
    let file_name = pointer
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(StrmMetadataError::NotFound)?;
    pointer
        .parent()
        .map(|parent| parent.join(format!("{file_name}{COMICINFO_SUFFIX}")))
        .ok_or(StrmMetadataError::NotFound)
}

pub fn read_external_comicinfo(book: &Book) -> Result<Option<Vec<u8>>> {
    let path = comicinfo_sidecar_path(book)?;
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_series_cover(state: &AppState, series_id: &str, dir: &Path, root: &Path) -> Result<()> {
    let Some(bytes) = series::get_thumbnail_bytes(state, series_id, "")? else {
        return Ok(());
    };
    let media_type = komga_media::detect::detect_media_type(&bytes);
    let path = existing_series_cover(dir)
        .unwrap_or_else(|| dir.join(format!("cover.{}", media_extension(&media_type))));
    ensure_contained(root, &path)?;
    atomic_write(&path, &bytes)
}

fn write_book_cover(state: &AppState, book: &Book, root: &Path) -> Result<()> {
    let Some(content) = book::get_thumbnail_bytes(state, &book.id, None)? else {
        return Ok(());
    };
    let pointer = PathBuf::from(url_to_file_path(&book.url));
    let parent = pointer.parent().ok_or(StrmMetadataError::NotFound)?;
    let extension = media_extension(&content.media_type);
    let path = parent.join(format!("{}.{}", book.name, extension));
    ensure_contained(root, &path)?;
    atomic_write(&path, &content.bytes)
}

pub fn write_series_metadata(state: &AppState, series_id: &str) -> Result<()> {
    let _guard = write_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    write_series_metadata_inner(state, series_id)
}

fn write_series_metadata_inner(state: &AppState, series_id: &str) -> Result<()> {
    let series = SeriesDao::new(state.db.clone())
        .find_by_id(series_id)?
        .ok_or(StrmMetadataError::NotFound)?;
    if !metadata_write_enabled(state, &series.library_id)? {
        return Ok(());
    }
    let books = BookDao::new(state.db.clone()).find_by_series_id(series_id)?;
    if !books.iter().any(Book::is_strm) {
        return Ok(());
    }
    let metadata = SeriesMetadataDao::new(state.db.clone())
        .find_by_id(series_id)?
        .ok_or(StrmMetadataError::NotFound)?;
    let root = library_root(state, &series.library_id)?;
    let dir = series_directory(&series, &books)?;
    ensure_contained(&root, &dir)?;

    if series.oneshot {
        for book in books.iter().filter(|book| book.is_strm()) {
            write_book_metadata_inner(state, &book.id)?;
        }
        return Ok(());
    }

    let series_json = dir.join(SERIES_JSON);
    ensure_contained(&root, &series_json)?;
    let existing = std::fs::read(&series_json).ok();
    let bytes = merge_series_json(existing.as_deref(), &metadata, books.len() as i32)?;
    atomic_write(&series_json, &bytes)?;
    write_series_cover(state, series_id, &dir, &root)
}

pub fn write_book_metadata(state: &AppState, book_id: &str) -> Result<()> {
    let _guard = write_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    write_book_metadata_inner(state, book_id)
}

fn write_book_metadata_inner(state: &AppState, book_id: &str) -> Result<()> {
    let book = BookDao::new(state.db.clone())
        .find_by_id(book_id)?
        .ok_or(StrmMetadataError::NotFound)?;
    if !book.is_strm() || !metadata_write_enabled(state, &book.library_id)? {
        return Ok(());
    }
    let metadata = BookMetadataDao::new(state.db.clone())
        .find_by_id(book_id)?
        .ok_or(StrmMetadataError::NotFound)?;
    let series_metadata = SeriesMetadataDao::new(state.db.clone())
        .find_by_id(&book.series_id)?
        .ok_or(StrmMetadataError::NotFound)?;
    let root = library_root(state, &book.library_id)?;
    let sidecar = comicinfo_sidecar_path(&book)?;
    ensure_contained(&root, &sidecar)?;
    atomic_write(&sidecar, &comicinfo_xml(&metadata, &series_metadata)?)?;
    write_book_cover(state, &book, &root)
}

pub fn schedule_series_write(state: &AppState, series_id: &str) {
    let enabled = SeriesDao::new(state.db.clone())
        .find_by_id(series_id)
        .ok()
        .flatten()
        .is_some_and(|series| metadata_write_enabled(state, &series.library_id).unwrap_or(false));
    if !enabled {
        return;
    }
    let state = state.clone();
    let series_id = series_id.to_string();
    tokio::spawn(async move {
        match tokio::task::spawn_blocking(move || write_series_metadata(&state, &series_id)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!("failed to write STRM series metadata: {error}"),
            Err(error) => tracing::warn!("STRM series metadata task failed: {error}"),
        }
    });
}

pub fn schedule_book_write(state: &AppState, book_id: &str) {
    let enabled = BookDao::new(state.db.clone())
        .find_by_id(book_id)
        .ok()
        .flatten()
        .filter(Book::is_strm)
        .is_some_and(|book| metadata_write_enabled(state, &book.library_id).unwrap_or(false));
    if !enabled {
        return;
    }
    let state = state.clone();
    let book_id = book_id.to_string();
    tokio::spawn(async move {
        match tokio::task::spawn_blocking(move || write_book_metadata(&state, &book_id)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!("failed to write STRM book metadata: {error}"),
            Err(error) => tracing::warn!("STRM book metadata task failed: {error}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::common::Author;
    use komga_core::model::series::{ReadingDirection, SeriesMetadata};
    use komga_core::model::thumbnail::{Dimension, ThumbnailBook, ThumbnailSeries, ThumbnailType};
    use komga_core::task::BookMetadataPatchCapability;
    use komga_core::time_codec;
    use komga_db::dao::book::BookMetadataDao;
    use komga_db::dao::thumbnail::{ThumbnailBookDao, ThumbnailSeriesDao};
    use std::collections::BTreeSet;
    use std::time::Duration;

    fn series_metadata() -> SeriesMetadata {
        SeriesMetadata {
            series_id: "s1".into(),
            status: SeriesStatus::Ongoing,
            title: "Example".into(),
            title_sort: "Example".into(),
            summary: "Summary".into(),
            reading_direction: Some(ReadingDirection::LeftToRight),
            publisher: "Publisher".into(),
            age_rating: Some(12),
            language: "en".into(),
            genres: BTreeSet::new(),
            tags: BTreeSet::new(),
            total_book_count: Some(4),
            sharing_labels: BTreeSet::new(),
            links: vec![],
            alternate_titles: vec![],
            status_lock: false,
            title_lock: false,
            title_sort_lock: false,
            summary_lock: false,
            reading_direction_lock: false,
            publisher_lock: false,
            age_rating_lock: false,
            language_lock: false,
            genres_lock: false,
            tags_lock: false,
            total_book_count_lock: false,
            sharing_labels_lock: false,
            links_lock: false,
            alternate_titles_lock: false,
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        }
    }

    fn book_metadata() -> BookMetadata {
        BookMetadata {
            book_id: "b1".into(),
            title: "Book Title".into(),
            summary: "Book summary".into(),
            number: "2".into(),
            number_sort: 2.0,
            release_date: Some(
                time::Date::from_calendar_date(2024, time::Month::March, 8).unwrap(),
            ),
            authors: vec![Author {
                name: "Writer One".into(),
                role: "writer".into(),
            }],
            notable_entities: vec![],
            tags: vec!["Action".into(), "Adventure".into()],
            isbn: "9780000000001".into(),
            links: vec![],
            title_lock: false,
            summary_lock: false,
            number_lock: false,
            number_sort_lock: false,
            release_date_lock: false,
            authors_lock: false,
            notable_entities_lock: false,
            tags_lock: false,
            isbn_lock: false,
            links_lock: false,
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        }
    }

    #[test]
    fn series_json_merges_managed_fields_and_preserves_unknown() {
        let existing = br#"{"metadata":{"comicid":"42","booktype":"Digital","custom":"keep"}}"#;
        let bytes = merge_series_json(Some(existing), &series_metadata(), 3).unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["metadata"]["name"], "Example");
        assert_eq!(json["metadata"]["publisher"], "Publisher");
        assert_eq!(json["metadata"]["comicid"], "42");
        assert_eq!(json["metadata"]["booktype"], "Digital");
        assert_eq!(json["metadata"]["custom"], "keep");
        assert_eq!(json["metadata"]["total_issues"], 4);
        assert_eq!(json["metadata"]["age_rating"], "12+");
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("series.json");
        std::fs::write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"new");
    }

    #[test]
    fn path_containment_rejects_escape() {
        let root = Path::new("/library");
        assert!(ensure_contained(root, Path::new("/library/series/cover.jpg")).is_ok());
        assert!(ensure_contained(root, Path::new("/other/cover.jpg")).is_err());
    }

    #[test]
    fn comicinfo_contains_book_and_series_fields() {
        let xml = String::from_utf8(comicinfo_xml(&book_metadata(), &series_metadata()).unwrap())
            .unwrap();
        assert!(xml.contains("<ComicInfo>"));
        assert!(xml.contains("<Title>Book Title</Title>"));
        assert!(xml.contains("<Series>Example</Series>"));
        assert!(xml.contains("<Writer>Writer One</Writer>"));
        assert!(xml.contains("<Publisher>Publisher</Publisher>"));
        assert!(xml.contains("<Tags>Action, Adventure</Tags>"));
    }

    #[tokio::test]
    async fn sidecar_write_follows_metadata_update_without_waiting_for_background_tasks() {
        let state = crate::service::series::tests::test_state();
        let root = tempfile::tempdir().unwrap();
        let series_dir = root.path().join("Example");
        std::fs::create_dir_all(&series_dir).unwrap();
        let pointer = series_dir.join("Example 1.pdf.strm");
        std::fs::write(&pointer, b"https://example.test/book.pdf\n").unwrap();
        let library_url = format!(
            "{}/",
            komga_media::scanner::path_to_url(root.path()).trim_end_matches('/')
        );
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES ('l1', 'Library', ?)",
                [&library_url],
            )
            .unwrap();
        LibrarySettingDao::new(state.kmrs_db.clone())
            .set(
                "l1",
                komga_db::dao::library_setting::LibrarySettings {
                    scan_strm: false,
                    real_time_scan: false,
                    write_metadata: true,
                },
            )
            .unwrap();

        let db = state.db.rw().unwrap();
        db.execute(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES ('s1', 'Example', ?, '2020-01-01 00:00:00.0', 'l1', '2020-01-01 00:00:00.0', '2020-01-01 00:00:00.0')",
            [komga_media::scanner::path_to_url(&series_dir)],
        )
        .unwrap();
        db.execute(
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT) \
             VALUES ('s1', 'ONGOING', 'Example', 'Example')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO BOOK_METADATA_AGGREGATION (SERIES_ID) VALUES ('s1')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
             VALUES ('b1', 'Example 1', ?, '2020-01-01 00:00:00.0', 's1', 'l1')",
            [komga_media::scanner::path_to_url(&pointer)],
        )
        .unwrap();
        db.execute(
            "INSERT INTO BOOK_METADATA (BOOK_ID, NUMBER, NUMBER_SORT, TITLE) \
             VALUES ('b1', '1', 1, 'Example 1')",
            [],
        )
        .unwrap();
        drop(db);

        let tasks = komga_db::dao::tasks::TasksDao::new(state.tasks_db.clone());
        let pending = komga_core::task::Task::scan_library(
            "unrelated-library",
            false,
            komga_core::task::DEFAULT_PRIORITY,
        );
        tasks.save(&pending).unwrap();
        schedule_book_write(&state, "b1");

        let sidecar = series_dir.join("Example 1.pdf.strm.ComicInfo.xml");
        let written = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if sidecar.is_file() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        assert!(
            written.is_ok(),
            "metadata sidecar write should follow the metadata update without a second queue wait"
        );
        tasks.delete(&pending.unique_id()).unwrap();
    }

    #[test]
    fn writes_series_and_book_sidecars_into_library() {
        let state = crate::service::series::tests::test_state();
        let root = tempfile::tempdir().unwrap();
        let series_dir = root.path().join("Example");
        std::fs::create_dir_all(&series_dir).unwrap();
        let pointer = series_dir.join("Example 1.pdf.strm");
        std::fs::write(&pointer, b"https://example.test/book.pdf\n").unwrap();
        std::fs::write(
            series_dir.join(SERIES_JSON),
            br#"{"metadata":{"custom":"keep","comicid":"42"}}"#,
        )
        .unwrap();

        let library_url = format!(
            "{}/",
            komga_media::scanner::path_to_url(root.path()).trim_end_matches('/')
        );
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES ('l1', 'Library', ?)",
                [&library_url],
            )
            .unwrap();
        LibrarySettingDao::new(state.kmrs_db.clone())
            .set(
                "l1",
                komga_db::dao::library_setting::LibrarySettings {
                    scan_strm: false,
                    real_time_scan: false,
                    write_metadata: true,
                },
            )
            .unwrap();

        let db = state.db.rw().unwrap();
        db.execute(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, CREATED_DATE, LAST_MODIFIED_DATE) \
             VALUES ('s1', 'Example', ?, '2020-01-01 00:00:00.0', 'l1', '2020-01-01 00:00:00.0', '2020-01-01 00:00:00.0')",
            [komga_media::scanner::path_to_url(&series_dir)],
        )
        .unwrap();
        db.execute(
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT) \
             VALUES ('s1', 'ONGOING', 'Example', 'Example')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO BOOK_METADATA_AGGREGATION (SERIES_ID) VALUES ('s1')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
             VALUES ('b1', 'Example 1', ?, '2020-01-01 00:00:00.0', 's1', 'l1')",
            [komga_media::scanner::path_to_url(&pointer)],
        )
        .unwrap();
        db.execute(
            "INSERT INTO BOOK_METADATA (BOOK_ID, NUMBER, NUMBER_SORT, TITLE) \
             VALUES ('b1', '1', 1, 'Example 1')",
            [],
        )
        .unwrap();
        drop(db);

        let png = b"\x89PNG\r\n\x1a\nkmrs-cover".to_vec();
        let now = time_codec::now_utc();
        ThumbnailSeriesDao::new(state.db.clone())
            .insert(&ThumbnailSeries {
                id: "thumbs1".into(),
                series_id: "s1".into(),
                thumbnail: Some(png.clone()),
                url: None,
                selected: true,
                type_: ThumbnailType::UserUploaded,
                media_type: "image/png".into(),
                file_size: png.len() as i64,
                dimension: Dimension {
                    width: 1,
                    height: 1,
                },
                created_date: now,
                last_modified_date: now,
            })
            .unwrap();
        ThumbnailBookDao::new(state.db.clone())
            .insert(&ThumbnailBook {
                id: "thumbb1".into(),
                book_id: "b1".into(),
                thumbnail: Some(png.clone()),
                url: None,
                selected: true,
                type_: ThumbnailType::UserUploaded,
                media_type: "image/png".into(),
                file_size: png.len() as i64,
                dimension: Dimension {
                    width: 1,
                    height: 1,
                },
                created_date: now,
                last_modified_date: now,
            })
            .unwrap();

        write_series_metadata(&state, "s1").unwrap();
        write_book_metadata(&state, "b1").unwrap();

        let series_json: Value =
            serde_json::from_slice(&std::fs::read(series_dir.join(SERIES_JSON)).unwrap()).unwrap();
        assert_eq!(series_json["metadata"]["custom"], "keep");
        assert_eq!(series_json["metadata"]["comicid"], "42");
        assert_eq!(series_json["metadata"]["name"], "Example");

        let book = BookDao::new(state.db.clone())
            .find_by_id("b1")
            .unwrap()
            .unwrap();
        let xml = read_external_comicinfo(&book).unwrap().unwrap();
        assert!(String::from_utf8(xml)
            .unwrap()
            .contains("<Title>Example 1</Title>"));
        assert_eq!(std::fs::read(series_dir.join("cover.png")).unwrap(), png);
        assert_eq!(
            std::fs::read(series_dir.join("Example 1.png")).unwrap(),
            png
        );

        std::fs::write(
            series_dir.join(format!("Example 1.pdf.strm{COMICINFO_SUFFIX}")),
            br#"<?xml version="1.0"?><ComicInfo><Title>Sidecar Title</Title><Series>Example</Series><Number>1</Number></ComicInfo>"#,
        )
        .unwrap();
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO MEDIA (BOOK_ID, STATUS, MEDIA_TYPE, PAGE_COUNT) \
                 VALUES ('b1', 'READY', 'application/pdf', 1)",
                [],
            )
            .unwrap();
        crate::service::metadata::refresh_book_metadata(
            &state,
            &book,
            &BookMetadataPatchCapability::all(),
        )
        .unwrap();
        assert_eq!(
            BookMetadataDao::new(state.db.clone())
                .find_by_id("b1")
                .unwrap()
                .unwrap()
                .title,
            "Sidecar Title"
        );
    }
}
