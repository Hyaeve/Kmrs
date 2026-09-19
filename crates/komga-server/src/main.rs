mod api;
mod auth;
mod config;
mod dto;
mod error;
mod events;
mod http;
mod search_index;
mod service;
mod settings;
mod sse;
mod state;
#[allow(dead_code)]
mod webpub;

use anyhow::Context;
use komga_db::pool::Database;
use komga_db::{Migrator, Placeholders};
use state::AppState;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = config::ServerConfig::from_env();
    std::fs::create_dir_all(&config.config_dir).context("create config dir")?;

    let db = Database::open(&config.database).context("open main database")?;
    let tasks_db = Database::open(&config.tasks_db).context("open tasks database")?;

    {
        let migrations = komga_db::main_migrations();
        let applied = Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .context("main db migration")?;
        if applied > 0 {
            tracing::info!("applied {applied} main db migrations");
        }
        let tasks_migrations = komga_db::tasks_migrations();
        Migrator::new(&tasks_migrations, Placeholders::default())
            .migrate(&tasks_db.rw())
            .context("tasks db migration")?;
    }

    let task_notify: service::TaskNotify = std::sync::Arc::new(tokio::sync::Notify::new());
    let search_index =
        Arc::new(komga_search::SearchIndex::open(&config.lucene_dir).context("open search index")?);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let state = AppState {
        sessions: auth::SessionStore::new(config.session_timeout),
        settings: Arc::new(settings::SettingsProvider::load(db.clone())),
        tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
        events: events::event_bus(),
        task_emitter: Arc::new(service::TaskEmitter::new(
            db.clone(),
            tasks_db.clone(),
            task_notify.clone(),
        )),
        search_index: search_index.clone(),
        shutdown_tx,
        db,
        tasks_db,
        config: Arc::new(config.clone()),
    };

    service::processor::TaskProcessor::start(state.clone(), task_notify);
    service::scheduler::ScanScheduler::start(state.clone());
    search_index::check_on_startup(&state);
    search_index::consume_events(state.clone());

    let app = build_router(state.clone());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("komga-server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(shutdown_rx))
    .await?;
    Ok(())
}

pub fn build_router(state: AppState) -> axum::Router {
    let routes = axum::Router::new()
        .merge(api::claim::router())
        .merge(api::login::router())
        .merge(api::users::router())
        .merge(api::libraries::router())
        .merge(api::referential::router())
        .merge(api::series::router())
        .merge(api::books::router())
        .merge(api::collections::router())
        .merge(api::readlists::router())
        .merge(api::tasks::router())
        .merge(api::opds_v1::router())
        .merge(api::opds_v2::router())
        .merge(api::openapi::router())
        .merge(api::kobo::router())
        .merge(api::koreader::router())
        .merge(api::syncpoints::router())
        .merge(api::page_hashes::router())
        .merge(api::transient_books::router())
        .merge(api::settings::router())
        .merge(api::client_settings::router())
        .merge(api::history::router())
        .merge(api::announcements::router())
        .merge(api::releases::router())
        .merge(api::filesystem::router())
        .merge(api::fonts::router())
        .merge(api::actuator::router())
        .merge(sse::router());

    routes
        .layer(axum::middleware::from_fn(
            http::error_path::error_path_middleware,
        ))
        .layer(axum::middleware::from_fn(http::etag::etag_middleware))
        .layer(axum::middleware::from_fn(
            http::cache::cache_control_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn shutdown_signal(mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = shutdown_rx.changed() => {},
    }
    tracing::info!("shutting down");
}
