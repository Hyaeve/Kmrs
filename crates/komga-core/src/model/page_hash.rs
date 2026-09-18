//! Equivalent models for `PageHash.kt` / `PageHashKnown.kt` / `PageHashUnknown.kt`.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHashAction {
    #[serde(rename = "DELETE_AUTO")]
    DeleteAuto,
    #[serde(rename = "DELETE_MANUAL")]
    DeleteManual,
    #[serde(rename = "IGNORE")]
    Ignore,
}

impl PageHashAction {
    pub fn as_str(self) -> &'static str {
        match self {
            PageHashAction::DeleteAuto => "DELETE_AUTO",
            PageHashAction::DeleteManual => "DELETE_MANUAL",
            PageHashAction::Ignore => "IGNORE",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "DELETE_AUTO" => PageHashAction::DeleteAuto,
            "DELETE_MANUAL" => PageHashAction::DeleteManual,
            "IGNORE" => PageHashAction::Ignore,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageHashKnown {
    pub hash: String,
    /// Negative values are normalized to None (consistent with the Kotlin `PageHash` constructor)
    pub size: Option<i64>,
    pub action: PageHashAction,
    pub delete_count: i32,
    /// Filled by the join count in find_all_known; not persisted
    pub match_count: i32,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

impl PageHashKnown {
    pub fn normalize_size(size: Option<i64>) -> Option<i64> {
        size.filter(|s| *s >= 0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageHashUnknown {
    pub hash: String,
    pub size: Option<i64>,
    pub match_count: i32,
}
