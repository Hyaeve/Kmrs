//! DTOs for `ClientSettingsController` (`ClientSettingDto.kt`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSettingDto {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_unauthorized: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSettingGlobalUpdateDto {
    pub value: String,
    pub allow_unauthorized: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientSettingUserUpdateDto {
    pub value: String,
}
