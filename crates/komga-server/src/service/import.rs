//! `BookImporter.kt`: imports a book file into a series (move/copy/hardlink), with upgrade
//! handling (media/metadata/thumbnails/progress/read-lists carried over) and sidecar import.

use crate::events::DomainEvent;
use crate::service::{book, series as series_service};
use crate::state::AppState;
use komga_core::model::book::Book;
use komga_core::model::history::{HistoricalEvent, HistoricalEventType};
use komga_core::model::media::{Media, MediaStatus};
use komga_core::model::series::Series;
use komga_core::model::sidecar::{SidecarStored, SidecarType};
use komga_core::model::thumbnail::ThumbnailType;
use komga_core::task::{BookMetadataPatchCapability, CopyMode, DEFAULT_PRIORITY};
use komga_core::time_codec;
use komga_db::dao::book::{BookDao, BookMetadataDao};
use komga_db::dao::history::HistoricalEventDao;
use komga_db::dao::library::LibraryDao;
use komga_db::dao::media::MediaDao;
use komga_db::dao::read_progress::ReadProgressDao;
use komga_db::dao::readlist::ReadListDao;
use komga_db::dao::series::SeriesDao;
use komga_db::dao::sidecar::SidecarDao;
use komga_db::dao::thumbnail::ThumbnailBookDao;
use komga_media::scanner::{path_to_url, Scanner};
use std::path::{Path, PathBuf};

/// The exception channel of `BookImporter.importBook`: a coded `CodedException` (its `code` is
/// what the `BookImported` failure event carries) or a plain JVM exception (its message).
#[derive(Debug)]
pub enum ImportError {
    Coded { code: &'static str, message: String },
    Plain(String),
}

impl ImportError {
    fn coded(code: &'static str, message: impl Into<String>) -> Self {
        Self::Coded {
            code,
            message: message.into(),
        }
    }

    fn plain(message: impl Into<String>) -> Self {
        Self::Plain(message.into())
    }

    /// `if (e is CodedException) e.code else e.message`
    pub fn event_message(&self) -> String {
        match self {
            ImportError::Coded { code, .. } => code.to_string(),
            ImportError::Plain(m) => m.clone(),
        }
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Coded { code, message } => write!(f, "{code}: {message}"),
            ImportError::Plain(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<komga_db::Error> for ImportError {
    fn from(e: komga_db::Error) -> Self {
        Self::Plain(e.to_string())
    }
}

impl From<std::io::Error> for ImportError {
    fn from(e: std::io::Error) -> Self {
        Self::Plain(e.to_string())
    }
}

type ImportResult<T> = std::result::Result<T, ImportError>;

/// `BookImporter.importBook`. The `BookImported` domain event is published on both paths,
/// like the Java implementation (failure events carry the error code when there is one).
pub fn import_book(
    state: &AppState,
    source_file: &Path,
    series: &Series,
    copy_mode: CopyMode,
    destination_name: Option<&str>,
    upgrade_book_id: Option<&str>,
) -> ImportResult<Book> {
    let source_url = path_to_url(source_file);
    match import_book_internal(
        state,
        source_file,
        series,
        copy_mode,
        destination_name,
        upgrade_book_id,
    ) {
        Ok(book) => {
            let _ = state.events.send(DomainEvent::BookImported {
                book: Some(book.clone()),
                source_file: source_url,
                success: true,
                message: None,
            });
            Ok(book)
        }
        Err(e) => {
            let _ = state.events.send(DomainEvent::BookImported {
                book: None,
                source_file: source_url,
                success: false,
                message: Some(e.event_message()),
            });
            Err(e)
        }
    }
}

fn import_book_internal(
    state: &AppState,
    source_file: &Path,
    series: &Series,
    copy_mode: CopyMode,
    destination_name: Option<&str>,
    upgrade_book_id: Option<&str>,
) -> ImportResult<Book> {
    if !source_file.exists() {
        return Err(ImportError::coded(
            "ERR_1018",
            format!("File not found: {}", source_file.display()),
        ));
    }
    if series.oneshot && upgrade_book_id.is_none_or(|id| id.is_empty()) {
        return Err(ImportError::plain(
            "Destination series is oneshot but upgradeBookId is missing",
        ));
    }

    let series_path = PathBuf::from(komga_core::dto::url_to_file_path(&series.url));
    for library in LibraryDao::new(state.db.clone()).find_all()? {
        let library_path = PathBuf::from(komga_core::dto::url_to_file_path(&library.root));
        if source_file.starts_with(&library_path) {
            return Err(ImportError::coded(
                "ERR_1019",
                "Cannot import file that is part of an existing library",
            ));
        }
    }

    let book_to_upgrade = match upgrade_book_id {
        Some(id) => {
            let found = BookDao::new(state.db.clone()).find_by_id(id)?;
            if let Some(b) = &found {
                if b.series_id != series.id {
                    return Err(ImportError::coded(
                        "ERR_1020",
                        format!("Book to upgrade ({id}) does not belong to series: {series:?}"),
                    ));
                }
            }
            found
        }
        None => None,
    };

    let dest_dir = if series.oneshot {
        series_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"))
    } else {
        series_path.clone()
    };

    let source_name = file_name(source_file);
    let source_extension = source_file
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dest_file = dest_dir.join(match destination_name {
        // `Paths.get("$destinationName.${extension}").name`: only the last segment is kept
        Some(name) => Path::new(&format!("{name}.{source_extension}"))
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        None => source_name.clone(),
    });

    let scanner = Scanner::new();
    let source_base_name = name_without_extension(&source_name);
    let sidecars: Vec<(komga_media::scanner::Sidecar, PathBuf)> = scanner
        .scan_book_sidecars(source_file)
        .into_iter()
        .map(|sidecar| {
            let dest_name = match destination_name {
                Some(name) => ignore_case_replace(
                    &file_name(Path::new(&sidecar.url)),
                    &source_base_name,
                    name,
                ),
                None => file_name(Path::new(&sidecar.url)),
            };
            (sidecar, dest_dir.join(dest_name))
        })
        .collect();

    let upgrade_path = book_to_upgrade
        .as_ref()
        .map(|b| PathBuf::from(komga_core::dto::url_to_file_path(&b.url)));
    let mut deleted_upgraded_file = false;
    if let Some(up) = &upgrade_path {
        if dest_file == *up {
            tracing::info!("Deleting existing file: {}", up.display());
            match std::fs::remove_file(up) {
                Ok(()) => {
                    insert_book_file_deleted(
                        state,
                        book_to_upgrade.as_ref().unwrap(),
                        "File was deleted to import an upgrade",
                    )?;
                    deleted_upgraded_file = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tracing::warn!("Could not delete upgraded book: {}", up.display());
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    if !deleted_upgraded_file && dest_file.exists() {
        return Err(ImportError::coded(
            "ERR_1021",
            format!("Destination file already exists: {}", dest_file.display()),
        ));
    }

    // delete existing sidecars of the upgraded book
    if let Some(up) = &upgrade_path {
        for sidecar in scanner.scan_book_sidecars(up) {
            let sidecar_path = PathBuf::from(komga_core::dto::url_to_file_path(&sidecar.url));
            tracing::info!("Deleting existing file: {}", sidecar_path.display());
            let _ = std::fs::remove_file(&sidecar_path);
        }
    }

    let sidecar_sources: Vec<(PathBuf, PathBuf)> = sidecars
        .iter()
        .map(|(sidecar, dest)| {
            (
                PathBuf::from(komga_core::dto::url_to_file_path(&sidecar.url)),
                dest.clone(),
            )
        })
        .collect();
    match copy_mode {
        CopyMode::Move => {
            tracing::info!(
                "Moving file {} to {}",
                source_file.display(),
                dest_file.display()
            );
            move_file(source_file, &dest_file, false)?;
            for (src, dest) in &sidecar_sources {
                tracing::info!("Moving file {} to {}", src.display(), dest.display());
                move_file(src, dest, true)?;
            }
        }
        CopyMode::Copy => {
            tracing::info!(
                "Copying file {} to {}",
                source_file.display(),
                dest_file.display()
            );
            std::fs::copy(source_file, &dest_file)?;
            for (src, dest) in &sidecar_sources {
                tracing::info!("Copying file {} to {}", src.display(), dest.display());
                std::fs::copy(src, dest)?;
            }
        }
        CopyMode::Hardlink => {
            let hardlink = || -> std::io::Result<()> {
                tracing::info!(
                    "Hardlink file {} to {}",
                    source_file.display(),
                    dest_file.display()
                );
                std::fs::hard_link(source_file, &dest_file)?;
                for (src, dest) in &sidecar_sources {
                    tracing::info!("Hardlink file {} to {}", src.display(), dest.display());
                    let _ = std::fs::remove_file(dest);
                    std::fs::hard_link(src, dest)?;
                }
                Ok(())
            };
            if let Err(e) = hardlink() {
                tracing::warn!("Filesystem does not support hardlinks, copying instead: {e}");
                std::fs::copy(source_file, &dest_file)?;
                for (src, dest) in &sidecar_sources {
                    std::fs::copy(src, dest)?;
                }
            }
        }
    }

    let mut imported_book = scanner.scan_file(&dest_file).ok_or_else(|| {
        ImportError::coded(
            "ERR_1022",
            format!(
                "Newly imported book could not be scanned: {}",
                dest_file.display()
            ),
        )
    })?;
    imported_book.library_id = series.library_id.clone();
    imported_book.oneshot = series.oneshot;

    let added = series_service::add_books(state, series, std::slice::from_ref(&imported_book))?;
    imported_book = added
        .into_iter()
        .next()
        .ok_or_else(|| ImportError::plain("book missing after addBooks"))?;

    if let Some(upgraded) = &book_to_upgrade {
        // copy media and mark it as outdated
        let media = MediaDao::new(state.db.clone())
            .find_by_id(&upgraded.id)?
            .ok_or_else(|| ImportError::plain(format!("no media for book {}", upgraded.id)))?;
        MediaDao::new(state.db.clone()).update(&Media {
            book_id: imported_book.id.clone(),
            status: MediaStatus::Outdated,
            ..media
        })?;

        // copy metadata
        let metadata = BookMetadataDao::new(state.db.clone())
            .find_by_id(&upgraded.id)?
            .ok_or_else(|| ImportError::plain(format!("no metadata for book {}", upgraded.id)))?;
        BookMetadataDao::new(state.db.clone()).update(&komga_core::model::book::BookMetadata {
            book_id: imported_book.id.clone(),
            ..metadata
        })?;

        // copy user uploaded thumbnails
        let thumbnail_dao = ThumbnailBookDao::new(state.db.clone());
        for mut thumbnail in
            thumbnail_dao.find_all_by_book_id_and_type(&upgraded.id, ThumbnailType::UserUploaded)?
        {
            thumbnail.book_id = imported_book.id.clone();
            thumbnail_dao.update(&thumbnail)?;
        }

        // copy read progress
        let progress_dao = ReadProgressDao::new(state.db.clone());
        let progresses: Vec<komga_core::model::read_progress::ReadProgress> = progress_dao
            .find_by_book(&upgraded.id)?
            .into_iter()
            .map(|p| komga_core::model::read_progress::ReadProgress {
                book_id: imported_book.id.clone(),
                ..p
            })
            .collect();
        progress_dao.save_many(&progresses)?;

        // replace upgraded book by imported book in read lists
        let readlist_dao = ReadListDao::new(state.db.clone());
        for mut rl in readlist_dao.find_all_containing_book_id(&upgraded.id)? {
            rl.book_ids = rl
                .book_ids
                .values()
                .enumerate()
                .map(|(idx, id)| {
                    if *id == upgraded.id {
                        (idx as i32, imported_book.id.clone())
                    } else {
                        (idx as i32, id.clone())
                    }
                })
                .collect();
            readlist_dao.update(&rl)?;
        }

        // delete upgraded book file on disk if it has not been replaced earlier
        if !deleted_upgraded_file {
            if let Some(up) = &upgrade_path {
                if up.exists() && std::fs::remove_file(up).is_ok() {
                    tracing::info!("Deleted existing file: {}", up.display());
                    insert_book_file_deleted(
                        state,
                        upgraded,
                        "File was deleted to import an upgrade",
                    )?;
                }
            }
        }

        book::delete_one(state, upgraded)?;

        // update series if one-shot, so it's not marked as not found during the next scan
        if series.oneshot {
            let mut updated_series = series.clone();
            updated_series.url = imported_book.url.clone();
            updated_series.file_last_modified = imported_book.file_last_modified;
            SeriesDao::new(state.db.clone()).update(&updated_series, true)?;
        }
    }

    series_service::sort_books(state, series)?;

    for (source_sidecar, dest_path) in &sidecars {
        match source_sidecar.type_ {
            SidecarType::Artwork => {
                state
                    .task_emitter
                    .refresh_book_local_artwork(&imported_book, DEFAULT_PRIORITY)?;
            }
            SidecarType::Metadata => {
                state.task_emitter.refresh_book_metadata(
                    &imported_book,
                    BookMetadataPatchCapability::all(),
                    DEFAULT_PRIORITY,
                )?;
            }
        }
        let mtime = std::fs::metadata(dest_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .map(time::OffsetDateTime::from)
            .unwrap_or_else(time_codec::now_utc);
        SidecarDao::new(state.db.clone()).save(&SidecarStored {
            url: path_to_url(dest_path),
            parent_url: path_to_url(dest_path.parent().unwrap_or_else(|| Path::new("/"))),
            last_modified_time: mtime,
            library_id: imported_book.library_id.clone(),
        })?;
    }

    insert_history(
        state,
        HistoricalEventType::BookImported,
        Some(&imported_book.id),
        Some(&series.id),
        [
            ("name", book_path(&imported_book)),
            ("source", source_file.display().to_string()),
            (
                "upgrade",
                if upgrade_book_id.is_some() {
                    "Yes".to_string()
                } else {
                    "No".to_string()
                },
            ),
        ],
    )?;

    Ok(imported_book)
}

fn book_path(book: &Book) -> String {
    komga_core::dto::url_to_file_path(&book.url)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn name_without_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(idx) => name[..idx].to_string(),
        None => name.to_string(),
    }
}

/// Kotlin's `String.replace(old, new, ignoreCase = true)`: every occurrence, case-insensitively.
fn ignore_case_replace(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let mut out = String::with_capacity(haystack.len());
    let mut rest = haystack;
    let needle_lower = needle.to_lowercase();
    while let Some(idx) = rest.to_lowercase().find(&needle_lower) {
        out.push_str(&rest[..idx]);
        out.push_str(replacement);
        rest = &rest[idx + needle.len()..];
    }
    out.push_str(rest);
    out
}

/// `moveTo(dest, overwrite)` semantics: without overwrite an existing destination is an error.
fn move_file(source: &Path, dest: &Path, overwrite: bool) -> std::io::Result<()> {
    if dest.exists() {
        if overwrite {
            std::fs::remove_file(dest)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Destination file already exists: {}", dest.display()),
            ));
        }
    }
    std::fs::rename(source, dest)
}

fn insert_book_file_deleted(state: &AppState, book: &Book, reason: &str) -> komga_db::Result<()> {
    insert_history(
        state,
        HistoricalEventType::BookFileDeleted,
        Some(&book.id),
        Some(&book.series_id),
        [("reason", reason.to_string()), ("name", book_path(book))],
    )
}

fn insert_history(
    state: &AppState,
    type_: HistoricalEventType,
    book_id: Option<&str>,
    series_id: Option<&str>,
    properties: impl IntoIterator<Item = (&'static str, String)>,
) -> komga_db::Result<()> {
    HistoricalEventDao::new(state.db.clone()).insert(&HistoricalEvent {
        id: String::new(),
        type_,
        book_id: book_id.map(str::to_string),
        series_id: series_id.map(str::to_string),
        properties: properties
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        timestamp: time_codec::now_utc(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::series::tests as series_tests;
    use komga_core::model::library::{ScanInterval, SeriesCover};
    use komga_core::model::read_progress::ReadProgress;
    use komga_core::model::user::KomgaUser;
    use komga_db::dao::user::UserDao;
    use std::collections::BTreeMap;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../komga/komga/src/test/resources")
    }

    fn make_library(id: &str, root: &str) -> komga_core::model::library::Library {
        let now = time_codec::now_utc();
        komga_core::model::library::Library {
            id: id.into(),
            name: "L".into(),
            root: root.into(),
            import_comicinfo_book: true,
            import_comicinfo_series: true,
            import_comicinfo_collection: true,
            import_comicinfo_readlist: true,
            import_comicinfo_series_append_volume: true,
            import_epub_book: true,
            import_epub_series: true,
            import_mylar_series: true,
            import_local_artwork: true,
            import_barcode_isbn: true,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: ScanInterval::Every6H,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: SeriesCover::First,
            hash_files: true,
            hash_pages: true,
            hash_koreader: true,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now,
            last_modified_date: now,
        }
    }

    struct Env {
        state: AppState,
        library_root: PathBuf,
        series_path: PathBuf,
        outside: PathBuf,
        #[allow(dead_code)]
        library: komga_core::model::library::Library,
        series: Series,
        _tmp: tempfile::TempDir,
    }

    fn setup(name: &str) -> Env {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join(name);
        let library_root = root.join("library");
        let series_path = library_root.join("series");
        let outside = root.join("outside");
        std::fs::create_dir_all(&series_path).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let state = series_tests::test_state();
        let library = make_library("lib1", &format!("file:{}/", library_root.display()));
        LibraryDao::new(state.db.clone()).insert(&library).unwrap();

        let series = Series {
            id: "s1".into(),
            name: "series".into(),
            url: format!("file:{}/", series_path.display()),
            file_last_modified: time_codec::now_utc(),
            library_id: "lib1".into(),
            book_count: 0,
            deleted_date: None,
            oneshot: false,
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        };
        SeriesDao::new(state.db.clone()).insert(&series).unwrap();
        Env {
            state,
            library_root,
            series_path,
            outside,
            library,
            series,
            _tmp: tmp,
        }
    }

    fn books_of_series(state: &AppState, series_id: &str) -> Vec<Book> {
        BookDao::new(state.db.clone())
            .find_by_series_id(series_id)
            .unwrap()
    }

    fn events(state: &AppState) -> tokio::sync::broadcast::Receiver<DomainEvent> {
        state.events.subscribe()
    }

    fn recv_book_imported(rx: &mut tokio::sync::broadcast::Receiver<DomainEvent>) -> DomainEvent {
        for _ in 0..10 {
            let event = rx.try_recv().unwrap();
            if matches!(event, DomainEvent::BookImported { .. }) {
                return event;
            }
        }
        panic!("no BookImported event received");
    }

    #[test]
    fn import_copy_creates_book_and_history() {
        let env = setup("copy");
        let src = env.outside.join("new.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &src).unwrap();
        let mut rx = events(&env.state);

        let book = import_book(&env.state, &src, &env.series, CopyMode::Copy, None, None).unwrap();

        assert!(src.exists(), "COPY keeps the source");
        assert!(env.series_path.join("new.cbz").exists());
        assert_eq!(book.library_id, "lib1");
        let books = books_of_series(&env.state, &env.series.id);
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].name, "new");
        // media + metadata were created by addBooks
        assert!(MediaDao::new(env.state.db.clone())
            .find_by_id(&books[0].id)
            .unwrap()
            .is_some());
        // series got sorted (book_count updated)
        assert_eq!(
            SeriesDao::new(env.state.db.clone())
                .find_by_id(&env.series.id)
                .unwrap()
                .unwrap()
                .book_count,
            1
        );
        // BookImported history row
        let count: i64 = env
            .state
            .db
            .ro()
            .query_row(
                "SELECT COUNT(*) FROM HISTORICAL_EVENT WHERE TYPE = 'BookImported'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        // success event (after the BookAdded emitted by addBooks)
        let event = recv_book_imported(&mut rx);
        let DomainEvent::BookImported {
            book: Some(_),
            success: true,
            message: None,
            ..
        } = event
        else {
            panic!("expected successful BookImported, got {event:?}");
        };
    }

    #[test]
    fn import_move_removes_source() {
        let env = setup("move");
        let src = env.outside.join("moveme.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &src).unwrap();

        import_book(&env.state, &src, &env.series, CopyMode::Move, None, None).unwrap();
        assert!(!src.exists());
        assert!(env.series_path.join("moveme.cbz").exists());
    }

    #[test]
    fn import_hardlink() {
        let env = setup("hardlink");
        let src = env.outside.join("linked.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &src).unwrap();

        import_book(
            &env.state,
            &src,
            &env.series,
            CopyMode::Hardlink,
            None,
            None,
        )
        .unwrap();
        // source kept (link or fallback copy, both leave the source in place)
        assert!(src.exists());
        assert!(env.series_path.join("linked.cbz").exists());
    }

    #[test]
    fn import_with_destination_name_renames_book_and_sidecars() {
        let env = setup("destname");
        let src = env.outside.join("plain.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &src).unwrap();
        let sidecar = env.outside.join("plain.jpg");
        std::fs::copy(fixtures().join("hashpage/tg.png/1.png"), &sidecar).unwrap();

        let book = import_book(
            &env.state,
            &src,
            &env.series,
            CopyMode::Copy,
            Some("Fancy Name"),
            None,
        )
        .unwrap();

        assert!(env.series_path.join("Fancy Name.cbz").exists());
        assert!(env.series_path.join("Fancy Name.jpg").exists());
        assert_eq!(book.name, "Fancy Name");
        // sidecar registered and local artwork task queued
        let stored = SidecarDao::new(env.state.db.clone()).find_all().unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].url.ends_with("Fancy%20Name.jpg"));
        let tasks = komga_db::dao::tasks::TasksDao::new(env.state.tasks_db.clone())
            .find_all()
            .unwrap();
        assert!(tasks
            .iter()
            .any(|t| t.unique_id().starts_with("REFRESH_BOOK_LOCAL_ARTWORK_")));
    }

    #[test]
    fn import_fails_when_destination_exists() {
        let env = setup("exists");
        let src = env.outside.join("v01.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &src).unwrap();
        std::fs::copy(&src, env.series_path.join("v01.cbz")).unwrap();
        let mut rx = events(&env.state);

        let err =
            import_book(&env.state, &src, &env.series, CopyMode::Copy, None, None).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Coded {
                code: "ERR_1021",
                ..
            }
        ));
        let event = rx.try_recv().unwrap();
        let DomainEvent::BookImported {
            book: None,
            success: false,
            message: Some(msg),
            ..
        } = event
        else {
            panic!("expected failed BookImported, got {event:?}");
        };
        assert_eq!(msg, "ERR_1021");
    }

    #[test]
    fn import_rejects_library_file_and_missing_file() {
        let env = setup("guards");
        let inside = env.library_root.join("already.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &inside).unwrap();
        let err =
            import_book(&env.state, &inside, &env.series, CopyMode::Copy, None, None).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Coded {
                code: "ERR_1019",
                ..
            }
        ));

        let missing = env.outside.join("nope.cbz");
        let err = import_book(
            &env.state,
            &missing,
            &env.series,
            CopyMode::Copy,
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ImportError::Coded {
                code: "ERR_1018",
                ..
            }
        ));
    }

    fn seed_upgrade_target(env: &Env) -> Book {
        // an existing book of the series, with media/progress/thumbnail uploaded
        let target_file = env.series_path.join("old.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &target_file).unwrap();
        let scanned = Scanner::new().scan_file(&target_file).unwrap();
        let mut book = scanned;
        book.library_id = "lib1".into();
        let added = series_service::add_books(&env.state, &env.series, &[book]).unwrap();
        let book = added.into_iter().next().unwrap();

        // analyze to READY
        let media = MediaDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        MediaDao::new(env.state.db.clone())
            .update(&Media {
                status: MediaStatus::Ready,
                media_type: Some("application/zip".into()),
                ..media
            })
            .unwrap();

        // user uploaded thumbnail
        ThumbnailBookDao::new(env.state.db.clone())
            .insert(&komga_core::model::thumbnail::ThumbnailBook {
                id: String::new(),
                book_id: book.id.clone(),
                thumbnail: Some(vec![1, 2, 3]),
                url: None,
                selected: true,
                type_: ThumbnailType::UserUploaded,
                media_type: "image/jpeg".into(),
                file_size: 3,
                dimension: komga_core::model::thumbnail::Dimension {
                    width: 1,
                    height: 1,
                },
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            })
            .unwrap();

        // read progress for a user
        let user = KomgaUser {
            id: "u1".into(),
            email: "u@x.y".into(),
            password: "x".into(),
            roles: Default::default(),
            shared_libraries_ids: Default::default(),
            shared_all_libraries: true,
            restrictions: Default::default(),
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        };
        UserDao::new(env.state.db.clone()).insert(&user).unwrap();
        ReadProgressDao::new(env.state.db.clone())
            .insert_or_update(&ReadProgress {
                book_id: book.id.clone(),
                user_id: "u1".into(),
                page: 1,
                completed: true,
                read_date: time_codec::now_utc(),
                device_id: String::new(),
                device_name: String::new(),
                locator: None,
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            })
            .unwrap();
        book
    }

    #[test]
    fn upgrade_replaces_book_and_carries_over() {
        let env = setup("upgrade");
        let old = seed_upgrade_target(&env);
        let src = env.outside.join("old.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &src).unwrap();
        let mut rx = events(&env.state);

        let book = import_book(
            &env.state,
            &src,
            &env.series,
            CopyMode::Move,
            None,
            Some(&old.id),
        )
        .unwrap();

        // old book deleted, new book present with the same name
        assert!(BookDao::new(env.state.db.clone())
            .find_by_id(&old.id)
            .unwrap()
            .is_none());
        assert_eq!(books_of_series(&env.state, &env.series.id).len(), 1);

        // media carried over and marked OUTDATED
        let media = MediaDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        assert_eq!(media.status, MediaStatus::Outdated);

        // uploaded thumbnail carried over
        let thumbnails = ThumbnailBookDao::new(env.state.db.clone())
            .find_all_by_book_id_and_type(&book.id, ThumbnailType::UserUploaded)
            .unwrap();
        assert_eq!(thumbnails.len(), 1);

        // read progress carried over
        let progress = ReadProgressDao::new(env.state.db.clone())
            .find_by_book_and_user(&book.id, "u1")
            .unwrap();
        assert!(progress.is_some());

        // history: one BookFileDeleted (upgrade deletion) and one BookImported
        let count: i64 = env
            .state
            .db
            .ro()
            .query_row(
                "SELECT COUNT(*) FROM HISTORICAL_EVENT WHERE TYPE = 'BookFileDeleted'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let props: BTreeMap<String, String> = {
            let conn = env.state.db.ro();
            let mut stmt = conn
                .prepare(
                    "SELECT KEY, VALUE FROM HISTORICAL_EVENT_PROPERTIES \
                     WHERE ID IN (SELECT ID FROM HISTORICAL_EVENT WHERE TYPE = 'BookImported')",
                )
                .unwrap();
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .unwrap()
                .collect::<std::result::Result<BTreeMap<_, _>, _>>()
                .unwrap()
        };
        assert_eq!(props.get("upgrade").map(String::as_str), Some("Yes"));

        // success event mentions the new book
        let event = recv_book_imported(&mut rx);
        let DomainEvent::BookImported {
            book: Some(b),
            success: true,
            ..
        } = event
        else {
            panic!("expected successful BookImported, got {event:?}");
        };
        assert_eq!(b.id, book.id);
    }

    #[test]
    fn upgrade_rejects_cross_series() {
        let env = setup("crossseries");
        let other = Series {
            id: "s2".into(),
            name: "other".into(),
            url: format!("file:{}/", env.series_path.display()),
            file_last_modified: time_codec::now_utc(),
            library_id: "lib1".into(),
            book_count: 0,
            deleted_date: None,
            oneshot: false,
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        };
        SeriesDao::new(env.state.db.clone()).insert(&other).unwrap();
        let old = seed_upgrade_target(&env);
        let src = env.outside.join("old.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &src).unwrap();

        let err = import_book(
            &env.state,
            &src,
            &other,
            CopyMode::Copy,
            None,
            Some(&old.id),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ImportError::Coded {
                code: "ERR_1020",
                ..
            }
        ));
    }

    #[test]
    fn oneshot_requires_upgrade_book_id() {
        let env = setup("oneshotguard");
        let oneshot_series = Series {
            oneshot: true,
            ..env.series.clone()
        };
        let src = env.outside.join("one.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &src).unwrap();

        let err = import_book(
            &env.state,
            &src,
            &oneshot_series,
            CopyMode::Copy,
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, ImportError::Plain(_)));
        assert_eq!(
            err.to_string(),
            "Destination series is oneshot but upgradeBookId is missing"
        );
    }
}
