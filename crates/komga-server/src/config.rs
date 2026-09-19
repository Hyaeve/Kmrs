//! Server configuration: aligned with `KomgaProperties` and the key defaults of application.yml.
//!
//! Precedence, lowest to highest: built-in defaults < TOML file < env vars < CLI flags.
//! The TOML file (`<config-dir>/kmrs.toml` by default) mirrors the original komga property
//! names (`komga.database.file`, `server.port`, `spring.security.oauth2.client.*`, ...).
//! env variable names follow Spring relaxed binding (uppercase, dots to underscores, dashes
//! removed: `komga.database.file` -> `KOMGA_DATABASE_FILE`).

use anyhow::Context;
use clap::Parser;
use komga_db::pool::{DatabaseConfig, JournalMode};
use komga_db::Placeholders;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Default, Parser)]
#[command(
    name = "kmrs",
    version,
    about = "komga-compatible media server, rewritten in Rust"
)]
pub struct Cli {
    /// Path to the TOML configuration file.
    /// Defaults to <config-dir>/kmrs.toml when that file exists.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// Base directory for the database, search index and fonts (komga.config-dir).
    #[arg(long, value_name = "DIR")]
    pub config_dir: Option<PathBuf>,
    /// HTTP listen port (server.port).
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,
}

/// Snapshot of environment variables, taken once so resolution stays hermetic in tests.
type Env = [(String, String)];

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub config_dir: PathBuf,
    pub database_file: PathBuf,
    pub tasks_db_file: PathBuf,
    pub lucene_dir: PathBuf,
    pub fonts_dir: PathBuf,
    pub port: u16,
    pub database: DatabaseConfig,
    pub tasks_db: DatabaseConfig,
    /// server.servlet.session.timeout, defaults to 7 days
    pub session_timeout: Duration,
    pub cors_allowed_origins: Vec<String>,
    pub page_hashing: u32,
    pub epub_divina_letter_count_threshold: usize,
    pub kobo_sync_item_limit: u32,
    pub kepubify_path: Option<PathBuf>,
    /// `server.servlet.context-path` (configurationSource of the settings DTO)
    pub server_context_path: Option<String>,
    pub oauth2: OAuth2Config,
    /// komga.file-hashing / libraries-scan-startup / delete-empty-collections / delete-empty-read-lists,
    /// substituted into the SQL migrations
    pub migration_placeholders: Placeholders,
}

/// OAuth2/OIDC client registrations. When empty, OAuth2 login is disabled (providers endpoint
/// returns an empty list and the authorization/callback endpoints 404, matching
/// `clientRegistrationRepository == null`).
#[derive(Debug, Clone, Default)]
pub struct OAuth2Config {
    pub registrations: Vec<OAuth2ClientRegistration>,
    /// `komga.oauth2-account-creation`, defaults to false
    pub account_creation: bool,
    /// `komga.oidc-email-verification`, defaults to true
    pub oidc_email_verification: bool,
}

#[derive(Debug, Clone)]
pub struct OAuth2ClientRegistration {
    pub registration_id: String,
    pub client_name: Option<String>,
    pub client_id: String,
    pub client_secret: String,
    /// defaults to `authorization_code`
    pub authorization_grant_type: String,
    pub redirect_uri: Option<String>,
    /// explicit scopes; empty means the OIDC defaults (`openid profile email`)
    pub scopes: Vec<String>,
    /// OIDC discovery base (`{issuer}/.well-known/openid-configuration`)
    pub issuer_uri: Option<String>,
    pub authorization_uri: Option<String>,
    pub token_uri: Option<String>,
    pub user_info_uri: Option<String>,
    pub user_name_attribute: Option<String>,
}

impl OAuth2ClientRegistration {
    fn empty(registration_id: String) -> Self {
        Self {
            registration_id,
            client_name: None,
            client_id: String::new(),
            client_secret: String::new(),
            authorization_grant_type: "authorization_code".into(),
            redirect_uri: None,
            scopes: vec![],
            issuer_uri: None,
            authorization_uri: None,
            token_uri: None,
            user_info_uri: None,
            user_name_attribute: None,
        }
    }

    /// Spring's `ClientRegistration.getClientName()`: defaults to the registration id
    pub fn client_name_or_id(&self) -> &str {
        self.client_name.as_deref().unwrap_or(&self.registration_id)
    }

    pub fn is_oidc(&self) -> bool {
        self.issuer_uri.is_some()
    }

    pub fn effective_scopes(&self) -> Vec<String> {
        if !self.scopes.is_empty() {
            self.scopes.clone()
        } else if self.is_oidc() {
            vec!["openid".into(), "profile".into(), "email".into()]
        } else {
            vec![]
        }
    }
}

// ---- TOML file schema ----

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    server: Option<FileServer>,
    komga: Option<FileKomga>,
    spring: Option<FileSpring>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileServer {
    port: Option<u16>,
    /// kmrs always resolves X-Forwarded-* headers (framework strategy)
    forward_headers_strategy: Option<String>,
    /// kmrs always shuts down gracefully
    shutdown: Option<String>,
    servlet: Option<FileServerServlet>,
    error: Option<FileServerError>,
    tomcat: Option<FileServerTomcat>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileServerServlet {
    context_path: Option<String>,
    session: Option<FileServerSession>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileServerSession {
    timeout: Option<ConfigDuration>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
#[allow(dead_code)] // accepted for komga parity, rejected with a warning at resolve time
struct FileServerError {
    include_message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
#[allow(dead_code)] // accepted for komga parity, rejected with a warning at resolve time
struct FileServerTomcat {
    relaxed_query_chars: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileKomga {
    config_dir: Option<PathBuf>,
    page_hashing: Option<u32>,
    epub_divina_letter_count_threshold: Option<usize>,
    oauth2_account_creation: Option<bool>,
    oidc_email_verification: Option<bool>,
    file_hashing: Option<bool>,
    libraries_scan_startup: Option<bool>,
    delete_empty_collections: Option<bool>,
    delete_empty_read_lists: Option<bool>,
    database: Option<FileDatabase>,
    tasks_db: Option<FileDatabase>,
    lucene: Option<FileLucene>,
    fonts: Option<FileFonts>,
    cors: Option<FileCors>,
    kobo: Option<FileKobo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileDatabase {
    file: Option<PathBuf>,
    /// Java batch insert chunking, not applicable to kmrs
    batch_chunk_size: Option<u32>,
    pool_size: Option<u32>,
    max_pool_size: Option<u32>,
    journal_mode: Option<String>,
    busy_timeout: Option<ConfigDuration>,
    pragmas: Option<HashMap<String, String>>,
    /// kmrs does not check whether the database sits on a local filesystem
    check_local_filesystem: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileLucene {
    data_directory: Option<PathBuf>,
    /// tantivy commits are managed by kmrs, not configurable
    commit_delay: Option<ConfigDuration>,
    /// the analyzer chain is fixed at build time; changing it would require a reindex
    index_analyzer: Option<FileIndexAnalyzer>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
#[allow(dead_code)] // accepted for komga parity, rejected with a warning at resolve time
struct FileIndexAnalyzer {
    min_gram: Option<u32>,
    max_gram: Option<u32>,
    preserve_original: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileFonts {
    data_directory: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileCors {
    allowed_origins: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileKobo {
    sync_item_limit: Option<u32>,
    kepubify_path: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSpring {
    security: Option<FileSpringSecurity>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSpringSecurity {
    oauth2: Option<FileSpringOauth2>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSpringOauth2 {
    client: Option<FileSpringOauth2Client>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSpringOauth2Client {
    registration: Option<HashMap<String, FileOauth2Registration>>,
    provider: Option<HashMap<String, FileOauth2Provider>>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileOauth2Registration {
    client_name: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    authorization_grant_type: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FileOauth2Provider {
    issuer_uri: Option<String>,
    authorization_uri: Option<String>,
    token_uri: Option<String>,
    user_info_uri: Option<String>,
    user_name_attribute: Option<String>,
}

/// Duration accepting Spring-style strings ("500ms", "10s", "30m", "1h", "7d");
/// a bare TOML integer means seconds.
#[derive(Debug, Clone, Copy)]
struct ConfigDuration(Duration);

impl<'de> Deserialize<'de> for ConfigDuration {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = ConfigDuration;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a duration string like \"7d\"/\"1h\"/\"30m\"/\"10s\"/\"500ms\", or seconds as an integer")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                parse_duration(v).map(ConfigDuration).map_err(E::custom)
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(ConfigDuration(Duration::from_secs(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                u64::try_from(v)
                    .map(|s| ConfigDuration(Duration::from_secs(s)))
                    .map_err(|_| E::custom(format!("negative duration: {v}")))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    // "ms" must be tried before "s"
    for (suffix, millis) in [
        ("ms", 1u64),
        ("s", 1_000),
        ("m", 60_000),
        ("h", 3_600_000),
        ("d", 86_400_000),
    ] {
        if let Some(num) = s.strip_suffix(suffix) {
            let n: u64 = num
                .trim()
                .parse()
                .map_err(|_| format!("invalid duration {s:?}"))?;
            return Ok(Duration::from_millis(n * millis));
        }
    }
    Err(format!(
        "invalid duration {s:?}: expected a number followed by ms/s/m/h/d"
    ))
}

// ---- resolution ----

impl ServerConfig {
    /// Env vars and defaults only, no config file. Used by tests.
    pub fn from_env() -> Self {
        let env: Vec<(String, String)> = std::env::vars().collect();
        Self::resolve(None, &Cli::default(), &env).expect("resolve config from env")
    }

    pub fn load(cli: &Cli) -> anyhow::Result<Self> {
        let env: Vec<(String, String)> = std::env::vars().collect();
        let file = Self::load_file(cli, &env)?;
        Self::resolve(file.as_ref(), cli, &env)
    }

    fn load_file(cli: &Cli, env: &Env) -> anyhow::Result<Option<FileConfig>> {
        let path = match &cli.config {
            Some(p) => Some(p.clone()),
            None => {
                let dir = cli
                    .config_dir
                    .clone()
                    .or_else(|| env_path(env, "KOMGA_CONFIG_DIR"))
                    .or_else(|| env_path(env, "KOMGA_CONFIGDIR"))
                    .unwrap_or_else(default_config_dir);
                let candidate = dir.join("kmrs.toml");
                candidate.exists().then_some(candidate)
            }
        };
        match path {
            Some(p) => {
                let text = std::fs::read_to_string(&p)
                    .with_context(|| format!("read config file {}", p.display()))?;
                let parsed: FileConfig = toml::from_str(&text)
                    .with_context(|| format!("parse config file {}", p.display()))?;
                tracing::info!("loaded configuration from {}", p.display());
                Ok(Some(parsed))
            }
            None => Ok(None),
        }
    }

    fn resolve(file: Option<&FileConfig>, cli: &Cli, env: &Env) -> anyhow::Result<Self> {
        let mut warnings: Vec<String> = vec![];
        let server = file.and_then(|f| f.server.as_ref());
        let komga = file.and_then(|f| f.komga.as_ref());

        let config_dir = cli
            .config_dir
            .clone()
            .or_else(|| env_path(env, "KOMGA_CONFIG_DIR"))
            .or_else(|| env_path(env, "KOMGA_CONFIGDIR"))
            .or_else(|| komga.and_then(|k| k.config_dir.clone()))
            .unwrap_or_else(default_config_dir);

        let database = merge_database(
            komga.and_then(|k| k.database.as_ref()),
            env,
            "KOMGA_DATABASE",
            config_dir.join("database.sqlite"),
            true,
            "komga.database",
            &mut warnings,
        )?;
        let tasks_db = merge_database(
            komga.and_then(|k| k.tasks_db.as_ref()),
            env,
            "KOMGA_TASKSDB",
            config_dir.join("tasks.sqlite"),
            false,
            "komga.tasks-db",
            &mut warnings,
        )?;

        let port = cli
            .port
            .map(u32::from)
            .or_else(|| env_u32(env, "SERVER_PORT"))
            .or_else(|| server.and_then(|s| s.port).map(u32::from))
            .unwrap_or(25600);
        let port = u16::try_from(port).context("server.port out of range")?;

        let session_timeout = env_duration(env, "SERVER_SERVLET_SESSION_TIMEOUT")
            .transpose()?
            .or_else(|| {
                server
                    .and_then(|s| s.servlet.as_ref())
                    .and_then(|s| s.session.as_ref())
                    .and_then(|s| s.timeout)
                    .map(|d| d.0)
            })
            .unwrap_or(Duration::from_secs(7 * 24 * 3600));

        let server_context_path = env_string(env, "SERVER_SERVLET_CONTEXT_PATH")
            .or_else(|| {
                server
                    .and_then(|s| s.servlet.as_ref())
                    .and_then(|s| s.context_path.clone())
            })
            .filter(|v| !v.is_empty());

        if let Some(strategy) = server.and_then(|s| s.forward_headers_strategy.as_deref()) {
            if !strategy.eq_ignore_ascii_case("framework") {
                warnings.push(format!(
                    "server.forward-headers-strategy={strategy:?}: kmrs always applies the framework strategy"
                ));
            }
        }
        if let Some(shutdown) = server.and_then(|s| s.shutdown.as_deref()) {
            if !shutdown.eq_ignore_ascii_case("graceful") {
                warnings.push(format!(
                    "server.shutdown={shutdown:?}: kmrs always shuts down gracefully"
                ));
            }
        }
        if server.and_then(|s| s.error.as_ref()).is_some() {
            warnings.push("server.error.include-message: not supported by kmrs".into());
        }
        if server.and_then(|s| s.tomcat.as_ref()).is_some() {
            warnings.push("server.tomcat.*: tomcat-specific, not supported by kmrs".into());
        }

        let lucene = komga.and_then(|k| k.lucene.as_ref());
        if lucene.and_then(|l| l.commit_delay).is_some() {
            warnings.push("komga.lucene.commit-delay: not supported by kmrs".into());
        }
        if lucene.and_then(|l| l.index_analyzer.as_ref()).is_some() {
            warnings.push("komga.lucene.index-analyzer.*: not supported by kmrs".into());
        }

        for warning in &warnings {
            tracing::warn!("{warning}");
        }

        Ok(Self {
            database_file: database.file.clone(),
            tasks_db_file: tasks_db.file.clone(),
            lucene_dir: env_path(env, "KOMGA_LUCENE_DATA_DIRECTORY")
                .or_else(|| lucene.and_then(|l| l.data_directory.clone()))
                .unwrap_or_else(|| config_dir.join("lucene")),
            fonts_dir: env_path(env, "KOMGA_FONTS_DATA_DIRECTORY")
                .or_else(|| {
                    komga
                        .and_then(|k| k.fonts.as_ref())
                        .and_then(|f| f.data_directory.clone())
                })
                .unwrap_or_else(|| config_dir.join("fonts")),
            config_dir,
            port,
            database,
            tasks_db,
            session_timeout,
            cors_allowed_origins: env_list(env, "KOMGA_CORS_ALLOWEDORIGINS")
                .or_else(|| {
                    komga
                        .and_then(|k| k.cors.as_ref())
                        .and_then(|c| c.allowed_origins.clone())
                })
                .unwrap_or_default(),
            page_hashing: env_u32(env, "KOMGA_PAGEHASHING")
                .or_else(|| komga.and_then(|k| k.page_hashing))
                .unwrap_or(3),
            epub_divina_letter_count_threshold: env_u32(
                env,
                "KOMGA_EPUBDIVINALETTERCOUNTTHRESHOLD",
            )
            .map(|v| v as usize)
            .or_else(|| komga.and_then(|k| k.epub_divina_letter_count_threshold))
            .unwrap_or(15),
            kobo_sync_item_limit: env_u32(env, "KOMGA_KOBO_SYNCITEMLIMIT")
                .or_else(|| {
                    komga
                        .and_then(|k| k.kobo.as_ref())
                        .and_then(|k| k.sync_item_limit)
                })
                .unwrap_or(100),
            kepubify_path: env_path(env, "KOMGA_KOBO_KEPUBIFY_PATH").or_else(|| {
                komga
                    .and_then(|k| k.kobo.as_ref())
                    .and_then(|k| k.kepubify_path.clone())
            }),
            server_context_path,
            oauth2: merge_oauth2(file.and_then(|f| f.spring.as_ref()), komga, env),
            migration_placeholders: Placeholders {
                library_file_hashing: env_bool(env, "KOMGA_FILEHASHING")
                    .or_else(|| komga.and_then(|k| k.file_hashing))
                    .unwrap_or(true),
                library_scan_startup: env_bool(env, "KOMGA_LIBRARIESSCANSTARTUP")
                    .or_else(|| komga.and_then(|k| k.libraries_scan_startup))
                    .unwrap_or(false),
                delete_empty_collections: env_bool(env, "KOMGA_DELETEEMPTYCOLLECTIONS")
                    .or_else(|| komga.and_then(|k| k.delete_empty_collections))
                    .unwrap_or(true),
                delete_empty_read_lists: env_bool(env, "KOMGA_DELETEEMPTYREADLISTS")
                    .or_else(|| komga.and_then(|k| k.delete_empty_read_lists))
                    .unwrap_or(true),
            },
        })
    }
}

fn merge_database(
    file: Option<&FileDatabase>,
    env: &Env,
    env_prefix: &str,
    default_file: PathBuf,
    register_udfs: bool,
    toml_path: &str,
    warnings: &mut Vec<String>,
) -> anyhow::Result<DatabaseConfig> {
    let journal_mode = env_string(env, &format!("{env_prefix}_JOURNALMODE"))
        .or_else(|| file.and_then(|d| d.journal_mode.clone()))
        .map(|mode| parse_journal_mode(&mode))
        .transpose()?
        .unwrap_or_default();
    if let Some(d) = file {
        if d.batch_chunk_size.is_some() {
            warnings.push(format!(
                "{toml_path}.batch-chunk-size: not supported by kmrs"
            ));
        }
        if d.check_local_filesystem.is_some() {
            warnings.push(format!(
                "{toml_path}.check-local-filesystem: not supported by kmrs"
            ));
        }
    }
    let mut pragmas: Vec<(String, String)> = file
        .and_then(|d| d.pragmas.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    pragmas.sort();
    Ok(DatabaseConfig {
        file: env_path(env, &format!("{env_prefix}_FILE"))
            .or_else(|| file.and_then(|d| d.file.clone()))
            .unwrap_or(default_file),
        pool_size: env_u32(env, &format!("{env_prefix}_POOLSIZE"))
            .or_else(|| file.and_then(|d| d.pool_size)),
        max_pool_size: env_u32(env, &format!("{env_prefix}_MAXPOOLSIZE"))
            .or_else(|| file.and_then(|d| d.max_pool_size))
            .unwrap_or(1),
        journal_mode,
        busy_timeout: env_duration(env, &format!("{env_prefix}_BUSYTIMEOUT"))
            .transpose()?
            .or_else(|| file.and_then(|d| d.busy_timeout).map(|d| d.0)),
        pragmas,
        register_udfs,
    })
}

fn parse_journal_mode(mode: &str) -> anyhow::Result<JournalMode> {
    match mode.to_ascii_uppercase().as_str() {
        "WAL" => Ok(JournalMode::Wal),
        "DELETE" => Ok(JournalMode::Delete),
        "TRUNCATE" => Ok(JournalMode::Truncate),
        "PERSIST" => Ok(JournalMode::Persist),
        "MEMORY" => Ok(JournalMode::Memory),
        "OFF" => Ok(JournalMode::Off),
        other => anyhow::bail!("invalid journal-mode {other:?}"),
    }
}

/// File registrations come first, env vars override individual fields on top
/// (same-id entries merge, matching Spring's property-source precedence).
fn merge_oauth2(spring: Option<&FileSpring>, komga: Option<&FileKomga>, env: &Env) -> OAuth2Config {
    const REG_PREFIX: &str = "SPRING_SECURITY_OAUTH2_CLIENT_REGISTRATION_";
    const PROV_PREFIX: &str = "SPRING_SECURITY_OAUTH2_CLIENT_PROVIDER_";

    let client = spring
        .and_then(|s| s.security.as_ref())
        .and_then(|s| s.oauth2.as_ref())
        .and_then(|o| o.client.as_ref());

    let mut registrations: HashMap<String, OAuth2ClientRegistration> = HashMap::new();
    let mut providers: HashMap<String, FileOauth2Provider> = HashMap::new();

    if let Some(client) = client {
        for (id, reg) in client.registration.clone().unwrap_or_default() {
            let entry = registrations
                .entry(id.clone())
                .or_insert_with(|| OAuth2ClientRegistration::empty(id));
            if let Some(v) = reg.client_name {
                entry.client_name = Some(v);
            }
            if let Some(v) = reg.client_id {
                entry.client_id = v;
            }
            if let Some(v) = reg.client_secret {
                entry.client_secret = v;
            }
            if let Some(v) = reg.authorization_grant_type {
                entry.authorization_grant_type = v;
            }
            if let Some(v) = reg.redirect_uri {
                entry.redirect_uri = Some(v);
            }
            if let Some(v) = reg.scope {
                entry.scopes = v;
            }
        }
        providers.extend(client.provider.clone().unwrap_or_default());
    }

    for (key, value) in env {
        if let Some(rest) = key.strip_prefix(REG_PREFIX) {
            let Some((id, field)) = rest.rsplit_once('_') else {
                continue;
            };
            // Spring lowercases map keys bound from env vars; the id must match the TOML/yaml form
            let id = id.to_ascii_lowercase();
            let reg = registrations
                .entry(id.clone())
                .or_insert_with(|| OAuth2ClientRegistration::empty(id));
            match field {
                "CLIENT-NAME" => reg.client_name = Some(value.clone()),
                "CLIENT-ID" => reg.client_id = value.clone(),
                "CLIENT-SECRET" => reg.client_secret = value.clone(),
                "AUTHORIZATION-GRANT-TYPE" => reg.authorization_grant_type = value.clone(),
                "REDIRECT-URI" => reg.redirect_uri = Some(value.clone()),
                "SCOPE" => {
                    reg.scopes = value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                }
                _ => {}
            }
        } else if let Some(rest) = key.strip_prefix(PROV_PREFIX) {
            let Some((id, field)) = rest.rsplit_once('_') else {
                continue;
            };
            let provider = providers.entry(id.to_ascii_lowercase()).or_default();
            match field {
                "ISSUER-URI" => provider.issuer_uri = Some(value.clone()),
                "AUTHORIZATION-URI" => provider.authorization_uri = Some(value.clone()),
                "TOKEN-URI" => provider.token_uri = Some(value.clone()),
                "USER-INFO-URI" => provider.user_info_uri = Some(value.clone()),
                "USER-NAME-ATTRIBUTE" => provider.user_name_attribute = Some(value.clone()),
                _ => {}
            }
        }
    }

    for (id, provider) in providers {
        let Some(reg) = registrations.get_mut(&id) else {
            continue;
        };
        if reg.issuer_uri.is_none() {
            reg.issuer_uri = provider.issuer_uri;
        }
        if reg.authorization_uri.is_none() {
            reg.authorization_uri = provider.authorization_uri;
        }
        if reg.token_uri.is_none() {
            reg.token_uri = provider.token_uri;
        }
        if reg.user_info_uri.is_none() {
            reg.user_info_uri = provider.user_info_uri;
        }
        if reg.user_name_attribute.is_none() {
            reg.user_name_attribute = provider.user_name_attribute;
        }
    }

    let mut registrations: Vec<OAuth2ClientRegistration> = registrations
        .into_values()
        .filter(|r| !r.client_id.is_empty())
        .collect();
    registrations.sort_by(|a, b| a.registration_id.cmp(&b.registration_id));

    OAuth2Config {
        registrations,
        account_creation: env_bool(env, "KOMGA_OAUTH2ACCOUNTCREATION")
            .or_else(|| komga.and_then(|k| k.oauth2_account_creation))
            .unwrap_or(false),
        oidc_email_verification: env_bool(env, "KOMGA_OIDCMAILVERIFICATION")
            .or_else(|| komga.and_then(|k| k.oidc_email_verification))
            .unwrap_or(true),
    }
}

// ---- env helpers ----

fn env_get<'a>(env: &'a Env, key: &str) -> Option<&'a str> {
    env.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

fn env_bool(env: &Env, key: &str) -> Option<bool> {
    env_get(env, key).map(|v| v.eq_ignore_ascii_case("true"))
}

fn env_string(env: &Env, key: &str) -> Option<String> {
    env_get(env, key).map(str::to_string)
}

fn env_u32(env: &Env, key: &str) -> Option<u32> {
    env_get(env, key).and_then(|v| v.parse().ok())
}

fn env_duration(env: &Env, key: &str) -> Option<anyhow::Result<Duration>> {
    env_get(env, key).map(|v| parse_duration(v).map_err(anyhow::Error::msg))
}

fn default_config_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".komga")
}

fn env_path(env: &Env, key: &str) -> Option<PathBuf> {
    env_get(env, key).map(PathBuf::from)
}

fn env_list(env: &Env, key: &str) -> Option<Vec<String>> {
    env_get(env, key).map(|v| {
        v.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn resolve(toml: &str, cli: Cli, env: &Env) -> ServerConfig {
        let file: FileConfig = toml::from_str(toml).unwrap();
        ServerConfig::resolve(Some(&file), &cli, env).unwrap()
    }

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("10s").unwrap(), Duration::from_secs(10));
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(
            parse_duration("7d").unwrap(),
            Duration::from_secs(7 * 86400)
        );
        assert!(parse_duration("7").is_err());
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn defaults_without_file() {
        let config = ServerConfig::resolve(None, &Cli::default(), &[]).unwrap();
        assert_eq!(config.port, 25600);
        assert_eq!(config.page_hashing, 3);
        assert_eq!(config.epub_divina_letter_count_threshold, 15);
        assert_eq!(config.kobo_sync_item_limit, 100);
        assert_eq!(config.session_timeout, Duration::from_secs(7 * 24 * 3600));
        assert!(config.oauth2.registrations.is_empty());
        assert!(!config.oauth2.account_creation);
        assert!(config.oauth2.oidc_email_verification);
        assert!(config.migration_placeholders.library_file_hashing);
        assert!(!config.migration_placeholders.library_scan_startup);
        assert_eq!(config.database.max_pool_size, 1);
        assert!(matches!(config.database.journal_mode, JournalMode::Wal));
        let home = default_config_dir();
        assert_eq!(config.database_file, home.join("database.sqlite"));
        assert_eq!(config.lucene_dir, home.join("lucene"));
    }

    #[test]
    fn file_values_apply_and_derive_from_config_dir() {
        let config = resolve(
            r#"
[komga]
config-dir = "/data/komga"
page-hashing = 5
delete-empty-collections = false

[komga.cors]
allowed-origins = ["https://a.example", "https://b.example"]

[komga.kobo]
sync-item-limit = 50
kepubify-path = "/usr/local/bin/kepubify"
"#,
            Cli::default(),
            &[],
        );
        assert_eq!(config.config_dir, PathBuf::from("/data/komga"));
        assert_eq!(
            config.database_file,
            PathBuf::from("/data/komga/database.sqlite")
        );
        assert_eq!(config.lucene_dir, PathBuf::from("/data/komga/lucene"));
        assert_eq!(config.fonts_dir, PathBuf::from("/data/komga/fonts"));
        assert_eq!(config.page_hashing, 5);
        assert!(!config.migration_placeholders.delete_empty_collections);
        assert_eq!(
            config.cors_allowed_origins,
            vec![
                "https://a.example".to_string(),
                "https://b.example".to_string()
            ]
        );
        assert_eq!(config.kobo_sync_item_limit, 50);
        assert_eq!(
            config.kepubify_path,
            Some(PathBuf::from("/usr/local/bin/kepubify"))
        );
    }

    #[test]
    fn precedence_file_env_cli() {
        let cli = Cli {
            config: None,
            config_dir: None,
            port: Some(9000),
        };
        let config = resolve(
            "[server]\nport = 8000\n",
            cli,
            &env(&[("SERVER_PORT", "7000")]),
        );
        assert_eq!(config.port, 9000);
        let config = resolve(
            "[server]\nport = 8000\n",
            Cli::default(),
            &env(&[("SERVER_PORT", "7000")]),
        );
        assert_eq!(config.port, 7000);
        let config = resolve("[server]\nport = 8000\n", Cli::default(), &[]);
        assert_eq!(config.port, 8000);
    }

    #[test]
    fn database_tuning_from_file_and_env() {
        let config = resolve(
            r#"
[komga.database]
journal-mode = "delete"
pool-size = 4
max-pool-size = 8
busy-timeout = "30s"
pragmas = { cache_size = "-2000", synchronous = "NORMAL" }
"#,
            Cli::default(),
            &env(&[("KOMGA_DATABASE_JOURNALMODE", "wal")]),
        );
        assert!(matches!(config.database.journal_mode, JournalMode::Wal));
        assert_eq!(config.database.pool_size, Some(4));
        assert_eq!(config.database.max_pool_size, 8);
        assert_eq!(config.database.busy_timeout, Some(Duration::from_secs(30)));
        assert_eq!(
            config.database.pragmas,
            vec![
                ("cache_size".to_string(), "-2000".to_string()),
                ("synchronous".to_string(), "NORMAL".to_string())
            ]
        );
        assert!(config.database.register_udfs);
        assert!(!config.tasks_db.register_udfs);
    }

    #[test]
    fn oauth2_file_plus_env_merge() {
        let config = resolve(
            r#"
[komga]
oauth2-account-creation = true

[spring.security.oauth2.client.registration.github]
client-id = "file-id"
client-secret = "file-secret"
client-name = "GitHub"
scope = ["read:user"]

[spring.security.oauth2.client.provider.github]
issuer-uri = "https://github.com"
"#,
            Cli::default(),
            &env(&[
                (
                    "SPRING_SECURITY_OAUTH2_CLIENT_REGISTRATION_GITHUB_CLIENT-SECRET",
                    "env-secret",
                ),
                (
                    "SPRING_SECURITY_OAUTH2_CLIENT_PROVIDER_GITHUB_TOKEN-URI",
                    "https://github.com/token",
                ),
                (
                    "SPRING_SECURITY_OAUTH2_CLIENT_REGISTRATION_OKTA_CLIENT-ID",
                    "okta-id",
                ),
                (
                    "SPRING_SECURITY_OAUTH2_CLIENT_PROVIDER_OKTA_ISSUER-URI",
                    "https://okta.example",
                ),
            ]),
        );
        assert!(config.oauth2.account_creation);
        assert_eq!(config.oauth2.registrations.len(), 2);
        let github = &config.oauth2.registrations[0];
        assert_eq!(github.registration_id, "github");
        assert_eq!(github.client_id, "file-id");
        assert_eq!(github.client_secret, "env-secret");
        assert_eq!(github.client_name_or_id(), "GitHub");
        assert_eq!(github.scopes, vec!["read:user".to_string()]);
        assert_eq!(github.issuer_uri.as_deref(), Some("https://github.com"));
        assert_eq!(
            github.token_uri.as_deref(),
            Some("https://github.com/token")
        );
        let okta = &config.oauth2.registrations[1];
        assert_eq!(okta.registration_id, "okta");
        assert!(okta.is_oidc());
        assert_eq!(
            okta.effective_scopes(),
            vec![
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string()
            ]
        );
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = toml::from_str::<FileConfig>("[komga]\npage-hshing = 5\n").unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn session_timeout_from_file() {
        let config = resolve(
            "[server.servlet.session]\ntimeout = \"12h\"\n",
            Cli::default(),
            &[],
        );
        assert_eq!(config.session_timeout, Duration::from_secs(12 * 3600));
        let config = resolve(
            "[server.servlet.session]\ntimeout = \"12h\"\n",
            Cli::default(),
            &env(&[("SERVER_SERVLET_SESSION_TIMEOUT", "1d")]),
        );
        assert_eq!(config.session_timeout, Duration::from_secs(86400));
    }
}
