//! Spring Boot Actuator endpoint subset (`/actuator/**`): health, info, metrics,
//! scheduledtasks, shutdown.
//!
//! Auth semantics, from komga's `SecurityConfiguration.kt` and `application.yml`:
//! - `/actuator/health`: permitAll — anonymous gets the bare status; ADMIN gets details
//!   (`management.endpoint.health.show-details: when_authorized`).
//! - `/actuator/info`: permitAll (`management.info.java/os.enabled: true`).
//! - `/actuator/shutdown`: anonymous (`management.endpoint.shutdown.access: unrestricted`).
//! - everything else (metrics, scheduledtasks): ADMIN only
//!   (`requestMatchers(EndpointRequest.toAnyEndpoint()).hasRole(ADMIN)`).
//!
//! All responses carry Spring's actuator media type `application/vnd.spring-boot.actuator.v3+json`.

use crate::auth::{MaybeAuth, RequireAuth};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use serde::Serialize;
use std::sync::OnceLock;

const ACTUATOR_JSON: &str = "application/vnd.spring-boot.actuator.v3+json";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/actuator/health", routing::get(get_health))
        .route("/actuator/info", routing::get(get_info))
        .route("/actuator/metrics", routing::get(get_metric_names))
        .route("/actuator/metrics/{name}", routing::get(get_metric))
        .route("/actuator/shutdown", routing::post(post_shutdown))
        .route(
            "/actuator/scheduledtasks",
            routing::get(get_scheduled_tasks),
        )
}

fn actuator_response<T: Serialize>(body: &T) -> Response {
    let mut response = Json(body).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, ACTUATOR_JSON.parse().unwrap());
    response
}

// region health

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthBody {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    components: Option<HealthComponents>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthComponents {
    db: HealthDbComponent,
    disk_space: HealthComponent,
}

/// Spring Boot's relational database health group: the database itself, then one entry per data source
#[derive(Serialize)]
struct HealthDbComponent {
    status: &'static str,
    components: std::collections::BTreeMap<String, HealthComponent>,
}

#[derive(Serialize)]
struct HealthComponent {
    status: &'static str,
    details: serde_json::Value,
}

/// Spring's `HealthEndpoint` with `show-details: when_authorized`: anonymous gets only the
/// status; details (db + diskSpace) are shown to ADMIN.
async fn get_health(State(state): State<AppState>, auth: MaybeAuth) -> Response {
    let admin = auth.0.as_ref().is_some_and(|a| a.user.is_admin());
    if !admin {
        return actuator_response(&HealthBody {
            status: "UP",
            components: None,
        });
    }

    let data_source = |db: &komga_db::pool::Database| match db
        .ro()
        .query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
    {
        Ok(1) => HealthComponent {
            status: "UP",
            details: serde_json::json!({
                "database": "SQLite",
                "validationQuery": "isValid()",
            }),
        },
        _ => HealthComponent {
            status: "DOWN",
            details: serde_json::json!({}),
        },
    };
    let mut db_components = std::collections::BTreeMap::new();
    db_components.insert("sqliteDataSourceRO".to_string(), data_source(&state.db));
    db_components.insert("sqliteDataSourceRW".to_string(), data_source(&state.db));
    db_components.insert(
        "tasksDataSourceRO".to_string(),
        data_source(&state.tasks_db),
    );
    db_components.insert(
        "tasksDataSourceRW".to_string(),
        data_source(&state.tasks_db),
    );

    let (total, free) = disk_space_k_bytes(&state.config.config_dir);
    let disk_component = HealthComponent {
        status: "UP",
        details: serde_json::json!({
            "total": total,
            "free": free,
            "threshold": 10_485_760i64,
            "exists": true,
        }),
    };

    actuator_response(&HealthBody {
        status: "UP",
        components: Some(HealthComponents {
            db: HealthDbComponent {
                status: "UP",
                components: db_components,
            },
            disk_space: disk_component,
        }),
    })
}

/// `df -k <path>`: total and available 1K-blocks of the volume holding the config dir.
fn disk_space_k_bytes(path: &std::path::Path) -> (i64, i64) {
    std::process::Command::new("df")
        .arg("-k")
        .arg(path)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|out| {
            let line = out.lines().nth(1)?.to_string();
            let fields: Vec<&str> = line.split_whitespace().collect();
            let total: i64 = fields.get(1)?.parse().ok()?;
            let free: i64 = fields.get(3)?.parse().ok()?;
            Some((total * 1024, free * 1024))
        })
        .unwrap_or((0, 0))
}

// endregion

// region info

#[derive(Serialize)]
struct InfoBody {
    git: InfoGit,
    build: InfoBuild,
    java: InfoJava,
    os: InfoOs,
}

#[derive(Serialize)]
struct InfoGit {
    branch: String,
    commit: InfoGitCommit,
}

#[derive(Serialize)]
struct InfoGitCommit {
    id: String,
    time: String,
}

#[derive(Serialize)]
struct InfoBuild {
    artifact: String,
    name: String,
    version: String,
    group: String,
}

#[derive(Serialize)]
struct InfoJava {
    version: String,
    vendor: InfoVendor,
    runtime: InfoRuntime,
    jvm: InfoJvm,
}

#[derive(Serialize)]
struct InfoVendor {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct InfoRuntime {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct InfoJvm {
    name: String,
    vendor: String,
    version: String,
}

#[derive(Serialize)]
struct InfoOs {
    name: String,
    version: String,
    arch: String,
}

/// `management.info.java/os.enabled: true`. The `java` section keeps Spring's shape for
/// compatibility but holds no made-up JVM values; `os` reports the platform.
async fn get_info() -> Response {
    let (os_name, os_version) = os_name_version();
    actuator_response(&InfoBody {
        git: InfoGit {
            branch: env!("GIT_BRANCH").to_string(),
            commit: InfoGitCommit {
                id: env!("GIT_COMMIT_ID").to_string(),
                time: env!("GIT_COMMIT_TIME").to_string(),
            },
        },
        build: InfoBuild {
            artifact: "komga-server".to_string(),
            name: "kmrs".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            group: "kmrs".to_string(),
        },
        java: InfoJava {
            version: "-".to_string(),
            vendor: InfoVendor {
                name: "-".to_string(),
                version: "-".to_string(),
            },
            runtime: InfoRuntime {
                name: "-".to_string(),
                version: "-".to_string(),
            },
            jvm: InfoJvm {
                name: "-".to_string(),
                vendor: "-".to_string(),
                version: "-".to_string(),
            },
        },
        os: InfoOs {
            name: os_name,
            version: os_version,
            arch: os_arch(),
        },
    })
}

/// `os.name` as Spring reports it (from `System.getProperty("os.name")`), with `uname -r` for the version.
fn os_name_version() -> (String, String) {
    let name = match std::env::consts::OS {
        "macos" => "Mac OS X",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    }
    .to_string();
    let version = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    (name, version)
}

/// `os.arch` as Spring reports it.
fn os_arch() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "amd64".to_string(),
        other => other.to_string(),
    }
}

// endregion

// region metrics

fn process_start() -> &'static (std::time::Instant, f64) {
    static START: OnceLock<(std::time::Instant, f64)> = OnceLock::new();
    START.get_or_init(|| {
        let epoch_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        (std::time::Instant::now(), epoch_millis)
    })
}

#[derive(Serialize)]
struct MetricNames {
    names: Vec<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricBody {
    name: &'static str,
    description: &'static str,
    base_unit: &'static str,
    measurements: Vec<MetricMeasurement>,
    available_tags: Vec<String>,
}

#[derive(Serialize)]
struct MetricMeasurement {
    statistic: &'static str,
    value: f64,
}

const METRICS: &[&str] = &[
    "jvm.memory.used",
    "komga.books",
    "komga.books.filesize",
    "komga.collections",
    "komga.libraries",
    "komga.readlists",
    "komga.series",
    "komga.sidecars",
    "process.cpu.usage",
    "process.start.time",
    "process.uptime",
];

/// Only ADMIN (`EndpointRequest.toAnyEndpoint().hasRole(ADMIN)`).
async fn get_metric_names(auth: RequireAuth) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    Ok(actuator_response(&MetricNames {
        names: METRICS.to_vec(),
    }))
}

async fn get_metric(
    State(_state): State<AppState>,
    auth: RequireAuth,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let body = match name.as_str() {
        "jvm.memory.used" => MetricBody {
            name: "jvm.memory.used",
            description: "The amount of used memory",
            base_unit: "bytes",
            measurements: vec![MetricMeasurement {
                statistic: "VALUE",
                value: rss_bytes() as f64,
            }],
            available_tags: vec![],
        },
        "process.start.time" => MetricBody {
            name: "process.start.time",
            description: "Start time of the process",
            base_unit: "milliseconds",
            measurements: vec![MetricMeasurement {
                statistic: "VALUE",
                value: process_start().1,
            }],
            available_tags: vec![],
        },
        "process.uptime" => MetricBody {
            name: "process.uptime",
            description: "The uptime of the Java virtual machine",
            base_unit: "seconds",
            measurements: vec![MetricMeasurement {
                statistic: "VALUE",
                value: process_start().0.elapsed().as_secs_f64(),
            }],
            available_tags: vec![],
        },
        "process.cpu.usage" => MetricBody {
            name: "process.cpu.usage",
            description: "The \"recent cpu usage\" for the Java Virtual Machine process",
            base_unit: "percent",
            measurements: vec![MetricMeasurement {
                statistic: "VALUE",
                value: cpu_usage_percent(),
            }],
            available_tags: vec![],
        },
        name if name.starts_with("komga.") => komga_gauge(name, &_state)?,
        _ => return Err(ApiError::not_found("")),
    };
    Ok(actuator_response(&body))
}

/// `MetricsPublisherController` gauges: entity counts read straight from the database.
fn komga_gauge(name: &str, state: &AppState) -> Result<MetricBody, ApiError> {
    let (description, base_unit, value) = match name {
        "komga.libraries" => (
            "Number of libraries",
            "libraries",
            count_of(state, "LIBRARY"),
        ),
        "komga.series" => ("Number of series", "series", count_of(state, "SERIES")),
        "komga.books" => ("Number of books", "books", count_of(state, "BOOK")),
        "komga.books.filesize" => (
            "Total file size of all books",
            "bytes",
            sum_of(state, "SELECT COALESCE(SUM(FILE_SIZE), 0) FROM BOOK"),
        ),
        "komga.collections" => (
            "Number of collections",
            "collections",
            count_of(state, "COLLECTION"),
        ),
        "komga.readlists" => (
            "Number of read lists",
            "read lists",
            count_of(state, "READLIST"),
        ),
        "komga.sidecars" => ("Number of sidecars", "sidecars", count_of(state, "SIDECAR")),
        _ => return Err(ApiError::not_found("")),
    };
    Ok(MetricBody {
        name: name_static(name),
        description: description_static(description),
        base_unit: base_unit_static(base_unit),
        measurements: vec![MetricMeasurement {
            statistic: "VALUE",
            value,
        }],
        available_tags: vec![],
    })
}

fn name_static(name: &str) -> &'static str {
    match name {
        "komga.libraries" => "komga.libraries",
        "komga.series" => "komga.series",
        "komga.books" => "komga.books",
        "komga.books.filesize" => "komga.books.filesize",
        "komga.collections" => "komga.collections",
        "komga.readlists" => "komga.readlists",
        "komga.sidecars" => "komga.sidecars",
        _ => unreachable!(),
    }
}

fn description_static(s: &str) -> &'static str {
    match s {
        "Number of libraries" => "Number of libraries",
        "Number of series" => "Number of series",
        "Number of books" => "Number of books",
        "Total file size of all books" => "Total file size of all books",
        "Number of collections" => "Number of collections",
        "Number of read lists" => "Number of read lists",
        "Number of sidecars" => "Number of sidecars",
        _ => unreachable!(),
    }
}

fn base_unit_static(s: &str) -> &'static str {
    match s {
        "libraries" => "libraries",
        "series" => "series",
        "books" => "books",
        "bytes" => "bytes",
        "collections" => "collections",
        "read lists" => "read lists",
        "sidecars" => "sidecars",
        _ => unreachable!(),
    }
}

fn count_of(state: &AppState, table: &str) -> f64 {
    state
        .db
        .ro()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0) as f64
}

fn sum_of(state: &AppState, sql: &str) -> f64 {
    state
        .db
        .ro()
        .query_row(sql, [], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as f64
}

/// Recent CPU usage of this process, via `ps -o %cpu` (percent).
fn cpu_usage_percent() -> f64 {
    std::process::Command::new("ps")
        .arg("-o")
        .arg("%cpu=")
        .arg("-p")
        .arg(std::process::id().to_string())
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}
fn rss_bytes() -> i64 {
    std::process::Command::new("ps")
        .arg("-o")
        .arg("rss=")
        .arg("-p")
        .arg(std::process::id().to_string())
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
        * 1024
}

// endregion

// region scheduledtasks

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduledTasksBody {
    cron: Vec<serde_json::Value>,
    fixed_delay: Vec<serde_json::Value>,
    fixed_rate: Vec<ScheduledTaskEntry>,
    custom: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct ScheduledTaskEntry {
    runnable: ScheduledTaskRunnable,
    #[serde(rename = "initialDelay")]
    initial_delay: u64,
    interval: u64,
}

#[derive(Serialize)]
struct ScheduledTaskRunnable {
    target: String,
}

/// Spring's `ScheduledTasksEndpoint`, fed by the scan scheduler's per-library interval tasks
/// (each with `initialDelay == interval == period`, like `FixedRateTask`).
/// Spring's `ScheduledTasksEndpoint`, fed by the scan scheduler's per-library interval tasks
/// plus the fixed-rate jobs (SSE heartbeat / task count, authentication-activity cleanup).
async fn get_scheduled_tasks(auth: RequireAuth) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let mut fixed_rate: Vec<ScheduledTaskEntry> =
        crate::service::scheduler::ScanScheduler::scheduled_tasks()
            .into_iter()
            .map(|registration| {
                let millis = registration.period.as_millis() as u64;
                ScheduledTaskEntry {
                    runnable: ScheduledTaskRunnable {
                        target: format!(
                            "ScanScheduler for library '{}'",
                            registration.library_name
                        ),
                    },
                    initial_delay: millis,
                    interval: millis,
                }
            })
            .collect();
    for (target, millis) in [
        ("SseController.heartbeat", 15_000u64),
        ("SseController.taskCount", 10_000u64),
        (
            "AuthenticationActivityCleanupController.cleanup",
            86_400_000u64,
        ),
    ] {
        fixed_rate.push(ScheduledTaskEntry {
            runnable: ScheduledTaskRunnable {
                target: target.to_string(),
            },
            initial_delay: millis,
            interval: millis,
        });
    }
    Ok(actuator_response(&ScheduledTasksBody {
        cron: vec![],
        fixed_delay: vec![],
        fixed_rate,
        custom: vec![],
    }))
}

// endregion

// region shutdown

#[derive(Serialize)]
struct ShutdownBody {
    message: &'static str,
}

/// `management.endpoint.shutdown.access: unrestricted`: anonymous shutdown. The response is
/// sent first; the actual shutdown fires shortly after, so the connection can complete.
async fn post_shutdown(State(state): State<AppState>) -> Response {
    let tx = state.shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let _ = tx.send(true);
    });
    actuator_response(&ShutdownBody {
        message: "Shutting down, bye...",
    })
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;
    use crate::config::ServerConfig;
    use crate::settings::SettingsProvider;
    use axum::body::Body;
    use axum::http::{HeaderMap, Request, StatusCode};
    use komga_core::model::user::{ContentRestrictions, KomgaUser, UserRole};
    use komga_core::time_codec::now_utc;
    use komga_db::dao::user::UserDao;
    use komga_db::pool::Database;
    use komga_db::{Migrator, Placeholders};
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_state() -> (AppState, tokio::sync::watch::Receiver<bool>) {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = komga_db::main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        let tasks_db = Database::open_in_memory(false).unwrap();
        let tasks_migrations = komga_db::tasks_migrations();
        Migrator::new(&tasks_migrations, Placeholders::default())
            .migrate(&tasks_db.rw())
            .unwrap();
        let config = ServerConfig::from_env();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let state = AppState {
            config: Arc::new(config.clone()),
            settings: Arc::new(SettingsProvider::load(db.clone())),
            task_emitter: Arc::new(crate::service::TaskEmitter::new(
                db.clone(),
                tasks_db.clone(),
                std::sync::Arc::new(tokio::sync::Notify::new()),
            )),
            db,
            tasks_db,
            sessions: auth::SessionStore::new(config.session_timeout),
            tsid: Arc::new(komga_core::tsid::TsidFactory::new_random_node()),
            events: crate::events::event_bus(),
            search_index: crate::state::test_search_index(),
            kepub: crate::service::kepub::KepubConverter::new(tempfile::tempdir().unwrap().keep()),
            kobo_proxy: crate::service::kobo_proxy::KoboProxy::new(),
            shutdown_tx,
        };
        (state, shutdown_rx)
    }

    fn test_router(state: AppState) -> Router {
        Router::new()
            .merge(router())
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth::auth_middleware,
            ))
            .with_state(state)
    }

    fn seed_user(state: &AppState, email: &str, admin: bool, key: &str) {
        let dao = UserDao::new(state.db.clone());
        let user_id = dao
            .insert(&KomgaUser {
                id: String::new(),
                email: email.to_string(),
                password: bcrypt::hash("pass", 10).unwrap(),
                roles: if admin {
                    [UserRole::Admin].into_iter().collect()
                } else {
                    BTreeSet::new()
                },
                shared_libraries_ids: BTreeSet::new(),
                shared_all_libraries: true,
                restrictions: ContentRestrictions::default(),
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
        dao.insert_api_key(&komga_core::model::user::ApiKey {
            id: String::new(),
            user_id,
            key: crate::auth::sha512_hex(key),
            comment: "test".into(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        })
        .unwrap();
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        api_key: Option<&str>,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(key) = api_key {
            builder = builder.header("X-API-Key", key);
        }
        let request = builder.body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, headers, bytes)
    }

    fn json(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_slice(bytes).unwrap()
    }

    #[tokio::test]
    async fn health_anonymous_gets_bare_status() {
        let (state, _rx) = test_state();
        let app = test_router(state);
        let (status, headers, bytes) = call(&app, "GET", "/actuator/health", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), ACTUATOR_JSON);
        let body = json(&bytes);
        assert_eq!(body, serde_json::json!({"status": "UP"}));
    }

    #[tokio::test]
    async fn health_admin_gets_details() {
        let (state, _rx) = test_state();
        seed_user(&state, "admin@komga.org", true, "k1");
        let app = test_router(state);
        let (status, _headers, bytes) = call(&app, "GET", "/actuator/health", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body["status"], "UP");
        assert_eq!(body["components"]["db"]["status"], "UP");
        for ds in [
            "sqliteDataSourceRO",
            "sqliteDataSourceRW",
            "tasksDataSourceRO",
            "tasksDataSourceRW",
        ] {
            let component = &body["components"]["db"]["components"][ds];
            assert_eq!(component["status"], "UP", "{ds}");
            assert_eq!(component["details"]["database"], "SQLite", "{ds}");
            assert_eq!(component["details"]["validationQuery"], "isValid()", "{ds}");
        }
        assert_eq!(
            body["components"]["diskSpace"]["details"]["threshold"],
            10_485_760i64
        );
        assert_eq!(body["components"]["diskSpace"]["details"]["exists"], true);
    }

    #[tokio::test]
    async fn health_non_admin_gets_bare_status() {
        let (state, _rx) = test_state();
        seed_user(&state, "user@komga.org", false, "k2");
        let app = test_router(state);
        let (status, _headers, bytes) = call(&app, "GET", "/actuator/health", Some("k2")).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body, serde_json::json!({"status": "UP"}));
    }

    #[tokio::test]
    async fn info_shape() {
        let (state, _rx) = test_state();
        let app = test_router(state);
        let (status, headers, bytes) = call(&app, "GET", "/actuator/info", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), ACTUATOR_JSON);
        let body = json(&bytes);
        assert_eq!(body["java"]["version"], "-");
        assert_eq!(body["java"]["vendor"]["name"], "-");
        assert_eq!(body["java"]["runtime"]["name"], "-");
        assert_eq!(body["java"]["jvm"]["vendor"], "-");
        assert_eq!(body["build"]["name"], "kmrs");
        assert_eq!(body["build"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["git"]["branch"], env!("GIT_BRANCH"));
        assert!(body["git"]["commit"]["id"].is_string());
        assert!(body["os"]["name"].is_string());
        assert!(body["os"]["version"].is_string());
        assert!(body["os"]["arch"].is_string());
    }

    #[tokio::test]
    async fn metrics_names_and_single_metric() {
        let (state, _rx) = test_state();
        seed_user(&state, "admin@komga.org", true, "k1");
        let app = test_router(state);
        let (status, _headers, bytes) = call(&app, "GET", "/actuator/metrics", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(
            body["names"],
            serde_json::json!([
                "jvm.memory.used",
                "komga.books",
                "komga.books.filesize",
                "komga.collections",
                "komga.libraries",
                "komga.readlists",
                "komga.series",
                "komga.sidecars",
                "process.cpu.usage",
                "process.start.time",
                "process.uptime"
            ])
        );

        for name in [
            "jvm.memory.used",
            "process.start.time",
            "process.uptime",
            "process.cpu.usage",
            "komga.libraries",
            "komga.books",
            "komga.books.filesize",
        ] {
            let (status, _headers, bytes) = call(
                &app,
                "GET",
                &format!("/actuator/metrics/{name}"),
                Some("k1"),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{name}");
            let body = json(&bytes);
            assert_eq!(body["name"], name);
            assert!(body["baseUnit"].is_string());
            assert_eq!(body["measurements"][0]["statistic"], "VALUE");
            assert!(body["measurements"][0]["value"].as_f64().unwrap() >= 0.0);
            assert_eq!(body["availableTags"], serde_json::json!([]));
        }

        let (status, _headers, bytes) =
            call(&app, "GET", "/actuator/metrics/process.uptime", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json(&bytes)["description"],
            "The uptime of the Java virtual machine"
        );

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/metrics/process.start.time",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["baseUnit"], "milliseconds");

        let (status, _headers, bytes) =
            call(&app, "GET", "/actuator/metrics/jvm.memory.used", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["baseUnit"], "bytes");
        assert!(json(&bytes)["measurements"][0]["value"].as_f64().unwrap() > 0.0);
    }

    #[tokio::test]
    async fn metrics_unknown_name_is_404() {
        let (state, _rx) = test_state();
        seed_user(&state, "admin@komga.org", true, "k1");
        let app = test_router(state);
        let (status, _headers, _bytes) =
            call(&app, "GET", "/actuator/metrics/does.not.exist", Some("k1")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn metrics_and_scheduledtasks_require_admin() {
        let (state, _rx) = test_state();
        seed_user(&state, "user@komga.org", false, "k2");
        let app = test_router(state);
        let (status, _headers, bytes) = call(&app, "GET", "/actuator/metrics", Some("k2")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json(&bytes)["message"], "403 FORBIDDEN");
        let (status, _headers, _bytes) =
            call(&app, "GET", "/actuator/scheduledtasks", Some("k2")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // and unauthenticated altogether
        let (status, _headers, _bytes) = call(&app, "GET", "/actuator/metrics", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn scheduledtasks_shape() {
        let (state, _rx) = test_state();
        seed_user(&state, "admin@komga.org", true, "k1");
        let app_state = state.clone();
        let app = test_router(state);

        let library = komga_core::model::library::Library {
            id: "lib-1".into(),
            name: "Manga".into(),
            ..service_series_test_library()
        };
        crate::service::scheduler::ScanScheduler::schedule_scan(&app_state, &library);

        let (status, headers, bytes) =
            call(&app, "GET", "/actuator/scheduledtasks", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), ACTUATOR_JSON);
        let body = json(&bytes);
        assert_eq!(body["cron"], serde_json::json!([]));
        assert_eq!(body["fixedDelay"], serde_json::json!([]));
        assert_eq!(body["custom"], serde_json::json!([]));
        let task = &body["fixedRate"][0];
        assert_eq!(
            task["runnable"]["target"],
            "ScanScheduler for library 'Manga'"
        );
        assert_eq!(task["initialDelay"], task["interval"]);
        assert_eq!(task["interval"], 3_600_000i64);

        // removing the only registration leaves an empty fixedRate
        crate::service::scheduler::ScanScheduler::schedule_scan(
            &app_state,
            &komga_core::model::library::Library {
                id: "lib-1".into(),
                name: "Manga".into(),
                ..service_series_test_library_disabled()
            },
        );

        // empty registry still exposes the fixed-rate jobs
        let (state2, _rx2) = test_state();
        seed_user(&state2, "admin@komga.org", true, "k1");
        let app2 = test_router(state2);
        let (status, _headers, bytes) =
            call(&app2, "GET", "/actuator/scheduledtasks", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        let targets: Vec<&str> = body["fixedRate"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["runnable"]["target"].as_str().unwrap())
            .collect();
        assert_eq!(
            targets,
            vec![
                "SseController.heartbeat",
                "SseController.taskCount",
                "AuthenticationActivityCleanupController.cleanup"
            ]
        );
        assert_eq!(body["fixedRate"][0]["initialDelay"], 15_000i64);
        assert_eq!(body["fixedRate"][0]["interval"], 15_000i64);
        assert_eq!(body["fixedRate"][2]["interval"], 86_400_000i64);
    }

    fn service_series_test_library() -> komga_core::model::library::Library {
        use komga_core::model::library::{ScanInterval, SeriesCover};
        komga_core::model::library::Library {
            id: String::new(),
            name: String::new(),
            root: "file:/l/".into(),
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
            scan_on_startup: false,
            scan_interval: ScanInterval::Hourly,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: SeriesCover::First,
            hash_files: false,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: false,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn service_series_test_library_disabled() -> komga_core::model::library::Library {
        let mut library = service_series_test_library();
        library.scan_interval = komga_core::model::library::ScanInterval::Disabled;
        library
    }

    #[tokio::test]
    async fn shutdown_is_unrestricted_and_fires() {
        let (state, mut rx) = test_state();
        let app = test_router(state);
        let (status, headers, bytes) = call(&app, "POST", "/actuator/shutdown", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), ACTUATOR_JSON);
        assert_eq!(
            json(&bytes),
            serde_json::json!({"message": "Shutting down, bye..."})
        );
        // the deferred shutdown fires shortly after
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.changed())
            .await
            .expect("shutdown not fired")
            .unwrap();
    }
}
