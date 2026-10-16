//! Per-connection UDFs and collations, aligned with `SqliteUdfDataSource.kt`:
//! - `REGEXP(pattern, text)`: case-insensitive contains match (Kotlin
//!   `toRegex(IGNORE_CASE).containsMatchIn`).
//! - `UDF_STRIP_ACCENTS(text)`: commons-lang3 `stripAccents`.
//! - `COLLATION_UNICODE_1`: ICU PRIMARY (for matching); `COLLATION_UNICODE_3`: ICU
//!   TERTIARY **natural sort** — text segments follow the configured sort locale,
//!   numeric segments compare by value (an extension over the Kotlin plain ICU
//!   ordering: "Page 2" sorts before "Page 10").
//!
//! These are not stored in the database file and must be registered once per
//! connection.

use komga_core::natural_sort::strip_accents;
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, Error};

pub const UDF_STRIP_ACCENTS: &str = "UDF_STRIP_ACCENTS";
pub const COLLATION_UNICODE_1: &str = "COLLATION_UNICODE_1";
pub const COLLATION_UNICODE_3: &str = "COLLATION_UNICODE_3";

pub fn register_all(conn: &Connection) -> rusqlite::Result<()> {
    create_regexp(conn)?;
    create_strip_accents(conn)?;
    create_unicode_collations(conn)?;
    Ok(())
}

fn create_regexp(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        "regexp",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let pattern = ctx.get::<String>(0).unwrap_or_default();
            let text = ctx.get::<String>(1).unwrap_or_default();
            let re = regex::RegexBuilder::new(&pattern)
                .case_insensitive(true)
                .build()
                .map_err(|e| Error::UserFunctionError(e.into()))?;
            Ok(if re.is_match(&text) { 1 } else { 0 })
        },
    )
}

fn create_strip_accents(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        UDF_STRIP_ACCENTS,
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let text: Option<String> = ctx.get(0)?;
            match text {
                None => Err(Error::UserFunctionError("Argument must not be null".into())),
                Some(text) => Ok(strip_accents(&text)),
            }
        },
    )
}

fn create_unicode_collations(conn: &Connection) -> rusqlite::Result<()> {
    // Locale comes from KOMGA_SORT_LOCALE / server.sort-locale (set once at
    // startup, default `und`), reproducing the previous
    // `CollatorPreferences::default()` root behavior unless configured.
    let collator1 = komga_core::sort_locale::primary_collator();

    // COLLATION_UNICODE_1 is used for equality/matching (case/accent-insensitive),
    // so it must stay a plain ICU compare: a tie-break would break `=` semantics.
    // COLLATION_UNICODE_3 is used only for ORDER BY and is an ICU-based natural
    // sort: text segments follow the configured locale (with a raw-string
    // tie-break so canonically equivalent strings like "á" vs "a\u{301}" keep
    // deterministic ORDER BY ... LIMIT boundaries), numeric segments compare by
    // value.
    conn.create_collation(COLLATION_UNICODE_1, move |a, b| collator1.compare(a, b))?;
    conn.create_collation(
        COLLATION_UNICODE_3,
        komga_core::sort_locale::compare_natural,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        register_all(&conn).unwrap();
        conn
    }

    #[test]
    fn regexp_is_case_insensitive_contains() {
        let conn = conn();
        let n: i64 = conn
            .query_row("SELECT 'Hello World' REGEXP 'hello w.*'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let n: i64 = conn
            .query_row("SELECT 'Hello' REGEXP '^hell'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let n: i64 = conn
            .query_row("SELECT 'Hello' REGEXP 'xyz'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn strip_accents_udf() {
        let conn = conn();
        let s: String = conn
            .query_row("SELECT UDF_STRIP_ACCENTS('Héllo')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(s, "Hello");
    }

    #[test]
    fn collations_order_and_match() {
        let conn = conn();
        // TERTIARY: different case is not equal but close; PRIMARY: case/accent insensitive
        let n: i64 = conn
      .query_row(
        "SELECT COUNT(*) FROM (SELECT 'a' x UNION ALL SELECT 'B' UNION ALL SELECT 'c') ORDER BY x COLLATE COLLATION_UNICODE_3",
        [],
        |r| r.get(0),
      )
      .unwrap();
        assert_eq!(n, 3);
        let eq: i64 = conn
            .query_row(
                "SELECT 'héllo' = 'HELLO' COLLATE COLLATION_UNICODE_1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(eq, 1);
        let eq: i64 = conn
            .query_row(
                "SELECT 'héllo' = 'hello' COLLATE COLLATION_UNICODE_3",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(eq, 0);
    }

    #[test]
    fn collations_break_canonical_equivalence_ties() {
        // ICU treats "á" and "a\u{301}" as equal; the raw-string tie-break makes
        // ORDER BY deterministic regardless of the input row order.
        let conn = conn();
        let rows: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT x FROM (SELECT 'á' x UNION ALL SELECT 'a' UNION ALL SELECT 'a\u{301}') ORDER BY x COLLATE COLLATION_UNICODE_3")
                .unwrap();
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        assert_eq!(rows, vec!["a", "a\u{301}", "á"]);
    }

    #[test]
    fn unicode_3_orders_numeric_segments_naturally_through_sql() {
        // The headline behavior of COLLATION_UNICODE_3 is the numeric natural
        // sort: through the registered collation, 'Page 2' must come before
        // 'Page 10' (plain tertiary would order 'Page 10' first).
        let conn = conn();
        conn.execute_batch(
            "CREATE TABLE t (x TEXT);
             INSERT INTO t VALUES ('Page 10'), ('Page 2'), ('Page 2a');",
        )
        .unwrap();
        let rows: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT x FROM t ORDER BY x COLLATE COLLATION_UNICODE_3")
                .unwrap();
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        assert_eq!(rows, vec!["Page 2", "Page 2a", "Page 10"]);
    }
}
