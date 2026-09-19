//! Readium/OPDS-2 progression DTOs: `R2Locator.kt`, `R2Device.kt`, `R2Progression.kt`,
//! `R2Positions.kt`, and `ReadProgressUpdateDto.kt`.

use crate::model::read_progress::ReadProgress;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// `R2Locator` is `@JsonInclude(NON_EMPTY)`: null fields, empty strings, and empty
/// collections are all omitted from the JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct R2Locator {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub href: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub locations: Option<R2Location>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub text: Option<R2Text>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    #[serde(rename = "koboSpan")]
    pub kobo_span: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct R2Location {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    pub fragments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub progression: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub position: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    #[serde(rename = "totalProgression")]
    pub total_progression: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct R2Text {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub highlight: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct R2Device {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct R2Progression {
    #[serde(with = "zoned_date_time")]
    pub modified: OffsetDateTime,
    pub device: R2Device,
    pub locator: R2Locator,
}

impl From<&ReadProgress> for R2Progression {
    /// `ReadProgress.toR2Progression()`
    fn from(p: &ReadProgress) -> Self {
        Self {
            modified: p.read_date,
            device: R2Device {
                id: p.device_id.clone(),
                name: p.device_name.clone(),
            },
            locator: p
                .locator
                .as_ref()
                .and_then(|l| serde_json::from_value(l.clone()).ok())
                .unwrap_or(R2Locator {
                    href: String::new(),
                    type_: String::new(),
                    title: None,
                    locations: None,
                    text: None,
                    kobo_span: None,
                }),
        }
    }
}

/// `ZonedDateTime` Jackson default (ISO_OFFSET_DATE_TIME): `yyyy-MM-dd'T'HH:mm:ss` plus a
/// fraction in 3-digit groups when nanos is non-zero, plus `Z` for UTC.
pub mod zoned_date_time {
    use serde::{Deserializer, Serializer};
    use time::OffsetDateTime;

    pub fn serialize<S: Serializer>(dt: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error> {
        let nanos = dt.nanosecond();
        let fraction = if nanos == 0 {
            String::new()
        } else {
            let digits = format!("{nanos:09}");
            let trimmed = digits.trim_end_matches('0');
            // Jackson writes the fraction in groups of 3 digits
            let len = trimmed.len().div_ceil(3) * 3;
            format!(".{}", &digits[..len])
        };
        serializer.serialize_str(&format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{fraction}Z",
            dt.year(),
            dt.month() as u8,
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second(),
        ))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<OffsetDateTime, D::Error> {
        time::serde::iso8601::deserialize(deserializer)
    }
}

/// Optional variant of `zoned_date_time`
pub mod zoned_date_time_opt {
    use serde::{Deserializer, Serializer};
    use time::OffsetDateTime;

    pub fn serialize<S: Serializer>(
        dt: &Option<OffsetDateTime>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match dt {
            Some(dt) => super::zoned_date_time::serialize(dt, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<OffsetDateTime>, D::Error> {
        Ok(Some(time::serde::iso8601::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct R2Positions {
    pub total: i32,
    pub positions: Vec<R2Locator>,
}

/// `ReadProgressUpdateDto`: page may be omitted only when completed is true
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadProgressUpdateDto {
    /// `@Positive`
    pub page: Option<i32>,
    pub completed: Option<bool>,
}

impl ReadProgressUpdateDto {
    /// The class-level `ReadProgressUpdateDtoConstraint`
    pub fn is_valid(&self) -> bool {
        self.page.is_some() || self.completed == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_non_empty_serialization() {
        let locator = R2Locator {
            href: String::new(),
            type_: String::new(),
            title: None,
            locations: None,
            text: None,
            kobo_span: None,
        };
        assert_eq!(serde_json::to_string(&locator).unwrap(), "{}");

        let locator = R2Locator {
            href: "OEBPS/ch1.xhtml".into(),
            type_: "application/xhtml+xml".into(),
            title: None,
            locations: Some(R2Location {
                fragments: vec![],
                progression: Some(0.5),
                position: None,
                total_progression: None,
            }),
            text: None,
            kobo_span: None,
        };
        let json = serde_json::to_value(&locator).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "href": "OEBPS/ch1.xhtml",
                "type": "application/xhtml+xml",
                "locations": {"progression": 0.5}
            })
        );
        assert_eq!(serde_json::from_value::<R2Locator>(json).unwrap(), locator);
    }

    #[test]
    fn progression_modified_format() {
        let dt = crate::time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap();
        let json = serde_json::to_value(R2Progression {
            modified: dt,
            device: R2Device {
                id: "d1".into(),
                name: "phone".into(),
            },
            locator: R2Locator {
                href: "h".into(),
                type_: "t".into(),
                title: None,
                locations: None,
                text: None,
                kobo_span: None,
            },
        })
        .unwrap();
        assert_eq!(json["modified"], "2024-01-02T03:04:05Z");

        let dt = crate::time_codec::parse_datetime_utc("2024-01-02 03:04:05.123456789").unwrap();
        let json = serde_json::to_value(R2Progression {
            modified: dt,
            device: R2Device {
                id: "d1".into(),
                name: "phone".into(),
            },
            locator: R2Locator {
                href: "h".into(),
                type_: "t".into(),
                title: None,
                locations: None,
                text: None,
                kobo_span: None,
            },
        })
        .unwrap();
        assert_eq!(json["modified"], "2024-01-02T03:04:05.123456789Z");

        let dt = crate::time_codec::parse_datetime_utc("2024-01-02 03:04:05.120").unwrap();
        let json = serde_json::to_value(R2Progression {
            modified: dt,
            device: R2Device {
                id: "d1".into(),
                name: "phone".into(),
            },
            locator: R2Locator {
                href: "h".into(),
                type_: "t".into(),
                title: None,
                locations: None,
                text: None,
                kobo_span: None,
            },
        })
        .unwrap();
        // ISO_OFFSET_DATE_TIME prints the fraction in groups of 3 digits
        assert_eq!(json["modified"], "2024-01-02T03:04:05.120Z");
    }

    #[test]
    fn update_dto_validation() {
        assert!(ReadProgressUpdateDto {
            page: Some(1),
            completed: None
        }
        .is_valid());
        assert!(ReadProgressUpdateDto {
            page: None,
            completed: Some(true)
        }
        .is_valid());
        assert!(!ReadProgressUpdateDto {
            page: None,
            completed: None
        }
        .is_valid());
        assert!(!ReadProgressUpdateDto {
            page: None,
            completed: Some(false)
        }
        .is_valid());
    }
}
