//! `KoboProxy.kt`: reverse proxy for unmatched `/kobo/{token}/*` requests to the Kobo store,
//! enabled by the `koboProxy` server setting.

use crate::api::kobo::KomgaSyncToken;
use crate::settings::KomgaSettings;
use axum::http::{HeaderMap, Method, StatusCode};
use std::sync::Arc;
use std::time::Duration;

const UPSTREAM: &str = "https://storeapi.kobo.com";
/// `KoboProxy.imageHostUrl`: redirect target for thumbnails unknown locally
pub const IMAGE_HOST_URL: &str =
    "https://cdn.kobo.com/book-images/{ImageId}/{Width}/{Height}/false/image.jpg";

const HEADERS_OUT_INCLUDE: &[&str] = &[
    "authorization",
    "user-agent",
    "accept",
    "accept-language",
    "content-type",
];

pub struct KoboProxy {
    client: reqwest::Client,
    upstream: String,
}

/// `ResponseEntity<JsonNode>`: upstream status, the `x-kobo-*` response headers, buffered body
pub struct ProxiedResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: serde_json::Value,
}

#[derive(Debug)]
pub enum ProxyError {
    /// upstream answered an error status (`ResponseStatusException(status, statusText)`)
    Upstream(StatusCode),
    Internal(String),
}

impl KoboProxy {
    /// `ClientHttpRequestFactorySettings`: 1 minute connect timeout, 1 minute read timeout
    pub fn new() -> Arc<Self> {
        Self::with_upstream(UPSTREAM)
    }

    pub fn with_upstream(upstream: &str) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(60))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client builds");
        Arc::new(Self {
            client,
            upstream: upstream.trim_end_matches('/').to_string(),
        })
    }

    pub fn is_enabled(settings: &KomgaSettings) -> bool {
        settings.kobo_proxy
    }

    /// `proxyCurrentRequest`. `sync_token` is the `includeSyncToken` semantics: the raw Kobo
    /// sync token is forwarded, and an upstream sync token in the response is re-wrapped into
    /// the Komga sync token.
    pub async fn proxy(
        &self,
        method: &Method,
        request_path: &str,
        query: Option<&str>,
        headers: &HeaderMap,
        body: Option<axum::body::Bytes>,
        sync_token: Option<&KomgaSyncToken>,
    ) -> Result<ProxiedResponse, ProxyError> {
        let path = strip_kobo_prefix(request_path).ok_or_else(|| {
            ProxyError::Internal(format!(
                "Could not get path from current request: {request_path}"
            ))
        })?;
        // EncodingMode.NONE: path and query are appended verbatim
        let mut url = format!("{}{path}", self.upstream);
        if let Some(query) = query.filter(|q| !q.is_empty()) {
            url.push('?');
            url.push_str(query);
        }

        let mut request = self.client.request(method.clone(), &url);
        for (name, value) in headers {
            let lower = name.as_str();
            if lower != "x-kobo-synctoken"
                && (HEADERS_OUT_INCLUDE.contains(&lower) || lower.starts_with("x-kobo-"))
            {
                request = request.header(name, value);
            }
        }
        if let Some(token) = sync_token {
            if token.raw_kobo_sync_token.trim().is_empty() {
                return Err(ProxyError::Internal(
                    "request must include sync token, but no raw Kobo sync token found".into(),
                ));
            }
            request = request.header("x-kobo-synctoken", &token.raw_kobo_sync_token);
        }
        if let Some(body) = body {
            request = request.body(body);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ProxyError::Internal(e.to_string()))?;
        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            return Err(ProxyError::Upstream(status));
        }

        let mut out_headers = HeaderMap::new();
        for (name, value) in response.headers() {
            if name.as_str().starts_with("x-kobo-") {
                out_headers.insert(name.clone(), value.clone());
            }
        }
        if let (Some(raw), Some(token)) = (
            out_headers
                .get("x-kobo-synctoken")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            sync_token,
        ) {
            let wrapped = KomgaSyncToken {
                raw_kobo_sync_token: raw,
                ..token.clone()
            }
            .to_base64();
            if let Ok(value) = axum::http::HeaderValue::from_str(&wrapped) {
                out_headers.insert("x-kobo-synctoken", value);
            }
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ProxyError::Internal(e.to_string()))?;
        let body = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|e| ProxyError::Internal(e.to_string()))?
        };
        Ok(ProxiedResponse {
            status,
            headers: out_headers,
            body,
        })
    }
}

/// `pathRegex`: `/kobo/[-\w]*(.*)` — the path after the token segment
fn strip_kobo_prefix(request_path: &str) -> Option<String> {
    let rest = request_path.strip_prefix("/kobo/")?;
    match rest.find('/') {
        Some(i) => Some(rest[i..].to_string()),
        None => Some(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_kobo_prefix() {
        assert_eq!(
            strip_kobo_prefix("/kobo/token-1/v1/library/sync"),
            Some("/v1/library/sync".to_string())
        );
        assert_eq!(strip_kobo_prefix("/kobo/token"), Some(String::new()));
        assert_eq!(strip_kobo_prefix("/other/x"), None);
    }
}
