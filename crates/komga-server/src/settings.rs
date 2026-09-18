//! Server runtime settings: aligned with `KomgaSettingsProvider` (stored in the SERVER_SETTINGS table, with an in-memory cache).

use komga_db::dao::settings::SettingsDao;
use komga_db::pool::Database;
use std::sync::RwLock;
use std::time::Duration;

const KEY_DELETE_EMPTY_COLLECTIONS: &str = "DELETE_EMPTY_COLLECTIONS";
const KEY_DELETE_EMPTY_READLISTS: &str = "DELETE_EMPTY_READLISTS";
const KEY_REMEMBER_ME_KEY: &str = "REMEMBER_ME_KEY";
const KEY_REMEMBER_ME_DURATION: &str = "REMEMBER_ME_DURATION";
const KEY_THUMBNAIL_SIZE: &str = "THUMBNAIL_SIZE";
const KEY_TASK_POOL_SIZE: &str = "TASK_POOL_SIZE";
const KEY_SERVER_PORT: &str = "SERVER_PORT";
const KEY_SERVER_CONTEXT_PATH: &str = "SERVER_CONTEXT_PATH";
const KEY_KOBO_PROXY: &str = "KOBO_PROXY";
const KEY_KOBO_PORT: &str = "KOBO_PORT";
const KEY_KEPUBIFY_PATH: &str = "KEPUBIFY_PATH";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThumbnailSize {
    #[default]
    Default,
    Medium,
    Large,
    XLarge,
}

impl ThumbnailSize {
    pub fn max_edge(self) -> u32 {
        match self {
            ThumbnailSize::Default => 300,
            ThumbnailSize::Medium => 600,
            ThumbnailSize::Large => 900,
            ThumbnailSize::XLarge => 1200,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ThumbnailSize::Default => "DEFAULT",
            ThumbnailSize::Medium => "MEDIUM",
            ThumbnailSize::Large => "LARGE",
            ThumbnailSize::XLarge => "XLARGE",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "MEDIUM" => ThumbnailSize::Medium,
            "LARGE" => ThumbnailSize::Large,
            "XLARGE" => ThumbnailSize::XLarge,
            _ => ThumbnailSize::Default,
        }
    }
}

#[derive(Debug, Clone)]
pub struct KomgaSettings {
    pub delete_empty_collections: bool,
    pub delete_empty_readlists: bool,
    pub remember_me_key: String,
    pub remember_me_duration: Duration,
    pub thumbnail_size: ThumbnailSize,
    pub task_pool_size: u32,
    pub server_port: Option<u16>,
    pub server_context_path: Option<String>,
    pub kobo_proxy: bool,
    pub kobo_port: Option<u16>,
    pub kepubify_path: Option<String>,
}

pub struct SettingsProvider {
    db: Database,
    inner: RwLock<KomgaSettings>,
}

impl SettingsProvider {
    pub fn load(db: Database) -> Self {
        let dao = SettingsDao::new(db.clone());
        let read = || -> Result<KomgaSettings, komga_db::Error> {
            let mut remember_me_key = dao.get_setting(KEY_REMEMBER_ME_KEY)?.unwrap_or_default();
            if remember_me_key.is_empty() {
                remember_me_key = random_remember_me_key();
                dao.save_setting(KEY_REMEMBER_ME_KEY, &remember_me_key)?;
            }
            Ok(KomgaSettings {
                delete_empty_collections: dao
                    .get_setting_bool(KEY_DELETE_EMPTY_COLLECTIONS)?
                    .unwrap_or(false),
                delete_empty_readlists: dao
                    .get_setting_bool(KEY_DELETE_EMPTY_READLISTS)?
                    .unwrap_or(false),
                remember_me_key,
                remember_me_duration: Duration::from_secs(
                    dao.get_setting_i64(KEY_REMEMBER_ME_DURATION)?
                        .unwrap_or(365)
                        .max(0) as u64
                        * 24
                        * 3600,
                ),
                thumbnail_size: ThumbnailSize::from_str(
                    &dao.get_setting(KEY_THUMBNAIL_SIZE)?.unwrap_or_default(),
                ),
                task_pool_size: dao.get_setting_i64(KEY_TASK_POOL_SIZE)?.unwrap_or(1).max(0) as u32,
                server_port: dao.get_setting_i64(KEY_SERVER_PORT)?.map(|v| v as u16),
                server_context_path: dao
                    .get_setting(KEY_SERVER_CONTEXT_PATH)?
                    .filter(|s| !s.is_empty()),
                kobo_proxy: dao.get_setting_bool(KEY_KOBO_PROXY)?.unwrap_or(false),
                kobo_port: dao.get_setting_i64(KEY_KOBO_PORT)?.map(|v| v as u16),
                kepubify_path: dao
                    .get_setting(KEY_KEPUBIFY_PATH)?
                    .filter(|s| !s.is_empty()),
            })
        };
        let inner = read().expect("failed to load server settings");
        Self {
            db,
            inner: RwLock::new(inner),
        }
    }

    pub fn get(&self) -> KomgaSettings {
        self.inner.read().unwrap().clone()
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, komga_db::Error> {
        match key {
            KEY_REMEMBER_ME_KEY => Ok(Some(self.get().remember_me_key)),
            _ => SettingsDao::new(self.db.clone()).get_setting(key),
        }
    }

    /// Called after PATCH /api/v1/settings: re-reads from the DB.
    pub fn reload(&self) {
        *self.inner.write().unwrap() = Self::load(self.db.clone()).get();
    }
}

/// komga `getRandomRememberMeKey`: 32 random alphanumeric characters.
fn random_remember_me_key() -> String {
    const ALPHANUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..32)
        .map(|_| ALPHANUM[(rand_u8() as usize) % ALPHANUM.len()] as char)
        .collect()
}

fn rand_u8() -> u8 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    RandomState::new().build_hasher().finish() as u8
}
