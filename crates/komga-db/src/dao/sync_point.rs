//! DAO for SYNC_POINT and its 5 child tables (the CRUD part of `SyncPointDao`;
//! sync-diff queries belong to the Kobo API phase and are not implemented here).

use super::{get_datetime, get_datetime_opt};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::sync_point::{
    SyncPoint, SyncPointBook, SyncPointReadList, SyncPointReadListBook,
};
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};

const SP_COLUMNS: &str = "ID, USER_ID, API_KEY_ID, CREATED_DATE";

const SPB_COLUMNS: &str = "SYNC_POINT_ID, BOOK_ID, BOOK_CREATED_DATE, BOOK_LAST_MODIFIED_DATE, \
 BOOK_FILE_LAST_MODIFIED, BOOK_FILE_SIZE, BOOK_FILE_HASH, BOOK_METADATA_LAST_MODIFIED_DATE, \
 BOOK_READ_PROGRESS_LAST_MODIFIED_DATE, BOOK_THUMBNAIL_ID, SYNCED";

const SPRL_COLUMNS: &str =
  "SYNC_POINT_ID, READLIST_ID, READLIST_NAME, READLIST_CREATED_DATE, READLIST_LAST_MODIFIED_DATE, SYNCED";

pub struct SyncPointDao {
    db: Database,
    tsid: TsidFactory,
}

impl SyncPointDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    // ---------- SYNC_POINT ----------

    fn row_to_sync_point(row: &Row<'_>) -> rusqlite::Result<SyncPoint> {
        Ok(SyncPoint {
            id: row.get(0)?,
            user_id: row.get(1)?,
            api_key_id: row.get(2)?,
            created_date: get_datetime(row, 3)?,
        })
    }

    pub fn insert(&self, sync_point: &SyncPoint) -> Result<String> {
        let conn = self.db.rw();
        let id = if sync_point.id.is_empty() {
            self.tsid.create_string()
        } else {
            sync_point.id.clone()
        };
        conn.execute(
            &format!("INSERT INTO SYNC_POINT ({SP_COLUMNS}) VALUES (?,?,?,?)"),
            params![
                id,
                sync_point.user_id,
                sync_point.api_key_id,
                time_codec::format_datetime(sync_point.created_date),
            ],
        )?;
        Ok(id)
    }

    pub fn find_by_id(&self, sync_point_id: &str) -> Result<Option<SyncPoint>> {
        let conn = self.db.ro();
        let mut stmt =
            conn.prepare(&format!("SELECT {SP_COLUMNS} FROM SYNC_POINT WHERE ID = ?"))?;
        let rows = stmt
            .query_map([sync_point_id], Self::row_to_sync_point)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().next())
    }

    pub fn find_by_user_id(&self, user_id: &str) -> Result<Vec<SyncPoint>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SP_COLUMNS} FROM SYNC_POINT WHERE USER_ID = ? ORDER BY CREATED_DATE DESC"
        ))?;
        let rows = stmt
            .query_map([user_id], Self::row_to_sync_point)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---------- SYNC_POINT_BOOK ----------

    fn row_to_book(row: &Row<'_>) -> rusqlite::Result<SyncPointBook> {
        Ok(SyncPointBook {
            sync_point_id: row.get(0)?,
            book_id: row.get(1)?,
            book_created_date: get_datetime(row, 2)?,
            book_last_modified_date: get_datetime(row, 3)?,
            book_file_last_modified: get_datetime(row, 4)?,
            book_file_size: row.get(5)?,
            book_file_hash: row.get(6)?,
            book_metadata_last_modified_date: get_datetime(row, 7)?,
            book_read_progress_last_modified_date: get_datetime_opt(row, 8)?,
            book_thumbnail_id: row.get(9)?,
            synced: row.get(10)?,
        })
    }

    pub fn insert_books(&self, books: &[SyncPointBook]) -> Result<()> {
        let conn = self.db.rw();
        for b in books {
            conn.execute(
                &format!(
                    "INSERT INTO SYNC_POINT_BOOK ({SPB_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?)"
                ),
                params![
                    b.sync_point_id,
                    b.book_id,
                    time_codec::format_datetime(b.book_created_date),
                    time_codec::format_datetime(b.book_last_modified_date),
                    time_codec::format_datetime(b.book_file_last_modified),
                    b.book_file_size,
                    b.book_file_hash,
                    time_codec::format_datetime(b.book_metadata_last_modified_date),
                    b.book_read_progress_last_modified_date
                        .map(time_codec::format_datetime),
                    b.book_thumbnail_id,
                    b.synced,
                ],
            )?;
        }
        Ok(())
    }

    pub fn find_books(
        &self,
        sync_point_id: &str,
        only_not_synced: bool,
    ) -> Result<Vec<SyncPointBook>> {
        let conn = self.db.ro();
        let sql = format!(
            "SELECT {SPB_COLUMNS} FROM SYNC_POINT_BOOK WHERE SYNC_POINT_ID = ?{}",
            if only_not_synced {
                " AND SYNCED = 0"
            } else {
                ""
            }
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([sync_point_id], Self::row_to_book)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn mark_books_synced(&self, sync_point_id: &str, book_ids: &[String]) -> Result<()> {
        if book_ids.is_empty() {
            return Ok(());
        }
        let conn = self.db.rw();
        let placeholders = book_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(sync_point_id.to_string())];
        for id in book_ids {
            values.push(Box::new(id.clone()));
        }
        conn.execute(
      &format!("UPDATE SYNC_POINT_BOOK SET SYNCED = 1 WHERE SYNC_POINT_ID = ? AND BOOK_ID IN ({placeholders})"),
      rusqlite::params_from_iter(values),
    )?;
        Ok(())
    }

    /// Deleted books are not in the target sync point; their sync state lives in a
    /// separate table (`onDuplicateKeyIgnore`).
    pub fn mark_books_removed_synced(
        &self,
        sync_point_id: &str,
        book_ids: &[String],
    ) -> Result<()> {
        let conn = self.db.rw();
        for id in book_ids {
            conn.execute(
        "INSERT OR IGNORE INTO SYNC_POINT_BOOK_REMOVED_SYNCED (SYNC_POINT_ID, BOOK_ID) VALUES (?, ?)",
        params![sync_point_id, id],
      )?;
        }
        Ok(())
    }

    pub fn find_books_removed_synced(&self, sync_point_id: &str) -> Result<Vec<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT BOOK_ID FROM SYNC_POINT_BOOK_REMOVED_SYNCED WHERE SYNC_POINT_ID = ?",
        )?;
        let rows = stmt
            .query_map([sync_point_id], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---------- SYNC_POINT_READLIST ----------

    fn row_to_readlist(row: &Row<'_>) -> rusqlite::Result<SyncPointReadList> {
        Ok(SyncPointReadList {
            sync_point_id: row.get(0)?,
            readlist_id: row.get(1)?,
            readlist_name: row.get(2)?,
            readlist_created_date: get_datetime(row, 3)?,
            readlist_last_modified_date: get_datetime(row, 4)?,
            synced: row.get(5)?,
        })
    }

    pub fn insert_readlist(&self, readlist: &SyncPointReadList) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            &format!("INSERT INTO SYNC_POINT_READLIST ({SPRL_COLUMNS}) VALUES (?,?,?,?,?,?)"),
            params![
                readlist.sync_point_id,
                readlist.readlist_id,
                readlist.readlist_name,
                time_codec::format_datetime(readlist.readlist_created_date),
                time_codec::format_datetime(readlist.readlist_last_modified_date),
                readlist.synced,
            ],
        )?;
        Ok(())
    }

    pub fn find_readlists(
        &self,
        sync_point_id: &str,
        only_not_synced: bool,
    ) -> Result<Vec<SyncPointReadList>> {
        let conn = self.db.ro();
        let sql = format!(
            "SELECT {SPRL_COLUMNS} FROM SYNC_POINT_READLIST WHERE SYNC_POINT_ID = ?{}",
            if only_not_synced {
                " AND SYNCED = 0"
            } else {
                ""
            }
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map([sync_point_id], Self::row_to_readlist)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn mark_readlists_synced(
        &self,
        sync_point_id: &str,
        readlist_ids: &[String],
    ) -> Result<()> {
        if readlist_ids.is_empty() {
            return Ok(());
        }
        let conn = self.db.rw();
        let placeholders = readlist_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(sync_point_id.to_string())];
        for id in readlist_ids {
            values.push(Box::new(id.clone()));
        }
        conn.execute(
      &format!("UPDATE SYNC_POINT_READLIST SET SYNCED = 1 WHERE SYNC_POINT_ID = ? AND READLIST_ID IN ({placeholders})"),
      rusqlite::params_from_iter(values),
    )?;
        Ok(())
    }

    pub fn mark_readlists_removed_synced(
        &self,
        sync_point_id: &str,
        readlist_ids: &[String],
    ) -> Result<()> {
        let conn = self.db.rw();
        for id in readlist_ids {
            conn.execute(
        "INSERT OR IGNORE INTO SYNC_POINT_READLIST_REMOVED_SYNCED (SYNC_POINT_ID, READLIST_ID) VALUES (?, ?)",
        params![sync_point_id, id],
      )?;
        }
        Ok(())
    }

    // ---------- SYNC_POINT_READLIST_BOOK ----------

    pub fn insert_readlist_books(&self, books: &[SyncPointReadListBook]) -> Result<()> {
        let conn = self.db.rw();
        for b in books {
            conn.execute(
        "INSERT INTO SYNC_POINT_READLIST_BOOK (SYNC_POINT_ID, READLIST_ID, BOOK_ID) VALUES (?, ?, ?)",
        params![b.sync_point_id, b.readlist_id, b.book_id],
      )?;
        }
        Ok(())
    }

    pub fn find_book_ids_by_readlist_ids(
        &self,
        sync_point_id: &str,
        readlist_ids: &[String],
    ) -> Result<Vec<SyncPointReadListBook>> {
        if readlist_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.ro();
        let placeholders = readlist_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(sync_point_id.to_string())];
        for id in readlist_ids {
            values.push(Box::new(id.clone()));
        }
        let mut stmt = conn.prepare(&format!(
            "SELECT SYNC_POINT_ID, READLIST_ID, BOOK_ID FROM SYNC_POINT_READLIST_BOOK \
       WHERE SYNC_POINT_ID = ? AND READLIST_ID IN ({placeholders})"
        ))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok(SyncPointReadListBook {
                    sync_point_id: r.get(0)?,
                    readlist_id: r.get(1)?,
                    book_id: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---------- deletion (child-table order matches Java) ----------

    pub fn delete_one(&self, sync_point_id: &str) -> Result<()> {
        let conn = self.db.rw();
        for table in [
            "SYNC_POINT_READLIST_REMOVED_SYNCED",
            "SYNC_POINT_READLIST_BOOK",
            "SYNC_POINT_READLIST",
            "SYNC_POINT_BOOK_REMOVED_SYNCED",
            "SYNC_POINT_BOOK",
        ] {
            conn.execute(
                &format!("DELETE FROM {table} WHERE SYNC_POINT_ID = ?"),
                [sync_point_id],
            )?;
        }
        conn.execute("DELETE FROM SYNC_POINT WHERE ID = ?", [sync_point_id])?;
        Ok(())
    }

    pub fn delete_by_user_id(&self, user_id: &str) -> Result<()> {
        let ids: Vec<String> = {
            let conn = self.db.ro();
            let mut stmt = conn.prepare("SELECT ID FROM SYNC_POINT WHERE USER_ID = ?")?;
            let ids = stmt
                .query_map([user_id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids
        };
        for id in ids {
            self.delete_one(&id)?;
        }
        Ok(())
    }

    pub fn delete_by_user_id_and_api_key_ids(
        &self,
        user_id: &str,
        api_key_ids: &[String],
    ) -> Result<()> {
        if api_key_ids.is_empty() {
            return Ok(());
        }
        let placeholders = api_key_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let ids: Vec<String> = {
            let conn = self.db.ro();
            let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(user_id.to_string())];
            for id in api_key_ids {
                values.push(Box::new(id.clone()));
            }
            let mut stmt = conn.prepare(&format!(
                "SELECT ID FROM SYNC_POINT WHERE USER_ID = ? AND API_KEY_ID IN ({placeholders})"
            ))?;
            let ids = stmt
                .query_map(rusqlite::params_from_iter(values), |r| {
                    r.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids
        };
        for id in ids {
            self.delete_one(&id)?;
        }
        Ok(())
    }

    pub fn delete_all(&self) -> Result<()> {
        let conn = self.db.rw();
        for table in [
            "SYNC_POINT_READLIST_REMOVED_SYNCED",
            "SYNC_POINT_READLIST_BOOK",
            "SYNC_POINT_READLIST",
            "SYNC_POINT_BOOK_REMOVED_SYNCED",
            "SYNC_POINT_BOOK",
            "SYNC_POINT",
        ] {
            conn.execute(&format!("DELETE FROM {table}"), [])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::time_codec::now_utc;

    fn dao() -> SyncPointDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        SyncPointDao::new(db)
    }

    /// SYNC_POINT has an FK to USER, so create the user before the test
    fn create_user(dao: &SyncPointDao, id: &str) {
        crate::dao::user::UserDao::new(dao.db.clone())
            .insert(&komga_core::model::user::KomgaUser {
                id: id.into(),
                email: format!("{id}@example.org"),
                password: "x".into(),
                roles: Default::default(),
                shared_libraries_ids: Default::default(),
                shared_all_libraries: true,
                restrictions: Default::default(),
                created_date: now_utc(),
                last_modified_date: now_utc(),
            })
            .unwrap();
    }

    fn sample_sync_point(user_id: &str) -> SyncPoint {
        SyncPoint {
            id: String::new(),
            user_id: user_id.into(),
            api_key_id: Some("key1".into()),
            created_date: now_utc(),
        }
    }

    fn sample_book(sync_point_id: &str, book_id: &str) -> SyncPointBook {
        SyncPointBook {
            sync_point_id: sync_point_id.into(),
            book_id: book_id.into(),
            book_created_date: now_utc(),
            book_last_modified_date: now_utc(),
            book_file_last_modified: now_utc(),
            book_file_size: 12345,
            book_file_hash: "abc123".into(),
            book_metadata_last_modified_date: now_utc(),
            book_read_progress_last_modified_date: None,
            book_thumbnail_id: None,
            synced: false,
        }
    }

    #[test]
    fn sync_point_lifecycle() {
        let dao = dao();
        create_user(&dao, "user1");
        let sp_id = dao.insert(&sample_sync_point("user1")).unwrap();
        assert_eq!(sp_id.len(), 13);

        let found = dao.find_by_id(&sp_id).unwrap().expect("not found");
        assert_eq!(found.user_id, "user1");
        assert_eq!(found.api_key_id.as_deref(), Some("key1"));
        assert_eq!(dao.find_by_user_id("user1").unwrap().len(), 1);

        dao.insert_books(&[sample_book(&sp_id, "b1"), sample_book(&sp_id, "b2")])
            .unwrap();
        let books = dao.find_books(&sp_id, false).unwrap();
        assert_eq!(books.len(), 2);
        assert_eq!(books[0].book_file_size, 12345);
        assert!(books[0].book_read_progress_last_modified_date.is_none());
        assert_eq!(dao.find_books(&sp_id, true).unwrap().len(), 2);

        dao.mark_books_synced(&sp_id, &["b1".to_string()]).unwrap();
        assert_eq!(dao.find_books(&sp_id, true).unwrap().len(), 1);

        dao.mark_books_removed_synced(&sp_id, &["b99".to_string()])
            .unwrap();
        assert_eq!(dao.find_books_removed_synced(&sp_id).unwrap(), vec!["b99"]);

        let readlist = SyncPointReadList {
            sync_point_id: sp_id.clone(),
            readlist_id: "rl1".into(),
            readlist_name: "On Deck".into(),
            readlist_created_date: now_utc(),
            readlist_last_modified_date: now_utc(),
            synced: false,
        };
        dao.insert_readlist(&readlist).unwrap();
        assert_eq!(dao.find_readlists(&sp_id, false).unwrap().len(), 1);
        assert_eq!(dao.find_readlists(&sp_id, true).unwrap().len(), 1);
        dao.mark_readlists_synced(&sp_id, &["rl1".to_string()])
            .unwrap();
        assert_eq!(dao.find_readlists(&sp_id, true).unwrap().len(), 0);
        dao.mark_readlists_removed_synced(&sp_id, &["rl99".to_string()])
            .unwrap();

        dao.insert_readlist_books(&[
            SyncPointReadListBook {
                sync_point_id: sp_id.clone(),
                readlist_id: "rl1".into(),
                book_id: "b1".into(),
            },
            SyncPointReadListBook {
                sync_point_id: sp_id.clone(),
                readlist_id: "rl1".into(),
                book_id: "b2".into(),
            },
        ])
        .unwrap();
        let rl_books = dao
            .find_book_ids_by_readlist_ids(&sp_id, &["rl1".to_string()])
            .unwrap();
        assert_eq!(rl_books.len(), 2);

        dao.delete_one(&sp_id).unwrap();
        assert!(dao.find_by_id(&sp_id).unwrap().is_none());
        for table in [
            "SYNC_POINT_BOOK",
            "SYNC_POINT_BOOK_REMOVED_SYNCED",
            "SYNC_POINT_READLIST",
            "SYNC_POINT_READLIST_BOOK",
            "SYNC_POINT_READLIST_REMOVED_SYNCED",
        ] {
            let n: i64 = dao
                .db
                .ro()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} not empty after delete_one");
        }
    }

    #[test]
    fn delete_by_user() {
        let dao = dao();
        create_user(&dao, "user1");
        create_user(&dao, "user2");
        let sp1 = dao.insert(&sample_sync_point("user1")).unwrap();
        let sp2 = dao.insert(&sample_sync_point("user1")).unwrap();
        let sp3 = dao.insert(&sample_sync_point("user2")).unwrap();
        for sp in [&sp1, &sp2, &sp3] {
            dao.insert_books(&[sample_book(sp, "b1")]).unwrap();
        }

        dao.delete_by_user_id("user1").unwrap();
        assert_eq!(dao.find_by_user_id("user1").unwrap().len(), 0);
        assert_eq!(dao.find_by_user_id("user2").unwrap().len(), 1);
        let books_left: i64 = dao
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM SYNC_POINT_BOOK", [], |r| r.get(0))
            .unwrap();
        assert_eq!(books_left, 1);
    }
}
