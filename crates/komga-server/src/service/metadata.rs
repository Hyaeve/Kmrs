//! `BookMetadataLifecycle.kt` / `SeriesMetadataLifecycle.kt` / `LocalArtworkLifecycle.kt` /
//! `OneShotSeriesProvider.kt`: metadata refresh, aggregation, and local artwork import.
//!
//! Provider registry order mirrors Spring's `List<Provider>` injection without `@Order`
//! annotations: classpath scanning order, i.e. FQN alphabetical
//! (`barcode.IsbnBarcodeProvider` → `comicrack.ComicInfoProvider` → `epub.EpubMetadataProvider`).
//! Later patches override earlier ones for unlocked fields.

use crate::events::DomainEvent;
use crate::service::book::MarkSelectedPreference;
use crate::service::{book, collection, readlist, series as series_service};
use crate::state::AppState;
use komga_core::model::book::{Book, BookMetadata};
use komga_core::model::library::Library;
use komga_core::model::series::Series;
use komga_core::model::thumbnail::{ThumbnailBook, ThumbnailSeries, ThumbnailType};
use komga_core::task::BookMetadataPatchCapability;
use komga_db::dao::book::{BookDao, BookMetadataDao};
use komga_db::dao::library::LibraryDao;
use komga_db::dao::media::MediaDao;
use komga_db::dao::series::{BookMetadataAggregationDao, SeriesMetadataDao};
use komga_db::Result;
use komga_media::metadata::artwork;
use komga_media::metadata::barcode::IsbnBarcodeProvider;
use komga_media::metadata::comicinfo::ComicInfoProvider;
use komga_media::metadata::epub::EpubMetadataProvider;
use komga_media::metadata::mylar::{compute_one_shot_patch, MylarSeriesProvider};
use komga_media::metadata::patch::{
    aggregate as aggregate_parts, apply_book_patch, apply_series_patch, most_frequent,
    AggregationParts, BookMetadataPatch, BookMetadataProvider, MetadataPatchTarget,
    MetadataProvider, SeriesMetadataFromBookProvider, SeriesMetadataPatch, SeriesMetadataProvider,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn library_of(state: &AppState, library_id: &str) -> Result<Library> {
    LibraryDao::new(state.db.clone())
        .find_by_id(library_id)?
        .ok_or_else(|| komga_db::Error::EnumValue(format!("no library {library_id}")))
}

fn book_path(book: &Book) -> PathBuf {
    PathBuf::from(komga_core::dto::url_to_file_path(&book.url))
}

fn series_path(series: &Series) -> PathBuf {
    PathBuf::from(komga_core::dto::url_to_file_path(&series.url))
}

/// `BookMetadataLifecycle.refreshMetadata`.
pub fn refresh_book_metadata(
    state: &AppState,
    book: &Book,
    capabilities: &BTreeSet<BookMetadataPatchCapability>,
) -> Result<()> {
    tracing::info!("Refresh metadata for book: {book:?} with capabilities: {capabilities:?}");
    let media = MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| komga_db::Error::EnumValue(format!("no media for book {}", book.id)))?;
    let library = library_of(state, &book.library_id)?;
    let mut changed = false;

    let providers: Vec<(&str, Box<dyn BookMetadataProvider>)> = vec![
        ("IsbnBarcodeProvider", Box::new(IsbnBarcodeProvider::new())),
        ("ComicInfoProvider", Box::new(ComicInfoProvider)),
        ("EpubMetadataProvider", Box::new(EpubMetadataProvider)),
    ];
    for (name, provider) in &providers {
        if capabilities
            .intersection(provider.capabilities())
            .next()
            .is_none()
        {
            tracing::info!("Provider does not support requested capabilities, skipping: {name}");
            continue;
        }
        if !(provider.should_library_handle_patch(&library, MetadataPatchTarget::Book)
            || provider.should_library_handle_patch(&library, MetadataPatchTarget::ReadList))
        {
            tracing::info!(
                "Library is not set to import book or read lists metadata for this provider, skipping: {name}"
            );
            continue;
        }

        let patch = provider.get_book_metadata_from_book(&book_path(book), &media);

        if provider.should_library_handle_patch(&library, MetadataPatchTarget::Book) {
            handle_patch_for_book_metadata(state, patch.as_ref(), book)?;
            changed = true;
        }
        if provider.should_library_handle_patch(&library, MetadataPatchTarget::ReadList) {
            if let Some(patch) = &patch {
                for entry in &patch.read_lists {
                    readlist::add_book_to_read_list(state, &entry.name, book, entry.number)?;
                }
            }
        }
    }

    if changed {
        let _ = state.events.send(DomainEvent::BookUpdated(book.clone()));
    }
    Ok(())
}

fn handle_patch_for_book_metadata(
    state: &AppState,
    patch: Option<&BookMetadataPatch>,
    book: &Book,
) -> Result<()> {
    let Some(patch) = patch else { return Ok(()) };
    let dao = BookMetadataDao::new(state.db.clone());
    let Some(existing) = dao.find_by_id(&book.id)? else {
        return Ok(());
    };
    let patched = apply_book_patch(patch, &existing);
    dao.update(&patched)?;
    Ok(())
}

/// `SeriesMetadataLifecycle.refreshMetadata`.
pub fn refresh_series_metadata(state: &AppState, series: &Series) -> Result<()> {
    tracing::info!("Refresh metadata for series: {series:?}");
    let library = library_of(state, &series.library_id)?;
    let mut changed = false;

    let from_book_providers: Vec<(&str, Box<dyn SeriesMetadataFromBookProvider>)> = vec![
        ("ComicInfoProvider", Box::new(ComicInfoProvider)),
        ("EpubMetadataProvider", Box::new(EpubMetadataProvider)),
    ];
    for (name, provider) in &from_book_providers {
        if !(provider.should_library_handle_patch(&library, MetadataPatchTarget::Series)
            || provider.should_library_handle_patch(&library, MetadataPatchTarget::Collection))
        {
            tracing::info!(
                "Library is not set to import series or collection metadata for this provider, skipping: {name}"
            );
            continue;
        }

        let books = BookDao::new(state.db.clone()).find_by_series_id(&series.id)?;
        let mut patches: Vec<SeriesMetadataPatch> = vec![];
        for book in &books {
            let media = MediaDao::new(state.db.clone())
                .find_by_id(&book.id)?
                .ok_or_else(|| {
                    komga_db::Error::EnumValue(format!("no media for book {}", book.id))
                })?;
            if let Some(patch) = provider.get_series_metadata_from_book(
                &book_path(book),
                &media,
                library.import_comicinfo_series_append_volume,
            ) {
                patches.push(patch);
            }
        }

        let collection_names: BTreeSet<String> = patches
            .iter()
            .flat_map(|p| p.collections.iter().cloned())
            .collect();
        if provider.should_library_handle_patch(&library, MetadataPatchTarget::Series) {
            handle_patch_for_series_metadata(state, patches, series)?;
            changed = true;
        }
        if provider.should_library_handle_patch(&library, MetadataPatchTarget::Collection) {
            for name in collection_names {
                collection::add_series_to_collection(state, &name, series)?;
            }
        }
    }

    if MylarSeriesProvider.should_library_handle_patch(&library, MetadataPatchTarget::Series) {
        if let Some(patch) =
            MylarSeriesProvider.get_series_metadata(&series_path(series), series.oneshot)
        {
            let dao = SeriesMetadataDao::new(state.db.clone());
            if let Some(existing) = dao.find_by_id(&series.id)? {
                dao.update(&apply_series_patch(&patch, &existing))?;
            }
            changed = true;
        }
    }

    if let Some(patch) = one_shot_patch_for(state, series)? {
        let dao = SeriesMetadataDao::new(state.db.clone());
        if let Some(existing) = dao.find_by_id(&series.id)? {
            dao.update(&apply_series_patch(&patch, &existing))?;
        }
        changed = true;
    }

    if changed {
        let _ = state
            .events
            .send(DomainEvent::SeriesUpdated(series.clone()));
    }
    Ok(())
}

fn handle_patch_for_series_metadata(
    state: &AppState,
    patches: Vec<SeriesMetadataPatch>,
    series: &Series,
) -> Result<()> {
    let genre_union: BTreeSet<String> = patches
        .iter()
        .filter_map(|p| p.genres.clone())
        .flatten()
        .collect();
    let aggregated = SeriesMetadataPatch {
        title: most_frequent(&patches, |p| p.title.clone()),
        title_sort: most_frequent(&patches, |p| p.title_sort.clone()),
        status: most_frequent(&patches, |p| p.status),
        // Kotlin's `ifEmpty { null }`: an empty union never overrides
        genres: if genre_union.is_empty() {
            None
        } else {
            Some(genre_union)
        },
        language: most_frequent(&patches, |p| p.language.clone()),
        summary: None,
        reading_direction: most_frequent(&patches, |p| p.reading_direction),
        age_rating: patches.iter().filter_map(|p| p.age_rating).max(),
        publisher: most_frequent(&patches, |p| p.publisher.clone()),
        total_book_count: patches.iter().filter_map(|p| p.total_book_count).max(),
        collections: BTreeSet::new(),
    };

    let dao = SeriesMetadataDao::new(state.db.clone());
    if let Some(existing) = dao.find_by_id(&series.id)? {
        dao.update(&apply_series_patch(&aggregated, &existing))?;
    }
    Ok(())
}

/// `OneShotSeriesProvider`: oneshot series take the single book's metadata (always enabled
/// for the SERIES target).
fn one_shot_patch_for(state: &AppState, series: &Series) -> Result<Option<SeriesMetadataPatch>> {
    if !series.oneshot {
        return Ok(None);
    }
    let book_ids = BookDao::new(state.db.clone()).find_all_ids_by_series_id(&series.id)?;
    let Some(first) = book_ids.first() else {
        return Ok(None);
    };
    let Some(metadata) = BookMetadataDao::new(state.db.clone()).find_by_id(first)? else {
        return Ok(None);
    };
    Ok(Some(compute_one_shot_patch(
        metadata.title,
        metadata.summary,
    )))
}

/// `SeriesMetadataLifecycle.aggregateMetadata`.
pub fn aggregate_series_metadata(state: &AppState, series: &Series) -> Result<()> {
    tracing::info!("Aggregate book metadata for series: {series:?}");
    let book_ids = BookDao::new(state.db.clone()).find_all_ids_by_series_id(&series.id)?;
    let metadata_dao = BookMetadataDao::new(state.db.clone());
    let metadatas: Vec<BookMetadata> = book_ids
        .iter()
        .filter_map(|id| metadata_dao.find_by_id(id).ok().flatten())
        .collect();
    let parts: AggregationParts = aggregate_parts(&metadatas);

    let aggregation_dao = BookMetadataAggregationDao::new(state.db.clone());
    let mut aggregation = aggregation_dao.find_by_id(&series.id)?.unwrap_or_else(|| {
        komga_core::model::series::BookMetadataAggregation {
            series_id: series.id.clone(),
            authors: vec![],
            tags: BTreeSet::new(),
            release_date: None,
            summary: String::new(),
            summary_number: String::new(),
            created_date: komga_core::time_codec::now_utc(),
            last_modified_date: komga_core::time_codec::now_utc(),
        }
    });
    aggregation.series_id = series.id.clone();
    aggregation.authors = parts.authors;
    aggregation.tags = parts.tags;
    aggregation.release_date = parts.release_date;
    aggregation.summary = parts.summary;
    aggregation.summary_number = parts.summary_number;
    if aggregation_dao.find_by_id(&series.id)?.is_some() {
        aggregation_dao.update(&aggregation)?;
    } else {
        aggregation_dao.insert(&aggregation)?;
    }

    let _ = state
        .events
        .send(DomainEvent::SeriesUpdated(series.clone()));
    Ok(())
}

/// `LocalArtworkLifecycle.refreshLocalArtwork` for a book.
pub fn refresh_book_local_artwork(state: &AppState, book: &Book) -> Result<()> {
    tracing::info!("Refresh local artwork for book: {book:?}");
    let library = library_of(state, &book.library_id)?;
    if !library.import_local_artwork {
        tracing::info!("Library is not set to import local artwork, skipping");
        return Ok(());
    }
    for draft in artwork::get_book_thumbnails(&book_path(book)) {
        let mark = if draft.selected {
            MarkSelectedPreference::IfNoneOrGenerated
        } else {
            MarkSelectedPreference::No
        };
        book::add_thumbnail_for_book(
            state,
            ThumbnailBook {
                id: String::new(),
                book_id: book.id.clone(),
                thumbnail: None,
                url: Some(draft.url),
                selected: draft.selected,
                type_: ThumbnailType::Sidecar,
                media_type: draft.media_type,
                file_size: draft.file_size,
                dimension: draft.dimension,
                created_date: komga_core::time_codec::now_utc(),
                last_modified_date: komga_core::time_codec::now_utc(),
            },
            mark,
        )?;
    }
    Ok(())
}

/// `LocalArtworkLifecycle.refreshLocalArtwork` for a series.
pub fn refresh_series_local_artwork(state: &AppState, series: &Series) -> Result<()> {
    tracing::info!("Refresh local artwork for series: {series:?}");
    let library = library_of(state, &series.library_id)?;
    if !library.import_local_artwork {
        tracing::info!("Library is not set to import local artwork, skipping");
        return Ok(());
    }
    for draft in artwork::get_series_thumbnails(&series_path(series), series.oneshot) {
        let mark = if draft.selected {
            MarkSelectedPreference::IfNoneOrGenerated
        } else {
            MarkSelectedPreference::No
        };
        series_service::add_thumbnail_for_series(
            state,
            ThumbnailSeries {
                id: String::new(),
                series_id: series.id.clone(),
                thumbnail: None,
                url: Some(draft.url),
                selected: draft.selected,
                type_: ThumbnailType::Sidecar,
                media_type: draft.media_type,
                file_size: draft.file_size,
                dimension: draft.dimension,
                created_date: komga_core::time_codec::now_utc(),
                last_modified_date: komga_core::time_codec::now_utc(),
            },
            mark,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::series as series_service;
    use komga_core::model::library::{ScanInterval, SeriesCover};
    use komga_core::model::media::{MediaFile, MediaStatus};
    use komga_core::model::series::SeriesMetadata;
    use komga_core::time_codec::{now_utc, parse_date};
    use komga_db::dao::collection::CollectionDao;
    use komga_db::dao::readlist::ReadListDao;
    use komga_db::dao::thumbnail::{ThumbnailBookDao, ThumbnailSeriesDao};
    use komga_db::pool::Database;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn visible_tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kmrs-metadata-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn library(id: &str, root: &Path) -> Library {
        let now = now_utc();
        Library {
            id: id.into(),
            name: "L".into(),
            root: format!("file:{}/", root.display()),
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
            repair_extensions: false,
            convert_to_cbz: false,
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

    fn seed_library(db: &Database, library: &Library) {
        LibraryDao::new(db.clone()).insert(library).unwrap();
    }

    fn png_bytes() -> Vec<u8> {
        use std::io::Read;
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/archives/zip.zip"),
        )
        .map(|zip| {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
            let mut bytes = vec![];
            archive
                .by_name("komga.png")
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        })
        .unwrap()
    }

    fn write_cbz(path: &Path, comic_info_xml: Option<&str>) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("page1.png", options).unwrap();
        writer.write_all(&png_bytes()).unwrap();
        if let Some(xml) = comic_info_xml {
            writer.start_file("ComicInfo.xml", options).unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    /// Creates a series with `count` books on disk (real cbz files with optional ComicInfo.xml),
    /// each with MEDIA/BOOK_METADATA rows and MEDIA_FILE entries for ComicInfo.xml.
    fn seed_series_with_books(
        state: &AppState,
        library_id: &str,
        series_name: &str,
        series_dir: &Path,
        books: &[(&str, Option<&str>)],
    ) -> (Series, Vec<Book>) {
        std::fs::create_dir_all(series_dir).unwrap();
        let series = series_service::create_series(
            state,
            &Series {
                id: String::new(),
                name: series_name.into(),
                url: format!("file:{}/", series_dir.display()),
                file_last_modified: now_utc(),
                library_id: library_id.into(),
                book_count: 0,
                deleted_date: None,
                oneshot: false,
                created_date: now_utc(),
                last_modified_date: now_utc(),
            },
        )
        .unwrap();

        let mut created = vec![];
        for (book_name, xml) in books {
            let book_path = series_dir.join(format!("{book_name}.cbz"));
            write_cbz(&book_path, *xml);
            let new_books = series_service::add_books(
                state,
                &series,
                &[Book {
                    id: String::new(),
                    name: book_name.to_string(),
                    url: format!("file:{}", book_path.display()),
                    file_last_modified: now_utc(),
                    series_id: series.id.clone(),
                    library_id: library_id.into(),
                    file_size: 1,
                    number: 0,
                    file_hash: String::new(),
                    file_hash_koreader: String::new(),
                    deleted_date: None,
                    oneshot: false,
                    created_date: now_utc(),
                    last_modified_date: now_utc(),
                }],
            )
            .unwrap();
            let book = new_books.into_iter().next().unwrap();
            // mark media READY, give it the page that exists in the cbz, and register
            // ComicInfo.xml as a media file
            let has_xml = xml.is_some();
            let media_dao = MediaDao::new(state.db.clone());
            let mut media = media_dao.find_by_id(&book.id).unwrap().unwrap();
            media.status = MediaStatus::Ready;
            media.media_type = Some("application/zip".into());
            media.page_count = 1;
            media.pages.push(komga_core::model::media::BookPage {
                file_name: "page1.png".into(),
                media_type: "image/png".into(),
                width: Some(48),
                height: Some(48),
                file_hash: String::new(),
                file_size: None,
            });
            if has_xml {
                media.files.push(MediaFile {
                    file_name: "ComicInfo.xml".into(),
                    media_type: Some("application/xml".into()),
                    sub_type: None,
                    file_size: None,
                });
            }
            media_dao.update(&media).unwrap();
            created.push(book);
        }
        (series, created)
    }

    fn book_metadata(state: &AppState, book_id: &str) -> BookMetadata {
        BookMetadataDao::new(state.db.clone())
            .find_by_id(book_id)
            .unwrap()
            .unwrap()
    }

    fn series_metadata(state: &AppState, series_id: &str) -> SeriesMetadata {
        SeriesMetadataDao::new(state.db.clone())
            .find_by_id(series_id)
            .unwrap()
            .unwrap()
    }

    const COMIC_INFO_FULL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ComicInfo>
  <Title>Book Title</Title>
  <Series>Alpha Series</Series>
  <Number>3</Number>
  <Count>7</Count>
  <Volume>2</Volume>
  <Summary>Hero summary</Summary>
  <Year>2019</Year>
  <Month>5</Month>
  <Day>12</Day>
  <Writer>Writer One, Writer Two</Writer>
  <Publisher>Pub One</Publisher>
  <Genre>action, fantasy</Genre>
  <Tags>seinen, adventure</Tags>
  <Web>https://example.org/alpha https://example.org/wiki</Web>
  <Manga>YesAndRightToLeft</Manga>
  <AgeRating>MA 15+</AgeRating>
  <LanguageISO>eng</LanguageISO>
  <AlternateSeries>Side Stories</AlternateSeries>
  <AlternateNumber>4</AlternateNumber>
  <StoryArc>Arc A, Arc B</StoryArc>
  <StoryArcNumber>2, 5</StoryArcNumber>
  <SeriesGroup>Alpha Universe</SeriesGroup>
</ComicInfo>"#;

    fn recv_until(
        rx: &mut tokio::sync::broadcast::Receiver<DomainEvent>,
        predicate: impl Fn(&DomainEvent) -> bool,
    ) -> DomainEvent {
        for _ in 0..50 {
            if let Ok(event) = rx.try_recv() {
                if predicate(&event) {
                    return event;
                }
            } else {
                panic!("event not received");
            }
        }
        panic!("event not received within limit");
    }

    #[test]
    fn refresh_book_metadata_applies_patch_locks_readlists_and_event() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("book-refresh");
        let library = library("lib-1", &root);
        seed_library(&state.db, &library);
        let (series, books) = seed_series_with_books(
            &state,
            "lib-1",
            "Alpha",
            &root.join("alpha"),
            &[("v01", Some(COMIC_INFO_FULL))],
        );
        let book = &books[0];

        // lock title and summary: the patch must not override them
        let mut existing = book_metadata(&state, &book.id);
        existing.title_lock = true;
        existing.summary_lock = true;
        let existing_title = existing.title.clone();
        BookMetadataDao::new(state.db.clone())
            .update(&existing)
            .unwrap();

        let mut rx = state.events.subscribe();
        refresh_book_metadata(&state, book, &BookMetadataPatchCapability::all()).unwrap();

        let metadata = book_metadata(&state, &book.id);
        assert_eq!(metadata.title, existing_title);
        assert_eq!(metadata.summary, "");
        assert_eq!(metadata.number, "3");
        assert_eq!(metadata.number_sort, 3.0);
        assert_eq!(metadata.release_date, parse_date("2019-05-12"));
        assert_eq!(
            metadata
                .authors
                .iter()
                .map(|a| (a.name.as_str(), a.role.as_str()))
                .collect::<Vec<_>>(),
            [("Writer One", "writer"), ("Writer Two", "writer")]
        );
        assert_eq!(metadata.tags, ["seinen", "adventure"]);
        assert_eq!(metadata.links.len(), 2);
        assert_eq!(metadata.links[0].label, "example.org");

        // READLIST target: alternateSeries + storyArc entries land in read lists
        let readlist_dao = ReadListDao::new(state.db.clone());
        let side = readlist_dao
            .find_by_name("Side Stories")
            .unwrap()
            .expect("Side Stories read list");
        assert_eq!(side.book_ids.values().collect::<Vec<_>>(), [&book.id]);
        let arc_a = readlist_dao
            .find_by_name("Arc A")
            .unwrap()
            .expect("Arc A read list");
        assert_eq!(
            arc_a
                .book_ids
                .iter()
                .map(|(n, b)| (*n, b.clone()))
                .collect::<Vec<_>>(),
            [(2, book.id.clone())]
        );
        let arc_b = readlist_dao
            .find_by_name("Arc B")
            .unwrap()
            .expect("Arc B read list");
        assert_eq!(
            arc_b
                .book_ids
                .iter()
                .map(|(n, b)| (*n, b.clone()))
                .collect::<Vec<_>>(),
            [(5, book.id.clone())]
        );

        recv_until(&mut rx, |e| matches!(e, DomainEvent::BookUpdated(_)));

        // collection comes from the series refresh, not here
        let _ = series;
    }

    #[test]
    fn refresh_book_metadata_skips_when_library_gates_off() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("book-gates");
        let mut library = library("lib-2", &root);
        library.import_comicinfo_book = false;
        library.import_comicinfo_readlist = false;
        seed_library(&state.db, &library);
        let (_, books) = seed_series_with_books(
            &state,
            "lib-2",
            "Alpha",
            &root.join("alpha"),
            &[("v01", Some(COMIC_INFO_FULL))],
        );
        let book = &books[0];

        refresh_book_metadata(&state, book, &BookMetadataPatchCapability::all()).unwrap();

        let metadata = book_metadata(&state, &book.id);
        assert_eq!(metadata.title, "v01");
        assert!(ReadListDao::new(state.db.clone())
            .find_by_name("Side Stories")
            .unwrap()
            .is_none());
    }

    #[test]
    fn refresh_book_metadata_capabilities_filter() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("book-caps");
        let library = library("lib-3", &root);
        seed_library(&state.db, &library);
        let (_, books) = seed_series_with_books(
            &state,
            "lib-3",
            "Alpha",
            &root.join("alpha"),
            &[("v01", Some(COMIC_INFO_FULL))],
        );
        let book = &books[0];

        // only TITLE: ComicInfo still applies (it has the TITLE capability), but only
        // fields covered by the requested set change? No: the whole patch is applied.
        // The gate is per-provider, not per-field.
        let mut caps = BTreeSet::new();
        caps.insert(BookMetadataPatchCapability::Isbn);
        refresh_book_metadata(&state, book, &caps).unwrap();
        // ComicInfo has no ISBN capability -> skipped; barcode finds nothing; EPUB not applicable
        let metadata = book_metadata(&state, &book.id);
        assert_eq!(metadata.title, "v01");
    }

    #[test]
    fn refresh_series_metadata_aggregates_and_creates_collections() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("series-refresh");
        let library = library("lib-4", &root);
        seed_library(&state.db, &library);

        let comic_a = COMIC_INFO_FULL
            .replace(
                "<Genre>action, fantasy</Genre>",
                "<Genre>action, drama</Genre>",
            )
            .replace(
                "<AgeRating>MA 15+</AgeRating>",
                "<AgeRating>Everyone 10+</AgeRating>",
            );
        let comic_b = COMIC_INFO_FULL
            .replace(
                "<Genre>action, fantasy</Genre>",
                "<Genre>fantasy, seinen</Genre>",
            )
            .replace("<AgeRating>MA 15+</AgeRating>", "<AgeRating>M</AgeRating>");
        let (series, _) = seed_series_with_books(
            &state,
            "lib-4",
            "Alpha",
            &root.join("alpha"),
            &[("v01", Some(&comic_a)), ("v02", Some(&comic_b))],
        );

        let mut rx = state.events.subscribe();
        refresh_series_metadata(&state, &series).unwrap();

        let metadata = series_metadata(&state, &series.id);
        assert_eq!(metadata.title, "Alpha Series (2)");
        assert_eq!(metadata.title_sort, "Alpha Series (2)");
        assert_eq!(
            metadata.genres,
            ["action", "drama", "fantasy", "seinen"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(metadata.age_rating, Some(17));
        assert_eq!(metadata.publisher, "Pub One");
        assert_eq!(
            metadata.reading_direction,
            Some(komga_core::model::series::ReadingDirection::RightToLeft)
        );
        assert_eq!(metadata.language, "en");
        assert_eq!(metadata.total_book_count, Some(7));

        // seriesGroup -> collection with the series as member
        let collection = CollectionDao::new(state.db.clone())
            .find_by_name("Alpha Universe")
            .unwrap()
            .expect("Alpha Universe collection");
        assert_eq!(collection.series_ids.as_slice(), [series.id.as_str()]);

        assert!(matches!(
            recv_until(&mut rx, |e| matches!(e, DomainEvent::SeriesUpdated(_))),
            DomainEvent::SeriesUpdated(_)
        ));
    }

    #[test]
    fn refresh_series_metadata_append_volume_toggle() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("series-append");
        let mut library = library("lib-5", &root);
        library.import_comicinfo_series_append_volume = false;
        seed_library(&state.db, &library);
        let (series, _) = seed_series_with_books(
            &state,
            "lib-5",
            "Alpha",
            &root.join("alpha"),
            &[("v01", Some(COMIC_INFO_FULL))],
        );

        refresh_series_metadata(&state, &series).unwrap();
        assert_eq!(series_metadata(&state, &series.id).title, "Alpha Series");
    }

    #[test]
    fn refresh_series_metadata_mylar_overrides() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("series-mylar");
        let library = library("lib-6", &root);
        seed_library(&state.db, &library);
        let series_dir = root.join("alpha");
        let (series, _) =
            seed_series_with_books(&state, "lib-6", "Alpha", &series_dir, &[("v01", None)]);

        std::fs::write(
            series_dir.join("series.json"),
            r#"{"metadata":{
              "type":"comic",
              "publisher":"Mylar Pub",
              "name":"Mylar Name",
              "comicid":"1",
              "year":2020,
              "description_text":"mylar description",
              "volume":3,
              "booktype":"comic",
              "age_rating":"15+",
              "comic_image":"x",
              "total_issues":42,
              "publication_run":"2019 - 2024",
              "status":"Ended"
            }}"#,
        )
        .unwrap();

        refresh_series_metadata(&state, &series).unwrap();
        let metadata = series_metadata(&state, &series.id);
        assert_eq!(metadata.title, "Mylar Name (2020)");
        assert_eq!(
            metadata.status,
            komga_core::model::series::SeriesStatus::Ended
        );
        assert_eq!(metadata.summary, "mylar description");
        assert_eq!(metadata.publisher, "Mylar Pub");
        assert_eq!(metadata.age_rating, Some(15));
        assert_eq!(metadata.total_book_count, Some(42));
    }

    #[test]
    fn refresh_series_metadata_one_shot() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("series-oneshot");
        let library = library("lib-7", &root);
        seed_library(&state.db, &library);
        let series_dir = root.join("ones");
        let (mut series, _) =
            seed_series_with_books(&state, "lib-7", "ones", &series_dir, &[("v01", None)]);
        // make it a oneshot series and set a custom book title/summary
        series.oneshot = true;
        komga_db::dao::series::SeriesDao::new(state.db.clone())
            .update(&series, false)
            .unwrap();
        let book_ids = BookDao::new(state.db.clone())
            .find_all_ids_by_series_id(&series.id)
            .unwrap();
        let mut book_meta = book_metadata(&state, &book_ids[0]);
        book_meta.title = "The One Shot".into();
        book_meta.summary = "oneshot summary".into();
        BookMetadataDao::new(state.db.clone())
            .update(&book_meta)
            .unwrap();

        refresh_series_metadata(&state, &series).unwrap();
        let metadata = series_metadata(&state, &series.id);
        assert_eq!(metadata.title, "The One Shot");
        assert_eq!(
            metadata.status,
            komga_core::model::series::SeriesStatus::Ended
        );
        assert_eq!(metadata.summary, "oneshot summary");
        assert_eq!(metadata.total_book_count, Some(1));
    }

    #[test]
    fn aggregate_series_metadata_computes_aggregation() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("aggregate");
        let library = library("lib-8", &root);
        seed_library(&state.db, &library);
        let (series, books) = seed_series_with_books(
            &state,
            "lib-8",
            "Alpha",
            &root.join("alpha"),
            &[("v01", None), ("v02", None)],
        );

        let mut m1 = book_metadata(&state, &books[0].id);
        m1.number_sort = 2.0;
        m1.release_date = parse_date("2021-03-01");
        m1.authors = vec![
            komga_core::model::common::Author::new("Alice", "writer"),
            komga_core::model::common::Author::new("Alice", "artist"),
        ];
        m1.tags = vec!["a".into(), "b".into()];
        m1.summary = String::new();
        BookMetadataDao::new(state.db.clone()).update(&m1).unwrap();

        let mut m2 = book_metadata(&state, &books[1].id);
        m2.number_sort = 1.0;
        m2.number = "1".into();
        m2.release_date = parse_date("2020-01-01");
        m2.authors = vec![
            komga_core::model::common::Author::new("Alice", "writer"),
            komga_core::model::common::Author::new("Bob", "writer"),
        ];
        m2.tags = vec!["b".into(), "c".into()];
        m2.summary = "early summary".into();
        BookMetadataDao::new(state.db.clone()).update(&m2).unwrap();

        let mut rx = state.events.subscribe();
        aggregate_series_metadata(&state, &series).unwrap();

        let aggregation = BookMetadataAggregationDao::new(state.db.clone())
            .find_by_id(&series.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            aggregation
                .authors
                .iter()
                .map(|a| (a.name.as_str(), a.role.as_str()))
                .collect::<Vec<_>>(),
            [("Alice", "writer"), ("Alice", "artist"), ("Bob", "writer")]
        );
        assert_eq!(
            aggregation.tags,
            ["a", "b", "c"]
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(aggregation.release_date, parse_date("2020-01-01"));
        assert_eq!(aggregation.summary, "early summary");
        assert_eq!(aggregation.summary_number, "1");
        assert!(matches!(
            recv_until(&mut rx, |e| matches!(e, DomainEvent::SeriesUpdated(_))),
            DomainEvent::SeriesUpdated(_)
        ));
    }

    #[test]
    fn refresh_local_artwork_book_and_series() {
        let state = series_service::tests::test_state();
        let root = visible_tempdir("artwork");
        let library = library("lib-9", &root);
        seed_library(&state.db, &library);
        let series_dir = root.join("alpha");
        let (series, books) =
            seed_series_with_books(&state, "lib-9", "Alpha", &series_dir, &[("v01", None)]);
        std::fs::write(series_dir.join("v01.jpg"), png_bytes()).unwrap();
        std::fs::write(series_dir.join("cover.png"), png_bytes()).unwrap();

        refresh_book_local_artwork(&state, &books[0]).unwrap();
        let book_thumbs = ThumbnailBookDao::new(state.db.clone())
            .find_all_by_book_id(&books[0].id)
            .unwrap();
        assert_eq!(book_thumbs.len(), 1);
        assert_eq!(book_thumbs[0].type_, ThumbnailType::Sidecar);
        assert!(book_thumbs[0].selected);
        assert_eq!(book_thumbs[0].media_type, "image/png");
        assert_eq!(
            (
                book_thumbs[0].dimension.width,
                book_thumbs[0].dimension.height
            ),
            (48, 48)
        );

        refresh_series_local_artwork(&state, &series).unwrap();
        let series_thumbs = ThumbnailSeriesDao::new(state.db.clone())
            .find_all_by_series_id(&series.id)
            .unwrap();
        assert_eq!(series_thumbs.len(), 1);
        assert_eq!(series_thumbs[0].type_, ThumbnailType::Sidecar);
        assert!(series_thumbs[0].selected);
    }

    #[tokio::test]
    async fn processor_refresh_chain_end_to_end() {
        use crate::service::processor::TaskProcessor;
        use komga_db::dao::tasks::TasksDao;

        let state = series_service::tests::test_state();
        let root = visible_tempdir("chain");
        let library = library("lib-10", &root);
        seed_library(&state.db, &library);
        let (series, books) = seed_series_with_books(
            &state,
            "lib-10",
            "Alpha",
            &root.join("alpha"),
            &[("v01", Some(COMIC_INFO_FULL))],
        );
        let book = &books[0];

        let notify: crate::service::TaskNotify = std::sync::Arc::new(tokio::sync::Notify::new());
        let emitter = crate::service::TaskEmitter::new(
            state.db.clone(),
            state.tasks_db.clone(),
            notify.clone(),
        );
        let handle = TaskProcessor::start(state.clone(), notify);
        emitter
            .refresh_book_metadata(
                book,
                BookMetadataPatchCapability::all(),
                komga_core::task::DEFAULT_PRIORITY,
            )
            .unwrap();

        for _ in 0..400 {
            if TasksDao::new(state.tasks_db.clone()).count().unwrap_or(1) == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        handle.abort();

        assert_eq!(TasksDao::new(state.tasks_db.clone()).count().unwrap(), 0);
        // book patch applied (title from ComicInfo), series aggregated, collection created
        assert_eq!(book_metadata(&state, &book.id).number, "3");
        let metadata = series_metadata(&state, &series.id);
        assert_eq!(metadata.title, "Alpha Series (2)");
        assert_eq!(metadata.total_book_count, Some(7));
        assert!(CollectionDao::new(state.db.clone())
            .find_by_name("Alpha Universe")
            .unwrap()
            .is_some());
    }
}
