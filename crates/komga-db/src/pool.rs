//! Connection pool: aligns with `DataSourcesConfiguration.kt`.
//! - Read/write separation under WAL: the RW pool is always 1, the RO pool is
//!   poolSize ?: min(CPU cores, maxPoolSize).
//! - Without WAL (or for an in-memory database): RO and RW share the same pool.
//! - Per connection: `PRAGMA foreign_keys=ON`, busy_timeout, journal_mode, and
//!   extra pragmas; main-database connections also register UDFs/collations (see
//!   the `udf` module), tasks-database connections do not.

use crate::udf;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::PathBuf;
use std::time::Duration;

pub type Pool = r2d2::Pool<SqliteConnectionManager>;
pub type PooledConn = r2d2::PooledConnection<SqliteConnectionManager>;

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
    /// Upper bound of pool_size, default 1 (same as komga)
    pub max_pool_size: u32,
    pub journal_mode: JournalMode,
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
            max_pool_size: 1,
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
        let make_manager = || {
            let config = config.clone();
            SqliteConnectionManager::file(&config.file).with_init(move |conn| {
                conn.execute_batch(&format!(
                    "PRAGMA journal_mode={}; PRAGMA foreign_keys=ON;",
                    config.journal_mode.as_str()
                ))?;
                if let Some(timeout) = config.busy_timeout {
                    conn.execute_batch(&format!("PRAGMA busy_timeout={};", timeout.as_millis()))?;
                }
                for (key, value) in &config.pragmas {
                    conn.execute_batch(&format!("PRAGMA {key}={value};"))?;
                }
                if config.register_udfs {
                    udf::register_all(conn)?;
                }
                Ok(())
            })
        };

        let pool_size = if config.is_memory() {
            1
        } else if let Some(size) = config.pool_size {
            size
        } else {
            std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1)
                .min(config.max_pool_size)
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
    pub fn rw(&self) -> PooledConn {
        self.rw.get().expect("RW pool exhausted")
    }

    /// Read connection.
    pub fn ro(&self) -> PooledConn {
        self.ro.get().expect("RO pool exhausted")
    }
}
