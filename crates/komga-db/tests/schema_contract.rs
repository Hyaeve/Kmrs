//! Schema contract: the schema produced by `Migrator` must equal the schema obtained
//! by executing the raw migration SQL files directly. The files under migrations/ are
//! byte-for-byte copies of komga's Flyway migrations (reconciled by
//! `cargo xtask sync-migrations`), so the direct-execution schema is what Java komga
//! produces. This catches drift between the embedded migration list and the files on
//! disk, ordering bugs, placeholder substitution errors, and DDL leaking out of the
//! Java (JDBC) migration ports.
//!
//! `tests/diff/diff.py` covers runtime API behavior against a live Java instance;
//! this test is its hermetic schema-level counterpart.

use komga_db::migrate::Migrator;
use komga_db::{main_migrations, tasks_migrations, Placeholders};
use rusqlite::Connection;
use std::path::Path;

// Flyway placeholder defaults from komga's application.yml, hardcoded here (not via
// `Placeholders::default`) so a wrong Rust default cannot cancel out on both sides.
const PLACEHOLDERS: &[(&str, &str)] = &[
    ("${library-file-hashing}", "true"),
    ("${library-scan-startup}", "false"),
    ("${delete-empty-collections}", "true"),
    ("${delete-empty-read-lists}", "true"),
];

/// Runs the raw SQL files of `dir` in filename order, like Flyway would.
fn oracle(dir: &str) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    let dir_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
    let mut files: Vec<String> = std::fs::read_dir(&dir_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir_path.display()))
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".sql"))
        .collect();
    files.sort();
    for file in files {
        let mut sql = std::fs::read_to_string(dir_path.join(&file)).unwrap();
        for (key, value) in PLACEHOLDERS {
            sql = sql.replace(key, value);
        }
        conn.execute_batch(&sql)
            .unwrap_or_else(|e| panic!("oracle migration {file} failed: {e}"));
    }
    conn
}

fn normalize(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ,", ",")
        .replace(" )", ")")
        .replace("( ", "(")
}

/// (type, name, tbl_name, normalized sql) of every user object, in a stable order.
fn inventory(conn: &Connection) -> Vec<(String, String, String, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_master \
             WHERE type IN ('table', 'index', 'trigger', 'view') \
             AND name NOT LIKE 'sqlite_%' AND tbl_name <> 'flyway_schema_history' \
             ORDER BY type, name",
        )
        .unwrap();
    stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            normalize(&r.get::<_, String>(3)?),
        ))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[test]
fn main_db_schema_matches_raw_migrations() {
    let migrated = Connection::open_in_memory().unwrap();
    let migrations = main_migrations();
    Migrator::new(&migrations, Placeholders::default())
        .migrate(&migrated)
        .unwrap();

    let expected = inventory(&oracle("migrations"));
    let actual = inventory(&migrated);
    assert_eq!(actual, expected);
}

#[test]
fn tasks_db_schema_matches_raw_migrations() {
    let migrated = Connection::open_in_memory().unwrap();
    let migrations = tasks_migrations();
    Migrator::new(&migrations, Placeholders::default())
        .migrate(&migrated)
        .unwrap();

    let expected = inventory(&oracle("migrations_tasks"));
    let actual = inventory(&migrated);
    assert_eq!(actual, expected);
}
