//! komf integration: provisions a komf metadata fetcher with a minted API key and the
//! Komga base URL through komf's `PATCH /api/config` (hot-reloaded, komf needs no
//! changes), then keeps it reconciled. komf calls back in Komga mode (REST + SSE).
//! komf's own API has no authentication, so the integration assumes a trusted network.

use crate::service::user::mint_api_key;
use crate::state::AppState;
use anyhow::{bail, Context};
use komga_db::dao::komf_integration::{KomfIntegration, KomfIntegrationDao, KomfIntegrationState};
use komga_db::dao::user::UserDao;
use std::time::Duration;

pub const API_KEY_COMMENT: &str = "Integration · komf";

/// Search crawls external providers, so proxied metadata calls get a more generous
/// budget than the client default.
const METADATA_PROXY_TIMEOUT: Duration = Duration::from_secs(30);

/// Serializes provision/disconnect so a manual reconfigure racing the reconciliation
/// loop cannot mint two keys and orphan one of them.
fn integration_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[derive(Clone)]
pub struct KomfClient {
    base_url: String,
    http: reqwest::Client,
    // a total timeout spans the response body, so the 10s budget of `http` would cut
    // long-lived SSE streams; this client carries only a connect timeout
    http_stream: reqwest::Client,
}

impl KomfClient {
    pub fn new(base_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kmrs/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        let http_stream = reqwest::Client::builder()
            .user_agent(concat!("kmrs/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            http_stream,
        }
    }

    /// komf serves a plain 200 at its root; used for the live reachability probe.
    pub async fn health(&self) -> anyhow::Result<()> {
        self.http
            .get(&self.base_url)
            .timeout(Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get_config(&self) -> anyhow::Result<KomfKomgaConfig> {
        let config: KomfConfigDto = self
            .http
            .get(format!("{}/api/config", self.base_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(config.komga)
    }

    pub async fn patch_config(&self, body: &serde_json::Value) -> anyhow::Result<()> {
        self.http
            .patch(format!("{}/api/config", self.base_url))
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

    /// Forwards to a job's SSE event stream. Same error split as `proxy_metadata`.
    pub async fn proxy_job_events(&self, path: &str) -> anyhow::Result<reqwest::Response> {
        Ok(self
            .http_stream
            .get(format!("{}{path}", self.base_url))
            .send()
            .await?)
    }

    async fn proxy(
        &self,
        method: reqwest::Method,
        path: &str,
        query: Option<&str>,
        body: Option<&serde_json::Value>,
        timeout: Duration,
    ) -> anyhow::Result<reqwest::Response> {
        let mut url = format!("{}{path}", self.base_url);
        if let Some(query) = query.filter(|q| !q.is_empty()) {
            url.push('?');
            url.push_str(query);
        }
        let mut request = self.http.request(method, url).timeout(timeout);
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
    let client = KomfClient::new(&row.url);
    let body = serde_json::json!({
        "komga": {
            "baseUri": row.base_url,
            "komgaApiKey": plain,
            "eventListener": { "enabled": true },
        }
    });
    if let Err(e) = client.patch_config(&body).await {
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
    if let Err(e) = KomfClient::new(&row.url)
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
            let komga = KomfClient::new(&row.url).get_config().await?;
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
    use crate::api::libraries::test_support::{insert_user, TestApp};
    use axum::http::StatusCode;
    use axum::routing::get;
    use axum::{Json, Router};
    use std::sync::{Arc, Mutex};

    fn test_state() -> AppState {
        TestApp::new(Router::new()).state
    }

    struct MockKomf {
        url: String,
        patches: Arc<Mutex<Vec<serde_json::Value>>>,
        base_uri: Arc<Mutex<Option<String>>>,
    }

    async fn serve_komf(patch_status: StatusCode) -> MockKomf {
        let patches = Arc::new(Mutex::new(vec![]));
        let base_uri = Arc::new(Mutex::new(None));
        let app = {
            let patches = patches.clone();
            let base_uri = base_uri.clone();
            Router::new().route("/", get(|| async { "komf-rs" })).route(
                "/api/config",
                get(move || {
                    let base_uri = base_uri.clone();
                    async move {
                        Json(serde_json::json!({
                            "komga": { "baseUri": base_uri.lock().unwrap().clone() }
                        }))
                    }
                })
                .patch(move |Json(body): Json<serde_json::Value>| {
                    let patches = patches.clone();
                    async move {
                        patches.lock().unwrap().push(body);
                        patch_status
                    }
                }),
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
        }
    }

    fn seed_integration(state: &AppState, url: &str) {
        KomfIntegrationDao::new(state.kmrs_db.clone())
            .upsert(url, "http://kmrs:25600")
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
    async fn reconcile_noops_when_unconfigured_or_unowned() {
        let state = test_state();
        reconcile_once(&state).await.unwrap();

        // a row that was never provisioned waits for the next PUT to pick an owner
        seed_integration(&state, "http://127.0.0.1:1");
        reconcile_once(&state).await.unwrap();
        assert_eq!(integration_row(&state).state, KomfIntegrationState::Pending);
    }
}
