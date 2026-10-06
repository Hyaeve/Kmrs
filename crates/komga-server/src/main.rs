mod api;
mod auth;
mod config;
mod dto;
mod error;
mod events;
mod http;
mod logging;
#[cfg(all(feature = "profiling", unix))]
mod profiling;
mod search_index;
mod service;
mod settings;
mod sse;
mod state;
mod thumbnails;
mod webhook;
#[allow(dead_code)]
mod webpub;
mod webui;
mod zip_archive;

use anyhow::Context;
use clap::Parser;
use komga_db::pool::Database;
use komga_db::Migrator;
use state::AppState;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

/// SQLite and other C code allocate through glibc malloc, whose per-thread
/// arenas (8×cores by default) only ever return the heap top to the OS:
/// transient allocations during scans and queries linger as touched-but-free
/// pages scattered across dozens of heaps. Two arenas bound that retention;
/// MALLOC_ARENA_MAX remains the override channel.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn cap_glibc_arenas() {
    if std::env::var_os("MALLOC_ARENA_MAX").is_none() {
        unsafe { libc::mallopt(libc::M_ARENA_MAX, 2) };
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn cap_glibc_arenas() {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cap_glibc_arenas();
    let cli = config::Cli::parse();
    // Start time must be recorded now, not lazily on the first metrics request,
    // or process.uptime starts counting from that request.
    service::metrics::process_start();
    let _log_guard = logging::init(&config::config_dir(&cli).join("logs"));

    #[cfg(all(feature = "profiling", unix))]
    profiling::spawn_heap_dump_listener();

    let config = config::ServerConfig::load(&cli)?;
    std::fs::create_dir_all(&config.config_dir).context("create config dir")?;

    // Sort locale must be set before any collator is constructed (lazily on the
    // first connection collation registration); later calls are ignored.
    komga_core::sort_locale::set_sort_locale(config.sort_locale.clone());

    let db = Database::open(&config.database).context("open main database")?;
    // Dedicated pools over the same file for background task execution; task
    // reads/writes never share pool slots with HTTP/API requests. The task side,
    // tasks queue and kmrs database are low-concurrency auxiliary pools.
    let task_db =
        Database::open(&config.database.aux_pools()).context("open task database pools")?;
    let tasks_db = Database::open(&config.tasks_db.aux_pools()).context("open tasks database")?;
    let kmrs_db = Database::open(&config.kmrs_db.aux_pools()).context("open kmrs database")?;

    {
        let migrations = komga_db::main_migrations();
        let applied = Migrator::new(&migrations, config.migration_placeholders.clone())
            .migrate(&*db.rw()?)
            .context("main db migration")?;
        if applied > 0 {
            tracing::info!("applied {applied} main db migrations");
        }
        let tasks_migrations = komga_db::tasks_migrations();
        Migrator::new(&tasks_migrations, config.migration_placeholders.clone())
            .migrate(&*tasks_db.rw()?)
            .context("tasks db migration")?;
        let kmrs_migrations = komga_db::kmrs_migrations();
        Migrator::new(&kmrs_migrations, config.migration_placeholders.clone())
            .migrate(&*kmrs_db.rw()?)
            .context("kmrs db migration")?;
    }

    let task_notify: service::TaskNotify = std::sync::Arc::new(tokio::sync::Notify::new());
    let search_rebuild = match komga_search::decide_startup(&config.lucene_dir) {
        komga_search::StartupDecision::Ready => false,
        komga_search::StartupDecision::Rebuild { wipe } => {
            if wipe {
                tracing::info!(
                    "wiping outdated search index at {}",
                    config.lucene_dir.display()
                );
                komga_search::wipe_index_dir(&config.lucene_dir).context("wipe search index")?;
            }
            true
        }
    };
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
        kepub: service::kepub::KepubConverter::new(service::kepub::default_tmp_dir()),
        kobo_proxy: service::kobo_proxy::KoboProxy::new(),
        webui_dir: webui::WebuiDir::new(service::webui_updater::initial_dir(&config)),
        shutdown_tx,
        db,
        task_db,
        tasks_db,
        kmrs_db,
        config: Arc::new(config.clone()),
    };

    service::processor::TaskProcessor::start(state.clone(), task_notify);
    service::scheduler::ScanScheduler::start(state.clone());
    service::maintenance::MaintenanceScheduler::start_auth_activity_cleanup(state.clone());
    service::maintenance::MaintenanceScheduler::start_history_cleanup(state.clone());
    // Thumbnail file storage: migrate existing blobs to files in the background. The
    // orphan sweep starts only after the migration — freshly written files are
    // referenced once their row is updated, and a concurrent sweep would delete them.
    let thumbnail_migration = if config.thumbnail_storage == config::ThumbnailStorage::File {
        let state = state.clone();
        Some(tokio::task::spawn_blocking(move || {
            thumbnails::migrate_blobs_to_files(&state)
        }))
    } else {
        None
    };
    let sweep_state = state.clone();
    tokio::spawn(async move {
        if let Some(migration) = thumbnail_migration {
            if let Err(e) = migration.await {
                tracing::error!("thumbnail file-storage migration task failed: {e}");
            }
        }
        service::maintenance::MaintenanceScheduler::start_thumbnail_sweep(sweep_state);
    });
    if config.webui_auto_update && config.webui_dir.is_some() {
        service::webui_updater::WebuiUpdater::start(state.clone());
    }
    service::komf::KomfProvisioner::start(state.clone());
    search_index::check_on_startup(&state, search_rebuild);
    search_index::consume_events(state.clone());
    webhook::consume_events(state.clone());
    service::reading_stats::consume_events(state.clone());

    let app = build_router(state.clone());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("kmrs listening on {addr}");
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
        .merge(api::oauth2::router())
        .merge(api::users::router())
        .merge(api::libraries::router())
        .merge(api::referential::router())
        .merge(api::series::router())
        .merge(api::books::router())
        .merge(api::collections::router())
        .merge(api::readlists::router())
        .merge(api::smartlists::router())
        .merge(api::tasks::router())
        .merge(api::komf::router())
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
        .merge(api::stats::router())
        .merge(api::announcements::router())
        .merge(api::releases::router())
        .merge(api::filesystem::router())
        .merge(api::fonts::router())
        .merge(api::actuator::router())
        .merge(sse::router());

    #[cfg(all(feature = "profiling", unix))]
    let routes = routes.merge(profiling::router());

    let app = routes
        .fallback(webui::fallback)
        .layer(axum::middleware::from_fn(
            http::error_path::error_path_middleware,
        ))
        .layer(axum::middleware::from_fn(http::etag::etag_middleware))
        .layer(axum::middleware::from_fn(
            http::cache::cache_control_middleware,
        ))
        .layer(axum::middleware::from_fn(
            http::www_authenticate::strip_challenge_for_xhr,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(http::offload::offload_middleware));

    // outermost: preflight OPTIONS must be answered before the auth middleware runs
    let app = match http::cors::layer(&state.config.cors_allowed_origins) {
        Some(cors) => app.layer(cors),
        None => app,
    };

    app.with_state(state)
}

async fn shutdown_signal(mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = shutdown_rx.changed() => {},
    }
    tracing::info!("shutting down");
}
