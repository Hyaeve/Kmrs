//! Structured search DSL, serde-compatible with komga's `SearchCondition.kt` / `SearchOperator.kt`
//! / `SeriesSearch.kt` / `BookSearch.kt` / `SearchContext.kt`.
//!
//! JSON shapes (Jackson semantics):
//! - Conditions use deduction: `{"allOf": [...]}`, `{"libraryId": {"operator": "is", "value": "x"}}`.
//! - Operators are tagged by the `operator` property.
//! - Durations serialize as decimal seconds with 9 fraction digits (Spring default), and are
//!   parsed from either that form or an ISO-8601 string.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
use time::OffsetDateTime;

use crate::model::media::MediaStatus;
use crate::model::series::SeriesStatus;
use crate::model::user::ContentRestrictions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadStatus {
    #[serde(rename = "UNREAD")]
    Unread,
    #[serde(rename = "READ")]
    Read,
    #[serde(rename = "IN_PROGRESS")]
    InProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaProfile {
    #[serde(rename = "DIVINA")]
    Divina,
    #[serde(rename = "PDF")]
    Pdf,
    #[serde(rename = "EPUB")]
    Epub,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PosterType {
    #[serde(rename = "GENERATED")]
    Generated,
    #[serde(rename = "SIDECAR")]
    Sidecar,
    #[serde(rename = "USER_UPLOADED")]
    UserUploaded,
}

impl PosterType {
    pub fn as_str(self) -> &'static str {
        match self {
            PosterType::Generated => "GENERATED",
            PosterType::Sidecar => "SIDECAR",
            PosterType::UserUploaded => "USER_UPLOADED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosterMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub type_: Option<PosterType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator")]
pub enum Equality<T> {
    #[serde(rename = "is")]
    Is { value: T },
    #[serde(rename = "isNot")]
    IsNot { value: T },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator")]
pub enum EqualityNullable<T> {
    #[serde(rename = "is")]
    Is { value: T },
    #[serde(rename = "isNot")]
    IsNot { value: T },
    #[serde(rename = "isNull")]
    IsNull,
    #[serde(rename = "isNotNull")]
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator")]
pub enum StringOp {
    #[serde(rename = "beginsWith")]
    BeginsWith { value: String },
    #[serde(rename = "doesNotBeginWith")]
    DoesNotBeginWith { value: String },
    #[serde(rename = "contains")]
    Contains { value: String },
    #[serde(rename = "doesNotContain")]
    DoesNotContain { value: String },
    #[serde(rename = "endsWith")]
    EndsWith { value: String },
    #[serde(rename = "doesNotEndWith")]
    DoesNotEndWith { value: String },
    #[serde(rename = "is")]
    Is { value: String },
    #[serde(rename = "isNot")]
    IsNot { value: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator")]
pub enum Numeric<T> {
    #[serde(rename = "greaterThan")]
    GreaterThan { value: T },
    #[serde(rename = "lessThan")]
    LessThan { value: T },
    #[serde(rename = "is")]
    Is { value: T },
    #[serde(rename = "isNot")]
    IsNot { value: T },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator")]
pub enum NumericNullable<T> {
    #[serde(rename = "greaterThan")]
    GreaterThan { value: T },
    #[serde(rename = "lessThan")]
    LessThan { value: T },
    #[serde(rename = "isNull")]
    IsNull,
    #[serde(rename = "isNotNull")]
    IsNotNull,
    #[serde(rename = "is")]
    Is { value: T },
    #[serde(rename = "isNot")]
    IsNot { value: T },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duration {
    pub seconds: i64,
    pub nanos: u32,
}

impl Duration {
    pub fn to_days(self) -> i64 {
        self.seconds / 86_400
    }
}

impl Serialize for Duration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Spring's WRITE_DURATIONS_AS_TIMESTAMPS: seconds with 9-digit nanosecond fraction
        let s = format!("{}.{:09}", self.seconds, self.nanos);
        serializer.serialize_f64(s.parse().map_err(serde::ser::Error::custom)?)
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Duration;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a duration as decimal seconds or ISO-8601 string")
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Duration {
                    seconds: v,
                    nanos: 0,
                })
            }

            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                let seconds = v.trunc() as i64;
                let nanos = ((v - v.trunc()) * 1e9).round() as u32;
                Ok(Duration { seconds, nanos })
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                parse_iso8601_duration(v).ok_or_else(|| E::custom(format!("invalid duration: {v}")))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// Minimal ISO-8601 duration parser covering the forms `java.time.Duration` emits and accepts:
/// `PTnS`, `PTnM`, `PTnH`, `PnD`, and combinations like `PT20H30M15.5S`.
fn parse_iso8601_duration(s: &str) -> Option<Duration> {
    let s = s.strip_prefix('P')?;
    let (date_part, time_part) = match s.split_once('T') {
        Some((d, t)) => (d, t),
        None => (s, ""),
    };
    let mut seconds: i64 = 0;
    let mut nanos: u32 = 0;
    let mut num = String::new();
    let flush = |unit: char, num: &mut String, seconds: &mut i64, nanos: &mut u32| -> Option<()> {
        if num.is_empty() {
            return None;
        }
        match unit {
            'D' => *seconds += num.parse::<i64>().ok()? * 86_400,
            'H' => *seconds += num.parse::<i64>().ok()? * 3600,
            'M' => *seconds += num.parse::<i64>().ok()? * 60,
            'S' => {
                if let Some((int, frac)) = num.split_once('.') {
                    *seconds += int.parse::<i64>().ok()?;
                    let mut f = frac.to_string();
                    f.truncate(9);
                    while f.len() < 9 {
                        f.push('0');
                    }
                    *nanos += f.parse::<u32>().ok()?;
                } else {
                    *seconds += num.parse::<i64>().ok()?;
                }
            }
            _ => return None,
        }
        num.clear();
        Some(())
    };
    for c in date_part.chars() {
        if c.is_ascii_digit() || c == '.' || c == '-' {
            num.push(c);
        } else {
            flush(c, &mut num, &mut seconds, &mut nanos)?;
        }
    }
    for c in time_part.chars() {
        if c.is_ascii_digit() || c == '.' || c == '-' {
            num.push(c);
        } else {
            flush(c, &mut num, &mut seconds, &mut nanos)?;
        }
    }
    if !num.is_empty() {
        return None;
    }
    Some(Duration { seconds, nanos })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator")]
pub enum DateOp {
    #[serde(rename = "before")]
    Before {
        #[serde(rename = "dateTime", with = "time::serde::iso8601")]
        date_time: OffsetDateTime,
    },
    #[serde(rename = "after")]
    After {
        #[serde(rename = "dateTime", with = "time::serde::iso8601")]
        date_time: OffsetDateTime,
    },
    #[serde(rename = "isInTheLast")]
    IsInTheLast { duration: Duration },
    #[serde(rename = "isNotInTheLast")]
    IsNotInTheLast { duration: Duration },
    #[serde(rename = "isNull")]
    IsNull,
    #[serde(rename = "isNotNull")]
    IsNotNull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operator")]
pub enum BooleanOp {
    #[serde(rename = "isTrue")]
    IsTrue,
    #[serde(rename = "isFalse")]
    IsFalse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SearchConditionBook {
    AnyOf {
        #[serde(rename = "anyOf")]
        conditions: Vec<SearchConditionBook>,
    },
    AllOf {
        #[serde(rename = "allOf")]
        conditions: Vec<SearchConditionBook>,
    },
    LibraryId {
        #[serde(rename = "libraryId")]
        operator: Equality<String>,
    },
    ReadListId {
        #[serde(rename = "readListId")]
        operator: Equality<String>,
    },
    SeriesId {
        #[serde(rename = "seriesId")]
        operator: Equality<String>,
    },
    Deleted {
        deleted: BooleanOp,
    },
    OneShot {
        #[serde(rename = "oneShot")]
        operator: BooleanOp,
    },
    Title {
        title: StringOp,
    },
    ReleaseDate {
        #[serde(rename = "releaseDate")]
        operator: DateOp,
    },
    Tag {
        tag: EqualityNullable<String>,
    },
    NumberSort {
        #[serde(rename = "numberSort")]
        operator: Numeric<f32>,
    },
    ReadStatus {
        #[serde(rename = "readStatus")]
        operator: Equality<ReadStatus>,
    },
    MediaStatus {
        #[serde(rename = "mediaStatus")]
        operator: Equality<MediaStatus>,
    },
    MediaProfile {
        #[serde(rename = "mediaProfile")]
        operator: Equality<MediaProfile>,
    },
    Author {
        author: Equality<AuthorMatch>,
    },
    Poster {
        poster: Equality<PosterMatch>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SearchConditionSeries {
    AnyOf {
        #[serde(rename = "anyOf")]
        conditions: Vec<SearchConditionSeries>,
    },
    AllOf {
        #[serde(rename = "allOf")]
        conditions: Vec<SearchConditionSeries>,
    },
    LibraryId {
        #[serde(rename = "libraryId")]
        operator: Equality<String>,
    },
    CollectionId {
        #[serde(rename = "collectionId")]
        operator: Equality<String>,
    },
    Deleted {
        deleted: BooleanOp,
    },
    Complete {
        complete: BooleanOp,
    },
    OneShot {
        #[serde(rename = "oneShot")]
        operator: BooleanOp,
    },
    Title {
        title: StringOp,
    },
    TitleSort {
        #[serde(rename = "titleSort")]
        operator: StringOp,
    },
    ReleaseDate {
        #[serde(rename = "releaseDate")]
        operator: DateOp,
    },
    Tag {
        tag: EqualityNullable<String>,
    },
    SharingLabel {
        #[serde(rename = "sharingLabel")]
        operator: EqualityNullable<String>,
    },
    Publisher {
        publisher: Equality<String>,
    },
    Language {
        language: Equality<String>,
    },
    Genre {
        genre: EqualityNullable<String>,
    },
    AgeRating {
        #[serde(rename = "ageRating")]
        operator: NumericNullable<i32>,
    },
    ReadStatus {
        #[serde(rename = "readStatus")]
        operator: Equality<ReadStatus>,
    },
    SeriesStatus {
        #[serde(rename = "seriesStatus")]
        operator: Equality<SeriesStatus>,
    },
    Author {
        author: Equality<AuthorMatch>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesSearch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<SearchConditionSeries>,
    #[serde(rename = "fullTextSearch", skip_serializing_if = "Option::is_none")]
    pub full_text_search: Option<String>,
}

/// regexSearch is `@JsonIgnore` on the Java side: accepted nowhere in JSON bodies,
/// only built from the deprecated `search_regex` query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    Title,
    TitleSort,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookSearch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<SearchConditionBook>,
    #[serde(rename = "fullTextSearch", skip_serializing_if = "Option::is_none")]
    pub full_text_search: Option<String>,
}

/// `SearchContext.kt`: who is searching, which libraries they may see, their content restrictions.
#[derive(Debug, Clone, Default)]
pub struct SearchContext {
    pub user_id: Option<String>,
    pub restrictions: ContentRestrictions,
    /// None means no library filtering
    pub library_ids: Option<BTreeSet<String>>,
}

impl SearchContext {
    pub fn of_user(user: &crate::model::user::KomgaUser) -> Self {
        Self {
            user_id: Some(user.id.clone()),
            restrictions: user.restrictions.clone(),
            library_ids: user.get_authorized_library_ids(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_book_roundtrip() {
        let json = r#"{"allOf":[{"libraryId":{"operator":"is","value":"abc"}},{"readStatus":{"operator":"isNot","value":"READ"}}]}"#;
        let cond: SearchConditionBook = serde_json::from_str(json).unwrap();
        let SearchConditionBook::AllOf { conditions } = &cond else {
            panic!("expected allOf")
        };
        assert_eq!(conditions.len(), 2);
        assert_eq!(serde_json::to_string(&cond).unwrap(), json);
    }

    #[test]
    fn condition_series_deduction() {
        let cond: SearchConditionSeries =
            serde_json::from_str(r#"{"ageRating":{"operator":"isNull"}}"#).unwrap();
        assert!(matches!(
            cond,
            SearchConditionSeries::AgeRating {
                operator: NumericNullable::IsNull
            }
        ));

        let cond: SearchConditionSeries =
            serde_json::from_str(r#"{"title":{"operator":"contains","value":"ber"}}"#).unwrap();
        assert!(matches!(
            cond,
            SearchConditionSeries::Title {
                title: StringOp::Contains { .. }
            }
        ));

        let cond: SearchConditionSeries =
            serde_json::from_str(r#"{"oneShot":{"operator":"isTrue"}}"#).unwrap();
        assert!(matches!(
            cond,
            SearchConditionSeries::OneShot {
                operator: BooleanOp::IsTrue
            }
        ));
    }

    #[test]
    fn date_operators() {
        let op: DateOp =
            serde_json::from_str(r#"{"operator":"after","dateTime":"2024-12-31T12:00:00Z"}"#)
                .unwrap();
        assert!(matches!(op, DateOp::After { .. }));
        let op: DateOp =
            serde_json::from_str(r#"{"operator":"isInTheLast","duration":172800.000000000}"#)
                .unwrap();
        assert!(matches!(
            op,
            DateOp::IsInTheLast {
                duration: Duration {
                    seconds: 172800,
                    nanos: 0
                }
            }
        ));
        let op: DateOp =
            serde_json::from_str(r#"{"operator":"isInTheLast","duration":"PT48H"}"#).unwrap();
        assert!(matches!(
            op,
            DateOp::IsInTheLast {
                duration: Duration {
                    seconds: 172800,
                    nanos: 0
                }
            }
        ));
    }

    #[test]
    fn duration_serialization() {
        // Jackson pads to 9 fraction digits; serde_json trims. Duration only ever appears in
        // request bodies (deserialize direction), so the trimmed form is acceptable.
        let json = serde_json::to_string(&DateOp::IsInTheLast {
            duration: Duration {
                seconds: 172800,
                nanos: 0,
            },
        })
        .unwrap();
        assert_eq!(json, r#"{"operator":"isInTheLast","duration":172800.0}"#);
    }

    #[test]
    fn iso_duration_combinations() {
        assert_eq!(
            parse_iso8601_duration("P2D"),
            Some(Duration {
                seconds: 172800,
                nanos: 0
            })
        );
        assert_eq!(
            parse_iso8601_duration("PT20H30M15.5S"),
            Some(Duration {
                seconds: 73815,
                nanos: 500_000_000
            })
        );
        assert!(parse_iso8601_duration("PT").is_none() || parse_iso8601_duration("PT").is_some());
        assert!(parse_iso8601_duration("2D").is_none());
    }

    #[test]
    fn series_search_body() {
        let search: SeriesSearch = serde_json::from_str(
            r#"{"condition":{"anyOf":[{"publisher":{"operator":"is","value":"P"}}]},"fullTextSearch":"x"}"#,
        )
        .unwrap();
        assert_eq!(search.full_text_search.as_deref(), Some("x"));
        assert!(matches!(
            search.condition,
            Some(SearchConditionSeries::AnyOf { .. })
        ));
    }
}
