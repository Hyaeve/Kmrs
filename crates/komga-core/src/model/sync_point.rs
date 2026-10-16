//! Equivalent model for `SyncPoint.kt` (Kobo sync points and their child tables).

use time::OffsetDateTime;

pub const ON_DECK_ID: &str = "KOMGA-ONDECK";

#[derive(Debug, Clone, PartialEq)]
pub struct SyncPoint {
    pub id: String,
    pub user_id: String,
    pub api_key_id: Option<String>,
    pub created_date: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncPointBook {
    pub sync_point_id: String,
    pub book_id: String,
    pub book_created_date: OffsetDateTime,
    pub book_last_modified_date: OffsetDateTime,
    pub book_file_last_modified: OffsetDateTime,
    pub book_file_size: i64,
    pub book_file_hash: String,
    pub book_metadata_last_modified_date: OffsetDateTime,
    pub book_read_progress_last_modified_date: Option<OffsetDateTime>,
    pub book_thumbnail_id: Option<String>,
    pub synced: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncPointReadList {
    pub sync_point_id: String,
    pub readlist_id: String,
    pub readlist_name: String,
    pub readlist_created_date: OffsetDateTime,
    pub readlist_last_modified_date: OffsetDateTime,
    pub synced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPointReadListBook {
    pub sync_point_id: String,
    pub readlist_id: String,
    pub book_id: String,
}
