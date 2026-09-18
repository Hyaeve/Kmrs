//! Equivalent model for `SeriesCollection.kt`.

use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesCollection {
    pub id: String,
    pub name: String,
    /// Whether manually ordered
    pub ordered: bool,
    /// The index is COLLECTION_SERIES.NUMBER (0-based)
    pub series_ids: Vec<String>,
    /// true means series_ids is filtered, not the full set (DTO-computed field, not persisted)
    pub filtered: bool,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}
