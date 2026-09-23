//! Migration integration test: runs all 92 main-database migrations plus the
//! tasks-database migrations, and verifies the history table and final schema.

use komga_db::migrate::{MigrateError, Migrator, Placeholders};
use komga_db::{main_migrations, tasks_migrations};
use rusqlite::Connection;

fn migrate_main(conn: &Connection) -> usize {
    let migrations = main_migrations();
    Migrator::new(&migrations, Placeholders::default())
        .migrate(conn)
        .expect("main migrations failed")
}

#[test]
fn fresh_db_applies_all_migrations() {
    let conn = Connection::open_in_memory().unwrap();
    let applied = migrate_main(&conn);
    assert_eq!(applied, 92, "87 SQL + 5 JDBC migrations");

    // history table: 92 rows all successful, 5 of them JDBC with NULL checksum
    let (total, jdbc, null_checksum): (i64, i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), \
       COALESCE(SUM(type = 'JDBC'), 0), \
       COALESCE(SUM(checksum IS NULL), 0) \
       FROM flyway_schema_history WHERE success",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(total, 92);
    assert_eq!(jdbc, 5);
    assert_eq!(null_checksum, 5);

    // idempotent: running again leaves no pending migrations
    assert_eq!(migrate_main(&conn), 0);

    // key tables and the view exist
    for name in [
        "LIBRARY",
        "SERIES",
        "SERIES_METADATA",
        "BOOK",
        "BOOK_METADATA",
        "BOOK_METADATA_AGGREGATION",
        "BOOK_PROJECTION",
        "MEDIA",
        "MEDIA_PAGE",
        "MEDIA_FILE",
        "THUMBNAIL_BOOK",
        "THUMBNAIL_SERIES",
        "THUMBNAIL_COLLECTION",
        "THUMBNAIL_READLIST",
        "USER",
        "USER_ROLE",
        "USER_API_KEY",
        "READ_PROGRESS",
        "READ_PROGRESS_SERIES",
        "COLLECTION",
        "READLIST",
        "SIDECAR",
        "SERVER_SETTINGS",
        "CLIENT_SETTINGS_GLOBAL",
        "CLIENT_SETTINGS_USER",
        "SYNC_POINT",
        "PAGE_HASH",
        "PAGE_HASH_THUMBNAIL",
        "HISTORICAL_EVENT",
        "AUTHENTICATION_ACTIVITY",
        "ANNOUNCEMENTS_READ",
        "LIBRARY_EXCLUSIONS",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?",
                [name],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "table {name} missing");
    }
    let view_exists: bool = conn
    .query_row(
      "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'view' AND name = 'SERIES_AND_BOOK_TAG'",
      [],
      |r| r.get(0),
    )
    .unwrap();
    assert!(view_exists, "view SERIES_AND_BOOK_TAG missing");

    // no triggers, no FTS
    let triggers: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(triggers, 0);

    // placeholder substitution took effect: initial SERVER_SETTINGS keys (the boolean
    // literal true is stored as '1' via TEXT affinity, same as the Java side)
    let delete_empty: String = conn
        .query_row(
            "SELECT VALUE FROM SERVER_SETTINGS WHERE KEY = 'DELETE_EMPTY_COLLECTIONS'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(delete_empty, "1");
}

#[test]
fn checksum_mismatch_is_detected() {
    let conn = Connection::open_in_memory().unwrap();
    migrate_main(&conn);
    // tamper with the checksum of an applied migration
    conn.execute(
        "UPDATE flyway_schema_history SET checksum = 42 WHERE version = '20200706141854'",
        [],
    )
    .unwrap();
    let migrations = main_migrations();
    let err = Migrator::new(&migrations, Placeholders::default())
        .migrate(&conn)
        .unwrap_err();
    assert!(
        matches!(err, MigrateError::ChecksumMismatch { .. }),
        "{err:?}"
    );
}

#[test]
fn unknown_applied_version_is_detected() {
    let conn = Connection::open_in_memory().unwrap();
    migrate_main(&conn);
    conn
    .execute(
      "INSERT INTO flyway_schema_history \
       (installed_rank, version, description, type, script, checksum, installed_by, execution_time, success) \
       VALUES (999, '99999999999999', 'from the future', 'SQL', 'V99999999999999__future.sql', 1, '', 0, 1)",
      [],
    )
    .unwrap();
    let migrations = main_migrations();
    let err = Migrator::new(&migrations, Placeholders::default())
        .migrate(&conn)
        .unwrap_err();
    assert!(
        matches!(err, MigrateError::AppliedNotResolved(_)),
        "{err:?}"
    );
}

#[test]
fn tasks_db_migrations() {
    let conn = Connection::open_in_memory().unwrap();
    let migrations = tasks_migrations();
    let applied = Migrator::new(&migrations, Placeholders::default())
        .migrate(&conn)
        .unwrap();
    assert_eq!(applied, 1);
    let task_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'TASK'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(task_table);
}
