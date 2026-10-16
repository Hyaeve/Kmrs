//! `ReadListCreationDto.kt` / `ReadListUpdateDto.kt` / `ReadListRequestMatchDto.kt`.

use crate::service::readlist::{ReadListMatch, ReadListRequestBookMatches, ReadListRequestMatch};
use komga_core::dto::dto_date;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadListCreationDto {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default = "default_ordered")]
    pub ordered: bool,
    pub book_ids: Vec<String>,
}

fn default_ordered() -> bool {
    true
}

impl ReadListCreationDto {
    /// `@NotBlank name` / `@NotEmpty @UniqueElements bookIds`
    pub fn violations(&self) -> Vec<crate::error::Violation> {
        let mut violations = vec![];
        if self.name.trim().is_empty() {
            violations.push(crate::error::Violation {
                field_name: "name".into(),
                message: "must not be blank".into(),
            });
        }
        violations.extend(crate::dto::collection::unique_violations(
            &self.book_ids,
            "bookIds",
        ));
        violations
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReadListUpdateDto {
    pub name: Option<String>,
    pub summary: Option<String>,
    pub book_ids: Option<Vec<String>>,
    pub ordered: Option<bool>,
}

impl ReadListUpdateDto {
    /// `@NullOrNotBlank name` / `@NullOrNotEmpty @UniqueElements bookIds`
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
        if let Some(book_ids) = &self.book_ids {
            if book_ids.is_empty() {
                violations.push(crate::error::Violation {
                    field_name: "bookIds".into(),
                    message: "Must be null or not empty".into(),
                });
            }
            violations.extend(crate::dto::collection::unique_violations(
                book_ids, "bookIds",
            ));
        }
        violations
    }
}

// region match DTOs (`ReadListRequestMatchDto.kt`)

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadListRequestMatchDto {
    pub read_list_match: ReadListMatchDto,
    pub requests: Vec<ReadListRequestBookMatchesDto>,
    pub error_code: String,
}

impl From<&ReadListRequestMatch> for ReadListRequestMatchDto {
    fn from(m: &ReadListRequestMatch) -> Self {
        Self {
            read_list_match: ReadListMatchDto::from(&m.read_list_match),
            requests: m
                .requests
                .iter()
                .map(ReadListRequestBookMatchesDto::from)
                .collect(),
            error_code: m.error_code.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadListMatchDto {
    pub name: String,
    pub error_code: String,
}

impl From<&ReadListMatch> for ReadListMatchDto {
    fn from(m: &ReadListMatch) -> Self {
        Self {
            name: m.name.clone(),
            error_code: m.error_code.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadListRequestBookMatchesDto {
    pub request: ReadListRequestBookDto,
    pub matches: Vec<ReadListRequestBookMatchDto>,
}

impl From<&ReadListRequestBookMatches> for ReadListRequestBookMatchesDto {
    fn from(m: &ReadListRequestBookMatches) -> Self {
        Self {
            request: ReadListRequestBookDto {
                series: m.request.series.clone(),
                number: m.request.number.clone(),
            },
            matches: m
                .matches
                .iter()
                .map(|(series, books)| ReadListRequestBookMatchDto {
                    series: ReadListRequestBookMatchSeriesDto {
                        series_id: series.id.clone(),
                        title: series.title.clone(),
                        release_date: series.release_date,
                    },
                    books: books
                        .iter()
                        .map(|b| ReadListRequestBookMatchBookDto {
                            book_id: b.id.clone(),
                            number: b.number.clone(),
                            title: b.title.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReadListRequestBookDto {
    pub series: BTreeSet<String>,
    pub number: String,
}

#[derive(Debug, Serialize)]
pub struct ReadListRequestBookMatchDto {
    pub series: ReadListRequestBookMatchSeriesDto,
    pub books: Vec<ReadListRequestBookMatchBookDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadListRequestBookMatchSeriesDto {
    pub series_id: String,
    pub title: String,
    #[serde(with = "dto_date")]
    pub release_date: Option<time::Date>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadListRequestBookMatchBookDto {
    pub book_id: String,
    pub number: String,
    pub title: String,
}

// endregion
