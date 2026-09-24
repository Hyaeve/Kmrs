//! Loose serde deserializers for DTO fields.
//!
//! The legacy komga WebUI serializes number inputs (`v-text-field type="number"`) and
//! some boolean inputs as JSON strings. Kotlin's Jackson backend coerces those
//! automatically; `serde_json`'s strict typing does not, so PATCH payloads such as
//! `{"taskPoolSize": "4"}` or `{"numberSort": "1.5"}` were rejected with 400.
//! These helpers accept both the native JSON type and its string form, mirroring
//! `json_u64`/`json_bool`/`optional_f64` in komga-riir.
//!
//! Semantics are preserved:
//! - single-`Option` fields: absent or explicit `null` → `None` (not provided);
//! - double-`Option` (`isSet`) fields: absent → `None`, explicit `null` → `Some(None)` (clear);
//! - wrong types and out-of-range / non-finite values still fail deserialization.

use serde::Deserialize;
use serde_json::Value;

/// Parse a JSON value as a boolean, tolerating string representations
/// ("true"/"false"/"1"/"0", case-insensitive); deliberately more lenient than
/// the Kotlin backend, which rejects "1"/"0".
fn loose_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(boolean) => Some(*boolean),
        Value::String(string) => match string.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Parse a JSON value as a signed integer, tolerating numeric strings.
fn loose_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(string) => string.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// Parse a JSON value as an unsigned 16-bit integer, tolerating numeric strings.
fn loose_u16(value: &Value) -> Option<u16> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|v| u16::try_from(v).ok()),
        Value::String(string) => string.trim().parse::<u16>().ok(),
        _ => None,
    }
}

/// Parse a JSON value as a finite floating point, tolerating numeric strings;
/// non-finite values are rejected (stricter than the Kotlin backend, which accepts them).
fn loose_f64(value: &Value) -> Option<f64> {
    let parsed = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(string) => string.trim().parse::<f64>().ok(),
        _ => None,
    };
    parsed.filter(|value| value.is_finite())
}

/// `Option<bool>` field: JSON bool or "true"/"false"/"1"/"0" strings; null → None.
/// The error message recommends the canonical "true"/"false" forms only.
pub fn loose_bool_opt<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(value) => loose_bool(&value)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected a boolean or \"true\"/\"false\"")),
    }
}

/// `Option<i64>` field: JSON integer or numeric string; null → None.
pub fn loose_i64_opt<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(value) => loose_i64(&value)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected an integer or a numeric string")),
    }
}

/// Required `i32` field: JSON integer or numeric string.
pub fn loose_i32<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    loose_i64(&value)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| serde::de::Error::custom("expected an integer or a numeric string"))
}

/// `Option<f32>` field: JSON number or numeric string; null → None; non-finite → error.
pub fn loose_f32_opt<'de, D>(deserializer: D) -> Result<Option<f32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(value) => loose_f64(&value)
            .map(|v| v as f32)
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or_else(|| {
                serde::de::Error::custom("expected a finite number or a numeric string")
            }),
    }
}

/// `isSet` double-`Option` field over a loose parse: absent → None,
/// explicit null → Some(None), value → Some(Some(v)).
fn loose_some<'de, D, T>(
    deserializer: D,
    parse: impl Fn(&Value) -> Option<T>,
    message: &'static str,
) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let inner = match Option::<Value>::deserialize(deserializer)? {
        None => None,
        Some(value) => Some(parse(&value).ok_or_else(|| serde::de::Error::custom(message))?),
    };
    Ok(Some(inner))
}

/// `isSet` `Option<Option<i32>>`: JSON integer or numeric string; explicit null clears.
pub fn loose_some_i32<'de, D>(deserializer: D) -> Result<Option<Option<i32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    loose_some(
        deserializer,
        |value| loose_i64(value).and_then(|v| i32::try_from(v).ok()),
        "expected an integer or a numeric string, or null",
    )
}

/// `isSet` `Option<Option<u16>>`: JSON integer or numeric string; explicit null clears.
pub fn loose_some_u16<'de, D>(deserializer: D) -> Result<Option<Option<u16>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    loose_some(
        deserializer,
        loose_u16,
        "expected an integer or a numeric string, or null",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Default, Debug, PartialEq)]
    #[serde(rename_all = "camelCase", default)]
    struct Sample {
        #[serde(deserialize_with = "loose_bool_opt")]
        flag: Option<bool>,
        #[serde(deserialize_with = "loose_i64_opt")]
        count: Option<i64>,
        #[serde(deserialize_with = "loose_f32_opt")]
        ratio: Option<f32>,
        #[serde(deserialize_with = "loose_some_i32")]
        age: Option<Option<i32>>,
        #[serde(deserialize_with = "loose_some_u16")]
        port: Option<Option<u16>>,
    }

    #[test]
    fn accepts_native_and_string_forms() {
        let s: Sample = serde_json::from_str(
            r#"{"flag":"true","count":"42","ratio":"1.5","age":"18","port":"8080"}"#,
        )
        .unwrap();
        assert_eq!(s.flag, Some(true));
        assert_eq!(s.count, Some(42));
        assert_eq!(s.ratio, Some(1.5));
        assert_eq!(s.age, Some(Some(18)));
        assert_eq!(s.port, Some(Some(8080)));

        let s: Sample =
            serde_json::from_str(r#"{"flag":false,"count":7,"ratio":2.0,"age":12,"port":17878}"#)
                .unwrap();
        assert_eq!(s.flag, Some(false));
        assert_eq!(s.count, Some(7));
        assert_eq!(s.ratio, Some(2.0));
        assert_eq!(s.age, Some(Some(12)));
        assert_eq!(s.port, Some(Some(17878)));

        // "1"/"0" are accepted by deliberate choice
        let s: Sample = serde_json::from_str(r#"{"flag":"1"}"#).unwrap();
        assert_eq!(s.flag, Some(true));
        let s: Sample = serde_json::from_str(r#"{"flag":"0"}"#).unwrap();
        assert_eq!(s.flag, Some(false));
    }

    #[test]
    fn preserves_absent_and_null_semantics() {
        let s: Sample = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(s, Sample::default());

        let s: Sample = serde_json::from_str(r#"{"age":null,"port":null}"#).unwrap();
        assert_eq!(s.age, Some(None));
        assert_eq!(s.port, Some(None));
        assert_eq!(s.flag, None);
    }

    #[test]
    fn rejects_invalid_values() {
        assert!(serde_json::from_str::<Sample>(r#"{"count":"abc"}"#).is_err());
        assert!(serde_json::from_str::<Sample>(r#"{"flag":"yes"}"#).is_err());
        assert!(serde_json::from_str::<Sample>(r#"{"ratio":"NaN"}"#).is_err());
        assert!(serde_json::from_str::<Sample>(r#"{"age":"3000000000"}"#).is_err());
        assert!(serde_json::from_str::<Sample>(r#"{"port":"70000"}"#).is_err());
        assert!(serde_json::from_str::<Sample>(r#"{"count":4.5}"#).is_err());
    }
}
