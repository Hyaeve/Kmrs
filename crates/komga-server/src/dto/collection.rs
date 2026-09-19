//! `CollectionCreationDto.kt` / `CollectionUpdateDto.kt` and their bean-validation messages.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionCreationDto {
    pub name: String,
    #[serde(default)]
    pub ordered: bool,
    pub series_ids: Vec<String>,
}

impl CollectionCreationDto {
    /// `@NotBlank name` / `@NotEmpty @UniqueElements seriesIds`
    pub fn violations(&self) -> Vec<crate::error::Violation> {
        let mut violations = vec![];
        if self.name.trim().is_empty() {
            violations.push(crate::error::Violation {
                field_name: "name".into(),
                message: "must not be blank".into(),
            });
        }
        violations.extend(unique_violations(&self.series_ids, "seriesIds"));
        violations
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CollectionUpdateDto {
    pub name: Option<String>,
    pub ordered: Option<bool>,
    pub series_ids: Option<Vec<String>>,
}

impl CollectionUpdateDto {
    /// `@NullOrNotBlank name` / `@NullOrNotEmpty @UniqueElements seriesIds`
    pub fn violations(&self) -> Vec<crate::error::Violation> {
        let mut violations = vec![];
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                violations.push(crate::error::Violation {
                    field_name: "name".into(),
                    message: "Must be null or not blank".into(),
                });
            }
        }
        if let Some(series_ids) = &self.series_ids {
            if series_ids.is_empty() {
                violations.push(crate::error::Violation {
                    field_name: "seriesIds".into(),
                    message: "Must be null or not empty".into(),
                });
            }
            violations.extend(unique_violations(series_ids, "seriesIds"));
        }
        violations
    }
}

/// `@UniqueElements` on a list of ids
pub(crate) fn unique_violations(values: &[String], field: &str) -> Vec<crate::error::Violation> {
    if values.is_empty() {
        return vec![crate::error::Violation {
            field_name: field.into(),
            message: "must not be empty".into(),
        }];
    }
    let mut seen = std::collections::BTreeSet::new();
    if values.iter().any(|v| !seen.insert(v)) {
        return vec![crate::error::Violation {
            field_name: field.into(),
            message: "must not contain duplicate elements".into(),
        }];
    }
    vec![]
}
