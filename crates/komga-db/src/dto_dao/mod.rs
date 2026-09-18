//! DTO query layer, ported from komga's `infrastructure/jooq/main/*DtoDao.kt` and the
//! DTO-returning queries of `SeriesCollectionDao` / `ReadListDao` / `ReferentialDao` /
//! `ReadProgressDtoDao`.
//!
//! These return API-shaped structs (`komga_core::dto`) rather than domain models, with
//! content-restriction and library filtering applied in SQL, mirroring the jOOQ queries.

pub mod book;
pub mod collection;
pub mod read_progress;
pub mod readlist;
pub mod referential;
pub mod series;

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

/// Placeholder for `LuceneHelper.searchEntitiesIds` until the tantivy index lands (M6):
/// a blank term means no filtering (None); a non-blank term can never match without an index.
pub fn lucene_ids_stub(term: Option<&str>) -> Option<Vec<String>> {
    match term {
        Some(t) if !t.trim().is_empty() => {
            tracing::warn!("full-text search is unavailable before M6; returning no results");
            Some(vec![])
        }
        _ => None,
    }
}
