//! Application state.

use crate::auth::SessionStore;
use crate::config::ServerConfig;
use crate::settings::SettingsProvider;
use komga_core::tsid::TsidFactory;
use komga_db::pool::Database;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub db: Database,
    /// Dedicated connection pools over the same `database.sqlite` file, used only
    /// by background task execution so it never contends with API connections.
    pub task_db: Database,
    pub tasks_db: Database,
    pub sessions: SessionStore,
    pub settings: Arc<SettingsProvider>,
    pub tsid: Arc<TsidFactory>,
    pub events: crate::events::EventBus,
    pub task_emitter: Arc<crate::service::TaskEmitter>,
    pub search_index: Arc<komga_search::SearchIndex>,
    pub kepub: Arc<crate::service::kepub::KepubConverter>,
    pub kobo_proxy: Arc<crate::service::kobo_proxy::KoboProxy>,
    pub webui_dir: crate::webui::WebuiDir,
    /// Broadcasts the shutdown request (actuator `/actuator/shutdown`)
    pub shutdown_tx: tokio::sync::watch::Sender<bool>,
}

/// A throwaway tantivy index for tests (one fresh directory per call).
#[cfg(test)]
pub(crate) fn test_search_index() -> Arc<komga_search::SearchIndex> {
    Arc::new(
        komga_search::SearchIndex::open(&tempfile::tempdir().unwrap().keep())
            .expect("test search index"),
    )
}

impl AppState {
    /// A clone of the state whose `db` points at the dedicated task pools.
    /// Background task execution (scan / analyze / hash / convert / maintenance)
    /// runs against this context, so its reads and writes never share pool slots
    /// with HTTP/API requests. The task emitter is re-pointed at the task pools
    /// as well — its follow-up candidate queries (e.g. `analyze_unknown_and_outdated_books`,
    /// `hash_books_without_hash`) are the heaviest scan-time reads. The task queue
    /// (`tasks_db`), event bus and settings are shared unchanged.
    pub fn task_context(&self) -> Self {
        Self {
            db: self.task_db.clone(),
            task_emitter: Arc::new(self.task_emitter.with_db(self.task_db.clone())),
            ..self.clone()
        }
    }

    /// Records authentication activity (success/failure); persisted asynchronously without blocking the request.
    pub async fn record_activity(
        &self,
        activity: &Option<crate::auth::ActivityDraft>,
        parts: &axum::http::request::Parts,
    ) {
        let Some(draft) = activity else { return };
        let db = self.db.clone();
        let ip = client_ip(parts);
        let user_agent = parts
            .headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let draft = crate::auth::ActivityDraft {
            user_id: draft.user_id.clone(),
            email: draft.email.clone(),
            api_key_id: draft.api_key_id.clone(),
            api_key_comment: draft.api_key_comment.clone(),
            success: draft.success,
            error: draft.error.clone(),
            source: draft.source.clone(),
        };
        tokio::task::spawn_blocking(move || {
            let activity = komga_core::model::user::AuthenticationActivity {
                user_id: draft.user_id,
                email: draft.email,
                api_key_id: draft.api_key_id,
                api_key_comment: draft.api_key_comment,
                ip,
                user_agent,
                success: draft.success,
                error: draft.error,
                date_time: komga_core::time_codec::now_utc(),
                source: Some(draft.source),
            };
            if let Err(e) = komga_db::dao::user::UserDao::new(db).insert_activity(&activity) {
                tracing::warn!("failed to record authentication activity: {e}");
            }
        });
    }
}

/// `forward-headers-strategy: framework`: prefers the first hop of X-Forwarded-For.
fn client_ip(parts: &axum::http::request::Parts) -> Option<String> {
    parts
        .headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| ci.0.ip().to_string())
        })
}
