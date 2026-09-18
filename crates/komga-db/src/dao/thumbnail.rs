//! DAOs for THUMBNAIL_BOOK / THUMBNAIL_SERIES / THUMBNAIL_COLLECTION / THUMBNAIL_READLIST.

use super::{get_datetime, invalid_column};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::thumbnail::{
    Dimension, ThumbnailBook, ThumbnailReadList, ThumbnailSeries, ThumbnailSeriesCollection,
    ThumbnailType,
};
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::{params_from_iter, Row};

const BOOK_COLUMNS: &str = "ID, BOOK_ID, THUMBNAIL, URL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT, CREATED_DATE, LAST_MODIFIED_DATE";
const SERIES_COLUMNS: &str = "ID, SERIES_ID, THUMBNAIL, URL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT, CREATED_DATE, LAST_MODIFIED_DATE";
const COLLECTION_COLUMNS: &str = "ID, COLLECTION_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT, CREATED_DATE, LAST_MODIFIED_DATE";
const READLIST_COLUMNS: &str = "ID, READLIST_ID, THUMBNAIL, SELECTED, TYPE, MEDIA_TYPE, FILE_SIZE, WIDTH, HEIGHT, CREATED_DATE, LAST_MODIFIED_DATE";

fn get_type(row: &Row<'_>, idx: usize) -> rusqlite::Result<ThumbnailType> {
    let s: String = row.get(idx)?;
    ThumbnailType::from_str(&s).ok_or_else(|| invalid_column(idx, "TYPE", &s))
}

pub struct ThumbnailBookDao {
    db: Database,
    tsid: TsidFactory,
}

impl ThumbnailBookDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_thumbnail(row: &Row<'_>) -> rusqlite::Result<ThumbnailBook> {
        Ok(ThumbnailBook {
            id: row.get(0)?,
            book_id: row.get(1)?,
            thumbnail: row.get(2)?,
            url: row.get(3)?,
            selected: row.get(4)?,
            type_: get_type(row, 5)?,
            media_type: row.get(6)?,
            file_size: row.get(7)?,
            dimension: Dimension {
                width: row.get(8)?,
                height: row.get(9)?,
            },
            created_date: get_datetime(row, 10)?,
            last_modified_date: get_datetime(row, 11)?,
        })
    }

    fn params(id: &str, t: &ThumbnailBook) -> Vec<Box<dyn rusqlite::ToSql>> {
        vec![
            Box::new(id.to_string()),
            Box::new(t.book_id.clone()),
            Box::new(t.thumbnail.clone()),
            Box::new(t.url.clone()),
            Box::new(t.selected),
            Box::new(t.type_.as_str().to_string()),
            Box::new(t.media_type.clone()),
            Box::new(t.file_size),
            Box::new(t.dimension.width),
            Box::new(t.dimension.height),
            Box::new(time_codec::format_datetime(t.created_date)),
            Box::new(time_codec::format_datetime(t.last_modified_date)),
        ]
    }

    pub fn find_by_id(&self, thumbnail_id: &str) -> Result<Option<ThumbnailBook>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM THUMBNAIL_BOOK WHERE ID = ?"
        ))?;
        let mut rows = stmt.query_map([thumbnail_id], Self::row_to_thumbnail)?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_all_by_book_id(&self, book_id: &str) -> Result<Vec<ThumbnailBook>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM THUMBNAIL_BOOK WHERE BOOK_ID = ?"
        ))?;
        let rows = stmt
            .query_map([book_id], Self::row_to_thumbnail)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_library_id_or_null(&self, thumbnail_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT BOOK.LIBRARY_ID FROM THUMBNAIL_BOOK LEFT JOIN BOOK ON THUMBNAIL_BOOK.BOOK_ID = BOOK.ID WHERE THUMBNAIL_BOOK.ID = ?",
        )?;
        let mut rows = stmt.query_map([thumbnail_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_series_id_or_null(&self, thumbnail_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT BOOK.SERIES_ID FROM THUMBNAIL_BOOK LEFT JOIN BOOK ON THUMBNAIL_BOOK.BOOK_ID = BOOK.ID WHERE THUMBNAIL_BOOK.ID = ?",
        )?;
        let mut rows = stmt.query_map([thumbnail_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_all_by_book_id_and_type(
        &self,
        book_id: &str,
        type_: ThumbnailType,
    ) -> Result<Vec<ThumbnailBook>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM THUMBNAIL_BOOK WHERE BOOK_ID = ? AND TYPE = ?"
        ))?;
        let rows = stmt
            .query_map((book_id, type_.as_str()), Self::row_to_thumbnail)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_selected_by_book_id(&self, book_id: &str) -> Result<Option<ThumbnailBook>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM THUMBNAIL_BOOK WHERE BOOK_ID = ? AND SELECTED = 1"
        ))?;
        let mut rows = stmt.query_map([book_id], Self::row_to_thumbnail)?;
        Ok(rows.next().transpose()?)
    }

    pub fn exists_by_id(&self, thumbnail_id: &str) -> Result<bool> {
        let conn = self.db.ro();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM THUMBNAIL_BOOK WHERE ID = ?",
            [thumbnail_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn insert(&self, thumbnail: &ThumbnailBook) -> Result<String> {
        let conn = self.db.rw();
        let id = if thumbnail.id.is_empty() {
            self.tsid.create_string()
        } else {
            thumbnail.id.clone()
        };
        conn.execute(
            &format!(
                "INSERT INTO THUMBNAIL_BOOK ({BOOK_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)"
            ),
            params_from_iter(Self::params(&id, thumbnail)),
        )?;
        Ok(id)
    }

    pub fn update(&self, thumbnail: &ThumbnailBook) -> Result<()> {
        let conn = self.db.rw();
        let sets = BOOK_COLUMNS
            .split(',')
            .map(|c| format!("{} = ?", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Self::params(&thumbnail.id, thumbnail);
        values.push(Box::new(thumbnail.id.clone()));
        conn.execute(
            &format!("UPDATE THUMBNAIL_BOOK SET {sets} WHERE ID = ?"),
            params_from_iter(values),
        )?;
        Ok(())
    }

    /// Marks the given thumbnail as selected and deselects all others of the same book.
    pub fn mark_selected(&self, thumbnail: &ThumbnailBook) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "UPDATE THUMBNAIL_BOOK SET SELECTED = 0 WHERE BOOK_ID = ? AND ID <> ?",
            (&thumbnail.book_id, &thumbnail.id),
        )?;
        conn.execute(
            "UPDATE THUMBNAIL_BOOK SET SELECTED = 1 WHERE BOOK_ID = ? AND ID = ?",
            (&thumbnail.book_id, &thumbnail.id),
        )?;
        Ok(())
    }

    pub fn delete(&self, thumbnail_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM THUMBNAIL_BOOK WHERE ID = ?", [thumbnail_id])?;
        Ok(())
    }

    pub fn delete_by_book_id(&self, book_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM THUMBNAIL_BOOK WHERE BOOK_ID = ?", [book_id])?;
        Ok(())
    }

    pub fn delete_by_book_ids(&self, book_ids: &[String]) -> Result<()> {
        let conn = self.db.rw();
        let mut stmt = conn.prepare("DELETE FROM THUMBNAIL_BOOK WHERE BOOK_ID = ?")?;
        for id in book_ids {
            stmt.execute([id])?;
        }
        Ok(())
    }

    pub fn delete_by_book_id_and_type(&self, book_id: &str, type_: ThumbnailType) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM THUMBNAIL_BOOK WHERE BOOK_ID = ? AND TYPE = ?",
            (book_id, type_.as_str()),
        )?;
        Ok(())
    }
}

pub struct ThumbnailSeriesDao {
    db: Database,
    tsid: TsidFactory,
}

impl ThumbnailSeriesDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_thumbnail(row: &Row<'_>) -> rusqlite::Result<ThumbnailSeries> {
        Ok(ThumbnailSeries {
            id: row.get(0)?,
            series_id: row.get(1)?,
            thumbnail: row.get(2)?,
            url: row.get(3)?,
            selected: row.get(4)?,
            type_: get_type(row, 5)?,
            media_type: row.get(6)?,
            file_size: row.get(7)?,
            dimension: Dimension {
                width: row.get(8)?,
                height: row.get(9)?,
            },
            created_date: get_datetime(row, 10)?,
            last_modified_date: get_datetime(row, 11)?,
        })
    }

    fn params(id: &str, t: &ThumbnailSeries) -> Vec<Box<dyn rusqlite::ToSql>> {
        vec![
            Box::new(id.to_string()),
            Box::new(t.series_id.clone()),
            Box::new(t.thumbnail.clone()),
            Box::new(t.url.clone()),
            Box::new(t.selected),
            Box::new(t.type_.as_str().to_string()),
            Box::new(t.media_type.clone()),
            Box::new(t.file_size),
            Box::new(t.dimension.width),
            Box::new(t.dimension.height),
            Box::new(time_codec::format_datetime(t.created_date)),
            Box::new(time_codec::format_datetime(t.last_modified_date)),
        ]
    }

    pub fn find_by_id(&self, thumbnail_id: &str) -> Result<Option<ThumbnailSeries>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SERIES_COLUMNS} FROM THUMBNAIL_SERIES WHERE ID = ?"
        ))?;
        let mut rows = stmt.query_map([thumbnail_id], Self::row_to_thumbnail)?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_all_by_series_id(&self, series_id: &str) -> Result<Vec<ThumbnailSeries>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SERIES_COLUMNS} FROM THUMBNAIL_SERIES WHERE SERIES_ID = ?"
        ))?;
        let rows = stmt
            .query_map([series_id], Self::row_to_thumbnail)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_library_id_or_null(&self, thumbnail_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT SERIES.LIBRARY_ID FROM THUMBNAIL_SERIES LEFT JOIN SERIES ON THUMBNAIL_SERIES.SERIES_ID = SERIES.ID WHERE THUMBNAIL_SERIES.ID = ?",
        )?;
        let mut rows = stmt.query_map([thumbnail_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_series_id_or_null(&self, thumbnail_id: &str) -> Result<Option<String>> {
        let conn = self.db.ro();
        let mut stmt =
            conn.prepare("SELECT SERIES_ID FROM THUMBNAIL_SERIES WHERE ID = ?")?;
        let mut rows = stmt.query_map([thumbnail_id], |r| r.get(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_all_by_series_id_and_type(
        &self,
        series_id: &str,
        type_: ThumbnailType,
    ) -> Result<Vec<ThumbnailSeries>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SERIES_COLUMNS} FROM THUMBNAIL_SERIES WHERE SERIES_ID = ? AND TYPE = ?"
        ))?;
        let rows = stmt
            .query_map((series_id, type_.as_str()), Self::row_to_thumbnail)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_selected_by_series_id(&self, series_id: &str) -> Result<Option<ThumbnailSeries>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SERIES_COLUMNS} FROM THUMBNAIL_SERIES WHERE SERIES_ID = ? AND SELECTED = 1"
        ))?;
        let mut rows = stmt.query_map([series_id], Self::row_to_thumbnail)?;
        Ok(rows.next().transpose()?)
    }

    pub fn insert(&self, thumbnail: &ThumbnailSeries) -> Result<String> {
        let conn = self.db.rw();
        let id = if thumbnail.id.is_empty() {
            self.tsid.create_string()
        } else {
            thumbnail.id.clone()
        };
        conn.execute(
            &format!(
                "INSERT INTO THUMBNAIL_SERIES ({SERIES_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)"
            ),
            params_from_iter(Self::params(&id, thumbnail)),
        )?;
        Ok(id)
    }

    pub fn update(&self, thumbnail: &ThumbnailSeries) -> Result<()> {
        let conn = self.db.rw();
        let sets = SERIES_COLUMNS
            .split(',')
            .map(|c| format!("{} = ?", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Self::params(&thumbnail.id, thumbnail);
        values.push(Box::new(thumbnail.id.clone()));
        conn.execute(
            &format!("UPDATE THUMBNAIL_SERIES SET {sets} WHERE ID = ?"),
            params_from_iter(values),
        )?;
        Ok(())
    }

    pub fn mark_selected(&self, thumbnail: &ThumbnailSeries) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "UPDATE THUMBNAIL_SERIES SET SELECTED = 0 WHERE SERIES_ID = ? AND ID <> ?",
            (&thumbnail.series_id, &thumbnail.id),
        )?;
        conn.execute(
            "UPDATE THUMBNAIL_SERIES SET SELECTED = 1 WHERE SERIES_ID = ? AND ID = ?",
            (&thumbnail.series_id, &thumbnail.id),
        )?;
        Ok(())
    }

    pub fn delete(&self, thumbnail_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM THUMBNAIL_SERIES WHERE ID = ?", [thumbnail_id])?;
        Ok(())
    }

    pub fn delete_by_series_id(&self, series_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM THUMBNAIL_SERIES WHERE SERIES_ID = ?",
            [series_id],
        )?;
        Ok(())
    }

    pub fn delete_by_series_ids(&self, series_ids: &[String]) -> Result<()> {
        let conn = self.db.rw();
        let mut stmt = conn.prepare("DELETE FROM THUMBNAIL_SERIES WHERE SERIES_ID = ?")?;
        for id in series_ids {
            stmt.execute([id])?;
        }
        Ok(())
    }
}

pub struct ThumbnailSeriesCollectionDao {
    db: Database,
    tsid: TsidFactory,
}

impl ThumbnailSeriesCollectionDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_thumbnail(row: &Row<'_>) -> rusqlite::Result<ThumbnailSeriesCollection> {
        Ok(ThumbnailSeriesCollection {
            id: row.get(0)?,
            collection_id: row.get(1)?,
            thumbnail: row.get(2)?,
            selected: row.get(3)?,
            type_: get_type(row, 4)?,
            media_type: row.get(5)?,
            file_size: row.get(6)?,
            dimension: Dimension {
                width: row.get(7)?,
                height: row.get(8)?,
            },
            created_date: get_datetime(row, 9)?,
            last_modified_date: get_datetime(row, 10)?,
        })
    }

    fn params(id: &str, t: &ThumbnailSeriesCollection) -> Vec<Box<dyn rusqlite::ToSql>> {
        vec![
            Box::new(id.to_string()),
            Box::new(t.collection_id.clone()),
            Box::new(t.thumbnail.clone()),
            Box::new(t.selected),
            Box::new(t.type_.as_str().to_string()),
            Box::new(t.media_type.clone()),
            Box::new(t.file_size),
            Box::new(t.dimension.width),
            Box::new(t.dimension.height),
            Box::new(time_codec::format_datetime(t.created_date)),
            Box::new(time_codec::format_datetime(t.last_modified_date)),
        ]
    }

    pub fn find_by_id(&self, thumbnail_id: &str) -> Result<Option<ThumbnailSeriesCollection>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLLECTION_COLUMNS} FROM THUMBNAIL_COLLECTION WHERE ID = ?"
        ))?;
        let mut rows = stmt.query_map([thumbnail_id], Self::row_to_thumbnail)?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_all_by_collection_id(
        &self,
        collection_id: &str,
    ) -> Result<Vec<ThumbnailSeriesCollection>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLLECTION_COLUMNS} FROM THUMBNAIL_COLLECTION WHERE COLLECTION_ID = ?"
        ))?;
        let rows = stmt
            .query_map([collection_id], Self::row_to_thumbnail)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_selected_by_collection_id(
        &self,
        collection_id: &str,
    ) -> Result<Option<ThumbnailSeriesCollection>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
      "SELECT {COLLECTION_COLUMNS} FROM THUMBNAIL_COLLECTION WHERE COLLECTION_ID = ? AND SELECTED = 1"
    ))?;
        let mut rows = stmt.query_map([collection_id], Self::row_to_thumbnail)?;
        Ok(rows.next().transpose()?)
    }

    pub fn insert(&self, thumbnail: &ThumbnailSeriesCollection) -> Result<String> {
        let conn = self.db.rw();
        let id = if thumbnail.id.is_empty() {
            self.tsid.create_string()
        } else {
            thumbnail.id.clone()
        };
        conn.execute(
      &format!("INSERT INTO THUMBNAIL_COLLECTION ({COLLECTION_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?)"),
      params_from_iter(Self::params(&id, thumbnail)),
    )?;
        Ok(id)
    }

    pub fn update(&self, thumbnail: &ThumbnailSeriesCollection) -> Result<()> {
        let conn = self.db.rw();
        let sets = COLLECTION_COLUMNS
            .split(',')
            .map(|c| format!("{} = ?", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Self::params(&thumbnail.id, thumbnail);
        values.push(Box::new(thumbnail.id.clone()));
        conn.execute(
            &format!("UPDATE THUMBNAIL_COLLECTION SET {sets} WHERE ID = ?"),
            params_from_iter(values),
        )?;
        Ok(())
    }

    pub fn mark_selected(&self, thumbnail: &ThumbnailSeriesCollection) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "UPDATE THUMBNAIL_COLLECTION SET SELECTED = 0 WHERE COLLECTION_ID = ? AND ID <> ?",
            (&thumbnail.collection_id, &thumbnail.id),
        )?;
        conn.execute(
            "UPDATE THUMBNAIL_COLLECTION SET SELECTED = 1 WHERE COLLECTION_ID = ? AND ID = ?",
            (&thumbnail.collection_id, &thumbnail.id),
        )?;
        Ok(())
    }

    pub fn delete(&self, thumbnail_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM THUMBNAIL_COLLECTION WHERE ID = ?",
            [thumbnail_id],
        )?;
        Ok(())
    }

    pub fn delete_by_collection_id(&self, collection_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM THUMBNAIL_COLLECTION WHERE COLLECTION_ID = ?",
            [collection_id],
        )?;
        Ok(())
    }

    pub fn delete_by_collection_ids(&self, collection_ids: &[String]) -> Result<()> {
        let conn = self.db.rw();
        let mut stmt = conn.prepare("DELETE FROM THUMBNAIL_COLLECTION WHERE COLLECTION_ID = ?")?;
        for id in collection_ids {
            stmt.execute([id])?;
        }
        Ok(())
    }
}

pub struct ThumbnailReadListDao {
    db: Database,
    tsid: TsidFactory,
}

impl ThumbnailReadListDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_thumbnail(row: &Row<'_>) -> rusqlite::Result<ThumbnailReadList> {
        Ok(ThumbnailReadList {
            id: row.get(0)?,
            read_list_id: row.get(1)?,
            thumbnail: row.get(2)?,
            selected: row.get(3)?,
            type_: get_type(row, 4)?,
            media_type: row.get(5)?,
            file_size: row.get(6)?,
            dimension: Dimension {
                width: row.get(7)?,
                height: row.get(8)?,
            },
            created_date: get_datetime(row, 9)?,
            last_modified_date: get_datetime(row, 10)?,
        })
    }

    fn params(id: &str, t: &ThumbnailReadList) -> Vec<Box<dyn rusqlite::ToSql>> {
        vec![
            Box::new(id.to_string()),
            Box::new(t.read_list_id.clone()),
            Box::new(t.thumbnail.clone()),
            Box::new(t.selected),
            Box::new(t.type_.as_str().to_string()),
            Box::new(t.media_type.clone()),
            Box::new(t.file_size),
            Box::new(t.dimension.width),
            Box::new(t.dimension.height),
            Box::new(time_codec::format_datetime(t.created_date)),
            Box::new(time_codec::format_datetime(t.last_modified_date)),
        ]
    }

    pub fn find_by_id(&self, thumbnail_id: &str) -> Result<Option<ThumbnailReadList>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {READLIST_COLUMNS} FROM THUMBNAIL_READLIST WHERE ID = ?"
        ))?;
        let mut rows = stmt.query_map([thumbnail_id], Self::row_to_thumbnail)?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_all_by_read_list_id(&self, read_list_id: &str) -> Result<Vec<ThumbnailReadList>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
            "SELECT {READLIST_COLUMNS} FROM THUMBNAIL_READLIST WHERE READLIST_ID = ?"
        ))?;
        let rows = stmt
            .query_map([read_list_id], Self::row_to_thumbnail)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_selected_by_read_list_id(
        &self,
        read_list_id: &str,
    ) -> Result<Option<ThumbnailReadList>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!(
      "SELECT {READLIST_COLUMNS} FROM THUMBNAIL_READLIST WHERE READLIST_ID = ? AND SELECTED = 1"
    ))?;
        let mut rows = stmt.query_map([read_list_id], Self::row_to_thumbnail)?;
        Ok(rows.next().transpose()?)
    }

    pub fn insert(&self, thumbnail: &ThumbnailReadList) -> Result<String> {
        let conn = self.db.rw();
        let id = if thumbnail.id.is_empty() {
            self.tsid.create_string()
        } else {
            thumbnail.id.clone()
        };
        conn.execute(
      &format!("INSERT INTO THUMBNAIL_READLIST ({READLIST_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?)"),
      params_from_iter(Self::params(&id, thumbnail)),
    )?;
        Ok(id)
    }

    pub fn update(&self, thumbnail: &ThumbnailReadList) -> Result<()> {
        let conn = self.db.rw();
        let sets = READLIST_COLUMNS
            .split(',')
            .map(|c| format!("{} = ?", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Self::params(&thumbnail.id, thumbnail);
        values.push(Box::new(thumbnail.id.clone()));
        conn.execute(
            &format!("UPDATE THUMBNAIL_READLIST SET {sets} WHERE ID = ?"),
            params_from_iter(values),
        )?;
        Ok(())
    }

    pub fn mark_selected(&self, thumbnail: &ThumbnailReadList) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "UPDATE THUMBNAIL_READLIST SET SELECTED = 0 WHERE READLIST_ID = ? AND ID <> ?",
            (&thumbnail.read_list_id, &thumbnail.id),
        )?;
        conn.execute(
            "UPDATE THUMBNAIL_READLIST SET SELECTED = 1 WHERE READLIST_ID = ? AND ID = ?",
            (&thumbnail.read_list_id, &thumbnail.id),
        )?;
        Ok(())
    }

    pub fn delete(&self, thumbnail_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM THUMBNAIL_READLIST WHERE ID = ?",
            [thumbnail_id],
        )?;
        Ok(())
    }

    pub fn delete_by_read_list_id(&self, read_list_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            "DELETE FROM THUMBNAIL_READLIST WHERE READLIST_ID = ?",
            [read_list_id],
        )?;
        Ok(())
    }

    pub fn delete_by_read_list_ids(&self, read_list_ids: &[String]) -> Result<()> {
        let conn = self.db.rw();
        let mut stmt = conn.prepare("DELETE FROM THUMBNAIL_READLIST WHERE READLIST_ID = ?")?;
        for id in read_list_ids {
            stmt.execute([id])?;
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

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    /// THUMBNAIL_BOOK/SERIES have FKs, so seed the library→series→book chain first.
    fn insert_book_chain(db: &Database, book_id: &str) {
        let conn = db.rw();
        conn
      .execute_batch(&format!(
        "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES ('lib1', 'L', 'file:/l/');
         INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES ('ser1', 'S', 'file:/l/s/', '2020-01-01 00:00:00', 'lib1');
         INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) VALUES ('{book_id}', 'B', 'file:/l/s/b.cbz', '2020-01-01 00:00:00', 'ser1', 'lib1');"
      ))
      .unwrap();
    }

    fn sample_book_thumbnail(book_id: &str, type_: ThumbnailType) -> ThumbnailBook {
        ThumbnailBook {
            id: String::new(),
            book_id: book_id.into(),
            thumbnail: Some(vec![1, 2, 3]),
            url: None,
            selected: false,
            type_,
            media_type: "image/jpeg".into(),
            file_size: 3,
            dimension: Dimension {
                width: 100,
                height: 200,
            },
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn book_crud_and_mark_selected() {
        let db = db();
        insert_book_chain(&db, "b1");
        let dao = ThumbnailBookDao::new(db);

        let id1 = dao
            .insert(&sample_book_thumbnail("b1", ThumbnailType::Generated))
            .unwrap();
        assert_eq!(id1.len(), 13);
        let mut sidecar = sample_book_thumbnail("b1", ThumbnailType::Sidecar);
        sidecar.thumbnail = None;
        sidecar.url = Some("file:/l/s/cover.jpg".into());
        let id2 = dao.insert(&sidecar).unwrap();

        let found = dao.find_by_id(&id2).unwrap().unwrap();
        assert_eq!(found.type_, ThumbnailType::Sidecar);
        assert_eq!(found.url.as_deref(), Some("file:/l/s/cover.jpg"));
        assert!(found.thumbnail.is_none());
        assert_eq!(
            found.dimension,
            Dimension {
                width: 100,
                height: 200
            }
        );

        assert_eq!(dao.find_all_by_book_id("b1").unwrap().len(), 2);
        assert_eq!(
            dao.find_all_by_book_id_and_type("b1", ThumbnailType::Generated)
                .unwrap()
                .len(),
            1
        );
        assert!(dao.exists_by_id(&id1).unwrap());

        let mut selected = dao.find_by_id(&id2).unwrap().unwrap();
        selected.selected = true;
        dao.mark_selected(&selected).unwrap();
        assert!(dao.find_selected_by_book_id("b1").unwrap().unwrap().id == id2);
        assert!(!dao.find_by_id(&id1).unwrap().unwrap().selected);

        let mut updated = dao.find_by_id(&id1).unwrap().unwrap();
        updated.media_type = "image/png".into();
        updated.dimension = Dimension {
            width: 1,
            height: 2,
        };
        dao.update(&updated).unwrap();
        let found = dao.find_by_id(&id1).unwrap().unwrap();
        assert_eq!(found.media_type, "image/png");
        assert_eq!(
            found.dimension,
            Dimension {
                width: 1,
                height: 2
            }
        );

        dao.delete_by_book_id_and_type("b1", ThumbnailType::Generated)
            .unwrap();
        assert_eq!(dao.find_all_by_book_id("b1").unwrap().len(), 1);
        dao.delete(&id2).unwrap();
        assert!(dao.find_by_id(&id2).unwrap().is_none());

        let id3 = dao
            .insert(&sample_book_thumbnail("b1", ThumbnailType::UserUploaded))
            .unwrap();
        dao.delete_by_book_ids(&["b1".to_string()]).unwrap();
        assert!(dao.find_by_id(&id3).unwrap().is_none());
    }

    #[test]
    fn series_crud() {
        let db = db();
        insert_book_chain(&db, "b1");
        let dao = ThumbnailSeriesDao::new(db);

        let t = ThumbnailSeries {
            id: String::new(),
            series_id: "ser1".into(),
            thumbnail: Some(vec![9, 9]),
            url: None,
            selected: true,
            type_: ThumbnailType::UserUploaded,
            media_type: "image/jpeg".into(),
            file_size: 2,
            dimension: Dimension {
                width: 10,
                height: 10,
            },
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        let id = dao.insert(&t).unwrap();
        let found = dao.find_selected_by_series_id("ser1").unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.type_, ThumbnailType::UserUploaded);

        dao.delete_by_series_ids(&["ser1".to_string()]).unwrap();
        assert!(dao.find_all_by_series_id("ser1").unwrap().is_empty());
    }

    #[test]
    fn collection_and_readlist_crud() {
        let db = db();
        {
            let conn = db.rw();
            conn.execute_batch(
                "INSERT INTO COLLECTION (ID, NAME, ORDERED, SERIES_COUNT) VALUES ('c1', 'C', 0, 0);
           INSERT INTO READLIST (ID, NAME, BOOK_COUNT) VALUES ('r1', 'R', 0);",
            )
            .unwrap();
        }
        let collection_dao = ThumbnailSeriesCollectionDao::new(db.clone());
        let readlist_dao = ThumbnailReadListDao::new(db);

        let tc = ThumbnailSeriesCollection {
            id: String::new(),
            collection_id: "c1".into(),
            thumbnail: vec![5, 5, 5],
            selected: false,
            type_: ThumbnailType::UserUploaded,
            media_type: "image/jpeg".into(),
            file_size: 3,
            dimension: Dimension {
                width: 5,
                height: 5,
            },
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        let cid = collection_dao.insert(&tc).unwrap();
        let mut found = collection_dao.find_by_id(&cid).unwrap().unwrap();
        assert_eq!(found.thumbnail, vec![5, 5, 5]);
        found.selected = true;
        collection_dao.mark_selected(&found).unwrap();
        assert!(collection_dao
            .find_selected_by_collection_id("c1")
            .unwrap()
            .is_some());
        collection_dao.delete_by_collection_id("c1").unwrap();
        assert!(collection_dao
            .find_all_by_collection_id("c1")
            .unwrap()
            .is_empty());

        let tr = ThumbnailReadList {
            id: String::new(),
            read_list_id: "r1".into(),
            thumbnail: vec![7],
            selected: false,
            type_: ThumbnailType::UserUploaded,
            media_type: "image/jpeg".into(),
            file_size: 1,
            dimension: Dimension {
                width: 7,
                height: 7,
            },
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        let rid = readlist_dao.insert(&tr).unwrap();
        assert!(readlist_dao.find_by_id(&rid).unwrap().is_some());
        readlist_dao
            .delete_by_read_list_ids(&["r1".to_string()])
            .unwrap();
        assert!(readlist_dao
            .find_all_by_read_list_id("r1")
            .unwrap()
            .is_empty());
    }
}
