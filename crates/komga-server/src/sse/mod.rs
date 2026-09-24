//! SSE event stream, ported from `interfaces/sse/SseController.kt`.

pub mod dto;

use crate::auth::RequireAuth;
use crate::events::DomainEvent;
#[cfg(test)]
use crate::state::test_search_index;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::response::Response;
use axum::{routing::get, Extension, Router};
use komga_core::model::user::KomgaUser;
use komga_db::dao::book::BookDao;
use komga_db::dao::tasks::TasksDao;
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

/// Per-connection timers. Kotlin schedules heartbeat and the task count globally for all
/// emitters; per-connection timers behave identically for each subscriber and keep no
/// shared state. Configurable so tests can speed them up.
#[derive(Debug, Clone, Copy)]
pub struct SseIntervals {
    pub heartbeat: Duration,
    pub task_status: Duration,
}

impl Default for SseIntervals {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(15),
            task_status: Duration::from_secs(10),
        }
    }
}

pub fn router() -> Router<AppState> {
    router_with_intervals(SseIntervals::default())
}

fn router_with_intervals(intervals: SseIntervals) -> Router<AppState> {
    Router::new()
        .route("/sse/v1/events", get(sse_events))
        .layer(Extension(intervals))
}

/// Who may receive an event (`emitSse`'s adminOnly/userIdOnly filters).
enum Scope {
    All,
    AdminOnly,
    UserOnly(String),
}

impl Scope {
    fn allows(&self, user: &KomgaUser) -> bool {
        match self {
            Scope::All => true,
            Scope::AdminOnly => user.is_admin(),
            Scope::UserOnly(id) => user.id == *id,
        }
    }
}

struct Outgoing {
    name: &'static str,
    data: serde_json::Value,
    scope: Scope,
}

/// One frame of the SSE wire format. Spring's `SseEventBuilder` writes fields without a
/// space after the colon (`data:{...}`, `event:Name`, `:comment`), matched here byte for byte.
enum Wire {
    Comment(&'static str),
    Event { name: &'static str, data: String },
}

impl Wire {
    fn encode(self) -> String {
        match self {
            Wire::Comment(comment) => format!(":{comment}\n\n"),
            Wire::Event { name, data } => format!("event:{name}\ndata:{data}\n\n"),
        }
    }
}

async fn sse_events(
    auth: RequireAuth,
    State(state): State<AppState>,
    Extension(intervals): Extension<SseIntervals>,
) -> Response {
    let user = auth.0.user;
    let (tx, rx) = mpsc::channel::<Wire>(64);

    tokio::spawn(async move {
        let mut events = state.events.subscribe();
        let mut heartbeat = tokio::time::interval(intervals.heartbeat);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut task_status = tokio::time::interval(intervals.task_status);
        task_status.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let wire = tokio::select! {
                _ = heartbeat.tick() => Wire::Comment("heartbeat"),
                _ = task_status.tick() => {
                    if !user.is_admin() {
                        continue;
                    }
                    match TasksDao::new(state.tasks_db.clone()).count_by_simple_type() {
                        Ok(counts) => {
                            let total = counts.values().sum();
                            let dto = dto::TaskQueueSseDto { count: total, count_by_type: counts };
                            Wire::Event { name: "TaskQueueStatus", data: serde_json::to_string(&dto).unwrap() }
                        }
                        Err(e) => {
                            tracing::warn!("could not count tasks for SSE: {e}");
                            continue;
                        }
                    }
                }
                event = events.recv() => {
                    match event {
                        Ok(domain) => match map_event(&state, &user, domain).await {
                            Some(outgoing) => Wire::Event { name: outgoing.name, data: outgoing.data.to_string() },
                            None => continue,
                        },
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("SSE receiver lagged by {n} events");
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            };
            if tx.send(wire).await.is_err() {
                break;
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(|wire| Ok::<_, Infallible>(wire.encode()));
    Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
        .header(axum::http::header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// `handleSseEvent`: maps a domain event to its SSE name/payload, applying the
/// adminOnly/userIdOnly filters. Returns None for events that are never sent
/// (`LibraryScanned`) or not meant for this user.
async fn map_event(state: &AppState, user: &KomgaUser, event: DomainEvent) -> Option<Outgoing> {
    macro_rules! out {
        ($name:expr, $dto:expr) => {
            Some(Outgoing {
                name: $name,
                data: serde_json::to_value($dto).unwrap(),
                scope: Scope::All,
            })
        };
        ($name:expr, $dto:expr, admin) => {
            Some(Outgoing {
                name: $name,
                data: serde_json::to_value($dto).unwrap(),
                scope: Scope::AdminOnly,
            })
        };
        ($name:expr, $dto:expr, user $id:expr) => {
            Some(Outgoing {
                name: $name,
                data: serde_json::to_value($dto).unwrap(),
                scope: Scope::UserOnly($id),
            })
        };
    }

    let outgoing = match event {
        DomainEvent::LibraryAdded(l) => {
            out!("LibraryAdded", dto::LibrarySseDto { library_id: l.id })
        }
        DomainEvent::LibraryUpdated(l) => {
            out!("LibraryChanged", dto::LibrarySseDto { library_id: l.id })
        }
        DomainEvent::LibraryDeleted(l) => {
            out!("LibraryDeleted", dto::LibrarySseDto { library_id: l.id })
        }
        DomainEvent::LibraryScanned(_) => return None,

        DomainEvent::SeriesAdded(s) => out!(
            "SeriesAdded",
            dto::SeriesSseDto {
                series_id: s.id,
                library_id: s.library_id
            }
        ),
        DomainEvent::SeriesUpdated(s) => out!(
            "SeriesChanged",
            dto::SeriesSseDto {
                series_id: s.id,
                library_id: s.library_id
            }
        ),
        DomainEvent::SeriesDeleted(s) => out!(
            "SeriesDeleted",
            dto::SeriesSseDto {
                series_id: s.id,
                library_id: s.library_id
            }
        ),

        DomainEvent::BookAdded(b) => out!(
            "BookAdded",
            dto::BookSseDto {
                book_id: b.id,
                series_id: b.series_id,
                library_id: b.library_id
            }
        ),
        DomainEvent::BookUpdated(b) => out!(
            "BookChanged",
            dto::BookSseDto {
                book_id: b.id,
                series_id: b.series_id,
                library_id: b.library_id
            }
        ),
        DomainEvent::BookDeleted(b) => out!(
            "BookDeleted",
            dto::BookSseDto {
                book_id: b.id,
                series_id: b.series_id,
                library_id: b.library_id
            }
        ),
        DomainEvent::BookImported {
            book,
            source_file,
            success,
            message,
        } => out!(
            "BookImported",
            dto::BookImportSseDto {
                book_id: book.map(|b| b.id),
                source_file,
                success,
                message,
            },
            admin
        ),

        DomainEvent::ReadListAdded(r) => out!(
            "ReadListAdded",
            dto::ReadListSseDto {
                read_list_id: r.id,
                book_ids: r.book_ids.into_values().collect()
            }
        ),
        DomainEvent::ReadListUpdated(r) => out!(
            "ReadListChanged",
            dto::ReadListSseDto {
                read_list_id: r.id,
                book_ids: r.book_ids.into_values().collect()
            }
        ),
        DomainEvent::ReadListDeleted(r) => out!(
            "ReadListDeleted",
            dto::ReadListSseDto {
                read_list_id: r.id,
                book_ids: r.book_ids.into_values().collect()
            }
        ),

        DomainEvent::CollectionAdded(c) => out!(
            "CollectionAdded",
            dto::CollectionSseDto {
                collection_id: c.id,
                series_ids: c.series_ids
            }
        ),
        DomainEvent::CollectionUpdated(c) => out!(
            "CollectionChanged",
            dto::CollectionSseDto {
                collection_id: c.id,
                series_ids: c.series_ids
            }
        ),
        DomainEvent::CollectionDeleted(c) => out!(
            "CollectionDeleted",
            dto::CollectionSseDto {
                collection_id: c.id,
                series_ids: c.series_ids
            }
        ),

        DomainEvent::ReadProgressChanged(p) => {
            out!("ReadProgressChanged", dto::ReadProgressSseDto { book_id: p.book_id, user_id: p.user_id.clone() }, user p.user_id)
        }
        DomainEvent::ReadProgressDeleted(p) => {
            out!("ReadProgressDeleted", dto::ReadProgressSseDto { book_id: p.book_id, user_id: p.user_id.clone() }, user p.user_id)
        }
        DomainEvent::ReadProgressSeriesChanged { series_id, user_id } => {
            out!("ReadProgressSeriesChanged", dto::ReadProgressSeriesSseDto { series_id, user_id: user_id.clone() }, user user_id)
        }
        DomainEvent::ReadProgressSeriesDeleted { series_id, user_id } => {
            out!("ReadProgressSeriesDeleted", dto::ReadProgressSeriesSseDto { series_id, user_id: user_id.clone() }, user user_id)
        }

        DomainEvent::ThumbnailBookAdded(t) => {
            let series_id = book_series_id(state, &t.book_id).await;
            out!(
                "ThumbnailBookAdded",
                dto::ThumbnailBookSseDto {
                    book_id: t.book_id,
                    series_id,
                    selected: t.selected
                }
            )
        }
        DomainEvent::ThumbnailBookDeleted(t) => {
            let series_id = book_series_id(state, &t.book_id).await;
            out!(
                "ThumbnailBookDeleted",
                dto::ThumbnailBookSseDto {
                    book_id: t.book_id,
                    series_id,
                    selected: t.selected
                }
            )
        }
        DomainEvent::ThumbnailSeriesAdded(t) => out!(
            "ThumbnailSeriesAdded",
            dto::ThumbnailSeriesSseDto {
                series_id: t.series_id,
                selected: t.selected
            }
        ),
        DomainEvent::ThumbnailSeriesDeleted(t) => out!(
            "ThumbnailSeriesDeleted",
            dto::ThumbnailSeriesSseDto {
                series_id: t.series_id,
                selected: t.selected
            }
        ),
        DomainEvent::ThumbnailSeriesCollectionAdded(t) => out!(
            "ThumbnailSeriesCollectionAdded",
            dto::ThumbnailSeriesCollectionSseDto {
                collection_id: t.collection_id,
                selected: t.selected
            }
        ),
        DomainEvent::ThumbnailSeriesCollectionDeleted(t) => out!(
            "ThumbnailSeriesCollectionDeleted",
            dto::ThumbnailSeriesCollectionSseDto {
                collection_id: t.collection_id,
                selected: t.selected
            }
        ),
        DomainEvent::ThumbnailReadListAdded(t) => out!(
            "ThumbnailReadListAdded",
            dto::ThumbnailReadListSseDto {
                read_list_id: t.read_list_id,
                selected: t.selected
            }
        ),
        DomainEvent::ThumbnailReadListDeleted(t) => out!(
            "ThumbnailReadListDeleted",
            dto::ThumbnailReadListSseDto {
                read_list_id: t.read_list_id,
                selected: t.selected
            }
        ),

        DomainEvent::UserUpdated {
            user: u,
            expire_session,
        } => {
            if !expire_session {
                return None;
            }
            out!("SessionExpired", dto::SessionExpiredDto { user_id: u.id.clone() }, user u.id)
        }
        DomainEvent::UserDeleted(u) => {
            out!("SessionExpired", dto::SessionExpiredDto { user_id: u.id.clone() }, user u.id)
        }
    }?;

    if outgoing.scope.allows(user) {
        Some(outgoing)
    } else {
        None
    }
}

/// `ThumbnailBookSseDto.seriesId`: unresolved book ids become an empty string
async fn book_series_id(state: &AppState, book_id: &str) -> String {
    BookDao::new(state.db.clone())
        .get_series_id_or_null(book_id)
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;
    use crate::settings::SettingsProvider;
    use crate::state::test_kmrs_db;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use komga_core::model::book::Book;
    use komga_core::model::library::Library;
    use komga_core::model::read_progress::ReadProgress;
    use komga_core::model::series::Series;
    use komga_core::model::thumbnail::{ThumbnailBook, ThumbnailType};
    use komga_core::model::user::{KomgaUser, UserRole};
    use komga_core::time_codec::now_utc;
    use komga_db::dao::user::UserDao;
    use komga_db::pool::Database;
    use komga_db::{Migrator, Placeholders};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let db = Database::open_in_memory(true).unwrap();
        Migrator::new(&komga_db::main_migrations(), Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        let tasks_db = Database::open_in_memory(false).unwrap();
        // dedicated task pools reuse the same in-memory database: task execution and assertions stay in sync
        let task_db = db.clone();
        Migrator::new(&komga_db::tasks_migrations(), Placeholders::default())
            .migrate(&tasks_db.rw())
            .unwrap();
        let config = crate::config::ServerConfig::from_env();
        AppState {
            config: Arc::new(config.clone()),
            db: db.clone(),
            task_db: task_db.clone(),
            tasks_db: tasks_db.clone(),
            kmrs_db: test_kmrs_db(),
            sessions: auth::SessionStore::new(Duration::from_secs(3600)),
            settings: Arc::new(SettingsProvider::load(db.clone())),
            tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
            events: crate::events::event_bus(),
            task_emitter: Arc::new(crate::service::TaskEmitter::new(
                db,
                tasks_db,
                Arc::new(tokio::sync::Notify::new()),
            )),
            search_index: test_search_index(),
            kepub: crate::service::kepub::KepubConverter::new(tempfile::tempdir().unwrap().keep()),
            kobo_proxy: crate::service::kobo_proxy::KoboProxy::new(),
            webui_dir: crate::webui::WebuiDir::default(),
            shutdown_tx: tokio::sync::watch::channel(false).0,
        }
    }

    fn test_app(state: &AppState) -> axum::Router {
        router_with_intervals(SseIntervals {
            heartbeat: Duration::from_millis(40),
            task_status: Duration::from_millis(40),
        })
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state.clone())
    }

    fn insert_user(db: &Database, email: &str, admin: bool) -> String {
        let user = KomgaUser {
            id: String::new(),
            email: email.into(),
            password: "x".into(),
            roles: if admin {
                [UserRole::Admin].into_iter().collect()
            } else {
                Default::default()
            },
            shared_libraries_ids: Default::default(),
            shared_all_libraries: true,
            restrictions: Default::default(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        UserDao::new(db.clone()).insert(&user).unwrap()
    }

    fn insert_api_key(db: &Database, user_id: &str, plain: &str) {
        let key = komga_core::model::user::ApiKey {
            id: String::new(),
            user_id: user_id.into(),
            key: auth::sha512_hex(plain),
            comment: "test".into(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        UserDao::new(db.clone()).insert_api_key(&key).unwrap();
    }

    fn sample_series(id: &str, library_id: &str) -> Series {
        Series {
            id: id.into(),
            name: "Berserk".into(),
            url: "file:/data/berserk/".into(),
            file_last_modified: now_utc(),
            library_id: library_id.into(),
            book_count: 0,
            deleted_date: None,
            oneshot: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn sample_book(id: &str, series_id: &str, library_id: &str) -> Book {
        Book {
            id: id.into(),
            name: "v01".into(),
            url: "file:/data/berserk/v01.cbz".into(),
            file_last_modified: now_utc(),
            series_id: series_id.into(),
            library_id: library_id.into(),
            file_size: 100,
            number: 0,
            file_hash: String::new(),
            file_hash_koreader: String::new(),
            deleted_date: None,
            oneshot: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn insert_library(db: &Database, id: &str) {
        let library = Library {
            id: id.into(),
            name: "Manga".into(),
            root: "file:/data/manga/".into(),
            import_comicinfo_book: false,
            import_comicinfo_series: false,
            import_comicinfo_collection: false,
            import_comicinfo_readlist: false,
            import_comicinfo_series_append_volume: false,
            import_epub_book: false,
            import_epub_series: false,
            import_mylar_series: false,
            import_local_artwork: false,
            import_barcode_isbn: false,
            scan_force_modified_time: false,
            scan_interval: komga_core::model::library::ScanInterval::Daily,
            scan_on_startup: false,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: komga_core::model::library::SeriesCover::First,
            hash_files: false,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: false,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        komga_db::dao::library::LibraryDao::new(db.clone())
            .insert(&library)
            .unwrap();
    }

    /// Waits until the per-connection task has subscribed to the bus
    async fn wait_subscribed(state: &AppState) {
        for _ in 0..100 {
            if state.events.receiver_count() > 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("SSE task did not subscribe in time");
    }

    async fn read_until(body: &mut Body, needle: &str, timeout: Duration) -> String {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut acc = String::new();
        while !acc.contains(needle) {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, body.frame()).await {
                Ok(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        acc.push_str(&String::from_utf8_lossy(&data));
                    }
                }
                _ => break,
            }
        }
        acc
    }

    async fn read_for(body: &mut Body, timeout: Duration) -> String {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut acc = String::new();
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, body.frame()).await {
                Ok(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        acc.push_str(&String::from_utf8_lossy(&data));
                    }
                }
                _ => break,
            }
        }
        acc
    }

    async fn connect(state: &AppState, api_key: &str) -> Body {
        let request = Request::get("/sse/v1/events")
            .header("X-API-Key", api_key)
            .body(Body::empty())
            .unwrap();
        let response = test_app(state).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap(),
            "text/event-stream"
        );
        response.into_body()
    }

    #[tokio::test]
    async fn unauthenticated_gets_401() {
        let state = test_state();
        let request = Request::get("/sse/v1/events").body(Body::empty()).unwrap();
        let response = test_app(&state).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn heartbeat_arrives() {
        let state = test_state();
        let user_id = insert_user(&state.db, "admin@komga.org", true);
        insert_api_key(&state.db, &user_id, "key1");

        let mut body = connect(&state, "key1").await;
        let acc = read_until(&mut body, ":heartbeat", Duration::from_secs(2)).await;
        assert!(acc.contains(":heartbeat"), "no heartbeat in: {acc:?}");
    }

    #[tokio::test]
    async fn series_added_event() {
        let state = test_state();
        let user_id = insert_user(&state.db, "admin@komga.org", true);
        insert_api_key(&state.db, &user_id, "key1");

        let mut body = connect(&state, "key1").await;
        wait_subscribed(&state).await;
        state
            .events
            .send(DomainEvent::SeriesAdded(sample_series("s1", "l1")))
            .unwrap();

        let acc = read_until(&mut body, "event:SeriesAdded", Duration::from_secs(2)).await;
        assert!(
            acc.contains("event:SeriesAdded\ndata:{\"seriesId\":\"s1\",\"libraryId\":\"l1\"}"),
            "unexpected payload in: {acc:?}"
        );
    }

    #[tokio::test]
    async fn task_queue_status_admin_only() {
        let state = test_state();
        let admin_id = insert_user(&state.db, "admin@komga.org", true);
        insert_api_key(&state.db, &admin_id, "admin-key");
        let user_id = insert_user(&state.db, "user@komga.org", false);
        insert_api_key(&state.db, &user_id, "user-key");

        state
            .task_emitter
            .submit(komga_core::task::Task::scan_library("l1", false, 4))
            .unwrap();

        let mut admin_body = connect(&state, "admin-key").await;
        let acc = read_until(
            &mut admin_body,
            "event:TaskQueueStatus",
            Duration::from_secs(2),
        )
        .await;
        assert!(
            acc.contains(
                "event:TaskQueueStatus\ndata:{\"count\":1,\"countByType\":{\"ScanLibrary\":1}}"
            ),
            "unexpected payload in: {acc:?}"
        );

        let mut user_body = connect(&state, "user-key").await;
        let acc = read_for(&mut user_body, Duration::from_millis(150)).await;
        assert!(
            !acc.contains("TaskQueueStatus"),
            "non-admin received TaskQueueStatus: {acc:?}"
        );
    }

    #[tokio::test]
    async fn book_imported_admin_only() {
        let state = test_state();
        let admin_id = insert_user(&state.db, "admin@komga.org", true);
        insert_api_key(&state.db, &admin_id, "admin-key");
        let user_id = insert_user(&state.db, "user@komga.org", false);
        insert_api_key(&state.db, &user_id, "user-key");

        let mut admin_body = connect(&state, "admin-key").await;
        let mut user_body = connect(&state, "user-key").await;
        wait_subscribed(&state).await;

        state
            .events
            .send(DomainEvent::BookImported {
                book: Some(sample_book("b1", "s1", "l1")),
                source_file: "/data/incoming/v01.cbz".into(),
                success: true,
                message: None,
            })
            .unwrap();

        let acc = read_until(
            &mut admin_body,
            "event:BookImported",
            Duration::from_secs(2),
        )
        .await;
        assert!(
            acc.contains("event:BookImported\ndata:{\"bookId\":\"b1\",\"sourceFile\":\"/data/incoming/v01.cbz\",\"success\":true,\"message\":null}"),
            "unexpected payload in: {acc:?}"
        );

        let acc = read_for(&mut user_body, Duration::from_millis(150)).await;
        assert!(
            !acc.contains("BookImported"),
            "non-admin received BookImported: {acc:?}"
        );
    }

    #[tokio::test]
    async fn read_progress_user_only() {
        let state = test_state();
        let owner_id = insert_user(&state.db, "owner@komga.org", false);
        insert_api_key(&state.db, &owner_id, "owner-key");
        let other_id = insert_user(&state.db, "other@komga.org", false);
        insert_api_key(&state.db, &other_id, "other-key");

        let mut owner_body = connect(&state, "owner-key").await;
        let mut other_body = connect(&state, "other-key").await;
        wait_subscribed(&state).await;

        let progress = ReadProgress {
            book_id: "b1".into(),
            user_id: owner_id.clone(),
            page: 3,
            completed: false,
            read_date: now_utc(),
            device_id: String::new(),
            device_name: String::new(),
            locator: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        state
            .events
            .send(DomainEvent::ReadProgressChanged(progress))
            .unwrap();

        let acc = read_until(
            &mut owner_body,
            "event:ReadProgressChanged",
            Duration::from_secs(2),
        )
        .await;
        assert!(
            acc.contains(&format!(
                "event:ReadProgressChanged\ndata:{{\"bookId\":\"b1\",\"userId\":\"{owner_id}\"}}"
            )),
            "unexpected payload in: {acc:?}"
        );

        let acc = read_for(&mut other_body, Duration::from_millis(150)).await;
        assert!(
            !acc.contains("ReadProgressChanged"),
            "other user received ReadProgressChanged: {acc:?}"
        );
    }

    #[tokio::test]
    async fn thumbnail_book_resolves_series_id() {
        let state = test_state();
        let user_id = insert_user(&state.db, "admin@komga.org", true);
        insert_api_key(&state.db, &user_id, "key1");

        insert_library(&state.db, "l1");
        komga_db::dao::series::SeriesDao::new(state.db.clone())
            .insert(&sample_series("s1", "l1"))
            .unwrap();
        komga_db::dao::book::BookDao::new(state.db.clone())
            .insert(&sample_book("b1", "s1", "l1"))
            .unwrap();

        let mut body = connect(&state, "key1").await;
        wait_subscribed(&state).await;

        let thumbnail = ThumbnailBook {
            id: "t1".into(),
            book_id: "b1".into(),
            thumbnail: None,
            url: None,
            selected: true,
            type_: ThumbnailType::Generated,
            media_type: "image/jpeg".into(),
            file_size: 10,
            dimension: komga_core::model::thumbnail::Dimension {
                width: 1,
                height: 1,
            },
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        state
            .events
            .send(DomainEvent::ThumbnailBookAdded(thumbnail))
            .unwrap();

        let acc = read_until(
            &mut body,
            "event:ThumbnailBookAdded",
            Duration::from_secs(2),
        )
        .await;
        assert!(
            acc.contains("event:ThumbnailBookAdded\ndata:{\"bookId\":\"b1\",\"seriesId\":\"s1\",\"selected\":true}"),
            "unexpected payload in: {acc:?}"
        );
    }
}
