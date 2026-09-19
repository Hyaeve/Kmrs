//! DAO for BOOK / BOOK_METADATA (including the AUTHOR/TAG/LINK child tables).

use super::{get_date, get_datetime, get_datetime_opt};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::book::{Author, Book, BookMetadata, WebLink};
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};

const BOOK_COLUMNS: &str =
    "ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, FILE_SIZE, NUMBER, \
 FILE_HASH, FILE_HASH_KOREADER, DELETED_DATE, ONESHOT, CREATED_DATE, LAST_MODIFIED_DATE";

const METADATA_COLUMNS: &str = "BOOK_ID, TITLE, TITLE_LOCK, SUMMARY, SUMMARY_LOCK, NUMBER, NUMBER_LOCK, \
 NUMBER_SORT, NUMBER_SORT_LOCK, RELEASE_DATE, RELEASE_DATE_LOCK, AUTHORS_LOCK, TAGS_LOCK, ISBN, ISBN_LOCK, \
 LINKS_LOCK, CREATED_DATE, LAST_MODIFIED_DATE";

pub struct BookDao {
    db: Database,
    tsid: TsidFactory,
}

impl BookDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_book(row: &Row<'_>) -> rusqlite::Result<Book> {
        Ok(Book {
            id: row.get(0)?,
            name: row.get(1)?,
            url: row.get(2)?,
            file_last_modified: get_datetime(row, 3)?,
            series_id: row.get(4)?,
            library_id: row.get(5)?,
            file_size: row.get(6)?,
            number: row.get(7)?,
            file_hash: row.get(8)?,
            file_hash_koreader: row.get(9)?,
            deleted_date: get_datetime_opt(row, 10)?,
            oneshot: row.get(11)?,
            created_date: get_datetime(row, 12)?,
            last_modified_date: get_datetime(row, 13)?,
        })
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<Book>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {BOOK_COLUMNS} FROM BOOK WHERE ID = ?"))?;
        let mut rows = stmt.query_map([id], Self::row_to_book)?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_library_id_or_null(&self, book_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT LIBRARY_ID FROM BOOK WHERE ID = ?")?;
        let mut rows = stmt.query_map([book_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_series_id_or_null(&self, book_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT SERIES_ID FROM BOOK WHERE ID = ?")?;
        let mut rows = stmt.query_map([book_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_by_series_id(&self, series_id: &str) -> Result<Vec<Book>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM BOOK WHERE SERIES_ID = ?"
        ))?;
        let books = stmt
            .query_map([series_id], Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    pub fn find_all(&self) -> Result<Vec<Book>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {BOOK_COLUMNS} FROM BOOK"))?;
        let books = stmt
            .query_map([], Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    /// `BookRepository.findAll(condition, context, pageable)`: domain-level conditional query.
    /// Sortable properties: `createdDate`, `seriesId`, `number` (others are ignored).
    pub fn find_all_by_condition(
        &self,
        condition: Option<&komga_core::search::SearchConditionBook>,
        ctx: &komga_core::search::SearchContext,
        sort: &[crate::dto_dao::SortOrder],
    ) -> Result<Vec<Book>> {
        let w = crate::search_sql::book_condition(condition, ctx);
        let mut join_sql = String::new();
        let mut join_params: Vec<rusqlite::types::Value> = vec![];
        for join in &w.joins {
            match join {
                crate::search_sql::RequiredJoin::BookMetadata => join_sql
                    .push_str(" INNER JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID"),
                crate::search_sql::RequiredJoin::SeriesMetadata => join_sql.push_str(
                    " INNER JOIN SERIES_METADATA ON BOOK.SERIES_ID = SERIES_METADATA.SERIES_ID",
                ),
                crate::search_sql::RequiredJoin::Media => {
                    join_sql.push_str(" INNER JOIN MEDIA ON BOOK.ID = MEDIA.BOOK_ID")
                }
                crate::search_sql::RequiredJoin::ReadProgress(user_id) => {
                    join_sql.push_str(
                        " LEFT JOIN READ_PROGRESS ON (BOOK.ID = READ_PROGRESS.BOOK_ID AND READ_PROGRESS.USER_ID = ?)",
                    );
                    join_params.push(rusqlite::types::Value::Text(user_id.clone()));
                }
                crate::search_sql::RequiredJoin::ReadList(id) => {
                    let alias = crate::search_sql::readlist_alias(id);
                    join_sql.push_str(&format!(
                        " LEFT JOIN READLIST_BOOK AS \"{alias}\" ON (\"{alias}\".BOOK_ID = BOOK.ID AND \"{alias}\".READLIST_ID = ?)"
                    ));
                    join_params.push(rusqlite::types::Value::Text(id.clone()));
                }
                _ => {}
            }
        }
        let order_by = sort
            .iter()
            .filter_map(|o| {
                let column = match o.property.as_str() {
                    "createdDate" => "BOOK.CREATED_DATE",
                    "seriesId" => "BOOK.SERIES_ID",
                    "number" => "BOOK.NUMBER",
                    _ => return None,
                };
                Some(format!(
                    "{} {}",
                    column,
                    if o.descending { "DESC" } else { "ASC" }
                ))
            })
            .collect::<Vec<_>>();
        // columns are qualified: the dynamic joins (MEDIA, BOOK_METADATA, ...) share column names
        let columns = BOOK_COLUMNS
            .split(',')
            .map(|c| format!("BOOK.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!("SELECT {columns} FROM BOOK{join_sql}");
        if !w.sql.is_empty() {
            sql.push_str(&format!(" WHERE {}", w.sql));
        }
        if !order_by.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", order_by.join(", ")));
        }
        let params: Vec<rusqlite::types::Value> = join_params.into_iter().chain(w.params).collect();
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&sql)?;
        let books = stmt
            .query_map(rusqlite::params_from_iter(params), Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    pub fn find_all_by_series_ids(&self, series_ids: &[String]) -> Result<Vec<Book>> {
        if series_ids.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.db.ro();
        let placeholders = series_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM BOOK WHERE SERIES_ID IN ({placeholders})"
        ))?;
        let books = stmt
            .query_map(
                rusqlite::params_from_iter(series_ids.iter()),
                Self::row_to_book,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    pub fn find_all_not_deleted_by_library_id_and_url_not_in(
        &self,
        library_id: &str,
        urls: &[String],
    ) -> Result<Vec<Book>> {
        let conn = self.db.ro();
        let sql = if urls.is_empty() {
            format!("SELECT {BOOK_COLUMNS} FROM BOOK WHERE LIBRARY_ID = ? AND DELETED_DATE IS NULL")
        } else {
            let placeholders = urls.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            format!(
                "SELECT {BOOK_COLUMNS} FROM BOOK WHERE LIBRARY_ID = ? AND DELETED_DATE IS NULL AND URL NOT IN ({placeholders})"
            )
        };
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(library_id.to_string())];
        params.extend(
            urls.iter()
                .map(|u| Box::new(u.clone()) as Box<dyn rusqlite::ToSql>),
        );
        let mut stmt = conn.prepare(&sql)?;
        let books = stmt
            .query_map(rusqlite::params_from_iter(params), Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    pub fn find_all_deleted_by_file_size(&self, file_size: i64) -> Result<Vec<Book>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM BOOK WHERE DELETED_DATE IS NOT NULL AND FILE_SIZE = ?"
        ))?;
        let books = stmt
            .query_map([file_size], Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    /// `BookRepository.findAllByHashKoreader`
    pub fn find_all_by_hash_koreader(&self, hash_koreader: &str) -> Result<Vec<Book>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM BOOK WHERE FILE_HASH_KOREADER = ?"
        ))?;
        let books = stmt
            .query_map([hash_koreader], Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    pub fn find_all_by_library_id_and_with_empty_hash(
        &self,
        library_id: &str,
    ) -> Result<Vec<Book>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM BOOK WHERE LIBRARY_ID = ? AND FILE_HASH = ''"
        ))?;
        let books = stmt
            .query_map([library_id], Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    pub fn find_all_by_library_id_and_with_empty_hash_koreader(
        &self,
        library_id: &str,
    ) -> Result<Vec<Book>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM BOOK WHERE LIBRARY_ID = ? AND FILE_HASH_KOREADER = ''"
        ))?;
        let books = stmt
            .query_map([library_id], Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(books)
    }

    pub fn find_all_ids_by_series_id(&self, series_id: &str) -> Result<Vec<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT ID FROM BOOK WHERE SERIES_ID = ?")?;
        let ids = stmt
            .query_map([series_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(ids)
    }

    /// Series cover candidates (`SeriesLifecycle.getThumbnailBytes`); deleted books are not
    /// filtered out, matching the jOOQ queries.
    pub fn find_first_id_in_series_or_null(&self, series_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT BOOK.ID FROM BOOK LEFT JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID \
             WHERE BOOK.SERIES_ID = ? ORDER BY BOOK_METADATA.NUMBER_SORT ASC LIMIT 1",
        )?;
        let mut rows = stmt.query_map([series_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_last_id_in_series_or_null(&self, series_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT BOOK.ID FROM BOOK LEFT JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID \
             WHERE BOOK.SERIES_ID = ? ORDER BY BOOK_METADATA.NUMBER_SORT DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map([series_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_first_unread_id_in_series_or_null(
        &self,
        series_id: &str,
        user_id: &str,
    ) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT BOOK.ID FROM BOOK LEFT JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID \
             LEFT JOIN READ_PROGRESS ON BOOK.ID = READ_PROGRESS.BOOK_ID AND READ_PROGRESS.USER_ID = ? \
             WHERE BOOK.SERIES_ID = ? AND (READ_PROGRESS.COMPLETED IS NULL OR READ_PROGRESS.COMPLETED = 0) \
             ORDER BY BOOK_METADATA.NUMBER_SORT ASC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![user_id, series_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    /// Returns (id, created_date); generates a TSID when id is empty.
    pub fn insert(&self, book: &Book) -> Result<String> {
        let conn = self.db.rw();
        let id = if book.id.is_empty() {
            self.tsid.create_string()
        } else {
            book.id.clone()
        };
        conn.execute(
            &format!("INSERT INTO BOOK ({BOOK_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)"),
            rusqlite::params_from_iter(book_params(&id, book)),
        )?;
        Ok(id)
    }

    /// Same as komga's `BookDao.updateBook`: updates all fields except
    /// ID/CREATED_DATE, setting LAST_MODIFIED_DATE to the current time (UTC).
    pub fn update(&self, book: &Book) -> Result<()> {
        let conn = self.db.rw();
        let mut values = book_params(&book.id, book);
        values.truncate(values.len() - 2); // drop CREATED_DATE/LAST_MODIFIED_DATE
        values.remove(0); // drop ID (not in SET; bound separately in WHERE)
        values.push(Box::new(time_codec::format_datetime(time_codec::now_utc())));
        values.push(Box::new(book.id.clone()));
        let sets = BOOK_COLUMNS
            .split(',')
            .map(|c| c.trim())
            .filter(|c| !["ID", "CREATED_DATE", "LAST_MODIFIED_DATE"].contains(c))
            .map(|c| format!("{c} = ?"))
            .collect::<Vec<_>>()
            .join(", ");
        conn.execute(
            &format!("UPDATE BOOK SET {sets}, LAST_MODIFIED_DATE = ? WHERE ID = ?"),
            rusqlite::params_from_iter(values),
        )?;
        Ok(())
    }

    /// Deletes only the BOOK row; cascading of metadata/media/thumbnail is the
    /// responsibility of the upper-layer lifecycle (same as komga).
    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM BOOK WHERE ID = ?", [id])?;
        Ok(())
    }
}

fn book_params(id: &str, b: &Book) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(id.to_string()),
        Box::new(b.name.clone()),
        Box::new(b.url.clone()),
        Box::new(time_codec::format_datetime(b.file_last_modified)),
        Box::new(b.series_id.clone()),
        Box::new(b.library_id.clone()),
        Box::new(b.file_size),
        Box::new(b.number),
        Box::new(b.file_hash.clone()),
        Box::new(b.file_hash_koreader.clone()),
        Box::new(b.deleted_date.map(time_codec::format_datetime)),
        Box::new(b.oneshot),
        Box::new(time_codec::format_datetime(b.created_date)),
        Box::new(time_codec::format_datetime(b.last_modified_date)),
    ]
}

pub struct BookMetadataDao {
    db: Database,
}

impl BookMetadataDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn row_to_metadata(row: &Row<'_>) -> rusqlite::Result<BookMetadata> {
        Ok(BookMetadata {
            book_id: row.get(0)?,
            title: row.get(1)?,
            title_lock: row.get(2)?,
            summary: row.get(3)?,
            summary_lock: row.get(4)?,
            number: row.get(5)?,
            number_lock: row.get(6)?,
            number_sort: row.get(7)?,
            number_sort_lock: row.get(8)?,
            release_date: get_date(row, 9)?,
            release_date_lock: row.get(10)?,
            authors_lock: row.get(11)?,
            tags_lock: row.get(12)?,
            isbn: row.get(13)?,
            isbn_lock: row.get(14)?,
            links_lock: row.get(15)?,
            created_date: get_datetime(row, 16)?,
            last_modified_date: get_datetime(row, 17)?,
            authors: Vec::new(), // filled in by the caller
            tags: Vec::new(),
            links: Vec::new(),
        })
    }

    fn fill_children(&self, metadata: &mut BookMetadata) -> Result<()> {
        let conn = self.db.ro();
        metadata.authors = {
            let mut stmt =
                conn.prepare("SELECT NAME, ROLE FROM BOOK_METADATA_AUTHOR WHERE BOOK_ID = ?")?;
            let authors = stmt
                .query_map([&metadata.book_id], |r| {
                    Ok(Author {
                        name: r.get(0)?,
                        role: r.get(1)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            authors
        };
        metadata.tags = {
            let mut stmt = conn.prepare("SELECT TAG FROM BOOK_METADATA_TAG WHERE BOOK_ID = ?")?;
            let tags = stmt
                .query_map([&metadata.book_id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            tags
        };
        metadata.links = {
            let mut stmt =
                conn.prepare("SELECT LABEL, URL FROM BOOK_METADATA_LINK WHERE BOOK_ID = ?")?;
            let links = stmt
                .query_map([&metadata.book_id], |r| {
                    Ok(WebLink {
                        label: r.get(0)?,
                        url: r.get(1)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            links
        };
        Ok(())
    }

    pub fn find_by_id(&self, book_id: &str) -> Result<Option<BookMetadata>> {
        let mut metadata = {
            let conn = self.db.ro();
            let mut stmt = conn.prepare(&format!(
                "SELECT {METADATA_COLUMNS} FROM BOOK_METADATA WHERE BOOK_ID = ?"
            ))?;
            let mut rows = stmt.query_map([book_id], Self::row_to_metadata)?;
            rows.next().transpose()?
        };
        if let Some(m) = metadata.as_mut() {
            self.fill_children(m)?;
        }
        Ok(metadata)
    }

    pub fn insert(&self, metadata: &BookMetadata) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      &format!("INSERT INTO BOOK_METADATA ({METADATA_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"),
      rusqlite::params_from_iter(metadata_params(metadata)),
    )?;
        self.insert_children(&conn, metadata)?;
        Ok(())
    }

    /// Same as komga's `BookMetadataDao.update`: updates the main table and sets
    /// LAST_MODIFIED_DATE to the current time; child tables are deleted and
    /// re-inserted.
    pub fn update(&self, metadata: &BookMetadata) -> Result<()> {
        let conn = self.db.rw();
        let mut values = metadata_params(metadata);
        values.truncate(values.len() - 2); // drop CREATED_DATE/LAST_MODIFIED_DATE
        values.remove(0); // drop BOOK_ID (not in SET; bound separately in WHERE)
        values.push(Box::new(time_codec::format_datetime(time_codec::now_utc())));
        values.push(Box::new(metadata.book_id.clone()));
        let sets = METADATA_COLUMNS
            .split(',')
            .map(|c| c.trim())
            .filter(|c| !["BOOK_ID", "CREATED_DATE", "LAST_MODIFIED_DATE"].contains(c))
            .map(|c| format!("{c} = ?"))
            .collect::<Vec<_>>()
            .join(", ");
        conn.execute(
            &format!("UPDATE BOOK_METADATA SET {sets}, LAST_MODIFIED_DATE = ? WHERE BOOK_ID = ?"),
            rusqlite::params_from_iter(values),
        )?;
        self.delete_children(&conn, &metadata.book_id)?;
        self.insert_children(&conn, metadata)?;
        Ok(())
    }

    pub fn delete(&self, book_id: &str) -> Result<()> {
        let conn = self.db.rw();
        self.delete_children(&conn, book_id)?;
        conn.execute("DELETE FROM BOOK_METADATA WHERE BOOK_ID = ?", [book_id])?;
        Ok(())
    }

    fn delete_children(&self, conn: &rusqlite::Connection, book_id: &str) -> Result<()> {
        conn.execute(
            "DELETE FROM BOOK_METADATA_AUTHOR WHERE BOOK_ID = ?",
            [book_id],
        )?;
        conn.execute("DELETE FROM BOOK_METADATA_TAG WHERE BOOK_ID = ?", [book_id])?;
        conn.execute(
            "DELETE FROM BOOK_METADATA_LINK WHERE BOOK_ID = ?",
            [book_id],
        )?;
        Ok(())
    }

    fn insert_children(&self, conn: &rusqlite::Connection, metadata: &BookMetadata) -> Result<()> {
        for author in &metadata.authors {
            conn.execute(
                "INSERT INTO BOOK_METADATA_AUTHOR (NAME, ROLE, BOOK_ID) VALUES (?, ?, ?)",
                params![author.name, author.role, metadata.book_id],
            )?;
        }
        for tag in &metadata.tags {
            conn.execute(
                "INSERT INTO BOOK_METADATA_TAG (TAG, BOOK_ID) VALUES (?, ?)",
                params![tag, metadata.book_id],
            )?;
        }
        for link in &metadata.links {
            conn.execute(
                "INSERT INTO BOOK_METADATA_LINK (LABEL, URL, BOOK_ID) VALUES (?, ?, ?)",
                params![link.label, link.url, metadata.book_id],
            )?;
        }
        Ok(())
    }
}

fn metadata_params(m: &BookMetadata) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(m.book_id.clone()),
        Box::new(m.title.clone()),
        Box::new(m.title_lock),
        Box::new(m.summary.clone()),
        Box::new(m.summary_lock),
        Box::new(m.number.clone()),
        Box::new(m.number_lock),
        Box::new(m.number_sort),
        Box::new(m.number_sort_lock),
        Box::new(m.release_date.map(komga_core::time_codec::format_date)),
        Box::new(m.release_date_lock),
        Box::new(m.authors_lock),
        Box::new(m.tags_lock),
        Box::new(m.isbn.clone()),
        Box::new(m.isbn_lock),
        Box::new(m.links_lock),
        Box::new(time_codec::format_datetime(m.created_date)),
        Box::new(time_codec::format_datetime(m.last_modified_date)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::library::LibraryDao;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::model::library::Library;
    use komga_core::time_codec::now_utc;

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    /// FK requires LIBRARY + SERIES rows; the SERIES DAO belongs to another group,
    /// so tests use raw SQL here.
    fn seed_library_series(db: &Database) -> (String, String) {
        let library_dao = LibraryDao::new(db.clone());
        let now = now_utc();
        let library = Library {
            id: String::new(),
            name: "L".into(),
            root: "file:/l/".into(),
            import_comicinfo_book: true,
            import_comicinfo_series: true,
            import_comicinfo_collection: true,
            import_comicinfo_readlist: true,
            import_comicinfo_series_append_volume: true,
            import_epub_book: true,
            import_epub_series: true,
            import_mylar_series: true,
            import_local_artwork: true,
            import_barcode_isbn: true,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: komga_core::model::ScanInterval::Every6H,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: komga_core::model::SeriesCover::First,
            hash_files: true,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now,
            last_modified_date: now,
        };
        let library_id = library_dao.insert(&library).unwrap();
        let series_id = "SERIES1";
        db.rw()
      .execute(
        "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES (?, ?, ?, ?, ?)",
        params![series_id, "S", "file:/l/s/", time_codec::format_datetime(now), library_id],
      )
      .unwrap();
        (library_id, series_id.to_string())
    }

    fn sample_book(library_id: &str, series_id: &str) -> Book {
        let now = now_utc();
        Book {
            id: String::new(),
            name: "book01.cbz".into(),
            url: "file:/l/s/book01.cbz".into(),
            file_last_modified: now,
            series_id: series_id.into(),
            library_id: library_id.into(),
            file_size: 12345,
            number: 0,
            file_hash: "abc123".into(),
            file_hash_koreader: String::new(),
            deleted_date: None,
            oneshot: false,
            created_date: now,
            last_modified_date: now,
        }
    }

    #[test]
    fn book_crud_roundtrip() {
        let db = db();
        let (library_id, series_id) = seed_library_series(&db);
        let dao = BookDao::new(db);

        let book = sample_book(&library_id, &series_id);
        let id = dao.insert(&book).unwrap();
        assert_eq!(id.len(), 13);

        let found = dao.find_by_id(&id).unwrap().expect("not found");
        assert_eq!(found.name, "book01.cbz");
        assert_eq!(found.file_size, 12345);
        assert_eq!(found.file_hash, "abc123");
        assert_eq!(found.file_hash_koreader, "");
        assert!(!found.deleted());
        assert!(!found.oneshot);

        let mut updated = found.clone();
        updated.name = "book02.cbz".into();
        updated.file_hash_koreader = "korhash".into();
        updated.deleted_date = Some(now_utc());
        updated.oneshot = true;
        dao.update(&updated).unwrap();

        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.name, "book02.cbz");
        assert_eq!(found.file_hash_koreader, "korhash");
        assert!(found.deleted());
        assert!(found.oneshot);

        assert_eq!(dao.find_by_series_id(&series_id).unwrap().len(), 1);
        assert_eq!(dao.find_all().unwrap().len(), 1);

        dao.delete(&id).unwrap();
        assert!(dao.find_by_id(&id).unwrap().is_none());
    }

    fn sample_metadata(book_id: &str) -> BookMetadata {
        let now = now_utc();
        BookMetadata {
            book_id: book_id.into(),
            title: "Chapter 1".into(),
            summary: "summary".into(),
            number: "1".into(),
            number_sort: 1.0,
            release_date: Some(
                time::Date::from_calendar_date(2020, time::Month::January, 2).unwrap(),
            ),
            authors: vec![
                Author {
                    name: "Author A".into(),
                    role: "writer".into(),
                },
                Author {
                    name: "Author B".into(),
                    role: "penciller".into(),
                },
            ],
            tags: vec!["action".into(), "fantasy".into()],
            isbn: "9781234567890".into(),
            links: vec![WebLink {
                label: "homepage".into(),
                url: "https://example.org".into(),
            }],
            title_lock: false,
            summary_lock: false,
            number_lock: false,
            number_sort_lock: false,
            release_date_lock: false,
            authors_lock: false,
            tags_lock: false,
            isbn_lock: false,
            links_lock: false,
            created_date: now,
            last_modified_date: now,
        }
    }

    #[test]
    fn metadata_crud_with_children() {
        let db = db();
        let (library_id, series_id) = seed_library_series(&db);
        let book_dao = BookDao::new(db.clone());
        let book_id = book_dao
            .insert(&sample_book(&library_id, &series_id))
            .unwrap();
        let dao = BookMetadataDao::new(db);

        let metadata = sample_metadata(&book_id);
        dao.insert(&metadata).unwrap();

        let found = dao.find_by_id(&book_id).unwrap().expect("not found");
        assert_eq!(found.title, "Chapter 1");
        assert_eq!(found.number, "1");
        assert_eq!(found.number_sort, 1.0);
        assert_eq!(
            found.release_date,
            Some(time::Date::from_calendar_date(2020, time::Month::January, 2).unwrap())
        );
        assert_eq!(found.authors.len(), 2);
        assert_eq!(found.authors[0].name, "Author A");
        assert_eq!(found.authors[1].role, "penciller");
        assert_eq!(found.tags, vec!["action", "fantasy"]);
        assert_eq!(found.isbn, "9781234567890");
        assert_eq!(found.links[0].url, "https://example.org");

        let mut updated = found.clone();
        updated.title = "Chapter 1.5".into();
        updated.title_lock = true;
        updated.release_date = None;
        updated.authors = vec![Author {
            name: "Author C".into(),
            role: "editor".into(),
        }];
        updated.tags = vec![];
        dao.update(&updated).unwrap();

        let found = dao.find_by_id(&book_id).unwrap().unwrap();
        assert_eq!(found.title, "Chapter 1.5");
        assert!(found.title_lock);
        assert_eq!(found.release_date, None);
        assert_eq!(found.authors.len(), 1);
        assert_eq!(found.authors[0].name, "Author C");
        assert!(found.tags.is_empty());

        dao.delete(&book_id).unwrap();
        assert!(dao.find_by_id(&book_id).unwrap().is_none());
        let children: i64 = dao
      .db
      .ro()
      .query_row(
        "SELECT (SELECT COUNT(*) FROM BOOK_METADATA_AUTHOR) + (SELECT COUNT(*) FROM BOOK_METADATA_TAG) + (SELECT COUNT(*) FROM BOOK_METADATA_LINK)",
        [],
        |r| r.get(0),
      )
      .unwrap();
        assert_eq!(children, 0);
    }
}
