//! `MylarSeriesProvider.kt`: series.json (Mylar) parsing and mapping.
//! Also hosts `OneShotSeriesProvider`'s pure patch computation (its DB lookup stays in the
//! service layer).

use crate::metadata::patch::{
    MetadataPatchTarget, MetadataProvider, SeriesMetadataPatch, SeriesMetadataProvider,
};
use komga_core::model::library::Library;
use komga_core::model::series::SeriesStatus;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

const SERIES_JSON: &str = "series.json";

#[derive(Debug, Deserialize)]
pub struct MylarSeries {
    pub metadata: MylarMetadata,
}

#[derive(Debug, Deserialize)]
pub struct MylarMetadata {
    #[serde(rename = "type")]
    pub type_: String,
    pub publisher: String,
    #[serde(default)]
    pub imprint: Option<String>,
    pub name: String,
    #[serde(alias = "cid")]
    pub comicid: String,
    pub year: i32,
    #[serde(default)]
    pub description_text: Option<String>,
    #[serde(default)]
    pub description_formatted: Option<String>,
    #[serde(default)]
    pub volume: Option<i32>,
    pub booktype: String,
    #[serde(default)]
    pub age_rating: Option<MylarAgeRating>,
    #[serde(alias = "ComicImage")]
    pub comic_image: String,
    pub total_issues: i32,
    pub publication_run: String,
    pub status: MylarStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum MylarStatus {
    Ended,
    Continuing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum MylarAgeRating {
    #[serde(rename = "All")]
    All,
    #[serde(rename = "9+")]
    Nine,
    #[serde(rename = "12+")]
    Twelve,
    #[serde(rename = "15+")]
    Fifteen,
    #[serde(rename = "17+")]
    Seventeen,
    #[serde(rename = "Adult")]
    Adult,
}

impl MylarAgeRating {
    pub fn age_rating(self) -> i32 {
        match self {
            MylarAgeRating::All => 0,
            MylarAgeRating::Nine => 9,
            MylarAgeRating::Twelve => 12,
            MylarAgeRating::Fifteen => 15,
            MylarAgeRating::Seventeen => 17,
            MylarAgeRating::Adult => 18,
        }
    }
}

pub struct MylarSeriesProvider;

impl SeriesMetadataProvider for MylarSeriesProvider {
    /// `getSeriesMetadata`: oneshot series are skipped; a missing or unparseable series.json
    /// yields no patch (Kotlin logs and returns null).
    fn get_series_metadata(
        &self,
        series_path: &Path,
        oneshot: bool,
    ) -> Option<SeriesMetadataPatch> {
        if oneshot {
            tracing::debug!("Disabled for oneshot series, skipping");
            return None;
        }
        let series_json_path = series_path.join(SERIES_JSON);
        if !series_json_path.exists() {
            tracing::debug!(
                "Series folder does not contain any {SERIES_JSON} file: {series_path:?}"
            );
            return None;
        }
        let bytes = std::fs::read(&series_json_path)
            .map_err(|e| {
                tracing::error!("Error while retrieving metadata from {SERIES_JSON}: {e}");
            })
            .ok()?;
        let series = serde_json::from_slice::<MylarSeries>(&bytes)
            .map_err(|e| {
                tracing::error!("Error while retrieving metadata from {SERIES_JSON}: {e}");
            })
            .ok()?;
        let metadata = series.metadata;

        let title = if metadata.volume.is_none_or(|v| v == 1) {
            metadata.name
        } else {
            format!("{} ({})", metadata.name, metadata.year)
        };

        Some(SeriesMetadataPatch {
            title: Some(title.clone()),
            title_sort: Some(title),
            status: Some(match metadata.status {
                MylarStatus::Ended => SeriesStatus::Ended,
                MylarStatus::Continuing => SeriesStatus::Ongoing,
            }),
            summary: metadata.description_formatted.or(metadata.description_text),
            reading_direction: None,
            publisher: Some(metadata.publisher),
            age_rating: metadata.age_rating.map(MylarAgeRating::age_rating),
            language: None,
            genres: None,
            // Ignore zero total_issues from mylar series.json: 0 is a placeholder for
            // "unknown" (e.g. still-running series), not a real count.
            total_book_count: (metadata.total_issues > 0).then_some(metadata.total_issues),
            collections: BTreeSet::new(),
        })
    }
}

impl MetadataProvider for MylarSeriesProvider {
    /// Only the SERIES target is handled (importMylarSeries); everything else is rejected.
    fn should_library_handle_patch(&self, library: &Library, target: MetadataPatchTarget) -> bool {
        matches!(target, MetadataPatchTarget::Series) && library.import_mylar_series
    }
}

/// `OneShotSeriesProvider.getSeriesMetadata`: the patch built from the single book's metadata
/// (title/titleSort = book title, ENDED, totalBookCount = 1).
pub fn compute_one_shot_patch(title: String, summary: String) -> SeriesMetadataPatch {
    SeriesMetadataPatch {
        title: Some(title.clone()),
        title_sort: Some(title),
        status: Some(SeriesStatus::Ended),
        summary: Some(summary),
        reading_direction: None,
        publisher: None,
        age_rating: None,
        language: None,
        genres: None,
        total_book_count: Some(1),
        collections: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn series_json(volume: Option<i32>) -> String {
        let volume = match volume {
            Some(v) => v.to_string(),
            None => "null".to_string(),
        };
        format!(
            r#"{{"metadata":{{
                "type":"comic",
                "publisher":"Hakusensha",
                "imprint":"Young Animal",
                "name":"Berserk",
                "comicid":"12345",
                "year":1989,
                "description_text":"Guts saga",
                "description_formatted":"<p>Guts saga</p>",
                "volume":{volume},
                "booktype":"comic",
                "age_rating":"Adult",
                "comic_image":"cover.jpg",
                "total_issues":41,
                "publication_run":"1989 - ",
                "status":"Ended"
            }}}}"#
        )
    }

    fn write_series_json(dir: &Path, content: &str) {
        let mut f = std::fs::File::create(dir.join(SERIES_JSON)).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    fn provider() -> MylarSeriesProvider {
        MylarSeriesProvider
    }

    #[test]
    fn full_metadata_volume_2() {
        let dir = std::env::temp_dir().join("kmrs-mylar-1");
        std::fs::create_dir_all(&dir).unwrap();
        write_series_json(&dir, &series_json(Some(2)));
        let patch = provider().get_series_metadata(&dir, false).unwrap();
        // volume != 1: title carries the year
        assert_eq!(patch.title.as_deref(), Some("Berserk (1989)"));
        assert_eq!(patch.title_sort.as_deref(), Some("Berserk (1989)"));
        assert_eq!(patch.status, Some(SeriesStatus::Ended));
        assert_eq!(
            patch.summary.as_deref(),
            Some("<p>Guts saga</p>"),
            "descriptionFormatted wins over descriptionText"
        );
        assert_eq!(patch.publisher.as_deref(), Some("Hakusensha"));
        assert_eq!(patch.age_rating, Some(18));
        assert_eq!(patch.total_book_count, Some(41));
        assert!(patch.collections.is_empty());
        assert!(patch.language.is_none() && patch.genres.is_none());
    }

    #[test]
    fn zero_total_issues_is_ignored() {
        let dir = std::env::temp_dir().join("kmrs-mylar-zero");
        std::fs::create_dir_all(&dir).unwrap();
        write_series_json(&dir, &series_json(Some(1)).replace("41", "0"));
        let patch = provider().get_series_metadata(&dir, false).unwrap();
        assert_eq!(patch.title.as_deref(), Some("Berserk"));
        assert_eq!(patch.total_book_count, None, "totalIssues == 0 is ignored");
    }

    #[test]
    fn volume_1_and_null_use_plain_name() {
        let dir = std::env::temp_dir().join("kmrs-mylar-2");
        std::fs::create_dir_all(&dir).unwrap();
        write_series_json(&dir, &series_json(Some(1)));
        let patch = provider().get_series_metadata(&dir, false).unwrap();
        assert_eq!(patch.title.as_deref(), Some("Berserk"));

        write_series_json(&dir, &series_json(None));
        let patch = provider().get_series_metadata(&dir, false).unwrap();
        assert_eq!(patch.title.as_deref(), Some("Berserk"));
    }

    #[test]
    fn continuing_maps_to_ongoing() {
        let dir = std::env::temp_dir().join("kmrs-mylar-3");
        std::fs::create_dir_all(&dir).unwrap();
        write_series_json(&dir, &series_json(Some(1)).replace("Ended", "Continuing"));
        let patch = provider().get_series_metadata(&dir, false).unwrap();
        assert_eq!(patch.status, Some(SeriesStatus::Ongoing));
    }

    #[test]
    fn oneshot_and_missing_and_broken() {
        let dir = std::env::temp_dir().join("kmrs-mylar-4");
        std::fs::create_dir_all(&dir).unwrap();
        write_series_json(&dir, &series_json(Some(1)));
        // oneshot series are skipped
        assert!(provider().get_series_metadata(&dir, true).is_none());
        // missing file
        assert!(provider()
            .get_series_metadata(&dir.join("nope"), false)
            .is_none());
        // unparseable
        write_series_json(&dir, "{not json");
        assert!(provider().get_series_metadata(&dir, false).is_none());
    }

    #[test]
    fn age_rating_values() {
        assert_eq!(MylarAgeRating::All.age_rating(), 0);
        assert_eq!(MylarAgeRating::Adult.age_rating(), 18);
        assert_eq!(
            serde_json::from_str::<MylarAgeRating>("\"17+\"").unwrap(),
            MylarAgeRating::Seventeen
        );
        assert!(serde_json::from_str::<MylarAgeRating>("\"MA 15+\"").is_err());
    }

    #[test]
    fn library_gate() {
        let mut library = komga_core::model::library::Library {
            id: "l1".into(),
            name: "lib".into(),
            root: "file:/data/".into(),
            import_comicinfo_book: true,
            import_comicinfo_series: true,
            import_comicinfo_collection: true,
            import_comicinfo_readlist: true,
            import_comicinfo_series_append_volume: true,
            import_epub_book: true,
            import_epub_series: true,
            import_mylar_series: true,
            import_local_artwork: true,
            import_barcode_isbn: true,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: komga_core::model::library::ScanInterval::Every6H,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: komga_core::model::library::SeriesCover::First,
            hash_files: true,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: komga_core::time_codec::now_utc(),
            last_modified_date: komga_core::time_codec::now_utc(),
        };
        let provider = provider();
        assert!(provider.should_library_handle_patch(&library, MetadataPatchTarget::Series));
        assert!(!provider.should_library_handle_patch(&library, MetadataPatchTarget::Book));
        assert!(!provider.should_library_handle_patch(&library, MetadataPatchTarget::Collection));
        assert!(!provider.should_library_handle_patch(&library, MetadataPatchTarget::ReadList));
        library.import_mylar_series = false;
        assert!(!provider.should_library_handle_patch(&library, MetadataPatchTarget::Series));
    }

    #[test]
    fn one_shot_patch() {
        let patch = compute_one_shot_patch("Book Title".to_string(), "A summary".to_string());
        assert_eq!(patch.title.as_deref(), Some("Book Title"));
        assert_eq!(patch.title_sort.as_deref(), Some("Book Title"));
        assert_eq!(patch.status, Some(SeriesStatus::Ended));
        assert_eq!(patch.summary.as_deref(), Some("A summary"));
        assert_eq!(patch.total_book_count, Some(1));
    }
}
