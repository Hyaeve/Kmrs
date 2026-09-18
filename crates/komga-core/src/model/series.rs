//! Equivalent models for `Series.kt` / `SeriesMetadata.kt` / `BookMetadataAggregation.kt`.
//! `url` is stored as a `file:/…` string (consistent with the DB URL column).

use serde::{Deserialize, Serialize};

pub use super::common::{Author, WebLink};
use std::collections::BTreeSet;
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub id: String,
    pub name: String,
    pub url: String,
    pub file_last_modified: OffsetDateTime,
    pub library_id: String,
    pub book_count: i32,
    /// Soft delete (trash)
    pub deleted_date: Option<OffsetDateTime>,
    pub oneshot: bool,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesMetadata {
    pub series_id: String,
    pub status: SeriesStatus,
    pub title: String,
    pub title_sort: String,
    pub summary: String,
    pub reading_direction: Option<ReadingDirection>,
    pub publisher: String,
    pub age_rating: Option<i32>,
    pub language: String,
    pub genres: BTreeSet<String>,
    pub tags: BTreeSet<String>,
    pub total_book_count: Option<i32>,
    pub sharing_labels: BTreeSet<String>,
    pub links: Vec<WebLink>,
    pub alternate_titles: Vec<AlternateTitle>,
    pub status_lock: bool,
    pub title_lock: bool,
    pub title_sort_lock: bool,
    pub summary_lock: bool,
    pub reading_direction_lock: bool,
    pub publisher_lock: bool,
    pub age_rating_lock: bool,
    pub language_lock: bool,
    pub genres_lock: bool,
    pub tags_lock: bool,
    pub total_book_count_lock: bool,
    pub sharing_labels_lock: bool,
    pub links_lock: bool,
    pub alternate_titles_lock: bool,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeriesStatus {
    #[serde(rename = "ENDED")]
    Ended,
    #[serde(rename = "ONGOING")]
    Ongoing,
    #[serde(rename = "ABANDONED")]
    Abandoned,
    #[serde(rename = "HIATUS")]
    Hiatus,
}

// Matches the pattern in library.rs; deliberately uses from_str instead of the FromStr trait
#[allow(clippy::should_implement_trait)]
impl SeriesStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SeriesStatus::Ended => "ENDED",
            SeriesStatus::Ongoing => "ONGOING",
            SeriesStatus::Abandoned => "ABANDONED",
            SeriesStatus::Hiatus => "HIATUS",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "ENDED" => SeriesStatus::Ended,
            "ONGOING" => SeriesStatus::Ongoing,
            "ABANDONED" => SeriesStatus::Abandoned,
            "HIATUS" => SeriesStatus::Hiatus,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadingDirection {
    #[serde(rename = "LEFT_TO_RIGHT")]
    LeftToRight,
    #[serde(rename = "RIGHT_TO_LEFT")]
    RightToLeft,
    #[serde(rename = "VERTICAL")]
    Vertical,
    #[serde(rename = "WEBTOON")]
    Webtoon,
}

// Matches the pattern in library.rs; deliberately uses from_str instead of the FromStr trait
#[allow(clippy::should_implement_trait)]
impl ReadingDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadingDirection::LeftToRight => "LEFT_TO_RIGHT",
            ReadingDirection::RightToLeft => "RIGHT_TO_LEFT",
            ReadingDirection::Vertical => "VERTICAL",
            ReadingDirection::Webtoon => "WEBTOON",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "LEFT_TO_RIGHT" => ReadingDirection::LeftToRight,
            "RIGHT_TO_LEFT" => ReadingDirection::RightToLeft,
            "VERTICAL" => ReadingDirection::Vertical,
            "WEBTOON" => ReadingDirection::Webtoon,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternateTitle {
    pub label: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BookMetadataAggregation {
    pub series_id: String,
    pub authors: Vec<Author>,
    pub tags: BTreeSet<String>,
    pub release_date: Option<Date>,
    pub summary: String,
    pub summary_number: String,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}
