//! Equivalent models for `Media.kt` / `BookPage.kt` / `MediaFile.kt`.
//!
//! A page's NUMBER in the DB is a 0-based positional index (see `MediaDao.insertPages`);
//! the model does not carry number and, like komga, uses the list position instead.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub book_id: String,
    pub status: MediaStatus,
    pub media_type: Option<String>,
    pub comment: Option<String>,
    pub page_count: i32,
    pub pages: Vec<BookPage>,
    pub files: Vec<MediaFile>,
    /// Java FQN of the MediaExtension (e.g. `…epub.EpubDivinaLayout`); not decoded at the DAO layer
    pub extension_class: Option<String>,
    /// gzip+JSON MediaExtension; stored and retrieved as-is at the DAO layer
    pub extension_value: Option<Vec<u8>>,
    pub epub_divina_compatible: bool,
    pub epub_is_kepub: bool,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaStatus {
    #[serde(rename = "UNKNOWN")]
    Unknown,
    #[serde(rename = "ERROR")]
    Error,
    #[serde(rename = "READY")]
    Ready,
    #[serde(rename = "UNSUPPORTED")]
    Unsupported,
    #[serde(rename = "OUTDATED")]
    Outdated,
}

impl MediaStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaStatus::Unknown => "UNKNOWN",
            MediaStatus::Error => "ERROR",
            MediaStatus::Ready => "READY",
            MediaStatus::Unsupported => "UNSUPPORTED",
            MediaStatus::Outdated => "OUTDATED",
        }
    }

    // Naming consistent with the enums throughout; not a std::str::FromStr implementation
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "UNKNOWN" => MediaStatus::Unknown,
            "ERROR" => MediaStatus::Error,
            "READY" => MediaStatus::Ready,
            "UNSUPPORTED" => MediaStatus::Unsupported,
            "OUTDATED" => MediaStatus::Outdated,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BookPage {
    pub file_name: String,
    pub media_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_hash: String,
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaFile {
    pub file_name: String,
    pub media_type: Option<String>,
    pub sub_type: Option<MediaFileSubType>,
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaFileSubType {
    #[serde(rename = "EPUB_PAGE")]
    EpubPage,
    #[serde(rename = "EPUB_ASSET")]
    EpubAsset,
}

impl MediaFileSubType {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaFileSubType::EpubPage => "EPUB_PAGE",
            MediaFileSubType::EpubAsset => "EPUB_ASSET",
        }
    }

    // Naming consistent with the enums throughout; not a std::str::FromStr implementation
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "EPUB_PAGE" => MediaFileSubType::EpubPage,
            "EPUB_ASSET" => MediaFileSubType::EpubAsset,
            _ => return None,
        })
    }
}
