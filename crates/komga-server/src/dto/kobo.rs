//! Kobo sync DTOs, ported from `interfaces/api/kobo/dto/`.
//!
//! All use Jackson's UpperCamelCase naming; `NON_NULL` classes skip null fields. `ZonedDateTime`
//! fields serialize with Jackson's `ISO_OFFSET_DATE_TIME` semantics (nanos padded to 9 digits,
//! trailing zeros stripped, `Z` for UTC) via `komga_core::time_codec::format_offset_date_time`.

use komga_core::model::read_progress::ReadProgress;
use komga_core::time_codec;
use komga_db::dto_dao::kobo::KoboBookMetadataRow;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub const DUMMY_ID: &str = "00000000-0000-0000-0000-000000000001";

/// Jackson `ISO_OFFSET_DATE_TIME` for the Kobo `ZonedDateTime` fields
pub fn format_zoned(dt: OffsetDateTime) -> String {
    time_codec::format_offset_date_time(dt)
}

mod zoned {
    use serde::{Deserializer, Serializer};
    use time::OffsetDateTime;

    pub fn serialize<S: Serializer>(dt: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::format_zoned(*dt))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<OffsetDateTime, D::Error> {
        time::serde::iso8601::deserialize(deserializer)
    }
}

mod zoned_opt {
    use serde::{Deserializer, Serializer};
    use time::OffsetDateTime;

    pub fn serialize<S: Serializer>(
        dt: &Option<OffsetDateTime>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match dt {
            Some(dt) => serializer.serialize_str(&super::format_zoned(*dt)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<OffsetDateTime>, D::Error> {
        Ok(Some(time::serde::iso8601::deserialize(deserializer)?))
    }
}

// region small value objects

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AmountDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency_code: Option<String>,
    pub total_amount: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContributorDto {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PublisherDto {
    #[serde(default)]
    pub imprint: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct KoboSeriesDto {
    pub id: String,
    pub name: String,
    pub number: String,
    #[serde(rename = "NumberFloat")]
    pub number_float: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PeriodDto {
    #[serde(with = "zoned")]
    pub from: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LocationDto {
    /// For type=KoboSpan values are in the form "kobo.x.y"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Typically "KoboSpan"
    #[serde(rename = "Type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The epub HTML resource
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormatDto {
    #[serde(rename = "EPUB3FL")]
    Epub3fl,
    #[serde(rename = "EPUB")]
    Epub,
    #[serde(rename = "EPUB3")]
    Epub3,
    #[serde(rename = "KEPUB")]
    Kepub,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DownloadUrlDto {
    #[serde(rename = "DrmType", default = "none_drm")]
    pub drm_type: String,
    pub format: FormatDto,
    pub size: i64,
    #[serde(default = "generic_platform")]
    pub platform: String,
    #[serde(rename = "Url")]
    pub url: String,
}

fn none_drm() -> String {
    "None".to_string()
}
fn generic_platform() -> String {
    "Generic".to_string()
}

// endregion

// region book metadata

/// `KoboBookMetadataDto`; `isKepub`/`isPrePaginated`/`fileSize` are `@JsonIgnore` in Jackson
/// (used server-side only), so they do not serialize.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct KoboBookMetadataDto {
    #[serde(rename = "Categories")]
    pub categories: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "ContributorRoles")]
    pub contributor_roles: Vec<ContributorDto>,
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "Contributors")]
    pub contributors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "CoverImageId")]
    pub cover_image_id: Option<String>,
    #[serde(rename = "CrossRevisionId")]
    pub cross_revision_id: String,
    #[serde(rename = "CurrentDisplayPrice")]
    pub current_display_price: AmountDto,
    #[serde(rename = "CurrentLoveDisplayPrice")]
    pub current_love_display_price: AmountDto,
    #[serde(skip_serializing_if = "Option::is_none", rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "DownloadUrls")]
    pub download_urls: Vec<DownloadUrlDto>,
    #[serde(rename = "EntitlementId")]
    pub entitlement_id: String,
    #[serde(rename = "ExternalIds")]
    pub external_ids: Vec<String>,
    #[serde(rename = "Genre")]
    pub genre: String,
    #[serde(rename = "IsEligibleForKoboLove")]
    pub is_eligible_for_kobo_love: bool,
    #[serde(rename = "IsInternetArchive")]
    pub is_internet_archive: bool,
    #[serde(rename = "IsPreOrder")]
    pub is_pre_order: bool,
    #[serde(rename = "IsSocialEnabled")]
    pub is_social_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none", rename = "Isbn")]
    pub isbn: Option<String>,
    /// 2-letter code
    #[serde(rename = "Language")]
    pub language: String,
    #[serde(rename = "PhoneticPronunciations")]
    pub phonetic_pronunciations: std::collections::BTreeMap<String, String>,
    #[serde(
        with = "zoned_opt",
        skip_serializing_if = "Option::is_none",
        rename = "PublicationDate"
    )]
    pub publication_date: Option<OffsetDateTime>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "Publisher")]
    pub publisher: Option<PublisherDto>,
    #[serde(rename = "RevisionId")]
    pub revision_id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "Series")]
    pub series: Option<KoboSeriesDto>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "Slug")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "SubTitle")]
    pub sub_title: Option<String>,
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "WorkId")]
    pub work_id: String,
    #[serde(skip)]
    pub is_kepub: bool,
    #[serde(skip)]
    pub is_pre_paginated: bool,
    #[serde(skip)]
    pub file_size: i64,
}

impl From<&KoboBookMetadataRow> for KoboBookMetadataDto {
    fn from(row: &KoboBookMetadataRow) -> Self {
        let publication_date = row
            .release_date
            .map(|d| d.midnight().assume_utc())
            .unwrap_or(row.created_date);
        Self {
            categories: vec![DUMMY_ID.to_string()],
            contributor_roles: row
                .authors
                .iter()
                .map(|name| ContributorDto { name: name.clone() })
                .collect(),
            contributors: row.authors.clone(),
            cover_image_id: row.cover_image_id.clone(),
            cross_revision_id: row.book_id.clone(),
            current_display_price: AmountDto {
                currency_code: Some("USD".to_string()),
                total_amount: 0,
            },
            current_love_display_price: AmountDto {
                currency_code: None,
                total_amount: 0,
            },
            // an empty summary would make Kobo skip the update, so it is forced to a blank space
            description: Some(if row.summary.is_empty() {
                " ".to_string()
            } else {
                row.summary.clone()
            }),
            download_urls: vec![],
            entitlement_id: row.book_id.clone(),
            external_ids: vec![],
            genre: DUMMY_ID.to_string(),
            is_eligible_for_kobo_love: false,
            is_internet_archive: false,
            is_pre_order: false,
            is_social_enabled: true,
            isbn: if row.isbn.is_empty() {
                None
            } else {
                Some(row.isbn.clone())
            },
            language: {
                let code: String = row.language.chars().take(2).collect();
                if code.is_empty() {
                    "en".to_string()
                } else {
                    code
                }
            },
            phonetic_pronunciations: Default::default(),
            publication_date: Some(publication_date),
            publisher: Some(PublisherDto {
                imprint: String::new(),
                name: row.publisher.clone(),
            }),
            revision_id: row.book_id.clone(),
            series: if row.oneshot {
                None
            } else {
                Some(KoboSeriesDto {
                    id: row.series_id.clone(),
                    name: row.series_title.clone(),
                    number: row.number.clone(),
                    number_float: row.number_sort,
                })
            },
            slug: None,
            sub_title: None,
            title: row.title.clone(),
            work_id: row.book_id.clone(),
            is_kepub: row.epub_is_kepub,
            is_pre_paginated: row.is_pre_paginated,
            file_size: row.file_size,
        }
    }
}

// endregion

// region entitlement

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct BookEntitlementDto {
    #[serde(rename = "Accessibility")]
    pub accessibility: String,
    pub active_period: PeriodDto,
    pub created: String,
    pub cross_revision_id: String,
    pub id: String,
    pub is_hidden_from_archive: bool,
    pub is_locked: bool,
    /// True if the book has been deleted or is not available
    pub is_removed: bool,
    pub last_modified: String,
    #[serde(rename = "OriginCategory")]
    pub origin_category: String,
    pub revision_id: String,
    #[serde(rename = "Status")]
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct BookEntitlementContainerDto {
    pub book_entitlement: BookEntitlementDto,
    pub book_metadata: KoboBookMetadataDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading_state: Option<ReadingStateDto>,
}

/// `SyncPoint.Book.toBookEntitlementDto(isRemoved)`
pub fn book_entitlement_of(
    book: &komga_core::model::sync_point::SyncPointBook,
    is_removed: bool,
    now: OffsetDateTime,
) -> BookEntitlementDto {
    BookEntitlementDto {
        accessibility: "Full".to_string(),
        active_period: PeriodDto { from: now },
        created: format_zoned(book.book_created_date),
        cross_revision_id: book.book_id.clone(),
        id: book.book_id.clone(),
        is_hidden_from_archive: false,
        is_locked: false,
        is_removed,
        last_modified: format_zoned(book.book_file_last_modified),
        origin_category: "Imported".to_string(),
        revision_id: book.book_id.clone(),
        status: "Active".to_string(),
    }
}

// endregion

// region reading state

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusDto {
    #[serde(rename = "ReadyToRead")]
    ReadyToRead,
    #[serde(rename = "Finished")]
    Finished,
    #[serde(rename = "Reading")]
    Reading,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BookmarkDto {
    #[serde(with = "zoned")]
    pub last_modified: OffsetDateTime,
    /// Total progression in the book, between 0 and 100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_percent: Option<f32>,
    /// Progression within the resource, between 0 and 100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_source_progress_percent: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StatisticsDto {
    #[serde(with = "zoned")]
    pub last_modified: OffsetDateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_time_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spent_reading_minutes: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StatusInfoDto {
    #[serde(with = "zoned")]
    pub last_modified: OffsetDateTime,
    pub status: StatusDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times_started_reading: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "zoned_opt")]
    pub last_time_finished: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "zoned_opt")]
    pub last_time_started_reading: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReadingStateDto {
    #[serde(default, with = "zoned_opt", skip_serializing_if = "Option::is_none")]
    pub created: Option<OffsetDateTime>,
    pub current_bookmark: BookmarkDto,
    pub entitlement_id: String,
    #[serde(with = "zoned")]
    pub last_modified: OffsetDateTime,
    #[serde(default, with = "zoned_opt", skip_serializing_if = "Option::is_none")]
    pub priority_timestamp: Option<OffsetDateTime>,
    pub statistics: StatisticsDto,
    pub status_info: StatusInfoDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WrappedReadingStateDto {
    pub reading_state: ReadingStateDto,
}

/// `ReadProgress.toDto()`
pub fn reading_state_of(progress: &ReadProgress) -> ReadingStateDto {
    let last = progress.last_modified_date;
    ReadingStateDto {
        created: Some(progress.created_date),
        current_bookmark: BookmarkDto {
            last_modified: last,
            progress_percent: progress
                .locator
                .as_ref()
                .and_then(|l| l.get("locations"))
                .and_then(|l| l.get("totalProgression"))
                .and_then(|v| v.as_f64())
                .map(|v| (v * 100.0) as f32),
            content_source_progress_percent: progress
                .locator
                .as_ref()
                .and_then(|l| l.get("locations"))
                .and_then(|l| l.get("progression"))
                .and_then(|v| v.as_f64())
                .map(|v| (v * 100.0) as f32),
            location: progress.locator.as_ref().map(|l| LocationDto {
                source: l
                    .get("href")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                value: l
                    .get("koboSpan")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                type_: Some("KoboSpan".to_string()),
            }),
        },
        entitlement_id: progress.book_id.clone(),
        last_modified: last,
        priority_timestamp: Some(last),
        statistics: StatisticsDto {
            last_modified: last,
            remaining_time_minutes: None,
            spent_reading_minutes: None,
        },
        status_info: StatusInfoDto {
            last_modified: last,
            status: if progress.completed {
                StatusDto::Finished
            } else {
                StatusDto::Reading
            },
            times_started_reading: Some(1),
            last_time_finished: None,
            last_time_started_reading: None,
        },
    }
}

/// `getEmptyReadProgressForBook`
pub fn empty_reading_state(book_id: &str, created: OffsetDateTime) -> ReadingStateDto {
    ReadingStateDto {
        created: Some(created),
        current_bookmark: BookmarkDto {
            last_modified: created,
            progress_percent: None,
            content_source_progress_percent: None,
            location: None,
        },
        entitlement_id: book_id.to_string(),
        last_modified: created,
        priority_timestamp: Some(created),
        statistics: StatisticsDto {
            last_modified: created,
            remaining_time_minutes: None,
            spent_reading_minutes: None,
        },
        status_info: StatusInfoDto {
            last_modified: created,
            status: StatusDto::ReadyToRead,
            times_started_reading: Some(0),
            last_time_finished: None,
            last_time_started_reading: None,
        },
    }
}

// endregion

// region update state

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReadingStateStateUpdateDto {
    #[serde(default)]
    pub reading_states: Vec<ReadingStateDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ResultDto {
    #[serde(rename = "Success")]
    Success,
    #[serde(rename = "Failure")]
    Failure,
    #[serde(rename = "Ignored")]
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct WrappedResultDto {
    pub result: ResultDto,
}

impl ResultDto {
    pub fn wrapped(self) -> WrappedResultDto {
        WrappedResultDto { result: self }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReadingStateUpdateResultDto {
    pub entitlement_id: String,
    pub current_bookmark_result: WrappedResultDto,
    pub statistics_result: WrappedResultDto,
    pub status_info_result: WrappedResultDto,
}

/// `UpdateResultDto` is an interface with a single implementation in komga, so the wire shape is
/// just the struct's fields (no discriminator).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum UpdateResultDto {
    ReadingState(ReadingStateUpdateResultDto),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct RequestResultDto {
    pub request_result: ResultDto,
    pub update_results: Vec<UpdateResultDto>,
}

pub fn update_success(book_id: &str) -> RequestResultDto {
    RequestResultDto {
        request_result: ResultDto::Success,
        update_results: vec![UpdateResultDto::ReadingState(ReadingStateUpdateResultDto {
            entitlement_id: book_id.to_string(),
            current_bookmark_result: ResultDto::Success.wrapped(),
            statistics_result: ResultDto::Ignored.wrapped(),
            status_info_result: ResultDto::Success.wrapped(),
        })],
    }
}

pub fn update_failure(book_id: &str) -> RequestResultDto {
    RequestResultDto {
        request_result: ResultDto::Failure,
        update_results: vec![UpdateResultDto::ReadingState(ReadingStateUpdateResultDto {
            entitlement_id: book_id.to_string(),
            current_bookmark_result: ResultDto::Failure.wrapped(),
            statistics_result: ResultDto::Failure.wrapped(),
            status_info_result: ResultDto::Failure.wrapped(),
        })],
    }
}

// endregion

// region tags

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TagTypeDto {
    // the Kobo protocol defines both tag kinds; komga only ever emits UserTag
    #[allow(dead_code)]
    #[serde(rename = "SystemTag")]
    SystemTag,
    #[serde(rename = "UserTag")]
    UserTag,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TagItemDto {
    pub revision_id: String,
    #[serde(rename = "Type")]
    pub type_: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TagDto {
    pub id: String,
    pub created: String,
    pub last_modified: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub type_: TagTypeDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<TagItemDto>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct WrappedTagDto {
    pub tag: TagDto,
}

/// `SyncPoint.ReadList.toWrappedTagDto(items)`
pub fn wrapped_tag_of(
    readlist: &komga_core::model::sync_point::SyncPointReadList,
    items: Option<Vec<TagItemDto>>,
) -> WrappedTagDto {
    WrappedTagDto {
        tag: TagDto {
            id: readlist.readlist_id.clone(),
            created: format_zoned(readlist.readlist_created_date),
            last_modified: format_zoned(readlist.readlist_last_modified_date),
            name: readlist.readlist_name.clone(),
            type_: TagTypeDto::UserTag,
            items,
        },
    }
}

// endregion

// region sync result

/// `SyncResultDto` is an interface; each wrapper serializes as a single-key object.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SyncResultDto {
    NewEntitlement {
        #[serde(rename = "NewEntitlement")]
        new_entitlement: BookEntitlementContainerDto,
    },
    ChangedEntitlement {
        #[serde(rename = "ChangedEntitlement")]
        changed_entitlement: BookEntitlementContainerDto,
    },
    ChangedProductMetadata {
        #[serde(rename = "ChangedProductMetadata")]
        changed_product_metadata: KoboBookMetadataDto,
    },
    NewTag {
        #[serde(rename = "NewTag")]
        new_tag: WrappedTagDto,
    },
    ChangedTag {
        #[serde(rename = "ChangedTag")]
        changed_tag: WrappedTagDto,
    },
    DeletedTag {
        #[serde(rename = "DeletedTag")]
        deleted_tag: WrappedTagDto,
    },
    ChangedReadingState {
        #[serde(rename = "ChangedReadingState")]
        changed_reading_state: WrappedReadingStateDto,
    },
}

// endregion

// region misc endpoints DTOs

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthDto {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(rename = "TokenType")]
    pub token_type: String,
    pub tracking_id: String,
    pub user_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ResourcesDto {
    pub resources: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TestsDto {
    pub result: String,
    pub test_key: String,
    #[serde(rename = "Tests")]
    pub tests: std::collections::BTreeMap<String, String>,
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row() -> KoboBookMetadataRow {
        KoboBookMetadataRow {
            book_id: "b1".into(),
            title: "Berserk v01".into(),
            number: "1".into(),
            number_sort: 1.0,
            isbn: "9781593070205".into(),
            summary: "Guts".into(),
            release_date: None,
            created_date: time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap(),
            series_id: "s1".into(),
            series_title: "Berserk".into(),
            publisher: "Hakusensha".into(),
            language: "ja".into(),
            file_size: 16227,
            oneshot: false,
            epub_is_kepub: false,
            is_pre_paginated: false,
            cover_image_id: Some("t1".into()),
            authors: vec!["Kentaro Miura".into()],
        }
    }

    #[test]
    fn metadata_dto_shape() {
        let dto = KoboBookMetadataDto::from(&sample_row());
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["Categories"], serde_json::json!([DUMMY_ID]));
        assert_eq!(json["CrossRevisionId"], "b1");
        assert_eq!(json["Description"], "Guts");
        assert_eq!(json["EntitlementId"], "b1");
        assert_eq!(json["Genre"], DUMMY_ID);
        assert_eq!(json["Isbn"], "9781593070205");
        assert_eq!(json["Language"], "ja");
        assert_eq!(json["PublicationDate"], "2024-01-02T03:04:05Z");
        assert_eq!(json["Publisher"]["Name"], "Hakusensha");
        assert_eq!(json["Series"]["Id"], "s1");
        assert_eq!(json["Series"]["NumberFloat"], 1.0);
        assert_eq!(json["Title"], "Berserk v01");
        assert_eq!(json["WorkId"], "b1");
        assert_eq!(json["CoverImageId"], "t1");
        assert_eq!(json["Contributors"], serde_json::json!(["Kentaro Miura"]));
        // @JsonIgnore fields never serialize
        assert!(json.get("IsKepub").is_none());
        assert!(json.get("IsPrePaginated").is_none());
        assert!(json.get("FileSize").is_none());
    }

    #[test]
    fn metadata_dto_edge_cases() {
        let mut row = sample_row();
        row.summary = String::new();
        row.isbn = String::new();
        row.language = String::new();
        row.oneshot = true;
        row.cover_image_id = None;
        let json = serde_json::to_value(KoboBookMetadataDto::from(&row)).unwrap();
        // blank summary is forced to a single space so Kobo still updates it
        assert_eq!(json["Description"], " ");
        assert!(json.get("Isbn").is_none());
        assert_eq!(json["Language"], "en");
        assert!(json.get("Series").is_none());
        assert!(json.get("CoverImageId").is_none());
    }

    #[test]
    fn entitlement_and_reading_state_shape() {
        let book = komga_core::model::sync_point::SyncPointBook {
            sync_point_id: "sp1".into(),
            book_id: "b1".into(),
            book_created_date: time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap(),
            book_last_modified_date: time_codec::parse_datetime_utc("2024-01-03 03:04:05").unwrap(),
            book_file_last_modified: time_codec::parse_datetime_utc("2024-01-04 03:04:05").unwrap(),
            book_file_size: 100,
            book_file_hash: "h".into(),
            book_metadata_last_modified_date: time_codec::parse_datetime_utc("2024-01-05 03:04:05")
                .unwrap(),
            book_read_progress_last_modified_date: None,
            book_thumbnail_id: None,
            synced: false,
        };
        let now = time_codec::parse_datetime_utc("2026-09-19 00:00:00").unwrap();
        let json = serde_json::to_value(book_entitlement_of(&book, false, now)).unwrap();
        assert_eq!(json["Accessibility"], "Full");
        assert_eq!(json["Created"], "2024-01-02T03:04:05Z");
        assert_eq!(json["IsRemoved"], false);
        assert_eq!(json["LastModified"], "2024-01-04T03:04:05Z");
        assert_eq!(json["OriginCategory"], "Imported");
        assert_eq!(json["Status"], "Active");

        let json = serde_json::to_value(empty_reading_state("b1", now)).unwrap();
        assert_eq!(json["EntitlementId"], "b1");
        assert_eq!(json["StatusInfo"]["Status"], "ReadyToRead");
        assert_eq!(json["StatusInfo"]["TimesStartedReading"], 0);
    }

    #[test]
    fn reading_state_of_progress() {
        let progress = ReadProgress {
            book_id: "b1".into(),
            user_id: "u1".into(),
            page: 5,
            completed: false,
            read_date: time_codec::parse_datetime_utc("2024-06-01 10:00:00").unwrap(),
            device_id: "d".into(),
            device_name: "dev".into(),
            locator: Some(serde_json::json!({
                "href": "page_5.xhtml",
                "koboSpan": "kobo.1.5",
                "locations": {"progression": 0.5, "totalProgression": 0.25}
            })),
            created_date: time_codec::parse_datetime_utc("2024-05-01 10:00:00").unwrap(),
            last_modified_date: time_codec::parse_datetime_utc("2024-06-01 10:00:00").unwrap(),
        };
        let json = serde_json::to_value(reading_state_of(&progress)).unwrap();
        assert_eq!(json["EntitlementId"], "b1");
        assert_eq!(json["CurrentBookmark"]["ProgressPercent"], 25.0);
        assert_eq!(
            json["CurrentBookmark"]["ContentSourceProgressPercent"],
            50.0
        );
        assert_eq!(
            json["CurrentBookmark"]["Location"]["Source"],
            "page_5.xhtml"
        );
        assert_eq!(json["CurrentBookmark"]["Location"]["Value"], "kobo.1.5");
        assert_eq!(json["StatusInfo"]["Status"], "Reading");
    }

    #[test]
    fn sync_result_wrappers() {
        let tag = wrapped_tag_of(
            &komga_core::model::sync_point::SyncPointReadList {
                sync_point_id: "sp1".into(),
                readlist_id: "rl1".into(),
                readlist_name: "On Deck".into(),
                readlist_created_date: time_codec::parse_datetime_utc("2024-01-02 03:04:05")
                    .unwrap(),
                readlist_last_modified_date: time_codec::parse_datetime_utc("2024-01-03 03:04:05")
                    .unwrap(),
                synced: false,
            },
            Some(vec![TagItemDto {
                revision_id: "b1".into(),
                type_: "ProductRevisionTagItem".into(),
            }]),
        );
        let json = serde_json::to_value(SyncResultDto::NewTag { new_tag: tag }).unwrap();
        assert_eq!(json["NewTag"]["Tag"]["Id"], "rl1");
        assert_eq!(json["NewTag"]["Tag"]["Type"], "UserTag");
        assert_eq!(json["NewTag"]["Tag"]["Items"][0]["RevisionId"], "b1");
        assert_eq!(
            json["NewTag"]["Tag"]["Items"][0]["Type"],
            "ProductRevisionTagItem"
        );

        let json = serde_json::to_value(update_success("b1")).unwrap();
        assert_eq!(json["RequestResult"], "Success");
        assert_eq!(json["UpdateResults"][0]["EntitlementId"], "b1");
        assert_eq!(
            json["UpdateResults"][0]["CurrentBookmarkResult"]["Result"],
            "Success"
        );
        assert_eq!(
            json["UpdateResults"][0]["StatisticsResult"]["Result"],
            "Ignored"
        );
    }

    #[test]
    fn download_url_and_auth_shapes() {
        let json = serde_json::to_value(DownloadUrlDto {
            drm_type: none_drm(),
            format: FormatDto::Epub3,
            size: 100,
            platform: generic_platform(),
            url: "http://h/x".into(),
        })
        .unwrap();
        assert_eq!(json["DrmType"], "None");
        assert_eq!(json["Format"], "EPUB3");
        assert_eq!(json["Platform"], "Generic");

        let json = serde_json::to_value(AuthDto {
            access_token: "a".into(),
            refresh_token: "r".into(),
            token_type: "Bearer".into(),
            tracking_id: "t".into(),
            user_key: "u".into(),
        })
        .unwrap();
        assert_eq!(json["AccessToken"], "a");
        assert_eq!(json["TokenType"], "Bearer");
    }
}
