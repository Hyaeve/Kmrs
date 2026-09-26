//! API error shapes, aligned with Spring Boot (`include-message: always`) and komga's `ErrorHandlingControllerAdvice`:
//! - business errors: `{timestamp,status,error,message,path}`.
//! - bean validation failures: 400 `{violations:[{fieldName,message}]}`.
//! - `EntityNotFoundException`: 404 empty body.
//! - `CodedException`: 400, message is `ERR_xxxx`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use komga_core::error::CodedError;
use serde::Serialize;

#[derive(Debug)]
#[allow(dead_code)] // variants like NotFoundEmpty are used by endpoints in M3
pub enum ApiError {
    /// 401 empty body + `WWW-Authenticate: Basic realm="Realm"`
    Unauthorized,
    /// 404 empty body (EntityNotFoundException semantics)
    NotFoundEmpty,
    /// Spring default error JSON
    Status { status: StatusCode, message: String },
    /// 400 + violations
    Violations(Vec<Violation>),
    /// 500
    Internal(String),
}

#[derive(Debug, Serialize)]
pub struct Violation {
    #[serde(rename = "fieldName")]
    pub field_name: String,
    pub message: String,
}

#[allow(dead_code)]
impl ApiError {
    pub fn unauthorized() -> Self {
        Self::Unauthorized
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::Status {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::Status {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Status {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Status {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    pub fn unsupported_media_type(message: impl Into<String>) -> Self {
        Self::Status {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            message: message.into(),
        }
    }
}

impl From<CodedError> for ApiError {
    fn from(e: CodedError) -> Self {
        Self::bad_request(e.0)
    }
}

impl From<komga_db::Error> for ApiError {
    fn from(e: komga_db::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    timestamp: String,
    status: u16,
    error: String,
    message: String,
    path: String,
}

#[derive(Serialize)]
struct ViolationsBody {
    violations: Vec<Violation>,
}

fn reason_phrase(status: StatusCode) -> String {
    status.canonical_reason().unwrap_or("").to_string()
}

/// Spring's `HttpStatus.toString()` uses the enum constant name, e.g. `NOT_FOUND`
fn spring_status_name(status: StatusCode) -> String {
    reason_phrase(status)
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// `ResponseStatusException.getMessage()`: `"404 NOT_FOUND"` without a reason,
/// `404 NOT_FOUND "reason"` with one.
fn spring_message(status: StatusCode, reason: &str) -> String {
    let base = format!("{} {}", status.as_u16(), spring_status_name(status));
    if reason.is_empty() {
        base
    } else {
        format!("{base} \"{reason}\"")
    }
}

/// Timestamp of Spring `DefaultErrorAttributes`: `yyyy-MM-dd'T'HH:mm:ss.SSS+00:00`.
fn now_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}+00:00",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.nanosecond() / 1_000_000,
    )
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                [(
                    axum::http::header::WWW_AUTHENTICATE,
                    "Basic realm=\"Realm\"",
                )],
            )
                .into_response(),
            ApiError::NotFoundEmpty => StatusCode::NOT_FOUND.into_response(),
            ApiError::Status { status, message } => {
                let body = ErrorBody {
                    timestamp: now_timestamp(),
                    status: status.as_u16(),
                    error: reason_phrase(status),
                    message: spring_message(status, &message),
                    path: String::new(), // filled in with the request path by error_path_middleware
                };
                let mut response = (status, Json(body)).into_response();
                response.headers_mut().insert(
                    crate::http::error_path::ERROR_MARKER,
                    axum::http::HeaderValue::from_static("1"),
                );
                response
            }
            ApiError::Violations(violations) => {
                (StatusCode::BAD_REQUEST, Json(ViolationsBody { violations })).into_response()
            }
            // Spring logs the uncaught exception behind a 500; without this the detail
            // (e.g. which archive entry failed to decompress) only exists in the response body
            ApiError::Internal(message) => {
                tracing::error!("{message}");
                let body = ErrorBody {
                    timestamp: now_timestamp(),
                    status: 500,
                    error: "Internal Server Error".into(),
                    message,
                    path: String::new(),
                };
                let mut response = (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response();
                response.headers_mut().insert(
                    crate::http::error_path::ERROR_MARKER,
                    axum::http::HeaderValue::from_static("1"),
                );
                response
            }
        }
    }
}
