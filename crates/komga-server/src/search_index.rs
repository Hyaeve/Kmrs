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
use komga_search::{EntityDoc, IndexOp};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    if let Err(e) = write(index, first).and_then(|_| index.commit()) {
        tracing::error!("rebuild index for {entity:?}: write page 0 failed: {e}");
        return;
    }
    for page in 1..pages {
        match fetch(page) {
            Ok((items, _)) => {
                if let Err(e) = write(index, items).and_then(|_| index.commit()) {
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

/// The index-relevant change carried by an event: entity, id, and whether it was deleted.
fn event_op(event: &DomainEvent) -> Option<(LuceneEntity, String, bool)> {
    Some(match event {
        DomainEvent::SeriesAdded(s) | DomainEvent::SeriesUpdated(s) => {
            (LuceneEntity::Series, s.id.clone(), false)
        }
        DomainEvent::SeriesDeleted(s) => (LuceneEntity::Series, s.id.clone(), true),
        DomainEvent::BookAdded(b) | DomainEvent::BookUpdated(b) => {
            (LuceneEntity::Book, b.id.clone(), false)
        }
        DomainEvent::BookDeleted(b) => (LuceneEntity::Book, b.id.clone(), true),
        DomainEvent::CollectionAdded(c) | DomainEvent::CollectionUpdated(c) => {
            (LuceneEntity::Collection, c.id.clone(), false)
        }
        DomainEvent::CollectionDeleted(c) => (LuceneEntity::Collection, c.id.clone(), true),
        DomainEvent::ReadListAdded(r) | DomainEvent::ReadListUpdated(r) => {
            (LuceneEntity::ReadList, r.id.clone(), false)
        }
        DomainEvent::ReadListDeleted(r) => (LuceneEntity::ReadList, r.id.clone(), true),
        _ => return None,
    })
}

/// A drained burst resolved to one change per (entity, id): only the last event
/// counts, because an upsert re-reads the DTO from the DB at apply time anyway, so
/// intermediate states of the same entity are never indexed.
#[derive(Default)]
struct EventBatch {
    book_upserts: Vec<String>,
    series_upserts: Vec<String>,
    collection_upserts: Vec<String>,
    readlist_upserts: Vec<String>,
    deletes: Vec<IndexOp>,
}

impl EventBatch {
    fn is_empty(&self) -> bool {
        self.book_upserts.is_empty()
            && self.series_upserts.is_empty()
            && self.collection_upserts.is_empty()
            && self.readlist_upserts.is_empty()
            && self.deletes.is_empty()
    }
}

fn coalesce(events: &[DomainEvent]) -> EventBatch {
    let mut latest: BTreeMap<(LuceneEntity, String), bool> = BTreeMap::new();
    for event in events {
        if let Some((entity, id, deleted)) = event_op(event) {
            latest.insert((entity, id), deleted);
        }
    }
    let mut batch = EventBatch::default();
    for ((entity, id), deleted) in latest {
        if deleted {
            batch.deletes.push(IndexOp::Delete { entity, id });
        } else {
            match entity {
                LuceneEntity::Book => batch.book_upserts.push(id),
                LuceneEntity::Series => batch.series_upserts.push(id),
                LuceneEntity::Collection => batch.collection_upserts.push(id),
                LuceneEntity::ReadList => batch.readlist_upserts.push(id),
            }
        }
    }
    batch
}

fn book_upsert_docs(state: &AppState, book_ids: &[String]) -> Vec<EntityDoc> {
    if book_ids.is_empty() {
        return vec![];
    }
    let books = match BookDtoDao::new(state.db.clone()).find_all_by_ids(book_ids, "unused") {
        Ok(books) => books,
        Err(e) => {
            tracing::error!("search index batch update: could not fetch books: {e}");
            return vec![];
        }
    };
    let oneshot_series_ids: Vec<String> = books
        .iter()
        .filter(|book| book.oneshot)
        .map(|book| book.series_id.clone())
        .collect();
    let oneshot_series: HashMap<String, SeriesDto> =
        match SeriesDtoDao::new(state.db.clone()).find_all_by_ids(&oneshot_series_ids, "unused") {
            Ok(series) => series.into_iter().map(|s| (s.id.clone(), s)).collect(),
            Err(e) => {
                tracing::error!("search index batch update: could not fetch oneshot series: {e}");
                HashMap::new()
            }
        };
    books
        .iter()
        .filter_map(|book| {
            let series = oneshot_series.get(&book.series_id);
            if book.oneshot && series.is_none() {
                // a missing series means broken data; skipping keeps the rest of the
                // batch applied instead of failing it wholesale
                tracing::error!(
                    "search index batch update: oneshot book {} has no series {}",
                    book.id,
                    book.series_id
                );
                return None;
            }
            Some(book_to_document(book, series))
        })
        .collect()
}

fn series_upsert_docs(state: &AppState, series_ids: &[String]) -> Vec<EntityDoc> {
    if series_ids.is_empty() {
        return vec![];
    }
    match SeriesDtoDao::new(state.db.clone()).find_all_by_ids(series_ids, "unused") {
        Ok(series) => series.iter().map(series_to_document).collect(),
        Err(e) => {
            tracing::error!("search index batch update: could not fetch series: {e}");
            vec![]
        }
    }
}

/// Applies one drained burst as a single writer-lock batch; returns true when the
/// index changed (so the caller schedules a commit).
fn handle_batch(state: &AppState, events: &[DomainEvent]) -> bool {
    let batch = coalesce(events);
    if batch.is_empty() {
        return false;
    }
    let mut ops = batch.deletes;
    ops.extend(
        book_upsert_docs(state, &batch.book_upserts)
            .into_iter()
            .map(IndexOp::Upsert),
    );
    ops.extend(
        series_upsert_docs(state, &batch.series_upserts)
            .into_iter()
            .map(IndexOp::Upsert),
    );
    // collection/readlist events are user-driven and rare, so per-id fetches suffice
    for id in &batch.collection_upserts {
        if let Some(doc) = lookup_collection_doc(state, id) {
            ops.push(IndexOp::Upsert(doc));
        }
    }
    for id in &batch.readlist_upserts {
        if let Some(doc) = lookup_readlist_doc(state, id) {
            ops.push(IndexOp::Upsert(doc));
        }
    }
    if ops.is_empty() {
        return false;
    }
    if let Err(e) = state.search_index.apply_ops(ops) {
        tracing::error!("search index batch update failed: {e}");
    }
    true
}

/// `LuceneAsyncCommitter` debounces commits to one per 2s window (`komgaProperties.lucene.commitDelay`).
const COMMIT_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// Upper bound of one drain. A scan burst can refill the queue while a batch is being
/// processed, so the drain is capped to keep one batch's DTO fetches bounded; the
/// remainder is picked up as the next batch.
const MAX_BATCH_EVENTS: usize = 4096;

/// Feeds on a lossless tap of the bus and keeps the index in sync (`consumeEvents`;
/// the Java side consumes events synchronously, so neither side drops any). Pending
/// events are drained and applied per batch — a scan emits several events per book,
/// and one DB round-trip plus writer lock per event could never keep up — while
/// commits stay debounced to one per 2s window (each commit is a segment fsync).
/// DTO reads use the task pools, not the API pools.
pub fn consume_events(
    state: AppState,
    mut events_rx: tokio::sync::mpsc::UnboundedReceiver<DomainEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let state = state.task_context();
        let mut commit_at: Option<tokio::time::Instant> = None;
        loop {
            let event = match commit_at {
                Some(deadline) => match tokio::time::timeout_at(deadline, events_rx.recv()).await {
                    Ok(received) => received,
                    Err(_) => {
                        let index = state.search_index.clone();
                        match tokio::task::spawn_blocking(move || index.commit()).await {
                            Ok(Ok(())) => commit_at = None,
                            Ok(Err(e)) => {
                                tracing::error!("search index commit failed: {e}");
                                commit_at = Some(tokio::time::Instant::now() + COMMIT_DELAY);
                            }
                            Err(e) => {
                                tracing::error!("search index commit task failed: {e}");
                                commit_at = Some(tokio::time::Instant::now() + COMMIT_DELAY);
                            }
                        }
                        continue;
                    }
                },
                None => events_rx.recv().await,
            };
            // all senders are gone (shutdown)
            let Some(event) = event else { break };
            let mut events = vec![event];
            while events.len() < MAX_BATCH_EVENTS {
                match events_rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(
                        tokio::sync::mpsc::error::TryRecvError::Empty
                        | tokio::sync::mpsc::error::TryRecvError::Disconnected,
                    ) => break,
                }
            }
            let state = state.clone();
            let touched = tokio::task::spawn_blocking(move || handle_batch(&state, &events))
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("search index event task failed: {e}");
                    false
                });
            if touched && commit_at.is_none() {
                commit_at = Some(tokio::time::Instant::now() + COMMIT_DELAY);
            }
        }
    })
}

/// `SearchIndexController.createIndexIfNoneExist`: rebuild when missing, upgrade by version.
///
/// `rebuild_required` comes from `komga_search::decide_startup` (fresh directory, Java
/// Lucene takeover, or analyzer version mismatch): `SearchIndex::open` always creates
/// the index, so an existence check here could never detect those cases.
pub fn check_on_startup(state: &AppState, rebuild_required: bool) {
    if rebuild_required {
        tracing::info!("search index requires rebuild, triggering full rebuild");
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
    use crate::api::collections::tests as shared;
    use komga_core::dto::book::{BookMetadataDto, MediaDto};
    use komga_core::dto::common::{AlternateTitleDto, AuthorDto};
    use komga_core::dto::series::{BookMetadataAggregationDto, SeriesMetadataDto};
    use komga_core::model::book::Book;
    use komga_core::model::collection::SeriesCollection;
    use komga_core::model::readlist::ReadList;
    use komga_core::model::series::Series;
    use komga_core::time_codec::now_utc;
    use std::collections::BTreeSet;
    use std::time::Duration;

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

    fn model_book(id: &str, series_id: &str) -> Book {
        Book {
            id: id.into(),
            name: id.into(),
            url: "file:/l/s/b.cbz".into(),
            file_last_modified: now_utc(),
            series_id: series_id.into(),
            library_id: "l1".into(),
            file_size: 100,
            number: 0,
            file_hash: String::new(),
            file_hash_koreader: String::new(),
            deleted_date: None,
            oneshot: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn model_series(id: &str) -> Series {
        Series {
            id: id.into(),
            name: id.into(),
            url: "file:/l/s/".into(),
            file_last_modified: now_utc(),
            library_id: "l1".into(),
            book_count: 0,
            deleted_date: None,
            oneshot: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn model_collection(id: &str) -> SeriesCollection {
        SeriesCollection {
            id: id.into(),
            name: id.into(),
            ordered: true,
            series_ids: vec![],
            filtered: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    fn model_readlist(id: &str) -> ReadList {
        ReadList {
            id: id.into(),
            name: id.into(),
            summary: String::new(),
            ordered: true,
            book_ids: std::collections::BTreeMap::new(),
            filtered: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn coalesce_keeps_only_the_last_event_per_entity() {
        let events = vec![
            DomainEvent::BookAdded(model_book("b1", "s1")),
            DomainEvent::BookUpdated(model_book("b1", "s1")),
            DomainEvent::BookAdded(model_book("b2", "s1")),
            DomainEvent::BookDeleted(model_book("b2", "s1")),
            DomainEvent::SeriesDeleted(model_series("s1")),
            DomainEvent::SeriesAdded(model_series("s1")),
            DomainEvent::CollectionUpdated(model_collection("c1")),
            DomainEvent::ReadProgressSeriesChanged {
                series_id: "s1".into(),
                user_id: "u1".into(),
            },
        ];
        let batch = coalesce(&events);
        assert_eq!(batch.book_upserts, vec!["b1".to_string()]);
        assert_eq!(batch.series_upserts, vec!["s1".to_string()]);
        assert_eq!(batch.collection_upserts, vec!["c1".to_string()]);
        assert!(batch.readlist_upserts.is_empty());
        assert_eq!(batch.deletes.len(), 1);
        assert!(matches!(
            &batch.deletes[0],
            IndexOp::Delete {
                entity: LuceneEntity::Book,
                id,
            } if id == "b2"
        ));
    }

    #[test]
    fn handle_batch_indexes_coalesced_changes() {
        let state = shared::test_state();
        shared::seed_base(&state.db);
        let exec = |sql: &str| shared::exec(&state.db, sql, []);
        exec(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, ONESHOT) \
             VALUES ('s4', 's4', 'file:/l/s4/', '2020-01-01 00:00:00.0', 'l1', 1)",
        );
        exec(
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, PUBLISHER) \
             VALUES ('s4', 'ONGOING', 'Shot', 'Shot', 'shotpub')",
        );
        exec(
            "INSERT INTO BOOK_METADATA_AGGREGATION (SERIES_ID, SUMMARY, SUMMARY_NUMBER) \
             VALUES ('s4', '', '')",
        );
        exec(
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, ONESHOT) \
             VALUES ('b5', 'b5', 'file:/l/s4/b.cbz', '2020-01-01 00:00:00.0', 's4', 'l1', 1)",
        );
        exec(
            "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, NUMBER, NUMBER_SORT) \
             VALUES ('b5', 'One Shot', '1', 1)",
        );
        exec(
            "INSERT INTO MEDIA (BOOK_ID, STATUS, MEDIA_TYPE, PAGE_COUNT) \
             VALUES ('b5', 'READY', 'application/zip', 10)",
        );
        exec(
            "INSERT INTO READLIST (ID, NAME, SUMMARY, ORDERED, BOOK_COUNT) \
             VALUES ('r1', 'Marvel', '', 1, 1)",
        );
        exec("INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES ('r1', 'b1', 0)");
        // pre-existing documents the batch must replace (b1) and remove (s3)
        state
            .search_index
            .add_documents(vec![
                EntityDoc {
                    entity: LuceneEntity::Book,
                    id: "b1".into(),
                    fields: vec![("title".into(), "stale".into())],
                },
                EntityDoc {
                    entity: LuceneEntity::Series,
                    id: "s3".into(),
                    fields: vec![("title".into(), "Gamma".into())],
                },
            ])
            .unwrap();
        state.search_index.commit().unwrap();

        let events = vec![
            DomainEvent::BookUpdated(model_book("b1", "s1")),
            DomainEvent::BookAdded(model_book("b2", "s1")),
            DomainEvent::BookDeleted(model_book("b2", "s1")),
            DomainEvent::BookDeleted(model_book("b3", "s2")),
            DomainEvent::BookAdded(model_book("b3", "s2")),
            DomainEvent::BookAdded(model_book("b5", "s4")),
            DomainEvent::SeriesUpdated(model_series("s1")),
            DomainEvent::SeriesAdded(model_series("s2")),
            DomainEvent::SeriesDeleted(model_series("s2")),
            DomainEvent::SeriesDeleted(model_series("s3")),
            DomainEvent::CollectionAdded(model_collection("c1")),
            DomainEvent::CollectionUpdated(model_collection("c2")),
            DomainEvent::ReadListAdded(model_readlist("r1")),
        ];
        assert!(handle_batch(&state, &events));
        state.search_index.commit().unwrap();

        let index = &state.search_index;
        let book = |term: &str| {
            index
                .search_entity_ids(Some(term), LuceneEntity::Book)
                .unwrap()
        };
        let series = |term: &str| {
            index
                .search_entity_ids(Some(term), LuceneEntity::Series)
                .unwrap()
        };
        assert_eq!(book("b1"), vec!["b1"]);
        assert!(
            book("stale").is_empty(),
            "upsert replaces the stale document"
        );
        assert!(book("b2").is_empty(), "add then delete indexes nothing");
        assert_eq!(book("b3"), vec!["b3"], "delete then add re-indexes");
        assert_eq!(book("shot"), vec!["b5"]);
        assert_eq!(
            book("publisher:shotpub"),
            vec!["b5"],
            "oneshot book merges the series metadata"
        );
        assert_eq!(series("alpha"), vec!["s1"]);
        assert!(series("beta").is_empty(), "add then delete indexes nothing");
        assert!(series("gamma").is_empty(), "delete removes the document");
        assert_eq!(
            index
                .search_entity_ids(Some("best"), LuceneEntity::Collection)
                .unwrap(),
            vec!["c1"]
        );
        assert_eq!(
            index
                .search_entity_ids(Some("another"), LuceneEntity::Collection)
                .unwrap(),
            vec!["c2"]
        );
        assert_eq!(
            index
                .search_entity_ids(Some("marvel"), LuceneEntity::ReadList)
                .unwrap(),
            vec!["r1"]
        );
    }

    #[tokio::test]
    async fn consumer_indexes_every_event_of_an_overflow_burst() {
        let state = shared::test_state();
        let events = state.events.tap();
        shared::seed_base(&state.db);
        let handle = consume_events(state.clone(), events);
        // the three real events sit at the front of a burst far beyond the
        // broadcast capacity: a broadcast receiver would skip ahead past them,
        // the tap queues every one of them
        for id in ["b1", "b2", "b3"] {
            let _ = state
                .events
                .send(DomainEvent::BookAdded(model_book(id, "s1")));
        }
        for i in 0..3000 {
            let _ = state
                .events
                .send(DomainEvent::BookAdded(model_book(&format!("x{i}"), "s1")));
        }
        let indexed = |id: &str| {
            state
                .search_index
                .search_entity_ids(Some(id), LuceneEntity::Book)
                .unwrap()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let found = loop {
            if indexed("b1") == ["b1"] && indexed("b2") == ["b2"] && indexed("b3") == ["b3"] {
                break true;
            }
            if std::time::Instant::now() > deadline {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        handle.abort();
        assert!(found, "every event of the burst must reach the index");
    }
}
