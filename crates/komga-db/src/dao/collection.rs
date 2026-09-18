//! DAO for COLLECTION + COLLECTION_SERIES, aligned with `SeriesCollectionDao`.

use super::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::collection::SeriesCollection;
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};

const COLUMNS: &str = "ID, NAME, ORDERED, SERIES_COUNT, CREATED_DATE, LAST_MODIFIED_DATE";

pub struct CollectionDao {
    db: Database,
    tsid: TsidFactory,
}

impl CollectionDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_collection(row: &Row<'_>) -> rusqlite::Result<(SeriesCollection, i32)> {
        Ok((
            SeriesCollection {
                id: row.get(0)?,
                name: row.get(1)?,
                ordered: row.get(2)?,
                series_ids: Vec::new(),
                filtered: false,
                created_date: get_datetime(row, 4)?,
                last_modified_date: get_datetime(row, 5)?,
            },
            row.get(3)?,
        ))
    }

    fn fill_members(&self, collections: &mut [(SeriesCollection, i32)]) -> Result<()> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT SERIES_ID FROM COLLECTION_SERIES WHERE COLLECTION_ID = ? ORDER BY NUMBER ASC",
        )?;
        for (collection, series_count) in collections.iter_mut() {
            collection.series_ids = stmt
                .query_map([&collection.id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            // Same as Java toDomain: filtered = persisted count != actual member count
            collection.filtered = *series_count as usize != collection.series_ids.len();
        }
        Ok(())
    }

    fn find_where(&self, condition: &str, param: Option<&str>) -> Result<Vec<SeriesCollection>> {
        let conn = self.db.ro();
        let sql = format!("SELECT {COLUMNS} FROM COLLECTION {condition}");
        let mut stmt = conn.prepare(&sql)?;
        let mut collections: Vec<(SeriesCollection, i32)> = match param {
            Some(p) => stmt.query_map([p], Self::row_to_collection)?,
            None => stmt.query_map([], Self::row_to_collection)?,
        }
        .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        self.fill_members(&mut collections)?;
        Ok(collections.into_iter().map(|(c, _)| c).collect())
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<SeriesCollection>> {
        Ok(self
            .find_where("WHERE ID = ?", Some(id))?
            .into_iter()
            .next())
    }

    /// Name matching uses jOOQ `equalIgnoreCase` (`lower(NAME) = lower(?)`).
    pub fn find_by_name(&self, name: &str) -> Result<Option<SeriesCollection>> {
        Ok(self
            .find_where("WHERE lower(NAME) = lower(?)", Some(name))?
            .into_iter()
            .next())
    }

    pub fn exists_by_name(&self, name: &str) -> Result<bool> {
        let conn = self.db.ro();
        Ok(conn.query_row(
            "SELECT COUNT(*) > 0 FROM COLLECTION WHERE lower(NAME) = lower(?)",
            [name],
            |r| r.get(0),
        )?)
    }

    pub fn find_all(&self) -> Result<Vec<SeriesCollection>> {
        self.find_where("ORDER BY NAME COLLATE COLLATION_UNICODE_3", None)
    }

    pub fn find_all_empty(&self) -> Result<Vec<SeriesCollection>> {
        self.find_where(
      "WHERE ID IN (SELECT c.ID FROM COLLECTION c LEFT JOIN COLLECTION_SERIES cs ON c.ID = cs.COLLECTION_ID WHERE cs.COLLECTION_ID IS NULL)",
      None,
    )
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.db.ro();
        Ok(conn.query_row("SELECT COUNT(*) FROM COLLECTION", [], |r| r.get(0))?)
    }

    /// insert: SERIES_COUNT = series_ids.len(); member NUMBER = index (0-based).
    pub fn insert(&self, collection: &SeriesCollection) -> Result<String> {
        let conn = self.db.rw();
        let id = if collection.id.is_empty() {
            self.tsid.create_string()
        } else {
            collection.id.clone()
        };
        conn.execute(
            "INSERT INTO COLLECTION (ID, NAME, ORDERED, SERIES_COUNT) VALUES (?,?,?,?)",
            params![
                id,
                collection.name,
                collection.ordered,
                collection.series_ids.len() as i64
            ],
        )?;
        self.insert_members(&conn, &id, &collection.series_ids)?;
        Ok(id)
    }

    /// update: name/flags/count + LAST_MODIFIED = app-side UTC now; members are
    /// fully deleted and re-inserted.
    pub fn update(&self, collection: &SeriesCollection) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
      "UPDATE COLLECTION SET NAME = ?, ORDERED = ?, SERIES_COUNT = ?, LAST_MODIFIED_DATE = ? WHERE ID = ?",
      params![
        collection.name,
        collection.ordered,
        collection.series_ids.len() as i64,
        time_codec::format_datetime(time_codec::now_utc()),
        collection.id,
      ],
    )?;
        conn.execute(
            "DELETE FROM COLLECTION_SERIES WHERE COLLECTION_ID = ?",
            [&collection.id],
        )?;
        self.insert_members(&conn, &collection.id, &collection.series_ids)?;
        Ok(())
    }

    fn insert_members(
        &self,
        conn: &rusqlite::Connection,
        collection_id: &str,
        series_ids: &[String],
    ) -> Result<()> {
        for (index, series_id) in series_ids.iter().enumerate() {
            conn.execute(
                "INSERT INTO COLLECTION_SERIES (COLLECTION_ID, SERIES_ID, NUMBER) VALUES (?,?,?)",
                params![collection_id, series_id, index as i64],
            )?;
        }
        Ok(())
    }

    pub fn remove_series_from_all(&self, series_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM COLLECTION_SERIES WHERE SERIES_ID = ?",
            [series_id],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM COLLECTION_SERIES WHERE COLLECTION_ID = ?",
            [id],
        )?;
        conn.execute("DELETE FROM COLLECTION WHERE ID = ?", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::time_codec::now_utc;

    fn dao() -> CollectionDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        CollectionDao::new(db)
    }

    fn seed_series(db: &Database, id: &str) {
        let conn = db.rw();
        conn.execute(
            "INSERT OR IGNORE INTO LIBRARY (ID, NAME, ROOT) VALUES ('lib1', 'L', 'file:/l/')",
            [],
        )
        .unwrap();
        conn.execute(
      "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES (?, ?, 'file:/l/s/', '2020-01-01 00:00:00.0', 'lib1')",
      params![id, id],
    )
    .unwrap();
    }

    fn sample() -> SeriesCollection {
        SeriesCollection {
            id: String::new(),
            name: "Best of 2024".into(),
            ordered: true,
            series_ids: vec!["s1".into(), "s2".into(), "s3".into()],
            filtered: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn crud_roundtrip() {
        let dao = dao();
        for s in ["s1", "s2", "s3"] {
            seed_series(&dao.db, s);
        }

        let id = dao.insert(&sample()).unwrap();
        let found = dao.find_by_id(&id).unwrap().expect("not found");
        assert_eq!(found.name, "Best of 2024");
        assert!(found.ordered);
        assert_eq!(found.series_ids, vec!["s1", "s2", "s3"]);
        assert!(!found.filtered);

        assert!(dao.exists_by_name("best OF 2024").unwrap());
        assert!(dao.find_by_name("BEST OF 2024").unwrap().is_some());
        assert!(!dao.exists_by_name("nope").unwrap());

        let mut updated = found.clone();
        updated.name = "Favorites".into();
        updated.series_ids = vec!["s3".into(), "s1".into()];
        dao.update(&updated).unwrap();
        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.name, "Favorites");
        assert_eq!(found.series_ids, vec!["s3", "s1"]);

        // NUMBER order preserved: s3 comes first
        let numbers: Vec<(String, i64)> = dao
      .db
      .ro()
      .prepare("SELECT SERIES_ID, NUMBER FROM COLLECTION_SERIES WHERE COLLECTION_ID = ? ORDER BY NUMBER")
      .unwrap()
      .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))
      .unwrap()
      .collect::<std::result::Result<Vec<_>, _>>()
      .unwrap();
        assert_eq!(numbers, vec![("s3".to_string(), 0), ("s1".to_string(), 1)]);

        dao.remove_series_from_all("s1").unwrap();
        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.series_ids, vec!["s3"]);
        // SERIES_COUNT was not updated by the removal → filtered is true (same as Java behavior)
        assert!(found.filtered);

        assert_eq!(dao.count().unwrap(), 1);
        dao.delete(&id).unwrap();
        assert!(dao.find_by_id(&id).unwrap().is_none());
        let members: i64 = dao
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM COLLECTION_SERIES", [], |r| r.get(0))
            .unwrap();
        assert_eq!(members, 0);
    }

    #[test]
    fn find_all_empty_collections() {
        let dao = dao();
        let empty = SeriesCollection {
            name: "Empty".into(),
            series_ids: vec![],
            ..sample()
        };
        dao.insert(&empty).unwrap();
        assert_eq!(dao.find_all_empty().unwrap().len(), 1);
        dao.delete(&dao.find_all_empty().unwrap()[0].id).unwrap();
        assert_eq!(dao.find_all_empty().unwrap().len(), 0);
    }
}
