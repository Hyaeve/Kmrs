//! Connection pool: aligns with `DataSourcesConfiguration.kt`.
//! - Read/write separation under WAL: the RW pool is always 1, the RO pool is
//!   poolSize ?: min(CPU cores, maxPoolSize).
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
//! - The task-side main database, the tasks queue and the kmrs database are
//!   auxiliary pools: their work is serial per worker (`TASK_POOL_SIZE` defaults
//!   to 1) or light API queries, so their read pool defaults to
//!   [`DEFAULT_AUX_READERS`] — build their config with
//!   [`DatabaseConfig::aux_pools`].
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
/// Default read-pool size for auxiliary pools (task-side main DB, tasks DB,
/// kmrs DB); an explicit `pool_size` always wins.
pub const DEFAULT_AUX_READERS: u32 = 2;

/// Statements at or above this duration are logged by [`profile_slow_query`].
const SLOW_QUERY: Duration = Duration::from_millis(500);

/// rusqlite profile hook, registered on every pooled connection. The clock
/// spans first step to statement completion, so lock waits and slow row-by-row
/// reads count too — a writer starved by another pool's transaction shows up.
fn profile_slow_query(sql: &str, duration: Duration) {
    if let Some(line) = slow_query_line(sql, duration) {
        tracing::warn!(target: "komga_db::slow_query", "{line}");
    }
}

fn slow_query_line(sql: &str, duration: Duration) -> Option<String> {
    if duration < SLOW_QUERY {
        return None;
    }
    let one_line: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = one_line.chars().take(200).collect();
    Some(format!(
        "slow query ({} ms): {truncated}",
        duration.as_millis()
    ))
}

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
    /// Read pool size; None = min(CPU cores, max_pool_size)
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

    /// Effective read-pool size: explicit `pool_size`, else min(CPU cores, max_pool_size).
    pub fn read_pool_size(&self) -> u32 {
        if self.is_memory() {
            1
        } else if let Some(size) = self.pool_size {
            size
        } else {
            std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1)
                .min(self.max_pool_size)
        }
    }

    /// Config for an auxiliary pool set (task-side main DB, tasks DB, kmrs DB):
    /// same settings, but the read pool defaults to [`DEFAULT_AUX_READERS`]
    /// instead of the CPU-based sizing the API side needs.
    pub fn aux_pools(&self) -> Self {
        let mut config = self.clone();
        if config.pool_size.is_none() {
            config.pool_size = Some(DEFAULT_AUX_READERS.min(config.max_pool_size));
        }
        config
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
        let pool_size = config.read_pool_size();
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
                conn.profile(Some(profile_slow_query));
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
            conn.profile(Some(profile_slow_query));
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
    fn slow_query_is_logged() {
        use std::sync::{Arc, Mutex};
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = {
            let buf = buf.clone();
            move || Buf(buf.clone())
        };
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer)
            .with_ansi(false)
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            let dir = tempfile::tempdir().unwrap();
            let db = Database::open(&file_config(dir.path(), 1)).unwrap();
            let conn = db.rw().unwrap();
            // a deliberately slow statement: tens of millions of recursive steps
            let sum: i64 = conn
                .query_row(
                    "WITH RECURSIVE r(x) AS \
                     (SELECT 1 UNION ALL SELECT x + 1 FROM r WHERE x < 30000000) \
                     SELECT sum(x) FROM r",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(sum > 0);
        });

        let logs = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(
            logs.contains("slow query") && logs.contains("WITH RECURSIVE"),
            "slow query was not logged: {logs}"
        );
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
    fn slow_query_line_threshold_and_format() {
        // below the threshold: silence
        assert_eq!(
            slow_query_line("SELECT 1", Duration::from_millis(499)),
            None
        );

        // whitespace is collapsed so the log line stays single-line
        let line = slow_query_line(
            "SELECT  BOOK.ID\nFROM BOOK  JOIN MEDIA ON BOOK.ID = MEDIA.BOOK_ID",
            Duration::from_millis(1500),
        )
        .unwrap();
        assert_eq!(
            line,
            "slow query (1500 ms): SELECT BOOK.ID FROM BOOK JOIN MEDIA ON BOOK.ID = MEDIA.BOOK_ID"
        );

        // long SQL is truncated for the log line
        let long = format!("SELECT {} FROM T", "X".repeat(400));
        let line = slow_query_line(&long, Duration::from_secs(2)).unwrap();
        assert!(line.len() <= 200 + "slow query (2000 ms): ".len());
        assert!(line.starts_with("slow query (2000 ms): SELECT XXX"));
    }

    #[test]
    fn default_read_pool_size() {
        // max-pool-size caps the CPU-based default
        let config = DatabaseConfig {
            max_pool_size: 1,
            ..Default::default()
        };
        assert_eq!(config.read_pool_size(), 1);

        // explicit pool-size wins over the formula
        let config = DatabaseConfig {
            pool_size: Some(3),
            max_pool_size: 1,
            ..Default::default()
        };
        assert_eq!(config.read_pool_size(), 3);
    }

    #[test]
    fn aux_pools_default_readers() {
        // unset pool-size falls back to the small auxiliary default
        let config = DatabaseConfig::default();
        assert_eq!(config.aux_pools().pool_size, Some(DEFAULT_AUX_READERS));

        // explicit pool-size applies to auxiliary pools too
        let config = DatabaseConfig {
            pool_size: Some(6),
            ..Default::default()
        };
        assert_eq!(config.aux_pools().pool_size, Some(6));

        // a smaller max-pool-size caps the auxiliary default
        let config = DatabaseConfig {
            max_pool_size: 1,
            ..Default::default()
        };
        assert_eq!(config.aux_pools().pool_size, Some(1));
    }

    #[test]
    fn aux_pools_cache_share_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = file_config(dir.path(), 8);
        config.pool_size = None;
        let db = Database::open(&config.aux_pools()).unwrap();
        // 16 MiB split between 2 readers + 1 writer exceeds the 2048 KiB ceiling
        assert_eq!(cache_size(&db), -2048);
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
