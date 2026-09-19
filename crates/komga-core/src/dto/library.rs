//! `LibraryDto.kt`, `ScanIntervalDto.kt`, `SeriesCoverDto.kt`.

use crate::model::library::{Library, ScanInterval, SeriesCover};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDto {
    pub id: String,
    pub name: String,
    pub root: String,
    #[serde(rename = "importComicInfoBook")]
    pub import_comicinfo_book: bool,
    #[serde(rename = "importComicInfoSeries")]
    pub import_comicinfo_series: bool,
    #[serde(rename = "importComicInfoCollection")]
    pub import_comicinfo_collection: bool,
    #[serde(rename = "importComicInfoReadList")]
    pub import_comicinfo_readlist: bool,
    #[serde(rename = "importComicInfoSeriesAppendVolume")]
    pub import_comicinfo_series_append_volume: bool,
    pub import_epub_book: bool,
    pub import_epub_series: bool,
    pub import_mylar_series: bool,
    pub import_local_artwork: bool,
    pub import_barcode_isbn: bool,
    pub scan_force_modified_time: bool,
    pub scan_interval: ScanIntervalDto,
    pub scan_on_startup: bool,
    pub scan_cbx: bool,
    pub scan_pdf: bool,
    pub scan_epub: bool,
    pub scan_directory_exclusions: BTreeSet<String>,
    pub repair_extensions: bool,
    pub convert_to_cbz: bool,
    pub empty_trash_after_scan: bool,
    pub series_cover: SeriesCoverDto,
    pub hash_files: bool,
    pub hash_pages: bool,
    pub hash_koreader: bool,
    pub analyze_dimensions: bool,
    pub oneshots_directory: Option<String>,
    pub unavailable: bool,
}

impl LibraryDto {
    /// `Library.toDto(includeRoot)`: non-admin users get an empty root
    pub fn of(library: &Library, include_root: bool) -> Self {
        Self {
            id: library.id.clone(),
            name: library.name.clone(),
            root: if include_root {
                super::url_to_file_path(&library.root)
            } else {
                String::new()
            },
            import_comicinfo_book: library.import_comicinfo_book,
            import_comicinfo_series: library.import_comicinfo_series,
            import_comicinfo_collection: library.import_comicinfo_collection,
            import_comicinfo_readlist: library.import_comicinfo_readlist,
            import_comicinfo_series_append_volume: library.import_comicinfo_series_append_volume,
            import_epub_book: library.import_epub_book,
            import_epub_series: library.import_epub_series,
            import_mylar_series: library.import_mylar_series,
            import_local_artwork: library.import_local_artwork,
            import_barcode_isbn: library.import_barcode_isbn,
            scan_force_modified_time: library.scan_force_modified_time,
            scan_interval: ScanIntervalDto::from(library.scan_interval),
            scan_on_startup: library.scan_on_startup,
            scan_cbx: library.scan_cbx,
            scan_pdf: library.scan_pdf,
            scan_epub: library.scan_epub,
            scan_directory_exclusions: library.scan_directory_exclusions.iter().cloned().collect(),
            repair_extensions: library.repair_extensions,
            convert_to_cbz: library.convert_to_cbz,
            empty_trash_after_scan: library.empty_trash_after_scan,
            series_cover: SeriesCoverDto::from(library.series_cover),
            hash_files: library.hash_files,
            hash_pages: library.hash_pages,
            hash_koreader: library.hash_koreader,
            analyze_dimensions: library.analyze_dimensions,
            oneshots_directory: library.oneshots_directory.clone(),
            unavailable: library.unavailable(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanIntervalDto {
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

impl From<ScanInterval> for ScanIntervalDto {
    fn from(i: ScanInterval) -> Self {
        match i {
            ScanInterval::Disabled => ScanIntervalDto::Disabled,
            ScanInterval::Hourly => ScanIntervalDto::Hourly,
            ScanInterval::Every6H => ScanIntervalDto::Every6H,
            ScanInterval::Every12H => ScanIntervalDto::Every12H,
            ScanInterval::Daily => ScanIntervalDto::Daily,
            ScanInterval::Weekly => ScanIntervalDto::Weekly,
        }
    }
}

impl ScanIntervalDto {
    /// `ScanIntervalDto.toDomain()`
    pub fn to_domain(self) -> ScanInterval {
        ScanInterval::from_str(self.as_str()).unwrap_or(ScanInterval::Disabled)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ScanIntervalDto::Disabled => "DISABLED",
            ScanIntervalDto::Hourly => "HOURLY",
            ScanIntervalDto::Every6H => "EVERY_6H",
            ScanIntervalDto::Every12H => "EVERY_12H",
            ScanIntervalDto::Daily => "DAILY",
            ScanIntervalDto::Weekly => "WEEKLY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeriesCoverDto {
    #[serde(rename = "FIRST")]
    First,
    #[serde(rename = "FIRST_UNREAD_OR_FIRST")]
    FirstUnreadOrFirst,
    #[serde(rename = "FIRST_UNREAD_OR_LAST")]
    FirstUnreadOrLast,
    #[serde(rename = "LAST")]
    Last,
}

impl From<SeriesCover> for SeriesCoverDto {
    fn from(c: SeriesCover) -> Self {
        match c {
            SeriesCover::First => SeriesCoverDto::First,
            SeriesCover::FirstUnreadOrFirst => SeriesCoverDto::FirstUnreadOrFirst,
            SeriesCover::FirstUnreadOrLast => SeriesCoverDto::FirstUnreadOrLast,
            SeriesCover::Last => SeriesCoverDto::Last,
        }
    }
}

impl SeriesCoverDto {
    /// `SeriesCoverDto.toDomain()`
    pub fn to_domain(self) -> SeriesCover {
        SeriesCover::from_str(self.as_str()).unwrap_or(SeriesCover::First)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SeriesCoverDto::First => "FIRST",
            SeriesCoverDto::FirstUnreadOrFirst => "FIRST_UNREAD_OR_FIRST",
            SeriesCoverDto::FirstUnreadOrLast => "FIRST_UNREAD_OR_LAST",
            SeriesCoverDto::Last => "LAST",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_library() -> Library {
        Library {
            id: "l1".into(),
            name: "Manga".into(),
            root: "file:/data/manga/".into(),
            import_comicinfo_book: true,
            import_comicinfo_series: true,
            import_comicinfo_collection: false,
            import_comicinfo_readlist: false,
            import_comicinfo_series_append_volume: false,
            import_epub_book: true,
            import_epub_series: true,
            import_mylar_series: false,
            import_local_artwork: true,
            import_barcode_isbn: false,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: ScanInterval::Daily,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec!["#recycle".into()],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: SeriesCover::First,
            hash_files: true,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: crate::time_codec::now_utc(),
            last_modified_date: crate::time_codec::now_utc(),
        }
    }

    #[test]
    fn dto_shape_and_root_restriction() {
        let library = sample_library();
        let admin_dto = LibraryDto::of(&library, true);
        assert_eq!(admin_dto.root, "/data/manga");
        assert_eq!(admin_dto.scan_interval, ScanIntervalDto::Daily);
        assert!(!admin_dto.unavailable);

        let user_dto = LibraryDto::of(&library, false);
        assert_eq!(user_dto.root, "");

        let json = serde_json::to_value(&admin_dto).unwrap();
        assert_eq!(json["importComicInfoBook"], true);
        assert_eq!(json["scanInterval"], "DAILY");
        assert_eq!(json["seriesCover"], "FIRST");
        assert_eq!(json["oneshotsDirectory"], serde_json::Value::Null);
        assert_eq!(json["unavailable"], false);
    }

    #[test]
    fn enum_domain_mapping() {
        assert_eq!(ScanIntervalDto::Every6H.to_domain(), ScanInterval::Every6H);
        assert_eq!(
            SeriesCoverDto::FirstUnreadOrLast.to_domain(),
            SeriesCover::FirstUnreadOrLast
        );
        assert_eq!(
            ScanIntervalDto::from(ScanInterval::Every12H),
            ScanIntervalDto::Every12H
        );
    }
}
