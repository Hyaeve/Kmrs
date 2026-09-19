//! DTO query layer, ported from komga's `infrastructure/jooq/main/*DtoDao.kt` and the
//! DTO-returning queries of `SeriesCollectionDao` / `ReadListDao` / `ReferentialDao` /
//! `ReadProgressDtoDao`.
//!
//! These return API-shaped structs (`komga_core::dto`) rather than domain models, with
//! content-restriction and library filtering applied in SQL, mirroring the jOOQ queries.

pub mod book;
pub mod collection;
pub mod kobo;
pub mod read_progress;
pub mod readlist;
pub mod referential;
pub mod series;

use komga_core::task::LuceneEntity;
use std::sync::Arc;

/// Spring `Sort.Order`: property + direction.
#[derive(Debug, Clone)]
pub struct SortOrder {
    pub property: String,
    pub descending: bool,
}

/// Spring `Pageable` semantics for the DTO queries.
#[derive(Debug, Clone)]
pub struct PageRequest {
    /// 0-based page number
    pub page: u32,
    pub size: u32,
    pub unpaged: bool,
    pub sort: Vec<SortOrder>,
}

impl PageRequest {
    pub fn offset(&self) -> u64 {
        self.page as u64 * self.size as u64
    }
}

/// DTO query result. The Spring `PageImpl` JSON is assembled by the server layer;
/// `sorted` reports whether an ORDER BY was actually applied (drives `sort.sorted` in the JSON).
#[derive(Debug)]
pub struct DtoPage<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub sorted: bool,
}

/// `LuceneHelper.searchEntitiesIds` as a seam: the DTO queries call this for full-text terms.
pub trait EntitySearcher: Send + Sync {
    /// None means no filtering (blank term); Some(ids) filters to those ids (possibly empty).
    fn search_entity_ids(&self, term: Option<&str>, entity: LuceneEntity) -> Option<Vec<String>>;
}

/// `LuceneHelper.searchEntitiesIds`: blank terms pass through unfiltered; without a wired
/// searcher, non-blank terms match nothing (same failure mode as komga without an index).
pub fn search_entity_ids(
    searcher: &Option<Arc<dyn EntitySearcher>>,
    term: Option<&str>,
    entity: LuceneEntity,
) -> Option<Vec<String>> {
    match term {
        Some(t) if !t.trim().is_empty() => match searcher {
            Some(searcher) => searcher.search_entity_ids(term, entity),
            None => {
                tracing::warn!(
                    "full-text search is unavailable (no index wired); returning no results"
                );
                Some(vec![])
            }
        },
        _ => None,
    }
}
