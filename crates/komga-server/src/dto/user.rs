//! DTOs for User/ApiKey/AuthenticationActivity, aligned with komga `interfaces/api/rest/dto/`.

use crate::dto::loose::loose_i32;
use komga_core::model::user::{AllowExclude, ApiKey, AuthenticationActivity, KomgaUser};
use komga_core::time_codec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use time::OffsetDateTime;

fn fmt(dt: OffsetDateTime) -> String {
    time_codec::format_dto_datetime(dt)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDto {
    pub id: String,
    pub email: String,
    pub roles: BTreeSet<String>,
    pub shared_all_libraries: bool,
    pub shared_libraries_ids: BTreeSet<String>,
    pub labels_allow: BTreeSet<String>,
    pub labels_exclude: BTreeSet<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_restriction: Option<AgeRestrictionDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AgeRestrictionDto {
    pub age: i32,
    pub restriction: AllowExclude,
}

impl From<&KomgaUser> for UserDto {
    fn from(user: &KomgaUser) -> Self {
        let mut roles: BTreeSet<String> =
            user.roles.iter().map(|r| r.as_str().to_string()).collect();
        roles.insert("USER".to_string());
        UserDto {
            id: user.id.clone(),
            email: user.email.clone(),
            roles,
            shared_all_libraries: user.shared_all_libraries,
            shared_libraries_ids: user.shared_libraries_ids.clone(),
            labels_allow: user.restrictions.labels_allow.clone(),
            labels_exclude: user.restrictions.labels_exclude.clone(),
            age_restriction: user
                .restrictions
                .age_restriction
                .map(|ar| AgeRestrictionDto {
                    age: ar.age,
                    restriction: ar.restriction,
                }),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCreationDto {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub roles: Vec<String>,
    pub age_restriction: Option<AgeRestrictionUpdateDto>,
    pub labels_allow: Option<BTreeSet<String>>,
    pub labels_exclude: Option<BTreeSet<String>>,
    pub shared_libraries: Option<SharedLibrariesUpdateDto>,
}

/// Double Option: outer None = not provided (keep current value), Some(None) = explicit null (clear),
/// matching the isSet semantics of komga `UserUpdateDto`.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdateDto {
    #[serde(default)]
    pub age_restriction: Option<Option<AgeRestrictionUpdateDto>>,
    #[serde(default)]
    pub labels_allow: Option<Option<BTreeSet<String>>>,
    #[serde(default)]
    pub labels_exclude: Option<Option<BTreeSet<String>>>,
    #[serde(default)]
    pub roles: Option<Option<BTreeSet<String>>>,
    #[serde(default)]
    pub shared_libraries: Option<Option<SharedLibrariesUpdateDto>>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct AgeRestrictionUpdateDto {
    #[serde(deserialize_with = "loose_i32")]
    pub age: i32,
    pub restriction: AllowExcludeDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedLibrariesUpdateDto {
    pub all: bool,
    #[serde(default)]
    pub library_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum AllowExcludeDto {
    #[serde(rename = "ALLOW_ONLY")]
    AllowOnly,
    #[serde(rename = "EXCLUDE")]
    Exclude,
    #[serde(rename = "NONE")]
    None,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyDto {
    pub id: String,
    pub user_id: String,
    pub key: String,
    pub comment: String,
    pub created_date: String,
    pub last_modified_date: String,
}

impl ApiKeyDto {
    pub fn of(key: &ApiKey) -> Self {
        Self {
            id: key.id.clone(),
            user_id: key.user_id.clone(),
            key: key.key.clone(),
            comment: key.comment.clone(),
            created_date: fmt(key.created_date),
            last_modified_date: fmt(key.last_modified_date),
        }
    }

    pub fn of_redacted(key: &ApiKey) -> Self {
        Self {
            key: "*".repeat(6),
            ..Self::of(key)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyRequestDto {
    pub comment: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationActivityDto {
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub api_key_id: Option<String>,
    pub api_key_comment: Option<String>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub date_time: String,
    pub source: Option<String>,
}

impl From<&AuthenticationActivity> for AuthenticationActivityDto {
    fn from(a: &AuthenticationActivity) -> Self {
        Self {
            user_id: a.user_id.clone(),
            email: a.email.clone(),
            api_key_id: a.api_key_id.clone(),
            api_key_comment: a.api_key_comment.clone(),
            ip: a.ip.clone(),
            user_agent: a.user_agent.clone(),
            success: a.success,
            error: a.error.clone(),
            date_time: fmt(a.date_time),
            source: a.source.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PasswordUpdateDto {
    pub password: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_restriction_accepts_string_numbers() {
        // the legacy WebUI serializes v-text-field number inputs as JSON strings
        let dto: AgeRestrictionUpdateDto =
            serde_json::from_str(r#"{"age":"15","restriction":"ALLOW_ONLY"}"#).unwrap();
        assert_eq!(dto.age, 15);

        let dto: AgeRestrictionUpdateDto =
            serde_json::from_str(r#"{"age":18,"restriction":"EXCLUDE"}"#).unwrap();
        assert_eq!(dto.age, 18);

        assert!(serde_json::from_str::<AgeRestrictionUpdateDto>(
            r#"{"age":"abc","restriction":"ALLOW_ONLY"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<AgeRestrictionUpdateDto>(
            r#"{"age":"3000000000","restriction":"ALLOW_ONLY"}"#
        )
        .is_err());
        // age is required
        assert!(
            serde_json::from_str::<AgeRestrictionUpdateDto>(r#"{"restriction":"ALLOW_ONLY"}"#)
                .is_err()
        );
    }
}
