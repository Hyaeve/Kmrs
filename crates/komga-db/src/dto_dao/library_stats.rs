//! Aggregation queries for the kmrs-private library-stats endpoint: per-library content
//! counts (series, books, cumulated filesize) and list memberships (read lists,
//! collections) under the caller's visibility. Visibility reuses the search-layer SQL
//! fragments (`content_restrictions_condition` / `library_ids_condition`), so the counts
//! match exactly what a search would return. kmrs-private, no Java equivalent.

use crate::error::Result;
use crate::pool::Database;
use crate::search_sql::{RequiredJoin, SqlWhere};
use rusqlite::types::Value;
use std::collections::{BTreeSet, HashMap};

pub struct LibraryStatsDtoDao {
    db: Database,
}

/// One library's visible content counts; libraries without content zero-fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryStatsRow {
    pub library_id: String,
    pub library_name: String,
    pub series: i64,
    pub books: i64,
    pub filesize: i64,
    /// Lists holding at least one visible member of this library (the `belongs_to`
    /// semantics of `ReadListDao`/`SeriesCollectionDao`): a list spanning libraries
    /// counts in each of them.
    pub readlists: i64,
    pub collections: i64,
}

/// Distinct lists holding at least one visible member across all visible libraries.
/// The total row needs these queries of its own: per-library counts sum a spanning list
/// once per touched library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MembershipTotals {
    pub readlists: i64,
    pub collections: i64,
}

/// Global content counts with no visibility scoping, for the admin server snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentTotals {
    pub libraries: i64,
    pub series: i64,
    pub books: i64,
    pub filesize: i64,
    pub collections: i64,
    pub readlists: i64,
    pub sidecars: i64,
}

/// SERIES_METADATA joins on its primary key (1:1), so when the visibility fragment does
/// not reference it the join cannot change the result and is skipped.
fn series_metadata_join(visibility: &SqlWhere, series_id_column: &str) -> String {
    if visibility.joins.contains(&RequiredJoin::SeriesMetadata) {
        format!(" LEFT JOIN SERIES_METADATA ON ({series_id_column} = SERIES_METADATA.SERIES_ID)")
    } else {
        String::new()
    }
}

fn where_clause(visibility: &SqlWhere) -> String {
    if visibility.sql.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", visibility.sql)
    }
}

fn readlist_from(book: &SqlWhere) -> String {
    format!(
        " FROM READLIST_BOOK INNER JOIN BOOK ON (READLIST_BOOK.BOOK_ID = BOOK.ID){}{}",
        series_metadata_join(book, "BOOK.SERIES_ID"),
        where_clause(book),
    )
}

fn collection_from(series: &SqlWhere) -> String {
    format!(
        " FROM COLLECTION_SERIES INNER JOIN SERIES ON (COLLECTION_SERIES.SERIES_ID = SERIES.ID){}{}",
        series_metadata_join(series, "SERIES.ID"),
        where_clause(series),
    )
}

/// (library_id, count) rows of a GROUP BY query.
fn count_rows(
    conn: &rusqlite::Connection,
    sql: &str,
    params: Vec<Value>,
) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn fill(
    rows: &mut [LibraryStatsRow],
    index: &HashMap<String, usize>,
    counts: Vec<(String, i64)>,
    set: impl Fn(&mut LibraryStatsRow, i64),
) {
    for (library_id, count) in counts {
        if let Some(&i) = index.get(&library_id) {
            set(&mut rows[i], count);
        }
    }
}

impl LibraryStatsDtoDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Every library in `library_ids` (None = all), ordered by name like
    /// `LibraryDao.find_all`, each with its visible content counts. `series`/`book` carry
    /// the full visibility of their table (content restrictions + library filter).
    pub fn per_library(
        &self,
        library_ids: Option<&BTreeSet<String>>,
        series: &SqlWhere,
        book: &SqlWhere,
    ) -> Result<Vec<LibraryStatsRow>> {
        if library_ids.is_some_and(BTreeSet::is_empty) {
            return Ok(vec![]);
        }
        let conn = self.db.ro()?;

        let (lib_sql, lib_params) = match library_ids {
            None => (
                "SELECT ID, NAME FROM LIBRARY ORDER BY NAME".to_string(),
                vec![],
            ),
            Some(ids) => {
                let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                (
                    format!("SELECT ID, NAME FROM LIBRARY WHERE ID IN ({ph}) ORDER BY NAME"),
                    ids.iter().map(|id| Value::Text(id.clone())).collect(),
                )
            }
        };
        let mut stmt = conn.prepare(&lib_sql)?;
        let libraries = stmt
            .query_map(rusqlite::params_from_iter(lib_params), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut rows: Vec<LibraryStatsRow> = libraries
            .into_iter()
            .map(|(library_id, library_name)| LibraryStatsRow {
                library_id,
                library_name,
                series: 0,
                books: 0,
                filesize: 0,
                readlists: 0,
                collections: 0,
            })
            .collect();
        let index: HashMap<String, usize> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (r.library_id.clone(), i))
            .collect();

        let series_sql = format!(
            "SELECT SERIES.LIBRARY_ID, COUNT(*) FROM SERIES{}{} GROUP BY SERIES.LIBRARY_ID",
            series_metadata_join(series, "SERIES.ID"),
            where_clause(series),
        );
        let counts = count_rows(&conn, &series_sql, series.params.clone())?;
        fill(&mut rows, &index, counts, |row, count| row.series = count);

        let book_sql = format!(
            "SELECT BOOK.LIBRARY_ID, COUNT(*), COALESCE(SUM(BOOK.FILE_SIZE), 0) FROM BOOK{}{} \
             GROUP BY BOOK.LIBRARY_ID",
            series_metadata_join(book, "BOOK.SERIES_ID"),
            where_clause(book),
        );
        let mut stmt = conn.prepare(&book_sql)?;
        let book_counts = stmt
            .query_map(
                rusqlite::params_from_iter(book.params.iter().cloned()),
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        for (library_id, count, filesize) in book_counts {
            if let Some(&i) = index.get(&library_id) {
                rows[i].books = count;
                rows[i].filesize = filesize;
            }
        }

        // membership counts follow the members' visibility: a list whose only members
        // in this library are invisible to the caller does not count
        let readlist_sql = format!(
            "SELECT BOOK.LIBRARY_ID, COUNT(DISTINCT READLIST_BOOK.READLIST_ID){} \
             GROUP BY BOOK.LIBRARY_ID",
            readlist_from(book),
        );
        let counts = count_rows(&conn, &readlist_sql, book.params.clone())?;
        fill(&mut rows, &index, counts, |row, count| {
            row.readlists = count;
        });

        let collection_sql = format!(
            "SELECT SERIES.LIBRARY_ID, COUNT(DISTINCT COLLECTION_SERIES.COLLECTION_ID){} \
             GROUP BY SERIES.LIBRARY_ID",
            collection_from(series),
        );
        let counts = count_rows(&conn, &collection_sql, series.params.clone())?;
        fill(&mut rows, &index, counts, |row, count| {
            row.collections = count;
        });

        Ok(rows)
    }

    pub fn content_totals(&self) -> Result<ContentTotals> {
        let conn = self.db.ro()?;
        let count = |table: &str| {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })
        };
        let (books, filesize) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(FILE_SIZE), 0) FROM BOOK",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )?;
        Ok(ContentTotals {
            libraries: count("LIBRARY")?,
            series: count("SERIES")?,
            books,
            filesize,
            collections: count("COLLECTION")?,
            readlists: count("READLIST")?,
            sidecars: count("SIDECAR")?,
        })
    }

    /// Distinct list counts over all visible libraries: the total row's read lists and
    /// collections. `series`/`book` are the same visibility fragments as `per_library`.
    pub fn membership_totals(
        &self,
        series: &SqlWhere,
        book: &SqlWhere,
    ) -> Result<MembershipTotals> {
        let conn = self.db.ro()?;

        let readlist_sql = format!(
            "SELECT COUNT(DISTINCT READLIST_BOOK.READLIST_ID){}",
            readlist_from(book)
        );
        let readlists: i64 = conn.query_row(
            &readlist_sql,
            rusqlite::params_from_iter(book.params.iter().cloned()),
            |r| r.get(0),
        )?;

        let collection_sql = format!(
            "SELECT COUNT(DISTINCT COLLECTION_SERIES.COLLECTION_ID){}",
            collection_from(series)
        );
        let collections: i64 = conn.query_row(
            &collection_sql,
            rusqlite::params_from_iter(series.params.iter().cloned()),
            |r| r.get(0),
        )?;

        Ok(MembershipTotals {
            readlists,
            collections,
        })
    }
}
