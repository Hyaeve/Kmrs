//! Smart list DTOs (kmrs-only extension, no Java equivalent).

use crate::error::Violation;
use komga_core::dto::dto_datetime;
use komga_core::model::smart_list::{SmartList, SmartListTarget, SmartListVisibility};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartListCreationDto {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    pub target: SmartListTarget,
    /// admin-only on the API; `None` means PRIVATE
    pub visibility: Option<SmartListVisibility>,
    /// admin-only on the API
    pub shared_with_user_ids: Option<Vec<String>>,
    /// `BookSearch` (target BOOK) or `SeriesSearch` (target SERIES)
    pub search: serde_json::Value,
}

impl SmartListCreationDto {
    pub fn violations(&self) -> Vec<Violation> {
        let mut violations = vec![];
        if self.name.trim().is_empty() {
            violations.push(Violation {
                field_name: "name".into(),
                message: "must not be blank".into(),
            });
        }
        if self.visibility == Some(SmartListVisibility::Shared)
            && self
                .shared_with_user_ids
                .as_ref()
                .is_none_or(|ids| ids.is_empty())
        {
            violations.push(Violation {
                field_name: "sharedWithUserIds".into(),
                message: "must not be empty when visibility is SHARED".into(),
            });
        }
        violations
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SmartListUpdateDto {
    pub name: Option<String>,
    pub summary: Option<String>,
    pub target: Option<SmartListTarget>,
    pub visibility: Option<SmartListVisibility>,
    pub shared_with_user_ids: Option<Vec<String>>,
    pub search: Option<serde_json::Value>,
}

impl SmartListUpdateDto {
    pub fn violations(&self) -> Vec<Violation> {
        let mut violations = vec![];
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                violations.push(Violation {
                    field_name: "name".into(),
                    message: "Must be null or not blank".into(),
                });
            }
        }
        if self.visibility == Some(SmartListVisibility::Shared)
            && self
                .shared_with_user_ids
                .as_ref()
                .is_some_and(|ids| ids.is_empty())
        {
            violations.push(Violation {
                field_name: "sharedWithUserIds".into(),
                message: "Must be null or not empty when visibility is SHARED".into(),
            });
        }
        violations
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartListDto {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub owner_id: String,
    pub target: SmartListTarget,
    pub visibility: SmartListVisibility,
    pub shared_with_user_ids: Vec<String>,
    /// The stored `BookSearch`/`SeriesSearch` document
    pub search: serde_json::Value,
    #[serde(with = "dto_datetime")]
    pub created_date: OffsetDateTime,
    #[serde(with = "dto_datetime")]
    pub last_modified_date: OffsetDateTime,
}

impl SmartListDto {
    /// Fails only when the stored JSON is corrupt, which is a server-side invariant.
    pub fn of(s: &SmartList, shared_with_user_ids: Vec<String>) -> Result<Self, serde_json::Error> {
        Ok(Self {
            id: s.id.clone(),
            name: s.name.clone(),
            summary: s.summary.clone(),
            owner_id: s.owner_user_id.clone(),
            target: s.target,
            visibility: s.visibility,
            shared_with_user_ids,
            search: serde_json::from_str(&s.search_json)?,
            created_date: s.created_date,
            last_modified_date: s.last_modified_date,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_violations() {
        let mut dto = SmartListCreationDto {
            name: "  ".into(),
            summary: String::new(),
            target: SmartListTarget::Book,
            visibility: None,
            shared_with_user_ids: None,
            search: serde_json::json!({}),
        };
        assert_eq!(dto.violations().len(), 1);
        dto.name = "ok".into();
        assert!(dto.violations().is_empty());
        dto.visibility = Some(SmartListVisibility::Shared);
        assert_eq!(dto.violations().len(), 1);
        // an absent scope counts as empty too
        dto.shared_with_user_ids = Some(vec![]);
        assert_eq!(dto.violations().len(), 1);
        dto.shared_with_user_ids = Some(vec!["u2".into()]);
        assert!(dto.violations().is_empty());
    }

    #[test]
    fn dto_carries_the_visibility_scope() {
        let created = komga_core::time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap();
        let list = SmartList {
            id: "sl1".into(),
            name: "Unread".into(),
            summary: String::new(),
            owner_user_id: "u1".into(),
            target: SmartListTarget::Book,
            visibility: SmartListVisibility::Shared,
            search_json: r#"{"condition": {"tag": {"operator": "is", "value": "manga"}}}"#.into(),
            created_date: created,
            last_modified_date: created,
        };
        let dto = SmartListDto::of(&list, vec!["u2".into()]).unwrap();
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["ownerId"], "u1");
        assert_eq!(json["target"], "BOOK");
        assert_eq!(json["visibility"], "SHARED");
        assert_eq!(json["sharedWithUserIds"], serde_json::json!(["u2"]));
        assert_eq!(
            json["search"],
            serde_json::json!({"condition": {"tag": {"operator": "is", "value": "manga"}}})
        );
    }
}
