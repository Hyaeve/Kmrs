//! Request DTOs for the library endpoints: `LibraryCreationDto.kt` / `LibraryUpdateDto.kt`.

use komga_core::dto::library::{ScanIntervalDto, SeriesCoverDto};
use serde::Deserialize;
use std::collections::BTreeSet;

/// serde_json cannot distinguish "key absent" from "key: null" for `Option<Option<T>>` on its
/// own; this restores the distinction komga's `isSet` tracking needs.
fn deserialize_some<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

fn default_true() -> bool {
    true
}

fn default_scan_interval() -> ScanIntervalDto {
    ScanIntervalDto::Every6H
}

fn default_series_cover() -> SeriesCoverDto {
    SeriesCoverDto::First
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCreationDto {
    pub name: String,
    pub root: String,
    #[serde(default = "default_true", rename = "importComicInfoBook")]
    pub import_comicinfo_book: bool,
    #[serde(default = "default_true", rename = "importComicInfoSeries")]
    pub import_comicinfo_series: bool,
    #[serde(default = "default_true", rename = "importComicInfoCollection")]
    pub import_comicinfo_collection: bool,
    #[serde(default = "default_true", rename = "importComicInfoReadList")]
    pub import_comicinfo_readlist: bool,
    #[serde(default = "default_true", rename = "importComicInfoSeriesAppendVolume")]
    pub import_comicinfo_series_append_volume: bool,
    #[serde(default = "default_true")]
    pub import_epub_book: bool,
    #[serde(default = "default_true")]
    pub import_epub_series: bool,
    #[serde(default = "default_true")]
    pub import_mylar_series: bool,
    #[serde(default = "default_true")]
    pub import_local_artwork: bool,
    #[serde(default = "default_true")]
    pub import_barcode_isbn: bool,
    #[serde(default)]
    pub scan_force_modified_time: bool,
    #[serde(default = "default_scan_interval")]
    pub scan_interval: ScanIntervalDto,
    #[serde(default)]
    pub scan_on_startup: bool,
    #[serde(default = "default_true")]
    pub scan_cbx: bool,
    #[serde(default = "default_true")]
    pub scan_pdf: bool,
    #[serde(default = "default_true")]
    pub scan_epub: bool,
    #[serde(default)]
    pub scan_directory_exclusions: BTreeSet<String>,
    #[serde(default)]
    pub repair_extensions: bool,
    #[serde(default)]
    pub convert_to_cbz: bool,
    #[serde(default)]
    pub empty_trash_after_scan: bool,
    #[serde(default = "default_series_cover")]
    pub series_cover: SeriesCoverDto,
    #[serde(default = "default_true")]
    pub hash_files: bool,
    #[serde(default)]
    pub hash_pages: bool,
    #[serde(default)]
    pub hash_koreader: bool,
    #[serde(default = "default_true")]
    pub analyze_dimensions: bool,
    pub oneshots_directory: Option<String>,
}

/// All fields optional; omit a field to keep the current value.
/// `scanDirectoryExclusions` and `oneshotsDirectory` use the double-Option isSet semantics of
/// komga's delegate-tracked properties: outer None = unchanged, Some(None) = cleared,
/// Some(Some(v)) = set.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LibraryUpdateDto {
    pub name: Option<String>,
    pub root: Option<String>,
    #[serde(rename = "importComicInfoBook")]
    pub import_comicinfo_book: Option<bool>,
    #[serde(rename = "importComicInfoSeries")]
    pub import_comicinfo_series: Option<bool>,
    #[serde(rename = "importComicInfoCollection")]
    pub import_comicinfo_collection: Option<bool>,
    #[serde(rename = "importComicInfoReadList")]
    pub import_comicinfo_readlist: Option<bool>,
    #[serde(rename = "importComicInfoSeriesAppendVolume")]
    pub import_comicinfo_series_append_volume: Option<bool>,
    pub import_epub_book: Option<bool>,
    pub import_epub_series: Option<bool>,
    pub import_mylar_series: Option<bool>,
    pub import_local_artwork: Option<bool>,
    pub import_barcode_isbn: Option<bool>,
    pub scan_force_modified_time: Option<bool>,
    pub scan_interval: Option<ScanIntervalDto>,
    pub scan_on_startup: Option<bool>,
    pub scan_cbx: Option<bool>,
    pub scan_pdf: Option<bool>,
    pub scan_epub: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub scan_directory_exclusions: Option<Option<BTreeSet<String>>>,
    pub repair_extensions: Option<bool>,
    pub convert_to_cbz: Option<bool>,
    pub empty_trash_after_scan: Option<bool>,
    pub series_cover: Option<SeriesCoverDto>,
    pub hash_files: Option<bool>,
    pub hash_pages: Option<bool>,
    pub hash_koreader: Option<bool>,
    pub analyze_dimensions: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub oneshots_directory: Option<Option<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_defaults_match_kotlin() {
        let dto: LibraryCreationDto =
            serde_json::from_str(r#"{"name":"Manga","root":"/data"}"#).unwrap();
        assert!(dto.import_comicinfo_book);
        assert!(dto.import_epub_book);
        assert!(dto.scan_cbx && dto.scan_pdf && dto.scan_epub);
        assert_eq!(dto.scan_interval, ScanIntervalDto::Every6H);
        assert_eq!(dto.series_cover, SeriesCoverDto::First);
        assert!(dto.hash_files);
        assert!(!dto.hash_pages);
        assert!(!dto.scan_force_modified_time);
        assert!(dto.analyze_dimensions);
        assert!(dto.scan_directory_exclusions.is_empty());
        assert_eq!(dto.oneshots_directory, None);
    }

    #[test]
    fn update_isset_semantics() {
        // omitted: outer None
        let dto: LibraryUpdateDto = serde_json::from_str(r#"{"name":"X"}"#).unwrap();
        assert_eq!(dto.scan_directory_exclusions, None);
        assert_eq!(dto.oneshots_directory, None);

        // explicit null: Some(None) = cleared
        let dto: LibraryUpdateDto =
            serde_json::from_str(r#"{"scanDirectoryExclusions":null,"oneshotsDirectory":null}"#)
                .unwrap();
        assert_eq!(dto.scan_directory_exclusions, Some(None));
        assert_eq!(dto.oneshots_directory, Some(None));

        // explicit value
        let dto: LibraryUpdateDto =
            serde_json::from_str(r#"{"oneshotsDirectory":"one-shots"}"#).unwrap();
        assert_eq!(dto.oneshots_directory, Some(Some("one-shots".to_string())));
    }
}
