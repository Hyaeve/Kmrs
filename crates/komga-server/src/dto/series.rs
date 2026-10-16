//! `SeriesMetadataUpdateDto.kt` (+ shared update DTOs): metadata PATCH body with `isSet`
//! semantics (double Option: outer None = absent, Some(None) = explicit null clear).

use crate::dto::loose::loose_some_i32;
use crate::error::Violation;
use komga_core::model::common::{Author, WebLink};
use komga_core::model::series::{AlternateTitle, ReadingDirection, SeriesMetadata, SeriesStatus};
use serde::Deserialize;
use std::collections::BTreeSet;

/// serde_json cannot distinguish "key absent" from "key: null" for `Option<Option<T>>` on its
/// own; this restores the distinction komga's `isSet` tracking needs.
fn deserialize_some<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebLinkUpdateDto {
    pub label: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlternateTitleUpdateDto {
    pub label: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SeriesMetadataUpdateDto {
    pub status: Option<SeriesStatus>,
    pub status_lock: Option<bool>,
    pub title: Option<String>,
    pub title_lock: Option<bool>,
    pub title_sort: Option<String>,
    pub title_sort_lock: Option<bool>,
    pub summary: Option<String>,
    pub summary_lock: Option<bool>,
    pub publisher: Option<String>,
    pub publisher_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub reading_direction: Option<Option<ReadingDirection>>,
    pub reading_direction_lock: Option<bool>,
    #[serde(deserialize_with = "loose_some_i32")]
    pub age_rating: Option<Option<i32>>,
    pub age_rating_lock: Option<bool>,
    pub language: Option<String>,
    pub language_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub genres: Option<Option<BTreeSet<String>>>,
    pub genres_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub tags: Option<Option<BTreeSet<String>>>,
    pub tags_lock: Option<bool>,
    #[serde(deserialize_with = "loose_some_i32")]
    pub total_book_count: Option<Option<i32>>,
    pub total_book_count_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub sharing_labels: Option<Option<BTreeSet<String>>>,
    pub sharing_labels_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub links: Option<Option<Vec<WebLinkUpdateDto>>>,
    pub links_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub alternate_titles: Option<Option<Vec<AlternateTitleUpdateDto>>>,
    pub alternate_titles_lock: Option<bool>,
}

/// `isSet` fields replace wholesale (Some(x) → x, Some(None) → cleared); absent fields keep the
/// existing value. Plain fields merge with `?:`.
macro_rules! apply_isset {
    ($existing:expr, $field:ident, $incoming:expr) => {
        match $incoming {
            Some(value) => value,
            None => $existing.$field.clone(),
        }
    };
}

impl SeriesMetadataUpdateDto {
    /// Bean-validation equivalents of the Kotlin constraints (`@NullOrNotBlank`, `@Positive(OrZero)`,
    /// `@NullOrBlankOrBCP47`, `@Valid` on nested DTOs); messages match the komga validators.
    pub fn violations(&self) -> Vec<Violation> {
        let mut violations = vec![];
        if let Some(title) = &self.title {
            if title.trim().is_empty() {
                violations.push(Violation {
                    field_name: "title".into(),
                    message: "Must be null or not blank".into(),
                });
            }
        }
        if let Some(title_sort) = &self.title_sort {
            if title_sort.trim().is_empty() {
                violations.push(Violation {
                    field_name: "titleSort".into(),
                    message: "Must be null or not blank".into(),
                });
            }
        }
        if let Some(Some(age)) = self.age_rating {
            if age < 0 {
                violations.push(Violation {
                    field_name: "ageRating".into(),
                    message: "must be greater than or equal to 0".into(),
                });
            }
        }
        if let Some(language) = &self.language {
            if !language.trim().is_empty()
                && !komga_media::metadata::patch::bcp47_is_valid(language)
            {
                violations.push(Violation {
                    field_name: "language".into(),
                    message: "Must be null or blank or valid BCP 47 language tag".into(),
                });
            }
        }
        if let Some(Some(count)) = self.total_book_count {
            if count <= 0 {
                violations.push(Violation {
                    field_name: "totalBookCount".into(),
                    message: "must be greater than 0".into(),
                });
            }
        }
        if let Some(Some(links)) = &self.links {
            violations.extend(web_link_violations(links));
        }
        if let Some(Some(titles)) = &self.alternate_titles {
            for (i, alt) in titles.iter().enumerate() {
                if alt.label.as_deref().is_none_or(|l| l.trim().is_empty()) {
                    violations.push(Violation {
                        field_name: format!("alternateTitles[{i}].label"),
                        message: "must not be blank".into(),
                    });
                }
                if alt.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
                    violations.push(Violation {
                        field_name: format!("alternateTitles[{i}].title"),
                        message: "must not be blank".into(),
                    });
                }
            }
        }
        violations
    }

    /// `SeriesController.updateSeriesMetadata` merge semantics.
    pub fn apply_to(self, existing: &SeriesMetadata) -> SeriesMetadata {
        SeriesMetadata {
            series_id: existing.series_id.clone(),
            status: self.status.unwrap_or(existing.status),
            status_lock: self.status_lock.unwrap_or(existing.status_lock),
            title: self.title.unwrap_or_else(|| existing.title.clone()),
            title_lock: self.title_lock.unwrap_or(existing.title_lock),
            title_sort: self
                .title_sort
                .unwrap_or_else(|| existing.title_sort.clone()),
            title_sort_lock: self.title_sort_lock.unwrap_or(existing.title_sort_lock),
            summary: self.summary.unwrap_or_else(|| existing.summary.clone()),
            summary_lock: self.summary_lock.unwrap_or(existing.summary_lock),
            language: self.language.unwrap_or_else(|| existing.language.clone()),
            language_lock: self.language_lock.unwrap_or(existing.language_lock),
            reading_direction: apply_isset!(existing, reading_direction, self.reading_direction),
            reading_direction_lock: self
                .reading_direction_lock
                .unwrap_or(existing.reading_direction_lock),
            publisher: self.publisher.unwrap_or_else(|| existing.publisher.clone()),
            publisher_lock: self.publisher_lock.unwrap_or(existing.publisher_lock),
            age_rating: apply_isset!(existing, age_rating, self.age_rating),
            age_rating_lock: self.age_rating_lock.unwrap_or(existing.age_rating_lock),
            genres: apply_isset!(existing, genres, self.genres.map(|g| g.unwrap_or_default())),
            genres_lock: self.genres_lock.unwrap_or(existing.genres_lock),
            tags: apply_isset!(existing, tags, self.tags.map(|t| t.unwrap_or_default())),
            tags_lock: self.tags_lock.unwrap_or(existing.tags_lock),
            total_book_count: apply_isset!(existing, total_book_count, self.total_book_count),
            total_book_count_lock: self
                .total_book_count_lock
                .unwrap_or(existing.total_book_count_lock),
            sharing_labels: apply_isset!(
                existing,
                sharing_labels,
                self.sharing_labels.map(|s| s.unwrap_or_default())
            ),
            sharing_labels_lock: self
                .sharing_labels_lock
                .unwrap_or(existing.sharing_labels_lock),
            links: apply_isset!(
                existing,
                links,
                self.links.map(|links| {
                    links
                        .unwrap_or_default()
                        .into_iter()
                        .map(|l| WebLink {
                            label: l.label.expect("validated @NotBlank"),
                            url: l.url.expect("validated @URL"),
                        })
                        .collect()
                })
            ),
            links_lock: self.links_lock.unwrap_or(existing.links_lock),
            alternate_titles: apply_isset!(
                existing,
                alternate_titles,
                self.alternate_titles.map(|titles| {
                    titles
                        .unwrap_or_default()
                        .into_iter()
                        .map(|t| AlternateTitle {
                            label: t.label.expect("validated @NotBlank"),
                            title: t.title.expect("validated @NotBlank"),
                        })
                        .collect()
                })
            ),
            alternate_titles_lock: self
                .alternate_titles_lock
                .unwrap_or(existing.alternate_titles_lock),
            created_date: existing.created_date,
            last_modified_date: existing.last_modified_date,
        }
    }
}

/// `@Valid` cascade for a list of WebLinkUpdateDto (`@NotBlank` label, `@URL` url);
/// property paths follow Spring's `links[i].field`.
pub(crate) fn web_link_violations(links: &[WebLinkUpdateDto]) -> Vec<Violation> {
    let mut violations = vec![];
    for (i, link) in links.iter().enumerate() {
        if link.label.as_deref().is_none_or(|l| l.trim().is_empty()) {
            violations.push(Violation {
                field_name: format!("links[{i}].label"),
                message: "must not be blank".into(),
            });
        }
        if let Some(url) = &link.url {
            if !is_valid_url(url) {
                violations.push(Violation {
                    field_name: format!("links[{i}].url"),
                    message: "must be a valid URL".into(),
                });
            }
        }
    }
    violations
}

/// Hibernate's `@URL` validator accepts anything `java.net.URL` parses: a scheme is required.
pub(crate) fn is_valid_url(value: &str) -> bool {
    let Some(scheme_end) = value.find(':') else {
        return false;
    };
    let scheme = &value[..scheme_end];
    !scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && value.len() > scheme_end + 1
}

pub(crate) fn author_of(dto: &crate::dto::book::AuthorUpdateDto) -> Author {
    Author::new(
        dto.name.as_deref().unwrap_or_default(),
        dto.role.as_deref().unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_metadata() -> SeriesMetadata {
        let now = komga_core::time_codec::now_utc();
        SeriesMetadata {
            series_id: "s1".into(),
            status: SeriesStatus::Ongoing,
            title: "Berserk".into(),
            title_sort: "Berserk".into(),
            summary: "Guts".into(),
            reading_direction: Some(ReadingDirection::RightToLeft),
            publisher: "Hakusensha".into(),
            age_rating: Some(18),
            language: "ja".into(),
            genres: ["action"].into_iter().map(String::from).collect(),
            tags: ["seinen"].into_iter().map(String::from).collect(),
            total_book_count: Some(41),
            sharing_labels: ["nsfw"].into_iter().map(String::from).collect(),
            links: vec![WebLink {
                label: "wiki".into(),
                url: "https://example.org".into(),
            }],
            alternate_titles: vec![AlternateTitle {
                label: "ja".into(),
                title: "ベルセルク".into(),
            }],
            status_lock: false,
            title_lock: false,
            title_sort_lock: false,
            summary_lock: false,
            reading_direction_lock: false,
            publisher_lock: false,
            age_rating_lock: false,
            language_lock: false,
            genres_lock: false,
            tags_lock: false,
            total_book_count_lock: false,
            sharing_labels_lock: false,
            links_lock: false,
            alternate_titles_lock: false,
            created_date: now,
            last_modified_date: now,
        }
    }

    #[test]
    fn absent_keys_keep_existing() {
        let dto: SeriesMetadataUpdateDto = serde_json::from_str("{}").unwrap();
        let base = base_metadata();
        let updated = dto.apply_to(&base);
        assert_eq!(updated, base);
    }

    #[test]
    fn plain_fields_merge_with_elvis() {
        let dto: SeriesMetadataUpdateDto = serde_json::from_str(
            r#"{"title":"New","titleLock":true,"status":"HIATUS","summary":null}"#,
        )
        .unwrap();
        let updated = dto.apply_to(&base_metadata());
        assert_eq!(updated.title, "New");
        assert!(updated.title_lock);
        assert_eq!(updated.status, SeriesStatus::Hiatus);
        // summary is not isSet-tracked: explicit null keeps the old value
        assert_eq!(updated.summary, "Guts");
    }

    #[test]
    fn isset_fields_replace_or_clear() {
        let dto: SeriesMetadataUpdateDto = serde_json::from_str(
            r#"{"readingDirection":null,"ageRating":null,"genres":null,"tags":[],"totalBookCount":null}"#,
        )
        .unwrap();
        let updated = dto.apply_to(&base_metadata());
        assert_eq!(updated.reading_direction, None);
        assert_eq!(updated.age_rating, None);
        assert!(updated.genres.is_empty());
        assert!(updated.tags.is_empty());
        assert_eq!(updated.total_book_count, None);

        let dto: SeriesMetadataUpdateDto = serde_json::from_str(
            r#"{"readingDirection":"VERTICAL","ageRating":12,"genres":["drama"],"sharingLabels":["kids"]}"#,
        )
        .unwrap();
        let updated = dto.apply_to(&base_metadata());
        assert_eq!(updated.reading_direction, Some(ReadingDirection::Vertical));
        assert_eq!(updated.age_rating, Some(12));
        assert_eq!(updated.genres, ["drama".to_string()].into_iter().collect());
        assert_eq!(
            updated.sharing_labels,
            ["kids".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn violations_match_kotlin_validators() {
        let dto: SeriesMetadataUpdateDto = serde_json::from_str(
            r#"{"title":"  ","ageRating":-1,"language":"not a language","totalBookCount":0,
               "links":[{"label":"","url":"nope"}],"alternateTitles":[{"label":"x"}]}"#,
        )
        .unwrap();
        let violations = dto.violations();
        let fields: Vec<&str> = violations.iter().map(|v| v.field_name.as_str()).collect();
        assert!(fields.contains(&"title"));
        assert!(fields.contains(&"ageRating"));
        assert!(fields.contains(&"language"));
        assert!(fields.contains(&"totalBookCount"));
        assert!(fields.contains(&"links[0].label"));
        assert!(fields.contains(&"links[0].url"));
        assert!(fields.contains(&"alternateTitles[0].title"));
        let messages: Vec<&str> = violations.iter().map(|v| v.message.as_str()).collect();
        assert!(messages.contains(&"Must be null or not blank"));
        assert!(messages.contains(&"must be greater than or equal to 0"));
        assert!(messages.contains(&"Must be null or blank or valid BCP 47 language tag"));
        assert!(messages.contains(&"must be greater than 0"));
        assert!(messages.contains(&"must not be blank"));
        assert!(messages.contains(&"must be a valid URL"));
    }

    #[test]
    fn valid_language_and_isbn_pass() {
        let dto: SeriesMetadataUpdateDto = serde_json::from_str(r#"{"language":"en-US"}"#).unwrap();
        assert!(dto.violations().is_empty());
    }

    #[test]
    fn numeric_isset_fields_accept_string_numbers() {
        // the legacy WebUI serializes v-text-field number inputs as JSON strings
        let dto: SeriesMetadataUpdateDto =
            serde_json::from_str(r#"{"ageRating":"18","totalBookCount":"41"}"#).unwrap();
        assert_eq!(dto.age_rating, Some(Some(18)));
        assert_eq!(dto.total_book_count, Some(Some(41)));

        // native numbers keep working, explicit null still clears
        let dto: SeriesMetadataUpdateDto =
            serde_json::from_str(r#"{"ageRating":12,"totalBookCount":null}"#).unwrap();
        assert_eq!(dto.age_rating, Some(Some(12)));
        assert_eq!(dto.total_book_count, Some(None));

        // non-numeric strings are still rejected
        assert!(serde_json::from_str::<SeriesMetadataUpdateDto>(r#"{"ageRating":"abc"}"#).is_err());
        assert!(
            serde_json::from_str::<SeriesMetadataUpdateDto>(r#"{"totalBookCount":1.5}"#).is_err()
        );
    }

    #[test]
    fn url_validation() {
        assert!(is_valid_url("https://example.org/x"));
        assert!(is_valid_url("ftp://host"));
        assert!(!is_valid_url("nope"));
        assert!(!is_valid_url(""));
        // `new URL("http://")` does not throw (empty host), so Hibernate's @URL accepts it
        assert!(is_valid_url("http://"));
    }
}
