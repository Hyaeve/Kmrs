//! Equivalent model for `ReadList.kt`.

use std::collections::BTreeMap;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct ReadList {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// Whether manually ordered
    pub ordered: bool,
    /// The key is READLIST_BOOK.NUMBER
    pub book_ids: BTreeMap<i32, String>,
    /// true means book_ids is filtered, not the full set (DTO-computed field, not persisted)
    pub filtered: bool,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}
