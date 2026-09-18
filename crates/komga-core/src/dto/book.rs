//! `BookDto.kt` / `MediaDto` / `BookMetadataDto` / `ReadProgressDto` / `PageDto.kt`.

use super::common::{AuthorDto, WebLinkDto};
use super::{dto_date, dto_datetime, format_binary_bytes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookDto {
    pub id: String,
    pub series_id: String,
    pub series_title: String,
    pub library_id: String,
    pub name: String,
    pub url: String,
    pub number: i32,
    #[serde(with = "dto_datetime")]
    pub created: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub file_last_modified: OffsetDateTime,
    pub size_bytes: i64,
    pub size: String,
    pub media: MediaDto,
    pub metadata: BookMetadataDto,
    pub read_progress: Option<ReadProgressDto>,
    pub deleted: bool,
    pub file_hash: String,
    pub oneshot: bool,
}

impl BookDto {
    pub fn size_of(size_bytes: i64) -> String {
        format_binary_bytes(size_bytes)
    }

    /// `BookDto.restrictUrl`: restricted (non-admin) users get only the file name
    /// (`FilenameUtils.getName`: last segment after `/` or `\`)
    pub fn restrict_url(mut self, restrict: bool) -> Self {
        if restrict {
            self.url = self
                .url
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&self.url)
                .to_string();
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDto {
    pub status: String,
    pub media_type: String,
    pub pages_count: i32,
    pub comment: String,
    pub epub_divina_compatible: bool,
    pub epub_is_kepub: bool,
    /// Derived from `mediaType` (`MediaType.fromMediaType(..)?.profile?.name ?: ""`)
    pub media_profile: String,
}

impl MediaDto {
    pub fn media_profile_of(media_type: &str) -> String {
        match media_type {
            "application/zip"
            | "application/x-rar-compressed"
            | "application/x-rar-compressed; version=4"
            | "application/x-rar-compressed; version=5" => "DIVINA",
            "application/pdf" => "PDF",
            "application/epub+zip" => "EPUB",
            _ => "",
        }
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookMetadataDto {
    pub title: String,
    pub title_lock: bool,
    pub summary: String,
    pub summary_lock: bool,
    pub number: String,
    pub number_lock: bool,
    pub number_sort: f32,
    pub number_sort_lock: bool,
    #[serde(with = "dto_date")]
    pub release_date: Option<Date>,
    pub release_date_lock: bool,
    pub authors: Vec<AuthorDto>,
    pub authors_lock: bool,
    pub tags: BTreeSet<String>,
    pub tags_lock: bool,
    pub isbn: String,
    pub isbn_lock: bool,
    pub links: Vec<WebLinkDto>,
    pub links_lock: bool,
    #[serde(with = "dto_datetime")]
    pub created: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadProgressDto {
    pub page: i32,
    pub completed: bool,
    #[serde(with = "dto_datetime")]
    pub read_date: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub created: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified: OffsetDateTime,
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDto {
    pub number: i32,
    pub file_name: String,
    pub media_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub size_bytes: Option<i64>,
    pub size: String,
}

impl PageDto {
    pub fn size_of(size_bytes: Option<i64>) -> String {
        size_bytes.map(format_binary_bytes).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_book() -> BookDto {
        let created = crate::time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap();
        BookDto {
            id: "b1".into(),
            series_id: "s1".into(),
            series_title: "Berserk".into(),
            library_id: "l1".into(),
            name: "Berserk v01".into(),
            url: "/data/berserk/Berserk v01.cbz".into(),
            number: 1,
            created,
            last_modified: created,
            file_last_modified: created,
            size_bytes: 1024,
            size: BookDto::size_of(1024),
            media: MediaDto {
                status: "READY".into(),
                media_type: "application/zip".into(),
                pages_count: 10,
                comment: String::new(),
                epub_divina_compatible: false,
                epub_is_kepub: false,
                media_profile: MediaDto::media_profile_of("application/zip"),
            },
            metadata: BookMetadataDto {
                title: "Berserk v01".into(),
                title_lock: false,
                summary: String::new(),
                summary_lock: false,
                number: "1".into(),
                number_lock: false,
                number_sort: 1.0,
                number_sort_lock: false,
                release_date: None,
                release_date_lock: false,
                authors: vec![],
                authors_lock: false,
                tags: BTreeSet::new(),
                tags_lock: false,
                isbn: String::new(),
                isbn_lock: false,
                links: vec![],
                links_lock: false,
                created,
                last_modified: created,
            },
            read_progress: None,
            deleted: false,
            file_hash: String::new(),
            oneshot: false,
        }
    }

    #[test]
    fn restrict_url_keeps_file_name() {
        let book = sample_book();
        assert_eq!(book.clone().restrict_url(true).url, "Berserk v01.cbz");
        assert_eq!(
            book.restrict_url(false).url,
            "/data/berserk/Berserk v01.cbz"
        );
    }

    #[test]
    fn json_shape() {
        let json = serde_json::to_value(sample_book()).unwrap();
        assert_eq!(json["created"], "2024-01-02T03:04:05Z");
        assert_eq!(json["size"], "1 KiB");
        assert_eq!(json["media"]["mediaProfile"], "DIVINA");
        assert_eq!(json["readProgress"], serde_json::Value::Null);
        assert_eq!(json["metadata"]["releaseDate"], serde_json::Value::Null);
    }

    #[test]
    fn roundtrip() {
        let book = sample_book();
        let json = serde_json::to_string(&book).unwrap();
        assert_eq!(serde_json::from_str::<BookDto>(&json).unwrap(), book);
    }

    #[test]
    fn page_json_shape() {
        let page = PageDto {
            number: 1,
            file_name: "p1.jpg".into(),
            media_type: "image/jpeg".into(),
            width: Some(800),
            height: Some(1200),
            size_bytes: Some(1536),
            size: PageDto::size_of(Some(1536)),
        };
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["size"], "1.5 KiB");
        assert_eq!(json["width"], 800);
    }
}
