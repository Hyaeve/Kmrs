//! Adoption of databases created by komga-riir.
//!
//! komga-riir builds the same komga schema but records its migration history in
//! sqlx's `_sqlx_migrations` table and never creates `flyway_schema_history`.
//! komga-riir's released builds do not carry a `book_projection` migration, so a
//! komga-riir database has no `BOOK_PROJECTION` table and kmrs applies its own
//! after stamping the adopted history. These tests build komga-riir-shaped
//! databases from a freshly migrated kmrs schema and assert that kmrs can reopen
//! them: history is rebuilt by stamping, kmrs's `book_projection` runs against
//! the adopted schema, and nothing kmrs does touches the sqlx history table.

use komga_db::migrate::{MigrateError, Migration, Migrator};
use komga_db::{main_migrations, Placeholders};
use rusqlite::{params, Connection};

/// kmrs's own `book_projection` migration version.
const KMRS_BOOK_PROJECTION_VERSION: i64 = 20260921111319;

/// The sqlx history table komga-riir creates (`crates/infrastructure/base/src/
/// sqlite/schema.rs`, `create_sqlx_migrations_table`).
const SQLX_DDL: &str = "CREATE TABLE _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
)";

fn migrate(conn: &Connection) -> usize {
    Migrator::new(&main_migrations(), Placeholders::default())
        .migrate(conn)
        .unwrap()
}

fn sql_migration_count() -> usize {
    main_migrations()
        .iter()
        .filter(|m| matches!(m, Migration::Sql(_)))
        .count()
}

/// Reads the SQL versions currently in `flyway_schema_history` (excluding
/// `skip`) and stamps them into a fresh `_sqlx_migrations` table — the shape
/// komga-riir leaves behind.
fn stamp_sqlx_history(conn: &Connection, skip: &[i64]) {
    let mut stmt = conn
        .prepare(
            "SELECT version, script FROM flyway_schema_history \
             WHERE success AND type = 'SQL' ORDER BY installed_rank",
        )
        .unwrap();
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    conn.execute_batch(SQLX_DDL).unwrap();
    let mut insert = conn
        .prepare(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES (?, ?, 1, X'', 0)",
        )
        .unwrap();
    for (version, script) in rows {
        let version: i64 = version.parse().unwrap();
        if skip.contains(&version) {
            continue;
        }
        let description = script
            .split("__")
            .nth(1)
            .unwrap()
            .strip_suffix(".sql")
            .unwrap()
            .replace('_', " ");
        insert.execute(params![version, description]).unwrap();
    }
}

/// Reshapes a freshly migrated kmrs database into a komga-riir-created one:
/// drops the Flyway history, stamps the SQL versions into `_sqlx_migrations`
/// (omitting kmrs's `book_projection` — komga-riir's released builds do not
/// carry it), and removes the `BOOK_PROJECTION` table.
fn convert_to_komga_riir(conn: &Connection) {
    stamp_sqlx_history(conn, &[KMRS_BOOK_PROJECTION_VERSION]);
    conn.execute_batch("DROP TABLE flyway_schema_history")
        .unwrap();
    conn.execute_batch("DROP TABLE BOOK_PROJECTION").unwrap();
}

fn history_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM flyway_schema_history WHERE success",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn sqlx_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM _sqlx_migrations", [], |r| r.get(0))
        .unwrap()
}

fn book_projection_shape(conn: &Connection) -> String {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'BOOK_PROJECTION'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn fresh_komga_riir_database_is_adopted() {
    let conn = Connection::open_in_memory().unwrap();
    // Build the full schema the way kmrs would, then reshape it into a
    // komga-riir-created database: sqlx history only, no book_projection table.
    migrate(&conn);
    convert_to_komga_riir(&conn);

    // Adoption stamps every sqlx-recorded version plus the JDBC ports; kmrs's
    // book_projection (absent from komga-riir) is then applied on top.
    let applied = migrate(&conn);
    assert_eq!(applied, 1);

    assert_eq!(history_count(&conn), main_migrations().len() as i64);

    // kmrs built its own table shape (with the FK kmrs's migration declares).
    let shape = book_projection_shape(&conn);
    assert!(
        shape.contains("FOREIGN KEY"),
        "kmrs builds its own table shape: {shape}"
    );

    // komga-riir's sqlx history is left untouched.
    assert_eq!(sqlx_count(&conn), sql_migration_count() as i64 - 1);

    // Reopening is a no-op.
    assert_eq!(migrate(&conn), 0);
}

#[test]
fn round_trip_after_adoption() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn);
    convert_to_komga_riir(&conn);
    migrate(&conn); // adopt

    let sqlx_before = sqlx_count(&conn);
    // kmrs reopens: fully consistent, sqlx history untouched.
    assert_eq!(migrate(&conn), 0);
    assert_eq!(sqlx_count(&conn), sqlx_before);
    assert_eq!(history_count(&conn), main_migrations().len() as i64);
}

#[test]
fn hybrid_database_absorbs_komga_riir_extra_migrations() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn);
    // A Java komga database (Flyway history ending before tags_view) that a
    // komga-riir build later opened: komga-riir applied tags_view and recorded it
    // only in its sqlx history, so the view exists while Flyway does not know
    // about it, and BOOK_PROJECTION does not exist yet.
    conn.execute_batch(
        "DELETE FROM flyway_schema_history WHERE version IN ('20260225161438', '20260921111319')",
    )
    .unwrap();
    conn.execute_batch("DROP TABLE BOOK_PROJECTION").unwrap();
    // Stamp komga-riir's sqlx history: every SQL migration except book_projection
    // (komga-riir's released builds do not carry it).
    stamp_sqlx_history(&conn, &[KMRS_BOOK_PROJECTION_VERSION]);
    // tags_view was deleted from Flyway above (it predates Java's history) but
    // the komga-riir build that opened this database did apply it and recorded it
    // only in its sqlx history — exactly the state the real hybrid database is in.
    conn.execute(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (?, ?, 1, X'', 0)",
        params![20260225161438_i64, "tags view"],
    )
    .unwrap();

    // tags_view is absorbed from sqlx (stamped, not re-created); only
    // book_projection is applied by kmrs.
    let applied = migrate(&conn);
    assert_eq!(applied, 1);
    assert_eq!(history_count(&conn), main_migrations().len() as i64);
    let tags_row: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM flyway_schema_history WHERE version = '20260225161438'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tags_row, 1);
    // The view komga-riir created is untouched (no CREATE VIEW collision).
    let views: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'view' AND name = 'SERIES_AND_BOOK_TAG'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(views, 1);
    // kmrs built its own BOOK_PROJECTION (missing until now).
    assert!(book_projection_shape(&conn).contains("FOREIGN KEY"));
    // Idempotent on reopen.
    assert_eq!(migrate(&conn), 0);
}

#[test]
fn fresh_database_still_migrates_normally() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(migrate(&conn), main_migrations().len());
    assert_eq!(migrate(&conn), 0);
}

#[test]
fn objects_without_any_history_are_still_refused() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn);
    conn.execute_batch("DROP TABLE flyway_schema_history")
        .unwrap();
    let err = Migrator::new(&main_migrations(), Placeholders::default())
        .migrate(&conn)
        .unwrap_err();
    assert!(matches!(err, MigrateError::NonEmptySchemaWithoutHistory));
}

#[test]
fn empty_sqlx_history_is_still_refused() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn);
    conn.execute_batch("DROP TABLE flyway_schema_history")
        .unwrap();
    conn.execute_batch(SQLX_DDL).unwrap();
    let err = Migrator::new(&main_migrations(), Placeholders::default())
        .migrate(&conn)
        .unwrap_err();
    assert!(matches!(err, MigrateError::NonEmptySchemaWithoutHistory));
}

#[test]
fn leftover_empty_history_table_is_re_adopted() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn);
    convert_to_komga_riir(&conn);
    // Simulate a failed adoption run from before the transactional DDL fix:
    // an empty `flyway_schema_history` table was left behind. The database
    // must still be openable — the empty history is re-adopted instead of
    // skipping adoption and failing out-of-order forever.
    conn.execute_batch(
        "CREATE TABLE \"flyway_schema_history\" (\"installed_rank\" INT NOT NULL, \"version\" VARCHAR(50), \"description\" VARCHAR(200) NOT NULL, \"type\" VARCHAR(20) NOT NULL, \"script\" VARCHAR(1000) NOT NULL, \"checksum\" INTEGER, \"installed_by\" VARCHAR(100) NOT NULL, \"installed_on\" TIMESTAMP NOT NULL DEFAULT (CURRENT_TIMESTAMP), \"execution_time\" INTEGER NOT NULL, \"success\" BOOLEAN NOT NULL, CONSTRAINT \"flyway_schema_history_pk\" PRIMARY KEY (\"installed_rank\"))"
    )
    .unwrap();

    let applied = migrate(&conn);
    assert_eq!(applied, 1); // kmrs book_projection applied on re-adoption
    assert_eq!(history_count(&conn), main_migrations().len() as i64);
    assert_eq!(sqlx_count(&conn), sql_migration_count() as i64 - 1);
}
