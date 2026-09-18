mod api;
mod auth;
mod config;
mod dto;
mod error;
mod http;
mod settings;
mod state;

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

    let state = AppState {
        sessions: auth::SessionStore::new(config.session_timeout),
        settings: Arc::new(settings::SettingsProvider::load(db.clone())),
        tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
        db,
        tasks_db,
        config: Arc::new(config.clone()),
    };

    let app = build_router(state.clone());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("komga-server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

pub fn build_router(state: AppState) -> axum::Router {
    let routes = axum::Router::new()
        .merge(api::claim::router())
        .merge(api::login::router())
        .merge(api::users::router());

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

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
