//! Spring Boot Actuator endpoint subset (`/actuator/**`): links, health, info, flyway, logfile,
//! metrics, scheduledtasks, sessions, shutdown.
//!
//! komga's `application.yml` sets `management.endpoints.web.exposure.include: "*"`, so Java
//! exposes everything Spring can auto-configure. Endpoints and meters that only dump
//! JVM/Spring internals (beans, conditions, env, configprops, loggers, mappings, heapdump,
//! threaddump, `jvm.*`/`system.*`/`http.server.requests` metrics) are intentionally not
//! implemented — a documented scope exclusion, not a gap. Caches, integrationgraph, quartz,
//! prometheus, httpexchanges and startup are absent on the Java side too (missing dependencies).
//!
//! Auth semantics, from komga's `SecurityConfiguration.kt` and `application.yml`:
//! - `/actuator/health`: permitAll — anonymous gets the bare status; ADMIN gets details
//!   (`management.endpoint.health.show-details: when_authorized`).
//! - `/actuator/info`: permitAll (`management.info.java/os.enabled: true`).
//! - `/actuator/shutdown`: anonymous (`management.endpoint.shutdown.access: unrestricted`).
//! - everything else (flyway, logfile, metrics, scheduledtasks, sessions): ADMIN only
//!   (`requestMatchers(EndpointRequest.toAnyEndpoint()).hasRole(ADMIN)`).
//!
//! All responses carry Spring's actuator media type `application/vnd.spring-boot.actuator.v3+json`.

use crate::auth::{MaybeAuth, RequireAuth};
use crate::error::ApiError;
use crate::http::pagination::QueryExt;
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
        .route("/actuator", routing::get(get_links))
        .route("/actuator/health", routing::get(get_health))
        .route("/actuator/info", routing::get(get_info))
        .route("/actuator/flyway", routing::get(get_flyway))
        .route("/actuator/logfile", routing::get(get_logfile))
        .route("/actuator/metrics", routing::get(get_metric_names))
        .route("/actuator/metrics/{name}", routing::get(get_metric))
        .route("/actuator/shutdown", routing::post(post_shutdown))
        .route(
            "/actuator/scheduledtasks",
            routing::get(get_scheduled_tasks),
        )
        .route(
            "/actuator/sessions",
            routing::get(get_sessions_for_username),
        )
        .route(
            "/actuator/sessions/{id}",
            routing::get(get_session).delete(delete_session),
        )
}

fn actuator_response<T: Serialize>(body: &T) -> Response {
    let mut response = Json(body).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, ACTUATOR_JSON.parse().unwrap());
    response
}

// region links

/// Spring's actuator discovery page (`/actuator`): links to every exposed endpoint.
async fn get_links(headers: axum::http::HeaderMap) -> Response {
    let base = base_url(&headers);
    let link =
        |href: String, templated: bool| serde_json::json!({"href": href, "templated": templated});
    let actuator = format!("{base}/actuator");
    let mut links = serde_json::Map::new();
    links.insert("self".into(), link(actuator.clone(), false));
    for endpoint in [
        "health",
        "info",
        "flyway",
        "logfile",
        "metrics",
        "scheduledtasks",
        "shutdown",
    ] {
        links.insert(
            endpoint.into(),
            link(format!("{actuator}/{endpoint}"), false),
        );
    }
    links.insert(
        "metrics-requiredMetricName".into(),
        link(format!("{actuator}/metrics/{{requiredMetricName}}"), true),
    );
    links.insert(
        "sessions".into(),
        link(format!("{actuator}/sessions{{?username}}"), true),
    );
    actuator_response(&serde_json::json!({ "_links": links }))
}

/// Scheme/host of this server as the client sees it, honoring the de-facto forwarded headers
/// (`forward-headers-strategy: framework`).
fn base_url(headers: &axum::http::HeaderMap) -> String {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let proto = get("x-forwarded-proto").unwrap_or_else(|| "http".to_string());
    let host = get("x-forwarded-host")
        .or_else(|| get("host"))
        .unwrap_or_else(|| "localhost".to_string());
    format!("{proto}://{host}")
}

// endregion

// region flyway

/// Spring's `FlywayEndpoint`. komga has a single Flyway bean (the tasks database is migrated
/// by a manually built instance that never enters the context), so only the main database shows.
async fn get_flyway(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let conn = state.db.ro();
    let mut stmt = conn
        .prepare(
            "SELECT installed_rank, version, description, type, script, checksum, installed_by, installed_on, execution_time, success \
             FROM flyway_schema_history ORDER BY installed_rank",
        )
        .map_err(komga_db::Error::from)?;
    let migrations: Vec<serde_json::Value> = stmt
        .query_map([], |r| {
            let installed_on: String = r.get(7)?;
            Ok(serde_json::json!({
                "type": r.get::<_, String>(3)?,
                "checksum": r.get::<_, Option<i32>>(5)?,
                "version": r.get::<_, Option<String>>(1)?,
                "description": r.get::<_, String>(2)?,
                "script": r.get::<_, String>(4)?,
                "state": if r.get::<_, bool>(9)? { "SUCCESS" } else { "FAILED" },
                "installedBy": r.get::<_, String>(6)?,
                "installedOn": komga_core::time_codec::parse_datetime_utc(&installed_on)
                    .map(komga_core::time_codec::format_offset_date_time),
                "installedRank": r.get::<_, i64>(0)?,
                "executionTime": r.get::<_, i64>(8)?,
            }))
        })
        .map_err(komga_db::Error::from)?
        .collect::<std::result::Result<_, _>>()
        .map_err(komga_db::Error::from)?;
    Ok(actuator_response(&serde_json::json!({
        "contexts": {
            "application": {
                "flywayBeans": {
                    "flyway": { "migrations": migrations }
                }
            }
        }
    })))
}

// endregion

// region sessions

/// Spring's `SessionsEndpoint`: `?username=` filters by the principal-name index. komga's index
/// is the login email — or the API-key hash once an API-key auth replaces the session context.
async fn get_sessions_for_username(
    State(state): State<AppState>,
    auth: RequireAuth,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let username = crate::http::pagination::parse_query_multi(query.as_deref().unwrap_or(""))
        .first("username")
        .map(str::to_string);
    let mut sessions = Vec::new();
    if let Some(username) = username {
        let dao = komga_db::dao::user::UserDao::new(state.db.clone());
        // user_id → email, resolved lazily once per distinct id
        let mut emails: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
        for (id, data) in state.sessions.all() {
            let indexed_name = match &data.api_key {
                Some(key) => Some(key.hash.clone()),
                None => emails
                    .entry(data.user_id.clone())
                    .or_insert_with(|| {
                        dao.find_by_id(&data.user_id)
                            .ok()
                            .flatten()
                            .map(|u| u.email)
                    })
                    .clone(),
            };
            if indexed_name.as_deref() == Some(username.as_str()) {
                sessions.push(session_descriptor(&id, &data, &state));
            }
        }
    }
    Ok(actuator_response(
        &serde_json::json!({ "sessions": sessions }),
    ))
}

async fn get_session(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let data = state
        .sessions
        .get(&id)
        .ok_or_else(|| ApiError::not_found(""))?;
    Ok(actuator_response(&session_descriptor(&id, &data, &state)))
}

async fn delete_session(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    state.sessions.invalidate(&id);
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// Spring's `SessionDescriptor`: the security context is the only attribute komga sessions carry.
fn session_descriptor(
    id: &str,
    data: &crate::auth::session::SessionData,
    state: &AppState,
) -> serde_json::Value {
    let instant = |millis: u64| {
        komga_core::time_codec::format_offset_date_time(
            time::OffsetDateTime::from_unix_timestamp_nanos(millis as i128 * 1_000_000).unwrap(),
        )
    };
    serde_json::json!({
        "id": id,
        "attributeNames": ["SPRING_SECURITY_CONTEXT"],
        "creationTime": instant(data.created_at),
        "lastAccessedTime": instant(data.last_accessed_at),
        "maxInactiveInterval": state.config.session_timeout.as_secs(),
        "expired": false,
    })
}

// endregion

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
    base_unit: Option<&'static str>,
    measurements: Vec<MetricMeasurement>,
    available_tags: Vec<AvailableTag>,
}

#[derive(Serialize)]
struct MetricMeasurement {
    statistic: &'static str,
    value: f64,
}

#[derive(Serialize)]
struct AvailableTag {
    tag: &'static str,
    values: Vec<String>,
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
    "komga.tasks.execution",
    "komga.tasks.failure",
    "process.cpu.usage",
    "process.start.time",
    "process.uptime",
];

/// MultiGauge-backed names (`MetricsPublisherController`): the meter exists only while its
/// per-library rows are non-empty, so an empty library makes the name vanish entirely.
const MULTI_GAUGES: &[&str] = &[
    "komga.series",
    "komga.books",
    "komga.books.filesize",
    "komga.sidecars",
];

/// Only ADMIN (`EndpointRequest.toAnyEndpoint().hasRole(ADMIN)`).
async fn get_metric_names(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let names = METRICS
        .iter()
        .filter(|name| !MULTI_GAUGES.contains(name) || !multi_gauge_rows(&state, name).is_empty())
        .copied()
        .collect();
    Ok(actuator_response(&MetricNames { names }))
}

async fn get_metric(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(name): Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    // Spring's `MetricsEndpoint`: repeated `tag=key:value` filters meters; no match → 404
    let tags: Vec<(String, String)> =
        crate::http::pagination::parse_query_multi(query.as_deref().unwrap_or(""))
            .all("tag")
            .iter()
            .filter_map(|t| {
                t.split_once(':')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();
    let body = match name.as_str() {
        "jvm.memory.used" => {
            reject_tags(&tags)?;
            MetricBody {
                name: "jvm.memory.used",
                description: "The amount of used memory",
                base_unit: Some("bytes"),
                measurements: vec![MetricMeasurement {
                    statistic: "VALUE",
                    value: rss_bytes() as f64,
                }],
                available_tags: vec![],
            }
        }
        "process.start.time" => {
            reject_tags(&tags)?;
            MetricBody {
                name: "process.start.time",
                description: "Start time of the process",
                base_unit: Some("milliseconds"),
                measurements: vec![MetricMeasurement {
                    statistic: "VALUE",
                    value: process_start().1,
                }],
                available_tags: vec![],
            }
        }
        "process.uptime" => {
            reject_tags(&tags)?;
            MetricBody {
                name: "process.uptime",
                description: "The uptime of the Java virtual machine",
                base_unit: Some("seconds"),
                measurements: vec![MetricMeasurement {
                    statistic: "VALUE",
                    value: process_start().0.elapsed().as_secs_f64(),
                }],
                available_tags: vec![],
            }
        }
        "process.cpu.usage" => {
            reject_tags(&tags)?;
            MetricBody {
                name: "process.cpu.usage",
                description: "The \"recent cpu usage\" for the Java Virtual Machine process",
                base_unit: Some("percent"),
                measurements: vec![MetricMeasurement {
                    statistic: "VALUE",
                    value: cpu_usage_percent(),
                }],
                available_tags: vec![],
            }
        }
        "komga.tasks.execution" => tasks_execution_metric(&tags)?,
        "komga.tasks.failure" => tasks_failure_metric(&tags)?,
        name if name.starts_with("komga.") => komga_gauge(name, &state, &tags)?,
        _ => return Err(ApiError::not_found("")),
    };
    Ok(actuator_response(&body))
}

fn reject_tags(tags: &[(String, String)]) -> Result<(), ApiError> {
    if tags.is_empty() {
        Ok(())
    } else {
        Err(ApiError::not_found(""))
    }
}

/// Narrows the tag filter to the single tag our meters carry; any other combination matches
/// no meter (Spring answers 404 then).
fn tag_value<'a>(tags: &'a [(String, String)], key: &str) -> Result<Option<&'a str>, ApiError> {
    match tags {
        [] => Ok(None),
        [(k, v)] if k == key => Ok(Some(v)),
        _ => Err(ApiError::not_found("")),
    }
}

fn tasks_execution_metric(tags: &[(String, String)]) -> Result<MetricBody, ApiError> {
    let metrics = crate::service::metrics::task_metrics();
    let filter = tag_value(tags, "type")?;
    let (count, total, max) = match filter {
        Some(task_type) => {
            let m = metrics
                .get(task_type)
                .ok_or_else(|| ApiError::not_found(""))?;
            (m.executions, m.total, m.max)
        }
        None => metrics.values().fold(
            (0, std::time::Duration::ZERO, std::time::Duration::ZERO),
            |(count, total, max), m| (count + m.executions, total + m.total, max.max(m.max)),
        ),
    };
    Ok(MetricBody {
        name: "komga.tasks.execution",
        description: "Task execution time",
        base_unit: Some("seconds"),
        measurements: vec![
            MetricMeasurement {
                statistic: "COUNT",
                value: count as f64,
            },
            MetricMeasurement {
                statistic: "TOTAL_TIME",
                value: total.as_secs_f64(),
            },
            MetricMeasurement {
                statistic: "MAX",
                value: max.as_secs_f64(),
            },
        ],
        available_tags: type_tags(&metrics, filter),
    })
}

fn tasks_failure_metric(tags: &[(String, String)]) -> Result<MetricBody, ApiError> {
    let metrics = crate::service::metrics::task_metrics();
    let filter = tag_value(tags, "type")?;
    let failures = match filter {
        Some(task_type) => {
            metrics
                .get(task_type)
                .ok_or_else(|| ApiError::not_found(""))?
                .failures
        }
        None => metrics.values().map(|m| m.failures).sum(),
    };
    Ok(MetricBody {
        name: "komga.tasks.failure",
        description: "Count of failed tasks",
        base_unit: None,
        measurements: vec![MetricMeasurement {
            statistic: "COUNT",
            value: failures as f64,
        }],
        available_tags: type_tags(&metrics, filter),
    })
}

fn type_tags(
    metrics: &std::collections::BTreeMap<&'static str, crate::service::metrics::TaskTypeMetrics>,
    filter: Option<&str>,
) -> Vec<AvailableTag> {
    match filter {
        Some(task_type) => vec![AvailableTag {
            tag: "type",
            values: vec![task_type.to_string()],
        }],
        None if metrics.is_empty() => vec![],
        None => vec![AvailableTag {
            tag: "type",
            values: metrics.keys().map(|k| k.to_string()).collect(),
        }],
    }
}

/// `MetricsPublisherController` gauges. MultiGauge-backed ones carry a `library` tag with one
/// value per library; the plain gauges have no tags.
fn komga_gauge(
    name: &str,
    state: &AppState,
    tags: &[(String, String)],
) -> Result<MetricBody, ApiError> {
    let (description, base_unit, value, available_tags) = match name {
        "komga.libraries" => {
            reject_tags(tags)?;
            (
                "The number of libraries",
                "count",
                count_of(state, "LIBRARY"),
                vec![],
            )
        }
        "komga.collections" => {
            reject_tags(tags)?;
            (
                "The number of collections",
                "count",
                count_of(state, "COLLECTION"),
                vec![],
            )
        }
        "komga.readlists" => {
            reject_tags(tags)?;
            (
                "The number of read lists",
                "count",
                count_of(state, "READLIST"),
                vec![],
            )
        }
        _ if MULTI_GAUGES.contains(&name) => {
            let filter = tag_value(tags, "library")?;
            let rows = multi_gauge_rows(state, name);
            if rows.is_empty() {
                return Err(ApiError::not_found(""));
            }
            let (value, available_tags) = match filter {
                Some(library) => (
                    rows.iter()
                        .find(|(id, _)| id == library)
                        .map(|(_, v)| *v)
                        .ok_or_else(|| ApiError::not_found(""))?,
                    vec![AvailableTag {
                        tag: "library",
                        values: vec![library.to_string()],
                    }],
                ),
                None => (
                    rows.iter().map(|(_, v)| v).sum(),
                    vec![AvailableTag {
                        tag: "library",
                        values: rows.into_iter().map(|(id, _)| id).collect(),
                    }],
                ),
            };
            let (description, base_unit) = match name {
                "komga.series" => ("The number of series", "count"),
                "komga.books" => ("The number of books", "count"),
                "komga.books.filesize" => ("The cumulated filesize of books", "bytes"),
                _ => ("The number of sidecars", "count"),
            };
            (description, base_unit, value, available_tags)
        }
        _ => return Err(ApiError::not_found("")),
    };
    Ok(MetricBody {
        name: name_static(name),
        description,
        base_unit: Some(base_unit),
        measurements: vec![MetricMeasurement {
            statistic: "VALUE",
            value,
        }],
        available_tags,
    })
}

/// Per-library rows of a MultiGauge (`countGroupedByLibraryId` / `getFilesizeGroupedByLibraryId`).
fn multi_gauge_rows(state: &AppState, name: &str) -> Vec<(String, f64)> {
    let sql = match name {
        "komga.series" => "SELECT LIBRARY_ID, COUNT(*) FROM SERIES GROUP BY LIBRARY_ID",
        "komga.books" => "SELECT LIBRARY_ID, COUNT(*) FROM BOOK GROUP BY LIBRARY_ID",
        "komga.books.filesize" => {
            "SELECT LIBRARY_ID, COALESCE(SUM(FILE_SIZE), 0) FROM BOOK GROUP BY LIBRARY_ID"
        }
        "komga.sidecars" => "SELECT LIBRARY_ID, COUNT(*) FROM SIDECAR GROUP BY LIBRARY_ID",
        _ => return vec![],
    };
    let conn = state.db.ro();
    let mut stmt = match conn.prepare(sql) {
        Ok(stmt) => stmt,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as f64))
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
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

fn count_of(state: &AppState, table: &str) -> f64 {
    state
        .db
        .ro()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
            r.get::<_, i64>(0)
        })
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

// region logfile

/// Spring Boot's logfile endpoint (`text/plain`). kmrs logs to stderr and keeps no log file,
/// so the body is always empty; the endpoint exists so the webui's download gets its 200.
async fn get_logfile(auth: RequireAuth) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    Ok(([(CONTENT_TYPE, "text/plain")], "").into_response())
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
            webui_dir: crate::webui::WebuiDir::default(),
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

    fn seed_user(state: &AppState, email: &str, admin: bool, key: &str) -> String {
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
            user_id: user_id.clone(),
            key: crate::auth::sha512_hex(key),
            comment: "test".into(),
            created_date: now_utc(),
            last_modified_date: now_utc(),
        })
        .unwrap();
        user_id
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
    async fn links_shape() {
        let (state, _rx) = test_state();
        let app = test_router(state);
        let (status, headers, bytes) = call(&app, "GET", "/actuator", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), ACTUATOR_JSON);
        let links = &json(&bytes)["_links"];
        for key in [
            "self",
            "health",
            "info",
            "flyway",
            "metrics",
            "metrics-requiredMetricName",
            "scheduledtasks",
            "sessions",
            "shutdown",
        ] {
            assert!(links.get(key).is_some(), "{key}");
        }
        assert_eq!(links["self"]["href"], "http://localhost/actuator");
        assert_eq!(links["self"]["templated"], false);
        assert_eq!(links["sessions"]["templated"], true);
        assert_eq!(
            links["sessions"]["href"],
            "http://localhost/actuator/sessions{?username}"
        );

        // forwarded headers drive the advertised base URL
        let request = Request::builder()
            .method("GET")
            .uri("/actuator")
            .header("X-Forwarded-Proto", "https")
            .header("X-Forwarded-Host", "komga.example.org")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let links = &json(&bytes)["_links"];
        assert_eq!(links["self"]["href"], "https://komga.example.org/actuator");
        assert_eq!(
            links["flyway"]["href"],
            "https://komga.example.org/actuator/flyway"
        );
    }

    #[tokio::test]
    async fn flyway_shape_and_auth() {
        let (state, _rx) = test_state();
        seed_user(&state, "admin@komga.org", true, "k1");
        seed_user(&state, "user@komga.org", false, "k2");
        let app = test_router(state);

        let (status, _headers, _bytes) = call(&app, "GET", "/actuator/flyway", Some("k2")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _headers, _bytes) = call(&app, "GET", "/actuator/flyway", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, headers, bytes) = call(&app, "GET", "/actuator/flyway", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), ACTUATOR_JSON);
        let body = json(&bytes);
        let migrations = body["contexts"]["application"]["flywayBeans"]["flyway"]["migrations"]
            .as_array()
            .unwrap();
        assert!(!migrations.is_empty());
        let first = &migrations[0];
        assert_eq!(first["installedRank"], 1);
        assert_eq!(first["type"], "SQL");
        assert_eq!(first["state"], "SUCCESS");
        assert!(first["script"].as_str().unwrap().starts_with('V'));
        assert!(first["installedOn"].as_str().unwrap().ends_with('Z'));
        assert!(first["description"].is_string());
        assert!(first["executionTime"].is_number());
    }

    #[tokio::test]
    async fn logfile_admin_only_and_empty() {
        let (state, _rx) = test_state();
        seed_user(&state, "admin@komga.org", true, "k1");
        seed_user(&state, "user@komga.org", false, "k2");
        let app = test_router(state);

        let (status, _headers, _bytes) = call(&app, "GET", "/actuator/logfile", Some("k2")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _headers, _bytes) = call(&app, "GET", "/actuator/logfile", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, headers, bytes) = call(&app, "GET", "/actuator/logfile", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "text/plain");
        assert!(bytes.is_empty());
    }

    #[tokio::test]
    async fn sessions_endpoints() {
        let (state, _rx) = test_state();
        let user_id = seed_user(&state, "admin@komga.org", true, "k1");
        let session_id = state.sessions.create(&user_id);
        let app = test_router(state.clone());

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/sessions?username=admin@komga.org",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        let sessions = body["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        let descriptor = &sessions[0];
        assert_eq!(descriptor["id"], session_id);
        assert_eq!(
            descriptor["attributeNames"],
            serde_json::json!(["SPRING_SECURITY_CONTEXT"])
        );
        assert_eq!(descriptor["maxInactiveInterval"], 604_800);
        assert_eq!(descriptor["expired"], false);
        assert!(descriptor["creationTime"].as_str().unwrap().ends_with('Z'));
        assert!(descriptor["lastAccessedTime"]
            .as_str()
            .unwrap()
            .ends_with('Z'));

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/sessions?username=nobody@komga.org",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["sessions"], serde_json::json!([]));

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            &format!("/actuator/sessions/{session_id}"),
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["id"], session_id);

        let (status, _headers, _bytes) =
            call(&app, "GET", "/actuator/sessions/unknown", Some("k1")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // once the session context is replaced by an API-key auth, the principal-name
        // index holds the key hash, not the email (Java's PRINCIPAL_NAME_INDEX_NAME)
        state
            .sessions
            .mark_api_key(&session_id, &user_id, "key1", "hash1");
        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/sessions?username=admin@komga.org",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["sessions"], serde_json::json!([]));
        let (status, _headers, bytes) =
            call(&app, "GET", "/actuator/sessions?username=hash1", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["sessions"].as_array().unwrap().len(), 1);

        let (status, _headers, _bytes) = call(
            &app,
            "DELETE",
            &format!("/actuator/sessions/{session_id}"),
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _headers, _bytes) = call(
            &app,
            "GET",
            &format!("/actuator/sessions/{session_id}"),
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn sessions_require_admin() {
        let (state, _rx) = test_state();
        seed_user(&state, "user@komga.org", false, "k2");
        let app = test_router(state);
        let (status, _headers, _bytes) =
            call(&app, "GET", "/actuator/sessions?username=x", Some("k2")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _headers, _bytes) = call(&app, "GET", "/actuator/sessions", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// A library with one series, one book (1 KiB) and one sidecar.
    fn seed_library_data(state: &AppState) {
        let conn = state.db.rw();
        conn.execute(
            "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES ('lib1', 'L1', 'file:/l1/')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO SERIES (ID, FILE_LAST_MODIFIED, NAME, URL, LIBRARY_ID) \
             VALUES ('s1', '2024-01-01', 'S1', 'file:/l1/s1', 'lib1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO BOOK (ID, FILE_LAST_MODIFIED, NAME, URL, SERIES_ID, LIBRARY_ID, FILE_SIZE) \
             VALUES ('b1', '2024-01-01', 'B1', 'file:/l1/s1/b1.cbz', 's1', 'lib1', 1024)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO SIDECAR (URL, PARENT_URL, LAST_MODIFIED_TIME, LIBRARY_ID) \
             VALUES ('file:/l1/s1/b1.json', 'file:/l1/s1/b1.cbz', '2024-01-01', 'lib1')",
            [],
        )
        .unwrap();
    }

    #[tokio::test]
    async fn metrics_names_and_single_metric() {
        let (state, _rx) = test_state();
        seed_user(&state, "admin@komga.org", true, "k1");
        let app = test_router(state.clone());
        let (status, _headers, bytes) = call(&app, "GET", "/actuator/metrics", Some("k1")).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        // MultiGauge-backed names (komga.series/books/books.filesize/sidecars) vanish without rows
        assert_eq!(
            body["names"],
            serde_json::json!([
                "jvm.memory.used",
                "komga.collections",
                "komga.libraries",
                "komga.readlists",
                "komga.tasks.execution",
                "komga.tasks.failure",
                "process.cpu.usage",
                "process.start.time",
                "process.uptime"
            ])
        );
        for name in [
            "komga.series",
            "komga.books",
            "komga.books.filesize",
            "komga.sidecars",
        ] {
            let (status, _headers, _bytes) = call(
                &app,
                "GET",
                &format!("/actuator/metrics/{name}"),
                Some("k1"),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{name}");
        }

        seed_library_data(&state);
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
                "komga.tasks.execution",
                "komga.tasks.failure",
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

        // MultiGauge-backed metrics carry a `library` tag, aggregated over all rows
        for (name, value) in [
            ("komga.series", 1.0),
            ("komga.books", 1.0),
            ("komga.books.filesize", 1024.0),
            ("komga.sidecars", 1.0),
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
            assert_eq!(body["measurements"][0]["statistic"], "VALUE");
            assert_eq!(body["measurements"][0]["value"], value);
            assert_eq!(
                body["availableTags"],
                serde_json::json!([{"tag": "library", "values": ["lib1"]}])
            );

            let (status, _headers, bytes) = call(
                &app,
                "GET",
                &format!("/actuator/metrics/{name}?tag=library:lib1"),
                Some("k1"),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{name}");
            let body = json(&bytes);
            assert_eq!(body["measurements"][0]["value"], value);
            assert_eq!(
                body["availableTags"],
                serde_json::json!([{"tag": "library", "values": ["lib1"]}])
            );

            // a tag no meter carries → 404, like Spring
            for query in ["tag=library:other", "tag=type:ScanLibrary"] {
                let (status, _headers, _bytes) = call(
                    &app,
                    "GET",
                    &format!("/actuator/metrics/{name}?{query}"),
                    Some("k1"),
                )
                .await;
                assert_eq!(status, StatusCode::NOT_FOUND, "{name}?{query}");
            }
        }

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/metrics/komga.books.filesize",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body["description"], "The cumulated filesize of books");
        assert_eq!(body["baseUnit"], "bytes");

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
    async fn tasks_metrics_shape_and_tag_filter() {
        let (state, _rx) = test_state();
        seed_user(&state, "admin@komga.org", true, "k1");
        let app = test_router(state);

        // a made-up type keeps the assertions deterministic: other tests in this binary
        // record real task types into the same process-global registry
        crate::service::metrics::record_task_execution(
            "NoRealTask",
            std::time::Duration::from_millis(120),
            true,
        );
        crate::service::metrics::record_task_execution(
            "NoRealTask",
            std::time::Duration::ZERO,
            false,
        );

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/metrics/komga.tasks.execution",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body["name"], "komga.tasks.execution");
        assert_eq!(body["description"], "Task execution time");
        assert_eq!(body["baseUnit"], "seconds");
        let statistics: Vec<&str> = body["measurements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["statistic"].as_str().unwrap())
            .collect();
        assert_eq!(statistics, ["COUNT", "TOTAL_TIME", "MAX"]);
        assert!(body["measurements"][0]["value"].as_f64().unwrap() >= 1.0);

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/metrics/komga.tasks.execution?tag=type:NoRealTask",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body["measurements"][0]["value"], 1.0);
        assert_eq!(body["measurements"][1]["value"], 0.12);
        assert_eq!(body["measurements"][2]["value"], 0.12);
        assert_eq!(
            body["availableTags"],
            serde_json::json!([{"tag": "type", "values": ["NoRealTask"]}])
        );

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/metrics/komga.tasks.failure",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&bytes);
        assert_eq!(body["name"], "komga.tasks.failure");
        assert_eq!(body["description"], "Count of failed tasks");
        assert!(body["baseUnit"].is_null());
        assert_eq!(body["measurements"][0]["statistic"], "COUNT");

        let (status, _headers, bytes) = call(
            &app,
            "GET",
            "/actuator/metrics/komga.tasks.failure?tag=type:NoRealTask",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&bytes)["measurements"][0]["value"], 1.0);

        let (status, _headers, _bytes) = call(
            &app,
            "GET",
            "/actuator/metrics/komga.tasks.execution?tag=type:DoesNotExist",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _headers, _bytes) = call(
            &app,
            "GET",
            "/actuator/metrics/komga.tasks.execution?tag=library:lib1",
            Some("k1"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
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
