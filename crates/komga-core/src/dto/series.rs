//! `SeriesDto.kt` / `SeriesMetadataDto` / `BookMetadataAggregationDto`.

use super::common::{AlternateTitleDto, AuthorDto, WebLinkDto};
use super::{dto_date, dto_datetime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesDto {
    pub id: String,
    pub library_id: String,
    pub name: String,
    pub url: String,
    #[serde(with = "dto_datetime")]
    pub created: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub file_last_modified: OffsetDateTime,
    pub books_count: i32,
    pub books_read_count: i32,
    pub books_unread_count: i32,
    pub books_in_progress_count: i32,
    pub metadata: SeriesMetadataDto,
    pub books_metadata: BookMetadataAggregationDto,
    pub deleted: bool,
    pub oneshot: bool,
}

impl SeriesDto {
    /// `SeriesDto.restrictUrl`: restricted (non-admin) users get an empty url
    pub fn restrict_url(mut self, restrict: bool) -> Self {
        if restrict {
            self.url = String::new();
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesMetadataDto {
    pub status: String,
    pub status_lock: bool,
    pub title: String,
    pub title_lock: bool,
    pub title_sort: String,
    pub title_sort_lock: bool,
    pub summary: String,
    pub summary_lock: bool,
    pub reading_direction: String,
    pub reading_direction_lock: bool,
    pub publisher: String,
    pub publisher_lock: bool,
    pub age_rating: Option<i32>,
    pub age_rating_lock: bool,
    pub language: String,
    pub language_lock: bool,
    pub genres: BTreeSet<String>,
    pub genres_lock: bool,
    pub tags: BTreeSet<String>,
    pub tags_lock: bool,
    pub total_book_count: Option<i32>,
    pub total_book_count_lock: bool,
    pub sharing_labels: BTreeSet<String>,
    pub sharing_labels_lock: bool,
    pub links: Vec<WebLinkDto>,
    pub links_lock: bool,
    pub alternate_titles: Vec<AlternateTitleDto>,
    pub alternate_titles_lock: bool,
    #[serde(with = "dto_datetime")]
    pub created: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookMetadataAggregationDto {
    pub authors: Vec<AuthorDto>,
    pub tags: BTreeSet<String>,
    #[serde(with = "dto_date")]
    pub release_date: Option<Date>,
    pub summary: String,
    pub summary_number: String,
    #[serde(with = "dto_datetime")]
    pub created: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_series() -> SeriesDto {
        let created = crate::time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap();
        SeriesDto {
            id: "s1".into(),
            library_id: "l1".into(),
            name: "Berserk".into(),
            url: "/data/berserk/".into(),
            created,
            last_modified: created,
            file_last_modified: created,
            books_count: 3,
            books_read_count: 1,
            books_unread_count: 1,
            books_in_progress_count: 1,
            metadata: SeriesMetadataDto {
                status: "ONGOING".into(),
                status_lock: false,
                title: "Berserk".into(),
                title_lock: false,
                title_sort: "Berserk".into(),
                title_sort_lock: false,
                summary: String::new(),
                summary_lock: false,
                reading_direction: "RIGHT_TO_LEFT".into(),
                reading_direction_lock: false,
                publisher: "Hakusensha".into(),
                publisher_lock: false,
                age_rating: Some(18),
                age_rating_lock: false,
                language: "ja".into(),
                language_lock: false,
                genres: BTreeSet::new(),
                genres_lock: false,
                tags: BTreeSet::new(),
                tags_lock: false,
                total_book_count: None,
                total_book_count_lock: false,
                sharing_labels: BTreeSet::new(),
                sharing_labels_lock: false,
                links: vec![],
                links_lock: false,
                alternate_titles: vec![],
                alternate_titles_lock: false,
                created,
                last_modified: created,
            },
            books_metadata: BookMetadataAggregationDto {
                authors: vec![],
                tags: BTreeSet::new(),
                release_date: None,
                summary: String::new(),
                summary_number: String::new(),
                created,
                last_modified: created,
            },
            deleted: false,
            oneshot: false,
        }
    }

    #[test]
    fn json_shape() {
        let json = serde_json::to_value(sample_series()).unwrap();
        assert_eq!(json["created"], "2024-01-02T03:04:05Z");
        assert_eq!(json["metadata"]["ageRating"], 18);
        // no NON_NULL: absent values serialize as null
        assert_eq!(json["metadata"]["totalBookCount"], serde_json::Value::Null);
        assert_eq!(
            json["booksMetadata"]["releaseDate"],
            serde_json::Value::Null
        );
        assert_eq!(json["booksCount"], 3);
        assert_eq!(json["deleted"], false);
    }

    #[test]
    fn restrict_url() {
        let series = sample_series();
        assert_eq!(series.clone().restrict_url(true).url, "");
        assert_eq!(series.restrict_url(false).url, "/data/berserk/");
    }

    #[test]
    fn roundtrip() {
        let series = sample_series();
        let json = serde_json::to_string(&series).unwrap();
        assert_eq!(serde_json::from_str::<SeriesDto>(&json).unwrap(), series);
    }
}
