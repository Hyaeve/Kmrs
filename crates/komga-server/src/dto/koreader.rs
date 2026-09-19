//! `DocumentProgressDto.kt` and `UserAuthenticationDto.kt` (KOReader sync payloads).

use serde::{Deserialize, Serialize};

/// KOReader progress payload. Jackson's `SnakeCaseStrategy` renames `deviceId` to `device_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentProgressDto {
    /// The document hash, computed using the KOReader partial MD5 algorithm
    pub document: String,
    /// Total progress percentage in the document, between 0 and 1
    pub percentage: f32,
    /// Current progress: a page number (1-based) for PDF/CBZ, or an EPUB position string
    pub progress: String,
    pub device: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserAuthenticationDto {
    pub authorized: String,
}

impl Default for UserAuthenticationDto {
    fn default() -> Self {
        Self {
            authorized: "OK".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_progress_snake_case() {
        let dto = DocumentProgressDto {
            document: "abc".into(),
            percentage: 0.5,
            progress: "12".into(),
            device: "Kobo".into(),
            device_id: "d1".into(),
        };
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "document": "abc",
                "percentage": 0.5,
                "progress": "12",
                "device": "Kobo",
                "device_id": "d1",
            })
        );
        assert_eq!(
            serde_json::from_value::<DocumentProgressDto>(json).unwrap(),
            dto
        );
    }

    #[test]
    fn user_authentication_default() {
        assert_eq!(
            serde_json::to_value(UserAuthenticationDto::default()).unwrap(),
            serde_json::json!({"authorized": "OK"})
        );
    }
}
