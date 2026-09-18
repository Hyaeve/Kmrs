//! DAO for LIBRARY + LIBRARY_EXCLUSIONS.

use super::{get_datetime, get_datetime_opt};
use crate::error::Result;
use crate::pool::Database;
use komga_core::model::{Library, ScanInterval, SeriesCover};
use komga_core::time_codec;
use komga_core::tsid::TsidFactory;
use rusqlite::{params, Row};

const COLUMNS: &str = "ID, NAME, ROOT, \
 IMPORT_COMICINFO_BOOK, IMPORT_COMICINFO_SERIES, IMPORT_COMICINFO_COLLECTION, IMPORT_COMICINFO_READLIST, \
 IMPORT_COMICINFO_SERIES_APPEND_VOLUME, IMPORT_EPUB_BOOK, IMPORT_EPUB_SERIES, IMPORT_MYLAR_SERIES, \
 IMPORT_LOCAL_ARTWORK, IMPORT_BARCODE_ISBN, SCAN_FORCE_MODIFIED_TIME, SCAN_STARTUP, SCAN_INTERVAL, \
 SCAN_CBX, SCAN_PDF, SCAN_EPUB, REPAIR_EXTENSIONS, CONVERT_TO_CBZ, EMPTY_TRASH_AFTER_SCAN, SERIES_COVER, \
 HASH_FILES, HASH_PAGES, HASH_KOREADER, ANALYZE_DIMENSIONS, ONESHOTS_DIRECTORY, UNAVAILABLE_DATE, \
 CREATED_DATE, LAST_MODIFIED_DATE";

pub struct LibraryDao {
    db: Database,
    tsid: TsidFactory,
}

impl LibraryDao {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            tsid: TsidFactory::new_random_node(),
        }
    }

    fn row_to_library(row: &Row<'_>) -> rusqlite::Result<Library> {
        let scan_interval: String = row.get(15)?;
        let series_cover: String = row.get(22)?;
        Ok(Library {
            id: row.get(0)?,
            name: row.get(1)?,
            root: row.get(2)?,
            import_comicinfo_book: row.get(3)?,
            import_comicinfo_series: row.get(4)?,
            import_comicinfo_collection: row.get(5)?,
            import_comicinfo_readlist: row.get(6)?,
            import_comicinfo_series_append_volume: row.get(7)?,
            import_epub_book: row.get(8)?,
            import_epub_series: row.get(9)?,
            import_mylar_series: row.get(10)?,
            import_local_artwork: row.get(11)?,
            import_barcode_isbn: row.get(12)?,
            scan_force_modified_time: row.get(13)?,
            scan_on_startup: row.get(14)?,
            scan_interval: ScanInterval::from_str(&scan_interval)
                .ok_or_else(|| super::invalid_column(15, "SCAN_INTERVAL", &scan_interval))?,
            scan_cbx: row.get(16)?,
            scan_pdf: row.get(17)?,
            scan_epub: row.get(18)?,
            repair_extensions: row.get(19)?,
            convert_to_cbz: row.get(20)?,
            empty_trash_after_scan: row.get(21)?,
            series_cover: SeriesCover::from_str(&series_cover)
                .ok_or_else(|| super::invalid_column(22, "SERIES_COVER", &series_cover))?,
            hash_files: row.get(23)?,
            hash_pages: row.get(24)?,
            hash_koreader: row.get(25)?,
            analyze_dimensions: row.get(26)?,
            oneshots_directory: row.get(27)?,
            unavailable_date: get_datetime_opt(row, 28)?,
            created_date: get_datetime(row, 29)?,
            last_modified_date: get_datetime(row, 30)?,
            scan_directory_exclusions: Vec::new(), // filled in by the caller
        })
    }

    fn fill_exclusions(&self, libraries: &mut [Library]) -> Result<()> {
        if libraries.is_empty() {
            return Ok(());
        }
        let conn = self.db.ro();
        let mut stmt = conn.prepare("SELECT LIBRARY_ID, EXCLUSION FROM LIBRARY_EXCLUSIONS")?;
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, exclusion) = row?;
            map.entry(id).or_default().push(exclusion);
        }
        for lib in libraries.iter_mut() {
            lib.scan_directory_exclusions = map.remove(&lib.id).unwrap_or_default();
        }
        Ok(())
    }

    pub fn find_all(&self) -> Result<Vec<Library>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM LIBRARY ORDER BY NAME"))?;
        let mut libraries = stmt
            .query_map([], Self::row_to_library)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        self.fill_exclusions(&mut libraries)?;
        Ok(libraries)
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<Library>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM LIBRARY WHERE ID = ?"))?;
        let mut libraries = stmt
            .query_map([id], Self::row_to_library)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        self.fill_exclusions(&mut libraries)?;
        Ok(libraries.into_iter().next())
    }

    pub fn insert(&self, library: &Library) -> Result<String> {
        let conn = self.db.rw();
        let id = if library.id.is_empty() {
            self.tsid.create_string()
        } else {
            library.id.clone()
        };
        conn.execute(
      &format!("INSERT INTO LIBRARY ({COLUMNS}) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"),
      rusqlite::params_from_iter(library_params(&id, library)),
    )?;
        self.replace_exclusions(&conn, &id, &library.scan_directory_exclusions)?;
        Ok(id)
    }

    pub fn update(&self, library: &Library) -> Result<()> {
        let conn = self.db.rw();
        let sets = COLUMNS
            .split(',')
            .map(|c| format!("{} = ?", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = library_params(&library.id, library);
        values.push(Box::new(library.id.clone()));
        conn.execute(
            &format!("UPDATE LIBRARY SET {sets} WHERE ID = ?"),
            rusqlite::params_from_iter(values),
        )?;
        self.replace_exclusions(&conn, &library.id, &library.scan_directory_exclusions)?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.rw();
        conn.execute("DELETE FROM LIBRARY_EXCLUSIONS WHERE LIBRARY_ID = ?", [id])?;
        conn.execute("DELETE FROM LIBRARY WHERE ID = ?", [id])?;
        Ok(())
    }

    fn replace_exclusions(
        &self,
        conn: &rusqlite::Connection,
        library_id: &str,
        exclusions: &[String],
    ) -> Result<()> {
        conn.execute(
            "DELETE FROM LIBRARY_EXCLUSIONS WHERE LIBRARY_ID = ?",
            [library_id],
        )?;
        for exclusion in exclusions {
            conn.execute(
                "INSERT INTO LIBRARY_EXCLUSIONS (LIBRARY_ID, EXCLUSION) VALUES (?, ?)",
                params![library_id, exclusion],
            )?;
        }
        Ok(())
    }
}

fn library_params(id: &str, l: &Library) -> Vec<Box<dyn rusqlite::ToSql>> {
    vec![
        Box::new(id.to_string()),
        Box::new(l.name.clone()),
        Box::new(l.root.clone()),
        Box::new(l.import_comicinfo_book),
        Box::new(l.import_comicinfo_series),
        Box::new(l.import_comicinfo_collection),
        Box::new(l.import_comicinfo_readlist),
        Box::new(l.import_comicinfo_series_append_volume),
        Box::new(l.import_epub_book),
        Box::new(l.import_epub_series),
        Box::new(l.import_mylar_series),
        Box::new(l.import_local_artwork),
        Box::new(l.import_barcode_isbn),
        Box::new(l.scan_force_modified_time),
        Box::new(l.scan_on_startup),
        Box::new(l.scan_interval.as_str().to_string()),
        Box::new(l.scan_cbx),
        Box::new(l.scan_pdf),
        Box::new(l.scan_epub),
        Box::new(l.repair_extensions),
        Box::new(l.convert_to_cbz),
        Box::new(l.empty_trash_after_scan),
        Box::new(l.series_cover.as_str().to_string()),
        Box::new(l.hash_files),
        Box::new(l.hash_pages),
        Box::new(l.hash_koreader),
        Box::new(l.analyze_dimensions),
        Box::new(l.oneshots_directory.clone()),
        Box::new(l.unavailable_date.map(time_codec::format_datetime)),
        Box::new(time_codec::format_datetime(l.created_date)),
        Box::new(time_codec::format_datetime(l.last_modified_date)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::time_codec::now_utc;

    fn dao() -> LibraryDao {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        LibraryDao::new(db)
    }

    fn sample() -> Library {
        Library {
            id: String::new(),
            name: "Manga".into(),
            root: "file:/data/manga/".into(),
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
            scan_interval: ScanInterval::Every6H,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec!["#recycle".into(), "@eaDir".into()],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: SeriesCover::First,
            hash_files: true,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn crud_roundtrip() {
        let dao = dao();
        let library = sample();
        let id = dao.insert(&library).unwrap();
        assert_eq!(id.len(), 13);

        let found = dao.find_by_id(&id).unwrap().expect("not found");
        assert_eq!(found.name, "Manga");
        assert_eq!(found.root, "file:/data/manga/");
        assert_eq!(found.scan_interval, ScanInterval::Every6H);
        assert_eq!(found.series_cover, SeriesCover::First);
        assert_eq!(
            found.scan_directory_exclusions,
            vec!["#recycle".to_string(), "@eaDir".to_string()]
        );
        assert!(found.hash_files);
        assert!(!found.hash_pages);

        let mut updated = found.clone();
        updated.name = "Comics".into();
        updated.scan_interval = ScanInterval::Weekly;
        updated.series_cover = SeriesCover::Last;
        updated.scan_directory_exclusions = vec![];
        updated.oneshots_directory = Some("oneshots".into());
        dao.update(&updated).unwrap();

        let found = dao.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.name, "Comics");
        assert_eq!(found.scan_interval, ScanInterval::Weekly);
        assert_eq!(found.series_cover, SeriesCover::Last);
        assert!(found.scan_directory_exclusions.is_empty());
        assert_eq!(found.oneshots_directory.as_deref(), Some("oneshots"));

        assert_eq!(dao.find_all().unwrap().len(), 1);
        dao.delete(&id).unwrap();
        assert!(dao.find_by_id(&id).unwrap().is_none());
        let exclusions: i64 = dao
            .db
            .ro()
            .query_row("SELECT COUNT(*) FROM LIBRARY_EXCLUSIONS", [], |r| r.get(0))
            .unwrap();
        assert_eq!(exclusions, 0);
    }
}
