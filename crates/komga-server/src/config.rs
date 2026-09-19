//! Server configuration: aligned with `KomgaProperties` and the key defaults of application.yml.
//! env variable names follow Spring relaxed binding (KOMGA_* uppercase with underscores).

use komga_db::pool::{DatabaseConfig, JournalMode};
use std::path::PathBuf;
use std::time::Duration;

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
    pub oauth2: OAuth2Config,
}

/// OAuth2/OIDC client registrations, from `SPRING_SECURITY_OAUTH2_CLIENT_*` env vars.
/// When empty, OAuth2 login is disabled (providers endpoint returns an empty list and the
/// authorization/callback endpoints 404, matching `clientRegistrationRepository == null`).
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

impl ServerConfig {
    pub fn from_env() -> Self {
        let config_dir = env_path("KOMGA_CONFIG_DIR")
            .or_else(|| env_path("KOMGA_CONFIGDIR"))
            .unwrap_or_else(|| dirs_home().join(".komga"));
        let database_file =
            env_path("KOMGA_DATABASE_FILE").unwrap_or_else(|| config_dir.join("database.sqlite"));
        let tasks_db_file =
            env_path("KOMGA_TASKSDB_FILE").unwrap_or_else(|| config_dir.join("tasks.sqlite"));
        let port = env_u16("SERVER_PORT", 25600);

        let database = DatabaseConfig {
            file: database_file.clone(),
            register_udfs: true,
            ..Default::default()
        };
        let tasks_db = DatabaseConfig {
            file: tasks_db_file.clone(),
            register_udfs: false,
            ..Default::default()
        };

        Self {
            lucene_dir: env_path("KOMGA_LUCENE_DATA_DIRECTORY")
                .unwrap_or_else(|| config_dir.join("lucene")),
            fonts_dir: env_path("KOMGA_FONTS_DATA_DIRECTORY")
                .unwrap_or_else(|| config_dir.join("fonts")),
            config_dir,
            database_file,
            tasks_db_file,
            port,
            database,
            tasks_db,
            session_timeout: Duration::from_secs(7 * 24 * 3600),
            cors_allowed_origins: env_list("KOMGA_CORS_ALLOWEDORIGINS"),
            page_hashing: 3,
            epub_divina_letter_count_threshold: 15,
            kobo_sync_item_limit: 100,
            kepubify_path: std::env::var("KOMGA_KOBO_KEPUBIFY_PATH")
                .ok()
                .map(PathBuf::from),
            oauth2: oauth2_from_env(),
        }
    }
}

/// Reads `SPRING_SECURITY_OAUTH2_CLIENT_REGISTRATION_{ID}_*` and
/// `SPRING_SECURITY_OAUTH2_CLIENT_PROVIDER_{ID}_*` env vars into registrations.
fn oauth2_from_env() -> OAuth2Config {
    const REG_PREFIX: &str = "SPRING_SECURITY_OAUTH2_CLIENT_REGISTRATION_";
    const PROV_PREFIX: &str = "SPRING_SECURITY_OAUTH2_CLIENT_PROVIDER_";

    let mut registrations: Vec<OAuth2ClientRegistration> = vec![];
    for (key, value) in std::env::vars() {
        if let Some(rest) = key.strip_prefix(REG_PREFIX) {
            let Some((id, field)) = rest.rsplit_once('_') else {
                continue;
            };
            let index = registrations
                .iter()
                .position(|r: &OAuth2ClientRegistration| r.registration_id == id)
                .unwrap_or_else(|| {
                    registrations.push(OAuth2ClientRegistration {
                        registration_id: id.to_string(),
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
                    });
                    registrations.len() - 1
                });
            let reg = &mut registrations[index];
            match field {
                "CLIENT-NAME" => reg.client_name = Some(value),
                "CLIENT-ID" => reg.client_id = value,
                "CLIENT-SECRET" => reg.client_secret = value,
                "AUTHORIZATION-GRANT-TYPE" => reg.authorization_grant_type = value,
                "REDIRECT-URI" => reg.redirect_uri = Some(value),
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
            let Some(reg) = registrations.iter_mut().find(|r| r.registration_id == id) else {
                continue;
            };
            match field {
                "ISSUER-URI" => reg.issuer_uri = Some(value),
                "AUTHORIZATION-URI" => reg.authorization_uri = Some(value),
                "TOKEN-URI" => reg.token_uri = Some(value),
                "USER-INFO-URI" => reg.user_info_uri = Some(value),
                "USER-NAME-ATTRIBUTE" => reg.user_name_attribute = Some(value),
                _ => {}
            }
        }
    }
    registrations.retain(|r| !r.client_id.is_empty());
    OAuth2Config {
        registrations,
        account_creation: env_bool("KOMGA_OAUTH2ACCOUNTCREATION", false),
        oidc_email_verification: env_bool("KOMGA_OIDCMAILVERIFICATION", true),
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(default)
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn env_u16(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_list(key: &str) -> Vec<String> {
    std::env::var(key)
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[allow(dead_code)]
fn journal_mode_from_env() -> JournalMode {
    JournalMode::Wal
}
