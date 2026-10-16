//! Equivalent models for `KomgaUser.kt`, `ApiKey.kt`, `AuthenticationActivity.kt`, `ContentRestrictions.kt`,
//! `AgeRestriction.kt`, and `UserRoles.kt`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum UserRole {
    #[serde(rename = "ADMIN")]
    Admin,
    #[serde(rename = "FILE_DOWNLOAD")]
    FileDownload,
    #[serde(rename = "PAGE_STREAMING")]
    PageStreaming,
    #[serde(rename = "KOBO_SYNC")]
    KoboSync,
    #[serde(rename = "KOREADER_SYNC")]
    KoreaderSync,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            UserRole::Admin => "ADMIN",
            UserRole::FileDownload => "FILE_DOWNLOAD",
            UserRole::PageStreaming => "PAGE_STREAMING",
            UserRole::KoboSync => "KOBO_SYNC",
            UserRole::KoreaderSync => "KOREADER_SYNC",
        }
    }
}

impl std::str::FromStr for UserRole {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "ADMIN" => UserRole::Admin,
            "FILE_DOWNLOAD" => UserRole::FileDownload,
            "PAGE_STREAMING" => UserRole::PageStreaming,
            "KOBO_SYNC" => UserRole::KoboSync,
            "KOREADER_SYNC" => UserRole::KoreaderSync,
            _ => return Err(()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AllowExclude {
    #[serde(rename = "ALLOW_ONLY")]
    AllowOnly,
    #[serde(rename = "EXCLUDE")]
    Exclude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgeRestriction {
    pub age: i32,
    pub restriction: AllowExclude,
}

/// `ContentRestrictions.kt`: labels are normalized at construction (lowercase + trim + drop blanks, allow minus exclude).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentRestrictions {
    pub age_restriction: Option<AgeRestriction>,
    pub labels_allow: BTreeSet<String>,
    pub labels_exclude: BTreeSet<String>,
}

impl ContentRestrictions {
    pub fn new(
        age_restriction: Option<AgeRestriction>,
        labels_allow: BTreeSet<String>,
        labels_exclude: BTreeSet<String>,
    ) -> Self {
        let exclude = lower_not_blank(labels_exclude);
        let allow = lower_not_blank(labels_allow)
            .into_iter()
            .filter(|l| !exclude.contains(l))
            .collect();
        Self {
            age_restriction,
            labels_allow: allow,
            labels_exclude: exclude,
        }
    }

    pub fn is_restricted(&self) -> bool {
        self.age_restriction.is_some()
            || !self.labels_allow.is_empty()
            || !self.labels_exclude.is_empty()
    }
}

/// `lowerNotBlank`: `map { lowercase().trim() }.filter { isNotBlank() }`.
pub fn lower_not_blank(labels: BTreeSet<String>) -> BTreeSet<String> {
    labels
        .into_iter()
        .map(|l| l.to_lowercase().trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct KomgaUser {
    pub id: String,
    pub email: String,
    pub password: String,
    pub roles: BTreeSet<UserRole>,
    pub shared_libraries_ids: BTreeSet<String>,
    pub shared_all_libraries: bool,
    pub restrictions: ContentRestrictions,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

impl KomgaUser {
    pub fn is_admin(&self) -> bool {
        self.roles.contains(&UserRole::Admin)
    }

    pub fn can_access_all_libraries(&self) -> bool {
        self.shared_all_libraries || self.is_admin()
    }

    pub fn can_access_library(&self, library_id: &str) -> bool {
        self.can_access_all_libraries() || self.shared_libraries_ids.contains(library_id)
    }

    /// Restricted users get their accessible library set (or its intersection with the input set); unrestricted users get None, meaning no filtering.
    /// Corresponds to `getAuthorizedLibraryIds`.
    pub fn get_authorized_library_ids(
        &self,
        library_ids: Option<&BTreeSet<String>>,
    ) -> Option<BTreeSet<String>> {
        match (self.can_access_all_libraries(), library_ids) {
            (false, Some(ids)) => Some(
                ids.intersection(&self.shared_libraries_ids)
                    .cloned()
                    .collect(),
            ),
            (false, None) => Some(self.shared_libraries_ids.clone()),
            (true, Some(ids)) => Some(ids.clone()),
            (true, None) => None,
        }
    }

    /// `isContentAllowed`: restrictions are based on the series' ageRating/sharingLabels.
    pub fn is_content_allowed(&self, age_rating: Option<i32>, sharing_labels: &[String]) -> bool {
        let labels: BTreeSet<String> = lower_not_blank(sharing_labels.iter().cloned().collect());

        let age_allowed = match &self.restrictions.age_restriction {
            Some(ar) if ar.restriction == AllowExclude::AllowOnly => {
                Some(age_rating.is_some_and(|a| a <= ar.age))
            }
            _ => None,
        };

        let label_allowed = if !self.restrictions.labels_allow.is_empty() {
            Some(!self.restrictions.labels_allow.is_disjoint(&labels))
        } else {
            None
        };

        let allowed = match (age_allowed, label_allowed) {
            (None, la) => la != Some(false),
            (aa, None) => aa != Some(false),
            (aa, la) => aa != Some(false) || la != Some(false),
        };
        if !allowed {
            return false;
        }

        let age_denied = match &self.restrictions.age_restriction {
            Some(ar) if ar.restriction == AllowExclude::Exclude => {
                age_rating.is_some_and(|a| a >= ar.age)
            }
            _ => false,
        };

        let label_denied = !self.restrictions.labels_exclude.is_empty()
            && !self.restrictions.labels_exclude.is_disjoint(&labels);

        !age_denied && !label_denied
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiKey {
    pub id: String,
    pub user_id: String,
    pub key: String,
    pub comment: String,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthenticationActivity {
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub api_key_id: Option<String>,
    pub api_key_comment: Option<String>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub date_time: OffsetDateTime,
    pub source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(restrictions: ContentRestrictions) -> KomgaUser {
        KomgaUser {
            id: "u1".into(),
            email: "a@b.c".into(),
            password: "x".into(),
            roles: BTreeSet::new(),
            shared_libraries_ids: BTreeSet::new(),
            shared_all_libraries: true,
            restrictions,
            created_date: crate::time_codec::now_utc(),
            last_modified_date: crate::time_codec::now_utc(),
        }
    }

    #[test]
    fn restrictions_normalize_labels() {
        let r = ContentRestrictions::new(
            None,
            [" Kids ", "ADULT"].into_iter().map(String::from).collect(),
            ["adult", "  "].into_iter().map(String::from).collect(),
        );
        assert_eq!(r.labels_allow, ["kids".to_string()].into_iter().collect());
        assert_eq!(
            r.labels_exclude,
            ["adult".to_string()].into_iter().collect()
        );
        assert!(r.is_restricted());
    }

    #[test]
    fn content_allowed_age_allow_only() {
        let u = user(ContentRestrictions::new(
            Some(AgeRestriction {
                age: 15,
                restriction: AllowExclude::AllowOnly,
            }),
            BTreeSet::new(),
            BTreeSet::new(),
        ));
        assert!(u.is_content_allowed(Some(12), &[]));
        assert!(!u.is_content_allowed(Some(18), &[]));
        assert!(!u.is_content_allowed(None, &[]));
    }

    #[test]
    fn content_allowed_age_exclude() {
        let u = user(ContentRestrictions::new(
            Some(AgeRestriction {
                age: 15,
                restriction: AllowExclude::Exclude,
            }),
            BTreeSet::new(),
            BTreeSet::new(),
        ));
        assert!(u.is_content_allowed(Some(12), &[]));
        assert!(!u.is_content_allowed(Some(15), &[]));
        assert!(u.is_content_allowed(None, &[]));
    }

    #[test]
    fn content_allowed_labels() {
        let u = user(ContentRestrictions::new(
            None,
            ["kids"].into_iter().map(String::from).collect(),
            BTreeSet::new(),
        ));
        assert!(u.is_content_allowed(None, &["Kids".to_string()]));
        assert!(!u.is_content_allowed(None, &["horror".to_string()]));
        assert!(!u.is_content_allowed(None, &[]));

        let u = user(ContentRestrictions::new(
            None,
            BTreeSet::new(),
            ["horror"].into_iter().map(String::from).collect(),
        ));
        assert!(!u.is_content_allowed(None, &["Horror".to_string()]));
        assert!(u.is_content_allowed(None, &["kids".to_string()]));
        assert!(u.is_content_allowed(None, &[]));
    }

    #[test]
    fn content_allowed_age_or_label_when_both_allow() {
        // When both an ALLOW_ONLY age restriction and allow labels are present, satisfying either is enough
        let u = user(ContentRestrictions::new(
            Some(AgeRestriction {
                age: 15,
                restriction: AllowExclude::AllowOnly,
            }),
            ["kids"].into_iter().map(String::from).collect(),
            BTreeSet::new(),
        ));
        assert!(u.is_content_allowed(Some(18), &["kids".to_string()]));
        assert!(u.is_content_allowed(Some(12), &[]));
        assert!(!u.is_content_allowed(Some(18), &["other".to_string()]));
    }

    #[test]
    fn authorized_library_ids() {
        let mut u = user(ContentRestrictions::default());
        assert_eq!(u.get_authorized_library_ids(None), None);

        u.shared_all_libraries = false;
        u.shared_libraries_ids = ["l1".to_string()].into_iter().collect();
        assert_eq!(
            u.get_authorized_library_ids(None),
            Some(["l1".to_string()].into_iter().collect())
        );
        let input: BTreeSet<String> = ["l1".to_string(), "l2".to_string()].into_iter().collect();
        assert_eq!(
            u.get_authorized_library_ids(Some(&input)),
            Some(["l1".to_string()].into_iter().collect())
        );

        u.roles.insert(UserRole::Admin);
        assert_eq!(u.get_authorized_library_ids(None), None);
    }
}
