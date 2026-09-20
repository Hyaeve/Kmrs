//! Server configuration.
//!
//! The configuration file is always `<config-dir>/config.toml`; the config dir itself comes
//! from `--config-dir` / `KOMGA_CONFIG_DIR` / `KOMGA_CONFIGDIR`, defaulting to `~/.komga`.
//! On first start the file is generated from the built-in defaults, carrying over values
//! from the Java komga's `application.yml`/`application.yaml` found in the same
//! directory — see the `java` module.
//!
//! Precedence, lowest to highest: built-in defaults < TOML file < env vars < CLI flags.
//! env variable names follow Spring relaxed binding (`database.file` -> `KOMGA_DATABASE_FILE`),
//! so existing komga deployments keep working.

mod file;
mod java;

use anyhow::Context;
use clap::Parser;
use file::{FileConfig, FileDatabase, FileOAuth2};
use komga_db::pool::{DatabaseConfig, JournalMode};
use komga_db::Placeholders;
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
    /// Base directory for config.toml, the database, search index and fonts.
    #[arg(long, value_name = "DIR")]
    pub config_dir: Option<PathBuf>,
    /// HTTP listen port (overrides server.port).
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,
}

/// Snapshot of environment variables, taken once so resolution stays hermetic in tests.
type Env = [(String, String)];

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub config_dir: PathBuf,
    pub lucene_dir: PathBuf,
    pub fonts_dir: PathBuf,
    pub port: u16,
    pub database: DatabaseConfig,
    pub tasks_db: DatabaseConfig,
    /// defaults to 7 days
    pub session_timeout: Duration,
    pub cors_allowed_origins: Vec<String>,
    pub page_hashing: u32,
    pub epub_divina_letter_count_threshold: usize,
    pub kobo_sync_item_limit: u32,
    pub kepubify_path: Option<PathBuf>,
    /// configurationSource of the settings DTO
    pub server_context_path: Option<String>,
    pub oauth2: OAuth2Config,
    /// substituted into the SQL migrations
    pub migration_placeholders: Placeholders,
}

/// OAuth2/OIDC client registrations. When empty, OAuth2 login is disabled (providers endpoint
/// returns an empty list and the authorization/callback endpoints 404, matching
/// `clientRegistrationRepository == null`).
#[derive(Debug, Clone, Default)]
pub struct OAuth2Config {
    pub registrations: Vec<OAuth2ClientRegistration>,
    pub account_creation: bool,
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

// ---- resolution ----

impl ServerConfig {
    /// Env vars and defaults only, no config file. Used by tests.
    pub fn from_env() -> Self {
        let env: Vec<(String, String)> = std::env::vars().collect();
        Self::resolve(None, &Cli::default(), &env).expect("resolve config from env")
    }

    pub fn load(cli: &Cli) -> anyhow::Result<Self> {
        let env: Vec<(String, String)> = std::env::vars().collect();
        Self::load_with(cli, &env)
    }

    fn load_with(cli: &Cli, env: &Env) -> anyhow::Result<Self> {
        let config_dir = cli
            .config_dir
            .clone()
            .or_else(|| env_path(env, "KOMGA_CONFIG_DIR"))
            .or_else(|| env_path(env, "KOMGA_CONFIGDIR"))
            .unwrap_or_else(default_config_dir);
        let path = config_dir.join("config.toml");
        let file = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read config file {}", path.display()))?;
            let parsed: FileConfig = toml::from_str(&text)
                .with_context(|| format!("parse config file {}", path.display()))?;
            tracing::info!("loaded configuration from {}", path.display());
            parsed
        } else {
            let migrated = java::migrate(&config_dir);
            let source = migrated.as_ref().map(|(_, p)| p.clone());
            let file = migrated.map(|(f, _)| f).unwrap_or_default();
            // env vars and CLI flags are runtime overrides; the generated file must
            // reflect only defaults and migrated values
            let render_cli = Cli {
                config_dir: Some(config_dir.clone()),
                port: None,
            };
            let rendered = Self::resolve(Some(&file), &render_cli, &[])?;
            match std::fs::create_dir_all(&config_dir).and_then(|()| {
                std::fs::write(&path, file::render(&file, &rendered, source.as_deref()))
            }) {
                Ok(()) => match &source {
                    Some(p) => tracing::info!(
                        "wrote configuration to {}, migrated from {}",
                        path.display(),
                        p.display()
                    ),
                    None => {
                        tracing::info!("wrote default configuration to {}", path.display())
                    }
                },
                Err(e) => tracing::warn!("could not write {}: {e:#}", path.display()),
            }
            file
        };
        Self::resolve(Some(&file), cli, env)
    }

    fn resolve(file: Option<&FileConfig>, cli: &Cli, env: &Env) -> anyhow::Result<Self> {
        let server = file.and_then(|f| f.server.as_ref());

        let config_dir = cli
            .config_dir
            .clone()
            .or_else(|| env_path(env, "KOMGA_CONFIG_DIR"))
            .or_else(|| env_path(env, "KOMGA_CONFIGDIR"))
            .unwrap_or_else(default_config_dir);

        let database = merge_database(
            file.and_then(|f| f.database.as_ref()),
            env,
            "KOMGA_DATABASE",
            config_dir.join("database.sqlite"),
            true,
        )?;
        let tasks_db = merge_database(
            file.and_then(|f| f.tasks_db.as_ref()),
            env,
            "KOMGA_TASKSDB",
            config_dir.join("tasks.sqlite"),
            false,
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
            .or_else(|| server.and_then(|s| s.session_timeout).map(|d| d.0))
            .unwrap_or(Duration::from_secs(7 * 24 * 3600));

        let server_context_path = env_string(env, "SERVER_SERVLET_CONTEXT_PATH")
            .or_else(|| server.and_then(|s| s.context_path.clone()))
            .filter(|v| !v.is_empty());

        Ok(Self {
            lucene_dir: env_path(env, "KOMGA_LUCENE_DATADIRECTORY")
                .or_else(|| {
                    file.and_then(|f| f.search.as_ref())
                        .and_then(|s| s.data_directory.clone())
                })
                .unwrap_or_else(|| config_dir.join("lucene")),
            fonts_dir: env_path(env, "KOMGA_FONTS_DATADIRECTORY")
                .or_else(|| {
                    file.and_then(|f| f.fonts.as_ref())
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
                    file.and_then(|f| f.cors.as_ref())
                        .and_then(|c| c.allowed_origins.clone())
                })
                .unwrap_or_default(),
            page_hashing: env_u32(env, "KOMGA_PAGEHASHING")
                .or_else(|| {
                    file.and_then(|f| f.books.as_ref())
                        .and_then(|b| b.page_hashing)
                })
                .unwrap_or(3),
            epub_divina_letter_count_threshold: env_u32(
                env,
                "KOMGA_EPUBDIVINALETTERCOUNTTHRESHOLD",
            )
            .map(|v| v as usize)
            .or_else(|| {
                file.and_then(|f| f.books.as_ref())
                    .and_then(|b| b.epub_divina_letter_count_threshold)
            })
            .unwrap_or(15),
            kobo_sync_item_limit: env_u32(env, "KOMGA_KOBO_SYNCITEMLIMIT")
                .or_else(|| {
                    file.and_then(|f| f.kobo.as_ref())
                        .and_then(|k| k.sync_item_limit)
                })
                .unwrap_or(100),
            kepubify_path: env_path(env, "KOMGA_KOBO_KEPUBIFYPATH").or_else(|| {
                file.and_then(|f| f.kobo.as_ref())
                    .and_then(|k| k.kepubify_path.clone())
            }),
            server_context_path,
            oauth2: merge_oauth2(file.and_then(|f| f.oauth2.as_ref()), env),
            migration_placeholders: Placeholders {
                library_file_hashing: env_bool(env, "KOMGA_FILEHASHING")
                    .or_else(|| {
                        file.and_then(|f| f.libraries.as_ref())
                            .and_then(|l| l.file_hashing)
                    })
                    .unwrap_or(true),
                library_scan_startup: env_bool(env, "KOMGA_LIBRARIESSCANSTARTUP")
                    .or_else(|| {
                        file.and_then(|f| f.libraries.as_ref())
                            .and_then(|l| l.scan_on_startup)
                    })
                    .unwrap_or(false),
                delete_empty_collections: env_bool(env, "KOMGA_DELETEEMPTYCOLLECTIONS")
                    .or_else(|| {
                        file.and_then(|f| f.libraries.as_ref())
                            .and_then(|l| l.delete_empty_collections)
                    })
                    .unwrap_or(true),
                delete_empty_read_lists: env_bool(env, "KOMGA_DELETEEMPTYREADLISTS")
                    .or_else(|| {
                        file.and_then(|f| f.libraries.as_ref())
                            .and_then(|l| l.delete_empty_read_lists)
                    })
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
) -> anyhow::Result<DatabaseConfig> {
    let journal_mode = env_string(env, &format!("{env_prefix}_JOURNALMODE"))
        .or_else(|| file.and_then(|d| d.journal_mode.clone()))
        .map(|mode| parse_journal_mode(&mode))
        .transpose()?
        .unwrap_or_default();
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
fn merge_oauth2(file: Option<&FileOAuth2>, env: &Env) -> OAuth2Config {
    const REG_PREFIX: &str = "SPRING_SECURITY_OAUTH2_CLIENT_REGISTRATION_";
    const PROV_PREFIX: &str = "SPRING_SECURITY_OAUTH2_CLIENT_PROVIDER_";

    #[derive(Default)]
    struct EnvProvider {
        issuer_uri: Option<String>,
        authorization_uri: Option<String>,
        token_uri: Option<String>,
        user_info_uri: Option<String>,
        user_name_attribute: Option<String>,
    }

    let mut registrations: HashMap<String, OAuth2ClientRegistration> = HashMap::new();
    let mut providers: HashMap<String, EnvProvider> = HashMap::new();

    if let Some(file) = file {
        for (id, reg) in file.registrations.clone().unwrap_or_default() {
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
            if let Some(v) = reg.scopes {
                entry.scopes = v;
            }
            if let Some(v) = reg.issuer_uri {
                entry.issuer_uri = Some(v);
            }
            if let Some(v) = reg.authorization_uri {
                entry.authorization_uri = Some(v);
            }
            if let Some(v) = reg.token_uri {
                entry.token_uri = Some(v);
            }
            if let Some(v) = reg.user_info_uri {
                entry.user_info_uri = Some(v);
            }
            if let Some(v) = reg.user_name_attribute {
                entry.user_name_attribute = Some(v);
            }
        }
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
            .or_else(|| file.and_then(|f| f.account_creation))
            .unwrap_or(false),
        oidc_email_verification: env_bool(env, "KOMGA_OIDCMAILVERIFICATION")
            .or_else(|| file.and_then(|f| f.oidc_email_verification))
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
    env_get(env, key).map(|v| file::parse_duration(v).map_err(anyhow::Error::msg))
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
        assert_eq!(config.database.file, home.join("database.sqlite"));
        assert_eq!(config.lucene_dir, home.join("lucene"));
    }

    #[test]
    fn file_values_apply_and_derive_from_config_dir() {
        let cli = Cli {
            config_dir: Some(PathBuf::from("/data/komga")),
            port: None,
        };
        let config = resolve(
            r#"
[books]
page-hashing = 5

[libraries]
delete-empty-collections = false

[cors]
allowed-origins = ["https://a.example", "https://b.example"]

[kobo]
sync-item-limit = 50
kepubify-path = "/usr/local/bin/kepubify"
"#,
            cli,
            &[],
        );
        assert_eq!(config.config_dir, PathBuf::from("/data/komga"));
        assert_eq!(
            config.database.file,
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
    fn kepubify_path_from_env() {
        // Spring canonical form (`komga.kobo.kepubify-path`, dashes removed)
        let config = resolve(
            "",
            Cli::default(),
            &env(&[("KOMGA_KOBO_KEPUBIFYPATH", "/opt/kepubify")]),
        );
        assert_eq!(
            config.kepubify_path,
            Some(std::path::PathBuf::from("/opt/kepubify"))
        );
    }

    #[test]
    fn data_directories_from_env() {
        let config = resolve(
            "",
            Cli::default(),
            &env(&[
                ("KOMGA_LUCENE_DATADIRECTORY", "/lucene"),
                ("KOMGA_FONTS_DATADIRECTORY", "/fonts"),
            ]),
        );
        assert_eq!(config.lucene_dir, std::path::PathBuf::from("/lucene"));
        assert_eq!(config.fonts_dir, std::path::PathBuf::from("/fonts"));
    }

    #[test]
    fn database_tuning_from_file_and_env() {
        let config = resolve(
            r#"
[database]
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
[oauth2]
account-creation = true

[oauth2.registrations.github]
client-id = "file-id"
client-secret = "file-secret"
client-name = "GitHub"
scopes = ["read:user"]
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
        let err = toml::from_str::<FileConfig>("[books]\npage-hshing = 5\n").unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn session_timeout_from_file() {
        let config = resolve("[server]\nsession-timeout = \"12h\"\n", Cli::default(), &[]);
        assert_eq!(config.session_timeout, Duration::from_secs(12 * 3600));
        let config = resolve(
            "[server]\nsession-timeout = \"12h\"\n",
            Cli::default(),
            &env(&[("SERVER_SERVLET_SESSION_TIMEOUT", "1d")]),
        );
        assert_eq!(config.session_timeout, Duration::from_secs(86400));
    }

    #[test]
    fn rendered_config_round_trips() {
        let cli = Cli {
            config_dir: Some(PathBuf::from("/data/komga")),
            port: None,
        };
        let file: FileConfig = toml::from_str(
            r#"
[database]
pool-size = 4
busy-timeout = "30s"
pragmas = { synchronous = "NORMAL" }

[kobo]
kepubify-path = "/usr/local/bin/kepubify"

[oauth2.registrations.github]
client-id = "gh-id"
client-secret = "gh-secret"
scopes = ["read:user"]
issuer-uri = "https://github.com"
"#,
        )
        .unwrap();
        let config = ServerConfig::resolve(Some(&file), &cli, &[]).unwrap();
        let rendered = file::render(&file, &config, None);
        let reparsed: FileConfig = toml::from_str(&rendered).unwrap();
        let config2 = ServerConfig::resolve(Some(&reparsed), &cli, &[]).unwrap();
        assert_eq!(config2.database.pool_size, Some(4));
        assert_eq!(config2.database.busy_timeout, Some(Duration::from_secs(30)));
        assert_eq!(
            config2.database.pragmas,
            vec![("synchronous".to_string(), "NORMAL".to_string())]
        );
        assert_eq!(
            config2.database.file,
            PathBuf::from("/data/komga/database.sqlite")
        );
        assert_eq!(
            config2.kepubify_path,
            Some(PathBuf::from("/usr/local/bin/kepubify"))
        );
        let reg = &config2.oauth2.registrations[0];
        assert_eq!(reg.registration_id, "github");
        assert_eq!(reg.client_id, "gh-id");
        assert_eq!(reg.client_secret, "gh-secret");
        assert_eq!(reg.scopes, vec!["read:user".to_string()]);
        assert_eq!(reg.issuer_uri.as_deref(), Some("https://github.com"));
    }

    #[test]
    fn config_toml_is_generated_on_first_start() {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli {
            config_dir: Some(dir.path().to_path_buf()),
            port: None,
        };
        let config = ServerConfig::load_with(&cli, &[]).unwrap();
        assert_eq!(config.port, 25600);
        let written = dir.path().join("config.toml");
        let text = std::fs::read_to_string(&written).unwrap();
        // defaults are commented out; only explicit overrides are written live
        assert!(text.contains("# port = 25600"));
        let parsed: FileConfig = toml::from_str(&text).unwrap();
        let reparsed = ServerConfig::resolve(Some(&parsed), &cli, &[]).unwrap();
        assert_eq!(reparsed.port, 25600);
        assert_eq!(reparsed.database.file, dir.path().join("database.sqlite"));
        assert_eq!(reparsed.lucene_dir, dir.path().join("lucene"));
    }

    #[test]
    fn first_start_migrates_java_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("application.yml"),
            "server:\n  port: 8080\nkomga:\n  page-hashing: 7\n",
        )
        .unwrap();
        let cli = Cli {
            config_dir: Some(dir.path().to_path_buf()),
            port: None,
        };
        let config = ServerConfig::load_with(&cli, &[]).unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.page_hashing, 7);
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("port = 8080"));
        assert!(text.contains("page-hashing = 7"));
        // untouched defaults stay commented
        assert!(text.contains("# session-timeout = \"7d\""));
    }

    #[test]
    fn existing_config_toml_wins_over_java_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "[server]\nport = 9000\n").unwrap();
        std::fs::write(
            dir.path().join("application.yml"),
            "server:\n  port: 8080\n",
        )
        .unwrap();
        let cli = Cli {
            config_dir: Some(dir.path().to_path_buf()),
            port: None,
        };
        let config = ServerConfig::load_with(&cli, &[]).unwrap();
        assert_eq!(config.port, 9000);
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert_eq!(text, "[server]\nport = 9000\n");
    }
}
