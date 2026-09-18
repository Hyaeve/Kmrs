//! `ReadProgressDtoDao.kt`: Tachiyomi/Mihon read-progress DTO queries.

use crate::error::Result;
use crate::pool::Database;
use komga_core::dto::tachiyomi::{TachiyomiReadProgressDto, TachiyomiReadProgressV2Dto};

/// `READ_PROGRESS.USER_ID = ? OR READ_PROGRESS.USER_ID IS NULL` (the LEFT JOIN condition)
const READ_PROGRESS_CONDITION: &str =
    "(READ_PROGRESS.USER_ID = ? OR READ_PROGRESS.USER_ID IS NULL)";

const COUNT_UNREAD: &str = "SUM(CASE WHEN READ_PROGRESS.COMPLETED IS NULL THEN 1 ELSE 0 END)";
const COUNT_READ: &str = "SUM(CASE WHEN READ_PROGRESS.COMPLETED = 1 THEN 1 ELSE 0 END)";
const COUNT_IN_PROGRESS: &str = "SUM(CASE WHEN READ_PROGRESS.COMPLETED = 0 THEN 1 ELSE 0 END)";

pub struct ReadProgressDtoDao {
    db: Database,
}

impl ReadProgressDtoDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn find_progress_v2_by_series(
        &self,
        series_id: &str,
        user_id: &str,
    ) -> Result<TachiyomiReadProgressV2Dto> {
        let conn = self.db.ro();

        let mut stmt = conn.prepare(&format!(
            "SELECT BOOK_METADATA.NUMBER_SORT, READ_PROGRESS.COMPLETED \
             FROM BOOK \
             LEFT JOIN READ_PROGRESS ON (BOOK.ID = READ_PROGRESS.BOOK_ID AND {READ_PROGRESS_CONDITION}) \
             LEFT JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID \
             WHERE BOOK.SERIES_ID = ? \
             ORDER BY BOOK_METADATA.NUMBER_SORT"
        ))?;
        let progress = stmt
            .query_map(rusqlite::params![user_id, series_id], |row| {
                Ok((row.get::<_, f32>(0)?, row.get::<_, Option<bool>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let max_number_sort: Option<f32> = conn.query_row(
            "SELECT MAX(BOOK_METADATA.NUMBER_SORT) \
             FROM BOOK LEFT JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID \
             WHERE BOOK.SERIES_ID = ?",
            [series_id],
            |row| row.get(0),
        )?;

        let books_count = books_count(
            &conn,
            &format!(
                "FROM BOOK \
                 LEFT JOIN READ_PROGRESS ON (BOOK.ID = READ_PROGRESS.BOOK_ID AND {READ_PROGRESS_CONDITION}) \
                 WHERE BOOK.SERIES_ID = ?"
            ),
            &[rusqlite::types::Value::Text(user_id.to_string()), rusqlite::types::Value::Text(series_id.to_string())],
        )?;

        Ok(TachiyomiReadProgressV2Dto {
            books_count: books_count.total(),
            books_read_count: books_count.read,
            books_unread_count: books_count.unread,
            books_in_progress_count: books_count.in_progress,
            last_read_continuous_number_sort: last_read(progress).unwrap_or(0.0),
            max_number_sort: max_number_sort.unwrap_or(0.0),
        })
    }

    pub fn find_progress_by_readlist(
        &self,
        readlist_id: &str,
        user_id: &str,
    ) -> Result<TachiyomiReadProgressDto> {
        let conn = self.db.ro();

        let mut stmt = conn.prepare(&format!(
            "SELECT ROW_NUMBER() OVER (ORDER BY READLIST_BOOK.NUMBER), READ_PROGRESS.COMPLETED \
             FROM BOOK \
             LEFT JOIN READ_PROGRESS ON (BOOK.ID = READ_PROGRESS.BOOK_ID AND {READ_PROGRESS_CONDITION}) \
             LEFT JOIN READLIST_BOOK ON BOOK.ID = READLIST_BOOK.BOOK_ID \
             WHERE READLIST_BOOK.READLIST_ID = ? \
             ORDER BY READLIST_BOOK.NUMBER"
        ))?;
        let progress = stmt
            .query_map(rusqlite::params![user_id, readlist_id], |row| {
                Ok((row.get::<_, i32>(0)?, row.get::<_, Option<bool>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let books_count = books_count(
            &conn,
            &format!(
                "FROM BOOK \
                 LEFT JOIN READ_PROGRESS ON (BOOK.ID = READ_PROGRESS.BOOK_ID AND {READ_PROGRESS_CONDITION}) \
                 LEFT JOIN READLIST_BOOK ON BOOK.ID = READLIST_BOOK.BOOK_ID \
                 WHERE READLIST_BOOK.READLIST_ID = ?"
            ),
            &[rusqlite::types::Value::Text(user_id.to_string()), rusqlite::types::Value::Text(readlist_id.to_string())],
        )?;

        Ok(TachiyomiReadProgressDto {
            books_count: books_count.total(),
            books_read_count: books_count.read,
            books_unread_count: books_count.unread,
            books_in_progress_count: books_count.in_progress,
            last_read_continuous_index: last_read(progress).unwrap_or(0),
        })
    }
}

struct BooksCount {
    unread: i32,
    read: i32,
    in_progress: i32,
}

impl BooksCount {
    fn total(&self) -> i32 {
        self.unread + self.read + self.in_progress
    }
}

/// The three SUM()s return NULL when the series/readlist has no books; komga's data always has
/// at least one book per series/readlist, so the COALESCE only guards degenerate databases.
fn books_count(
    conn: &rusqlite::Connection,
    from_where: &str,
    params: &[rusqlite::types::Value],
) -> Result<BooksCount> {
    let (unread, read, in_progress) = conn.query_row(
        &format!(
            "SELECT COALESCE({COUNT_UNREAD}, 0), COALESCE({COUNT_READ}, 0), COALESCE({COUNT_IN_PROGRESS}, 0) {from_where}"
        ),
        rusqlite::params_from_iter(params),
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    Ok(BooksCount {
        unread,
        read,
        in_progress,
    })
}

/// `lastRead()`: the value of the last entry of the leading run of completed books.
fn last_read<T>(progress: Vec<(T, Option<bool>)>) -> Option<T> {
    progress
        .into_iter()
        .take_while(|(_, completed)| *completed == Some(true))
        .last()
        .map(|(value, _)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = crate::main_migrations();
        crate::Migrator::new(&migrations, crate::Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    fn exec(db: &Database, sql: &str, params: impl rusqlite::Params) {
        db.rw().execute(sql, params).unwrap();
    }

    /// Series s1 with 4 books (number_sort 1..4); readlist r1 with the same 4 books in order.
    /// User u1 has read b1, b2; b3 is in progress; b4 unread.
    fn seed(db: &Database) {
        exec(
            db,
            "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES ('l1', 'lib', 'file:/data/')",
            [],
        );
        exec(db, "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES ('s1', 's1', 'file:/data/x/', '2024-01-01 00:00:00.0', 'l1')", []);
        exec(
            db,
            "INSERT INTO USER (ID, EMAIL, PASSWORD) VALUES ('u1', 'u@x.y', 'x')",
            [],
        );
        for (id, number_sort) in [("b1", 1.0), ("b2", 2.0), ("b3", 3.0), ("b4", 4.0)] {
            exec(
                db,
                "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) VALUES (?, ?, 'file:/data/x.cbz', '2024-01-01 00:00:00.0', 's1', 'l1')",
                rusqlite::params![id, id],
            );
            exec(
                db,
                "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, NUMBER, NUMBER_SORT) VALUES (?, ?, '', ?)",
                rusqlite::params![id, id, number_sort],
            );
        }
        exec(db, "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) VALUES ('b1', 'u1', 10, 1, '2024-01-01 00:00:00.0')", []);
        exec(db, "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) VALUES ('b2', 'u1', 10, 1, '2024-01-01 00:00:00.0')", []);
        exec(db, "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) VALUES ('b3', 'u1', 5, 0, '2024-01-01 00:00:00.0')", []);

        exec(
            db,
            "INSERT INTO READLIST (ID, NAME, BOOK_COUNT) VALUES ('r1', 'r1', 4)",
            [],
        );
        for (i, id) in ["b1", "b2", "b3", "b4"].iter().enumerate() {
            exec(
                db,
                "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES ('r1', ?, ?)",
                rusqlite::params![id, i as i32],
            );
        }
    }

    #[test]
    fn v2_by_series() {
        let db = db();
        seed(&db);
        let dao = ReadProgressDtoDao::new(db.clone());

        let dto = dao.find_progress_v2_by_series("s1", "u1").unwrap();
        assert_eq!(dto.books_count, 4);
        assert_eq!(dto.books_read_count, 2);
        assert_eq!(dto.books_in_progress_count, 1);
        assert_eq!(dto.books_unread_count, 1);
        // b1 and b2 form the leading completed run: last is number_sort 2
        assert_eq!(dto.last_read_continuous_number_sort, 2.0);
        assert_eq!(dto.max_number_sort, 4.0);
    }

    #[test]
    fn v2_by_series_stops_at_first_incomplete() {
        let db = db();
        seed(&db);
        // b1 unread, b2 completed: the leading run is empty
        exec(&db, "DELETE FROM READ_PROGRESS WHERE BOOK_ID = 'b1'", []);
        let dao = ReadProgressDtoDao::new(db.clone());

        let dto = dao.find_progress_v2_by_series("s1", "u1").unwrap();
        assert_eq!(dto.last_read_continuous_number_sort, 0.0);
        assert_eq!(dto.books_read_count, 1);
        assert_eq!(dto.books_unread_count, 2);
    }

    #[test]
    fn v2_by_series_is_per_user() {
        let db = db();
        seed(&db);
        let dao = ReadProgressDtoDao::new(db.clone());

        // another user has no progress: all 4 unread
        let dto = dao.find_progress_v2_by_series("s1", "nobody").unwrap();
        assert_eq!(dto.books_count, 4);
        assert_eq!(dto.books_unread_count, 4);
        assert_eq!(dto.books_read_count, 0);
        assert_eq!(dto.last_read_continuous_number_sort, 0.0);
    }

    #[test]
    fn by_readlist() {
        let db = db();
        seed(&db);
        let dao = ReadProgressDtoDao::new(db.clone());

        let dto = dao.find_progress_by_readlist("r1", "u1").unwrap();
        assert_eq!(dto.books_count, 4);
        assert_eq!(dto.books_read_count, 2);
        assert_eq!(dto.books_in_progress_count, 1);
        assert_eq!(dto.books_unread_count, 1);
        // positions are 1-based row numbers over READLIST_BOOK.NUMBER
        assert_eq!(dto.last_read_continuous_index, 2);
    }

    #[test]
    fn by_readlist_order_uses_readlist_number() {
        let db = db();
        seed(&db);
        // reverse the readlist order: b4(0), b3(1), b2(2), b1(3)
        exec(&db, "UPDATE READLIST_BOOK SET NUMBER = CASE BOOK_ID WHEN 'b1' THEN 3 WHEN 'b2' THEN 2 WHEN 'b3' THEN 1 ELSE 0 END", []);
        let dao = ReadProgressDtoDao::new(db.clone());

        // b4 is first and unread: the leading run is empty even though b1/b2 are read
        let dto = dao.find_progress_by_readlist("r1", "u1").unwrap();
        assert_eq!(dto.last_read_continuous_index, 0);

        // complete b4: the run stops at b3 (in progress)
        exec(&db, "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) VALUES ('b4', 'u1', 10, 1, '2024-01-01 00:00:00.0')", []);
        let dto = dao.find_progress_by_readlist("r1", "u1").unwrap();
        assert_eq!(dto.last_read_continuous_index, 1);

        // complete b3 as well: b4, b3, b2, b1 are all read in the new order
        exec(
            &db,
            "UPDATE READ_PROGRESS SET COMPLETED = 1 WHERE BOOK_ID = 'b3'",
            [],
        );
        let dto = dao.find_progress_by_readlist("r1", "u1").unwrap();
        assert_eq!(dto.last_read_continuous_index, 4);
    }
}
