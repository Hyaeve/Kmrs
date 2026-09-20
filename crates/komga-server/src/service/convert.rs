//! `BookConverter.kt`: RAR→CBZ conversion and extension repair, plus
//! `BookPageEditor.removeHashedPages` and the duplicate-page discovery from
//! `PageHashLifecycle.getBookPagesToDeleteAutomatically`.

use crate::events::DomainEvent;
use crate::service::book::BookAction;
use crate::state::AppState;
use komga_core::model::book::Book;
use komga_core::model::history::{HistoricalEvent, HistoricalEventType};
use komga_core::model::library::Library;
use komga_core::model::media::{Media, MediaStatus};
use komga_core::task::BookPageNumbered;
use komga_core::time_codec;
use komga_db::dao::book::BookDao;
use komga_db::dao::history::HistoricalEventDao;
use komga_db::dao::library::LibraryDao;
use komga_db::dao::media::MediaDao;
use komga_db::dao::page_hash::PageHashDao;
use komga_media::analyzer::Analyzer;
use komga_media::container;
use komga_media::scanner::Scanner;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Process-level failure/skip memory, matching the `@Service`-scoped mutable lists on the JVM.
static FAILED_CONVERSIONS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(vec![]));
static SKIPPED_REPAIRS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(vec![]));
static FAILED_PAGE_REMOVAL: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(vec![]));

const CBZ_EXTENSION: &str = "cbz";
const CONVERTIBLE_TYPES: [&str; 2] = [
    "application/x-rar-compressed; version=4",
    "application/x-rar-compressed; version=5",
];

/// `mediaTypeToExtension` (`MediaType.fileExtension`)
const TYPE_RAR_4: &str = "application/x-rar-compressed; version=4";
const TYPE_RAR_5: &str = "application/x-rar-compressed; version=5";
const TYPE_ZIP: &str = "application/zip";
const TYPE_PDF: &str = "application/pdf";
const TYPE_EPUB: &str = "application/epub+zip";

fn media_type_to_extension() -> [(&'static str, &'static str); 5] {
    [
        (TYPE_RAR_4, "cbr"),
        (TYPE_RAR_5, "cbr"),
        (TYPE_ZIP, "cbz"),
        (TYPE_PDF, "pdf"),
        (TYPE_EPUB, "epub"),
    ]
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("{0}")]
    Plain(String),
    #[error("db: {0}")]
    Db(#[from] komga_db::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("media: {0}")]
    Media(#[from] komga_media::MediaError),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
}

type ConvertResult<T> = std::result::Result<T, ConvertError>;

/// `BookConverter.getConvertibleBooks`: RAR books when the library converts to CBZ.
pub fn get_convertible_books(state: &AppState, library: &Library) -> komga_db::Result<Vec<Book>> {
    if !library.convert_to_cbz {
        tracing::info!("CBZ conversion is not enabled, skipping");
        return Ok(vec![]);
    }
    let books = find_all_by_library_id_and_media_types(state, &library.id, &CONVERTIBLE_TYPES)?;
    tracing::info!("Found {} books to convert", books.len());
    Ok(books)
}

/// `BookDao.findAllByLibraryIdAndMediaTypes`
fn find_all_by_library_id_and_media_types(
    state: &AppState,
    library_id: &str,
    media_types: &[&str],
) -> komga_db::Result<Vec<Book>> {
    let conn = state.db.ro();
    let placeholders = media_types
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn.prepare(&format!(
        "SELECT {book_columns} FROM BOOK LEFT JOIN MEDIA ON BOOK.ID = MEDIA.BOOK_ID \
         WHERE BOOK.LIBRARY_ID = ? AND MEDIA.MEDIA_TYPE IN ({placeholders})",
        book_columns = book_columns()
    ))?;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(library_id.to_string())];
    params.extend(
        media_types
            .iter()
            .map(|t| Box::new(t.to_string()) as Box<dyn rusqlite::ToSql>),
    );
    let books = stmt
        .query_map(rusqlite::params_from_iter(params), row_to_book)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(books)
}

/// `BookConverter.getMismatchedExtensionBooks`
pub fn get_mismatched_extension_books(
    state: &AppState,
    library: &Library,
) -> komga_db::Result<Vec<Book>> {
    let mut books = vec![];
    for (media_type, extension) in media_type_to_extension() {
        books.extend(find_all_by_library_id_and_mismatched_extension(
            state,
            &library.id,
            media_type,
            extension,
        )?);
    }
    Ok(books)
}

/// `BookDao.findAllByLibraryIdAndMismatchedExtension`
fn find_all_by_library_id_and_mismatched_extension(
    state: &AppState,
    library_id: &str,
    media_type: &str,
    extension: &str,
) -> komga_db::Result<Vec<Book>> {
    let conn = state.db.ro();
    let mut stmt = conn.prepare(&format!(
        "SELECT {book_columns} FROM BOOK LEFT JOIN MEDIA ON BOOK.ID = MEDIA.BOOK_ID \
         WHERE BOOK.LIBRARY_ID = ? AND MEDIA.MEDIA_TYPE = ? AND BOOK.URL NOT LIKE ?",
        book_columns = book_columns()
    ))?;
    let books = stmt
        .query_map(
            rusqlite::params![library_id, media_type, format!("%.{extension}")],
            row_to_book,
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(books)
}

/// `BookConverter.convertToCbz`: rewrites the RAR as a stored CBZ after verification,
/// restores page hashes, deletes the old file, and records history + event.
pub fn convert_to_cbz(state: &AppState, book: &Book) -> ConvertResult<()> {
    let library = LibraryDao::new(state.db.clone())
        .find_by_id(&book.library_id)?
        .ok_or_else(|| ConvertError::Plain(format!("no library {}", book.library_id)))?;
    if !library.convert_to_cbz {
        tracing::info!(
            "Book conversion is disabled for the library, it may have changed since the task was submitted, skipping"
        );
        return Ok(());
    }
    if FAILED_CONVERSIONS.lock().unwrap().contains(&book.id) {
        tracing::info!("Book conversion already failed before, skipping");
        return Ok(());
    }

    let book_path = book_path(book);
    let scanner = Scanner::new();
    match scanner.scan_file(&book_path) {
        Some(scanned) => {
            if !same_millis(scanned.file_last_modified, book.file_last_modified) {
                tracing::info!("Book has changed on disk, skipping");
                return Ok(());
            }
        }
        None => {
            return Err(ConvertError::Plain(format!(
                "File not found: {}",
                book_path.display()
            )))
        }
    }

    let media = MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| ConvertError::Plain(format!("no media for book {}", book.id)))?;

    if !CONVERTIBLE_TYPES.contains(&media.media_type.as_deref().unwrap_or("")) {
        return Err(ConvertError::Plain(format!(
            "{:?} cannot be converted. Must be one of {CONVERTIBLE_TYPES:?}",
            media.media_type
        )));
    }
    if media.status != MediaStatus::Ready {
        return Err(ConvertError::Media(komga_media::MediaError::NotReady));
    }

    let destination_path = book_path
        .parent()
        .unwrap_or_else(|| Path::new("/"))
        .join(format!(
            "{}.{CBZ_EXTENSION}",
            name_without_extension(&file_name(&book_path))
        ));
    if destination_path.exists() {
        return Err(ConvertError::Plain(format!(
            "Destination file already exists: {}",
            destination_path.display()
        )));
    }

    tracing::info!("Copying archive content to {}", destination_path.display());
    let entries: Vec<String> = media
        .pages
        .iter()
        .map(|p| p.file_name.clone())
        .chain(media.files.iter().map(|f| f.file_name.clone()))
        .collect();
    let write_result = (|| -> ConvertResult<()> {
        let file = std::fs::File::create(&destination_path)?;
        let mut zip = crate::zip_archive::ZipWriter::new(file);
        for entry in &entries {
            let bytes = container::get_file_content(&book_path, &media, entry)?;
            zip.add_entry(entry, std::io::Cursor::new(bytes))?;
        }
        zip.finish()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&destination_path);
        return Err(e);
    }

    let mut converted_book = scanner.scan_file(&destination_path).ok_or_else(|| {
        ConvertError::Plain(format!(
            "Newly converted book could not be scanned: {}",
            file_name(&destination_path)
        ))
    })?;
    converted_book.id = book.id.clone();
    converted_book.series_id = book.series_id.clone();
    converted_book.library_id = book.library_id.clone();

    let analyzer = Analyzer::new(
        state.config.page_hashing,
        state.settings.get().thumbnail_size.max_edge(),
        state.config.epub_divina_letter_count_threshold,
        state
            .kepub
            .kepubify_path(&state.settings.get(), &state.config),
    );
    let mut converted_media = analyzer
        .analyze(&destination_path, library.analyze_dimensions)
        .media;
    converted_media.book_id = converted_book.id.clone();

    let conversion_failure = |message: &str| -> ConvertError {
        let _ = std::fs::remove_file(&destination_path);
        FAILED_CONVERSIONS.lock().unwrap().push(book.id.clone());
        ConvertError::Plain(message.to_string())
    };
    if converted_media.status != MediaStatus::Ready {
        return Err(conversion_failure(
            "Converted file could not be analyzed, aborting conversion",
        ));
    }
    if converted_media.media_type.as_deref() != Some(TYPE_ZIP) {
        return Err(conversion_failure(
            "Converted file is not a zip file, aborting conversion",
        ));
    }
    let converted_pages: Vec<(String, String)> = converted_media
        .pages
        .iter()
        .map(|p| (base_name(&p.file_name), p.media_type.clone()))
        .collect();
    let original_pages: Vec<(String, String)> = media
        .pages
        .iter()
        .map(|p| (base_name(&p.file_name), p.media_type.clone()))
        .collect();
    if !original_pages.iter().all(|p| converted_pages.contains(p)) {
        return Err(conversion_failure(
            "Converted file does not contain all pages from existing file, aborting conversion",
        ));
    }
    let converted_files: Vec<String> = converted_media
        .files
        .iter()
        .map(|f| base_name(&f.file_name))
        .collect();
    let original_files: Vec<String> = media
        .files
        .iter()
        .map(|f| base_name(&f.file_name))
        .collect();
    if !original_files.iter().all(|f| converted_files.contains(f)) {
        return Err(conversion_failure(
            "Converted file does not contain all files from existing file, aborting conversion",
        ));
    }

    if book_path.exists() && std::fs::remove_file(&book_path).is_ok() {
        tracing::info!("Deleted old file: {}", book_path.display());
        insert_history(
            state,
            HistoricalEventType::BookFileDeleted,
            Some(&book.id),
            Some(&book.series_id),
            [
                (
                    "reason",
                    "File was deleted after conversion to CBZ".to_string(),
                ),
                ("name", book_path.display().to_string()),
            ],
        )?;
    }

    let media_with_hashes = Media {
        pages: restore_hash_from(&converted_media.pages, &media.pages),
        ..converted_media
    };
    BookDao::new(state.db.clone()).update(&converted_book)?;
    MediaDao::new(state.db.clone()).update(&media_with_hashes)?;

    insert_history(
        state,
        HistoricalEventType::BookConverted,
        Some(&converted_book.id),
        Some(&converted_book.series_id),
        [
            ("name", destination_path.display().to_string()),
            ("former file", book_path.display().to_string()),
        ],
    )?;
    let _ = state.events.send(DomainEvent::BookUpdated(converted_book));
    Ok(())
}

/// `BookConverter.repairExtension`: renames the file to the extension matching its media type.
pub fn repair_extension(state: &AppState, book: &Book) -> ConvertResult<()> {
    let library = LibraryDao::new(state.db.clone())
        .find_by_id(&book.library_id)?
        .ok_or_else(|| ConvertError::Plain(format!("no library {}", book.library_id)))?;
    if !library.repair_extensions {
        tracing::info!(
            "Repair extensions is disabled for the library, it may have changed since the task was submitted, skipping"
        );
        return Ok(());
    }
    if SKIPPED_REPAIRS.lock().unwrap().contains(&book.id) {
        tracing::info!("Extension repair has already been skipped before, skipping");
        return Ok(());
    }

    let book_path = book_path(book);
    if !book_path.exists() {
        return Err(ConvertError::Plain(format!(
            "File not found: {}",
            book_path.display()
        )));
    }

    let media = MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| ConvertError::Plain(format!("no media for book {}", book.id)))?;

    let type_to_extension = media_type_to_extension();
    let media_type = media.media_type.as_deref().unwrap_or("");
    if !type_to_extension.iter().any(|(t, _)| *t == media_type) {
        let keys: Vec<&str> = type_to_extension.iter().map(|(t, _)| *t).collect();
        return Err(ConvertError::Plain(format!(
            "{media_type} cannot be repaired. Must be one of {keys:?}"
        )));
    }

    let actual_extension = book_path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    if actual_extension.eq_ignore_ascii_case("epub") && media_type == TYPE_ZIP {
        SKIPPED_REPAIRS.lock().unwrap().push(book.id.clone());
        tracing::info!(
            "EPUB file detected as zip should not be repaired, skipping: {}",
            book_path.display()
        );
        return Ok(());
    }

    let correct_extension = type_to_extension
        .iter()
        .find(|(t, _)| *t == media_type)
        .map(|(_, e)| *e)
        .unwrap_or("");
    if correct_extension == actual_extension {
        tracing::info!(
            "MediaType ({}) and extension ({}) already match, skipping",
            media.media_type.as_deref().unwrap_or(""),
            actual_extension
        );
        SKIPPED_REPAIRS.lock().unwrap().push(book.id.clone());
        // Kotlin has no early return here: it falls through to the rename, which then fails
        // because the destination is the book itself
    }

    let destination_path = book_path
        .parent()
        .unwrap_or_else(|| Path::new("/"))
        .join(format!(
            "{}.{correct_extension}",
            name_without_extension(&file_name(&book_path))
        ));
    if destination_path.exists() {
        return Err(ConvertError::Plain(format!(
            "Destination file already exists: {}",
            destination_path.display()
        )));
    }

    tracing::info!(
        "Renaming {} to {}",
        book_path.display(),
        destination_path.display()
    );
    std::fs::rename(&book_path, &destination_path)?;

    let scanner = Scanner::new();
    let mut repaired_book = scanner.scan_file(&destination_path).ok_or_else(|| {
        ConvertError::Plain(format!(
            "Repaired book could not be scanned: {}",
            file_name(&destination_path)
        ))
    })?;
    repaired_book.id = book.id.clone();
    repaired_book.series_id = book.series_id.clone();
    repaired_book.library_id = book.library_id.clone();

    BookDao::new(state.db.clone()).update(&repaired_book)?;
    Ok(())
}

/// `BookPageEditor.removeHashedPages`: rebuilds the zip without the duplicate pages.
/// Returns `Some(BookAction::GenerateThumbnail)` when page 1 was removed.
pub fn remove_hashed_pages(
    state: &AppState,
    book: &Book,
    pages_to_delete: &[BookPageNumbered],
) -> ConvertResult<Option<BookAction>> {
    if FAILED_PAGE_REMOVAL.lock().unwrap().contains(&book.id) {
        tracing::info!("Book page removal already failed before, skipping");
        return Ok(None);
    }

    let book_path = book_path(book);
    let scanner = Scanner::new();
    match scanner.scan_file(&book_path) {
        Some(scanned) => {
            if !same_millis(scanned.file_last_modified, book.file_last_modified) {
                tracing::info!(
                    "Book has changed on disk, skipping. Db: {:?}. Scanned: {:?}",
                    book.file_last_modified,
                    scanned.file_last_modified
                );
                return Ok(None);
            }
        }
        None => {
            return Err(ConvertError::Plain(format!(
                "File not found: {}",
                book_path.display()
            )))
        }
    }

    let media = MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| ConvertError::Plain(format!("no media for book {}", book.id)))?;

    if media.media_type.as_deref() != Some(TYPE_ZIP) {
        return Err(ConvertError::Plain(format!(
            "{:?} cannot be converted. Must be one of [\"{TYPE_ZIP}\"]",
            media.media_type
        )));
    }
    if media.status != MediaStatus::Ready {
        return Err(ConvertError::Media(komga_media::MediaError::NotReady));
    }

    let pages_to_keep: Vec<komga_core::model::media::BookPage> = media
        .pages
        .iter()
        .enumerate()
        .filter(|(index, page)| {
            !pages_to_delete.iter().any(|candidate| {
                candidate.file_hash == page.file_hash
                    && candidate.media_type == page.media_type
                    && candidate.file_name == page.file_name
                    && candidate.page_number == (index + 1) as i32
            })
        })
        .map(|(_, page)| page.clone())
        .collect();
    if media.pages.len() != pages_to_keep.len() + pages_to_delete.len() {
        tracing::info!(
            "Should be removing {} pages from book, but count doesn't add up, skipping",
            pages_to_delete.len()
        );
        return Ok(None);
    }

    tracing::info!(
        "Start removal of {} pages for book: {book:?}",
        pages_to_delete.len()
    );

    let temp_file = book_path
        .parent()
        .unwrap_or_else(|| Path::new("/"))
        .join(format!(
            "komga_page_removal_{}.tmp",
            komga_core::tsid::TsidFactory::new_random_node().create_string()
        ));
    let write_result = (|| -> ConvertResult<()> {
        let file = std::fs::File::create(&temp_file)?;
        let mut zip = crate::zip_archive::ZipWriter::new(file);
        let entries: Vec<String> = pages_to_keep
            .iter()
            .map(|p| p.file_name.clone())
            .chain(media.files.iter().map(|f| f.file_name.clone()))
            .collect();
        for entry in &entries {
            let bytes = container::get_file_content(&book_path, &media, entry)?;
            zip.add_entry(entry, std::io::Cursor::new(bytes))?;
        }
        zip.finish()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&temp_file);
        return Err(e);
    }

    let mut created_book = scanner.scan_file(&temp_file).ok_or_else(|| {
        ConvertError::Plain(format!(
            "Newly created book could not be scanned: {}",
            temp_file.display()
        ))
    })?;
    created_book.id = book.id.clone();
    created_book.series_id = book.series_id.clone();
    created_book.library_id = book.library_id.clone();

    let analyzer = Analyzer::new(
        state.config.page_hashing,
        state.settings.get().thumbnail_size.max_edge(),
        state.config.epub_divina_letter_count_threshold,
        state
            .kepub
            .kepubify_path(&state.settings.get(), &state.config),
    );
    let library = LibraryDao::new(state.db.clone())
        .find_by_id(&book.library_id)?
        .ok_or_else(|| ConvertError::Plain(format!("no library {}", book.library_id)))?;
    let mut created_media = analyzer
        .analyze(&temp_file, library.analyze_dimensions)
        .media;
    created_media.book_id = created_book.id.clone();

    let removal_failure = |message: &str| -> ConvertError {
        let _ = std::fs::remove_file(&temp_file);
        FAILED_PAGE_REMOVAL.lock().unwrap().push(book.id.clone());
        ConvertError::Plain(message.to_string())
    };
    if created_media.status != MediaStatus::Ready {
        return Err(removal_failure(
            "Created file could not be analyzed, aborting page removal",
        ));
    }
    if created_media.media_type.as_deref() != Some(TYPE_ZIP) {
        return Err(removal_failure(
            "Created file is not a zip file, aborting page removal",
        ));
    }
    let created_pages: Vec<(String, String)> = created_media
        .pages
        .iter()
        .map(|p| (base_name(&p.file_name), p.media_type.clone()))
        .collect();
    let keep_pages: Vec<(String, String)> = pages_to_keep
        .iter()
        .map(|p| (base_name(&p.file_name), p.media_type.clone()))
        .collect();
    if !keep_pages.iter().all(|p| created_pages.contains(p)) {
        return Err(removal_failure(
            "Created file does not contain all pages to keep from existing file, aborting conversion",
        ));
    }
    let created_files: Vec<String> = created_media
        .files
        .iter()
        .map(|f| base_name(&f.file_name))
        .collect();
    let original_files: Vec<String> = media
        .files
        .iter()
        .map(|f| base_name(&f.file_name))
        .collect();
    if !original_files.iter().all(|f| created_files.contains(f)) {
        return Err(removal_failure(
            "Created file does not contain all files from existing file, aborting page removal",
        ));
    }

    std::fs::rename(&temp_file, &book_path)?;
    let mut new_book = scanner.scan_file(&book_path).ok_or_else(|| {
        ConvertError::Plain(format!(
            "Newly created book could not be scanned after replacing existing one: {}",
            book_path.display()
        ))
    })?;
    new_book.id = book.id.clone();
    new_book.series_id = book.series_id.clone();
    new_book.library_id = book.library_id.clone();

    let media_with_hashes = Media {
        pages: restore_hash_from(&created_media.pages, &media.pages),
        ..created_media
    };
    BookDao::new(state.db.clone()).update(&new_book)?;
    MediaDao::new(state.db.clone()).update(&media_with_hashes)?;

    let page_hash_dao = PageHashDao::new(state.db.clone());
    for page in pages_to_delete {
        if let Some(known) = page_hash_dao.find_known(&page.file_hash)? {
            let mut updated = known;
            updated.delete_count += 1;
            page_hash_dao.update(&updated)?;
        }
    }

    for page in pages_to_delete {
        insert_history(
            state,
            HistoricalEventType::DuplicatePageDeleted,
            Some(&book.id),
            Some(&book.series_id),
            [
                ("name", book_path.display().to_string()),
                ("page", page.file_name.clone()),
                ("hash", page.file_hash.clone()),
            ],
        )?;
    }
    let _ = state.events.send(DomainEvent::BookUpdated(new_book));

    Ok(if pages_to_delete.iter().any(|p| p.page_number == 1) {
        Some(BookAction::GenerateThumbnail)
    } else {
        None
    })
}

/// `PageHashLifecycle.getBookPagesToDeleteAutomatically`: every page whose hash is known with a
/// DELETE_AUTO action, grouped by book id.
pub fn get_book_pages_to_delete_automatically(
    state: &AppState,
    library: &Library,
) -> komga_db::Result<BTreeMap<String, Vec<BookPageNumbered>>> {
    let conn = state.db.ro();
    let mut stmt = conn.prepare(
        "SELECT p.BOOK_ID, p.FILE_NAME, p.NUMBER, p.FILE_HASH, p.MEDIA_TYPE, p.FILE_SIZE \
         FROM MEDIA_PAGE p INNER JOIN PAGE_HASH ph ON p.FILE_HASH = ph.HASH \
         INNER JOIN BOOK b ON b.ID = p.BOOK_ID \
         WHERE ph.ACTION = 'DELETE_AUTO' AND b.LIBRARY_ID = ?",
    )?;
    let rows = stmt
        .query_map([&library.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                BookPageNumbered {
                    file_name: row.get(1)?,
                    page_number: row.get::<_, i32>(2)? + 1,
                    file_hash: row.get(3)?,
                    media_type: row.get(4)?,
                    file_size: row.get(5)?,
                    width: None,
                    height: None,
                },
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut map: BTreeMap<String, Vec<BookPageNumbered>> = BTreeMap::new();
    for (book_id, page) in rows {
        map.entry(book_id).or_default().push(page);
    }
    Ok(map)
}

/// `restoreHashFrom`: carry hashes onto pages that match on (fileSize, mediaType, fileName).
fn restore_hash_from(
    new_pages: &[komga_core::model::media::BookPage],
    restore_from: &[komga_core::model::media::BookPage],
) -> Vec<komga_core::model::media::BookPage> {
    new_pages
        .iter()
        .map(|new_page| {
            restore_from
                .iter()
                .find(|old| {
                    old.file_size == new_page.file_size
                        && old.media_type == new_page.media_type
                        && old.file_name == new_page.file_name
                        && !old.file_hash.is_empty()
                })
                .map(|old| komga_core::model::media::BookPage {
                    file_hash: old.file_hash.clone(),
                    ..new_page.clone()
                })
                .unwrap_or_else(|| new_page.clone())
        })
        .collect()
}

/// `notEquals`: millisecond-truncated instant comparison
fn same_millis(a: time::OffsetDateTime, b: time::OffsetDateTime) -> bool {
    time_codec::truncate_to_millis(a) == time_codec::truncate_to_millis(b)
}

fn book_path(book: &Book) -> PathBuf {
    PathBuf::from(komga_core::dto::url_to_file_path(&book.url))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn name_without_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(idx) => name[..idx].to_string(),
        None => name.to_string(),
    }
}

/// commons-io `FilenameUtils.getName`: last path segment
fn base_name(name: &str) -> String {
    name.rsplit(['/', '\\']).next().unwrap_or(name).to_string()
}

fn insert_history(
    state: &AppState,
    type_: HistoricalEventType,
    book_id: Option<&str>,
    series_id: Option<&str>,
    properties: impl IntoIterator<Item = (&'static str, String)>,
) -> Result<(), ConvertError> {
    HistoricalEventDao::new(state.db.clone()).insert(&HistoricalEvent {
        id: String::new(),
        type_,
        book_id: book_id.map(str::to_string),
        series_id: series_id.map(str::to_string),
        properties: properties
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        timestamp: time_codec::now_utc(),
    })?;
    Ok(())
}

fn book_columns() -> &'static str {
    "BOOK.ID, BOOK.NAME, BOOK.URL, BOOK.FILE_LAST_MODIFIED, BOOK.SERIES_ID, BOOK.LIBRARY_ID, \
     BOOK.FILE_SIZE, BOOK.NUMBER, BOOK.FILE_HASH, BOOK.FILE_HASH_KOREADER, BOOK.DELETED_DATE, \
     BOOK.ONESHOT, BOOK.CREATED_DATE, BOOK.LAST_MODIFIED_DATE"
}

fn row_to_book(row: &rusqlite::Row<'_>) -> rusqlite::Result<Book> {
    use komga_db::dao::{get_datetime, get_datetime_opt};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::book as book_service;
    use crate::service::series::tests as series_tests;
    use komga_core::model::library::{ScanInterval, SeriesCover};
    use komga_core::model::page_hash::{PageHashAction, PageHashKnown};
    use komga_core::model::series::Series;
    use komga_db::dao::series::SeriesDao;
    use std::io::Write;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources")
    }

    fn make_library(id: &str, root: &str, convert: bool, repair: bool) -> Library {
        let now = time_codec::now_utc();
        Library {
            id: id.into(),
            name: "L".into(),
            root: root.into(),
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
            scan_directory_exclusions: vec![],
            repair_extensions: repair,
            convert_to_cbz: convert,
            empty_trash_after_scan: false,
            series_cover: SeriesCover::First,
            hash_files: true,
            hash_pages: true,
            hash_koreader: true,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now,
            last_modified_date: now,
        }
    }

    struct Env {
        state: AppState,
        library: Library,
        series: Series,
        series_path: PathBuf,
        _tmp: tempfile::TempDir,
    }

    fn setup(name: &str, convert: bool, repair: bool) -> Env {
        let tmp = tempfile::TempDir::new().unwrap();
        let series_path = tmp.path().join(name);
        std::fs::create_dir_all(&series_path).unwrap();

        let state = series_tests::test_state();
        let library = make_library(
            "lib1",
            &format!("file:{}/", tmp.path().display()),
            convert,
            repair,
        );
        LibraryDao::new(state.db.clone()).insert(&library).unwrap();
        let series = Series {
            id: "s1".into(),
            name: "series".into(),
            url: format!("file:{}/", series_path.display()),
            file_last_modified: time_codec::now_utc(),
            library_id: "lib1".into(),
            book_count: 0,
            deleted_date: None,
            oneshot: false,
            created_date: time_codec::now_utc(),
            last_modified_date: time_codec::now_utc(),
        };
        SeriesDao::new(state.db.clone()).insert(&series).unwrap();
        Env {
            state,
            library,
            series,
            series_path,
            _tmp: tmp,
        }
    }

    fn seed_analyzed_book(env: &Env, file: &Path, media_type: &str) -> Book {
        let scanned = Scanner::new().scan_file(file).unwrap();
        let mut book = scanned;
        book.library_id = "lib1".into();
        let added = crate::service::series::add_books(&env.state, &env.series, &[book]).unwrap();
        let book = added.into_iter().next().unwrap();
        let analysis = Analyzer::new(3, 300, 15, None).analyze(file, true);
        let mut media = analysis.media;
        media.book_id = book.id.clone();
        media.status = MediaStatus::Ready;
        media.media_type = Some(media_type.to_string());
        MediaDao::new(env.state.db.clone()).update(&media).unwrap();
        // re-read for the audit timestamps persisted by the DAO
        let mut book = BookDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        book.library_id = "lib1".into();
        book
    }

    #[test]
    fn convertible_books_only_rar_when_enabled() {
        let env = setup("convertible", true, false);
        let rar = env.series_path.join("a.cbr");
        std::fs::copy(fixtures().join("archives/rar4.rar"), &rar).unwrap();
        let zip = env.series_path.join("b.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &zip).unwrap();
        seed_analyzed_book(&env, &rar, TYPE_RAR_4);
        seed_analyzed_book(&env, &zip, TYPE_ZIP);

        let books = get_convertible_books(&env.state, &env.library).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].name, "a");

        // disabled library yields nothing
        let mut lib = env.library.clone();
        lib.convert_to_cbz = false;
        assert!(get_convertible_books(&env.state, &lib).unwrap().is_empty());
    }

    #[test]
    fn mismatched_extension_books() {
        let env = setup("mismatched", false, true);
        let wrong = env.series_path.join("wrong.cbz");
        std::fs::copy(fixtures().join("archives/rar4.rar"), &wrong).unwrap();
        let right = env.series_path.join("right.cbr");
        std::fs::copy(fixtures().join("archives/rar4.rar"), &right).unwrap();
        seed_analyzed_book(&env, &wrong, TYPE_RAR_4);
        seed_analyzed_book(&env, &right, TYPE_RAR_4);

        let books = get_mismatched_extension_books(&env.state, &env.library).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].name, "wrong");
    }

    #[test]
    fn convert_rar_to_cbz() {
        let env = setup("convert", true, false);
        let rar = env.series_path.join("v01.cbr");
        std::fs::copy(fixtures().join("archives/rar4.rar"), &rar).unwrap();
        let book = seed_analyzed_book(&env, &rar, TYPE_RAR_4);
        let mut rx = env.state.events.subscribe();

        convert_to_cbz(&env.state, &book).unwrap();

        let cbz = env.series_path.join("v01.cbz");
        assert!(cbz.exists());
        assert!(!rar.exists(), "old rar file is deleted");

        // media is READY zip with all pages
        let media = MediaDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        assert_eq!(media.status, MediaStatus::Ready);
        assert_eq!(media.media_type.as_deref(), Some(TYPE_ZIP));
        assert_eq!(media.page_count, 3);

        // the new zip is openable and contains the same pages
        let names = {
            let f = std::fs::File::open(&cbz).unwrap();
            let mut archive = zip::ZipArchive::new(f).unwrap();
            (0..archive.len())
                .map(|i| archive.by_index(i).unwrap().name().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names, vec!["komga-1.png", "komga-2.png", "komga-3.png"]);

        // BookConverted history row
        let count: i64 = env
            .state
            .db
            .ro()
            .query_row(
                "SELECT COUNT(*) FROM HISTORICAL_EVENT WHERE TYPE = 'BookConverted'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        // and BookFileDeleted for the old rar
        let count: i64 = env
            .state
            .db
            .ro()
            .query_row(
                "SELECT COUNT(*) FROM HISTORICAL_EVENT WHERE TYPE = 'BookFileDeleted'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // BookUpdated event
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DomainEvent::BookUpdated(_)));

        // idempotent second run against the converted book: media type is now zip
        let converted = BookDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        let err = convert_to_cbz(&env.state, &converted).unwrap_err();
        assert!(err.to_string().contains("cannot be converted"));
    }

    #[test]
    fn convert_skips_when_changed_on_disk() {
        let env = setup("changed", true, false);
        let rar = env.series_path.join("v01.cbr");
        std::fs::copy(fixtures().join("archives/rar4.rar"), &rar).unwrap();
        let mut book = seed_analyzed_book(&env, &rar, TYPE_RAR_4);
        // simulate a stale DB record: file newer than book.fileLastModified
        book.file_last_modified -= time::Duration::days(1);

        convert_to_cbz(&env.state, &book).unwrap();
        assert!(rar.exists(), "conversion skipped, old file kept");
        assert!(!env.series_path.join("v01.cbz").exists());
    }

    #[test]
    fn convert_rejects_zip_books() {
        let env = setup("zipreject", true, false);
        let zip = env.series_path.join("v01.cbz");
        std::fs::copy(fixtures().join("archives/zip.zip"), &zip).unwrap();
        let book = seed_analyzed_book(&env, &zip, TYPE_ZIP);

        let err = convert_to_cbz(&env.state, &book).unwrap_err();
        assert!(err.to_string().contains("cannot be converted"));
    }

    #[test]
    fn repair_extension_renames_rar_content() {
        let env = setup("repair", false, true);
        let wrong = env.series_path.join("wrong.cbz");
        std::fs::copy(fixtures().join("archives/rar4.rar"), &wrong).unwrap();
        let book = seed_analyzed_book(&env, &wrong, TYPE_RAR_4);

        repair_extension(&env.state, &book).unwrap();

        let renamed = env.series_path.join("wrong.cbr");
        assert!(renamed.exists());
        assert!(!wrong.exists());
        let updated = BookDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        assert!(updated.url.ends_with("wrong.cbr"));
    }

    #[test]
    fn repair_extension_skips_epub_detected_as_zip() {
        let env = setup("repair-epub", false, true);
        let epub = env.series_path.join("book.epub");
        std::fs::copy(fixtures().join("archives/epub3.epub"), &epub).unwrap();
        // force the media type to plain zip (mis-detected epub)
        let book = seed_analyzed_book(&env, &epub, TYPE_ZIP);

        repair_extension(&env.state, &book).unwrap();
        assert!(epub.exists(), "epub named files are not repaired");
    }

    fn seed_zip_book_with_duplicate_page(env: &Env, name: &str) -> Book {
        // two identical pages (1.png == 2.png on disk) plus a distinct jpeg
        let png = std::fs::read(fixtures().join("hashpage/tg.png/1.png")).unwrap();
        let jpg = std::fs::read(fixtures().join("hashpage/e-sou.jpeg/1.jpg")).unwrap();
        let file = env.series_path.join(name);
        {
            let f = std::fs::File::create(&file).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let options = zip::write::FileOptions::<()>::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("p1.png", options).unwrap();
            zip.write_all(&png).unwrap();
            zip.start_file("p2.png", options).unwrap();
            zip.write_all(&png).unwrap();
            zip.start_file("p3.jpg", options).unwrap();
            zip.write_all(&jpg).unwrap();
            zip.finish().unwrap();
        }
        let book = seed_analyzed_book(env, &file, TYPE_ZIP);
        // hash pages so that hashes are present on the pages
        book_service::hash_pages_and_persist(&env.state, &book).unwrap();
        book
    }

    #[test]
    fn remove_hashed_pages_rebuilds_zip() {
        let env = setup("dedup", false, false);
        let book = seed_zip_book_with_duplicate_page(&env, "dup.cbz");
        let media = MediaDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        assert_eq!(media.page_count, 3);

        // delete the second page (same content as the first)
        let page_to_delete = BookPageNumbered {
            file_name: "p2.png".into(),
            media_type: "image/png".into(),
            width: None,
            height: None,
            file_hash: media.pages[1].file_hash.clone(),
            file_size: media.pages[1].file_size,
            page_number: 2,
        };
        // register the hash as DELETE_AUTO so deleteCount is bumped
        PageHashDao::new(env.state.db.clone())
            .insert(
                &PageHashKnown {
                    hash: media.pages[1].file_hash.clone(),
                    size: None,
                    action: PageHashAction::DeleteAuto,
                    delete_count: 0,
                    match_count: 0,
                    created_date: time_codec::now_utc(),
                    last_modified_date: time_codec::now_utc(),
                },
                None,
            )
            .unwrap();
        let mut rx = env.state.events.subscribe();

        let action =
            remove_hashed_pages(&env.state, &book, std::slice::from_ref(&page_to_delete)).unwrap();
        assert!(
            action.is_none(),
            "page 1 was not removed, no thumbnail regeneration"
        );

        let media = MediaDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        assert_eq!(media.page_count, 2);
        assert_eq!(media.pages[0].file_name, "p1.png");
        assert_eq!(media.pages[1].file_name, "p3.jpg");
        // hashes restored from the previous pages
        assert!(!media.pages[0].file_hash.is_empty());
        // delete count bumped
        let known = PageHashDao::new(env.state.db.clone())
            .find_known(&page_to_delete.file_hash)
            .unwrap()
            .unwrap();
        assert_eq!(known.delete_count, 1);
        // history + event
        let count: i64 = env
            .state
            .db
            .ro()
            .query_row(
                "SELECT COUNT(*) FROM HISTORICAL_EVENT WHERE TYPE = 'DuplicatePageDeleted'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DomainEvent::BookUpdated(_)));
    }

    #[test]
    fn remove_page_one_requests_thumbnail() {
        let env = setup("dedup-first", false, false);
        let book = seed_zip_book_with_duplicate_page(&env, "dup1.cbz");
        let media = MediaDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();

        let page_to_delete = BookPageNumbered {
            file_name: "p1.png".into(),
            media_type: "image/png".into(),
            width: None,
            height: None,
            file_hash: media.pages[0].file_hash.clone(),
            file_size: media.pages[0].file_size,
            page_number: 1,
        };
        let action =
            remove_hashed_pages(&env.state, &book, std::slice::from_ref(&page_to_delete)).unwrap();
        assert!(matches!(action, Some(BookAction::GenerateThumbnail)));
    }

    #[test]
    fn delete_automatically_groups_by_book() {
        let env = setup("dedup-auto", false, false);
        let book = seed_zip_book_with_duplicate_page(&env, "auto.cbz");
        let media = MediaDao::new(env.state.db.clone())
            .find_by_id(&book.id)
            .unwrap()
            .unwrap();
        PageHashDao::new(env.state.db.clone())
            .insert(
                &PageHashKnown {
                    hash: media.pages[0].file_hash.clone(),
                    size: None,
                    action: PageHashAction::DeleteAuto,
                    delete_count: 0,
                    match_count: 0,
                    created_date: time_codec::now_utc(),
                    last_modified_date: time_codec::now_utc(),
                },
                None,
            )
            .unwrap();

        let map = get_book_pages_to_delete_automatically(&env.state, &env.library).unwrap();
        assert_eq!(map.len(), 1);
        let pages = &map[&book.id];
        // both pages sharing the hash are candidates
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].page_number, 1);
        assert_eq!(pages[1].page_number, 2);
    }
}
