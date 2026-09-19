//! OAuth2 DTOs (`OAuth2Controller.kt`'s `OAuth2ClientDto` and the token/userinfo payloads).

use serde::{Deserialize, Serialize};

/// `GET /api/v1/oauth2/providers` item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuth2ClientDto {
    pub name: String,
    #[serde(rename = "registrationId")]
    pub registration_id: String,
}

/// Token endpoint response (`application/json`).
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: Option<String>,
    #[allow(dead_code)]
    pub token_type: Option<String>,
    /// RFC 6749 error fields; komga maps any exchange failure to `invalid_token_response`
    #[allow(dead_code)]
    pub error: Option<String>,
    #[allow(dead_code)]
    pub error_description: Option<String>,
}

/// The claims we read from the user-info endpoint.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserInfoClaims {
    pub email: Option<String>,
    #[serde(rename = "email_verified")]
    pub email_verified: Option<bool>,
}

/// One entry of the GitHub `/emails` response.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubEmail {
    pub email: Option<String>,
    pub verified: Option<bool>,
    pub primary: Option<bool>,
}

/// OIDC discovery document (subset).
#[derive(Debug, Clone, Deserialize)]
pub struct OidcDiscovery {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
}
