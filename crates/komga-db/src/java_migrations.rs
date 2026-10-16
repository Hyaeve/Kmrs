//! Rust ports of komga's 5 Java migrations (`komga/src/flyway/kotlin/db/migration/sqlite/`).
//! They only run when upgrading a database from an older komga version; on a fresh
//! database they are all no-ops.
//! Recorded in the history as type='JDBC', checksum NULL, script=Java class FQN,
//! matching Flyway.

use super::migrate::{MigrateError, Migration};
use komga_core::natural_sort::strip_accents;
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Connection};
use std::io::{Read, Write};

pub fn java_migrations() -> Vec<Migration> {
    vec![
        Migration::Jdbc {
            class_name: "db.migration.sqlite.V20200810154730__thumbnails_part_2",
            apply: thumbnails_part_2,
        },
        Migration::Jdbc {
            class_name: "db.migration.sqlite.V20200820150923__metadata_fields_part_2",
            apply: metadata_fields_part_2,
        },
        Migration::Jdbc {
            class_name: "db.migration.sqlite.V20210624165023__missing_series_metadata",
            apply: missing_series_metadata,
        },
        Migration::Jdbc {
            class_name: "db.migration.sqlite.V20230801104436__fix_incorrect_language_codes",
            apply: fix_incorrect_language_codes,
        },
        Migration::Jdbc {
            class_name: "db.migration.sqlite.V20240422132621__fix_read_progress_locators",
            apply: fix_read_progress_locators,
        },
    ]
}

/// V20200810154730: move MEDIA.THUMBNAIL into the newly created THUMBNAIL_BOOK, adding TSIDs.
fn thumbnails_part_2(conn: &Connection, tsid: &TsidFactory) -> Result<(), MigrateError> {
    let mut stmt = conn.prepare("SELECT THUMBNAIL, BOOK_ID FROM MEDIA")?;
    let rows: Vec<(Option<Vec<u8>>, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (thumbnail, book_id) in rows {
        conn.execute(
      "INSERT INTO THUMBNAIL_BOOK(ID, THUMBNAIL, SELECTED, TYPE, BOOK_ID) values (?, ?, 1, 'GENERATED', ?)",
      params![tsid.create_string(), thumbnail, book_id],
    )?;
    }
    Ok(())
}

/// V20200820150923: backfill AGE_RATING/PUBLISHER/READING_DIRECTION and their lock
/// flags per series.
fn metadata_fields_part_2(conn: &Connection, _tsid: &TsidFactory) -> Result<(), MigrateError> {
    let mut stmt = conn.prepare(
    "select m.AGE_RATING, m.AGE_RATING_LOCK, m.PUBLISHER, m.PUBLISHER_LOCK, m.READING_DIRECTION, m.READING_DIRECTION_LOCK, b.SERIES_ID, m.NUMBER_SORT \
     from BOOK_METADATA m left join BOOK B on B.ID = m.BOOK_ID",
  )?;
    #[derive(Default)]
    struct Row {
        age_rating: Option<i64>,
        age_rating_lock: Option<i64>,
        publisher: String,
        publisher_lock: Option<i64>,
        reading_direction: Option<String>,
        reading_direction_lock: Option<i64>,
        series_id: Option<String>,
        number_sort: Option<f64>,
    }
    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok(Row {
                age_rating: r.get(0)?,
                age_rating_lock: r.get(1)?,
                publisher: r.get(2)?,
                publisher_lock: r.get(3)?,
                reading_direction: r.get(4)?,
                reading_direction_lock: r.get(5)?,
                series_id: r.get(6)?,
                number_sort: r.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut groups: std::collections::HashMap<String, Vec<Row>> = std::collections::HashMap::new();
    for row in rows {
        if let Some(series_id) = row.series_id.clone() {
            groups.entry(series_id).or_default().push(row);
        }
    }

    for (series_id, v) in groups {
        let age_rating = v.iter().filter_map(|r| r.age_rating).max();
        let age_rating_lock = v.iter().filter_map(|r| r.age_rating_lock).max();
        let publisher = v
            .iter()
            .filter(|r| !r.publisher.is_empty())
            .max_by(|a, b| {
                a.number_sort
                    .partial_cmp(&b.number_sort)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.publisher.clone())
            .unwrap_or_default();
        let publisher_lock = v.iter().filter_map(|r| r.publisher_lock).max();
        // Kotlin groupingBy.eachCount + maxByOrNull: ties resolve to the first one seen
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for dir in v.iter().filter_map(|r| r.reading_direction.as_deref()) {
            match counts.iter_mut().find(|(k, _)| *k == dir) {
                Some((_, n)) => *n += 1,
                None => counts.push((dir, 1)),
            }
        }
        let reading_dir = counts.iter().max_by_key(|(_, n)| *n).map(|(k, _)| *k);
        let reading_dir_lock = v.iter().filter_map(|r| r.reading_direction_lock).max();

        conn.execute(
      "UPDATE SERIES_METADATA SET AGE_RATING = ?, AGE_RATING_LOCK = ?, PUBLISHER = ?, PUBLISHER_LOCK = ?, READING_DIRECTION = ?, READING_DIRECTION_LOCK = ? WHERE SERIES_ID = ?",
      params![
        age_rating,
        age_rating_lock,
        publisher,
        publisher_lock,
        reading_dir,
        reading_dir_lock,
        series_id
      ],
    )?;
    }
    Ok(())
}

/// V20210624165023: insert default metadata rows and aggregation rows for series
/// missing SERIES_METADATA.
fn missing_series_metadata(conn: &Connection, _tsid: &TsidFactory) -> Result<(), MigrateError> {
    let mut stmt = conn.prepare(
    "select s.ID, s.NAME from SERIES s where s.ID not in (select sm.SERIES_ID from SERIES_METADATA sm)",
  )?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (id, name) in &rows {
        conn.execute(
      "INSERT INTO SERIES_METADATA(SERIES_ID, STATUS, TITLE, TITLE_SORT, READING_DIRECTION, AGE_RATING) VALUES (?,'ONGOING',?,?,?,?)",
      params![id, name, strip_accents(name), Option::<String>::None, Option::<i64>::None],
    )?;
        conn.execute(
            "INSERT INTO BOOK_METADATA_AGGREGATION(SERIES_ID, RELEASE_DATE) VALUES (?,?)",
            params![id, Option::<String>::None],
        )?;
    }
    Ok(())
}

/// V20230801104436: normalize LANGUAGE to BCP47 (ICU `ULocale.forLanguageTag().toLanguageTag()`).
///
/// Note: ICU4X does not replace deprecated aliases (e.g. iw→he), a known deviation
/// from ICU4J for such inputs; it only affects libraries created before 2023-08 with
/// a deprecated alias as language code, and the difference is accepted.
fn fix_incorrect_language_codes(
    conn: &Connection,
    _tsid: &TsidFactory,
) -> Result<(), MigrateError> {
    let mut stmt = conn
    .prepare("select m.SERIES_ID, m.LANGUAGE from SERIES_METADATA m where LANGUAGE <> '' and LANGUAGE <> 'en'")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (series_id, language) in rows {
        let normalized = normalize_language_tag(&language);
        if normalized != language {
            conn.execute(
                "update SERIES_METADATA set LANGUAGE = ? where SERIES_ID = ?",
                params![normalized, series_id],
            )?;
        }
    }
    Ok(())
}

fn normalize_language_tag(value: &str) -> String {
    if value.trim().is_empty() {
        return String::new();
    }
    use std::str::FromStr;
    icu_locale::Locale::from_str(value)
        .map(|l| l.to_string())
        .unwrap_or_default()
}

/// V20240422132621: fix the href in READ_PROGRESS.LOCATOR (gzip+JSON), keeping only
/// the path after /resource/.
fn fix_read_progress_locators(conn: &Connection, _tsid: &TsidFactory) -> Result<(), MigrateError> {
    let mut stmt = conn.prepare(
        "select r.BOOK_ID, r.USER_ID, r.LOCATOR from READ_PROGRESS r where LOCATOR is not null",
    )?;
    let rows: Vec<(String, String, Vec<u8>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (book_id, user_id, locator) in rows {
        // The Java version catches all exceptions per row and skips
        let _ = fix_one_locator(conn, &book_id, &user_id, &locator);
    }
    Ok(())
}

fn fix_one_locator(
    conn: &Connection,
    book_id: &str,
    user_id: &str,
    locator: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut json_bytes = Vec::new();
    flate2::read::GzDecoder::new(locator).read_to_end(&mut json_bytes)?;
    let mut value: serde_json::Value = serde_json::from_slice(&json_bytes)?;
    let Some(href) = value.get("href").and_then(|h| h.as_str()) else {
        return Ok(());
    };
    let correct = match href.find("/resource/") {
        Some(idx) => &href[idx + "/resource/".len()..],
        None => href,
    };
    value["href"] = serde_json::Value::String(correct.to_string());
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(serde_json::to_string(&value)?.as_bytes())?;
    let gz = encoder.finish()?;
    conn.execute(
        "update READ_PROGRESS set LOCATOR = ? where BOOK_ID = ? and USER_ID = ?",
        params![gz, book_id, user_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tag_normalize() {
        assert_eq!(normalize_language_tag("EN"), "en");
        assert_eq!(normalize_language_tag("zh-hans-cn"), "zh-Hans-CN");
        assert_eq!(normalize_language_tag("EN-us"), "en-US");
        // ISO 639-3 three-letter codes are kept as-is (ICU4J likewise does not map eng→en)
        assert_eq!(normalize_language_tag("eng"), "eng");
        assert_eq!(normalize_language_tag("not a tag"), "");
        assert_eq!(normalize_language_tag("  "), "");
    }

    #[test]
    fn locator_href_fix() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
      .write_all(br#"{"href":"/api/v1/books/xxx/resource/OEBPS/images/cover.jpg","type":"image/jpeg"}"#)
      .unwrap();
        let gz = encoder.finish().unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE READ_PROGRESS(BOOK_ID TEXT, USER_ID TEXT, LOCATOR BLOB);")
            .unwrap();
        conn.execute(
            "INSERT INTO READ_PROGRESS VALUES ('b1','u1',?)",
            params![gz],
        )
        .unwrap();

        fix_read_progress_locators(&conn, &TsidFactory::new(1)).unwrap();

        let out: Vec<u8> = conn
            .query_row("SELECT LOCATOR FROM READ_PROGRESS", [], |r| r.get(0))
            .unwrap();
        let mut json_bytes = Vec::new();
        flate2::read::GzDecoder::new(&out[..])
            .read_to_end(&mut json_bytes)
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json_bytes).unwrap();
        assert_eq!(value["href"], "OEBPS/images/cover.jpg");
        assert_eq!(value["type"], "image/jpeg");
    }
}
