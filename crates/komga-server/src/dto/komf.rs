//! DTOs for the kmrs-private komf integration endpoints.

use komga_db::dao::tracker_link::TrackerLink;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KomfIntegrationDto {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// whether an auth key is stored on the integration row; the value itself is
    /// write-only and never leaves the server
    pub auth_key_set: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub komf_reachable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KomfIntegrationUpdateDto {
    pub url: Option<String>,
    pub base_url: Option<String>,
    /// per-integration override for komf-rs's KOMF_AUTH_KEY gate; wins over the
    /// `komf.auth-key` config preset. Absent keeps the stored override, blank clears it.
    pub auth_key: Option<String>,
}

/// komf's `POST /api/komga/metadata/identify` request; field names follow komf's DTO.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KomfIdentifyRequestDto {
    pub library_id: Option<String>,
    pub series_id: String,
    pub provider: String,
    pub provider_series_id: String,
}

/// One entry of komf's `GET /api/komga/metadata/search` response. `provider` stays a
/// plain string: komf's provider set is open-ended (unknown values round-trip as-is).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KomfSeriesSearchResultDto {
    pub url: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    pub title: String,
    pub provider: String,
    pub result_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// komf's job handle, returned by identify and series match.
#[derive(Debug, Serialize, Deserialize)]
pub struct KomfMetadataJobResponseDto {
    pub id: String,
}

/// One entry of komf's `GET /api/jobs`. `status` stays a plain string: komf owns the
/// enum (unknown values round-trip as-is).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KomfJobDto {
    pub series_id: String,
    pub id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

/// komf's `GET /api/jobs` paged response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KomfJobPageDto {
    pub content: Vec<KomfJobDto>,
    pub total_pages: i32,
    pub current_page: i32,
}

/// One per-user series → platform entry binding, stored in kmrs.sqlite.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerLinkDto {
    pub series_id: String,
    pub provider: String,
    pub track_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub track_mode: String,
    pub chapter_offset: i32,
    pub created_date: String,
    pub last_modified_date: String,
}

impl From<TrackerLink> for TrackerLinkDto {
    fn from(link: TrackerLink) -> Self {
        TrackerLinkDto {
            series_id: link.series_id,
            provider: link.provider,
            track_id: link.track_id,
            title: link.title,
            track_mode: link.track_mode.as_str().to_string(),
            chapter_offset: link.chapter_offset,
            created_date: komga_core::time_codec::format_datetime(link.created_date),
            last_modified_date: komga_core::time_codec::format_datetime(link.last_modified_date),
        }
    }
}

/// Request body for binding a series to one platform entry. `trackMode` is
/// `auto` (resolve chapter/volume from book names), `chapter` or `volume`;
/// `chapterOffset` shifts the pushed chapter number.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerLinkUpsertDto {
    pub provider: String,
    pub track_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub track_mode: Option<String>,
    #[serde(default)]
    pub chapter_offset: Option<i32>,
}

/// Per-user display preferences for the tracker module: the series-detail
/// tracker module only renders in these libraries; empty means every library.
/// `defaultTracker` is preselected in the bind dialog.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerPreferencesDto {
    #[serde(default)]
    pub libraries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_tracker: Option<String>,
}
