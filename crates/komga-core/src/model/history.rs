//! Equivalent model for `HistoricalEvent.kt`. TYPE is stored in the DB as a camelCase string (e.g. `BookFileDeleted`).
//! Events are append-only; properties are stored in the HISTORICAL_EVENT_PROPERTIES child table.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoricalEvent {
    pub id: String,
    pub type_: HistoricalEventType,
    pub book_id: Option<String>,
    pub series_id: Option<String>,
    pub properties: BTreeMap<String, String>,
    /// komga uses `LocalDateTime.now()` in the system default time zone here; callers must decide the time-zone semantics themselves
    pub timestamp: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoricalEventType {
    #[serde(rename = "BookFileDeleted")]
    BookFileDeleted,
    #[serde(rename = "SeriesFolderDeleted")]
    SeriesFolderDeleted,
    #[serde(rename = "BookConverted")]
    BookConverted,
    #[serde(rename = "BookImported")]
    BookImported,
    #[serde(rename = "DuplicatePageDeleted")]
    DuplicatePageDeleted,
}

impl HistoricalEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            HistoricalEventType::BookFileDeleted => "BookFileDeleted",
            HistoricalEventType::SeriesFolderDeleted => "SeriesFolderDeleted",
            HistoricalEventType::BookConverted => "BookConverted",
            HistoricalEventType::BookImported => "BookImported",
            HistoricalEventType::DuplicatePageDeleted => "DuplicatePageDeleted",
        }
    }

    // Matches the DAO pattern used throughout (library.rs's from_str); deliberately not the standard FromStr trait to avoid misuse
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "BookFileDeleted" => HistoricalEventType::BookFileDeleted,
            "SeriesFolderDeleted" => HistoricalEventType::SeriesFolderDeleted,
            "BookConverted" => HistoricalEventType::BookConverted,
            "BookImported" => HistoricalEventType::BookImported,
            "DuplicatePageDeleted" => HistoricalEventType::DuplicatePageDeleted,
            _ => return None,
        })
    }
}
