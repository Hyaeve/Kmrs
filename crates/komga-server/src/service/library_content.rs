//! `LibraryContentLifecycle.kt`: library scan orchestration (soft-delete of vanished content,
//! restore of moved/renamed content by hash, sidecar tracking, trash handling).

use crate::events::DomainEvent;
use crate::state::AppState;
use komga_core::model::book::{Book, BookMetadata};
use komga_core::model::library::Library;
use komga_core::model::media::{Media, MediaStatus};
use komga_core::model::series::{Series, SeriesMetadata};
use komga_core::model::sidecar::{SidecarSource, SidecarStored, SidecarType};
use komga_core::model::thumbnail::{ThumbnailBook, ThumbnailSeries, ThumbnailType};
use komga_core::task::{BookMetadataPatchCapability, DEFAULT_PRIORITY};
use komga_core::time_codec;
use komga_db::dao::book::{BookDao, BookMetadataDao};
use komga_db::dao::collection::CollectionDao;
use komga_db::dao::library::LibraryDao;
use komga_db::dao::media::MediaDao;
use komga_db::dao::read_progress::ReadProgressDao;
use komga_db::dao::readlist::ReadListDao;
use komga_db::dao::series::{SeriesDao, SeriesMetadataDao};
use komga_db::dao::sidecar::SidecarDao;
use komga_db::dao::thumbnail::{ThumbnailBookDao, ThumbnailSeriesDao};
use komga_db::Result;
use komga_media::hash::compute_hash;
use komga_media::scanner::{ScanError, ScanOptions, Scanner};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ScanRootError {
    #[error(transparent)]
    Db(#[from] komga_db::Error),
    #[error(transparent)]
    Scan(#[from] ScanError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type ScanRootResult<T> = std::result::Result<T, ScanRootError>;

pub fn scan_root_folder(
    state: &AppState,
    library: &Library,
    scan_deep: bool,
) -> ScanRootResult<()> {
    tracing::info!("Scan root folder for library: {library:?}");
    let root = PathBuf::from(komga_core::dto::url_to_file_path(&library.root));
    let options = ScanOptions {
        force_directory_modified_time: library.scan_force_modified_time,
        oneshots_dir: library.oneshots_directory.clone(),
        scan_cbx: library.scan_cbx,
        scan_pdf: library.scan_pdf,
        scan_epub: library.scan_epub,
        directory_exclusions: library.scan_directory_exclusions.iter().cloned().collect(),
    };
    let scan_result = match Scanner::new().scan_root_folder(&root, &options) {
        Ok(result) => result,
        Err(e) => {
            // the library root is not accessible: mark it unavailable, then fail the scan
            let mut updated = library.clone();
            updated.unavailable_date = Some(time_codec::now_utc());
            LibraryDao::new(state.db.clone()).update(&updated)?;
            let _ = state.events.send(DomainEvent::LibraryUpdated(updated));
            return Err(ScanRootError::Scan(e));
        }
    };

    if library.unavailable_date.is_some() {
        let mut updated = library.clone();
        updated.unavailable_date = None;
        LibraryDao::new(state.db.clone()).update(&updated)?;
        let _ = state.events.send(DomainEvent::LibraryUpdated(updated));
    }

    let scanned_series: Vec<(Series, Vec<Book>)> = scan_result
        .series
        .into_iter()
        .map(|(series, books)| {
            (
                Series {
                    library_id: library.id.clone(),
                    ..series
                },
                books
                    .into_iter()
                    .map(|book| Book {
                        library_id: library.id.clone(),
                        ..book
                    })
                    .collect(),
            )
        })
        .collect();

    let series_dao = SeriesDao::new(state.db.clone());
    // delete series that don't exist anymore
    if scanned_series.is_empty() {
        tracing::info!("Scan returned no series, soft deleting all existing series");
        let existing = series_dao.find_by_library_id(&library.id)?;
        crate::service::series::soft_delete_many(state, &existing)?;
    } else {
        let urls: Vec<String> = scanned_series.iter().map(|(s, _)| s.url.clone()).collect();
        let gone =
            series_dao.find_all_not_deleted_by_library_id_and_url_not_in(&library.id, &urls)?;
        if !gone.is_empty() {
            tracing::info!("Soft deleting series not on disk anymore: {gone:?}");
            crate::service::series::soft_delete_many(state, &gone)?;
        }
    }

    // delete books that don't exist anymore. We need to do this now, so trash bin can work
    let book_dao = BookDao::new(state.db.clone());
    let book_urls: Vec<String> = scanned_series
        .iter()
        .flat_map(|(_, books)| books.iter().map(|b| b.url.clone()))
        .collect();
    let gone_books =
        book_dao.find_all_not_deleted_by_library_id_and_url_not_in(&library.id, &book_urls)?;
    let mut series_to_sort_and_refresh: Vec<Series> = vec![];
    if !gone_books.is_empty() {
        tracing::info!("Soft deleting books not on disk anymore: {gone_books:?}");
        crate::service::book::soft_delete_many(state, &gone_books)?;
        let mut seen = BTreeSet::new();
        for book in &gone_books {
            if seen.insert(book.series_id.clone()) {
                if let Some(series) = series_dao.find_by_id(&book.series_id)? {
                    series_to_sort_and_refresh.push(series);
                }
            }
        }
    }
    // series that had books deleted are refreshed even if their file modified date did not
    // change (NFS/SMB caches can hide the change)
    let series_url_with_deleted_books: BTreeSet<String> = series_to_sort_and_refresh
        .iter()
        .map(|s| s.url.clone())
        .collect();

    for (new_series, new_books) in &scanned_series {
        match series_dao
            .find_not_deleted_by_library_id_and_url_or_null(&library.id, &new_series.url)?
        {
            None => {
                tracing::info!("Adding new series: {new_series:?}");
                let created = crate::service::series::create_series(state, new_series)?;
                let added = crate::service::series::add_books(state, &created, new_books)?;
                try_restore_series(state, &created, &added)?;
                try_restore_books(state, &added)?;
                series_to_sort_and_refresh.push(created);
            }
            Some(existing_series) => {
                let series_changed = time_codec::truncate_to_millis(new_series.file_last_modified)
                    != time_codec::truncate_to_millis(existing_series.file_last_modified)
                    || existing_series.deleted_date.is_some()
                    || series_url_with_deleted_books.contains(&new_series.url);
                if series_changed {
                    tracing::info!("Series changed on disk, updating: {existing_series:?}");
                    series_dao.update(
                        &Series {
                            file_last_modified: new_series.file_last_modified,
                            deleted_date: None,
                            ..existing_series.clone()
                        },
                        true,
                    )?;
                }
                if scan_deep || series_changed {
                    let existing_books = book_dao.find_by_series_id(&existing_series.id)?;
                    for new_book in new_books {
                        let Some(existing_book) = existing_books
                            .iter()
                            .find(|b| b.url == new_book.url && b.deleted_date.is_none())
                        else {
                            continue;
                        };
                        if time_codec::truncate_to_millis(new_book.file_last_modified)
                            == time_codec::truncate_to_millis(existing_book.file_last_modified)
                        {
                            continue;
                        }
                        // hashing is only worth it when the size matches and we have a baseline
                        let hash = if existing_book.file_size == new_book.file_size
                            && !existing_book.file_hash.is_empty()
                        {
                            Some(compute_hash(&book_path(new_book))?)
                        } else {
                            None
                        };
                        if hash.as_deref() == Some(existing_book.file_hash.as_str()) {
                            tracing::info!(
                                "Book changed on disk, but still has the same hash, no need to reset media status: {existing_book:?}"
                            );
                            book_dao.update(&Book {
                                file_last_modified: new_book.file_last_modified,
                                file_size: new_book.file_size,
                                file_hash: hash.expect("checked above"),
                                ..existing_book.clone()
                            })?;
                        } else {
                            tracing::info!(
                                "Book changed on disk, update and reset media status: {existing_book:?}"
                            );
                            let media_dao = MediaDao::new(state.db.clone());
                            if let Some(media) = media_dao.find_by_id(&existing_book.id)? {
                                media_dao.update(&Media {
                                    status: MediaStatus::Outdated,
                                    ..media
                                })?;
                            }
                            book_dao.update(&Book {
                                file_last_modified: new_book.file_last_modified,
                                file_size: new_book.file_size,
                                file_hash: hash.unwrap_or_default(),
                                ..existing_book.clone()
                            })?;
                        }
                    }

                    let existing_urls: BTreeSet<&str> = existing_books
                        .iter()
                        .filter(|b| b.deleted_date.is_none())
                        .map(|b| b.url.as_str())
                        .collect();
                    let books_to_add: Vec<Book> = new_books
                        .iter()
                        .filter(|b| !existing_urls.contains(b.url.as_str()))
                        .cloned()
                        .collect();
                    if !books_to_add.is_empty() {
                        tracing::info!("Adding new books: {books_to_add:?}");
                        let added = crate::service::series::add_books(
                            state,
                            &existing_series,
                            &books_to_add,
                        )?;
                        try_restore_books(state, &added)?;
                        series_to_sort_and_refresh.push(existing_series);
                    }
                }
            }
        }
    }

    // for all series where books have been removed or added, sort and refresh metadata
    let mut seen = BTreeSet::new();
    for series in &series_to_sort_and_refresh {
        if seen.insert(series.id.clone()) {
            crate::service::series::sort_books(state, series)?;
            state
                .task_emitter
                .refresh_series_metadata(&series.id, DEFAULT_PRIORITY)?;
        }
    }

    // sidecars: refresh the owning entity when a sidecar appears or changes
    let sidecar_dao = SidecarDao::new(state.db.clone());
    let existing_sidecars = sidecar_dao.find_all()?;
    for new_sidecar in &scan_result.sidecars {
        let changed = match existing_sidecars.iter().find(|s| s.url == new_sidecar.url) {
            Some(existing) => {
                time_codec::truncate_to_millis(existing.last_modified_time)
                    != time_codec::truncate_to_millis(new_sidecar.last_modified_time)
            }
            None => true,
        };
        if !changed {
            continue;
        }
        match new_sidecar.source {
            SidecarSource::Series => {
                if let Some(series) = series_dao.find_not_deleted_by_library_id_and_url_or_null(
                    &library.id,
                    &new_sidecar.parent_url,
                )? {
                    match new_sidecar.type_ {
                        SidecarType::Artwork => state
                            .task_emitter
                            .refresh_series_local_artwork(&series.id, DEFAULT_PRIORITY)?,
                        SidecarType::Metadata => state
                            .task_emitter
                            .refresh_series_metadata(&series.id, DEFAULT_PRIORITY)?,
                    }
                }
            }
            SidecarSource::Book => {
                if let Some(book) =
                    find_not_deleted_book_by_url(&state.db, &library.id, &new_sidecar.parent_url)?
                {
                    match new_sidecar.type_ {
                        SidecarType::Artwork => state
                            .task_emitter
                            .refresh_book_local_artwork(&book, DEFAULT_PRIORITY)?,
                        SidecarType::Metadata => state.task_emitter.refresh_book_metadata(
                            &book,
                            BookMetadataPatchCapability::all(),
                            DEFAULT_PRIORITY,
                        )?,
                    }
                }
            }
        }
        sidecar_dao.save(&SidecarStored {
            url: new_sidecar.url.clone(),
            parent_url: new_sidecar.parent_url.clone(),
            last_modified_time: new_sidecar.last_modified_time,
            library_id: library.id.clone(),
        })?;
    }

    // cleanup sidecars that don't exist anymore
    let new_urls: BTreeSet<&str> = scan_result
        .sidecars
        .iter()
        .map(|s| s.url.as_str())
        .collect();
    let gone_urls: Vec<String> = existing_sidecars
        .iter()
        .filter(|s| !new_urls.contains(s.url.as_str()))
        .map(|s| s.url.clone())
        .collect();
    if !gone_urls.is_empty() {
        sidecar_dao.delete_by_library_id_and_urls(&library.id, &gone_urls)?;
    }

    if library.empty_trash_after_scan {
        empty_trash(state, library)?;
    } else {
        cleanup_empty_sets(state)?;
    }

    let _ = state
        .events
        .send(DomainEvent::LibraryScanned(library.clone()));
    Ok(())
}

pub fn empty_trash(state: &AppState, library: &Library) -> Result<()> {
    tracing::info!("Empty trash for library: {library:?}");
    use komga_core::search::*;
    let series_to_delete = SeriesDao::new(state.db.clone()).find_all_by_condition(
        Some(&SearchConditionSeries::AllOf {
            conditions: vec![
                SearchConditionSeries::LibraryId {
                    operator: Equality::Is {
                        value: library.id.clone(),
                    },
                },
                SearchConditionSeries::Deleted {
                    deleted: BooleanOp::IsTrue,
                },
            ],
        }),
        &SearchContext::default(),
    )?;
    crate::service::series::delete_many(state, &series_to_delete)?;

    let books_to_delete = BookDao::new(state.db.clone()).find_all_by_condition(
        Some(&SearchConditionBook::AllOf {
            conditions: vec![
                SearchConditionBook::LibraryId {
                    operator: Equality::Is {
                        value: library.id.clone(),
                    },
                },
                SearchConditionBook::Deleted {
                    deleted: BooleanOp::IsTrue,
                },
            ],
        }),
        &SearchContext::default(),
        &[],
    )?;
    crate::service::book::delete_many(state, &books_to_delete)?;
    let series_dao = SeriesDao::new(state.db.clone());
    let mut seen = BTreeSet::new();
    for book in &books_to_delete {
        if seen.insert(book.series_id.clone()) {
            if let Some(series) = series_dao.find_by_id(&book.series_id)? {
                crate::service::series::sort_books(state, &series)?;
            }
        }
    }

    cleanup_empty_sets(state)
}

fn cleanup_empty_sets(state: &AppState) -> Result<()> {
    let settings = state.settings.get();
    if settings.delete_empty_collections {
        crate::service::collection::delete_empty_collections(state)?;
    }

    if settings.delete_empty_readlists {
        crate::service::readlist::delete_empty_read_lists(state)?;
    }
    Ok(())
}

/// A moved/renamed series is matched against soft-deleted series by book count and by every
/// book's size and hash; on a match, metadata (locked fields win), user thumbnails, and
/// collection memberships are transferred, and books go through `try_restore_books`.
fn try_restore_series(
    state: &AppState,
    new_series: &Series,
    new_books: &[Book],
) -> ScanRootResult<()> {
    let book_sizes: BTreeSet<i64> = new_books.iter().map(|b| b.file_size).collect();
    let book_dao = BookDao::new(state.db.clone());
    let series_dao = SeriesDao::new(state.db.clone());
    let deleted = series_dao.find_all_by_condition(
        Some(&komga_core::search::SearchConditionSeries::Deleted {
            deleted: komga_core::search::BooleanOp::IsTrue,
        }),
        &komga_core::search::SearchContext::default(),
    )?;
    let mut candidates = vec![];
    for deleted_candidate in deleted {
        let deleted_books = book_dao.find_by_series_id(&deleted_candidate.id)?;
        let deleted_sizes: BTreeSet<i64> = deleted_books.iter().map(|b| b.file_size).collect();
        if new_books.len() == deleted_books.len()
            && book_sizes.is_superset(&deleted_sizes)
            && deleted_sizes.is_superset(&book_sizes)
            && deleted_books.iter().all(|b| !b.file_hash.is_empty())
        {
            candidates.push((deleted_candidate, deleted_books));
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }

    // hash the new books so they can be matched against the deleted ones
    let mut new_books_with_hash = vec![];
    for book in new_books {
        let mut stored = book_dao
            .find_by_id(&book.id)?
            .expect("new book was just inserted");
        stored.file_hash = compute_hash(&book_path(book))?;
        book_dao.update(&stored)?;
        new_books_with_hash.push(stored);
    }
    let new_hashes: BTreeSet<&str> = new_books_with_hash
        .iter()
        .map(|b| b.file_hash.as_str())
        .collect();
    let found = candidates.into_iter().find(|(_, books)| {
        let deleted_hashes: BTreeSet<&str> = books.iter().map(|b| b.file_hash.as_str()).collect();
        deleted_hashes.is_superset(&new_hashes) && new_hashes.is_superset(&deleted_hashes)
    });
    let Some((deleted_series, _)) = found else {
        return Ok(());
    };

    // copy metadata; locked fields win over the fresh ones
    let metadata_dao = SeriesMetadataDao::new(state.db.clone());
    let deleted_metadata = metadata_dao
        .find_by_id(&deleted_series.id)?
        .expect("deleted series has metadata");
    let new_metadata = metadata_dao
        .find_by_id(&new_series.id)?
        .expect("new series has metadata");
    metadata_dao.update(&SeriesMetadata {
        series_id: new_series.id.clone(),
        title: if deleted_metadata.title_lock {
            deleted_metadata.title.clone()
        } else {
            new_metadata.title.clone()
        },
        title_sort: if deleted_metadata.title_sort_lock {
            deleted_metadata.title_sort.clone()
        } else {
            new_metadata.title_sort.clone()
        },
        ..deleted_metadata
    })?;

    // move user uploaded thumbnails
    let thumbnail_dao = ThumbnailSeriesDao::new(state.db.clone());
    for thumbnail in thumbnail_dao
        .find_all_by_series_id_and_type(&deleted_series.id, ThumbnailType::UserUploaded)?
    {
        thumbnail_dao.update(&ThumbnailSeries {
            series_id: new_series.id.clone(),
            ..thumbnail
        })?;
    }

    // replace the deleted series in collections
    let collection_dao = CollectionDao::new(state.db.clone());
    for mut collection in collection_dao.find_all_containing_series_id(&deleted_series.id)? {
        collection.series_ids = collection
            .series_ids
            .iter()
            .map(|id| {
                if id == &deleted_series.id {
                    new_series.id.clone()
                } else {
                    id.clone()
                }
            })
            .collect();
        collection_dao.update(&collection)?;
    }

    try_restore_books(state, &new_books_with_hash)?;

    crate::service::series::delete_many(state, std::slice::from_ref(&deleted_series))?;
    Ok(())
}

/// A moved/renamed book is matched against soft-deleted books by size, then hash; on a match,
/// media, thumbnails, metadata (locked title wins), read progress, and readlist memberships are
/// transferred, and the deleted book is removed.
fn try_restore_books(state: &AppState, new_books: &[Book]) -> ScanRootResult<()> {
    let book_dao = BookDao::new(state.db.clone());
    for book_to_add in new_books {
        let deleted_candidates: Vec<Book> = book_dao
            .find_all_deleted_by_file_size(book_to_add.file_size)?
            .into_iter()
            .filter(|b| !b.file_hash.is_empty())
            .collect();
        if deleted_candidates.is_empty() {
            continue;
        }

        // if the book has no hash, compute it and store it
        let book_with_hash = if !book_to_add.file_hash.is_empty() {
            book_to_add.clone()
        } else {
            let mut stored = book_dao
                .find_by_id(&book_to_add.id)?
                .expect("new book was just inserted");
            stored.file_hash = compute_hash(&book_path(book_to_add))?;
            book_dao.update(&stored)?;
            stored
        };

        let Some(matched) = deleted_candidates
            .into_iter()
            .find(|b| b.file_hash == book_with_hash.file_hash)
        else {
            continue;
        };

        // copy media
        let media_dao = MediaDao::new(state.db.clone());
        if let Some(media) = media_dao.find_by_id(&matched.id)? {
            media_dao.update(&Media {
                book_id: book_to_add.id.clone(),
                ..media
            })?;
        }

        // move generated and user uploaded thumbnails
        let thumbnail_dao = ThumbnailBookDao::new(state.db.clone());
        for type_ in [ThumbnailType::Generated, ThumbnailType::UserUploaded] {
            for thumbnail in thumbnail_dao.find_all_by_book_id_and_type(&matched.id, type_)? {
                thumbnail_dao.update(&ThumbnailBook {
                    book_id: book_to_add.id.clone(),
                    ..thumbnail
                })?;
            }
        }

        // copy metadata; a locked title wins over the fresh one
        let metadata_dao = BookMetadataDao::new(state.db.clone());
        let deleted_metadata = metadata_dao
            .find_by_id(&matched.id)?
            .expect("deleted book has metadata");
        let new_metadata = metadata_dao
            .find_by_id(&book_to_add.id)?
            .expect("new book has metadata");
        metadata_dao.update(&BookMetadata {
            book_id: book_to_add.id.clone(),
            title: if deleted_metadata.title_lock {
                deleted_metadata.title.clone()
            } else {
                new_metadata.title.clone()
            },
            ..deleted_metadata
        })?;
        if !deleted_metadata.title_lock {
            state.task_emitter.refresh_book_metadata(
                book_to_add,
                [BookMetadataPatchCapability::Title].into_iter().collect(),
                DEFAULT_PRIORITY,
            )?;
        }

        // copy read progress
        let progress_dao = ReadProgressDao::new(state.db.clone());
        let progresses: Vec<komga_core::model::read_progress::ReadProgress> = progress_dao
            .find_by_book(&matched.id)?
            .into_iter()
            .map(|p| komga_core::model::read_progress::ReadProgress {
                book_id: book_to_add.id.clone(),
                ..p
            })
            .collect();
        progress_dao.save_many(&progresses)?;

        // replace the deleted book in read lists
        let readlist_dao = ReadListDao::new(state.db.clone());
        for mut readlist in readlist_dao.find_all_containing_book_id(&matched.id)? {
            readlist.book_ids = readlist
                .book_ids
                .values()
                .enumerate()
                .map(|(index, id)| {
                    (
                        index as i32,
                        if id == &matched.id {
                            book_to_add.id.clone()
                        } else {
                            id.clone()
                        },
                    )
                })
                .collect();
            readlist_dao.update(&readlist)?;
        }

        crate::service::book::delete_one(state, &matched)?;
    }
    Ok(())
}

/// `BookRepository.findNotDeletedByLibraryIdAndUrlOrNull`
fn find_not_deleted_book_by_url(
    db: &komga_db::pool::Database,
    library_id: &str,
    url: &str,
) -> Result<Option<Book>> {
    fn datetime(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<time::OffsetDateTime> {
        let s: String = row.get(idx)?;
        time_codec::parse_datetime_utc(&s).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                idx,
                rusqlite::types::Type::Text,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid datetime: {s}"),
                )
                .into(),
            )
        })
    }
    let conn = db.ro();
    let mut stmt = conn.prepare(
        "SELECT ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, FILE_SIZE, NUMBER, FILE_HASH, FILE_HASH_KOREADER, DELETED_DATE, ONESHOT, CREATED_DATE, LAST_MODIFIED_DATE \
         FROM BOOK WHERE LIBRARY_ID = ? AND URL = ? AND DELETED_DATE IS NULL ORDER BY LAST_MODIFIED_DATE DESC",
    )?;
    let mut rows = stmt.query_map(rusqlite::params![library_id, url], |row| {
        let deleted_date: Option<String> = row.get(10)?;
        Ok(Book {
            id: row.get(0)?,
            name: row.get(1)?,
            url: row.get(2)?,
            file_last_modified: datetime(row, 3)?,
            series_id: row.get(4)?,
            library_id: row.get(5)?,
            file_size: row.get(6)?,
            number: row.get(7)?,
            file_hash: row.get(8)?,
            file_hash_koreader: row.get(9)?,
            deleted_date: deleted_date
                .map(|s| time_codec::parse_datetime_utc(&s).expect("stored datetime is valid")),
            oneshot: row.get(11)?,
            created_date: datetime(row, 12)?,
            last_modified_date: datetime(row, 13)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

fn book_path(book: &Book) -> PathBuf {
    PathBuf::from(komga_core::dto::url_to_file_path(&book.url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::series::tests::{seed_user, test_state};
    use komga_db::dao::settings::SettingsDao;
    use komga_db::dao::tasks::TasksDao;
    use komga_db::pool::Database;
    use komga_media::scanner::path_to_url;
    use std::path::Path;

    fn library(db: &Database, id: &str, root: &Path) -> Library {
        db.rw()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, 'L', ?)",
                rusqlite::params![id, path_to_url(root)],
            )
            .unwrap();
        LibraryDao::new(db.clone()).find_by_id(id).unwrap().unwrap()
    }

    /// The scan root must not be dot-prefixed (komga skips dot directories), and tempfile's
    /// own directory is: use a plain subdirectory instead
    fn scan_root(tmp: &tempfile::TempDir) -> PathBuf {
        let root = tmp.path().join("library-root");
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn scan(state: &AppState, lib: &Library) {
        scan_root_folder(state, lib, false).unwrap();
    }

    fn all_books(state: &AppState) -> Vec<Book> {
        BookDao::new(state.db.clone()).find_all().unwrap()
    }

    fn all_series(state: &AppState) -> Vec<Series> {
        SeriesDao::new(state.db.clone()).find_all().unwrap()
    }

    #[test]
    fn first_scan_creates_series_books_media_metadata_and_sidecars() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        write_file(&root.join("berserk"), "v01.cbz", b"book-one");
        write_file(&root.join("berserk"), "v02.cbz", b"book-two!");
        write_file(&root.join("berserk"), "cover.jpg", b"cover");
        write_file(&root.join("berserk"), "series.json", b"{}");
        write_file(&root.join("solo"), "v01.cbz", b"solo-book");
        let lib = library(&state.db, "lib1", &root);

        scan(&state, &lib);

        let series = all_series(&state);
        assert_eq!(series.len(), 2);
        let berserk = series.iter().find(|s| s.name == "berserk").unwrap();
        assert_eq!(berserk.book_count, 2);

        let books = all_books(&state);
        assert_eq!(books.len(), 3);
        // every book has media (UNKNOWN) and metadata
        for book in &books {
            let media = MediaDao::new(state.db.clone())
                .find_by_id(&book.id)
                .unwrap()
                .unwrap();
            assert_eq!(media.status, MediaStatus::Unknown);
            assert!(BookMetadataDao::new(state.db.clone())
                .find_by_id(&book.id)
                .unwrap()
                .is_some());
        }
        // natural sort renumbers v01 < v02
        let v01 = books
            .iter()
            .find(|b| b.name == "v01" && b.series_id == berserk.id)
            .unwrap();
        let v02 = books
            .iter()
            .find(|b| b.name == "v02" && b.series_id == berserk.id)
            .unwrap();
        assert!(v01.number < v02.number);

        // sidecars recorded: cover.jpg (ARTWORK/SERIES), series.json (METADATA/SERIES)
        let sidecars = SidecarDao::new(state.db.clone()).find_all().unwrap();
        assert_eq!(sidecars.len(), 2);
        assert!(sidecars.iter().any(|s| s.url.ends_with("cover.jpg")));
        assert!(sidecars.iter().any(|s| s.url.ends_with("series.json")));

        // sortBooks queued numbering refreshes and a series metadata refresh
        let task_ids: Vec<String> = TasksDao::new(state.tasks_db.clone())
            .find_all()
            .unwrap()
            .iter()
            .map(|t| t.unique_id())
            .collect();
        assert!(task_ids
            .iter()
            .any(|id| id.starts_with("REFRESH_SERIES_METADATA_")));
        assert!(task_ids
            .iter()
            .any(|id| id.starts_with("REFRESH_BOOK_METADATA_")));
        // sidecar refreshes
        assert!(task_ids
            .iter()
            .any(|id| id.starts_with("REFRESH_SERIES_LOCAL_ARTWORK_")));
    }

    #[test]
    fn second_scan_without_changes_is_a_no_op() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        write_file(&root.join("s1"), "v01.cbz", b"one");
        let lib = library(&state.db, "lib1", &root);
        scan(&state, &lib);
        let book_rows: i64 = state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM BOOK", [], |r| r.get(0))
            .unwrap();
        let media_rows: i64 = state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM MEDIA", [], |r| r.get(0))
            .unwrap();

        let mut rx = state.events.subscribe();
        scan(&state, &lib);

        let book_rows_after: i64 = state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM BOOK", [], |r| r.get(0))
            .unwrap();
        assert_eq!(book_rows, book_rows_after);
        let media_rows_after: i64 = state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM MEDIA", [], |r| r.get(0))
            .unwrap();
        assert_eq!(media_rows, media_rows_after);
        // only the LibraryScanned event fires; nothing else changed
        let mut saw_scanned = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                DomainEvent::LibraryScanned(_) => saw_scanned = true,
                other => panic!("unexpected event after no-op scan: {other:?}"),
            }
        }
        assert!(saw_scanned);
    }

    #[test]
    fn changed_book_resets_media_to_outdated() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        let book_path = write_file(&root.join("s1"), "v01.cbz", b"one");
        let lib = library(&state.db, "lib1", &root);
        scan(&state, &lib);
        let book = all_books(&state).into_iter().next().unwrap();

        // change content (size changes) and age the stored mtimes so the series and the book
        // both look changed to the scanner
        std::fs::write(&book_path, b"one-longer-content").unwrap();
        state
            .db
            .rw()
            .execute(
                "UPDATE BOOK SET FILE_LAST_MODIFIED = '2020-01-01 00:00:00.0' WHERE ID = ?",
                [&book.id],
            )
            .unwrap();
        state
            .db
            .rw()
            .execute(
                "UPDATE SERIES SET FILE_LAST_MODIFIED = '2020-01-01 00:00:00.0'",
                [],
            )
            .unwrap();

        scan(&state, &lib);

        let media = MediaDao::new(state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        assert_eq!(media.status, MediaStatus::Outdated);
    }

    #[test]
    fn changed_book_with_same_hash_keeps_media_ready() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        let book_path = write_file(&root.join("s1"), "v01.cbz", b"same-bytes");
        let lib = library(&state.db, "lib1", &root);
        scan(&state, &lib);
        let book = all_books(&state).into_iter().next().unwrap();

        // store the hash of the current content, then age the stored mtimes
        let hash = compute_hash(&book_path).unwrap();
        state.db.rw().execute(
            "UPDATE BOOK SET FILE_HASH = ?, FILE_LAST_MODIFIED = '2020-01-01 00:00:00.0' WHERE ID = ?",
            rusqlite::params![hash, book.id],
        ).unwrap();
        state
            .db
            .rw()
            .execute(
                "UPDATE SERIES SET FILE_LAST_MODIFIED = '2020-01-01 00:00:00.0'",
                [],
            )
            .unwrap();

        scan(&state, &lib);

        let media = MediaDao::new(state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        assert_eq!(media.status, MediaStatus::Unknown);
        let book = BookDao::new(state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        assert!(
            book.file_last_modified
                > time_codec::parse_datetime_utc("2020-01-01 00:00:00.0").unwrap()
        );
    }

    #[test]
    fn vanished_book_is_soft_deleted_and_series_resorted() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        write_file(&root.join("s1"), "v01.cbz", b"one");
        let path2 = write_file(&root.join("s1"), "v02.cbz", b"two!");
        let lib = library(&state.db, "lib1", &root);
        scan(&state, &lib);
        assert_eq!(all_books(&state).len(), 2);

        std::fs::remove_file(&path2).unwrap();
        scan(&state, &lib);

        let books = all_books(&state);
        let gone = books.iter().find(|b| b.name == "v02").unwrap();
        assert!(gone.deleted_date.is_some());
        let alive = books.iter().find(|b| b.name == "v01").unwrap();
        assert!(alive.deleted_date.is_none());
        let series = all_series(&state).into_iter().next().unwrap();
        // Kotlin's sortBooks counts soft-deleted books too (findAllBySeriesId has no trash filter)
        assert_eq!(series.book_count, 2);
    }

    #[test]
    fn vanished_series_is_soft_deleted_with_books() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        write_file(&root.join("s1"), "v01.cbz", b"one");
        write_file(&root.join("s2"), "v01.cbz", b"two");
        let lib = library(&state.db, "lib1", &root);
        scan(&state, &lib);
        assert_eq!(all_series(&state).len(), 2);

        std::fs::remove_dir_all(root.join("s1")).unwrap();
        scan(&state, &lib);

        let s1 = all_series(&state)
            .into_iter()
            .find(|s| s.name == "s1")
            .unwrap();
        assert!(s1.deleted_date.is_some());
        let book = all_books(&state)
            .into_iter()
            .find(|b| b.series_id == s1.id)
            .unwrap();
        assert!(book.deleted_date.is_some());
        let s2 = all_series(&state)
            .into_iter()
            .find(|s| s.name == "s2")
            .unwrap();
        assert!(s2.deleted_date.is_none());
    }

    #[test]
    fn moved_book_is_restored_by_hash() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        let book_path = write_file(&root.join("s1"), "v01.cbz", b"content-bytes");
        let lib = library(&state.db, "lib1", &root);
        let user = seed_user(&state.db, "a@b.c");
        scan(&state, &lib);

        // hash the book (HashBook is a later task; set it directly)
        let book = all_books(&state).into_iter().next().unwrap();
        let hash = compute_hash(&book_path).unwrap();
        state
            .db
            .rw()
            .execute(
                "UPDATE BOOK SET FILE_HASH = ? WHERE ID = ?",
                rusqlite::params![hash, book.id],
            )
            .unwrap();
        // give it READY media with a page, read progress, and a readlist
        let media_dao = MediaDao::new(state.db.clone());
        let mut media = media_dao.find_by_id(&book.id).unwrap().unwrap();
        media.status = MediaStatus::Ready;
        media.page_count = 42;
        media_dao.update(&media).unwrap();
        let progress_dao = ReadProgressDao::new(state.db.clone());
        progress_dao
            .insert_or_update(&komga_core::model::read_progress::ReadProgress {
                book_id: book.id.clone(),
                user_id: user.id.clone(),
                page: 42,
                completed: true,
                read_date: time_codec::now_utc(),
                device_id: String::new(),
                device_name: String::new(),
                locator: None,
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            })
            .unwrap();
        let readlist_dao = ReadListDao::new(state.db.clone());
        let readlist_id = readlist_dao
            .insert(&komga_core::model::readlist::ReadList {
                id: String::new(),
                name: "rl".into(),
                summary: String::new(),
                ordered: false,
                book_ids: [(0, book.id.clone())].into_iter().collect(),
                filtered: false,
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            })
            .unwrap();

        // the book "moves": old dir removed, same content elsewhere
        std::fs::remove_dir_all(root.join("s1")).unwrap();
        scan(&state, &lib);
        assert!(BookDao::new(state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap()
            .deleted_date
            .is_some());

        write_file(&root.join("s2"), "v01-renamed.cbz", b"content-bytes");
        scan(&state, &lib);

        let restored = all_books(&state)
            .into_iter()
            .find(|b| b.deleted_date.is_none())
            .unwrap();
        assert_eq!(restored.name, "v01-renamed");
        // media copied
        let media = media_dao.find_by_id(&restored.id).unwrap().unwrap();
        assert_eq!(media.status, MediaStatus::Ready);
        assert_eq!(media.page_count, 42);
        // progress copied
        let progress = progress_dao
            .find_by_book_and_user(&restored.id, &user.id)
            .unwrap()
            .unwrap();
        assert!(progress.completed);
        assert_eq!(progress.page, 42);
        // readlist now points at the restored book
        let readlist = readlist_dao.find_by_id(&readlist_id).unwrap().unwrap();
        assert_eq!(
            readlist.book_ids.values().collect::<Vec<_>>(),
            vec![&restored.id]
        );
        // the deleted book is gone for good
        assert!(BookDao::new(state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn moved_series_is_restored_with_locked_metadata_and_collections() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        write_file(&root.join("s1"), "v01.cbz", b"aaa");
        write_file(&root.join("s1"), "v02.cbz", b"bbbb");
        let lib = library(&state.db, "lib1", &root);
        scan(&state, &lib);

        let s1 = all_series(&state).into_iter().next().unwrap();
        // lock a custom title on the series metadata
        let metadata_dao = SeriesMetadataDao::new(state.db.clone());
        let mut metadata = metadata_dao.find_by_id(&s1.id).unwrap().unwrap();
        metadata.title = "Custom Title".into();
        metadata.title_lock = true;
        metadata_dao.update(&metadata).unwrap();
        // hash the books and put the series in a collection
        for book in all_books(&state) {
            let hash = compute_hash(std::path::Path::new(&komga_core::dto::url_to_file_path(
                &book.url,
            )))
            .unwrap();
            state
                .db
                .rw()
                .execute(
                    "UPDATE BOOK SET FILE_HASH = ? WHERE ID = ?",
                    rusqlite::params![hash, book.id],
                )
                .unwrap();
        }
        let collection_dao = CollectionDao::new(state.db.clone());
        let collection_id = collection_dao
            .insert(&komga_core::model::collection::SeriesCollection {
                id: String::new(),
                name: "col".into(),
                ordered: false,
                series_ids: vec![s1.id.clone()],
                filtered: false,
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            })
            .unwrap();
        // give books READY media so restore has something to copy
        let media_dao = MediaDao::new(state.db.clone());
        for book in all_books(&state) {
            let mut media = media_dao.find_by_id(&book.id).unwrap().unwrap();
            media.status = MediaStatus::Ready;
            media_dao.update(&media).unwrap();
        }

        // series "moves" to a new directory
        std::fs::remove_dir_all(root.join("s1")).unwrap();
        scan(&state, &lib);
        assert!(SeriesDao::new(state.db.clone())
            .find_by_id(&s1.id)
            .unwrap()
            .unwrap()
            .deleted_date
            .is_some());

        write_file(&root.join("s2"), "v01.cbz", b"aaa");
        write_file(&root.join("s2"), "v02.cbz", b"bbbb");
        scan(&state, &lib);

        let alive = all_series(&state)
            .into_iter()
            .find(|s| s.deleted_date.is_none())
            .unwrap();
        assert_eq!(alive.name, "s2");
        // locked title wins over the directory name
        let metadata = metadata_dao.find_by_id(&alive.id).unwrap().unwrap();
        assert_eq!(metadata.title, "Custom Title");
        // unlocked titleSort follows the new series
        assert_eq!(metadata.title_sort, "s2");
        // collection points at the restored series
        let collection = collection_dao.find_by_id(&collection_id).unwrap().unwrap();
        assert_eq!(collection.series_ids, vec![alive.id.clone()]);
        // books have their READY media back
        for book in BookDao::new(state.db.clone())
            .find_by_series_id(&alive.id)
            .unwrap()
        {
            let media = media_dao.find_by_id(&book.id).unwrap().unwrap();
            assert_eq!(media.status, MediaStatus::Ready);
        }
        // the deleted series is gone for good
        assert!(SeriesDao::new(state.db.clone())
            .find_by_id(&s1.id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn empty_trash_after_scan_hard_deletes() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        write_file(&root.join("s1"), "v01.cbz", b"one");
        let lib = library(&state.db, "lib1", &root);
        scan(&state, &lib);
        std::fs::remove_dir_all(root.join("s1")).unwrap();
        scan(&state, &lib);
        assert!(all_series(&state)
            .into_iter()
            .next()
            .unwrap()
            .deleted_date
            .is_some());

        // enable emptyTrashAfterScan
        state
            .db
            .rw()
            .execute(
                "UPDATE LIBRARY SET EMPTY_TRASH_AFTER_SCAN = 1 WHERE ID = 'lib1'",
                [],
            )
            .unwrap();
        let lib = LibraryDao::new(state.db.clone())
            .find_by_id("lib1")
            .unwrap()
            .unwrap();
        scan(&state, &lib);

        assert!(all_series(&state).is_empty());
        assert!(all_books(&state).is_empty());
    }

    #[test]
    fn cleanup_removes_empty_collections_and_readlists() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        write_file(&root.join("s1"), "v01.cbz", b"one");
        let lib = library(&state.db, "lib1", &root);

        let settings_dao = SettingsDao::new(state.db.clone());
        settings_dao
            .save_setting_bool("DELETE_EMPTY_COLLECTIONS", true)
            .unwrap();
        settings_dao
            .save_setting_bool("DELETE_EMPTY_READLISTS", true)
            .unwrap();
        state.settings.reload();

        let collection_dao = CollectionDao::new(state.db.clone());
        collection_dao
            .insert(&komga_core::model::collection::SeriesCollection {
                id: String::new(),
                name: "empty-col".into(),
                ordered: false,
                series_ids: vec![],
                filtered: false,
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            })
            .unwrap();
        let readlist_dao = ReadListDao::new(state.db.clone());
        readlist_dao
            .insert(&komga_core::model::readlist::ReadList {
                id: String::new(),
                name: "empty-rl".into(),
                summary: String::new(),
                ordered: false,
                book_ids: Default::default(),
                filtered: false,
                created_date: time_codec::now_utc(),
                last_modified_date: time_codec::now_utc(),
            })
            .unwrap();

        let mut rx = state.events.subscribe();
        scan(&state, &lib);

        assert!(collection_dao.find_all().unwrap().is_empty());
        assert!(readlist_dao.find_all().unwrap().is_empty());
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(events
            .iter()
            .any(|e| matches!(e, DomainEvent::CollectionDeleted(_))));
        assert!(events
            .iter()
            .any(|e| matches!(e, DomainEvent::ReadListDeleted(_))));
    }

    #[test]
    fn unavailable_library_is_marked_then_recovered() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let missing = scan_root(&tmp).join("missing");
        let lib = library(&state.db, "lib1", &missing);

        let mut rx = state.events.subscribe();
        let err = scan_root_folder(&state, &lib, false).unwrap_err();
        assert!(matches!(
            err,
            ScanRootError::Scan(ScanError::DirectoryNotFound(_))
        ));
        let lib_after = LibraryDao::new(state.db.clone())
            .find_by_id("lib1")
            .unwrap()
            .unwrap();
        assert!(lib_after.unavailable_date.is_some());
        assert!(std::iter::from_fn(|| rx.try_recv().ok())
            .any(|e| matches!(e, DomainEvent::LibraryUpdated(_))));

        // the root comes back: the flag is cleared on the next scan
        std::fs::create_dir_all(&missing).unwrap();
        write_file(&missing, "v01.cbz", b"one");
        scan(&state, &lib_after);
        let lib_recovered = LibraryDao::new(state.db.clone())
            .find_by_id("lib1")
            .unwrap()
            .unwrap();
        assert!(lib_recovered.unavailable_date.is_none());
    }

    #[test]
    fn oneshot_directory_creates_oneshot_series_per_book() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        write_file(&root.join("oneshots"), "story-a.cbz", b"aaa");
        write_file(&root.join("oneshots"), "story-b.cbz", b"bbb");
        library(&state.db, "lib1", &root);
        state
            .db
            .rw()
            .execute(
                "UPDATE LIBRARY SET ONESHOTS_DIRECTORY = 'oneshots' WHERE ID = 'lib1'",
                [],
            )
            .unwrap();
        let lib = LibraryDao::new(state.db.clone())
            .find_by_id("lib1")
            .unwrap()
            .unwrap();

        scan(&state, &lib);

        let series = all_series(&state);
        assert_eq!(series.len(), 2);
        assert!(series.iter().all(|s| s.oneshot));
        let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"story-a"));
        assert!(names.contains(&"story-b"));
        // books are oneshot too
        assert!(all_books(&state).iter().all(|b| b.oneshot));
    }

    #[test]
    fn scan_force_modified_time_takes_max_of_dir_and_books() {
        let state = test_state();
        let tmp = tempfile::tempdir().unwrap();
        let root = scan_root(&tmp);
        let book = write_file(&root.join("s1"), "v01.cbz", b"one");
        library(&state.db, "lib1", &root);
        state
            .db
            .rw()
            .execute(
                "UPDATE LIBRARY SET SCAN_FORCE_MODIFIED_TIME = 1 WHERE ID = 'lib1'",
                [],
            )
            .unwrap();
        let lib = LibraryDao::new(state.db.clone())
            .find_by_id("lib1")
            .unwrap()
            .unwrap();

        // make the book clearly newer than its parent directory
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        std::fs::File::open(&book)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let book_mtime_millis = later
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        scan(&state, &lib);

        let series = all_series(&state).into_iter().next().unwrap();
        let series_millis = (series.file_last_modified.unix_timestamp_nanos() / 1_000_000) as u128;
        assert_eq!(series_millis, book_mtime_millis);
    }
}
