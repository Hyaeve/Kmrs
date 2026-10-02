//! DTOs for the kmrs-private komf integration endpoints.

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
