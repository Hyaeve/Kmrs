//! Per-connection UDFs and collations, aligned with `SqliteUdfDataSource.kt`:
//! - `REGEXP(pattern, text)`: case-insensitive contains match (Kotlin
//!   `toRegex(IGNORE_CASE).containsMatchIn`).
//! - `UDF_STRIP_ACCENTS(text)`: commons-lang3 `stripAccents`.
//! - `COLLATION_UNICODE_1`: ICU PRIMARY (for matching); `COLLATION_UNICODE_3`: ICU
//!   TERTIARY (for sorting).
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
    use icu_collator::options::{CollatorOptions, Strength};
    use icu_collator::{Collator, CollatorPreferences};

    let mut options_primary = CollatorOptions::default();
    options_primary.strength = Some(Strength::Primary);
    let mut options_tertiary = CollatorOptions::default();
    options_tertiary.strength = Some(Strength::Tertiary);
    // The Java side uses Collator.getInstance() (default locale, equivalent to root behavior)
    let collator1 = Collator::try_new(CollatorPreferences::default(), options_primary)
        .map_err(|e| Error::UserFunctionError(e.into()))?;
    let collator3 = Collator::try_new(CollatorPreferences::default(), options_tertiary)
        .map_err(|e| Error::UserFunctionError(e.into()))?;

    conn.create_collation(COLLATION_UNICODE_1, move |a, b| collator1.compare(a, b))?;
    conn.create_collation(COLLATION_UNICODE_3, move |a, b| collator3.compare(a, b))?;
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
}
