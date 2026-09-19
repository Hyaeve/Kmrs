//! `TaskEmitter.kt`: submits tasks to the queue (tasks.sqlite) and nudges the processor.

use komga_core::model::book::Book;
use komga_core::model::library::Library;
use komga_core::task::{
    BookMetadataPatchCapability, BookTaskKind, CopyMode, LibraryTaskKind, LuceneEntity,
    SeriesTaskKind, Task, DEFAULT_PRIORITY,
};
use komga_db::dao::tasks::TasksDao;
use komga_db::pool::Database;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Wakes the task processor after a save, like `TaskAddedEvent` wakes the Java executor.
pub type TaskNotify = Arc<tokio::sync::Notify>;

#[derive(Clone)]
pub struct TaskEmitter {
    db: Database,
    tasks_db: Database,
    notify: TaskNotify,
}

impl TaskEmitter {
    pub fn new(db: Database, tasks_db: Database, notify: TaskNotify) -> Self {
        Self {
            db,
            tasks_db,
            notify,
        }
    }

    pub fn submit(&self, task: Task) -> komga_db::Result<()> {
        tracing::info!("Sending task: {}", task.describe());
        TasksDao::new(self.tasks_db.clone()).save(&task)?;
        self.notify.notify_one();
        Ok(())
    }

    pub fn submit_many(&self, tasks: &[Task]) -> komga_db::Result<()> {
        if tasks.is_empty() {
            return Ok(());
        }
        tracing::info!("Sending {} tasks", tasks.len());
        TasksDao::new(self.tasks_db.clone()).save_many(tasks)?;
        self.notify.notify_one();
        Ok(())
    }

    pub fn scan_library(
        &self,
        library_id: &str,
        scan_deep: bool,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::scan_library(library_id, scan_deep, priority))
    }

    pub fn empty_trash(&self, library_id: &str, priority: i32) -> komga_db::Result<()> {
        self.submit(Task::library(
            LibraryTaskKind::EmptyTrash,
            library_id,
            priority,
        ))
    }

    /// UNKNOWN and OUTDATED books of the library, sorted by (seriesId, number)
    pub fn analyze_unknown_and_outdated_books(&self, library: &Library) -> komga_db::Result<()> {
        use komga_core::search::*;
        let books = komga_db::dao::book::BookDao::new(self.db.clone()).find_all_by_condition(
            Some(&SearchConditionBook::AllOf {
                conditions: vec![
                    SearchConditionBook::LibraryId {
                        operator: Equality::Is {
                            value: library.id.clone(),
                        },
                    },
                    SearchConditionBook::AnyOf {
                        conditions: vec![
                            SearchConditionBook::MediaStatus {
                                operator: Equality::Is {
                                    value: komga_core::model::media::MediaStatus::Unknown,
                                },
                            },
                            SearchConditionBook::MediaStatus {
                                operator: Equality::Is {
                                    value: komga_core::model::media::MediaStatus::Outdated,
                                },
                            },
                        ],
                    },
                ],
            }),
            &SearchContext::default(),
            &[
                komga_db::dto_dao::SortOrder {
                    property: "seriesId".to_string(),
                    descending: false,
                },
                komga_db::dto_dao::SortOrder {
                    property: "number".to_string(),
                    descending: false,
                },
            ],
        )?;
        let tasks: Vec<Task> = books
            .iter()
            .map(|b| Task::analyze_book(&b.id, DEFAULT_PRIORITY, b.series_id.clone()))
            .collect();
        self.submit_many(&tasks)
    }

    pub fn hash_books_without_hash(&self, library: &Library) -> komga_db::Result<()> {
        if !library.hash_files {
            return Ok(());
        }
        let books = komga_db::dao::book::BookDao::new(self.db.clone())
            .find_all_by_library_id_and_with_empty_hash(&library.id)?;
        let tasks: Vec<Task> = books
            .iter()
            .map(|b| {
                Task::book(
                    BookTaskKind::HashBook,
                    &b.id,
                    komga_core::task::LOWEST_PRIORITY,
                    None,
                )
            })
            .collect();
        self.submit_many(&tasks)
    }

    pub fn hash_books_without_hash_koreader(&self, library: &Library) -> komga_db::Result<()> {
        if !library.hash_koreader {
            return Ok(());
        }
        let books = komga_db::dao::book::BookDao::new(self.db.clone())
            .find_all_by_library_id_and_with_empty_hash_koreader(&library.id)?;
        let tasks: Vec<Task> = books
            .iter()
            .map(|b| {
                Task::book(
                    BookTaskKind::HashBookKoreader,
                    &b.id,
                    komga_core::task::LOWEST_PRIORITY,
                    None,
                )
            })
            .collect();
        self.submit_many(&tasks)
    }

    pub fn find_books_with_missing_page_hash(
        &self,
        library_id: &str,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::library(
            LibraryTaskKind::FindBooksWithMissingPageHash,
            library_id,
            priority,
        ))
    }

    pub fn hash_book_pages(&self, book_ids: &[String], priority: i32) -> komga_db::Result<()> {
        let tasks: Vec<Task> = book_ids
            .iter()
            .map(|id| Task::book(BookTaskKind::HashBookPages, id, priority, None))
            .collect();
        self.submit_many(&tasks)
    }

    pub fn find_books_to_convert(&self, library_id: &str, priority: i32) -> komga_db::Result<()> {
        self.submit(Task::library(
            LibraryTaskKind::FindBooksToConvert,
            library_id,
            priority,
        ))
    }

    pub fn find_duplicate_pages_to_delete(
        &self,
        library_id: &str,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::library(
            LibraryTaskKind::FindDuplicatePagesToDelete,
            library_id,
            priority,
        ))
    }

    pub fn analyze_book(&self, book: &Book, priority: i32) -> komga_db::Result<()> {
        self.submit(Task::analyze_book(
            &book.id,
            priority,
            book.series_id.clone(),
        ))
    }

    pub fn analyze_books(&self, books: &[Book], priority: i32) -> komga_db::Result<()> {
        let tasks: Vec<Task> = books
            .iter()
            .map(|b| Task::analyze_book(&b.id, priority, b.series_id.clone()))
            .collect();
        self.submit_many(&tasks)
    }

    pub fn generate_book_thumbnail(&self, book_id: &str, priority: i32) -> komga_db::Result<()> {
        self.submit(Task::book(
            BookTaskKind::GenerateBookThumbnail,
            book_id,
            priority,
            None,
        ))
    }

    pub fn generate_book_thumbnails(
        &self,
        book_ids: &[String],
        priority: i32,
    ) -> komga_db::Result<()> {
        let tasks: Vec<Task> = book_ids
            .iter()
            .map(|id| Task::book(BookTaskKind::GenerateBookThumbnail, id, priority, None))
            .collect();
        self.submit_many(&tasks)
    }

    pub fn refresh_book_metadata(
        &self,
        book: &Book,
        capabilities: BTreeSet<BookMetadataPatchCapability>,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::refresh_book_metadata(
            &book.id,
            capabilities,
            priority,
            book.series_id.clone(),
        ))
    }

    pub fn refresh_series_metadata(&self, series_id: &str, priority: i32) -> komga_db::Result<()> {
        self.submit(Task::series(
            SeriesTaskKind::RefreshSeriesMetadata,
            series_id,
            priority,
        ))
    }

    pub fn aggregate_series_metadata(
        &self,
        series_id: &str,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::series(
            SeriesTaskKind::AggregateSeriesMetadata,
            series_id,
            priority,
        ))
    }

    pub fn refresh_book_local_artwork(&self, book: &Book, priority: i32) -> komga_db::Result<()> {
        self.submit(Task::book(
            BookTaskKind::RefreshBookLocalArtwork,
            &book.id,
            priority,
            None,
        ))
    }

    pub fn refresh_books_local_artwork(
        &self,
        books: &[Book],
        priority: i32,
    ) -> komga_db::Result<()> {
        let tasks: Vec<Task> = books
            .iter()
            .map(|b| Task::book(BookTaskKind::RefreshBookLocalArtwork, &b.id, priority, None))
            .collect();
        self.submit_many(&tasks)
    }

    pub fn refresh_series_local_artwork(
        &self,
        series_id: &str,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::series(
            SeriesTaskKind::RefreshSeriesLocalArtwork,
            series_id,
            priority,
        ))
    }

    pub fn import_book(
        &self,
        source_file: &str,
        series_id: &str,
        copy_mode: CopyMode,
        destination_name: Option<&str>,
        upgrade_book_id: Option<&str>,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::ImportBook(komga_core::task::ImportBook {
            source_file: source_file.to_string(),
            series_id: series_id.to_string(),
            copy_mode,
            destination_name: destination_name.map(str::to_string),
            upgrade_book_id: upgrade_book_id.map(str::to_string),
            priority,
            group_id: Some(series_id.to_string()),
            unique_id: String::new(),
        }))
    }

    pub fn rebuild_index(
        &self,
        entities: Option<BTreeSet<LuceneEntity>>,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::RebuildIndex(komga_core::task::RebuildIndex {
            entities,
            priority,
            group_id: None,
            unique_id: String::new(),
        }))
    }

    pub fn delete_book(&self, book_id: &str, priority: i32) -> komga_db::Result<()> {
        self.submit(Task::book(
            BookTaskKind::DeleteBook,
            book_id,
            priority,
            None,
        ))
    }

    pub fn delete_series(&self, series_id: &str, priority: i32) -> komga_db::Result<()> {
        self.submit(Task::series(
            SeriesTaskKind::DeleteSeries,
            series_id,
            priority,
        ))
    }

    pub fn find_book_thumbnails_to_regenerate(
        &self,
        for_bigger_result_only: bool,
        priority: i32,
    ) -> komga_db::Result<()> {
        self.submit(Task::FindBookThumbnailsToRegenerate(
            komga_core::task::FindBookThumbnailsToRegenerate {
                for_bigger_result_only,
                priority,
                group_id: None,
                unique_id: String::new(),
            },
        ))
    }
}
