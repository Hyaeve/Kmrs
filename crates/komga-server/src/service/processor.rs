//! `TaskProcessor.kt` / `TaskHandler.kt`: drives the tasks.sqlite queue.
//!
//! Queue lifecycle: claimed-but-unfinished tasks are disowned at startup, then a supervisor task
//! drains the queue whenever the emitter nudges it (`TaskAddedEvent` equivalent). Concurrency is
//! bounded by `task_pool_size`; GROUP_ID exclusion is enforced by the SQL in `take_first`, so
//! workers never run two tasks of the same group at once.

use crate::service::{book, library_content, series, TaskNotify};
use crate::state::AppState;
use komga_core::model::library::Library;
use komga_core::task::{BookMetadataPatchCapability, Task, LOWEST_PRIORITY};
use komga_db::dao::book::BookDao;
use komga_db::dao::library::LibraryDao;
use komga_db::dao::series::SeriesDao;
use komga_db::dao::tasks::TasksDao;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct TaskProcessor;

impl TaskProcessor {
    /// Disowns leftover tasks, then drives the queue in the background.
    /// The `notify` handle must be the one the `TaskEmitter` was built with.
    pub fn start(state: AppState, notify: TaskNotify) -> tokio::task::JoinHandle<()> {
        disown_at_startup(&state);
        tokio::spawn(async move {
            loop {
                drain_queue(&state).await;
                notify.notified().await;
            }
        })
    }
}

fn disown_at_startup(state: &AppState) {
    match TasksDao::new(state.tasks_db.clone()).disown() {
        Ok(n) if n > 0 => tracing::info!("Reset {n} tasks that were not finished"),
        Ok(_) => {}
        Err(e) => tracing::error!("Failed to disown tasks: {e}"),
    }
}

static WORKER_ID: AtomicU32 = AtomicU32::new(0);

/// Processes tasks until none are runnable, with at most `task_pool_size` concurrent workers.
async fn drain_queue(state: &AppState) {
    let pool_size = state.settings.get().task_pool_size.max(1) as usize;
    let mut set = tokio::task::JoinSet::new();
    loop {
        while set.len() < pool_size && has_available(state) {
            let st = state.clone();
            let id = WORKER_ID.fetch_add(1, Ordering::Relaxed) + 1;
            set.spawn(async move { process_one(&st, id).await });
        }
        if set.is_empty() {
            break;
        }
        if let Some(Err(e)) = set.join_next().await {
            tracing::error!("Task worker panicked: {e}");
        }
    }
}

fn has_available(state: &AppState) -> bool {
    TasksDao::new(state.tasks_db.clone())
        .has_available()
        .unwrap_or(false)
}

/// Claims and runs a single task, then removes it from the queue (success or failure alike).
async fn process_one(state: &AppState, worker: u32) {
    let owner = format!("taskProcessor-{worker}");
    let task = match TasksDao::new(state.tasks_db.clone()).take_first(&owner) {
        Ok(Some(task)) => task,
        Ok(None) => return,
        Err(e) => {
            tracing::error!("Failed to take task: {e}");
            return;
        }
    };
    let st = state.clone();
    let task_for_blocking = task.clone();
    // lifecycle work is synchronous and can run for minutes; keep it off the async threads
    let result = tokio::task::spawn_blocking(move || handle_task(&st, &task_for_blocking)).await;
    if let Err(e) = result {
        tracing::error!("Task {} execution panicked: {e}", task.describe());
    }
    if let Err(e) = TasksDao::new(state.tasks_db.clone()).delete(&task.unique_id()) {
        tracing::error!("Failed to delete task {}: {e}", task.unique_id());
    }
}

fn handle_task(state: &AppState, task: &Task) {
    tracing::info!("Executing task: {}", task.describe());
    hook_start(&task.unique_id());
    if let Err(e) = dispatch_task(state, task) {
        tracing::error!("Task {} execution failed: {e}", task.describe());
    }
    hook_end(&task.unique_id());
}

#[cfg(test)]
pub(crate) static HANDLE_LOG: std::sync::Mutex<
    Vec<(String, std::time::Instant, Option<std::time::Instant>)>,
> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn hook_start(id: &str) {
    HANDLE_LOG
        .lock()
        .unwrap()
        .push((id.to_string(), std::time::Instant::now(), None));
}

#[cfg(test)]
fn hook_end(id: &str) {
    let mut log = HANDLE_LOG.lock().unwrap();
    if let Some(entry) = log.iter_mut().rev().find(|e| e.0 == id && e.2.is_none()) {
        entry.2 = Some(std::time::Instant::now());
    }
}

#[cfg(not(test))]
fn hook_start(_id: &str) {}

#[cfg(not(test))]
fn hook_end(_id: &str) {}

pub(crate) fn dispatch_task(state: &AppState, task: &Task) -> anyhow::Result<()> {
    match task {
        Task::ScanLibrary(t) => {
            let Some(library) = LibraryDao::new(state.db.clone()).find_by_id(&t.library_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Library does not exist",
                    task.describe()
                );
                return Ok(());
            };
            library_content::scan_root_folder(state, &library, t.scan_deep)?;
            let emitter = &state.task_emitter;
            emitter.analyze_unknown_and_outdated_books(&library)?;
            if library.repair_extensions {
                // the converter lands in M8; there is nothing to submit
                tracing::debug!(
                    "repairExtensions is enabled for library {} but is deferred to M8",
                    library.id
                );
            }
            emitter.find_books_to_convert(&library.id, LOWEST_PRIORITY)?;
            emitter.find_books_with_missing_page_hash(&library.id, LOWEST_PRIORITY)?;
            emitter.find_duplicate_pages_to_delete(&library.id, LOWEST_PRIORITY)?;
            emitter.hash_books_without_hash(&library)?;
            emitter.hash_books_without_hash_koreader(&library)?;
            Ok(())
        }
        Task::EmptyTrash(t) => {
            let Some(library) = LibraryDao::new(state.db.clone()).find_by_id(&t.library_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Library does not exist",
                    task.describe()
                );
                return Ok(());
            };
            library_content::empty_trash(state, &library)?;
            Ok(())
        }
        Task::AnalyzeBook(t) => {
            let Some(book) = BookDao::new(state.db.clone()).find_by_id(&t.book_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Book does not exist",
                    task.describe()
                );
                return Ok(());
            };
            let actions = book::analyze_and_persist(state, &book)?;
            if actions.contains(&book::BookAction::GenerateThumbnail) {
                state
                    .task_emitter
                    .generate_book_thumbnail(&book.id, t.priority + 1)?;
            }
            if actions.contains(&book::BookAction::RefreshMetadata) {
                state.task_emitter.refresh_book_metadata(
                    &book,
                    BookMetadataPatchCapability::all(),
                    t.priority + 1,
                )?;
            }
            Ok(())
        }
        Task::GenerateBookThumbnail(t) => {
            let Some(book) = BookDao::new(state.db.clone()).find_by_id(&t.book_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Book does not exist",
                    task.describe()
                );
                return Ok(());
            };
            book::generate_thumbnail_and_persist(state, &book)?;
            Ok(())
        }
        Task::RefreshBookMetadata(t) => {
            let Some(book) = BookDao::new(state.db.clone()).find_by_id(&t.book_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Book does not exist",
                    task.describe()
                );
                return Ok(());
            };
            crate::service::metadata::refresh_book_metadata(state, &book, &t.capabilities)?;
            state
                .task_emitter
                .refresh_series_metadata(&book.series_id, t.priority - 1)?;
            Ok(())
        }
        Task::RefreshSeriesMetadata(t) => {
            let Some(series) = SeriesDao::new(state.db.clone()).find_by_id(&t.series_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Series does not exist",
                    task.describe()
                );
                return Ok(());
            };
            crate::service::metadata::refresh_series_metadata(state, &series)?;
            state
                .task_emitter
                .aggregate_series_metadata(&t.series_id, t.priority)?;
            Ok(())
        }
        Task::AggregateSeriesMetadata(t) => {
            let Some(series) = SeriesDao::new(state.db.clone()).find_by_id(&t.series_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Series does not exist",
                    task.describe()
                );
                return Ok(());
            };
            crate::service::metadata::aggregate_series_metadata(state, &series)?;
            Ok(())
        }
        Task::RefreshBookLocalArtwork(t) => {
            let Some(book) = BookDao::new(state.db.clone()).find_by_id(&t.book_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Book does not exist",
                    task.describe()
                );
                return Ok(());
            };
            crate::service::metadata::refresh_book_local_artwork(state, &book)?;
            Ok(())
        }
        Task::RefreshSeriesLocalArtwork(t) => {
            let Some(series) = SeriesDao::new(state.db.clone()).find_by_id(&t.series_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Series does not exist",
                    task.describe()
                );
                return Ok(());
            };
            crate::service::metadata::refresh_series_local_artwork(state, &series)?;
            Ok(())
        }
        Task::HashBook(t) => {
            let Some(book) = BookDao::new(state.db.clone()).find_by_id(&t.book_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Book does not exist",
                    task.describe()
                );
                return Ok(());
            };
            book::hash_and_persist(state, &book)?;
            Ok(())
        }
        Task::HashBookKoreader(t) => {
            let Some(book) = BookDao::new(state.db.clone()).find_by_id(&t.book_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Book does not exist",
                    task.describe()
                );
                return Ok(());
            };
            book::hash_koreader_and_persist(state, &book)?;
            Ok(())
        }
        Task::HashBookPages(t) => {
            let Some(book) = BookDao::new(state.db.clone()).find_by_id(&t.book_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Book does not exist",
                    task.describe()
                );
                return Ok(());
            };
            book::hash_pages_and_persist(state, &book)?;
            Ok(())
        }
        Task::FindBookThumbnailsToRegenerate(t) => {
            let ids = book::find_book_thumbnails_to_regenerate(state, t.for_bigger_result_only)?;
            state
                .task_emitter
                .generate_book_thumbnails(&ids, t.priority)?;
            Ok(())
        }
        Task::FindBooksWithMissingPageHash(t) => {
            let Some(library) = LibraryDao::new(state.db.clone()).find_by_id(&t.library_id)? else {
                tracing::warn!(
                    "Cannot execute task {}: Library does not exist",
                    task.describe()
                );
                return Ok(());
            };
            let ids = book_ids_with_missing_page_hash(state, &library)?;
            state.task_emitter.hash_book_pages(&ids, t.priority + 1)?;
            Ok(())
        }
        Task::FindDuplicatePagesToDelete(_) => {
            tracing::warn!(
                "FindDuplicatePagesToDelete is not implemented until M8 (page hash management)"
            );
            Ok(())
        }
        Task::FindBooksToConvert(_) | Task::ConvertBook(_) | Task::RepairExtension(_) => {
            tracing::warn!("Book conversion is not implemented until M8 (BookConverter)");
            Ok(())
        }
        Task::RemoveHashedPages(_) => {
            tracing::warn!("RemoveHashedPages is not implemented until M8 (BookPageEditor)");
            Ok(())
        }
        Task::ImportBook(_) => {
            tracing::warn!("ImportBook is not implemented until M8 (BookImporter)");
            Ok(())
        }
        Task::RebuildIndex(t) => {
            crate::search_index::rebuild_index(state, t.entities.clone());
            Ok(())
        }
        Task::UpgradeIndex(_) => {
            crate::search_index::upgrade_index(state);
            Ok(())
        }
        Task::DeleteBook(t) => {
            // Kotlin's DeleteBook branch has no warn log for a missing book
            let Some(book) = BookDao::new(state.db.clone()).find_by_id(&t.book_id)? else {
                return Ok(());
            };
            if book.oneshot {
                let series = SeriesDao::new(state.db.clone())
                    .find_by_id(&book.series_id)?
                    .expect("book references a missing series");
                series::delete_series_files(state, &series)?;
            } else {
                book::delete_book_files(state, &book)?;
            }
            Ok(())
        }
        Task::DeleteSeries(t) => {
            let Some(series) = SeriesDao::new(state.db.clone()).find_by_id(&t.series_id)? else {
                return Ok(());
            };
            series::delete_series_files(state, &series)?;
            Ok(())
        }
    }
}

/// `PageHashLifecycle.getBookIdsWithMissingPageHash`: zip-profile books with fewer hashed pages
/// than `min(page_count, pageHashing * 2)`; empty when the library does not hash pages.
fn book_ids_with_missing_page_hash(
    state: &AppState,
    library: &Library,
) -> komga_db::Result<Vec<String>> {
    if !library.hash_pages {
        tracing::info!("Page hashing is not enabled, skipping");
        return Ok(vec![]);
    }
    let conn = state.db.ro();
    let needed = (state.config.page_hashing * 2) as i64;
    let mut stmt = conn.prepare(
        "SELECT BOOK.ID FROM BOOK \
         LEFT JOIN MEDIA_PAGE ON BOOK.ID = MEDIA_PAGE.BOOK_ID \
         LEFT JOIN MEDIA ON BOOK.ID = MEDIA.BOOK_ID \
         WHERE BOOK.LIBRARY_ID = ? AND MEDIA.STATUS = 'READY' AND MEDIA.MEDIA_TYPE IN ('application/zip') \
         GROUP BY BOOK.ID \
         HAVING SUM(CASE WHEN MEDIA_PAGE.FILE_HASH = '' THEN 0 ELSE 1 END) < \
         CASE WHEN COUNT(MEDIA_PAGE.BOOK_ID) < ? THEN COUNT(MEDIA_PAGE.BOOK_ID) ELSE ? END",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![library.id, needed, needed], |r| r.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    tracing::info!("Found {} books with missing page hash", ids.len());
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::series::tests as series_tests;
    use komga_core::model::book::Book;
    use komga_core::model::library::{ScanInterval, SeriesCover};
    use komga_core::model::media::{Media, MediaStatus};
    use komga_core::task::{BookTaskKind, LibraryTaskKind, SeriesTaskKind, DEFAULT_PRIORITY};
    use komga_core::time_codec::{format_datetime, now_utc};
    use komga_db::dao::media::MediaDao;
    use komga_db::pool::Database;
    use std::path::Path;

    fn test_library(id: &str, root: &str) -> Library {
        let now = now_utc();
        Library {
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

    fn seed_library(db: &Database, library: &Library) {
        LibraryDao::new(db.clone()).insert(library).unwrap();
    }

    fn seed_series(db: &Database, library_id: &str, id: &str) {
        db.rw()
            .execute(
                "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES (?, ?, ?, ?, ?)",
                rusqlite::params![id, "S", "file:/l/s/", format_datetime(now_utc()), library_id],
            )
            .unwrap();
    }

    fn seed_book(db: &Database, library_id: &str, series_id: &str, url: &str) -> Book {
        let now = now_utc();
        let book = Book {
            id: String::new(),
            name: "v01".into(),
            url: url.into(),
            file_last_modified: now,
            series_id: series_id.into(),
            library_id: library_id.into(),
            file_size: 0,
            number: 1,
            file_hash: String::new(),
            file_hash_koreader: String::new(),
            deleted_date: None,
            oneshot: false,
            created_date: now,
            last_modified_date: now,
        };
        let id = BookDao::new(db.clone()).insert(&book).unwrap();
        let book = Book { id, ..book };
        MediaDao::new(db.clone())
            .insert(&Media {
                book_id: book.id.clone(),
                status: MediaStatus::Unknown,
                media_type: None,
                comment: None,
                page_count: 0,
                pages: vec![],
                files: vec![],
                extension_class: None,
                extension_value: None,
                epub_divina_compatible: false,
                epub_is_kepub: false,
                created_date: now,
                last_modified_date: now,
            })
            .unwrap();
        book
    }

    fn fixture_zip(dest: &Path) -> String {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../komga/komga/src/test/resources/archives/zip.zip");
        std::fs::copy(&src, dest).unwrap();
        dest.display().to_string()
    }

    /// A visible (non-hidden) subdirectory inside a temp dir: the scanner skips dot-dirs.
    fn visible_tempdir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("komga-rs-processor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn wait_until<F: Fn() -> bool>(f: F) -> bool {
        for _ in 0..400 {
            if f() {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        false
    }

    fn queue_empty(state: &AppState) -> bool {
        TasksDao::new(state.tasks_db.clone()).count().unwrap_or(1) == 0
    }

    fn task_ids(state: &AppState) -> Vec<String> {
        TasksDao::new(state.tasks_db.clone())
            .find_all()
            .unwrap()
            .iter()
            .map(|t| t.unique_id())
            .collect()
    }

    fn thumbnail_count(state: &AppState) -> i64 {
        state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM THUMBNAIL_BOOK", [], |r| r.get(0))
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn disown_at_startup_resets_claimed() {
        let state = series_tests::test_state();
        let dao = TasksDao::new(state.tasks_db.clone());
        dao.save(&Task::scan_library("lib1", false, DEFAULT_PRIORITY))
            .unwrap();
        // claim it so it has an owner
        assert!(dao.take_first("dead-worker").unwrap().is_some());
        assert!(!dao.has_available().unwrap());

        disown_at_startup(&state);
        assert!(dao.has_available().unwrap());
    }

    #[tokio::test]
    async fn scan_library_end_to_end() {
        let state = series_tests::test_state();
        let root = visible_tempdir("e2e");
        let book_dir = root.join("series1");
        std::fs::create_dir_all(&book_dir).unwrap();
        fixture_zip(&book_dir.join("v01.cbz"));
        let library = test_library("lib-e2e", &format!("file:{}/", root.display()));
        seed_library(&state.db, &library);

        let notify: TaskNotify = std::sync::Arc::new(tokio::sync::Notify::new());
        let emitter = crate::service::TaskEmitter::new(
            state.db.clone(),
            state.tasks_db.clone(),
            notify.clone(),
        );
        let handle = TaskProcessor::start(state.clone(), notify);
        emitter
            .scan_library("lib-e2e", false, komga_core::task::HIGHEST_PRIORITY)
            .unwrap();

        let done = wait_until(|| queue_empty(&state) && thumbnail_count(&state) > 0).await;
        handle.abort();
        assert!(
            done,
            "queue did not drain in time; pending: {:?}",
            task_ids(&state)
        );

        let series_count: i64 = state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM SERIES", [], |r| r.get(0))
            .unwrap();
        assert_eq!(series_count, 1);
        let (status, page_count): (String, i32) = state
            .db
            .ro()
            .query_row("SELECT STATUS, PAGE_COUNT FROM MEDIA", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(status, "READY");
        assert_eq!(page_count, 1);
        let page_rows: i64 = state
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM MEDIA_PAGE", [], |r| r.get(0))
            .unwrap();
        assert_eq!(page_rows, 1);
        // HashBook derived from the scan chain
        let file_hash: String = state
            .db
            .ro()
            .query_row("SELECT FILE_HASH FROM BOOK", [], |r| r.get(0))
            .unwrap();
        assert!(!file_hash.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn same_group_never_concurrent() {
        let state = series_tests::test_state();
        state
            .db
            .rw()
            .execute(
                "INSERT INTO SERVER_SETTINGS (KEY, VALUE) VALUES ('TASK_POOL_SIZE', '2')",
                [],
            )
            .unwrap();
        state.settings.reload();

        let root = visible_tempdir("group");
        seed_library(
            &state.db,
            &test_library("lib-g", &format!("file:{}/", root.display())),
        );
        seed_series(&state.db, "lib-g", "s1");
        seed_series(&state.db, "lib-g", "s2");
        let mut book_ids = vec![];
        for i in 0..3 {
            let path = root.join(format!("b{i}.cbz"));
            fixture_zip(&path);
            book_ids.push(seed_book(
                &state.db,
                "lib-g",
                "s1",
                &format!("file:{}", path.display()),
            ));
        }
        let other = seed_book(
            &state.db,
            "lib-g",
            "s2",
            &format!("file:{}", root.join("other.cbz").display()),
        );
        fixture_zip(&root.join("other.cbz"));

        let notify: TaskNotify = std::sync::Arc::new(tokio::sync::Notify::new());
        let emitter = crate::service::TaskEmitter::new(
            state.db.clone(),
            state.tasks_db.clone(),
            notify.clone(),
        );
        HANDLE_LOG.lock().unwrap().clear();
        let handle = TaskProcessor::start(state.clone(), notify);
        for book in &book_ids {
            emitter.analyze_book(book, DEFAULT_PRIORITY).unwrap();
        }
        emitter.analyze_book(&other, DEFAULT_PRIORITY).unwrap();

        let ids: Vec<String> = book_ids.iter().map(|b| b.id.clone()).collect();
        let done = wait_until(|| {
            let log = HANDLE_LOG.lock().unwrap();
            ids.iter().all(|id| {
                log.iter()
                    .any(|e| e.0 == format!("ANALYZE_BOOK_{id}") && e.2.is_some())
            }) && queue_empty(&state)
        })
        .await;
        handle.abort();
        assert!(done, "analyze tasks did not finish in time");

        let log = HANDLE_LOG.lock().unwrap();
        let mut intervals: Vec<(std::time::Instant, std::time::Instant)> = ids
            .iter()
            .filter_map(|id| {
                log.iter()
                    .find(|e| e.0 == format!("ANALYZE_BOOK_{id}"))
                    .map(|e| (e.1, e.2.unwrap()))
            })
            .collect();
        intervals.sort();
        for pair in intervals.windows(2) {
            assert!(
                pair[1].0 >= pair[0].1,
                "same-group tasks overlapped: {:?}",
                intervals
            );
        }
    }

    #[tokio::test]
    async fn failing_task_does_not_block_queue() {
        let state = series_tests::test_state();
        let root = visible_tempdir("fail");
        seed_library(
            &state.db,
            &test_library("lib-f", &format!("file:{}/", root.display())),
        );
        seed_series(&state.db, "lib-f", "s1");
        let book = seed_book(
            &state.db,
            "lib-f",
            "s1",
            &format!("file:{}", root.join("v01.cbz").display()),
        );
        fixture_zip(&root.join("v01.cbz"));

        let notify: TaskNotify = std::sync::Arc::new(tokio::sync::Notify::new());
        let emitter = crate::service::TaskEmitter::new(
            state.db.clone(),
            state.tasks_db.clone(),
            notify.clone(),
        );
        let handle = TaskProcessor::start(state.clone(), notify);
        // analyze a book that does not exist: warns and is discarded
        emitter
            .submit(Task::analyze_book(
                "no-such-book",
                DEFAULT_PRIORITY,
                "s1".into(),
            ))
            .unwrap();
        emitter
            .submit(Task::book(
                BookTaskKind::HashBook,
                &book.id,
                DEFAULT_PRIORITY,
                None,
            ))
            .unwrap();

        let done = wait_until(|| queue_empty(&state)).await;
        handle.abort();
        assert!(done, "queue did not drain: {:?}", task_ids(&state));
        let hash: String = state
            .db
            .ro()
            .query_row("SELECT FILE_HASH FROM BOOK WHERE ID = ?", [&book.id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!hash.is_empty());
    }

    #[test]
    fn refresh_book_metadata_derives_series_refresh() {
        let state = series_tests::test_state();
        seed_library(&state.db, &test_library("lib-r", "file:/l/"));
        seed_series(&state.db, "lib-r", "s1");
        let book = seed_book(&state.db, "lib-r", "s1", "file:/l/s/v01.cbz");

        dispatch_task(
            &state,
            &Task::refresh_book_metadata(
                &book.id,
                BookMetadataPatchCapability::all(),
                DEFAULT_PRIORITY,
                book.series_id.clone(),
            ),
        )
        .unwrap();

        let ids = task_ids(&state);
        assert_eq!(ids, vec!["REFRESH_SERIES_METADATA_s1".to_string()]);

        // and the derived series refresh derives the aggregate in turn
        let task = TasksDao::new(state.tasks_db.clone())
            .take_first("test-worker")
            .unwrap()
            .unwrap();
        assert_eq!(task.unique_id(), "REFRESH_SERIES_METADATA_s1");
        dispatch_task(&state, &task).unwrap();
        TasksDao::new(state.tasks_db.clone())
            .delete(&task.unique_id())
            .unwrap();
        let ids = task_ids(&state);
        assert_eq!(ids, vec!["AGGREGATE_SERIES_METADATA_s1".to_string()]);
    }

    #[test]
    fn missing_page_hash_sql() {
        let state = series_tests::test_state();
        let root = visible_tempdir("mph");
        seed_library(
            &state.db,
            &test_library("lib-m", &format!("file:{}/", root.display())),
        );
        seed_series(&state.db, "lib-m", "s1");
        seed_series(&state.db, "lib-m", "s2");
        seed_series(&state.db, "lib-m", "s3");
        seed_series(&state.db, "lib-m", "s4");
        let now = format_datetime(now_utc());
        // book with missing hashes (zip)
        let b1 = seed_book(&state.db, "lib-m", "s1", "file:/x/b1.cbz");
        // book fully hashed (zip)
        let b2 = seed_book(&state.db, "lib-m", "s2", "file:/x/b2.cbz");
        // pdf book with missing hashes: not a hashable media type
        let b3 = seed_book(&state.db, "lib-m", "s3", "file:/x/b3.pdf");
        // book not READY
        let b4 = seed_book(&state.db, "lib-m", "s4", "file:/x/b4.cbz");
        let dao = MediaDao::new(state.db.clone());
        for (book, status, media_type) in [
            (&b1, MediaStatus::Ready, "application/zip"),
            (&b2, MediaStatus::Ready, "application/zip"),
            (&b3, MediaStatus::Ready, "application/pdf"),
            (&b4, MediaStatus::Unknown, "application/zip"),
        ] {
            dao.update(&Media {
                book_id: book.id.clone(),
                status,
                media_type: Some(media_type.into()),
                comment: None,
                page_count: 4,
                pages: vec![],
                files: vec![],
                extension_class: None,
                extension_value: None,
                epub_divina_compatible: false,
                epub_is_kepub: false,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
        }
        let insert_page = |book_id: &str, number: i32, hash: &str| {
            state
                .db
                .rw()
                .execute(
                    "INSERT INTO MEDIA_PAGE (BOOK_ID, FILE_NAME, MEDIA_TYPE, NUMBER, FILE_HASH) VALUES (?, ?, 'image/png', ?, ?)",
                    rusqlite::params![book_id, format!("p{number}.png"), number, hash],
                )
                .unwrap();
        };
        for i in 0..4 {
            insert_page(&b1.id, i, if i == 0 { "h" } else { "" });
            insert_page(&b2.id, i, "h");
            insert_page(&b3.id, i, "");
            insert_page(&b4.id, i, "");
        }
        let _ = now;

        let library = LibraryDao::new(state.db.clone())
            .find_by_id("lib-m")
            .unwrap()
            .unwrap();
        let ids = book_ids_with_missing_page_hash(&state, &library).unwrap();
        assert_eq!(ids, vec![b1.id.clone()]);

        // dispatching the task submits HashBookPages for the found book
        dispatch_task(
            &state,
            &Task::library(
                LibraryTaskKind::FindBooksWithMissingPageHash,
                "lib-m",
                DEFAULT_PRIORITY,
            ),
        )
        .unwrap();
        assert_eq!(task_ids(&state), vec![format!("HASH_BOOK_PAGES_{}", b1.id)]);
    }

    #[test]
    fn aggregate_and_local_artwork_are_deferred() {
        let state = series_tests::test_state();
        seed_library(&state.db, &test_library("lib-d", "file:/l/"));
        seed_series(&state.db, "lib-d", "s1");
        // these warn and complete without submitting anything
        dispatch_task(
            &state,
            &Task::series(
                SeriesTaskKind::AggregateSeriesMetadata,
                "s1",
                DEFAULT_PRIORITY,
            ),
        )
        .unwrap();
        dispatch_task(
            &state,
            &Task::series(
                SeriesTaskKind::RefreshSeriesLocalArtwork,
                "s1",
                DEFAULT_PRIORITY,
            ),
        )
        .unwrap();
        assert!(task_ids(&state).is_empty());
    }
}
