//! Background task model, ported from `application/tasks/Task.kt`.
//!
//! Tasks are persisted in `tasks.sqlite`'s `TASK` table: `CLASS` holds the Java FQN
//! (`org.gotson.komga.application.tasks.Task$<SimpleType>`), `SIMPLE_TYPE` the class simple name,
//! and `PAYLOAD` the Jackson JSON of the task (all properties, including inherited
//! `priority`/`groupId`/`uniqueId`). Field names must match Jackson's output exactly so Java komga
//! can read tasks written by kmrs and vice versa.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const HIGHEST_PRIORITY: i32 = 8;
pub const HIGH_PRIORITY: i32 = 6;
pub const DEFAULT_PRIORITY: i32 = 4;
pub const LOW_PRIORITY: i32 = 2;
pub const LOWEST_PRIORITY: i32 = 0;

const CLASS_PREFIX: &str = "org.gotson.komga.application.tasks.Task$";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BookMetadataPatchCapability {
    #[serde(rename = "TITLE")]
    Title,
    #[serde(rename = "SUMMARY")]
    Summary,
    #[serde(rename = "NUMBER")]
    Number,
    #[serde(rename = "NUMBER_SORT")]
    NumberSort,
    #[serde(rename = "RELEASE_DATE")]
    ReleaseDate,
    #[serde(rename = "AUTHORS")]
    Authors,
    #[serde(rename = "TAGS")]
    Tags,
    #[serde(rename = "ISBN")]
    Isbn,
    #[serde(rename = "READ_LISTS")]
    ReadLists,
    #[serde(rename = "THUMBNAILS")]
    Thumbnails,
    #[serde(rename = "LINKS")]
    Links,
}

impl BookMetadataPatchCapability {
    pub fn all() -> BTreeSet<Self> {
        [
            Self::Title,
            Self::Summary,
            Self::Number,
            Self::NumberSort,
            Self::ReleaseDate,
            Self::Authors,
            Self::Tags,
            Self::Isbn,
            Self::ReadLists,
            Self::Thumbnails,
            Self::Links,
        ]
        .into_iter()
        .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyMode {
    #[serde(rename = "MOVE")]
    Move,
    #[serde(rename = "COPY")]
    Copy,
    #[serde(rename = "HARDLINK")]
    Hardlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LuceneEntity {
    #[serde(rename = "Book")]
    Book,
    #[serde(rename = "Series")]
    Series,
    #[serde(rename = "Collection")]
    Collection,
    #[serde(rename = "ReadList")]
    ReadList,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookPageNumbered {
    pub file_name: String,
    pub media_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_hash: String,
    pub file_size: Option<i64>,
    pub page_number: i32,
}

macro_rules! task_struct {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            $(pub $field: $ty,)*
            #[serde(default = "default_priority")]
            pub priority: i32,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub group_id: Option<String>,
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub unique_id: String,
        }
    };
}

fn default_priority() -> i32 {
    DEFAULT_PRIORITY
}

task_struct!(ScanLibrary {
    library_id: String,
    scan_deep: bool
});
task_struct!(LibraryTask { library_id: String });
task_struct!(AnalyzeBook { book_id: String });
task_struct!(BookTask { book_id: String });
task_struct!(RefreshBookMetadata {
    book_id: String,
    capabilities: BTreeSet<BookMetadataPatchCapability>,
});
task_struct!(SeriesTask { series_id: String });
task_struct!(ImportBook {
    source_file: String,
    series_id: String,
    copy_mode: CopyMode,
    destination_name: Option<String>,
    upgrade_book_id: Option<String>,
});
task_struct!(RemoveHashedPages {
    book_id: String,
    pages: Vec<BookPageNumbered>,
});
task_struct!(RebuildIndex {
    entities: Option<BTreeSet<LuceneEntity>>,
});
task_struct!(EmptyTask {});
task_struct!(FindBookThumbnailsToRegenerate {
    for_bigger_result_only: bool
});

/// A background task. The variant order matches `Task.kt`.
#[derive(Debug, Clone, PartialEq)]
pub enum Task {
    ScanLibrary(ScanLibrary),
    FindBooksToConvert(LibraryTask),
    FindBooksWithMissingPageHash(LibraryTask),
    FindDuplicatePagesToDelete(LibraryTask),
    EmptyTrash(LibraryTask),
    AnalyzeBook(AnalyzeBook),
    GenerateBookThumbnail(BookTask),
    RefreshBookMetadata(RefreshBookMetadata),
    HashBook(BookTask),
    HashBookPages(BookTask),
    HashBookKoreader(BookTask),
    RefreshSeriesMetadata(SeriesTask),
    AggregateSeriesMetadata(SeriesTask),
    RefreshBookLocalArtwork(BookTask),
    RefreshSeriesLocalArtwork(SeriesTask),
    ImportBook(ImportBook),
    ConvertBook(AnalyzeBook),
    RepairExtension(AnalyzeBook),
    RemoveHashedPages(RemoveHashedPages),
    RebuildIndex(RebuildIndex),
    UpgradeIndex(EmptyTask),
    DeleteBook(BookTask),
    DeleteSeries(SeriesTask),
    FindBookThumbnailsToRegenerate(FindBookThumbnailsToRegenerate),
}

impl Task {
    pub fn scan_library(library_id: &str, scan_deep: bool, priority: i32) -> Task {
        Task::ScanLibrary(ScanLibrary {
            library_id: library_id.to_string(),
            scan_deep,
            priority,
            group_id: None,
            unique_id: String::new(),
        })
    }

    pub fn library(kind: LibraryTaskKind, library_id: &str, priority: i32) -> Task {
        let t = LibraryTask {
            library_id: library_id.to_string(),
            priority,
            group_id: None,
            unique_id: String::new(),
        };
        match kind {
            LibraryTaskKind::FindBooksToConvert => Task::FindBooksToConvert(t),
            LibraryTaskKind::FindBooksWithMissingPageHash => Task::FindBooksWithMissingPageHash(t),
            LibraryTaskKind::FindDuplicatePagesToDelete => Task::FindDuplicatePagesToDelete(t),
            LibraryTaskKind::EmptyTrash => Task::EmptyTrash(t),
        }
    }

    pub fn book(
        kind: BookTaskKind,
        book_id: &str,
        priority: i32,
        group_id: Option<String>,
    ) -> Task {
        let t = BookTask {
            book_id: book_id.to_string(),
            priority,
            group_id: group_id.clone(),
            unique_id: String::new(),
        };
        match kind {
            BookTaskKind::GenerateBookThumbnail => Task::GenerateBookThumbnail(t),
            BookTaskKind::HashBook => Task::HashBook(t),
            BookTaskKind::HashBookPages => Task::HashBookPages(t),
            BookTaskKind::HashBookKoreader => Task::HashBookKoreader(t),
            BookTaskKind::RefreshBookLocalArtwork => Task::RefreshBookLocalArtwork(t),
            BookTaskKind::DeleteBook => Task::DeleteBook(t),
        }
    }

    pub fn analyze_book(book_id: &str, priority: i32, group_id: String) -> Task {
        Task::AnalyzeBook(AnalyzeBook {
            book_id: book_id.to_string(),
            priority,
            group_id: Some(group_id),
            unique_id: String::new(),
        })
    }

    pub fn refresh_book_metadata(
        book_id: &str,
        capabilities: BTreeSet<BookMetadataPatchCapability>,
        priority: i32,
        group_id: String,
    ) -> Task {
        Task::RefreshBookMetadata(RefreshBookMetadata {
            book_id: book_id.to_string(),
            capabilities,
            priority,
            group_id: Some(group_id),
            unique_id: String::new(),
        })
    }

    pub fn series(kind: SeriesTaskKind, series_id: &str, priority: i32) -> Task {
        let t = SeriesTask {
            series_id: series_id.to_string(),
            priority,
            group_id: match kind {
                SeriesTaskKind::RefreshSeriesMetadata | SeriesTaskKind::AggregateSeriesMetadata => {
                    Some(series_id.to_string())
                }
                _ => None,
            },
            unique_id: String::new(),
        };
        match kind {
            SeriesTaskKind::RefreshSeriesMetadata => Task::RefreshSeriesMetadata(t),
            SeriesTaskKind::AggregateSeriesMetadata => Task::AggregateSeriesMetadata(t),
            SeriesTaskKind::RefreshSeriesLocalArtwork => Task::RefreshSeriesLocalArtwork(t),
            SeriesTaskKind::DeleteSeries => Task::DeleteSeries(t),
        }
    }

    pub fn unique_id(&self) -> String {
        match self {
            Task::ScanLibrary(t) => format!("SCAN_LIBRARY_{}_DEEP_{}", t.library_id, t.scan_deep),
            Task::FindBooksToConvert(t) => format!("FIND_BOOKS_TO_CONVERT_{}", t.library_id),
            Task::FindBooksWithMissingPageHash(t) => {
                format!("FIND_BOOKS_WITH_MISSING_PAGE_HASH_{}", t.library_id)
            }
            Task::FindDuplicatePagesToDelete(t) => {
                format!("FIND_DUPLICATE_PAGES_TO_DELETE_{}", t.library_id)
            }
            Task::EmptyTrash(t) => format!("EMPTY_TRASH_{}", t.library_id),
            Task::AnalyzeBook(t) => format!("ANALYZE_BOOK_{}", t.book_id),
            Task::GenerateBookThumbnail(t) => format!("GENERATE_BOOK_THUMBNAIL_{}", t.book_id),
            Task::RefreshBookMetadata(t) => format!("REFRESH_BOOK_METADATA_{}", t.book_id),
            Task::HashBook(t) => format!("HASH_BOOK_{}", t.book_id),
            Task::HashBookPages(t) => format!("HASH_BOOK_PAGES_{}", t.book_id),
            Task::HashBookKoreader(t) => format!("HASH_BOOK_KOREADER_{}", t.book_id),
            Task::RefreshSeriesMetadata(t) => format!("REFRESH_SERIES_METADATA_{}", t.series_id),
            Task::AggregateSeriesMetadata(t) => {
                format!("AGGREGATE_SERIES_METADATA_{}", t.series_id)
            }
            Task::RefreshBookLocalArtwork(t) => {
                format!("REFRESH_BOOK_LOCAL_ARTWORK_{}", t.book_id)
            }
            Task::RefreshSeriesLocalArtwork(t) => {
                format!("REFRESH_SERIES_LOCAL_ARTWORK_{}", t.series_id)
            }
            Task::ImportBook(t) => format!("IMPORT_BOOK_{}_{}", t.series_id, t.source_file),
            Task::ConvertBook(t) => format!("CONVERT_BOOK_{}", t.book_id),
            Task::RepairExtension(t) => format!("REPAIR_EXTENSION_{}", t.book_id),
            Task::RemoveHashedPages(t) => format!("REMOVE_HASHED_PAGES_{}", t.book_id),
            Task::RebuildIndex(_) => "REBUILD_INDEX".to_string(),
            Task::UpgradeIndex(_) => "UPGRADE_INDEX".to_string(),
            Task::DeleteBook(t) => format!("DELETE_BOOK_{}", t.book_id),
            Task::DeleteSeries(t) => format!("DELETE_SERIES_{}", t.series_id),
            Task::FindBookThumbnailsToRegenerate(_) => {
                "FIND_BOOK_THUMBNAILS_TO_REGENERATE".to_string()
            }
        }
    }

    pub fn priority(&self) -> i32 {
        match self {
            Task::ScanLibrary(t) => t.priority,
            Task::FindBooksToConvert(t)
            | Task::FindBooksWithMissingPageHash(t)
            | Task::FindDuplicatePagesToDelete(t)
            | Task::EmptyTrash(t) => t.priority,
            Task::AnalyzeBook(t) | Task::ConvertBook(t) | Task::RepairExtension(t) => t.priority,
            Task::GenerateBookThumbnail(t)
            | Task::HashBook(t)
            | Task::HashBookPages(t)
            | Task::HashBookKoreader(t)
            | Task::RefreshBookLocalArtwork(t)
            | Task::DeleteBook(t) => t.priority,
            Task::RefreshBookMetadata(t) => t.priority,
            Task::RefreshSeriesMetadata(t)
            | Task::AggregateSeriesMetadata(t)
            | Task::RefreshSeriesLocalArtwork(t)
            | Task::DeleteSeries(t) => t.priority,
            Task::ImportBook(t) => t.priority,
            Task::RemoveHashedPages(t) => t.priority,
            Task::RebuildIndex(t) => t.priority,
            Task::UpgradeIndex(t) => t.priority,
            Task::FindBookThumbnailsToRegenerate(t) => t.priority,
        }
    }

    pub fn group_id(&self) -> Option<String> {
        match self {
            Task::ScanLibrary(t) => t.group_id.clone(),
            Task::FindBooksToConvert(t)
            | Task::FindBooksWithMissingPageHash(t)
            | Task::FindDuplicatePagesToDelete(t)
            | Task::EmptyTrash(t) => t.group_id.clone(),
            Task::AnalyzeBook(t) | Task::ConvertBook(t) | Task::RepairExtension(t) => {
                t.group_id.clone()
            }
            Task::GenerateBookThumbnail(t)
            | Task::HashBook(t)
            | Task::HashBookPages(t)
            | Task::HashBookKoreader(t)
            | Task::RefreshBookLocalArtwork(t)
            | Task::DeleteBook(t) => t.group_id.clone(),
            Task::RefreshBookMetadata(t) => t.group_id.clone(),
            Task::RefreshSeriesMetadata(t)
            | Task::AggregateSeriesMetadata(t)
            | Task::RefreshSeriesLocalArtwork(t)
            | Task::DeleteSeries(t) => t.group_id.clone(),
            Task::ImportBook(t) => t.group_id.clone(),
            Task::RemoveHashedPages(t) => t.group_id.clone(),
            Task::RebuildIndex(t) => t.group_id.clone(),
            Task::UpgradeIndex(t) => t.group_id.clone(),
            Task::FindBookThumbnailsToRegenerate(t) => t.group_id.clone(),
        }
    }

    pub fn simple_type(&self) -> &'static str {
        match self {
            Task::ScanLibrary(_) => "ScanLibrary",
            Task::FindBooksToConvert(_) => "FindBooksToConvert",
            Task::FindBooksWithMissingPageHash(_) => "FindBooksWithMissingPageHash",
            Task::FindDuplicatePagesToDelete(_) => "FindDuplicatePagesToDelete",
            Task::EmptyTrash(_) => "EmptyTrash",
            Task::AnalyzeBook(_) => "AnalyzeBook",
            Task::GenerateBookThumbnail(_) => "GenerateBookThumbnail",
            Task::RefreshBookMetadata(_) => "RefreshBookMetadata",
            Task::HashBook(_) => "HashBook",
            Task::HashBookPages(_) => "HashBookPages",
            Task::HashBookKoreader(_) => "HashBookKoreader",
            Task::RefreshSeriesMetadata(_) => "RefreshSeriesMetadata",
            Task::AggregateSeriesMetadata(_) => "AggregateSeriesMetadata",
            Task::RefreshBookLocalArtwork(_) => "RefreshBookLocalArtwork",
            Task::RefreshSeriesLocalArtwork(_) => "RefreshSeriesLocalArtwork",
            Task::ImportBook(_) => "ImportBook",
            Task::ConvertBook(_) => "ConvertBook",
            Task::RepairExtension(_) => "RepairExtension",
            Task::RemoveHashedPages(_) => "RemoveHashedPages",
            Task::RebuildIndex(_) => "RebuildIndex",
            Task::UpgradeIndex(_) => "UpgradeIndex",
            Task::DeleteBook(_) => "DeleteBook",
            Task::DeleteSeries(_) => "DeleteSeries",
            Task::FindBookThumbnailsToRegenerate(_) => "FindBookThumbnailsToRegenerate",
        }
    }

    pub fn class_name(&self) -> String {
        format!("{}{}", CLASS_PREFIX, self.simple_type())
    }

    /// The Jackson payload: all fields plus inherited `priority`/`groupId`/`uniqueId`.
    pub fn to_payload(&self) -> serde_json::Value {
        let mut value = match self {
            Task::ScanLibrary(t) => serde_json::to_value(t),
            Task::FindBooksToConvert(t)
            | Task::FindBooksWithMissingPageHash(t)
            | Task::FindDuplicatePagesToDelete(t)
            | Task::EmptyTrash(t) => serde_json::to_value(t),
            Task::AnalyzeBook(t) | Task::ConvertBook(t) | Task::RepairExtension(t) => {
                serde_json::to_value(t)
            }
            Task::GenerateBookThumbnail(t)
            | Task::HashBook(t)
            | Task::HashBookPages(t)
            | Task::HashBookKoreader(t)
            | Task::RefreshBookLocalArtwork(t)
            | Task::DeleteBook(t) => serde_json::to_value(t),
            Task::RefreshBookMetadata(t) => serde_json::to_value(t),
            Task::RefreshSeriesMetadata(t)
            | Task::AggregateSeriesMetadata(t)
            | Task::RefreshSeriesLocalArtwork(t)
            | Task::DeleteSeries(t) => serde_json::to_value(t),
            Task::ImportBook(t) => serde_json::to_value(t),
            Task::RemoveHashedPages(t) => serde_json::to_value(t),
            Task::RebuildIndex(t) => serde_json::to_value(t),
            Task::UpgradeIndex(t) => serde_json::to_value(t),
            Task::FindBookThumbnailsToRegenerate(t) => serde_json::to_value(t),
        }
        .expect("task serialization cannot fail");
        // Jackson serializes null groupId; the per-struct skip is only for construction ergonomics
        let obj = value.as_object_mut().expect("task payload is an object");
        if !obj.contains_key("groupId") {
            obj.insert("groupId".to_string(), serde_json::Value::Null);
        }
        obj.insert(
            "uniqueId".to_string(),
            serde_json::Value::String(self.unique_id()),
        );
        value
    }

    /// Deserializes by the stored `CLASS` FQN; unknown classes yield None (TasksDao logs and skips them).
    pub fn from_payload(class: &str, payload: &str) -> Option<Task> {
        fn parse<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
            serde_json::from_str(s).ok()
        }
        let simple = class.strip_prefix(CLASS_PREFIX)?;
        Some(match simple {
            "ScanLibrary" => Task::ScanLibrary(parse(payload)?),
            "FindBooksToConvert" => Task::FindBooksToConvert(parse(payload)?),
            "FindBooksWithMissingPageHash" => Task::FindBooksWithMissingPageHash(parse(payload)?),
            "FindDuplicatePagesToDelete" => Task::FindDuplicatePagesToDelete(parse(payload)?),
            "EmptyTrash" => Task::EmptyTrash(parse(payload)?),
            "AnalyzeBook" => Task::AnalyzeBook(parse(payload)?),
            "GenerateBookThumbnail" => Task::GenerateBookThumbnail(parse(payload)?),
            "RefreshBookMetadata" => Task::RefreshBookMetadata(parse(payload)?),
            "HashBook" => Task::HashBook(parse(payload)?),
            "HashBookPages" => Task::HashBookPages(parse(payload)?),
            "HashBookKoreader" => Task::HashBookKoreader(parse(payload)?),
            "RefreshSeriesMetadata" => Task::RefreshSeriesMetadata(parse(payload)?),
            "AggregateSeriesMetadata" => Task::AggregateSeriesMetadata(parse(payload)?),
            "RefreshBookLocalArtwork" => Task::RefreshBookLocalArtwork(parse(payload)?),
            "RefreshSeriesLocalArtwork" => Task::RefreshSeriesLocalArtwork(parse(payload)?),
            "ImportBook" => Task::ImportBook(parse(payload)?),
            "ConvertBook" => Task::ConvertBook(parse(payload)?),
            "RepairExtension" => Task::RepairExtension(parse(payload)?),
            "RemoveHashedPages" => Task::RemoveHashedPages(parse(payload)?),
            "RebuildIndex" => Task::RebuildIndex(parse(payload)?),
            "UpgradeIndex" => Task::UpgradeIndex(parse(payload)?),
            "DeleteBook" => Task::DeleteBook(parse(payload)?),
            "DeleteSeries" => Task::DeleteSeries(parse(payload)?),
            "FindBookThumbnailsToRegenerate" => {
                Task::FindBookThumbnailsToRegenerate(parse(payload)?)
            }
            _ => return None,
        })
    }

    /// `Task.toString()` for logs, e.g. `ScanLibrary(libraryId='x', scanDeep='false', priority='4')`
    pub fn describe(&self) -> String {
        match self {
            Task::ScanLibrary(t) => format!(
                "ScanLibrary(libraryId='{}', scanDeep='{}', priority='{}')",
                t.library_id, t.scan_deep, t.priority
            ),
            _ => format!("{}(priority='{}')", self.simple_type(), self.priority()),
        }
    }
}

pub enum LibraryTaskKind {
    FindBooksToConvert,
    FindBooksWithMissingPageHash,
    FindDuplicatePagesToDelete,
    EmptyTrash,
}

pub enum BookTaskKind {
    GenerateBookThumbnail,
    HashBook,
    HashBookPages,
    HashBookKoreader,
    RefreshBookLocalArtwork,
    DeleteBook,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeriesTaskKind {
    RefreshSeriesMetadata,
    AggregateSeriesMetadata,
    RefreshSeriesLocalArtwork,
    DeleteSeries,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_ids() {
        assert_eq!(
            Task::scan_library("lib1", true, HIGHEST_PRIORITY).unique_id(),
            "SCAN_LIBRARY_lib1_DEEP_true"
        );
        assert_eq!(
            Task::analyze_book("b1", DEFAULT_PRIORITY, "s1".into()).unique_id(),
            "ANALYZE_BOOK_b1"
        );
        assert_eq!(
            Task::series(
                SeriesTaskKind::AggregateSeriesMetadata,
                "s1",
                DEFAULT_PRIORITY
            )
            .unique_id(),
            "AGGREGATE_SERIES_METADATA_s1"
        );
    }

    #[test]
    fn group_ids() {
        assert_eq!(
            Task::analyze_book("b1", DEFAULT_PRIORITY, "s1".into()).group_id(),
            Some("s1".to_string())
        );
        assert_eq!(
            Task::series(
                SeriesTaskKind::RefreshSeriesMetadata,
                "s1",
                DEFAULT_PRIORITY
            )
            .group_id(),
            Some("s1".to_string())
        );
        assert_eq!(
            Task::book(BookTaskKind::HashBook, "b1", LOWEST_PRIORITY, None).group_id(),
            None
        );
    }

    #[test]
    fn payload_roundtrip() {
        let task = Task::scan_library("lib1", false, DEFAULT_PRIORITY);
        let payload = task.to_payload();
        assert_eq!(
            payload,
            serde_json::json!({
                "libraryId": "lib1",
                "scanDeep": false,
                "priority": 4,
                "groupId": null,
                "uniqueId": "SCAN_LIBRARY_lib1_DEEP_false",
            })
        );

        let class = task.class_name();
        assert_eq!(class, "org.gotson.komga.application.tasks.Task$ScanLibrary");
        let parsed = Task::from_payload(&class, &payload.to_string()).unwrap();
        assert_eq!(parsed.unique_id(), task.unique_id());
        assert_eq!(parsed.priority(), 4);
    }

    #[test]
    fn payload_with_group() {
        let task = Task::analyze_book("b1", HIGH_PRIORITY, "s1".into());
        let payload = task.to_payload();
        assert_eq!(payload["groupId"], "s1");
        let parsed = Task::from_payload(&task.class_name(), &payload.to_string()).unwrap();
        assert_eq!(parsed.group_id(), Some("s1".to_string()));
    }

    #[test]
    fn payload_with_capabilities_and_entities() {
        let task = Task::refresh_book_metadata(
            "b1",
            BookMetadataPatchCapability::all(),
            DEFAULT_PRIORITY,
            "s1".into(),
        );
        let payload = task.to_payload();
        assert_eq!(
            payload["capabilities"],
            serde_json::json!([
                "TITLE",
                "SUMMARY",
                "NUMBER",
                "NUMBER_SORT",
                "RELEASE_DATE",
                "AUTHORS",
                "TAGS",
                "ISBN",
                "READ_LISTS",
                "THUMBNAILS",
                "LINKS"
            ])
        );
        let parsed = Task::from_payload(&task.class_name(), &payload.to_string()).unwrap();
        assert_eq!(parsed.unique_id(), "REFRESH_BOOK_METADATA_b1");
    }

    #[test]
    fn unknown_class_yields_none() {
        assert!(Task::from_payload("com.example.Other", "{}").is_none());
        assert!(Task::from_payload("org.gotson.komga.application.tasks.Task$Nope", "{}").is_none());
    }
}
