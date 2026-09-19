//! Search index lifecycle, ported from `SearchIndexLifecycle.kt` and the startup check of
//! `interfaces/scheduler/SearchIndexController.kt`.
//!
//! Owns the mapping of DTOs to index documents (`LuceneEntity.kt` field by field), full and
//! incremental index maintenance (rebuild/upgrade/event-driven), and the startup version check.

use crate::events::DomainEvent;
use crate::state::AppState;
use komga_core::dto::book::BookDto;
use komga_core::dto::collection::CollectionDto;
use komga_core::dto::readlist::ReadListDto;
use komga_core::dto::series::SeriesDto;
use komga_core::model::user::ContentRestrictions;
use komga_core::search::{BookSearch, SearchContext, SeriesSearch};
use komga_core::task::{EmptyTask, LuceneEntity, Task, HIGHEST_PRIORITY};
use komga_db::dto_dao::book::BookDtoDao;
use komga_db::dto_dao::collection::CollectionDtoDao;
use komga_db::dto_dao::readlist::ReadListDtoDao;
use komga_db::dto_dao::series::SeriesDtoDao;
use komga_db::dto_dao::{EntitySearcher, PageRequest};
use komga_search::EntityDoc;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Adapts the crate-local `komga_search::EntitySearcher` to the dto_dao-facing one.
struct SearcherAdapter(Arc<komga_search::SearchIndex>);

impl EntitySearcher for SearcherAdapter {
    fn search_entity_ids(&self, term: Option<&str>, entity: LuceneEntity) -> Option<Vec<String>> {
        self.0.search_entity_ids(term, entity)
    }
}

/// The dto_dao-facing searcher for endpoint call sites.
pub fn searcher(state: &AppState) -> Arc<dyn EntitySearcher> {
    Arc::new(SearcherAdapter(state.search_index.clone()))
}

pub const INDEX_VERSION: i32 = 8;
const BATCH_SIZE: u32 = 5000;

fn anonymous_context() -> SearchContext {
    // `SearchContext.ofAnonymousUser()`
    SearchContext {
        user_id: Some("unused".to_string()),
        restrictions: ContentRestrictions::default(),
        library_ids: None,
    }
}

fn page_request(page: u32) -> PageRequest {
    PageRequest {
        page,
        size: BATCH_SIZE,
        unpaged: false,
        sort: vec![],
    }
}

fn year(date: &time::Date) -> String {
    format!("{:04}", date.year())
}

fn push(fields: &mut Vec<(String, String)>, name: &str, value: impl Into<String>) {
    fields.push((name.to_string(), value.into()));
}

/// `BookDto.toDocument()`
fn book_to_document(book: &BookDto, series: Option<&SeriesDto>) -> EntityDoc {
    let mut fields = vec![];
    push(&mut fields, "title", &book.metadata.title);
    push(&mut fields, "isbn", &book.metadata.isbn);
    for tag in &book.metadata.tags {
        push(&mut fields, "tag", tag);
    }
    for author in &book.metadata.authors {
        push(&mut fields, "author", &author.name);
        push(&mut fields, &author.role, &author.name);
    }
    if let Some(date) = &book.metadata.release_date {
        push(&mut fields, "release_date", year(date));
    }
    push(&mut fields, "status", &book.media.status);
    push(&mut fields, "deleted", book.deleted.to_string());
    push(&mut fields, "oneshot", book.oneshot.to_string());
    if book.oneshot {
        // `SeriesDto.oneshotDocument`: the series metadata is merged into the book document
        let series = series.expect("oneshot book requires its series for indexing");
        push(&mut fields, "publisher", &series.metadata.publisher);
        push(&mut fields, "status", &series.metadata.status);
        push(
            &mut fields,
            "reading_direction",
            &series.metadata.reading_direction,
        );
        if let Some(age) = series.metadata.age_rating {
            push(&mut fields, "age_rating", age.to_string());
        }
        if !series.metadata.language.is_empty() {
            push(&mut fields, "language", &series.metadata.language);
        }
        for genre in &series.metadata.genres {
            push(&mut fields, "genre", genre);
        }
        for label in &series.metadata.sharing_labels {
            push(&mut fields, "sharing_label", label);
        }
        push(&mut fields, "complete", "true");
    }
    EntityDoc {
        entity: LuceneEntity::Book,
        id: book.id.clone(),
        fields,
    }
}

/// `SeriesDto.toDocument()`
fn series_to_document(series: &SeriesDto) -> EntityDoc {
    let mut fields = vec![];
    let metadata = &series.metadata;
    push(&mut fields, "title", &metadata.title);
    if metadata.title_sort != metadata.title {
        push(&mut fields, "title", &metadata.title_sort);
    }
    for alt in &metadata.alternate_titles {
        push(&mut fields, "title", &alt.title);
    }
    push(&mut fields, "publisher", &metadata.publisher);
    push(&mut fields, "status", &metadata.status);
    push(
        &mut fields,
        "reading_direction",
        &metadata.reading_direction,
    );
    if let Some(age) = metadata.age_rating {
        push(&mut fields, "age_rating", age.to_string());
    }
    if !metadata.language.is_empty() {
        push(&mut fields, "language", &metadata.language);
    }
    for tag in &metadata.tags {
        push(&mut fields, "series_tag", tag);
        push(&mut fields, "tag", tag);
    }
    for tag in &series.books_metadata.tags {
        push(&mut fields, "book_tag", tag);
        push(&mut fields, "tag", tag);
    }
    for genre in &metadata.genres {
        push(&mut fields, "genre", genre);
    }
    for label in &metadata.sharing_labels {
        push(&mut fields, "sharing_label", label);
    }
    if let Some(total) = metadata.total_book_count {
        push(&mut fields, "total_book_count", total.to_string());
    }
    push(&mut fields, "book_count", series.books_count.to_string());
    for author in &series.books_metadata.authors {
        push(&mut fields, "author", &author.name);
        push(&mut fields, &author.role, &author.name);
    }
    if let Some(date) = &series.books_metadata.release_date {
        push(&mut fields, "release_date", year(date));
    }
    push(&mut fields, "deleted", series.deleted.to_string());
    push(&mut fields, "oneshot", series.oneshot.to_string());
    if let Some(total) = metadata.total_book_count {
        push(
            &mut fields,
            "complete",
            (total == series.books_count).to_string(),
        );
    }
    EntityDoc {
        entity: LuceneEntity::Series,
        id: series.id.clone(),
        fields,
    }
}

fn collection_to_document(collection: &CollectionDto) -> EntityDoc {
    EntityDoc {
        entity: LuceneEntity::Collection,
        id: collection.id.clone(),
        fields: vec![("name".to_string(), collection.name.clone())],
    }
}

fn readlist_to_document(readlist: &ReadListDto) -> EntityDoc {
    EntityDoc {
        entity: LuceneEntity::ReadList,
        id: readlist.id.clone(),
        fields: vec![("name".to_string(), readlist.name.clone())],
    }
}

fn rebuild_one(state: &AppState, entity: LuceneEntity) {
    let index = state.search_index.clone();
    if let Err(e) = index.delete_entity_type(entity) {
        tracing::error!("rebuild index for {entity:?}: could not delete existing documents: {e}");
        return;
    }
    match entity {
        LuceneEntity::Book => rebuild_pages(
            entity,
            &index,
            |page| {
                BookDtoDao::new(state.db.clone())
                    .find_all(
                        &BookSearch {
                            condition: None,
                            full_text_search: None,
                        },
                        &anonymous_context(),
                        &page_request(page),
                    )
                    .map(|p| (p.items, p.total))
            },
            |index, books: Vec<BookDto>| {
                let series_dao = SeriesDtoDao::new(state.db.clone());
                let docs: Vec<EntityDoc> = books
                    .iter()
                    .map(|book| {
                        let series = book
                            .oneshot
                            .then(|| {
                                series_dao
                                    .find_by_id(&book.series_id, "unused")
                                    .ok()
                                    .flatten()
                            })
                            .flatten();
                        book_to_document(book, series.as_ref())
                    })
                    .collect();
                index.add_documents(docs)
            },
        ),
        LuceneEntity::Series => rebuild_pages(
            entity,
            &index,
            |page| {
                SeriesDtoDao::new(state.db.clone())
                    .find_all(
                        &SeriesSearch {
                            condition: None,
                            full_text_search: None,
                        },
                        None,
                        &anonymous_context(),
                        &page_request(page),
                    )
                    .map(|p| (p.items, p.total))
            },
            |index, series: Vec<SeriesDto>| {
                index.add_documents(series.iter().map(series_to_document).collect())
            },
        ),
        LuceneEntity::Collection => rebuild_pages(
            entity,
            &index,
            |page| {
                CollectionDtoDao::new(state.db.clone())
                    .find_all(
                        None,
                        None,
                        None,
                        &page_request(page),
                        &ContentRestrictions::default(),
                    )
                    .map(|p| (p.items, p.total))
            },
            |index, collections: Vec<CollectionDto>| {
                index.add_documents(collections.iter().map(collection_to_document).collect())
            },
        ),
        LuceneEntity::ReadList => rebuild_pages(
            entity,
            &index,
            |page| {
                ReadListDtoDao::new(state.db.clone())
                    .find_all(
                        None,
                        None,
                        None,
                        &page_request(page),
                        &ContentRestrictions::default(),
                    )
                    .map(|p| (p.items, p.total))
            },
            |index, readlists: Vec<ReadListDto>| {
                index.add_documents(readlists.iter().map(readlist_to_document).collect())
            },
        ),
    }
}

fn rebuild_pages<T, F, W>(
    entity: LuceneEntity,
    index: &komga_search::SearchIndex,
    mut fetch: F,
    mut write: W,
) where
    F: FnMut(u32) -> komga_db::Result<(Vec<T>, i64)>,
    W: FnMut(&komga_search::SearchIndex, Vec<T>) -> komga_search::Result<()>,
{
    let (first, total) = match fetch(0) {
        Ok(first) => first,
        Err(e) => {
            tracing::error!("rebuild index for {entity:?}: first page failed: {e}");
            return;
        }
    };
    let pages = ((total + BATCH_SIZE as i64 - 1) / BATCH_SIZE as i64).max(1) as u32;
    if let Err(e) = write(index, first) {
        tracing::error!("rebuild index for {entity:?}: write page 0 failed: {e}");
        return;
    }
    for page in 1..pages {
        match fetch(page) {
            Ok((items, _)) => {
                if let Err(e) = write(index, items) {
                    tracing::error!("rebuild index for {entity:?}: write page {page} failed: {e}");
                    return;
                }
            }
            Err(e) => {
                tracing::error!("rebuild index for {entity:?}: page {page} failed: {e}");
                return;
            }
        }
    }
    tracing::info!("Wrote {entity:?} index ({total} entities)");
}

/// `SearchIndexLifecycle.rebuildIndex`: full rebuild of the given entities (all by default).
pub fn rebuild_index(state: &AppState, entities: Option<BTreeSet<LuceneEntity>>) {
    let target: BTreeSet<LuceneEntity> = entities.unwrap_or_else(|| {
        [
            LuceneEntity::Book,
            LuceneEntity::Series,
            LuceneEntity::Collection,
            LuceneEntity::ReadList,
        ]
        .into_iter()
        .collect()
    });
    tracing::info!("Rebuild index for: {target:?}");
    for entity in target {
        rebuild_one(state, entity);
    }
    if let Err(e) = state.search_index.set_index_version(INDEX_VERSION) {
        tracing::error!("set index version after rebuild failed: {e}");
    }
}

/// `SearchIndexLifecycle.upgradeIndex` + version stamp.
pub fn upgrade_index(state: &AppState) {
    state.search_index.upgrade();
    if let Err(e) = state.search_index.set_index_version(INDEX_VERSION) {
        tracing::error!("set index version after upgrade failed: {e}");
    }
}

fn lookup_book_doc(state: &AppState, book_id: &str) -> Option<EntityDoc> {
    let book = BookDtoDao::new(state.db.clone())
        .find_by_id(book_id, "unused")
        .ok()
        .flatten()?;
    let series = book
        .oneshot
        .then(|| {
            SeriesDtoDao::new(state.db.clone())
                .find_by_id(&book.series_id, "unused")
                .ok()
                .flatten()
        })
        .flatten();
    Some(book_to_document(&book, series.as_ref()))
}

fn lookup_series_doc(state: &AppState, series_id: &str) -> Option<EntityDoc> {
    let series = SeriesDtoDao::new(state.db.clone())
        .find_by_id(series_id, "unused")
        .ok()
        .flatten()?;
    Some(series_to_document(&series))
}

fn lookup_collection_doc(state: &AppState, collection_id: &str) -> Option<EntityDoc> {
    let collection = CollectionDtoDao::new(state.db.clone())
        .find_by_id(collection_id, None, &ContentRestrictions::default())
        .ok()
        .flatten()?;
    Some(collection_to_document(&CollectionDto::from(&collection)))
}

fn lookup_readlist_doc(state: &AppState, readlist_id: &str) -> Option<EntityDoc> {
    let readlist = ReadListDtoDao::new(state.db.clone())
        .find_by_id(readlist_id, None, &ContentRestrictions::default())
        .ok()
        .flatten()?;
    Some(readlist_to_document(&ReadListDto::from(&readlist)))
}

fn handle_event(state: &AppState, event: &DomainEvent) {
    let index = &state.search_index;
    let result: komga_search::Result<()> = (|| {
        match event {
            DomainEvent::SeriesAdded(series) => {
                if let Some(doc) = lookup_series_doc(state, &series.id) {
                    index.add_documents(vec![doc])?;
                }
            }
            DomainEvent::SeriesUpdated(series) => {
                if let Some(doc) = lookup_series_doc(state, &series.id) {
                    index.update_document(LuceneEntity::Series, &series.id, doc)?;
                }
            }
            DomainEvent::SeriesDeleted(series) => {
                index.delete_documents(LuceneEntity::Series, &series.id)?;
            }
            DomainEvent::BookAdded(book) => {
                if let Some(doc) = lookup_book_doc(state, &book.id) {
                    index.add_documents(vec![doc])?;
                }
            }
            DomainEvent::BookUpdated(book) => {
                if let Some(doc) = lookup_book_doc(state, &book.id) {
                    index.update_document(LuceneEntity::Book, &book.id, doc)?;
                }
            }
            DomainEvent::BookDeleted(book) => {
                index.delete_documents(LuceneEntity::Book, &book.id)?;
            }
            DomainEvent::CollectionAdded(collection) => {
                if let Some(doc) = lookup_collection_doc(state, &collection.id) {
                    index.add_documents(vec![doc])?;
                }
            }
            DomainEvent::CollectionUpdated(collection) => {
                if let Some(doc) = lookup_collection_doc(state, &collection.id) {
                    index.update_document(LuceneEntity::Collection, &collection.id, doc)?;
                }
            }
            DomainEvent::CollectionDeleted(collection) => {
                index.delete_documents(LuceneEntity::Collection, &collection.id)?;
            }
            DomainEvent::ReadListAdded(readlist) => {
                if let Some(doc) = lookup_readlist_doc(state, &readlist.id) {
                    index.add_documents(vec![doc])?;
                }
            }
            DomainEvent::ReadListUpdated(readlist) => {
                if let Some(doc) = lookup_readlist_doc(state, &readlist.id) {
                    index.update_document(LuceneEntity::ReadList, &readlist.id, doc)?;
                }
            }
            DomainEvent::ReadListDeleted(readlist) => {
                index.delete_documents(LuceneEntity::ReadList, &readlist.id)?;
            }
            _ => {}
        }
        Ok(())
    })();
    if let Err(e) = result {
        tracing::error!("search index update failed: {e}");
    }
}

/// Subscribes to the domain event bus and keeps the index in sync (`consumeEvents`).
pub fn consume_events(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut receiver = state.events.subscribe();
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let state = state.clone();
                    tokio::task::spawn_blocking(move || handle_event(&state, &event))
                        .await
                        .unwrap_or_else(|e| tracing::error!("search index event task failed: {e}"));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("search index event consumer lagged by {n} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

/// `SearchIndexController.createIndexIfNoneExist`: rebuild when missing, upgrade by version.
pub fn check_on_startup(state: &AppState) {
    if !komga_search::SearchIndex::exists(&state.config.lucene_dir) {
        tracing::info!("Lucene index not found, trigger rebuild");
        let _ = state.task_emitter.rebuild_index(None, HIGHEST_PRIORITY);
        return;
    }
    let version = state.search_index.index_version();
    tracing::info!("Lucene index version: {version}");
    if version < 6 {
        let _ = state.task_emitter.submit(Task::UpgradeIndex(EmptyTask {
            priority: HIGHEST_PRIORITY,
            group_id: None,
            unique_id: String::new(),
        }));
        let _ = state.task_emitter.rebuild_index(
            Some([LuceneEntity::Series].into_iter().collect()),
            HIGHEST_PRIORITY,
        );
    } else if version < INDEX_VERSION {
        let _ = state.task_emitter.rebuild_index(
            Some([LuceneEntity::Series].into_iter().collect()),
            HIGHEST_PRIORITY,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::dto::book::{BookMetadataDto, MediaDto};
    use komga_core::dto::common::{AlternateTitleDto, AuthorDto};
    use komga_core::dto::series::{BookMetadataAggregationDto, SeriesMetadataDto};
    use std::collections::BTreeSet;

    fn dt() -> time::OffsetDateTime {
        komga_core::time_codec::parse_datetime_utc("2024-01-02 03:04:05").unwrap()
    }

    fn date(y: i32, m: u8, d: u8) -> time::Date {
        time::Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap()
    }

    fn book_metadata() -> BookMetadataDto {
        BookMetadataDto {
            title: "Berserk v01".into(),
            title_lock: false,
            summary: String::new(),
            summary_lock: false,
            number: "1".into(),
            number_lock: false,
            number_sort: 1.0,
            number_sort_lock: false,
            release_date: Some(date(1990, 8, 25)),
            release_date_lock: false,
            authors: vec![AuthorDto {
                name: "Kentaro Miura".into(),
                role: "writer".into(),
            }],
            authors_lock: false,
            tags: ["seinen".to_string()].into_iter().collect(),
            tags_lock: false,
            isbn: "9781593070205".into(),
            isbn_lock: false,
            links: vec![],
            links_lock: false,
            created: dt(),
            last_modified: dt(),
        }
    }

    fn book(oneshot: bool) -> BookDto {
        BookDto {
            id: "b1".into(),
            series_id: "s1".into(),
            series_title: "Berserk".into(),
            library_id: "l1".into(),
            name: "v01".into(),
            url: "/data/berserk/v01.cbz".into(),
            number: 1,
            created: dt(),
            last_modified: dt(),
            file_last_modified: dt(),
            size_bytes: 1024,
            size: "1 KiB".into(),
            media: MediaDto {
                status: "READY".into(),
                media_type: "application/zip".into(),
                pages_count: 10,
                comment: String::new(),
                epub_divina_compatible: false,
                epub_is_kepub: false,
                media_profile: "DIVINA".into(),
            },
            metadata: book_metadata(),
            read_progress: None,
            deleted: false,
            file_hash: String::new(),
            oneshot,
        }
    }

    fn series_metadata() -> SeriesMetadataDto {
        SeriesMetadataDto {
            status: "ONGOING".into(),
            status_lock: false,
            title: "Berserk".into(),
            title_lock: false,
            title_sort: "Berserk, The".into(),
            title_sort_lock: false,
            summary: "Guts".into(),
            summary_lock: false,
            reading_direction: "RIGHT_TO_LEFT".into(),
            reading_direction_lock: false,
            publisher: "Hakusensha".into(),
            publisher_lock: false,
            age_rating: Some(18),
            age_rating_lock: false,
            language: "ja".into(),
            language_lock: false,
            genres: ["action".to_string()].into_iter().collect(),
            genres_lock: false,
            tags: ["seinen".to_string()].into_iter().collect(),
            tags_lock: false,
            total_book_count: Some(41),
            total_book_count_lock: false,
            sharing_labels: BTreeSet::new(),
            sharing_labels_lock: false,
            links: vec![],
            links_lock: false,
            alternate_titles: vec![AlternateTitleDto {
                label: "ja".into(),
                title: "ベルセルク".into(),
            }],
            alternate_titles_lock: false,
            created: dt(),
            last_modified: dt(),
        }
    }

    fn series(books_count: i32) -> SeriesDto {
        SeriesDto {
            id: "s1".into(),
            library_id: "l1".into(),
            name: "Berserk".into(),
            url: "/data/berserk/".into(),
            created: dt(),
            last_modified: dt(),
            file_last_modified: dt(),
            books_count,
            books_read_count: 0,
            books_unread_count: books_count,
            books_in_progress_count: 0,
            metadata: series_metadata(),
            books_metadata: BookMetadataAggregationDto {
                authors: vec![AuthorDto {
                    name: "Kentaro Miura".into(),
                    role: "writer".into(),
                }],
                tags: ["dark fantasy".to_string()].into_iter().collect(),
                release_date: Some(date(1990, 8, 25)),
                summary: "Guts".into(),
                summary_number: "1".into(),
                created: dt(),
                last_modified: dt(),
            },
            deleted: false,
            oneshot: false,
        }
    }

    fn fields(doc: &EntityDoc) -> Vec<(&str, &str)> {
        doc.fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    #[test]
    fn book_document_fields() {
        let doc = book_to_document(&book(false), None);
        assert_eq!(doc.entity, LuceneEntity::Book);
        assert_eq!(doc.id, "b1");
        let f = fields(&doc);
        assert!(f.contains(&("title", "Berserk v01")));
        assert!(f.contains(&("isbn", "9781593070205")));
        assert!(f.contains(&("tag", "seinen")));
        assert!(f.contains(&("author", "Kentaro Miura")));
        assert!(f.contains(&("writer", "Kentaro Miura")));
        assert!(f.contains(&("release_date", "1990")));
        assert!(f.contains(&("status", "READY")));
        assert!(f.contains(&("deleted", "false")));
        assert!(f.contains(&("oneshot", "false")));
        assert!(!f.iter().any(|(k, _)| *k == "complete"));
    }

    #[test]
    fn oneshot_book_merges_series_fields() {
        let mut book = book(true);
        book.oneshot = true;
        let doc = book_to_document(&book, Some(&series(1)));
        let f = fields(&doc);
        assert!(f.contains(&("publisher", "Hakusensha")));
        assert!(f.contains(&("status", "ONGOING")));
        assert!(f.contains(&("reading_direction", "RIGHT_TO_LEFT")));
        assert!(f.contains(&("age_rating", "18")));
        assert!(f.contains(&("language", "ja")));
        assert!(f.contains(&("genre", "action")));
        assert!(f.contains(&("complete", "true")));
    }

    #[test]
    fn series_document_fields() {
        let doc = series_to_document(&series(40));
        assert_eq!(doc.entity, LuceneEntity::Series);
        assert_eq!(doc.id, "s1");
        let f = fields(&doc);
        // title, titleSort (differs), alternate title
        assert_eq!(f.iter().filter(|(k, _)| *k == "title").count(), 3);
        assert!(f.contains(&("title", "Berserk")));
        assert!(f.contains(&("title", "Berserk, The")));
        assert!(f.contains(&("title", "ベルセルク")));
        assert!(f.contains(&("publisher", "Hakusensha")));
        assert!(f.contains(&("status", "ONGOING")));
        assert!(f.contains(&("reading_direction", "RIGHT_TO_LEFT")));
        assert!(f.contains(&("age_rating", "18")));
        assert!(f.contains(&("language", "ja")));
        assert!(f.contains(&("series_tag", "seinen")));
        assert!(f.contains(&("book_tag", "dark fantasy")));
        assert!(f.contains(&("genre", "action")));
        assert!(f.contains(&("total_book_count", "41")));
        assert!(f.contains(&("book_count", "40")));
        assert!(f.contains(&("author", "Kentaro Miura")));
        assert!(f.contains(&("release_date", "1990")));
        assert!(f.contains(&("complete", "false")));
        // 41 != 40: incomplete
        assert!(f.contains(&("deleted", "false")));
        assert!(f.contains(&("oneshot", "false")));
    }

    #[test]
    fn series_document_complete_when_counts_match() {
        let doc = series_to_document(&series(41));
        assert!(fields(&doc).contains(&("complete", "true")));
    }

    #[test]
    fn collection_and_readlist_documents() {
        let c = collection_to_document(&CollectionDto {
            id: "c1".into(),
            name: "Best".into(),
            ordered: true,
            series_ids: vec![],
            created_date: dt(),
            last_modified_date: dt(),
            filtered: false,
        });
        assert_eq!(c.entity, LuceneEntity::Collection);
        assert_eq!(fields(&c), vec![("name", "Best")]);

        let r = readlist_to_document(&ReadListDto {
            id: "r1".into(),
            name: "Marvel".into(),
            summary: String::new(),
            ordered: true,
            book_ids: vec![],
            created_date: dt(),
            last_modified_date: dt(),
            filtered: false,
        });
        assert_eq!(r.entity, LuceneEntity::ReadList);
        assert_eq!(fields(&r), vec![("name", "Marvel")]);
    }
}
