//! DTOs for `SettingsController` (`SettingsDto.kt`, `SettingsUpdateDto.kt`, `SettingMultiSource.kt`,
//! `ThumbnailSizeDto.kt`).

use serde::{Deserialize, Serialize};

use crate::dto::loose::{loose_bool_opt, loose_i64_opt};
use crate::settings::ThumbnailSize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_empty_collections: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_empty_read_lists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remember_me_duration_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_size: Option<ThumbnailSizeDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_pool_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_port: Option<SettingMultiSource<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_context_path: Option<SettingMultiSource<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kobo_proxy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kepubify_path: Option<SettingMultiSource<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_upload_file_size_bytes: Option<i64>,
}
impl SettingsDto {
    /// `SettingsDto.public()`: non-admin users only see the upload size limit
    pub fn public(self) -> Self {
        Self {
            delete_empty_collections: None,
            delete_empty_read_lists: None,
            remember_me_duration_days: None,
            thumbnail_size: None,
            task_pool_size: None,
            server_port: None,
            server_context_path: None,
            kobo_proxy: None,
            kepubify_path: None,
            max_upload_file_size_bytes: self.max_upload_file_size_bytes,
        }
    }
}

/// Java response shape: port / context-path / kepubify-path are configuration-only in kmrs,
/// so `database_source` always serializes null here.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingMultiSource<T: Serialize> {
    // Jackson has no NON_NULL here: all three keys are always present, even when null
    pub configuration_source: Option<T>,
    pub database_source: Option<T>,
    pub effective_value: Option<T>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThumbnailSizeDto {
    #[serde(rename = "DEFAULT")]
    Default,
    #[serde(rename = "MEDIUM")]
    Medium,
    #[serde(rename = "LARGE")]
    Large,
    #[serde(rename = "XLARGE")]
    XLarge,
}

impl From<ThumbnailSize> for ThumbnailSizeDto {
    fn from(s: ThumbnailSize) -> Self {
        match s {
            ThumbnailSize::Default => ThumbnailSizeDto::Default,
            ThumbnailSize::Medium => ThumbnailSizeDto::Medium,
            ThumbnailSize::Large => ThumbnailSizeDto::Large,
            ThumbnailSize::XLarge => ThumbnailSizeDto::XLarge,
        }
    }
}

impl From<ThumbnailSizeDto> for ThumbnailSize {
    fn from(d: ThumbnailSizeDto) -> Self {
        match d {
            ThumbnailSizeDto::Default => ThumbnailSize::Default,
            ThumbnailSizeDto::Medium => ThumbnailSize::Medium,
            ThumbnailSizeDto::Large => ThumbnailSize::Large,
            ThumbnailSizeDto::XLarge => ThumbnailSize::XLarge,
        }
    }
}

impl ThumbnailSizeDto {
    pub fn as_str(self) -> &'static str {
        match self {
            ThumbnailSizeDto::Default => "DEFAULT",
            ThumbnailSizeDto::Medium => "MEDIUM",
            ThumbnailSizeDto::Large => "LARGE",
            ThumbnailSizeDto::XLarge => "XLARGE",
        }
    }
}

/// `SettingsUpdateDto`: every field is optional.
/// Java's `serverPort` / `serverContextPath` / `kepubifyPath` are configuration-only in
/// kmrs, and `koboPort` is gone entirely; all four are silently ignored when sent.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsUpdateDto {
    #[serde(deserialize_with = "loose_bool_opt")]
    pub delete_empty_collections: Option<bool>,
    #[serde(deserialize_with = "loose_bool_opt")]
    pub delete_empty_read_lists: Option<bool>,
    #[serde(deserialize_with = "loose_i64_opt")]
    pub remember_me_duration_days: Option<i64>,
    #[serde(deserialize_with = "loose_bool_opt")]
    pub renew_remember_me_key: Option<bool>,
    pub thumbnail_size: Option<ThumbnailSizeDto>,
    #[serde(deserialize_with = "loose_i64_opt")]
    pub task_pool_size: Option<i64>,
    #[serde(deserialize_with = "loose_bool_opt")]
    pub kobo_proxy: Option<bool>,
}
