//! komf integration: provisions a komf metadata fetcher with a minted API key and the
//! Komga base URL through komf's `PATCH /api/config` (hot-reloaded, komf needs no
//! changes), then keeps it reconciled. komf calls back in Komga mode (REST + SSE).
//! komf's own API is unauthenticated unless its optional `KOMF_AUTH_KEY` gate is set;
//! kmrs then presents the configured key as a Bearer credential on every request
//! (see `komf_auth_key`). Without a key the integration assumes a trusted network.

use crate::service::user::mint_api_key;
use crate::state::AppState;
use anyhow::{bail, Context};
use komga_db::dao::komf_integration::{KomfIntegration, KomfIntegrationDao, KomfIntegrationState};
use komga_db::dao::user::UserDao;
use std::time::Duration;

pub const API_KEY_COMMENT: &str = "Integration · komf";

/// Proxied calls that fan out to external providers (metadata search, the OAuth
/// token exchange) get a more generous budget than the client default.
pub const METADATA_PROXY_TIMEOUT: Duration = Duration::from_secs(30);

/// Serializes provision/disconnect so a manual reconfigure racing the reconciliation
/// loop cannot mint two keys and orphan one of them.
fn integration_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[derive(Clone)]
pub struct KomfClient {
    base_url: String,
    /// komf-rs's optional `KOMF_AUTH_KEY` gate credential; komf ignores the header
    /// entirely while its gate is off, so attaching it unconditionally is safe
    auth_key: Option<String>,
    http: reqwest::Client,
    // reqwest follows redirects by default, which would swallow the 302 komf's OAuth
    // endpoints answer with; this client surfaces them to the caller
    http_no_redirect: reqwest::Client,
    // a total timeout spans the response body, so the 10s budget of `http` would cut
    // long-lived SSE streams; this client carries only a connect timeout
    http_stream: reqwest::Client,
}

impl KomfClient {
    pub fn new(base_url: &str, auth_key: Option<&str>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kmrs/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        let http_no_redirect = reqwest::Client::builder()
            .user_agent(concat!("kmrs/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client");
        let http_stream = reqwest::Client::builder()
            .user_agent(concat!("kmrs/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_key: auth_key
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_string),
            http,
            http_no_redirect,
            http_stream,
        }
    }

    /// komf-rs expects its auth key base64-encoded in the Bearer token, keeping the
    /// raw key out of request headers and access logs.
    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth_key {
            Some(key) => {
                use base64::Engine as _;
                request.header(
                    reqwest::header::AUTHORIZATION,
                    format!(
                        "Bearer {}",
                        base64::engine::general_purpose::STANDARD.encode(key.as_bytes())
                    ),
                )
            }
            None => request,
        }
    }

    fn url(&self, path: &str, query: Option<&str>) -> String {
        let mut url = format!("{}{path}", self.base_url);
        if let Some(query) = query.filter(|q| !q.is_empty()) {
            url.push('?');
            url.push_str(query);
        }
        url
    }

    /// komf serves a plain 200 at its root; used for the live reachability probe.
    pub async fn health(&self) -> anyhow::Result<()> {
        self.authorize(self.http.get(&self.base_url))
            .timeout(Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get_config(&self) -> anyhow::Result<KomfKomgaConfig> {
        let config: KomfConfigDto = self
            .authorize(self.http.get(format!("{}/api/config", self.base_url)))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(config.komga)
    }

    pub async fn patch_config(&self, body: &serde_json::Value) -> anyhow::Result<()> {
        self.authorize(self.http.patch(format!("{}/api/config", self.base_url)))
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Forwards to a komga-mode metadata endpoint. Transport failures (unreachable,
    /// timeout) are the Err case; komf's own status codes come back in Ok so the
    /// caller can relay them untouched.
    pub async fn proxy_metadata(
        &self,
        method: reqwest::Method,
        path: &str,
        query: Option<&str>,
        body: Option<&serde_json::Value>,
    ) -> anyhow::Result<reqwest::Response> {
        self.proxy(method, path, query, body, METADATA_PROXY_TIMEOUT)
            .await
    }

    /// Forwards to komf's `/api/config`. Same error split as `proxy_metadata`.
    pub async fn proxy_config(
        &self,
        method: reqwest::Method,
        body: Option<&serde_json::Value>,
    ) -> anyhow::Result<reqwest::Response> {
        self.proxy(method, "/api/config", None, body, Duration::from_secs(10))
            .await
    }

    /// Forwards to komf's jobs API (`/api/jobs`). Same error split as `proxy_metadata`.
    pub async fn proxy_jobs(
        &self,
        method: reqwest::Method,
        path: &str,
        query: Option<&str>,
    ) -> anyhow::Result<reqwest::Response> {
        self.proxy(method, path, query, None, Duration::from_secs(10))
            .await
    }

    /// Forwards to a job-events SSE stream. Same error split as `proxy_metadata`.
    pub async fn proxy_job_events(
        &self,
        path: &str,
        query: Option<&str>,
    ) -> anyhow::Result<reqwest::Response> {
        Ok(self
            .authorize(self.http_stream.get(self.url(path, query)))
            .send()
            .await?)
    }

    /// Forwards to komf's OAuth API (`/api/oauth`). Same error split as
    /// `proxy_metadata`. komf builds the OAuth callback URL from the request's host
    /// headers, so they are copied from the incoming browser request: komf's own
    /// instance address is internal and must not end up in the redirect the browser
    /// follows.
    pub async fn proxy_oauth(
        &self,
        method: reqwest::Method,
        path: &str,
        query: Option<&str>,
        headers: &axum::http::HeaderMap,
        timeout: Duration,
    ) -> anyhow::Result<reqwest::Response> {
        let url = self.url(path, query);
        let mut request = self
            .authorize(self.http_no_redirect.request(method, url))
            .timeout(timeout);
        for name in ["host", "x-forwarded-host", "x-forwarded-proto"] {
            if let Some(value) = headers.get(name) {
                request = request.header(name, value.clone());
            }
        }
        Ok(request.send().await?)
    }

    async fn proxy(
        &self,
        method: reqwest::Method,
        path: &str,
        query: Option<&str>,
        body: Option<&serde_json::Value>,
        timeout: Duration,
    ) -> anyhow::Result<reqwest::Response> {
        let url = self.url(path, query);
        let mut request = self
            .authorize(self.http.request(method, url))
            .timeout(timeout);
        if let Some(body) = body {
            request = request.json(body);
        }
        Ok(request.send().await?)
    }
}

/// The komga section of komf's `GET /api/config`; field names follow komf's DTO.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KomfKomgaConfig {
    pub base_uri: Option<String>,
}

/// A client for the integration's komf, presenting the effective auth key: the
/// per-integration override from the kmrs.sqlite row wins over the `komf.auth-key`
/// config preset, so every caller (provisioner, proxies, health probes) uses the same
/// credential.
pub fn client(state: &AppState, row: &KomfIntegration) -> KomfClient {
    let auth_key = row
        .auth_key
        .as_deref()
        .or(state.config.komf_auth_key.as_deref());
    KomfClient::new(&row.url, auth_key)
}

/// komf-rs's auth gate answers 401 when the presented key is missing or wrong; without
/// a hint that surfaces as a bare HTTP status while the health probe keeps reporting
/// the login page's 200 as "reachable".
fn with_auth_hint<T>(result: anyhow::Result<T>) -> anyhow::Result<T> {
    match result {
        Ok(v) => Ok(v),
        Err(e) => {
            let unauthorized = e
                .chain()
                .find_map(|cause| cause.downcast_ref::<reqwest::Error>())
                .and_then(|e| e.status())
                .is_some_and(|s| s == reqwest::StatusCode::UNAUTHORIZED);
            Err(if unauthorized {
                e.context(
                    "komf answered 401; check the integration's auth key (per-integration override when set, else komf.auth-key / KOMGA_KOMF_AUTHKEY) against komf's KOMF_AUTH_KEY",
                )
            } else {
                e
            })
        }
    }
}

#[derive(serde::Deserialize)]
struct KomfConfigDto {
    komga: KomfKomgaConfig,
}

/// Mint a fresh key and push it with the Komga base URL to komf. On success the
/// integration row moves to `connected`; on failure to `error`, for the background
/// reconciliation to retry.
pub async fn provision(state: &AppState, owner_user_id: &str) -> anyhow::Result<()> {
    let _guard = integration_lock().lock().await;
    let dao = KomfIntegrationDao::new(state.kmrs_db.clone());
    let Some(row) = dao.get()? else {
        bail!("komf integration is not configured");
    };
    match try_provision(state, &row, owner_user_id).await {
        Ok(api_key_id) => {
            dao.mark_connected(owner_user_id, &api_key_id)?;
            tracing::info!("komf integration connected to {}", row.url);
            Ok(())
        }
        Err(e) => {
            if let Err(db_err) = dao.mark_error(&format!("{e:#}")) {
                tracing::warn!("failed to record komf integration error: {db_err}");
            }
            Err(e)
        }
    }
}

async fn try_provision(
    state: &AppState,
    row: &KomfIntegration,
    owner_user_id: &str,
) -> anyhow::Result<String> {
    let user_dao = UserDao::new(state.db.clone());
    // reconnects revoke the previous key so keys don't accumulate on the owner account
    if let (Some(key_id), Some(owner)) = (&row.api_key_id, &row.owner_user_id) {
        if let Err(e) = user_dao.delete_api_key_by_id_and_user_id(key_id, owner) {
            tracing::warn!("failed to revoke previous komf API key: {e}");
        }
    }
    let (api_key, plain) = mint_api_key(state.db.clone(), owner_user_id, API_KEY_COMMENT)?;
    let client = client(state, row);
    let body = serde_json::json!({
        "komga": {
            "baseUri": row.base_url,
            "komgaApiKey": plain,
            "eventListener": { "enabled": true },
        }
    });
    if let Err(e) = with_auth_hint(client.patch_config(&body).await) {
        // the minted key is useless while komf doesn't know it; don't leave it behind
        let _ = user_dao.delete_api_key_by_id_and_user_id(&api_key.id, owner_user_id);
        return Err(e).context("patch komf config");
    }
    Ok(api_key.id)
}

/// Best-effort teardown: komf is told to drop the key and stop listening, but an
/// unreachable komf never blocks the local cleanup.
pub async fn disconnect(state: &AppState) -> anyhow::Result<()> {
    let _guard = integration_lock().lock().await;
    let dao = KomfIntegrationDao::new(state.kmrs_db.clone());
    let Some(row) = dao.get()? else {
        return Ok(());
    };
    if let Err(e) = client(state, &row)
        .patch_config(&serde_json::json!({
            "komga": { "komgaApiKey": "", "eventListener": { "enabled": false } }
        }))
        .await
    {
        tracing::warn!("failed to clear komf config at {}: {e:#}", row.url);
    }
    if let (Some(key_id), Some(owner)) = (&row.api_key_id, &row.owner_user_id) {
        UserDao::new(state.db.clone()).delete_api_key_by_id_and_user_id(key_id, owner)?;
    }
    dao.delete()?;
    Ok(())
}

pub struct KomfProvisioner;

impl KomfProvisioner {
    /// Periodic reconciliation: first pass on startup, then every minute. Spawned
    /// unconditionally; no-ops until the integration is configured.
    pub fn start(state: AppState) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                // interval's first tick completes immediately
                interval.tick().await;
                if let Err(e) = reconcile_once(&state).await {
                    tracing::warn!("komf reconciliation failed: {e:#}");
                }
            }
        })
    }
}

async fn reconcile_once(state: &AppState) -> anyhow::Result<()> {
    let dao = KomfIntegrationDao::new(state.kmrs_db.clone());
    let Some(row) = dao.get()? else {
        return Ok(());
    };
    match row.state {
        KomfIntegrationState::Pending | KomfIntegrationState::Error => {
            // a row that was never provisioned has no owner yet; the next PUT picks one
            // and provisions inline
            if let Some(owner) = &row.owner_user_id {
                provision(state, owner).await?;
            }
        }
        KomfIntegrationState::Connected => {
            let komga = with_auth_hint(client(state, &row).get_config().await)?;
            if komga.base_uri.as_deref() != Some(row.base_url.as_str()) {
                tracing::info!("komf baseUri drifted from the integration row, re-provisioning");
                if let Some(owner) = &row.owner_user_id {
                    provision(state, owner).await?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::{insert_user, test_config, TestApp};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::{Json, Router};

    use std::sync::{Arc, Mutex};

    fn test_state() -> AppState {
        TestApp::new(Router::new()).state
    }

    fn test_state_with_auth_key(key: &str) -> AppState {
        let mut config = test_config();
        config.komf_auth_key = Some(key.to_string());
        TestApp::with_config(Router::new(), config).state
    }

    struct MockKomf {
        url: String,
        patches: Arc<Mutex<Vec<serde_json::Value>>>,
        base_uri: Arc<Mutex<Option<String>>>,
        authorization: Arc<Mutex<Vec<Option<String>>>>,
        get_status: Arc<Mutex<StatusCode>>,
    }

    async fn serve_komf(patch_status: StatusCode) -> MockKomf {
        let patches = Arc::new(Mutex::new(vec![]));
        let base_uri = Arc::new(Mutex::new(None));
        let authorization = Arc::new(Mutex::new(vec![]));
        let get_status = Arc::new(Mutex::new(StatusCode::OK));
        let app = {
            let patches = patches.clone();
            let base_uri = base_uri.clone();
            let authorization = authorization.clone();
            let get_status = get_status.clone();
            Router::new().route("/", get(|| async { "komf-rs" })).route(
                "/api/config",
                get(move || {
                    let base_uri = base_uri.clone();
                    let get_status = get_status.clone();
                    async move {
                        let status = *get_status.lock().unwrap();
                        if status == StatusCode::OK {
                            Json(serde_json::json!({
                                "komga": { "baseUri": base_uri.lock().unwrap().clone() }
                            }))
                            .into_response()
                        } else {
                            status.into_response()
                        }
                    }
                })
                .patch(
                    move |headers: axum::http::HeaderMap, Json(body): Json<serde_json::Value>| {
                        let patches = patches.clone();
                        let authorization = authorization.clone();
                        async move {
                            authorization.lock().unwrap().push(
                                headers
                                    .get(axum::http::header::AUTHORIZATION)
                                    .and_then(|v| v.to_str().ok())
                                    .map(str::to_string),
                            );
                            patches.lock().unwrap().push(body);
                            patch_status
                        }
                    },
                ),
            )
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        MockKomf {
            url: format!("http://{addr}"),
            patches,
            base_uri,
            authorization,
            get_status,
        }
    }
    fn seed_integration(state: &AppState, url: &str) {
        seed_integration_with_key(state, url, None);
    }

    fn seed_integration_with_key(state: &AppState, url: &str, auth_key: Option<&str>) {
        KomfIntegrationDao::new(state.kmrs_db.clone())
            .upsert(url, "http://kmrs:25600", auth_key)
            .unwrap();
    }

    fn integration_row(state: &AppState) -> KomfIntegration {
        KomfIntegrationDao::new(state.kmrs_db.clone())
            .get()
            .unwrap()
            .unwrap()
    }

    fn owner_keys(state: &AppState, owner: &str) -> Vec<komga_core::model::user::ApiKey> {
        UserDao::new(state.db.clone())
            .find_api_keys_by_user_id(owner)
            .unwrap()
    }

    #[tokio::test]
    async fn provision_patches_config_and_marks_connected() {
        let state = test_state();
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);

        provision(&state, &admin).await.unwrap();

        let patches = komf.patches.lock().unwrap();
        assert_eq!(patches.len(), 1);
        let komga = &patches[0]["komga"];
        assert_eq!(komga["baseUri"], "http://kmrs:25600");
        assert_eq!(komga["eventListener"]["enabled"], true);
        let plain = komga["komgaApiKey"].as_str().unwrap().to_string();
        drop(patches);

        let row = integration_row(&state);
        assert_eq!(row.state, KomfIntegrationState::Connected);
        assert_eq!(row.owner_user_id.as_deref(), Some(admin.as_str()));
        assert_eq!(row.last_error, None);

        // the key komf received is the one stored (as SHA-512) under the owner
        let keys = owner_keys(&state, &admin);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, row.api_key_id.unwrap());
        assert_eq!(keys[0].comment, API_KEY_COMMENT);
        assert_eq!(keys[0].key, crate::auth::sha512_hex(&plain));
    }

    #[tokio::test]
    async fn provision_sends_bearer_token_when_auth_key_configured() {
        use base64::Engine as _;
        let state = test_state_with_auth_key("s3cret");
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);

        provision(&state, &admin).await.unwrap();

        let authorization = komf.authorization.lock().unwrap();
        let expected = format!(
            "Bearer {}",
            base64::engine::general_purpose::STANDARD.encode("s3cret")
        );
        assert_eq!(authorization.as_slice(), [Some(expected)]);
    }

    #[tokio::test]
    async fn provision_sends_no_authorization_header_without_auth_key() {
        let state = test_state();
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);

        provision(&state, &admin).await.unwrap();

        assert_eq!(komf.authorization.lock().unwrap().as_slice(), [None]);
    }

    #[tokio::test]
    async fn row_auth_key_overrides_the_config_preset() {
        use base64::Engine as _;
        let state = test_state_with_auth_key("config-key");
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration_with_key(&state, &komf.url, Some("row-key"));

        provision(&state, &admin).await.unwrap();

        let expected = format!(
            "Bearer {}",
            base64::engine::general_purpose::STANDARD.encode("row-key")
        );
        assert_eq!(
            komf.authorization.lock().unwrap().as_slice(),
            [Some(expected)]
        );
    }

    #[tokio::test]
    async fn config_preset_applies_when_the_row_has_no_override() {
        use base64::Engine as _;
        let state = test_state_with_auth_key("config-key");
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);

        provision(&state, &admin).await.unwrap();

        let expected = format!(
            "Bearer {}",
            base64::engine::general_purpose::STANDARD.encode("config-key")
        );
        assert_eq!(
            komf.authorization.lock().unwrap().as_slice(),
            [Some(expected)]
        );
    }

    #[tokio::test]
    async fn unauthorized_patch_hints_at_auth_key_mismatch() {
        let state = test_state_with_auth_key("s3cret");
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::UNAUTHORIZED).await;
        seed_integration(&state, &komf.url);

        assert!(provision(&state, &admin).await.is_err());

        let row = integration_row(&state);
        assert_eq!(row.state, KomfIntegrationState::Error);
        let error = row.last_error.unwrap();
        assert!(error.contains("401"), "unexpected error: {error}");
        assert!(error.contains("KOMF_AUTH_KEY"), "unexpected error: {error}");
        assert!(owner_keys(&state, &admin).is_empty());
    }

    #[tokio::test]
    async fn failed_patch_marks_error_and_drops_the_minted_key() {
        let state = test_state();
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::UNPROCESSABLE_ENTITY).await;
        seed_integration(&state, &komf.url);

        assert!(provision(&state, &admin).await.is_err());

        let row = integration_row(&state);
        assert_eq!(row.state, KomfIntegrationState::Error);
        assert!(row.last_error.is_some());
        assert!(owner_keys(&state, &admin).is_empty());
    }

    #[tokio::test]
    async fn reprovision_revokes_the_previous_key() {
        let state = test_state();
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);
        provision(&state, &admin).await.unwrap();
        let first_key_id = integration_row(&state).api_key_id.unwrap();

        seed_integration(&state, &komf.url);
        provision(&state, &admin).await.unwrap();

        let keys = owner_keys(&state, &admin);
        assert_eq!(keys.len(), 1);
        assert_ne!(keys[0].id, first_key_id);
    }

    #[tokio::test]
    async fn disconnect_clears_komf_revokes_key_and_removes_row() {
        let state = test_state();
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);
        provision(&state, &admin).await.unwrap();

        disconnect(&state).await.unwrap();

        let patches = komf.patches.lock().unwrap();
        let last = patches.last().unwrap();
        assert_eq!(last["komga"]["komgaApiKey"], "");
        assert_eq!(last["komga"]["eventListener"]["enabled"], false);
        drop(patches);
        assert!(KomfIntegrationDao::new(state.kmrs_db.clone())
            .get()
            .unwrap()
            .is_none());
        assert!(owner_keys(&state, &admin).is_empty());
    }

    #[tokio::test]
    async fn disconnect_tolerates_unreachable_komf() {
        let state = test_state();
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);
        provision(&state, &admin).await.unwrap();

        // komf is gone (nothing listens on port 1); local cleanup must still succeed
        seed_integration(&state, "http://127.0.0.1:1");
        disconnect(&state).await.unwrap();

        assert!(KomfIntegrationDao::new(state.kmrs_db.clone())
            .get()
            .unwrap()
            .is_none());
        assert!(owner_keys(&state, &admin).is_empty());
    }

    #[tokio::test]
    async fn connected_drift_triggers_reprovision() {
        let state = test_state();
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);
        provision(&state, &admin).await.unwrap();
        *komf.base_uri.lock().unwrap() = Some("http://elsewhere:9999".to_string());

        reconcile_once(&state).await.unwrap();

        assert_eq!(komf.patches.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn connected_reconcile_hints_at_auth_key_mismatch() {
        let state = test_state();
        let admin = insert_user(&state.db, "admin@x.c", true, true, &[]);
        let komf = serve_komf(StatusCode::NO_CONTENT).await;
        seed_integration(&state, &komf.url);
        provision(&state, &admin).await.unwrap();
        *komf.get_status.lock().unwrap() = StatusCode::UNAUTHORIZED;

        let error = reconcile_once(&state).await.unwrap_err();

        let error = format!("{error:#}");
        assert!(error.contains("401"), "unexpected error: {error}");
        assert!(error.contains("KOMF_AUTH_KEY"), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn reconcile_noops_when_unconfigured_or_unowned() {
        let state = test_state();
        reconcile_once(&state).await.unwrap();

        // a row that was never provisioned waits for the next PUT to pick an owner
        seed_integration(&state, "http://127.0.0.1:1");
        reconcile_once(&state).await.unwrap();
        assert_eq!(integration_row(&state).state, KomfIntegrationState::Pending);
    }
}
