//! `BookMetadataUpdateDto.kt` (+ `AuthorUpdateDto.kt`): book metadata PATCH body, with the
//! `patch` merge semantics of `BookMetadataUpdateDto.patch`.

use crate::dto::loose::loose_f32_opt;
use crate::dto::series::{author_of, web_link_violations, WebLinkUpdateDto};
use crate::error::Violation;
use komga_core::model::book::BookMetadata;
use komga_core::model::common::WebLink;
use serde::Deserialize;
use time::Date;

fn deserialize_some<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

/// Double-Option wrapper over komga-core's `dto_date` (`yyyy-MM-dd`).
fn deserialize_some_date<'de, D>(deserializer: D) -> Result<Option<Option<Date>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(komga_core::dto::dto_date::deserialize(deserializer)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorUpdateDto {
    pub name: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BookMetadataUpdateDto {
    pub title: Option<String>,
    pub title_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub summary: Option<Option<String>>,
    pub summary_lock: Option<bool>,
    pub number: Option<String>,
    pub number_lock: Option<bool>,
    #[serde(deserialize_with = "loose_f32_opt")]
    pub number_sort: Option<f32>,
    pub number_sort_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some_date")]
    pub release_date: Option<Option<Date>>,
    pub release_date_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub authors: Option<Option<Vec<AuthorUpdateDto>>>,
    pub authors_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub tags: Option<Option<Vec<String>>>,
    pub tags_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub isbn: Option<Option<String>>,
    pub isbn_lock: Option<bool>,
    #[serde(deserialize_with = "deserialize_some")]
    pub links: Option<Option<Vec<WebLinkUpdateDto>>>,
    pub links_lock: Option<bool>,
}

macro_rules! apply_isset {
    ($existing:expr, $field:ident, $incoming:expr) => {
        match $incoming {
            Some(value) => value,
            None => $existing.$field.clone(),
        }
    };
}

impl BookMetadataUpdateDto {
    /// `@NullOrNotBlank` (title, number), `@NullOrBlankOrISBN`, `@Valid` on authors/links.
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
        if let Some(number) = &self.number {
            if number.trim().is_empty() {
                violations.push(Violation {
                    field_name: "number".into(),
                    message: "Must be null or not blank".into(),
                });
            }
        }
        if let Some(Some(isbn)) = &self.isbn {
            if !isbn.trim().is_empty() && !is_valid_isbn13(isbn) {
                violations.push(Violation {
                    field_name: "isbn".into(),
                    message: "Must be null or blank or valid ISBN-13".into(),
                });
            }
        }
        if let Some(Some(authors)) = &self.authors {
            for (i, author) in authors.iter().enumerate() {
                if author.name.as_deref().is_none_or(|n| n.trim().is_empty()) {
                    violations.push(Violation {
                        field_name: format!("authors[{i}].name"),
                        message: "must not be blank".into(),
                    });
                }
                if author.role.as_deref().is_none_or(|r| r.trim().is_empty()) {
                    violations.push(Violation {
                        field_name: format!("authors[{i}].role"),
                        message: "must not be blank".into(),
                    });
                }
            }
        }
        if let Some(Some(links)) = &self.links {
            violations.extend(web_link_violations(links));
        }
        violations
    }

    /// `BookMetadataUpdateDto.patch` merge semantics.
    pub fn apply_to(self, existing: &BookMetadata) -> BookMetadata {
        BookMetadata {
            book_id: existing.book_id.clone(),
            title: self.title.unwrap_or_else(|| existing.title.clone()),
            title_lock: self.title_lock.unwrap_or(existing.title_lock),
            summary: match self.summary {
                Some(summary) => summary.unwrap_or_default(),
                None => existing.summary.clone(),
            },
            summary_lock: self.summary_lock.unwrap_or(existing.summary_lock),
            number: self.number.unwrap_or_else(|| existing.number.clone()),
            number_lock: self.number_lock.unwrap_or(existing.number_lock),
            number_sort: self.number_sort.unwrap_or(existing.number_sort),
            number_sort_lock: self.number_sort_lock.unwrap_or(existing.number_sort_lock),
            release_date: apply_isset!(existing, release_date, self.release_date),
            release_date_lock: self.release_date_lock.unwrap_or(existing.release_date_lock),
            authors: match self.authors {
                Some(authors) => authors.unwrap_or_default().iter().map(author_of).collect(),
                None => existing.authors.clone(),
            },
            authors_lock: self.authors_lock.unwrap_or(existing.authors_lock),
            tags: match self.tags {
                // Kotlin Set semantics: dedup by first occurrence
                Some(tags) => {
                    let mut seen = std::collections::HashSet::new();
                    tags.unwrap_or_default()
                        .into_iter()
                        .filter(|t| seen.insert(t.clone()))
                        .collect()
                }
                None => existing.tags.clone(),
            },
            tags_lock: self.tags_lock.unwrap_or(existing.tags_lock),
            isbn: match self.isbn {
                Some(isbn) => isbn
                    .map(|i| i.chars().filter(char::is_ascii_digit).collect())
                    .unwrap_or_default(),
                None => existing.isbn.clone(),
            },
            isbn_lock: self.isbn_lock.unwrap_or(existing.isbn_lock),
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
            created_date: existing.created_date,
            last_modified_date: existing.last_modified_date,
        }
    }
}

/// Hibernate `@ISBN` (type ISBN-13): hyphens/spaces are stripped, 978/979 prefix and check digit.
fn is_valid_isbn13(value: &str) -> bool {
    let digits: Vec<u32> = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c.to_digit(10).expect("digit"))
        .collect();
    if digits.len() != 13 {
        return false;
    }
    let stripped: String = digits.iter().map(u32::to_string).collect();
    if !stripped.starts_with("978") && !stripped.starts_with("979") {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .take(12)
        .enumerate()
        .map(|(i, d)| if i % 2 == 0 { *d } else { d * 3 })
        .sum();
    (10 - (sum % 10)) % 10 == digits[12]
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::common::Author;

    fn base_metadata() -> BookMetadata {
        BookMetadata {
            book_id: "b1".into(),
            title: "Berserk v01".into(),
            summary: "Guts".into(),
            number: "1".into(),
            number_sort: 1.0,
            release_date: Some(Date::from_calendar_date(1990, time::Month::January, 1).unwrap()),
            authors: vec![Author::new("Kentaro Miura", "writer")],
            tags: vec!["seinen".into()],
            isbn: "9781234567897".into(),
            links: vec![WebLink {
                label: "wiki".into(),
                url: "https://example.org".into(),
            }],
            title_lock: false,
            summary_lock: false,
            number_lock: false,
            number_sort_lock: false,
            release_date_lock: false,
            authors_lock: false,
            tags_lock: false,
            isbn_lock: false,
            links_lock: false,
            created_date: komga_core::time_codec::now_utc(),
            last_modified_date: komga_core::time_codec::now_utc(),
        }
    }

    #[test]
    fn absent_keys_keep_existing() {
        let dto: BookMetadataUpdateDto = serde_json::from_str("{}").unwrap();
        let base = base_metadata();
        assert_eq!(dto.apply_to(&base), base);
    }

    #[test]
    fn isset_summary_release_date() {
        let dto: BookMetadataUpdateDto =
            serde_json::from_str(r#"{"summary":null,"releaseDate":null}"#).unwrap();
        let updated = dto.apply_to(&base_metadata());
        assert_eq!(updated.summary, "");
        assert_eq!(updated.release_date, None);

        let dto: BookMetadataUpdateDto =
            serde_json::from_str(r#"{"summary":"new","releaseDate":"2020-05-01"}"#).unwrap();
        let updated = dto.apply_to(&base_metadata());
        assert_eq!(updated.summary, "new");
        assert_eq!(
            updated.release_date,
            Date::from_calendar_date(2020, time::Month::May, 1).ok()
        );
    }

    #[test]
    fn isset_authors_tags_isbn_links() {
        let dto: BookMetadataUpdateDto = serde_json::from_str(
            r#"{"authors":[{"name":"  Studio Gaga  ","role":" Penciller "}],"tags":null,
               "isbn":"978-2-01-234567-8","links":[]}"#,
        )
        .unwrap();
        let updated = dto.apply_to(&base_metadata());
        assert_eq!(
            updated.authors,
            vec![Author::new("Studio Gaga", "penciller")]
        );
        assert!(updated.tags.is_empty());
        assert_eq!(updated.isbn, "9782012345678");
        assert!(updated.links.is_empty());

        let dto: BookMetadataUpdateDto =
            serde_json::from_str(r#"{"authors":null,"isbn":null}"#).unwrap();
        let updated = dto.apply_to(&base_metadata());
        assert!(updated.authors.is_empty());
        assert_eq!(updated.isbn, "");
    }

    #[test]
    fn violations_match_kotlin_validators() {
        let dto: BookMetadataUpdateDto = serde_json::from_str(
            r#"{"title":"  ","number":" ","isbn":"9781234567890",
               "authors":[{"role":"writer"}],"links":[{"url":"nope"}]}"#,
        )
        .unwrap();
        let violations = dto.violations();
        let fields: Vec<&str> = violations.iter().map(|v| v.field_name.as_str()).collect();
        assert!(fields.contains(&"title"));
        assert!(fields.contains(&"number"));
        assert!(fields.contains(&"isbn"));
        assert!(fields.contains(&"authors[0].name"));
        assert!(fields.contains(&"links[0].url"));
    }

    #[test]
    fn isbn13_rules() {
        assert!(is_valid_isbn13("9781234567897"));
        assert!(is_valid_isbn13("978-1-23-456789-7"));
        assert!(!is_valid_isbn13("9781234567890")); // bad check digit
        assert!(!is_valid_isbn13("9771234567897")); // bad prefix
        assert!(!is_valid_isbn13("123"));
    }

    #[test]
    fn number_sort_accepts_string_and_rejects_non_finite() {
        // the legacy WebUI serializes v-text-field number inputs as JSON strings
        let dto: BookMetadataUpdateDto = serde_json::from_str(r#"{"numberSort":"1.5"}"#).unwrap();
        assert_eq!(dto.number_sort, Some(1.5));

        // native numbers and explicit null keep working
        let dto: BookMetadataUpdateDto = serde_json::from_str(r#"{"numberSort":2.0}"#).unwrap();
        assert_eq!(dto.number_sort, Some(2.0));
        let dto: BookMetadataUpdateDto = serde_json::from_str(r#"{"numberSort":null}"#).unwrap();
        assert_eq!(dto.number_sort, None);

        // non-numeric / non-finite values are still rejected
        assert!(serde_json::from_str::<BookMetadataUpdateDto>(r#"{"numberSort":"abc"}"#).is_err());
        assert!(serde_json::from_str::<BookMetadataUpdateDto>(r#"{"numberSort":"NaN"}"#).is_err());
        assert!(
            serde_json::from_str::<BookMetadataUpdateDto>(r#"{"numberSort":"Infinity"}"#).is_err()
        );
    }
}
