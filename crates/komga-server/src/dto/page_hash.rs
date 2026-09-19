//! DTOs for `PageHashController` (`PageHashKnownDto.kt`, `PageHashUnknownDto.kt`,
//! `PageHashMatchDto.kt`, `PageHashCreationDto.kt`).

use komga_core::dto::dto_datetime;
use komga_core::model::page_hash::PageHashAction;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageHashKnownDto {
    pub hash: String,
    pub size: Option<i64>,
    pub action: PageHashAction,
    pub delete_count: i32,
    pub match_count: i32,
    #[serde(with = "dto_datetime")]
    pub created: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified: OffsetDateTime,
}

impl From<&komga_core::model::page_hash::PageHashKnown> for PageHashKnownDto {
    fn from(p: &komga_core::model::page_hash::PageHashKnown) -> Self {
        Self {
            hash: p.hash.clone(),
            size: p.size,
            action: p.action,
            delete_count: p.delete_count,
            match_count: p.match_count,
            created: p.created_date,
            last_modified: p.last_modified_date,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageHashUnknownDto {
    pub hash: String,
    pub size: Option<i64>,
    pub match_count: i32,
}

impl From<&komga_core::model::page_hash::PageHashUnknown> for PageHashUnknownDto {
    fn from(p: &komga_core::model::page_hash::PageHashUnknown) -> Self {
        Self {
            hash: p.hash.clone(),
            size: p.size,
            match_count: p.match_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageHashMatchDto {
    pub book_id: String,
    pub url: String,
    pub page_number: i32,
    pub file_name: String,
    pub file_size: i64,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageHashCreationDto {
    pub hash: String,
    pub size: Option<i64>,
    pub action: PageHashAction,
}
