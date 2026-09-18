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
        }
    }
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
