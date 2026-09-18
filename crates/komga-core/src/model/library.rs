//! Equivalent model for `Library.kt`. `root` is stored as a `file:/…` string (consistent with the DB URL column).

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub root: String,
    pub import_comicinfo_book: bool,
    pub import_comicinfo_series: bool,
    pub import_comicinfo_collection: bool,
    pub import_comicinfo_readlist: bool,
    pub import_comicinfo_series_append_volume: bool,
    pub import_epub_book: bool,
    pub import_epub_series: bool,
    pub import_mylar_series: bool,
    pub import_local_artwork: bool,
    pub import_barcode_isbn: bool,
    pub scan_force_modified_time: bool,
    pub scan_on_startup: bool,
    pub scan_interval: ScanInterval,
    pub scan_cbx: bool,
    pub scan_pdf: bool,
    pub scan_epub: bool,
    pub scan_directory_exclusions: Vec<String>,
    pub repair_extensions: bool,
    pub convert_to_cbz: bool,
    pub empty_trash_after_scan: bool,
    pub series_cover: SeriesCover,
    pub hash_files: bool,
    pub hash_pages: bool,
    pub hash_koreader: bool,
    pub analyze_dimensions: bool,
    pub oneshots_directory: Option<String>,
    pub unavailable_date: Option<OffsetDateTime>,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

impl Library {
    pub fn unavailable(&self) -> bool {
        self.unavailable_date.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanInterval {
    #[serde(rename = "DISABLED")]
    Disabled,
    #[serde(rename = "HOURLY")]
    Hourly,
    #[serde(rename = "EVERY_6H")]
    Every6H,
    #[serde(rename = "EVERY_12H")]
    Every12H,
    #[serde(rename = "DAILY")]
    Daily,
    #[serde(rename = "WEEKLY")]
    Weekly,
}

impl ScanInterval {
    pub fn as_str(self) -> &'static str {
        match self {
            ScanInterval::Disabled => "DISABLED",
            ScanInterval::Hourly => "HOURLY",
            ScanInterval::Every6H => "EVERY_6H",
            ScanInterval::Every12H => "EVERY_12H",
            ScanInterval::Daily => "DAILY",
            ScanInterval::Weekly => "WEEKLY",
        }
    }

    // Matches the DAO pattern used throughout; deliberately uses from_str instead of the FromStr trait
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "DISABLED" => ScanInterval::Disabled,
            "HOURLY" => ScanInterval::Hourly,
            "EVERY_6H" => ScanInterval::Every6H,
            "EVERY_12H" => ScanInterval::Every12H,
            "DAILY" => ScanInterval::Daily,
            "WEEKLY" => ScanInterval::Weekly,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeriesCover {
    #[serde(rename = "FIRST")]
    First,
    #[serde(rename = "FIRST_UNREAD_OR_FIRST")]
    FirstUnreadOrFirst,
    #[serde(rename = "FIRST_UNREAD_OR_LAST")]
    FirstUnreadOrLast,
    #[serde(rename = "LAST")]
    Last,
}

impl SeriesCover {
    pub fn as_str(self) -> &'static str {
        match self {
            SeriesCover::First => "FIRST",
            SeriesCover::FirstUnreadOrFirst => "FIRST_UNREAD_OR_FIRST",
            SeriesCover::FirstUnreadOrLast => "FIRST_UNREAD_OR_LAST",
            SeriesCover::Last => "LAST",
        }
    }

    // Matches the DAO pattern used throughout; deliberately uses from_str instead of the FromStr trait
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "FIRST" => SeriesCover::First,
            "FIRST_UNREAD_OR_FIRST" => SeriesCover::FirstUnreadOrFirst,
            "FIRST_UNREAD_OR_LAST" => SeriesCover::FirstUnreadOrLast,
            "LAST" => SeriesCover::Last,
            _ => return None,
        })
    }
}
