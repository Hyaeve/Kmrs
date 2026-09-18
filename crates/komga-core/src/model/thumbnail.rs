//! Thumbnail models, corresponding to `ThumbnailBook.kt` / `ThumbnailSeries.kt` / `ThumbnailSeriesCollection.kt` / `ThumbnailReadList.kt`.
//! GENERATED/USER_UPLOADED bytes are stored in a DB blob; SIDECAR only stores a URL (the file is read on demand).

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Dimension {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThumbnailType {
    #[serde(rename = "GENERATED")]
    Generated,
    #[serde(rename = "SIDECAR")]
    Sidecar,
    #[serde(rename = "USER_UPLOADED")]
    UserUploaded,
}

impl ThumbnailType {
    pub fn as_str(self) -> &'static str {
        match self {
            ThumbnailType::Generated => "GENERATED",
            ThumbnailType::Sidecar => "SIDECAR",
            ThumbnailType::UserUploaded => "USER_UPLOADED",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "GENERATED" => ThumbnailType::Generated,
            "SIDECAR" => ThumbnailType::Sidecar,
            "USER_UPLOADED" => ThumbnailType::UserUploaded,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailBook {
    pub id: String,
    pub book_id: String,
    pub thumbnail: Option<Vec<u8>>,
    pub url: Option<String>,
    pub selected: bool,
    pub type_: ThumbnailType,
    pub media_type: String,
    pub file_size: i64,
    pub dimension: Dimension,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailSeries {
    pub id: String,
    pub series_id: String,
    pub thumbnail: Option<Vec<u8>>,
    pub url: Option<String>,
    pub selected: bool,
    pub type_: ThumbnailType,
    pub media_type: String,
    pub file_size: i64,
    pub dimension: Dimension,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailSeriesCollection {
    pub id: String,
    pub collection_id: String,
    pub thumbnail: Vec<u8>,
    pub selected: bool,
    pub type_: ThumbnailType,
    pub media_type: String,
    pub file_size: i64,
    pub dimension: Dimension,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailReadList {
    pub id: String,
    pub read_list_id: String,
    pub thumbnail: Vec<u8>,
    pub selected: bool,
    pub type_: ThumbnailType,
    pub media_type: String,
    pub file_size: i64,
    pub dimension: Dimension,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}
