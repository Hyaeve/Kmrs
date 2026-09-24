//! Flyway-compatible migrator: aligns with Java Flyway 13's behavior on SQLite,
//! so the same database.sqlite can be opened/upgraded interchangeably by Java komga
//! and kmrs.
//!
//! Alignment points:
//! - `flyway_schema_history` table structure and row contents (type='SQL'/'JDBC',
//!   checksum, installed_by='').
//! - checksum = Flyway `ChecksumCalculator`: feed the UTF-8 bytes of each line
//!   (split on \n/\r\n/\r) of the raw resource into CRC32 (line endings excluded),
//!   strip the BOM from the first line, and convert the result to i32. Placeholders
//!   are NOT substituted for the checksum, only for execution (verified against
//!   Java's recorded checksums for the 3 migrations using ${...} placeholders).
//! - One transaction per migration (SQLite DDL is transactional); a failure rolls
//!   back entirely and leaves no history row.
//! - validate semantics: applied versions must exist locally; SQL migration
//!   checksums must match; a local version smaller than the max applied version
//!   but not applied → error (no out-of-order).
//! - komga-riir adoption: databases built by komga-riir (a parallel Rust
//!   reimplementation) record history in sqlx's `_sqlx_migrations` table and have
//!   no `flyway_schema_history`. When such a schema is detected, kmrs rebuilds
//!   the Flyway history from the sqlx rows and stamps the Java (JDBC) data-fix
//!   ports from an explicit whitelist as applied (they must not run against
//!   komga-riir data: several query columns that only exist in old Java komga
//!   schemas) before the normal validate/apply path runs.
//! - komga-riir hybrid absorption: when a Java komga / kmrs database was later
//!   opened by komga-riir, migrations komga-riir applied beyond the Flyway
//!   history are recorded only in `_sqlx_migrations` while their schema objects
//!   already exist. Such versions are absorbed (stamped) into the Flyway history
//!   so kmrs does not re-run their DDL.

use komga_core::tsid::TsidFactory;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::time::Instant;

pub struct SqlMigration {
    pub file_name: &'static str,
    pub sql: &'static str,
}

type JdbcApply = fn(&Connection, &TsidFactory) -> Result<(), MigrateError>;

/// One migration: either a SQL file or the Rust port of a Java migration.
pub enum Migration {
    Sql(SqlMigration),
    Jdbc {
        /// script in the history: Java class FQN, e.g. `db.migration.sqlite.V20200810154730__thumbnails_part_2`
        class_name: &'static str,
        apply: JdbcApply,
    },
}

impl Migration {
    fn file_stem(&self) -> &'static str {
        match self {
            Migration::Sql(m) => m.file_name.strip_suffix(".sql").unwrap_or(m.file_name),
            Migration::Jdbc { class_name, .. } => class_name.rsplit('.').next().unwrap(),
        }
    }

    /// `V20260225161438__tags_view` → (20260225161438, "tags view")
    fn parse_meta(&self) -> (u64, String) {
        let stem = self.file_stem();
        let stem = stem
            .strip_prefix('V')
            .expect("migration name must start with V");
        let (version, description) = stem
            .split_once("__")
            .expect("migration name must contain __");
        (
            version.parse().expect("migration version must be numeric"),
            description.replace('_', " "),
        )
    }

    fn version(&self) -> u64 {
        self.parse_meta().0
    }

    fn description(&self) -> String {
        self.parse_meta().1
    }

    fn script(&self) -> String {
        match self {
            Migration::Sql(m) => m.file_name.to_string(),
            Migration::Jdbc { class_name, .. } => class_name.to_string(),
        }
    }

    fn type_(&self) -> &'static str {
        match self {
            Migration::Sql(_) => "SQL",
            Migration::Jdbc { .. } => "JDBC",
        }
    }
}

/// komga's 4 Flyway placeholders (default values from application.yml).
#[derive(Debug, Clone)]
pub struct Placeholders {
    /// komga.file-hashing
    pub library_file_hashing: bool,
    /// komga.libraries-scan-startup
    pub library_scan_startup: bool,
    /// komga.delete-empty-collections
    pub delete_empty_collections: bool,
    /// komga.delete-empty-read-lists
    pub delete_empty_read_lists: bool,
}

impl Default for Placeholders {
    fn default() -> Self {
        Self {
            library_file_hashing: true,
            library_scan_startup: false,
            delete_empty_collections: true,
            delete_empty_read_lists: true,
        }
    }
}

impl Placeholders {
    pub fn substitute(&self, sql: &str) -> String {
        sql.replace(
            "${library-file-hashing}",
            bool_str(self.library_file_hashing),
        )
        .replace(
            "${library-scan-startup}",
            bool_str(self.library_scan_startup),
        )
        .replace(
            "${delete-empty-collections}",
            bool_str(self.delete_empty_collections),
        )
        .replace(
            "${delete-empty-read-lists}",
            bool_str(self.delete_empty_read_lists),
        )
    }
}

fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

/// Equivalent of Flyway's `ChecksumCalculator`, over the raw resource content
/// (placeholders are substituted later, only for execution).
pub fn flyway_checksum(sql: &str) -> i32 {
    let mut hasher = crc32fast::Hasher::new();
    let mut lines = sql.lines();
    if let Some(first) = lines.next() {
        let first = first.strip_prefix('\u{FEFF}').unwrap_or(first);
        hasher.update(first.as_bytes());
    }
    for line in lines {
        hasher.update(line.as_bytes());
    }
    hasher.finalize() as i32
}

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("database has objects but no flyway_schema_history table; baseline is not supported")]
    NonEmptySchemaWithoutHistory,
    #[error("applied migration not resolved locally: version {0}")]
    AppliedNotResolved(String),
    #[error("checksum mismatch for applied migration {version}: database={db} local={local}")]
    ChecksumMismatch {
        version: String,
        db: i32,
        local: i32,
    },
    #[error(
        "resolved migration not applied to database: version {0} (out-of-order is not allowed)"
    )]
    NotAppliedOutOfOrder(String),
    #[error("migration {0} failed: {1}")]
    Failed(String, #[source] rusqlite::Error),
    #[error("java migration {0} failed: {1}")]
    JdbcFailed(String, #[source] Box<MigrateError>),
    #[error("failed migrations detected in flyway_schema_history (success=0)")]
    FailedMigrationPresent,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

const HISTORY_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS "flyway_schema_history" (
    "installed_rank" INT NOT NULL,
    "version" VARCHAR(50),
    "description" VARCHAR(200) NOT NULL,
    "type" VARCHAR(20) NOT NULL,
    "script" VARCHAR(1000) NOT NULL,
    "checksum" INTEGER,
    "installed_by" VARCHAR(100) NOT NULL,
    "installed_on" TIMESTAMP NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    "execution_time" INTEGER NOT NULL,
    "success" BOOLEAN NOT NULL,
    CONSTRAINT "flyway_schema_history_pk" PRIMARY KEY ("installed_rank")
);
CREATE INDEX IF NOT EXISTS "flyway_schema_history_s_idx" ON "flyway_schema_history" ("success");
"#;

struct AppliedRow {
    version: Option<String>,
    type_: String,
    checksum: Option<i32>,
}

pub struct Migrator<'a> {
    migrations: &'a [Migration],
    placeholders: Placeholders,
    tsid_factory: TsidFactory,
}

impl<'a> Migrator<'a> {
    pub fn new(migrations: &'a [Migration], placeholders: Placeholders) -> Self {
        Self {
            migrations,
            placeholders,
            tsid_factory: TsidFactory::new_random_node(),
        }
    }

    pub fn migrate(&self, conn: &Connection) -> Result<usize, MigrateError> {
        let local: BTreeMap<u64, &Migration> =
            self.migrations.iter().map(|m| (m.version(), m)).collect();

        let history_existed = table_exists(conn, "flyway_schema_history")?;
        // An empty history table counts as no history: a leftover empty table
        // from a crashed or interrupted adoption must not brick the database
        // by skipping adoption and then failing out-of-order; re-adopt instead.
        let history_is_empty = !history_existed
            || conn.query_row("SELECT COUNT(*) FROM flyway_schema_history", [], |r| {
                r.get::<_, i64>(0)
            })? == 0;
        if history_is_empty && schema_has_user_objects(conn)? {
            // komga-riir records history in sqlx's `_sqlx_migrations` and never
            // creates `flyway_schema_history`. Rebuild the Flyway history from the
            // sqlx rows before the normal validate/apply path runs; if there is no
            // adoptable history, keep the baseline-is-not-supported behavior.
            if !adopt_from_sqlx_history(conn, &local)? {
                return Err(MigrateError::NonEmptySchemaWithoutHistory);
            }
        }
        conn.execute_batch(HISTORY_DDL)?;

        // validate: every applied migration must resolve locally, and SQL checksums must match
        let applied = load_applied(conn)?;
        if has_failed_rows(conn)? {
            return Err(MigrateError::FailedMigrationPresent);
        }
        for row in &applied {
            let version: u64 = row
                .version
                .as_deref()
                .and_then(|v| v.parse().ok())
                // non-numeric versions (e.g. baseline rows) are never produced by komga; error out
                .ok_or_else(|| {
                    MigrateError::AppliedNotResolved(row.version.clone().unwrap_or_default())
                })?;
            let local_migration = local
                .get(&version)
                .ok_or_else(|| MigrateError::AppliedNotResolved(version.to_string()))?;
            if row.type_ == "SQL" {
                let local_checksum = match local_migration {
                    // Flyway checksums the raw resource content, placeholders NOT substituted
                    Migration::Sql(m) => flyway_checksum(m.sql),
                    Migration::Jdbc { .. } => unreachable!(),
                };
                let db_checksum = row.checksum.unwrap_or(0);
                if db_checksum != local_checksum {
                    return Err(MigrateError::ChecksumMismatch {
                        version: version.to_string(),
                        db: db_checksum,
                        local: local_checksum,
                    });
                }
            }
        }

        // A database that komga-riir opened on top of a Java komga / kmrs one
        // records the migrations it ran beyond the Flyway history only in
        // `_sqlx_migrations`; absorb them so they are not re-applied. This runs
        // after validation so a failing database is not mutated first; the write
        // is idempotent and a later retry self-heals either way.
        absorb_sqlx_versions(conn, &local)?;

        // Reload the applied set: absorbed versions now appear in the history
        // and must not be considered pending below.
        let applied = load_applied(conn)?;
        let max_applied = applied
            .iter()
            .filter_map(|r| r.version.as_deref()?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);

        // out-of-order check + collect pending migrations
        let mut pending: Vec<&Migration> = Vec::new();
        for (version, migration) in &local {
            let version_string = version.to_string();
            let is_applied = applied
                .iter()
                .any(|r| r.version.as_deref() == Some(version_string.as_str()));
            if is_applied {
                continue;
            }
            if *version < max_applied {
                return Err(MigrateError::NotAppliedOutOfOrder(version_string));
            }
            pending.push(migration);
        }

        let mut count = 0;
        for migration in pending {
            self.apply_one(conn, migration)?;
            count += 1;
        }
        Ok(count)
    }

    fn apply_one(&self, conn: &Connection, migration: &Migration) -> Result<(), MigrateError> {
        let version = migration.version();
        let description = migration.description();
        let start = Instant::now();
        let resolved_checksum;
        {
            let tx = conn.unchecked_transaction()?;
            match migration {
                Migration::Sql(m) => {
                    let resolved = self.placeholders.substitute(m.sql);
                    // checksum over the raw resource, like Flyway
                    resolved_checksum = Some(flyway_checksum(m.sql));
                    tx.execute_batch(&resolved)
                        .map_err(|e| MigrateError::Failed(m.file_name.to_string(), e))?;
                }
                Migration::Jdbc { class_name, apply } => {
                    resolved_checksum = None;
                    if let Err(e) = apply(&tx, &self.tsid_factory) {
                        return Err(MigrateError::JdbcFailed(
                            class_name.to_string(),
                            Box::new(e),
                        ));
                    }
                }
            }
            let next_rank: i64 = tx.query_row(
                "SELECT COALESCE(MAX(installed_rank), 0) + 1 FROM flyway_schema_history",
                [],
                |r| r.get(0),
            )?;
            tx.execute(
        "INSERT INTO flyway_schema_history \
         (installed_rank, version, description, type, script, checksum, installed_by, installed_on, execution_time, success) \
         VALUES (?, ?, ?, ?, ?, ?, '', CURRENT_TIMESTAMP, ?, 1)",
        params![
          next_rank,
          version.to_string(),
          description,
          migration.type_(),
          migration.script(),
          resolved_checksum,
          start.elapsed().as_millis() as i64,
        ],
      )?;
            tx.commit()?;
        }
        tracing::info!(target: "komga_db::migrate", version, script = migration.script(), "migration applied");
        Ok(())
    }
}

/// Inserts a history row for an already-applied migration, mirroring the rows
/// `apply_one` would have written (Flyway-compatible column values).
fn stamp_applied(conn: &Connection, rank: i64, migration: &Migration) -> Result<(), MigrateError> {
    let checksum = match migration {
        Migration::Sql(m) => Some(flyway_checksum(m.sql)),
        Migration::Jdbc { .. } => None,
    };
    conn.execute(
        "INSERT INTO flyway_schema_history \
         (installed_rank, version, description, type, script, checksum, installed_by, installed_on, execution_time, success) \
         VALUES (?, ?, ?, ?, ?, ?, '', CURRENT_TIMESTAMP, 0, 1)",
        params![
            rank,
            migration.version().to_string(),
            migration.description(),
            migration.type_(),
            migration.script(),
            checksum,
        ],
    )?;
    Ok(())
}

/// The versions komga-riir recorded as successfully applied in its sqlx history
/// (`_sqlx_migrations`), in ascending order.
fn load_sqlx_versions(conn: &Connection) -> Result<Vec<u64>, rusqlite::Error> {
    if !table_exists(conn, "_sqlx_migrations")? {
        return Ok(Vec::new());
    }
    conn.prepare("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")?
        .query_map([], |r| r.get::<_, i64>(0).map(|v| v as u64))?
        .collect()
}

/// The Java (JDBC) data-fix ports that adoption stamps as applied. They must
/// not run against komga-riir-shaped data: they query columns that only exist
/// in old Java komga schemas (e.g. `thumbnails_part_2` selects
/// `MEDIA.THUMBNAIL`, renamed to `_UNUSED`), so executing them would fail
/// rather than no-op. An explicit whitelist makes the choice loud: a future
/// JDBC port not listed here is not stamped and would be applied (and likely
/// fail) instead of being silently skipped.
const JDBC_PORTS_STAMPED_ON_ADOPTION: &[&str] = &[
    "V20200810154730__thumbnails_part_2",
    "V20200820150923__metadata_fields_part_2",
    "V20210624165023__missing_series_metadata",
    "V20230801104436__fix_incorrect_language_codes",
    "V20240422132621__fix_read_progress_locators",
];

/// Whether `version` counts as applied on komga-riir's side: recorded in its
/// sqlx history. komga-riir never records `book_projection`, so kmrs applies
/// it itself.
fn sqlx_counts_as_applied(version: u64, sqlx_versions: &[u64]) -> bool {
    sqlx_versions.contains(&version)
}

/// Logs every sqlx-recorded version kmrs does not know: adoption and
/// absorption skip them, and silence would hide a komga-riir schema drift.
fn warn_unknown_sqlx_versions(sqlx_versions: &[u64], local: &BTreeMap<u64, &Migration>) {
    for version in sqlx_versions {
        if !local.contains_key(version) {
            tracing::warn!(
                target: "komga_db::migrate",
                version,
                "komga-riir sqlx history records a version kmrs does not know; skipping"
            );
        }
    }
}

/// Rebuilds `flyway_schema_history` from komga-riir's sqlx history
/// (`_sqlx_migrations`), so an existing komga-riir database can be opened and
/// upgraded like a Java komga one.
///
/// Stamp rules:
/// - SQL migrations that komga-riir actually ran (matching versions);
/// - the Java (JDBC) data-fix ports on the explicit whitelist, which must not
///   run against komga-riir-shaped data (they query columns that only exist in
///   old Java komga schemas, so they would fail rather than no-op).
///
/// Returns `Ok(false)` when there is nothing adoptable; the caller keeps the
/// original "no history" error in that case.
fn adopt_from_sqlx_history(
    conn: &Connection,
    local: &BTreeMap<u64, &Migration>,
) -> Result<bool, MigrateError> {
    let sqlx_versions = load_sqlx_versions(conn)?;
    if sqlx_versions.is_empty() {
        return Ok(false);
    }
    warn_unknown_sqlx_versions(&sqlx_versions, local);

    // Create the history table inside the stamping transaction. SQLite DDL is
    // transactional, so a failed stamping run rolls the table back too and the
    // database stays adoptable on the next open; creating it outside the
    // transaction would leave an empty `flyway_schema_history` behind, which
    // makes the next open skip adoption and fail with `NotAppliedOutOfOrder`.
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(HISTORY_DDL)?;
    let mut rank: i64 = 0;
    for (version, migration) in local {
        let stamps_as_applied = sqlx_counts_as_applied(*version, &sqlx_versions)
            || matches!(
                migration,
                Migration::Jdbc { .. }
                    if JDBC_PORTS_STAMPED_ON_ADOPTION.contains(&migration.file_stem())
            );
        if stamps_as_applied {
            rank += 1;
            stamp_applied(&tx, rank, migration)?;
        }
    }
    if rank == 0 {
        return Ok(false);
    }
    tx.commit()?;
    tracing::info!(
        target: "komga_db::migrate",
        stamped = rank,
        "adopted komga-riir schema from _sqlx_migrations"
    );
    Ok(true)
}

/// When a database already has a Flyway history (Java komga or kmrs) and
/// komga-riir later opened it, migrations komga-riir applied beyond that history
/// are recorded only in its sqlx table while their schema objects already exist.
/// Stamp those versions into the Flyway history so kmrs does not re-run their
/// DDL (e.g. `CREATE VIEW` / `CREATE TABLE` collisions).
fn absorb_sqlx_versions(
    conn: &Connection,
    local: &BTreeMap<u64, &Migration>,
) -> Result<(), MigrateError> {
    let sqlx_versions = load_sqlx_versions(conn)?;
    if sqlx_versions.is_empty() {
        return Ok(());
    }
    warn_unknown_sqlx_versions(&sqlx_versions, local);

    let tx = conn.unchecked_transaction()?;
    let mut stamped = 0;
    for (version, migration) in local {
        if !sqlx_counts_as_applied(*version, &sqlx_versions) {
            continue;
        }
        let already: i64 = conn.query_row(
            "SELECT COUNT(*) FROM flyway_schema_history WHERE version = ?",
            [version.to_string()],
            |r| r.get(0),
        )?;
        if already > 0 {
            continue;
        }
        let next_rank: i64 = conn.query_row(
            "SELECT COALESCE(MAX(installed_rank), 0) + 1 FROM flyway_schema_history",
            [],
            |r| r.get(0),
        )?;
        stamp_applied(&tx, next_rank, migration)?;
        stamped += 1;
    }
    tx.commit()?;
    if stamped > 0 {
        tracing::info!(
            target: "komga_db::migrate",
            absorbed = stamped,
            "absorbed komga-riir migrations recorded only in _sqlx_migrations"
        );
    }
    Ok(())
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
}

/// Flyway's non-empty schema check: sqlite_master contains objects other than
/// flyway_schema_history.
fn schema_has_user_objects(conn: &Connection) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name <> 'flyway_schema_history'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
}

fn load_applied(conn: &Connection) -> Result<Vec<AppliedRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
    "SELECT version, type, checksum FROM flyway_schema_history WHERE success ORDER BY installed_rank",
  )?;
    let rows = stmt.query_map([], |r| {
        Ok(AppliedRow {
            version: r.get(0)?,
            type_: r.get(1)?,
            checksum: r.get(2)?,
        })
    })?;
    rows.collect()
}

fn has_failed_rows(conn: &Connection) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) FROM flyway_schema_history WHERE NOT success",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_line_ending_insensitive() {
        let lf = "CREATE TABLE a(\n  id int\n);\n";
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(flyway_checksum(lf), flyway_checksum(&crlf));
    }

    #[test]
    fn checksum_strips_bom() {
        let plain = "SELECT 1;";
        let bom = "\u{FEFF}SELECT 1;";
        assert_eq!(flyway_checksum(plain), flyway_checksum(bom));
    }

    #[test]
    fn placeholder_substitution() {
        let p = Placeholders::default();
        let sql = "DEFAULT ${library-file-hashing}, ${library-scan-startup}, ${delete-empty-collections}, ${delete-empty-read-lists}";
        assert_eq!(p.substitute(sql), "DEFAULT true, false, true, true");
    }

    #[test]
    fn meta_parsing() {
        let m = Migration::Sql(SqlMigration {
            file_name: "V20260225161438__tags_view.sql",
            sql: "",
        });
        assert_eq!(m.version(), 20260225161438);
        assert_eq!(m.description(), "tags view");
        assert_eq!(m.script(), "V20260225161438__tags_view.sql");
        assert_eq!(m.type_(), "SQL");
    }
}
