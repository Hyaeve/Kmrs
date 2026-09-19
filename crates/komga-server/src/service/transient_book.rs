//! `TransientBookLifecycle.kt` and `TransientBookCache.kt`: transient (uploaded but not yet
//! imported) books, kept in an in-memory cache with 1h expiry.

use crate::state::AppState;
use komga_core::model::book::Book;
use komga_core::model::media::Media;
use komga_core::search::{SearchConditionSeries, SearchContext};
use komga_db::dao::library::LibraryDao;
use komga_db::dao::series::SeriesDao;
use komga_media::analyzer::Analyzer;
use komga_media::metadata::comicinfo::ComicInfoProvider;
use komga_media::metadata::epub::EpubMetadataProvider;
use komga_media::metadata::patch::{BookMetadataProvider, SeriesMetadataFromBookProvider};
use komga_media::scanner::{ScanOptions, Scanner};
use komga_media::PageContent;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// `TransientBook.kt`
#[derive(Debug, Clone)]
pub struct TransientBook {
    pub book: Book,
    pub media: Media,
    pub metadata: TransientBookMetadata,
}

#[derive(Debug, Clone, Default)]
pub struct TransientBookMetadata {
    pub number: Option<f32>,
    pub series_id: Option<String>,
}

/// Caffeine `expireAfterAccess(1h)`: the whole point of the cache is that devices re-fetch
/// within the hour, so scanned books need not outlive it.
fn cache() -> &'static moka::sync::Cache<String, TransientBook> {
    static CACHE: OnceLock<moka::sync::Cache<String, TransientBook>> = OnceLock::new();
    CACHE.get_or_init(|| {
        moka::sync::Cache::builder()
            .time_to_idle(std::time::Duration::from_secs(3600))
            .build()
    })
}

pub fn find_by_id(id: &str) -> Option<TransientBook> {
    cache().get(id)
}

pub fn save(book: TransientBook) {
    cache().insert(book.book.id.clone(), book);
}

pub fn save_many(books: Vec<TransientBook>) {
    for book in books {
        save(book);
    }
}

/// `PathContainedInPath` for the scan request (`ERR_1017`).
#[derive(Debug, thiserror::Error)]
#[error("Cannot scan folder that is part of an existing library")]
pub struct PathContainedError;

impl PathContainedError {
    pub fn code(&self) -> &'static str {
        "ERR_1017"
    }
}

/// `TransientBookLifecycle.scanAndPersist`: scan a folder outside all libraries, cache the books.
pub fn scan_and_persist(
    state: &AppState,
    file_path: &Path,
) -> Result<Vec<TransientBook>, PathContainedError> {
    let folder = absolute(file_path);
    for library in LibraryDao::new(state.db.clone())
        .find_all()
        .map_err(|_| PathContainedError)?
    {
        let library_path = PathBuf::from(komga_core::dto::url_to_file_path(&library.root));
        if folder.starts_with(&library_path) {
            return Err(PathContainedError);
        }
    }

    let result = Scanner::new()
        .scan_root_folder(&folder, &ScanOptions::default())
        .map_err(|_| PathContainedError)?;
    let books: Vec<TransientBook> = result
        .series
        .into_iter()
        .flat_map(|(_, books)| books)
        .map(|book| TransientBook {
            media: Media {
                book_id: book.id.clone(),
                status: komga_core::model::media::MediaStatus::Unknown,
                media_type: None,
                comment: None,
                page_count: 0,
                pages: vec![],
                files: vec![],
                extension_class: None,
                extension_value: None,
                epub_divina_compatible: false,
                epub_is_kepub: false,
                created_date: komga_core::time_codec::now_utc(),
                last_modified_date: komga_core::time_codec::now_utc(),
            },
            metadata: TransientBookMetadata::default(),
            book,
        })
        .collect();
    save_many(books.clone());
    Ok(books)
}

/// `TransientBookLifecycle.analyzeAndPersist`: analyze and resolve (series, number) from metadata.
pub fn analyze_and_persist(state: &AppState, transient_book: &TransientBook) -> TransientBook {
    let analyzer = Analyzer::new(
        state.config.page_hashing,
        state.settings.get().thumbnail_size.max_edge(),
        state.config.epub_divina_letter_count_threshold,
    );
    let analysis = analyzer.analyze(&book_path(&transient_book.book), true);
    let media = analysis.media;

    let with_media = TransientBook {
        media: media.clone(),
        ..transient_book.clone()
    };
    let (series_id, number) = get_metadata(state, &with_media);

    let updated = TransientBook {
        metadata: TransientBookMetadata { number, series_id },
        ..with_media
    };
    save(updated.clone());
    updated
}

/// `TransientBookLifecycle.getMetadata`: first NUMBER_SORT patch wins for the number;
/// series is matched by exact title first, then by contained title.
fn get_metadata(state: &AppState, transient_book: &TransientBook) -> (Option<String>, Option<f32>) {
    let path = book_path(&transient_book.book);
    let media = &transient_book.media;

    // komga filters book metadata providers to those with the NUMBER_SORT capability
    let number = ComicInfoProvider
        .get_book_metadata_from_book(&path, media)
        .and_then(|p| p.number_sort)
        .or_else(|| {
            EpubMetadataProvider
                .get_book_metadata_from_book(&path, media)
                .and_then(|p| p.number_sort)
        });

    let series_providers: Vec<Box<dyn SeriesMetadataFromBookProvider>> =
        vec![Box::new(ComicInfoProvider), Box::new(EpubMetadataProvider)];
    let mut series_names: Vec<String> = vec![];
    for provider in &series_providers {
        let append = provider.supports_append_volume();
        for &flag in &[true, false] {
            let title = if flag == append {
                provider
                    .get_series_metadata_from_book(&path, media, flag)
                    .and_then(|p| p.title)
            } else {
                None
            };
            if let Some(title) = title.filter(|t| !t.is_empty()) {
                series_names.push(title);
            }
        }
    }

    let series = if !series_names.is_empty() {
        let dao = SeriesDao::new(state.db.clone());
        let exact: Vec<SearchConditionSeries> = series_names
            .iter()
            .map(|n| SearchConditionSeries::Title {
                title: komga_core::search::StringOp::Is { value: n.clone() },
            })
            .collect();
        let exact_matches = dao
            .find_all_by_condition(
                Some(&SearchConditionSeries::AnyOf { conditions: exact }),
                &SearchContext::default(),
            )
            .unwrap_or_default();
        match exact_matches.into_iter().next() {
            Some(s) => Some(s),
            None => {
                let contains: Vec<SearchConditionSeries> = series_names
                    .iter()
                    .map(|n| SearchConditionSeries::Title {
                        title: komga_core::search::StringOp::Contains { value: n.clone() },
                    })
                    .collect();
                dao.find_all_by_condition(
                    Some(&SearchConditionSeries::AnyOf {
                        conditions: contains,
                    }),
                    &SearchContext::default(),
                )
                .unwrap_or_default()
                .into_iter()
                .next()
            }
        }
    } else {
        None
    };

    (series.map(|s| s.id), number)
}

/// `TransientBookLifecycle.getBookPage`.
pub fn get_book_page(
    transient_book: &TransientBook,
    number: usize,
) -> Result<PageContent, komga_media::error::MediaError> {
    let media = &transient_book.media;
    let bytes =
        komga_media::container::get_page_content(&book_path(&transient_book.book), media, number)?;
    let media_type = if komga_media::container::media_profile(media.media_type.as_deref())
        == Some(komga_core::search::MediaProfile::Pdf)
    {
        komga_media::detect::IMAGE_JPEG.to_string()
    } else {
        media.pages[number - 1].media_type.clone()
    };
    Ok(PageContent { bytes, media_type })
}

fn book_path(book: &Book) -> PathBuf {
    PathBuf::from(komga_core::dto::url_to_file_path(&book.url))
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    }
}
