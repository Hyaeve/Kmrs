//! Smart list lifecycle (kmrs-only enhancement with no Java equivalent): CRUD for
//! user-owned persisted search filters living in `kmrs.sqlite`.

use crate::events::DomainEvent;
use crate::state::AppState;
use komga_core::model::smart_list::{SmartList, SmartListTarget};
use komga_core::search::{
    BookSearch, SearchConditionBook, SearchConditionSeries, SearchContext, SeriesSearch,
};
use komga_db::dao::smart_list::SmartListDao;
use komga_db::dao::smart_list_thumbnail::SmartListThumbnailDao;
use komga_db::dao::user::UserDao;
use komga_db::dto_dao::book::BookDtoDao;
use komga_db::dto_dao::series::SeriesDtoDao;
use komga_db::dto_dao::PageRequest;

pub const DUPLICATE_NAME_MESSAGE: &str = "Smart list name already exists";

#[derive(Debug)]
pub enum SmartListError {
    DuplicateName,
    Db(komga_db::Error),
}

impl From<komga_db::Error> for SmartListError {
    fn from(e: komga_db::Error) -> Self {
        Self::Db(e)
    }
}

pub type Result<T> = std::result::Result<T, SmartListError>;

pub fn dao(state: &AppState) -> SmartListDao {
    SmartListDao::new(state.kmrs_db.clone())
}

/// Combines the stored filter with an overlay sent by the client (page-side
/// filters on the detail page). Both conditions AND together; an overlay
/// full-text term replaces the stored one, matching how a fresh search box
/// input supersedes the saved query.
pub fn merge_book_search(stored: &BookSearch, overlay: Option<&BookSearch>) -> BookSearch {
    let overlay = overlay.filter(|o| {
        o.condition.is_some()
            || o.full_text_search
                .as_deref()
                .is_some_and(|q| !q.trim().is_empty())
    });
    BookSearch {
        condition: match (
            stored.condition.as_ref(),
            overlay.and_then(|o| o.condition.as_ref()),
        ) {
            (Some(a), Some(b)) => Some(SearchConditionBook::AllOf {
                conditions: vec![a.clone(), b.clone()],
            }),
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        },
        full_text_search: overlay
            .and_then(|o| o.full_text_search.clone())
            .or_else(|| stored.full_text_search.clone()),
    }
}

pub fn merge_series_search(stored: &SeriesSearch, overlay: Option<&SeriesSearch>) -> SeriesSearch {
    let overlay = overlay.filter(|o| {
        o.condition.is_some()
            || o.full_text_search
                .as_deref()
                .is_some_and(|q| !q.trim().is_empty())
    });
    SeriesSearch {
        condition: match (
            stored.condition.as_ref(),
            overlay.and_then(|o| o.condition.as_ref()),
        ) {
            (Some(a), Some(b)) => Some(SearchConditionSeries::AllOf {
                conditions: vec![a.clone(), b.clone()],
            }),
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        },
        full_text_search: overlay
            .and_then(|o| o.full_text_search.clone())
            .or_else(|| stored.full_text_search.clone()),
    }
}

pub fn add_smart_list(
    state: &AppState,
    smart_list: SmartList,
    shared_with: &[String],
) -> Result<SmartList> {
    tracing::info!("Adding new smart list: {}", smart_list.name);
    let dao = dao(state);
    if dao.exists_by_name(&smart_list.owner_user_id, &smart_list.name)? {
        return Err(SmartListError::DuplicateName);
    }
    let id = dao.insert(&smart_list)?;
    dao.set_shares(&id, shared_with)?;
    let created = dao
        .find_by_id(&id)?
        .expect("smart list not found after insert");
    let _ = state
        .events
        .send(DomainEvent::SmartListAdded(created.clone()));
    Ok(created)
}

pub fn update_smart_list(
    state: &AppState,
    to_update: &SmartList,
    shared_with: Option<&[String]>,
) -> Result<()> {
    tracing::info!("Update smart list: {}", to_update.name);
    let dao = dao(state);
    let existing = dao
        .find_by_id(&to_update.id)?
        .expect("cannot update smart list that does not exist");
    if !existing.name.eq_ignore_ascii_case(&to_update.name)
        && dao.exists_by_name(&to_update.owner_user_id, &to_update.name)?
    {
        return Err(SmartListError::DuplicateName);
    }
    dao.update(to_update)?;
    // None keeps the current scope (PATCH semantics), Some replaces it
    if let Some(shared_with) = shared_with {
        dao.set_shares(&to_update.id, shared_with)?;
    }
    let _ = state
        .events
        .send(DomainEvent::SmartListUpdated(to_update.clone()));
    Ok(())
}

pub fn delete_smart_list(state: &AppState, smart_list: &SmartList) -> komga_db::Result<()> {
    let dao = dao(state);
    dao.delete(&smart_list.id)?;
    SmartListThumbnailDao::new(state.kmrs_db.clone()).delete_by_smart_list_id(&smart_list.id)?;
    let _ = state
        .events
        .send(DomainEvent::SmartListDeleted(smart_list.clone()));
    Ok(())
}

fn notify_thumbnail_changed(state: &AppState, smart_list: &SmartList) {
    let _ = state.events.send(DomainEvent::SmartListThumbnailChanged {
        smart_list_id: smart_list.id.clone(),
        user_id: smart_list.owner_user_id.clone(),
    });
}

pub fn add_thumbnail(
    state: &AppState,
    smart_list: &SmartList,
    thumbnail: komga_core::model::thumbnail::ThumbnailSmartList,
) -> komga_db::Result<komga_core::model::thumbnail::ThumbnailSmartList> {
    let dao = SmartListThumbnailDao::new(state.kmrs_db.clone());
    let id = dao.insert(&thumbnail)?;
    let mut thumbnail = thumbnail;
    thumbnail.id = id;
    if thumbnail.selected {
        dao.mark_selected(&thumbnail)?;
    }
    notify_thumbnail_changed(state, smart_list);
    Ok(thumbnail)
}

pub fn mark_selected_thumbnail(
    state: &AppState,
    smart_list: &SmartList,
    thumbnail: &komga_core::model::thumbnail::ThumbnailSmartList,
) -> komga_db::Result<()> {
    SmartListThumbnailDao::new(state.kmrs_db.clone()).mark_selected(thumbnail)?;
    notify_thumbnail_changed(state, smart_list);
    Ok(())
}

pub fn delete_thumbnail(
    state: &AppState,
    smart_list: &SmartList,
    thumbnail: &komga_core::model::thumbnail::ThumbnailSmartList,
) -> komga_db::Result<()> {
    SmartListThumbnailDao::new(state.kmrs_db.clone()).delete(&thumbnail.id)?;
    notify_thumbnail_changed(state, smart_list);
    Ok(())
}

/// The selected cover wins over the generated mosaic; the mosaic is cached in the kmrs
/// database together with a content fingerprint, and only rebuilt when the fingerprint
/// changes. The fingerprint is best-effort: it hashes at most the first 24 matched ids
/// in the query's (unsorted) order, so content changes deeper in the match list do not
/// invalidate the cache. Evaluation uses the owner's view (a deleted owner falls back
/// to an unrestricted context, same as list evaluation): the items endpoints evaluate
/// per requesting user, but the shared mosaic is built once from the owner's context —
/// a sharee with stricter library or age restrictions may therefore see covers in a
/// mosaic that their own filter would hide, an accepted trade-off of sharing one cache.
pub fn get_thumbnail_bytes(state: &AppState, smart_list: &SmartList) -> komga_db::Result<Vec<u8>> {
    if let Some(selected) = SmartListThumbnailDao::new(state.kmrs_db.clone())
        .find_selected_by_smart_list_id(&smart_list.id)?
    {
        return Ok(selected.thumbnail);
    }

    let owner = UserDao::new(state.db.clone()).find_by_id(&smart_list.owner_user_id)?;
    let ctx = match &owner {
        Some(owner) => SearchContext::of_user(owner),
        // orphaned list (owner deleted): evaluate without restrictions rather than failing
        None => SearchContext::default(),
    };
    let user_id = owner.map(|o| o.id).unwrap_or_default();

    // the fingerprint prefix keeps caches apart across targets
    let (prefix, ids): (&str, Vec<String>) = match smart_list.target {
        SmartListTarget::Book => {
            let search: BookSearch =
                serde_json::from_str(&smart_list.search_json).map_err(|e| {
                    komga_db::Error::EnumValue(format!("corrupt smart list search: {e}"))
                })?;
            let page = BookDtoDao::new(state.db.clone())
                .with_searcher(Some(crate::search_index::searcher(state)))
                .find_all(
                    &search,
                    &ctx,
                    &PageRequest {
                        page: 0,
                        size: 24,
                        unpaged: false,
                        sort: Vec::new(),
                    },
                )?;
            ("book:", page.items.into_iter().map(|b| b.id).collect())
        }
        SmartListTarget::Series => {
            let search: SeriesSearch =
                serde_json::from_str(&smart_list.search_json).map_err(|e| {
                    komga_db::Error::EnumValue(format!("corrupt smart list search: {e}"))
                })?;
            let page = SeriesDtoDao::new(state.db.clone())
                .with_searcher(Some(crate::search_index::searcher(state)))
                .find_all(
                    &search,
                    None,
                    &ctx,
                    &PageRequest {
                        page: 0,
                        size: 24,
                        unpaged: false,
                        sort: Vec::new(),
                    },
                )?;
            ("series:", page.items.into_iter().map(|s| s.id).collect())
        }
    };
    let fingerprint = format!("{prefix}{}", ids.join(","));

    let dao = SmartListThumbnailDao::new(state.kmrs_db.clone());
    if let Some(cached) = dao.find_generated_by_smart_list_id(&smart_list.id, &fingerprint)? {
        return Ok(cached.thumbnail);
    }

    let mut images = Vec::new();
    for id in ids.iter().take(4) {
        let bytes = match smart_list.target {
            SmartListTarget::Book => crate::service::book::get_thumbnail_bytes(state, id, None)?
                .map(|content| content.bytes),
            SmartListTarget::Series => {
                crate::service::series::get_thumbnail_bytes(state, id, &user_id)?
            }
        };
        if let Some(bytes) = bytes {
            images.push(bytes);
        }
    }
    let bytes = crate::service::collection::create_mosaic(
        &images,
        state.settings.get().thumbnail_size.max_edge(),
    )?;
    dao.upsert_generated(&smart_list.id, &bytes, &fingerprint)?;
    notify_thumbnail_changed(state, smart_list);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::search::{Equality, EqualityNullable, StringOp};

    fn tag_is(value: &str) -> SearchConditionBook {
        SearchConditionBook::Tag {
            tag: EqualityNullable::Is {
                value: value.into(),
            },
        }
    }

    #[test]
    fn merge_and_combines_conditions_with_allof() {
        let stored = BookSearch {
            condition: Some(tag_is("manga")),
            full_text_search: Some("manga".into()),
        };
        let overlay = BookSearch {
            condition: Some(SearchConditionBook::LibraryId {
                operator: Equality::Is {
                    value: "lib2".into(),
                },
            }),
            full_text_search: Some("naruto".into()),
        };
        let merged = merge_book_search(&stored, Some(&overlay));
        let SearchConditionBook::AllOf { conditions } = merged.condition.unwrap() else {
            panic!("expected allOf");
        };
        assert_eq!(conditions.len(), 2);
        assert_eq!(conditions[0], tag_is("manga"));
        // the overlay term replaces the stored one
        assert_eq!(merged.full_text_search.as_deref(), Some("naruto"));
    }

    #[test]
    fn merge_without_overlay_keeps_the_stored_search() {
        let stored = BookSearch {
            condition: Some(tag_is("manga")),
            full_text_search: Some("manga".into()),
        };
        let merged = merge_book_search(&stored, None);
        assert_eq!(merged, stored);
        // an empty overlay behaves like no overlay
        let empty = BookSearch {
            condition: None,
            full_text_search: None,
        };
        assert_eq!(merge_book_search(&stored, Some(&empty)), stored);
    }

    #[test]
    fn merge_only_overlay_takes_its_parts() {
        let stored = SeriesSearch {
            condition: None,
            full_text_search: None,
        };
        let overlay = SeriesSearch {
            condition: Some(SearchConditionSeries::Title {
                title: StringOp::Contains { value: "x".into() },
            }),
            full_text_search: None,
        };
        let merged = merge_series_search(&stored, Some(&overlay));
        assert_eq!(merged.condition, overlay.condition);
        assert!(merged.full_text_search.is_none());
    }
}
