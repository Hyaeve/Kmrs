//! DAO for READLIST + READLIST_BOOK, aligned with `ReadListDao`.

use super::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::readlist::ReadList;
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};
use std::collections::BTreeMap;

const COLUMNS: &str = "ID, NAME, SUMMARY, ORDERED, BOOK_COUNT, CREATED_DATE, LAST_MODIFIED_DATE";

pub struct ReadListDao {
    db: Database,
    tsid: TsidFactory,
}

impl ReadListDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_readlist(row: &Row<'_>) -> rusqlite::Result<(ReadList, i32)> {
        Ok((
            ReadList {
                id: row.get(0)?,
                name: row.get(1)?,
                summary: row.get(2)?,
                ordered: row.get(3)?,
                book_ids: BTreeMap::new(),
                filtered: false,
                created_date: get_datetime(row, 5)?,
                last_modified_date: get_datetime(row, 6)?,
            },
            row.get(4)?,
        ))
    }

    fn fill_members(&self, readlists: &mut [(ReadList, i32)]) -> Result<()> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT NUMBER, BOOK_ID FROM READLIST_BOOK WHERE READLIST_ID = ? ORDER BY NUMBER ASC",
        )?;
        for (readlist, book_count) in readlists.iter_mut() {
            readlist.book_ids = stmt
                .query_map([&readlist.id], |r| {
                    Ok((r.get::<_, i32>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
            // Same as Java toDomain: filtered = persisted count != actual member count
            readlist.filtered = *book_count as usize != readlist.book_ids.len();
        }
        Ok(())
    }

    fn find_where(&self, condition: &str, param: Option<&str>) -> Result<Vec<ReadList>> {
        let conn = self.db.ro();
        let sql = format!("SELECT {COLUMNS} FROM READLIST {condition}");
        let mut stmt = conn.prepare(&sql)?;
        let mut readlists: Vec<(ReadList, i32)> = match param {
            Some(p) => stmt.query_map([p], Self::row_to_readlist)?,
            None => stmt.query_map([], Self::row_to_readlist)?,
        }
        .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        self.fill_members(&mut readlists)?;
        Ok(readlists.into_iter().map(|(r, _)| r).collect())
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<ReadList>> {
        Ok(self
            .find_where("WHERE ID = ?", Some(id))?
            .into_iter()
            .next())
    }

    /// Name matching uses jOOQ `equalIgnoreCase` (`lower(NAME) = lower(?)`).
    pub fn find_by_name(&self, name: &str) -> Result<Option<ReadList>> {
        Ok(self
            .find_where("WHERE lower(NAME) = lower(?)", Some(name))?
            .into_iter()
            .next())
    }

    pub fn exists_by_name(&self, name: &str) -> Result<bool> {
        let conn = self.db.ro();
        Ok(conn.query_row(
            "SELECT COUNT(*) > 0 FROM READLIST WHERE lower(NAME) = lower(?)",
            [name],
            |r| r.get(0),
        )?)
    }

    pub fn find_all(&self) -> Result<Vec<ReadList>> {
        self.find_where("ORDER BY NAME COLLATE COLLATION_UNICODE_3", None)
    }

    pub fn find_all_empty(&self) -> Result<Vec<ReadList>> {
        self.find_where(
      "WHERE ID IN (SELECT rl.ID FROM READLIST rl LEFT JOIN READLIST_BOOK rlb ON rl.ID = rlb.READLIST_ID WHERE rlb.READLIST_ID IS NULL)",
      None,
    )
    }

    /// `ReadListRepository.findAllContainingBookId` with no restriction/library filters
    /// (the restore path passes none).
    pub fn find_all_containing_book_id(&self, book_id: &str) -> Result<Vec<ReadList>> {
        self.find_where(
            "WHERE ID IN (SELECT READLIST_ID FROM READLIST_BOOK WHERE BOOK_ID = ?)",
            Some(book_id),
        )
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.db.ro();
        Ok(conn.query_row("SELECT COUNT(*) FROM READLIST", [], |r| r.get(0))?)
    }

    /// insert: BOOK_COUNT = book_ids.len(); member NUMBER = the map key.
    pub fn insert(&self, readlist: &ReadList) -> Result<String> {
        let conn = self.db.rw();
        let id = if readlist.id.is_empty() {
            self.tsid.create_string()
        } else {
            readlist.id.clone()
        };
        conn.execute(
            "INSERT INTO READLIST (ID, NAME, SUMMARY, ORDERED, BOOK_COUNT) VALUES (?,?,?,?,?)",
            params![
                id,
                readlist.name,
                readlist.summary,
                readlist.ordered,
                readlist.book_ids.len() as i64
            ],
        )?;
        self.insert_members(&conn, &id, &readlist.book_ids)?;
        Ok(id)
    }

    /// update: name/summary/flags/count + LAST_MODIFIED = app-side UTC now; members
    /// are fully deleted and re-inserted.
    pub fn update(&self, readlist: &ReadList) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      "UPDATE READLIST SET NAME = ?, SUMMARY = ?, ORDERED = ?, BOOK_COUNT = ?, LAST_MODIFIED_DATE = ? WHERE ID = ?",
      params![
        readlist.name,
        readlist.summary,
        readlist.ordered,
        readlist.book_ids.len() as i64,
        time_codec::format_datetime(time_codec::now_utc()),
        readlist.id,
      ],
    )?;
        conn.execute(
            "DELETE FROM READLIST_BOOK WHERE READLIST_ID = ?",
            [&readlist.id],
        )?;
        self.insert_members(&conn, &readlist.id, &readlist.book_ids)?;
        Ok(())
    }

    fn insert_members(
        &self,
        conn: &rusqlite::Connection,
        readlist_id: &str,
        book_ids: &BTreeMap<i32, String>,
    ) -> Result<()> {
        for (number, book_id) in book_ids {
            conn.execute(
                "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES (?,?,?)",
                params![readlist_id, book_id, number],
            )?;
        }
        Ok(())
    }

    pub fn remove_book_from_all(&self, book_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM READLIST_BOOK WHERE BOOK_ID = ?", [book_id])?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM READLIST_BOOK WHERE READLIST_ID = ?", [id])?;
        conn.execute("DELETE FROM READLIST WHERE ID = ?", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::time_codec::now_utc;

    fn dao() -> ReadListDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        ReadListDao::new(db)
    }

    fn seed_book(db: &Database, id: &str) {
        let conn = db.rw();
        conn.execute(
            "INSERT OR IGNORE INTO LIBRARY (ID, NAME, ROOT) VALUES ('lib1', 'L', 'file:/l/')",
            [],
        )
        .unwrap();
        conn.execute(
      "INSERT OR IGNORE INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES ('s1', 'S', 'file:/l/s/', '2020-01-01 00:00:00.0', 'lib1')",
      [],
    )
    .unwrap();
        conn.execute(
      "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) VALUES (?, ?, 'file:/l/s/b.cbz', '2020-01-01 00:00:00.0', 's1', 'lib1')",
      params![id, id],
    )
    .unwrap();
    }

    fn sample() -> ReadList {
        ReadList {
            id: String::new(),
            name: "Reading order".into(),
            summary: "Crossover event".into(),
            ordered: true,
            book_ids: BTreeMap::from([
                (0, "b1".to_string()),
                (1, "b2".to_string()),
                (5, "b3".to_string()),
            ]),
            filtered: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn crud_roundtrip() {
        let dao = dao();
        for b in ["b1", "b2", "b3"] {
            seed_book(&dao.db, b);
        }

        let id = dao.insert(&sample()).unwrap();
        let found = dao.find_by_id(&id).unwrap().expect("not found");
        assert_eq!(found.name, "Reading order");
        assert_eq!(found.summary, "Crossover event");
        assert!(found.ordered);
        // NUMBER keeps the sparse ordinals (map keys), not renumbered by index
        assert_eq!(
            found.book_ids,
            BTreeMap::from([
                (0, "b1".to_string()),
                (1, "b2".to_string()),
                (5, "b3".to_string())
            ])
        );
        assert!(!found.filtered);

        assert!(dao.exists_by_name("reading ORDER").unwrap());
        assert!(dao.find_by_name("READING ORDER").unwrap().is_some());

        let mut updated = found.clone();
        updated.name = "New order".into();
        updated.summary = String::new();
        updated.book_ids = BTreeMap::from([(0, "b3".to_string()), (1, "b1".to_string())]);
        dao.update(&updated).unwrap();
        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.name, "New order");
        assert_eq!(found.summary, "");
        assert_eq!(
            found.book_ids,
            BTreeMap::from([(0, "b3".to_string()), (1, "b1".to_string())])
        );

        dao.remove_book_from_all("b1").unwrap();
        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.book_ids, BTreeMap::from([(0, "b3".to_string())]));
        // BOOK_COUNT was not updated by the removal → filtered is true (same as Java behavior)
        assert!(found.filtered);

        assert_eq!(dao.count().unwrap(), 1);
        dao.delete(&id).unwrap();
        assert!(dao.find_by_id(&id).unwrap().is_none());
        let members: i64 = dao
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM READLIST_BOOK", [], |r| r.get(0))
            .unwrap();
        assert_eq!(members, 0);
    }

    #[test]
    fn find_all_empty_readlists() {
        let dao = dao();
        let empty = ReadList {
            name: "Empty".into(),
            book_ids: BTreeMap::new(),
            ..sample()
        };
        dao.insert(&empty).unwrap();
        assert_eq!(dao.find_all_empty().unwrap().len(), 1);
    }
}
