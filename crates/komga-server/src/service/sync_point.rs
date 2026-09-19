//! `SyncPointLifecycle.kt`: creation of sync points (with the on-deck list) and the take-* flows
//! that page unsynced entities and mark them synced.

use crate::state::AppState;
use komga_core::model::sync_point::{
    SyncPoint, SyncPointBook, SyncPointReadList, SyncPointReadListBook,
};
use komga_core::model::user::KomgaUser;
use komga_core::search::{BooleanOp, Equality, MediaProfile, SearchConditionBook, SearchContext};
use komga_core::time_codec;
use komga_db::dao::sync_point::{SyncPage, SyncPointDao};
use komga_db::dto_dao::book::BookDtoDao;
use komga_db::dto_dao::PageRequest;
use rusqlite::types::Value;
use std::collections::BTreeSet;
use time::OffsetDateTime;

pub const ON_DECK_ID: &str = "KOMGA-ONDECK";
pub const ON_DECK_NAME: &str = "On Deck";

/// `createSyncPoint(user, apiKeyId, libraryIds)`; `libraryIds == None` syncs everything.
pub fn create_sync_point(
    state: &AppState,
    user: &KomgaUser,
    api_key_id: Option<&str>,
    library_ids: Option<&[String]>,
) -> komga_db::Result<SyncPoint> {
    let ctx = SearchContext::of_user(user);
    let sync_point_id = state.tsid.create_string();
    let created_at = time_codec::now_utc();

    let dao = SyncPointDao::new(state.db.clone());
    dao.insert(&SyncPoint {
        id: sync_point_id.clone(),
        user_id: user.id.clone(),
        api_key_id: api_key_id.map(str::to_string),
        created_date: created_at,
    })?;

    // ready epub-profile books, optionally scoped to libraries
    let conditions = match library_ids {
        Some(ids) if !ids.is_empty() => vec![
            SearchConditionBook::AnyOf {
                conditions: ids
                    .iter()
                    .map(|id| SearchConditionBook::LibraryId {
                        operator: Equality::Is { value: id.clone() },
                    })
                    .collect(),
            },
            SearchConditionBook::MediaStatus {
                operator: Equality::Is {
                    value: komga_core::model::media::MediaStatus::Ready,
                },
            },
            SearchConditionBook::MediaProfile {
                operator: Equality::Is {
                    value: MediaProfile::Epub,
                },
            },
            SearchConditionBook::Deleted {
                deleted: BooleanOp::IsFalse,
            },
        ],
        _ => vec![
            SearchConditionBook::MediaStatus {
                operator: Equality::Is {
                    value: komga_core::model::media::MediaStatus::Ready,
                },
            },
            SearchConditionBook::MediaProfile {
                operator: Equality::Is {
                    value: MediaProfile::Epub,
                },
            },
            SearchConditionBook::Deleted {
                deleted: BooleanOp::IsFalse,
            },
        ],
    };
    let condition = SearchConditionBook::AllOf { conditions };
    let books = komga_db::dao::book::BookDao::new(state.db.clone()).find_all_by_condition(
        Some(&condition),
        &ctx,
        &[],
    )?;

    let now = time_codec::now_utc();
    let rows: Vec<SyncPointBook> = books
        .iter()
        .map(|b| {
            let (metadata_modified, rp_modified, thumbnail_id) =
                book_sync_columns(state, &b.id, &user.id).unwrap_or((None, None, None));
            SyncPointBook {
                sync_point_id: sync_point_id.clone(),
                book_id: b.id.clone(),
                book_created_date: b.created_date,
                book_last_modified_date: b.last_modified_date,
                book_file_last_modified: b.file_last_modified,
                book_file_size: b.file_size,
                book_file_hash: b.file_hash.clone(),
                book_metadata_last_modified_date: metadata_modified.unwrap_or(now),
                book_read_progress_last_modified_date: rp_modified,
                book_thumbnail_id: thumbnail_id,
                synced: false,
            }
        })
        .collect();
    dao.insert_books(&rows)?;

    add_on_deck(state, &sync_point_id, user, library_ids, created_at)?;

    dao.find_by_id(&sync_point_id)?.ok_or_else(|| {
        komga_db::Error::EnumValue(format!(
            "sync point not found after insert: {sync_point_id}"
        ))
    })
}

fn book_sync_columns(
    state: &AppState,
    book_id: &str,
    user_id: &str,
) -> komga_db::Result<(
    Option<OffsetDateTime>,
    Option<OffsetDateTime>,
    Option<String>,
)> {
    let conn = state.db.ro();
    let mut stmt = conn.prepare(
        "SELECT BOOK_METADATA.LAST_MODIFIED_DATE, READ_PROGRESS.LAST_MODIFIED_DATE, THUMBNAIL_BOOK.ID \
         FROM BOOK_METADATA \
         LEFT JOIN READ_PROGRESS ON BOOK_METADATA.BOOK_ID = READ_PROGRESS.BOOK_ID AND READ_PROGRESS.USER_ID = ? \
         LEFT JOIN THUMBNAIL_BOOK ON BOOK_METADATA.BOOK_ID = THUMBNAIL_BOOK.BOOK_ID AND THUMBNAIL_BOOK.SELECTED = 1 \
         WHERE BOOK_METADATA.BOOK_ID = ?",
    )?;
    let mut rows = stmt.query_map(rusqlite::params![user_id, book_id], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let Some((metadata, rp, thumbnail)) = rows.next().transpose()? else {
        return Ok((None, None, None));
    };
    Ok((
        metadata.and_then(|s| time_codec::parse_datetime_utc(&s)),
        rp.and_then(|s| time_codec::parse_datetime_utc(&s)),
        thumbnail,
    ))
}

/// `addOnDeck`: the "On Deck" read list entry (only when the list is non-empty), with the most
/// recent read date of the on-deck series.
fn add_on_deck(
    state: &AppState,
    sync_point_id: &str,
    user: &KomgaUser,
    library_ids: Option<&[String]>,
    created_at: OffsetDateTime,
) -> komga_db::Result<()> {
    let filter: Option<BTreeSet<String>> = library_ids.map(|ids| ids.iter().cloned().collect());
    let page = BookDtoDao::new(state.db.clone()).find_all_on_deck(
        &user.id,
        filter.as_ref(),
        &user.restrictions,
        &PageRequest {
            page: 0,
            size: 20,
            unpaged: true,
            sort: vec![],
        },
    )?;
    if page.items.is_empty() {
        return Ok(());
    }

    let dao = SyncPointDao::new(state.db.clone());
    let entries: Vec<SyncPointReadListBook> = page
        .items
        .iter()
        .map(|b| SyncPointReadListBook {
            sync_point_id: sync_point_id.to_string(),
            readlist_id: ON_DECK_ID.to_string(),
            book_id: b.id.clone(),
        })
        .collect();
    dao.insert_readlist_books(&entries)?;

    let series_ids: BTreeSet<String> = page.items.iter().map(|b| b.series_id.clone()).collect();
    let most_recent = most_recent_read_date(state, &user.id, &series_ids)?.unwrap_or(created_at);
    dao.insert_readlist(&SyncPointReadList {
        sync_point_id: sync_point_id.to_string(),
        readlist_id: ON_DECK_ID.to_string(),
        readlist_name: ON_DECK_NAME.to_string(),
        readlist_created_date: created_at,
        readlist_last_modified_date: most_recent,
        synced: false,
    })?;
    Ok(())
}

fn most_recent_read_date(
    state: &AppState,
    user_id: &str,
    series_ids: &BTreeSet<String>,
) -> komga_db::Result<Option<OffsetDateTime>> {
    if series_ids.is_empty() {
        return Ok(None);
    }
    let conn = state.db.ro();
    let placeholders = series_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let mut values: Vec<Value> = vec![Value::Text(user_id.to_string())];
    values.extend(series_ids.iter().map(|id| Value::Text(id.clone())));
    let mut stmt = conn.prepare(&format!(
        "SELECT MAX(MOST_RECENT_READ_DATE) FROM READ_PROGRESS_SERIES WHERE USER_ID = ? AND SERIES_ID IN ({placeholders})"
    ))?;
    let mut rows = stmt.query_map(rusqlite::params_from_iter(values), |row| {
        row.get::<_, Option<String>>(0)
    })?;
    let Some(date) = rows.next().transpose()? else {
        return Ok(None);
    };
    Ok(date.and_then(|s| time_codec::parse_datetime_utc(&s)))
}

// region take-* flows

pub fn take_books(
    state: &AppState,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointBook>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result = dao.find_books_by_id_page(to_sync_point_id, true, page, size)?;
    dao.mark_books_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|b| b.book_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

pub fn take_books_added(
    state: &AppState,
    from_sync_point_id: &str,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointBook>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result = dao.find_books_added(from_sync_point_id, to_sync_point_id, true, page, size)?;
    dao.mark_books_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|b| b.book_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

pub fn take_books_changed(
    state: &AppState,
    from_sync_point_id: &str,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointBook>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result = dao.find_books_changed(from_sync_point_id, to_sync_point_id, true, page, size)?;
    dao.mark_books_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|b| b.book_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

pub fn take_books_removed(
    state: &AppState,
    from_sync_point_id: &str,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointBook>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result = dao.find_books_removed(from_sync_point_id, to_sync_point_id, true, page, size)?;
    dao.mark_books_removed_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|b| b.book_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

pub fn take_books_read_progress_changed(
    state: &AppState,
    from_sync_point_id: &str,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointBook>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result = dao.find_books_read_progress_changed(
        from_sync_point_id,
        to_sync_point_id,
        true,
        page,
        size,
    )?;
    dao.mark_books_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|b| b.book_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

pub fn take_read_lists(
    state: &AppState,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointReadList>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result = dao.find_readlists_by_id_page(to_sync_point_id, true, page, size)?;
    dao.mark_readlists_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|r| r.readlist_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

pub fn take_read_lists_added(
    state: &AppState,
    from_sync_point_id: &str,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointReadList>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result =
        dao.find_readlists_added(from_sync_point_id, to_sync_point_id, true, page, size)?;
    dao.mark_readlists_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|r| r.readlist_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

pub fn take_read_lists_changed(
    state: &AppState,
    from_sync_point_id: &str,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointReadList>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result =
        dao.find_readlists_changed(from_sync_point_id, to_sync_point_id, true, page, size)?;
    dao.mark_readlists_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|r| r.readlist_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

pub fn take_read_lists_removed(
    state: &AppState,
    from_sync_point_id: &str,
    to_sync_point_id: &str,
    page: u32,
    size: u32,
) -> komga_db::Result<SyncPage<SyncPointReadList>> {
    let dao = SyncPointDao::new(state.db.clone());
    let result =
        dao.find_readlists_removed(from_sync_point_id, to_sync_point_id, true, page, size)?;
    dao.mark_readlists_removed_synced(
        to_sync_point_id,
        &result
            .content
            .iter()
            .map(|r| r.readlist_id.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

// endregion
