//! DAO for the kmrs-only `SMART_LIST_THUMBNAIL` table: generated mosaics plus user-uploaded
//! covers of smart lists, same selected-one-wins model as the read-list covers.

use super::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::thumbnail::{Dimension, ThumbnailSmartList, ThumbnailType};
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};

const COLUMNS: &str = "ID, SMART_LIST_ID, THUMBNAIL, FINGERPRINT, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT, CREATED_DATE, LAST_MODIFIED_DATE";

fn row_to_thumbnail(row: &Row<'_>) -> rusqlite::Result<ThumbnailSmartList> {
    let type_: String = row.get(5)?;
    Ok(ThumbnailSmartList {
        id: row.get(0)?,
        smart_list_id: row.get(1)?,
        thumbnail: row.get(2)?,
        fingerprint: row.get(3)?,
        selected: row.get(4)?,
        type_: ThumbnailType::from_str(&type_).unwrap_or(ThumbnailType::UserUploaded),
        media_type: row.get(6)?,
        file_size: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
        dimension: Dimension {
            width: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
            height: row.get::<_, Option<i32>>(9)?.unwrap_or(0),
        },
        created_date: get_datetime(row, 10)?,
        last_modified_date: get_datetime(row, 11)?,
    })
}

pub struct SmartListThumbnailDao {
    db: Database,
    tsid: TsidFactory,
}

impl SmartListThumbnailDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<ThumbnailSmartList>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM SMART_LIST_THUMBNAIL WHERE ID = ?"
        ))?;
        let found = stmt.query_map([id], row_to_thumbnail)?.next().transpose()?;
        Ok(found)
    }

    pub fn find_all_by_smart_list_id(
        &self,
        smart_list_id: &str,
    ) -> Result<Vec<ThumbnailSmartList>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM SMART_LIST_THUMBNAIL WHERE SMART_LIST_ID = ? ORDER BY CREATED_DATE ASC"
        ))?;
        let rows = stmt
            .query_map([smart_list_id], row_to_thumbnail)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_selected_by_smart_list_id(
        &self,
        smart_list_id: &str,
    ) -> Result<Option<ThumbnailSmartList>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM SMART_LIST_THUMBNAIL WHERE SMART_LIST_ID = ? AND SELECTED = 1"
        ))?;
        let found = stmt
            .query_map([smart_list_id], row_to_thumbnail)?
            .next()
            .transpose()?;
        Ok(found)
    }

    pub fn insert(&self, thumbnail: &ThumbnailSmartList) -> Result<String> {
        let conn = self.db.rw()?;
        let id = if thumbnail.id.is_empty() {
            self.tsid.create_string()
        } else {
            thumbnail.id.clone()
        };
        conn.execute(
            "INSERT INTO SMART_LIST_THUMBNAIL \
             (ID, SMART_LIST_ID, THUMBNAIL, FINGERPRINT, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT) \
             VALUES (?,?,?,?,?,?,?,?,?,?)",
            params![
                id,
                thumbnail.smart_list_id,
                thumbnail.thumbnail,
                thumbnail.fingerprint,
                thumbnail.selected,
                thumbnail.type_.as_str(),
                thumbnail.media_type,
                thumbnail.file_size,
                thumbnail.dimension.width,
                thumbnail.dimension.height,
            ],
        )?;
        Ok(id)
    }

    /// exactly one thumbnail per list stays selected, mirroring `ThumbnailReadListDao.mark_selected`
    pub fn mark_selected(&self, thumbnail: &ThumbnailSmartList) -> Result<()> {
        let mut conn = self.db.rw()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE SMART_LIST_THUMBNAIL SET SELECTED = 0 WHERE SMART_LIST_ID = ? AND ID <> ?",
            (&thumbnail.smart_list_id, &thumbnail.id),
        )?;
        tx.execute(
            "UPDATE SMART_LIST_THUMBNAIL SET SELECTED = 1 WHERE SMART_LIST_ID = ? AND ID = ?",
            (&thumbnail.smart_list_id, &thumbnail.id),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.db
            .rw()?
            .execute("DELETE FROM SMART_LIST_THUMBNAIL WHERE ID = ?", [id])?;
        Ok(())
    }

    pub fn delete_by_smart_list_id(&self, smart_list_id: &str) -> Result<()> {
        self.db.rw()?.execute(
            "DELETE FROM SMART_LIST_THUMBNAIL WHERE SMART_LIST_ID = ?",
            [smart_list_id],
        )?;
        Ok(())
    }

    /// cached generated mosaic for the current content fingerprint, when up to date
    pub fn find_generated_by_smart_list_id(
        &self,
        smart_list_id: &str,
        fingerprint: &str,
    ) -> Result<Option<ThumbnailSmartList>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM SMART_LIST_THUMBNAIL \
             WHERE SMART_LIST_ID = ? AND TYPE = 'GENERATED' AND FINGERPRINT = ?"
        ))?;
        let found = stmt
            .query_map(
                rusqlite::params![smart_list_id, fingerprint],
                row_to_thumbnail,
            )?
            .next()
            .transpose()?;
        Ok(found)
    }

    /// store (or replace) the generated mosaic for a fingerprint. Not atomic across
    /// the two statements: concurrent generations with different fingerprints can
    /// delete each other's row, and identical fingerprints can both insert — the
    /// next request re-runs the same deterministic path and converges either way
    pub fn upsert_generated(
        &self,
        smart_list_id: &str,
        thumbnail: &[u8],
        fingerprint: &str,
    ) -> Result<()> {
        let conn = self.db.rw()?;
        conn.execute(
            "INSERT INTO SMART_LIST_THUMBNAIL \
             (ID, SMART_LIST_ID, THUMBNAIL, FINGERPRINT, SELECTED, TYPE) VALUES (?,?,?,?,0,'GENERATED')",
            params![self.tsid.create_string(), smart_list_id, thumbnail, fingerprint],
        )?;
        // one GENERATED row per list: drop stale generations (a user-uploaded cover may be selected)
        conn.execute(
            "DELETE FROM SMART_LIST_THUMBNAIL WHERE SMART_LIST_ID = ? AND TYPE = 'GENERATED' AND FINGERPRINT <> ?",
            params![smart_list_id, fingerprint],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kmrs_migrations, Migrator, Placeholders};
    use komga_core::time_codec::now_utc;

    fn dao() -> SmartListThumbnailDao {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        SmartListThumbnailDao::new(db)
    }

    fn thumbnail(list_id: &str, selected: bool, type_: ThumbnailType) -> ThumbnailSmartList {
        ThumbnailSmartList {
            id: String::new(),
            smart_list_id: list_id.into(),
            thumbnail: vec![1, 2, 3],
            fingerprint: String::new(),
            selected,
            type_,
            media_type: "image/jpeg".into(),
            file_size: 3,
            dimension: Dimension {
                width: 12,
                height: 8,
            },
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn multiple_covers_with_single_selection() {
        let dao = dao();
        let a = dao
            .insert(&thumbnail("sl1", true, ThumbnailType::UserUploaded))
            .unwrap();
        let b = dao
            .insert(&thumbnail("sl1", true, ThumbnailType::UserUploaded))
            .unwrap();

        // two uploads both wanting selection: housekeeping keeps the first selected
        let first = dao.find_by_id(&a).unwrap().unwrap();
        dao.mark_selected(&first).unwrap();
        let all = dao.find_all_by_smart_list_id("sl1").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.iter().filter(|t| t.selected).count(), 1);
        assert!(dao.find_selected_by_smart_list_id("sl1").unwrap().is_some());

        dao.delete(&b).unwrap();
        assert_eq!(dao.find_all_by_smart_list_id("sl1").unwrap().len(), 1);
        dao.delete_by_smart_list_id("sl1").unwrap();
        assert!(dao.find_all_by_smart_list_id("sl1").unwrap().is_empty());
    }

    #[test]
    fn generated_mosaic_roundtrip_replaces_stale_rows() {
        let dao = dao();
        dao.upsert_generated("sl1", &[9, 9], "a,b").unwrap();
        let found = dao
            .find_generated_by_smart_list_id("sl1", "a,b")
            .unwrap()
            .unwrap();
        assert_eq!(found.thumbnail, [9, 9]);
        assert!(!found.selected);

        // a new fingerprint replaces the stale generation
        dao.upsert_generated("sl1", &[7], "a,b,c").unwrap();
        assert!(dao
            .find_generated_by_smart_list_id("sl1", "a,b")
            .unwrap()
            .is_none());
        assert!(dao
            .find_generated_by_smart_list_id("sl1", "a,b,c")
            .unwrap()
            .is_some());
        assert_eq!(dao.find_all_by_smart_list_id("sl1").unwrap().len(), 1);
    }
}
