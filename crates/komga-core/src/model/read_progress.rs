//! Equivalent models for `ReadProgress.kt` and the READ_PROGRESS_SERIES aggregate table.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct ReadProgress {
    pub book_id: String,
    pub user_id: String,
    pub page: i32,
    pub completed: bool,
    pub read_date: OffsetDateTime,
    pub device_id: String,
    pub device_name: String,
    /// R2Locator JSON (gzip-encoded/decoded at the DAO layer); unrecognized fields are preserved for round-trip compatibility with old data.
    pub locator: Option<serde_json::Value>,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

/// READ_PROGRESS_SERIES aggregate row, fully recomputed by ReadProgressDao on progress changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadProgressSeries {
    pub series_id: String,
    pub user_id: String,
    pub read_count: i32,
    pub in_progress_count: i32,
    pub most_recent_read_date: Option<OffsetDateTime>,
    pub last_modified_date: Option<OffsetDateTime>,
}
