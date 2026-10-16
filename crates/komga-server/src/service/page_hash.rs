//! `PageHashLifecycle.kt`: page-hash lookups and registration.

use crate::state::AppState;
use komga_core::model::book::Book;
use komga_core::model::page_hash::{PageHashAction, PageHashKnown};
use komga_core::task::BookPageNumbered;
use komga_db::dao::book::BookDao;
use komga_db::dao::page_hash::PageHashDao;
use komga_db::dto_dao::PageRequest;
use komga_media::container::get_book_page;
use komga_media::PageContent;
use std::collections::BTreeMap;

/// `getBookIdsWithMissingPageHash` lives in the task processor (single SQL there);
/// the remaining lifecycle lives here.
/// `PageHashLifecycle.getPage`: the image bytes of any page carrying this hash (first match).
pub fn get_page(
    state: &AppState,
    page_hash: &str,
    resize_to: Option<u32>,
) -> komga_db::Result<Option<PageContent>> {
    let first = PageHashDao::new(state.db.clone())
        .find_matches_by_hash_paged(
            page_hash,
            &PageRequest {
                page: 0,
                size: 1,
                unpaged: false,
                sort: vec![],
            },
        )?
        .items
        .into_iter()
        .next();
    let Some(match_) = first else { return Ok(None) };
    let Some(book) = BookDao::new(state.db.clone()).find_by_id(&match_.book_id)? else {
        return Ok(None);
    };
    let page = get_book_page(
        &book_path(&book),
        &book.name,
        &media_of(state, &book)?,
        match_.page_number as usize,
        None,
        resize_to,
    )
    .map_err(|e| komga_db::Error::EnumValue(e.to_string()))?;
    Ok(Some(page))
}

/// `PageHashLifecycle.getBookPagesToDeleteAutomatically`: pages registered with DELETE_AUTO,
/// grouped by book id.
#[allow(dead_code)] // called by the task processor's FindDuplicatePagesToDelete branch
pub fn get_book_pages_to_delete_automatically(
    state: &AppState,
    library_id: &str,
) -> komga_db::Result<BTreeMap<String, Vec<BookPageNumbered>>> {
    PageHashDao::new(state.db.clone())
        .find_matches_by_known_hash_action(&[PageHashAction::DeleteAuto], Some(library_id))
}

/// `PageHashLifecycle.createOrUpdate`: register a known hash (with a small thumbnail on insert),
/// or update the action of an existing one.
pub fn create_or_update(state: &AppState, page_hash: &PageHashKnown) -> komga_db::Result<()> {
    let dao = PageHashDao::new(state.db.clone());
    match dao.find_known(&page_hash.hash)? {
        None => {
            let thumbnail = get_page(state, &page_hash.hash, Some(500))?.map(|p| p.bytes);
            dao.insert(page_hash, thumbnail.as_deref())?;
        }
        Some(existing) => {
            dao.update(&PageHashKnown {
                action: page_hash.action,
                ..existing
            })?;
        }
    }
    Ok(())
}

fn media_of(state: &AppState, book: &Book) -> komga_db::Result<komga_core::model::media::Media> {
    komga_db::dao::media::MediaDao::new(state.db.clone())
        .find_by_id(&book.id)?
        .ok_or_else(|| komga_db::Error::EnumValue(format!("no media for book {}", book.id)))
}

fn book_path(book: &Book) -> std::path::PathBuf {
    std::path::PathBuf::from(komga_core::dto::url_to_file_path(&book.url))
}
