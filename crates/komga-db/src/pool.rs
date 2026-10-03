//! Connection pool: aligns with `DataSourcesConfiguration.kt`.
//! - Read/write separation under WAL: the RW pool is always 1, the RO pool is
//!   poolSize ?: clamp(CPU cores, 8, maxPoolSize). Read queries are I/O-bound, so
//!   even small boxes benefit from more readers than cores.
//! - Without WAL (or for an in-memory database): RO and RW share the same pool.
//! - Per connection: `PRAGMA foreign_keys=ON`, busy_timeout (default 30s), journal_mode,
//!   and extra pragmas; main-database connections also register UDFs/collations (see
//!   the `udf` module), tasks-database connections do not.
//! - Background task execution (library scan / analysis / hashing / conversion /
//!   maintenance) runs against a dedicated `Database` over the same file, opened
//!   with [`Database::open`]; under WAL its reads and writes never contend with
//!   API connections for pool slots. The task write pool is a second writer on
//!   the file, hence the default busy timeout. (Without WAL, RO and RW share one
//!   pool inside each `Database`; the task/API split still holds.)
//! - Page cache: each `Database` splits a fixed budget (default 16 MiB) between
//!   its connections. Per-connection caches duplicate one another, so the budget
//!   — not the connection count — is what bounds cache memory (up to the 512 KiB
//!   per-connection floor, which user-enlarged pools can push past the budget);
//!   the working set does not grow with more readers, and the OS page cache
//!   covers the rest. An explicit `cache_size` pragma overrides the default.

use crate::udf;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::PathBuf;
use std::time::Duration;

pub type Pool = r2d2::Pool<SqliteConnectionManager>;
pub type PooledConn = r2d2::PooledConnection<SqliteConnectionManager>;

/// Per-`Database` page-cache budget split between its connections.
const CACHE_BUDGET_KIB: u32 = 16 * 1024;
/// Clamp range for one connection's share: the floor keeps B-tree interior pages
/// resident on large pools, the ceiling stops tiny pools from over-allocating.
const MIN_CONN_CACHE_KIB: u32 = 512;
const MAX_CONN_CACHE_KIB: u32 = 2048;

/// A connection's share of the cache budget; `None` when `pragmas` already pins
/// `cache_size` — an explicit per-connection value always wins.
fn conn_cache_kib(config: &DatabaseConfig, pool_size: u32) -> Option<u32> {
    if config.pragmas.iter().any(|(key, _)| {
        key.rsplit('.')
            .next()
            .is_some_and(|k| k.eq_ignore_ascii_case("cache_size"))
    }) {
        return None;
    }
    let conns = if config.should_separate_read_from_writes() {
        pool_size + 1
    } else {
        1
    };
    Some((CACHE_BUDGET_KIB / conns).clamp(MIN_CONN_CACHE_KIB, MAX_CONN_CACHE_KIB))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JournalMode {
    #[default]
    Wal,
    Delete,
    Truncate,
    Persist,
    Memory,
    Off,
}

impl JournalMode {
    fn as_str(self) -> &'static str {
        match self {
            JournalMode::Wal => "WAL",
            JournalMode::Delete => "DELETE",
            JournalMode::Truncate => "TRUNCATE",
            JournalMode::Persist => "PERSIST",
            JournalMode::Memory => "MEMORY",
            JournalMode::Off => "OFF",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub file: PathBuf,
    /// Read pool size; None = clamp(CPU cores, 8, max_pool_size)
    pub pool_size: Option<u32>,
    /// Upper bound of pool_size, default 16; komga's default 1 serializes concurrent reads
    pub max_pool_size: u32,
    pub journal_mode: JournalMode,
    /// Busy timeout for each connection; `None` falls back to 30s. SQLite's own
    /// default is 0 (fail immediately), which breaks the moment the main database
    /// and the task pools both carry a write connection.
    pub busy_timeout: Option<Duration>,
    pub pragmas: Vec<(String, String)>,
    /// true for the main database: register REGEXP/UDF_STRIP_ACCENTS/COLLATION_UNICODE_*
    pub register_udfs: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            file: PathBuf::new(),
            pool_size: None,
            max_pool_size: 16,
            journal_mode: JournalMode::Wal,
            busy_timeout: None,
            pragmas: Vec::new(),
            register_udfs: true,
        }
    }
}

impl DatabaseConfig {
    fn is_memory(&self) -> bool {
        let f = self.file.to_string_lossy();
        f.contains(":memory:") || f.contains("mode=memory")
    }

    fn should_separate_read_from_writes(&self) -> bool {
        !self.is_memory() && self.journal_mode == JournalMode::Wal
    }
}

#[derive(Clone)]
pub struct Database {
    rw: Pool,
    ro: Pool,
}

impl Database {
    pub fn open(config: &DatabaseConfig) -> Result<Self, r2d2::Error> {
        // SQLite's default busy_timeout is 0 (fail immediately). The API write
        // pool and the task write pool are separate writers on the same file, so
        // concurrent writes are expected and must wait rather than error out.
        let busy_timeout = config.busy_timeout.unwrap_or(Duration::from_secs(30));
        let pool_size = if config.is_memory() {
            1
        } else if let Some(size) = config.pool_size {
            size
        } else {
            std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1)
                .clamp(8.min(config.max_pool_size), config.max_pool_size)
        };
        let cache_kib = conn_cache_kib(config, pool_size);
        let make_manager = || {
            let config = config.clone();
            SqliteConnectionManager::file(&config.file).with_init(move |conn| {
                conn.execute_batch(&format!(
                    "PRAGMA journal_mode={}; PRAGMA foreign_keys=ON; PRAGMA busy_timeout={};",
                    config.journal_mode.as_str(),
                    busy_timeout.as_millis()
                ))?;
                for (key, value) in &config.pragmas {
                    conn.execute_batch(&format!("PRAGMA {key}={value};"))?;
                }
                if let Some(kib) = cache_kib {
                    conn.execute_batch(&format!("PRAGMA cache_size=-{kib};"))?;
                }
                if config.register_udfs {
                    udf::register_all(conn)?;
                }
                Ok(())
            })
        };

        let rw = Pool::builder().max_size(1).build(make_manager())?;
        let ro = if config.should_separate_read_from_writes() {
            Pool::builder().max_size(pool_size).build(make_manager())?
        } else {
            rw.clone()
        };
        Ok(Self { rw, ro })
    }

    /// In-memory database (for tests), shared single connection.
    pub fn open_in_memory(register_udfs: bool) -> Result<Self, r2d2::Error> {
        let manager = SqliteConnectionManager::memory().with_init(move |conn| {
            conn.execute_batch("PRAGMA foreign_keys=ON;")?;
            if register_udfs {
                udf::register_all(conn)?;
            }
            Ok(())
        });
        let pool = Pool::builder().max_size(1).build(manager)?;
        Ok(Self {
            rw: pool.clone(),
            ro: pool,
        })
    }

    /// Write connection (always a single connection under WAL).
    pub fn rw(&self) -> crate::Result<PooledConn> {
        Ok(self.rw.get()?)
    }

    /// Read connection.
    pub fn ro(&self) -> crate::Result<PooledConn> {
        Ok(self.ro.get()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_config(dir: &std::path::Path, pool_size: u32) -> DatabaseConfig {
        DatabaseConfig {
            file: dir.join("test.sqlite"),
            pool_size: Some(pool_size),
            register_udfs: false,
            ..Default::default()
        }
    }

    fn cache_size(db: &Database) -> i64 {
        db.ro()
            .unwrap()
            .query_row("PRAGMA cache_size", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn budget_divided_between_connections() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&file_config(dir.path(), 8)).unwrap();
        // 16 MiB split between 8 readers + 1 writer
        assert_eq!(cache_size(&db), -1820);
    }

    #[test]
    fn user_cache_size_wins() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = file_config(dir.path(), 8);
        config.pragmas = vec![("CACHE_SIZE".into(), "-8192".into())];
        let db = Database::open(&config).unwrap();
        assert_eq!(cache_size(&db), -8192);

        // schema-qualified form must also suppress the default
        let dir = tempfile::tempdir().unwrap();
        let mut config = file_config(dir.path(), 8);
        config.pragmas = vec![("main.cache_size".into(), "-4096".into())];
        let db = Database::open(&config).unwrap();
        assert_eq!(cache_size(&db), -4096);
    }

    #[test]
    fn per_connection_share_is_clamped() {
        // non-WAL shares one connection: the whole budget exceeds the 2048 KiB ceiling
        let dir = tempfile::tempdir().unwrap();
        let mut config = file_config(dir.path(), 8);
        config.journal_mode = JournalMode::Delete;
        let db = Database::open(&config).unwrap();
        assert_eq!(cache_size(&db), -2048);

        // 65 connections: the divided share drops below the 512 KiB floor
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&file_config(dir.path(), 64)).unwrap();
        assert_eq!(cache_size(&db), -512);
    }
}
