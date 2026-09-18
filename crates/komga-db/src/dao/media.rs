//! DAO for MEDIA / MEDIA_PAGE / MEDIA_FILE.

use super::get_datetime;
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::media::{BookPage, Media, MediaFile, MediaFileSubType, MediaStatus};
use komga_core::time_codec;
use rusqlite::{params, Row};

const MEDIA_COLUMNS: &str = "BOOK_ID, STATUS, MEDIA_TYPE, COMMENT, PAGE_COUNT, \
 EPUB_DIVINA_COMPATIBLE, EPUB_IS_KEPUB, EXTENSION_CLASS, EXTENSION_VALUE_BLOB, \
 CREATED_DATE, LAST_MODIFIED_DATE";

pub struct MediaDao {
    db: Database,
}

impl MediaDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    fn row_to_media(row: &Row<'_>) -> rusqlite::Result<Media> {
        let status: String = row.get(1)?;
        Ok(Media {
            book_id: row.get(0)?,
            status: MediaStatus::from_str(&status)
                .ok_or_else(|| super::invalid_column(1, "STATUS", &status))?,
            media_type: row.get(2)?,
            comment: row.get(3)?,
            page_count: row.get(4)?,
            epub_divina_compatible: row.get(5)?,
            epub_is_kepub: row.get(6)?,
            extension_class: row.get(7)?,
            extension_value: row.get(8)?,
            created_date: get_datetime(row, 9)?,
            last_modified_date: get_datetime(row, 10)?,
            pages: Vec::new(), // filled in by the caller
            files: Vec::new(),
        })
    }

    fn row_to_page(row: &Row<'_>) -> rusqlite::Result<BookPage> {
        Ok(BookPage {
            file_name: row.get(0)?,
            media_type: row.get(1)?,
            width: row.get(2)?,
            height: row.get(3)?,
            file_hash: row.get(4)?,
            file_size: row.get(5)?,
        })
    }

    fn row_to_file(row: &Row<'_>) -> rusqlite::Result<MediaFile> {
        let sub_type: Option<String> = row.get(2)?;
        Ok(MediaFile {
            file_name: row.get(0)?,
            media_type: row.get(1)?,
            sub_type: sub_type
                .map(|s| {
                    MediaFileSubType::from_str(&s)
                        .ok_or_else(|| super::invalid_column(2, "SUB_TYPE", &s))
                })
                .transpose()?,
            file_size: row.get(3)?,
        })
    }

    pub fn find_pages(&self, book_id: &str) -> Result<Vec<BookPage>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
      "SELECT FILE_NAME, MEDIA_TYPE, WIDTH, HEIGHT, FILE_HASH, FILE_SIZE FROM MEDIA_PAGE WHERE BOOK_ID = ? ORDER BY NUMBER",
    )?;
        let pages = stmt
            .query_map([book_id], Self::row_to_page)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(pages)
    }

    fn find_files(&self, book_id: &str) -> Result<Vec<MediaFile>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(
            "SELECT FILE_NAME, MEDIA_TYPE, SUB_TYPE, FILE_SIZE FROM MEDIA_FILE WHERE BOOK_ID = ?",
        )?;
        let files = stmt
            .query_map([book_id], Self::row_to_file)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(files)
    }

    pub fn find_by_id(&self, book_id: &str) -> Result<Option<Media>> {
        let mut media = {
            let conn = self.db.ro();
            let mut stmt = conn.prepare(&format!(
                "SELECT {MEDIA_COLUMNS} FROM MEDIA WHERE BOOK_ID = ?"
            ))?;
            let mut rows = stmt.query_map([book_id], Self::row_to_media)?;
            rows.next().transpose()?
        };
        if let Some(m) = media.as_mut() {
            m.pages = self.find_pages(book_id)?;
            m.files = self.find_files(book_id)?;
        }
        Ok(media)
    }

    pub fn insert(&self, media: &Media) -> Result<()> {
        let conn = self.db.rw();
        conn.execute(
            &format!("INSERT INTO MEDIA ({MEDIA_COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?)"),
            rusqlite::params_from_iter(media_params(media)),
        )?;
        insert_pages(&conn, &media.book_id, &media.pages)?;
        insert_files(&conn, &media.book_id, &media.files)?;
        Ok(())
    }

    /// Same as komga's `MediaDao.update`: updates the main table and sets
    /// LAST_MODIFIED_DATE to the current time; pages/files are deleted and
    /// re-inserted.
    pub fn update(&self, media: &Media) -> Result<()> {
        let conn = self.db.rw();
        let mut values = media_params(media);
        values.truncate(values.len() - 2); // drop CREATED_DATE/LAST_MODIFIED_DATE
        values.remove(0); // drop BOOK_ID
        values.push(Box::new(time_codec::format_datetime(time_codec::now_utc())));
        values.push(Box::new(media.book_id.clone()));
        let sets = MEDIA_COLUMNS
            .split(',')
            .map(|c| c.trim())
            .filter(|c| !["BOOK_ID", "CREATED_DATE", "LAST_MODIFIED_DATE"].contains(c))
            .map(|c| format!("{c} = ?"))
            .collect::<Vec<_>>()
            .join(", ");
        conn.execute(
            &format!("UPDATE MEDIA SET {sets}, LAST_MODIFIED_DATE = ? WHERE BOOK_ID = ?"),
            rusqlite::params_from_iter(values),
        )?;
        conn.execute("DELETE FROM MEDIA_PAGE WHERE BOOK_ID = ?", [&media.book_id])?;
        conn.execute("DELETE FROM MEDIA_FILE WHERE BOOK_ID = ?", [&media.book_id])?;
        insert_pages(&conn, &media.book_id, &media.pages)?;
        insert_files(&conn, &media.book_id, &media.files)?;
        Ok(())
    }

    /// Replaces pages only (e.g. for PAGE_HASH scenarios); leaves the main table
    /// and files untouched.
    pub fn replace_pages(&self, book_id: &str, pages: &[BookPage]) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM MEDIA_PAGE WHERE BOOK_ID = ?", [book_id])?;
        insert_pages(&conn, book_id, pages)?;
        Ok(())
    }

    pub fn delete(&self, book_id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM MEDIA_PAGE WHERE BOOK_ID = ?", [book_id])?;
        conn.execute("DELETE FROM MEDIA_FILE WHERE BOOK_ID = ?", [book_id])?;
        conn.execute("DELETE FROM MEDIA WHERE BOOK_ID = ?", [book_id])?;
        Ok(())
    }
}

fn media_params(m: &Media) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(m.book_id.clone()),
        Box::new(m.status.as_str().to_string()),
        Box::new(m.media_type.clone()),
        Box::new(m.comment.clone()),
        Box::new(m.page_count),
        Box::new(m.epub_divina_compatible),
        Box::new(m.epub_is_kepub),
        Box::new(m.extension_class.clone()),
        Box::new(m.extension_value.clone()),
        Box::new(time_codec::format_datetime(m.created_date)),
        Box::new(time_codec::format_datetime(m.last_modified_date)),
    ]
}

/// Page NUMBER is the 0-based positional index (`MediaDao.insertPages`'s forEachIndexed).
fn insert_pages(conn: &rusqlite::Connection, book_id: &str, pages: &[BookPage]) -> Result<()> {
    for (index, page) in pages.iter().enumerate() {
        conn.execute(
      "INSERT INTO MEDIA_PAGE (BOOK_ID, FILE_NAME, MEDIA_TYPE, NUMBER, WIDTH, HEIGHT, FILE_HASH, FILE_SIZE) \
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
      params![
        book_id,
        page.file_name,
        page.media_type,
        index as i64,
        page.width,
        page.height,
        page.file_hash,
        page.file_size,
      ],
    )?;
    }
    Ok(())
}

fn insert_files(conn: &rusqlite::Connection, book_id: &str, files: &[MediaFile]) -> Result<()> {
    for file in files {
        conn.execute(
      "INSERT INTO MEDIA_FILE (BOOK_ID, FILE_NAME, MEDIA_TYPE, SUB_TYPE, FILE_SIZE) VALUES (?, ?, ?, ?, ?)",
      params![
        book_id,
        file.file_name,
        file.media_type,
        file.sub_type.map(|s| s.as_str()),
        file.file_size,
      ],
    )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::library::LibraryDao;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::model::library::Library;
    use komga_core::time_codec::now_utc;

    fn db_with_book() -> (Database, String) {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();

        let now = now_utc();
        let library_dao = LibraryDao::new(db.clone());
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
        db.rw()
      .execute(
        "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) VALUES ('S1', 'S', 'file:/l/s/', ?, ?)",
        params![time_codec::format_datetime(now), library_id],
      )
      .unwrap();
        db.rw()
      .execute(
        "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) VALUES ('B1', 'b.cbz', 'file:/l/s/b.cbz', ?, 'S1', ?)",
        params![time_codec::format_datetime(now), library_id],
      )
      .unwrap();
        (db, "B1".to_string())
    }

    fn sample_media(book_id: &str) -> Media {
        let now = now_utc();
        Media {
            book_id: book_id.into(),
            status: MediaStatus::Ready,
            media_type: Some("application/zip".into()),
            comment: None,
            page_count: 2,
            pages: vec![
                BookPage {
                    file_name: "p01.jpg".into(),
                    media_type: "image/jpeg".into(),
                    width: Some(800),
                    height: Some(1200),
                    file_hash: "hash1".into(),
                    file_size: Some(100),
                },
                BookPage {
                    file_name: "p02.jpg".into(),
                    media_type: "image/jpeg".into(),
                    width: None,
                    height: None,
                    file_hash: String::new(),
                    file_size: None,
                },
            ],
            files: vec![
                MediaFile {
                    file_name: "OEBPS/p01.xhtml".into(),
                    media_type: Some("application/xhtml+xml".into()),
                    sub_type: Some(MediaFileSubType::EpubPage),
                    file_size: Some(50),
                },
                MediaFile {
                    file_name: "OEBPS/css.css".into(),
                    media_type: None,
                    sub_type: None,
                    file_size: None,
                },
            ],
            extension_class: Some("org.gotson.komga.domain.model.MediaExtension".into()),
            extension_value: Some(vec![1, 2, 3]),
            epub_divina_compatible: true,
            epub_is_kepub: false,
            created_date: now,
            last_modified_date: now,
        }
    }

    #[test]
    fn media_crud_with_pages_and_files() {
        let (db, book_id) = db_with_book();
        let dao = MediaDao::new(db);

        let media = sample_media(&book_id);
        dao.insert(&media).unwrap();

        let found = dao.find_by_id(&book_id).unwrap().expect("not found");
        assert_eq!(found.status, MediaStatus::Ready);
        assert_eq!(found.media_type.as_deref(), Some("application/zip"));
        assert_eq!(found.page_count, 2);
        assert!(found.epub_divina_compatible);
        assert!(!found.epub_is_kepub);
        assert_eq!(
            found.extension_class.as_deref(),
            Some("org.gotson.komga.domain.model.MediaExtension")
        );
        assert_eq!(found.extension_value.as_deref(), Some(&[1u8, 2, 3][..]));

        assert_eq!(found.pages.len(), 2);
        assert_eq!(found.pages[0].file_name, "p01.jpg");
        assert_eq!(found.pages[0].width, Some(800));
        assert_eq!(found.pages[0].file_hash, "hash1");
        assert_eq!(found.pages[1].width, None);
        assert_eq!(found.pages[1].file_size, None);

        assert_eq!(found.files.len(), 2);
        assert_eq!(found.files[0].sub_type, Some(MediaFileSubType::EpubPage));
        assert_eq!(found.files[1].sub_type, None);

        // page NUMBER starts from 0
        let numbers: Vec<i64> = {
            let conn = dao.db.ro();
            let mut stmt = conn
                .prepare("SELECT NUMBER FROM MEDIA_PAGE WHERE BOOK_ID = ? ORDER BY NUMBER")
                .unwrap();
            stmt.query_map([&book_id], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(numbers, vec![0, 1]);

        let mut updated = found.clone();
        updated.status = MediaStatus::Outdated;
        updated.comment = Some("ERR_1006".into());
        updated.epub_is_kepub = true;
        updated.pages = vec![updated.pages[1].clone()];
        updated.files = vec![];
        dao.update(&updated).unwrap();

        let found = dao.find_by_id(&book_id).unwrap().unwrap();
        assert_eq!(found.status, MediaStatus::Outdated);
        assert_eq!(found.comment.as_deref(), Some("ERR_1006"));
        assert!(found.epub_is_kepub);
        assert_eq!(found.pages.len(), 1);
        assert_eq!(found.pages[0].file_name, "p02.jpg");
        assert!(found.files.is_empty());

        dao.delete(&book_id).unwrap();
        assert!(dao.find_by_id(&book_id).unwrap().is_none());
        let remaining: i64 = dao
            .db
            .ro()
            .query_row(
                "SELECT (SELECT COUNT(*) FROM MEDIA_PAGE) + (SELECT COUNT(*) FROM MEDIA_FILE)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn replace_pages_only() {
        let (db, book_id) = db_with_book();
        let dao = MediaDao::new(db);
        dao.insert(&sample_media(&book_id)).unwrap();

        let new_pages = vec![BookPage {
            file_name: "only.jpg".into(),
            media_type: "image/png".into(),
            width: Some(1),
            height: Some(2),
            file_hash: "h".into(),
            file_size: Some(3),
        }];
        dao.replace_pages(&book_id, &new_pages).unwrap();

        let pages = dao.find_pages(&book_id).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].file_name, "only.jpg");

        // files and the main table are unaffected
        let found = dao.find_by_id(&book_id).unwrap().unwrap();
        assert_eq!(found.files.len(), 2);
        assert_eq!(found.status, MediaStatus::Ready);
    }
}
