//! `ReferentialDao.kt`: referential (v1 and v2) queries.
//!
//! v1 methods return plain lists/sets; v2 methods go through `find_generic`, mirroring the
//! jOOQ query shape: table-driven joins, restriction/library/filter conditions, DISTINCT
//! select, count + page.

use crate::dto_dao::{DtoPage, PageRequest};
use crate::error::Result;
use crate::pool::Database;
use crate::search_sql::{content_restrictions_condition, RequiredJoin};
use komga_core::dto::common::AuthorDto;
use komga_core::model::common::Author;
use komga_core::natural_sort::strip_accents;
use komga_core::search::{FilterBy, FilterByEntity, FilterTags, SearchContext};
use rusqlite::types::Value;
use rusqlite::Row;
use std::collections::BTreeSet;
use time::Date;

const U3: &str = "COLLATE COLLATION_UNICODE_3";

pub struct ReferentialDao {
    db: Database,
}

impl ReferentialDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    // region v1: authors

    pub fn find_all_authors_by_name(
        &self,
        search: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Author>> {
        let mut q = Query::new(
            "SELECT DISTINCT BOOK_METADATA_AUTHOR.NAME, BOOK_METADATA_AUTHOR.ROLE FROM BOOK_METADATA_AUTHOR",
        );
        q.where_contains("BOOK_METADATA_AUTHOR.NAME", search);
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN BOOK ON BOOK_METADATA_AUTHOR.BOOK_ID = BOOK.ID");
            q.where_in("BOOK.LIBRARY_ID", ids);
        }
        q.order_by(&format!("BOOK_METADATA_AUTHOR.NAME {U3}"));
        self.fetch_authors(q)
    }

    pub fn find_all_authors_by_name_and_library(
        &self,
        search: &str,
        library_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Author>> {
        let mut q = Query::new(
            "SELECT DISTINCT BOOK_METADATA_AGGREGATION_AUTHOR.NAME, BOOK_METADATA_AGGREGATION_AUTHOR.ROLE \
             FROM BOOK_METADATA_AGGREGATION_AUTHOR LEFT JOIN SERIES ON BOOK_METADATA_AGGREGATION_AUTHOR.SERIES_ID = SERIES.ID",
        );
        q.where_contains("BOOK_METADATA_AGGREGATION_AUTHOR.NAME", search);
        q.where_eq("SERIES.LIBRARY_ID", library_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("BOOK_METADATA_AGGREGATION_AUTHOR.NAME {U3}"));
        self.fetch_authors(q)
    }

    pub fn find_all_authors_by_name_and_collection(
        &self,
        search: &str,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Author>> {
        let mut q = Query::new(
            "SELECT DISTINCT BOOK_METADATA_AGGREGATION_AUTHOR.NAME, BOOK_METADATA_AGGREGATION_AUTHOR.ROLE \
             FROM BOOK_METADATA_AGGREGATION_AUTHOR \
             LEFT JOIN COLLECTION_SERIES ON BOOK_METADATA_AGGREGATION_AUTHOR.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        if filter_library_ids.is_some() {
            q.from.push_str(
                " LEFT JOIN SERIES ON BOOK_METADATA_AGGREGATION_AUTHOR.SERIES_ID = SERIES.ID",
            );
        }
        q.where_contains("BOOK_METADATA_AGGREGATION_AUTHOR.NAME", search);
        q.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("BOOK_METADATA_AGGREGATION_AUTHOR.NAME {U3}"));
        self.fetch_authors(q)
    }

    pub fn find_all_authors_by_name_and_series(
        &self,
        search: &str,
        series_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Author>> {
        let mut q = Query::new(
            "SELECT DISTINCT BOOK_METADATA_AGGREGATION_AUTHOR.NAME, BOOK_METADATA_AGGREGATION_AUTHOR.ROLE \
             FROM BOOK_METADATA_AGGREGATION_AUTHOR",
        );
        if filter_library_ids.is_some() {
            q.from.push_str(
                " LEFT JOIN SERIES ON BOOK_METADATA_AGGREGATION_AUTHOR.SERIES_ID = SERIES.ID",
            );
        }
        q.where_contains("BOOK_METADATA_AGGREGATION_AUTHOR.NAME", search);
        q.where_eq("BOOK_METADATA_AGGREGATION_AUTHOR.SERIES_ID", series_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("BOOK_METADATA_AGGREGATION_AUTHOR.NAME {U3}"));
        self.fetch_authors(q)
    }

    pub fn find_all_authors_names_by_name(
        &self,
        search: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q =
            Query::new("SELECT DISTINCT BOOK_METADATA_AUTHOR.NAME FROM BOOK_METADATA_AUTHOR");
        q.where_contains("BOOK_METADATA_AUTHOR.NAME", search);
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN BOOK ON BOOK_METADATA_AUTHOR.BOOK_ID = BOOK.ID");
            q.where_in("BOOK.LIBRARY_ID", ids);
        }
        q.order_by(&format!("BOOK_METADATA_AUTHOR.NAME {U3}"));
        self.fetch_strings(q)
    }

    pub fn find_all_authors_roles(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q =
            Query::new("SELECT DISTINCT BOOK_METADATA_AUTHOR.ROLE FROM BOOK_METADATA_AUTHOR");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN BOOK ON BOOK_METADATA_AUTHOR.BOOK_ID = BOOK.ID");
            q.where_in("BOOK.LIBRARY_ID", ids);
        }
        q.order_by("BOOK_METADATA_AUTHOR.ROLE");
        self.fetch_strings(q)
    }

    // endregion

    // region v1: genres

    pub fn find_all_genres(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new("SELECT DISTINCT GENRE FROM SERIES_METADATA_GENRE");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA_GENRE.SERIES_ID = SERIES.ID");
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("GENRE {U3}"));
        self.fetch_strings(q)
    }

    pub fn find_all_genres_by_libraries(
        &self,
        library_ids: &BTreeSet<String>,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT DISTINCT GENRE FROM SERIES_METADATA_GENRE \
             LEFT JOIN SERIES ON SERIES_METADATA_GENRE.SERIES_ID = SERIES.ID",
        );
        q.where_in("SERIES.LIBRARY_ID", library_ids);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("GENRE {U3}"));
        self.fetch_strings(q)
    }

    pub fn find_all_genres_by_collection(
        &self,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT DISTINCT GENRE FROM SERIES_METADATA_GENRE \
             LEFT JOIN COLLECTION_SERIES ON SERIES_METADATA_GENRE.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        if filter_library_ids.is_some() {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA_GENRE.SERIES_ID = SERIES.ID");
        }
        q.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("GENRE {U3}"));
        self.fetch_strings(q)
    }

    // endregion

    // region v1: series and book tags

    pub fn find_all_series_and_book_tags(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut book_branch = Query::new("SELECT BOOK_METADATA_TAG.TAG FROM BOOK_METADATA_TAG");
        let mut series_branch =
            Query::new("SELECT SERIES_METADATA_TAG.TAG FROM SERIES_METADATA_TAG");
        if let Some(ids) = filter_library_ids {
            book_branch
                .from
                .push_str(" LEFT JOIN BOOK ON BOOK_METADATA_TAG.BOOK_ID = BOOK.ID");
            book_branch.where_in("BOOK.LIBRARY_ID", ids);
            series_branch
                .from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA_TAG.SERIES_ID = SERIES.ID");
            series_branch.where_in("SERIES.LIBRARY_ID", ids);
        }
        let (sql, params) = union_of(book_branch, series_branch);
        self.fetch_sorted_tags(&sql, params)
    }

    pub fn find_all_series_and_book_tags_by_libraries(
        &self,
        library_ids: &BTreeSet<String>,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut book_branch = Query::new(
            "SELECT BOOK_METADATA_TAG.TAG FROM BOOK_METADATA_TAG \
             LEFT JOIN BOOK ON BOOK_METADATA_TAG.BOOK_ID = BOOK.ID",
        );
        book_branch.where_in("BOOK.LIBRARY_ID", library_ids);
        let mut series_branch = Query::new(
            "SELECT SERIES_METADATA_TAG.TAG FROM SERIES_METADATA_TAG \
             LEFT JOIN SERIES ON SERIES_METADATA_TAG.SERIES_ID = SERIES.ID",
        );
        series_branch.where_in("SERIES.LIBRARY_ID", library_ids);
        if let Some(ids) = filter_library_ids {
            book_branch.where_in("BOOK.LIBRARY_ID", ids);
            series_branch.where_in("SERIES.LIBRARY_ID", ids);
        }
        let (sql, params) = union_of(book_branch, series_branch);
        self.fetch_sorted_tags(&sql, params)
    }

    pub fn find_all_series_and_book_tags_by_collection(
        &self,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        // the book side goes through the aggregation table here, not BOOK_METADATA_TAG
        let mut book_branch = Query::new(
            "SELECT BOOK_METADATA_AGGREGATION_TAG.TAG FROM BOOK_METADATA_AGGREGATION_TAG \
             LEFT JOIN SERIES ON BOOK_METADATA_AGGREGATION_TAG.SERIES_ID = SERIES.ID \
             LEFT JOIN COLLECTION_SERIES ON BOOK_METADATA_AGGREGATION_TAG.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        book_branch.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        let mut series_branch = Query::new(
            "SELECT SERIES_METADATA_TAG.TAG FROM SERIES_METADATA_TAG \
             LEFT JOIN COLLECTION_SERIES ON SERIES_METADATA_TAG.SERIES_ID = COLLECTION_SERIES.SERIES_ID \
             LEFT JOIN SERIES ON SERIES_METADATA_TAG.SERIES_ID = SERIES.ID",
        );
        series_branch.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        if let Some(ids) = filter_library_ids {
            book_branch.where_in("SERIES.LIBRARY_ID", ids);
            series_branch.where_in("SERIES.LIBRARY_ID", ids);
        }
        let (sql, params) = union_of(book_branch, series_branch);
        self.fetch_sorted_tags(&sql, params)
    }

    // endregion

    // region v1: series tags

    pub fn find_all_series_tags(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new("SELECT TAG FROM SERIES_METADATA_TAG");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA_TAG.SERIES_ID = SERIES.ID");
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("TAG {U3}"));
        self.fetch_strings_dedup(q)
    }

    pub fn find_all_series_tags_by_library(
        &self,
        library_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT TAG FROM SERIES_METADATA_TAG \
             LEFT JOIN SERIES ON SERIES_METADATA_TAG.SERIES_ID = SERIES.ID",
        );
        q.where_eq("SERIES.LIBRARY_ID", library_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("TAG {U3}"));
        self.fetch_strings_dedup(q)
    }

    pub fn find_all_series_tags_by_collection(
        &self,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT TAG FROM SERIES_METADATA_TAG \
             LEFT JOIN COLLECTION_SERIES ON SERIES_METADATA_TAG.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        if filter_library_ids.is_some() {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA_TAG.SERIES_ID = SERIES.ID");
        }
        q.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("TAG {U3}"));
        self.fetch_strings_dedup(q)
    }

    // endregion

    // region v1: book tags

    pub fn find_all_book_tags(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new("SELECT TAG FROM BOOK_METADATA_TAG");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN BOOK ON BOOK_METADATA_TAG.BOOK_ID = BOOK.ID");
            q.where_in("BOOK.LIBRARY_ID", ids);
        }
        q.order_by(&format!("TAG {U3}"));
        self.fetch_strings_dedup(q)
    }

    pub fn find_all_book_tags_by_series(
        &self,
        series_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT TAG FROM BOOK_METADATA_TAG \
             LEFT JOIN BOOK ON BOOK_METADATA_TAG.BOOK_ID = BOOK.ID",
        );
        q.where_eq("BOOK.SERIES_ID", series_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("BOOK.LIBRARY_ID", ids);
        }
        q.order_by(&format!("TAG {U3}"));
        self.fetch_strings_dedup(q)
    }

    pub fn find_all_book_tags_by_readlist(
        &self,
        readlist_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT TAG FROM BOOK_METADATA_TAG \
             LEFT JOIN BOOK ON BOOK_METADATA_TAG.BOOK_ID = BOOK.ID \
             LEFT JOIN READLIST_BOOK ON BOOK_METADATA_TAG.BOOK_ID = READLIST_BOOK.BOOK_ID",
        );
        q.where_eq("READLIST_BOOK.READLIST_ID", readlist_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("BOOK.LIBRARY_ID", ids);
        }
        q.order_by(&format!("TAG {U3}"));
        self.fetch_strings_dedup(q)
    }

    // endregion

    // region v1: languages

    pub fn find_all_languages(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new("SELECT DISTINCT LANGUAGE FROM SERIES_METADATA");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID");
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.where_raw("LANGUAGE <> ''");
        q.order_by("LANGUAGE");
        self.fetch_strings(q)
    }

    pub fn find_all_languages_by_libraries(
        &self,
        library_ids: &BTreeSet<String>,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT DISTINCT LANGUAGE FROM SERIES_METADATA \
             LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID",
        );
        q.where_raw("LANGUAGE <> ''");
        q.where_in("SERIES.LIBRARY_ID", library_ids);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by("LANGUAGE");
        self.fetch_strings(q)
    }

    pub fn find_all_languages_by_collection(
        &self,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT DISTINCT LANGUAGE FROM SERIES_METADATA \
             LEFT JOIN COLLECTION_SERIES ON SERIES_METADATA.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        if filter_library_ids.is_some() {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID");
        }
        q.where_raw("LANGUAGE <> ''");
        q.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by("LANGUAGE");
        self.fetch_strings(q)
    }

    // endregion

    // region v1: publishers

    pub fn find_all_publishers(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new("SELECT DISTINCT PUBLISHER FROM SERIES_METADATA");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID");
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.where_raw("PUBLISHER <> ''");
        q.order_by(&format!("PUBLISHER {U3}"));
        self.fetch_strings(q)
    }

    pub fn find_all_publishers_by_libraries(
        &self,
        library_ids: &BTreeSet<String>,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT DISTINCT PUBLISHER FROM SERIES_METADATA \
             LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID",
        );
        q.where_raw("PUBLISHER <> ''");
        q.where_in("SERIES.LIBRARY_ID", library_ids);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("PUBLISHER {U3}"));
        self.fetch_strings(q)
    }

    pub fn find_all_publishers_by_collection(
        &self,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT DISTINCT PUBLISHER FROM SERIES_METADATA \
             LEFT JOIN COLLECTION_SERIES ON SERIES_METADATA.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        if filter_library_ids.is_some() {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID");
        }
        q.where_raw("PUBLISHER <> ''");
        q.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("PUBLISHER {U3}"));
        self.fetch_strings(q)
    }

    // endregion

    // region v1: age ratings

    pub fn find_all_age_ratings(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Option<i32>>> {
        let mut q = Query::new("SELECT DISTINCT AGE_RATING FROM SERIES_METADATA");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID");
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by("AGE_RATING");
        self.fetch_ints(q)
    }

    pub fn find_all_age_ratings_by_libraries(
        &self,
        library_ids: &BTreeSet<String>,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Option<i32>>> {
        let mut q = Query::new(
            "SELECT DISTINCT AGE_RATING FROM SERIES_METADATA \
             LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID",
        );
        q.where_in("SERIES.LIBRARY_ID", library_ids);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by("AGE_RATING");
        self.fetch_ints(q)
    }

    pub fn find_all_age_ratings_by_collection(
        &self,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Option<i32>>> {
        let mut q = Query::new(
            "SELECT DISTINCT AGE_RATING FROM SERIES_METADATA \
             LEFT JOIN COLLECTION_SERIES ON SERIES_METADATA.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        if filter_library_ids.is_some() {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA.SERIES_ID = SERIES.ID");
        }
        q.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by("AGE_RATING");
        self.fetch_ints(q)
    }

    // endregion

    // region v1: series release dates

    pub fn find_all_series_release_dates(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Date>> {
        let mut q = Query::new("SELECT DISTINCT RELEASE_DATE FROM BOOK_METADATA_AGGREGATION");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN SERIES ON BOOK_METADATA_AGGREGATION.SERIES_ID = SERIES.ID");
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.where_raw("RELEASE_DATE IS NOT NULL");
        q.order_by("RELEASE_DATE DESC");
        self.fetch_dates(q)
    }

    pub fn find_all_series_release_dates_by_libraries(
        &self,
        library_ids: &BTreeSet<String>,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Date>> {
        let mut q = Query::new(
            "SELECT DISTINCT RELEASE_DATE FROM BOOK_METADATA_AGGREGATION \
             LEFT JOIN SERIES ON BOOK_METADATA_AGGREGATION.SERIES_ID = SERIES.ID",
        );
        q.where_in("SERIES.LIBRARY_ID", library_ids);
        q.where_raw("RELEASE_DATE IS NOT NULL");
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by("RELEASE_DATE DESC");
        self.fetch_dates(q)
    }

    pub fn find_all_series_release_dates_by_collection(
        &self,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<Date>> {
        let mut q = Query::new(
            "SELECT DISTINCT RELEASE_DATE FROM BOOK_METADATA_AGGREGATION \
             LEFT JOIN COLLECTION_SERIES ON BOOK_METADATA_AGGREGATION.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        if filter_library_ids.is_some() {
            q.from
                .push_str(" LEFT JOIN SERIES ON BOOK_METADATA_AGGREGATION.SERIES_ID = SERIES.ID");
        }
        q.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        q.where_raw("RELEASE_DATE IS NOT NULL");
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by("RELEASE_DATE DESC");
        self.fetch_dates(q)
    }

    // endregion

    // region v1: sharing labels

    pub fn find_all_sharing_labels(
        &self,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new("SELECT DISTINCT LABEL FROM SERIES_METADATA_SHARING");
        if let Some(ids) = filter_library_ids {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA_SHARING.SERIES_ID = SERIES.ID");
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("LABEL {U3}"));
        self.fetch_strings(q)
    }

    pub fn find_all_sharing_labels_by_libraries(
        &self,
        library_ids: &BTreeSet<String>,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT DISTINCT LABEL FROM SERIES_METADATA_SHARING \
             LEFT JOIN SERIES ON SERIES_METADATA_SHARING.SERIES_ID = SERIES.ID",
        );
        q.where_in("SERIES.LIBRARY_ID", library_ids);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("LABEL {U3}"));
        self.fetch_strings(q)
    }

    pub fn find_all_sharing_labels_by_collection(
        &self,
        collection_id: &str,
        filter_library_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<String>> {
        let mut q = Query::new(
            "SELECT DISTINCT LABEL FROM SERIES_METADATA_SHARING \
             LEFT JOIN COLLECTION_SERIES ON SERIES_METADATA_SHARING.SERIES_ID = COLLECTION_SERIES.SERIES_ID",
        );
        if filter_library_ids.is_some() {
            q.from
                .push_str(" LEFT JOIN SERIES ON SERIES_METADATA_SHARING.SERIES_ID = SERIES.ID");
        }
        q.where_eq("COLLECTION_SERIES.COLLECTION_ID", collection_id);
        if let Some(ids) = filter_library_ids {
            q.where_in("SERIES.LIBRARY_ID", ids);
        }
        q.order_by(&format!("LABEL {U3}"));
        self.fetch_strings(q)
    }

    // endregion

    // region v2

    pub fn find_authors(
        &self,
        ctx: &SearchContext,
        search: Option<&str>,
        role: Option<&str>,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<AuthorDto>> {
        let extra_condition = role.map(|r| {
            (
                "BOOK_METADATA_AUTHOR.ROLE = ?".to_string(),
                vec![Value::Text(r.to_string())],
            )
        });
        self.find_generic(
            ctx,
            search,
            filter_by,
            page,
            "BOOK_METADATA_AUTHOR",
            Some("BOOK_METADATA_AUTHOR.NAME"),
            None,
            Some("BOOK_METADATA_AUTHOR.BOOK_ID"),
            &["BOOK_METADATA_AUTHOR.ROLE"],
            extra_condition,
            None,
            |row| {
                let name: String = row.get(0)?;
                let role: String = row.get(1)?;
                Ok(Some(AuthorDto::from(&Author::new(&name, &role))))
            },
        )
    }

    pub fn find_authors_roles(
        &self,
        ctx: &SearchContext,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<String>> {
        self.find_generic(
            ctx,
            None,
            filter_by,
            page,
            "BOOK_METADATA_AUTHOR",
            None,
            None,
            Some("BOOK_METADATA_AUTHOR.BOOK_ID"),
            &["BOOK_METADATA_AUTHOR.ROLE"],
            None,
            Some("BOOK_METADATA_AUTHOR.ROLE"),
            |row| Ok(Some(row.get::<_, String>(0)?)),
        )
    }

    pub fn find_authors_names(
        &self,
        ctx: &SearchContext,
        search: Option<&str>,
        role: Option<&str>,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<String>> {
        let extra_condition = role.map(|r| {
            (
                "BOOK_METADATA_AUTHOR.ROLE = ?".to_string(),
                vec![Value::Text(r.to_string())],
            )
        });
        self.find_generic(
            ctx,
            search,
            filter_by,
            page,
            "BOOK_METADATA_AUTHOR",
            Some("BOOK_METADATA_AUTHOR.NAME"),
            None,
            Some("BOOK_METADATA_AUTHOR.BOOK_ID"),
            &[],
            extra_condition,
            None,
            |row| Ok(Some(row.get::<_, String>(0)?)),
        )
    }

    pub fn find_genres(
        &self,
        ctx: &SearchContext,
        search: Option<&str>,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<String>> {
        require_library_or_collection(filter_by);
        self.find_generic(
            ctx,
            search,
            filter_by,
            page,
            "SERIES_METADATA_GENRE",
            Some("GENRE"),
            Some("SERIES_METADATA_GENRE.SERIES_ID"),
            None,
            &[],
            None,
            None,
            |row| Ok(Some(row.get::<_, String>(0)?)),
        )
    }

    pub fn find_sharing_labels(
        &self,
        ctx: &SearchContext,
        search: Option<&str>,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<String>> {
        require_library_or_collection(filter_by);
        self.find_generic(
            ctx,
            search,
            filter_by,
            page,
            "SERIES_METADATA_SHARING",
            Some("LABEL"),
            Some("SERIES_METADATA_SHARING.SERIES_ID"),
            None,
            &[],
            None,
            None,
            |row| Ok(Some(row.get::<_, String>(0)?)),
        )
    }

    pub fn find_languages(
        &self,
        ctx: &SearchContext,
        search: Option<&str>,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<String>> {
        require_library_or_collection(filter_by);
        self.find_generic(
            ctx,
            search,
            filter_by,
            page,
            "SERIES_METADATA",
            Some("LANGUAGE"),
            Some("SERIES_METADATA.SERIES_ID"),
            None,
            &[],
            Some(("LANGUAGE <> ''".to_string(), vec![])),
            None,
            |row| Ok(Some(row.get::<_, String>(0)?)),
        )
    }

    pub fn find_publishers(
        &self,
        ctx: &SearchContext,
        search: Option<&str>,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<String>> {
        require_library_or_collection(filter_by);
        self.find_generic(
            ctx,
            search,
            filter_by,
            page,
            "SERIES_METADATA",
            Some("PUBLISHER"),
            Some("SERIES_METADATA.SERIES_ID"),
            None,
            &[],
            Some(("PUBLISHER <> ''".to_string(), vec![])),
            None,
            |row| Ok(Some(row.get::<_, String>(0)?)),
        )
    }

    pub fn find_tags(
        &self,
        ctx: &SearchContext,
        search: Option<&str>,
        filter_by: Option<&FilterBy>,
        include_tags: FilterTags,
        page: &PageRequest,
    ) -> Result<DtoPage<String>> {
        let (table, series_id_field, book_id_field) = match include_tags {
            FilterTags::Series => (
                "SERIES_METADATA_TAG",
                Some("SERIES_METADATA_TAG.SERIES_ID"),
                None,
            ),
            FilterTags::Book => ("BOOK_METADATA_TAG", None, Some("BOOK_METADATA_TAG.BOOK_ID")),
            FilterTags::Both => (
                "SERIES_AND_BOOK_TAG",
                Some("SERIES_AND_BOOK_TAG.SERIES_ID"),
                None,
            ),
        };
        self.find_generic(
            ctx,
            search,
            filter_by,
            page,
            table,
            Some("TAG"),
            series_id_field,
            book_id_field,
            &[],
            None,
            None,
            |row| Ok(Some(row.get::<_, String>(0)?)),
        )
    }

    pub fn find_age_ratings(
        &self,
        ctx: &SearchContext,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<i32>> {
        require_library_or_collection(filter_by);
        // NULL age ratings are counted in the total but dropped from the items, as in komga
        self.find_generic(
            ctx,
            None,
            filter_by,
            page,
            "SERIES_METADATA",
            None,
            Some("SERIES_METADATA.SERIES_ID"),
            None,
            &["AGE_RATING"],
            None,
            Some("AGE_RATING"),
            |row| row.get::<_, Option<i32>>(0),
        )
    }

    pub fn find_series_release_years(
        &self,
        ctx: &SearchContext,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
    ) -> Result<DtoPage<String>> {
        require_library_or_collection(filter_by);
        let restriction = content_restrictions_condition(&ctx.restrictions);

        let mut q = Query::new(
            "SELECT DISTINCT CAST(STRFTIME('%Y', BOOK_METADATA_AGGREGATION.RELEASE_DATE) AS INTEGER) \
             FROM BOOK_METADATA_AGGREGATION",
        );
        if restriction.joins.contains(&RequiredJoin::SeriesMetadata) {
            q.from
            .push_str(" INNER JOIN SERIES_METADATA ON BOOK_METADATA_AGGREGATION.SERIES_ID = SERIES_METADATA.SERIES_ID");
        }
        let library_join = ctx.library_ids.as_ref().is_some_and(|ids| !ids.is_empty())
            || filter_by.map(|f| f.type_) == Some(FilterByEntity::Library);
        if library_join {
            q.from
                .push_str(" LEFT JOIN SERIES ON BOOK_METADATA_AGGREGATION.SERIES_ID = SERIES.ID");
        }
        if filter_by.map(|f| f.type_) == Some(FilterByEntity::Collection) {
            q.from
            .push_str(" LEFT JOIN COLLECTION_SERIES ON BOOK_METADATA_AGGREGATION.SERIES_ID = COLLECTION_SERIES.SERIES_ID");
        }
        // unreachable from the v2 controller (only LIBRARY/COLLECTION filters are accepted),
        // kept for parity with the Kotlin query
        if filter_by.map(|f| f.type_) == Some(FilterByEntity::ReadList) {
            q.from.push_str(
                " LEFT JOIN BOOK ON BOOK_METADATA_AGGREGATION.SERIES_ID = BOOK.SERIES_ID \
                        LEFT JOIN READLIST_BOOK ON BOOK.ID = READLIST_BOOK.BOOK_ID",
            );
        }

        if !restriction.sql.is_empty() {
            q.wheres.push(restriction.sql);
            q.params.extend(restriction.params);
        }
        if let Some(ids) = &ctx.library_ids {
            let (sql, params) = in_clause("SERIES.LIBRARY_ID", ids);
            q.wheres.push(sql);
            q.params.extend(params);
        }
        if let Some(fb) = filter_by {
            let field = match fb.type_ {
                FilterByEntity::Library => "SERIES.LIBRARY_ID",
                FilterByEntity::Collection => "COLLECTION_SERIES.COLLECTION_ID",
                FilterByEntity::Series => "BOOK_METADATA_AGGREGATION.SERIES_ID",
                FilterByEntity::ReadList => "READLIST_BOOK.READLIST_ID",
            };
            let (sql, params) = in_clause(field, &fb.ids);
            q.wheres.push(sql);
            q.params.extend(params);
        }

        let (where_sql, where_params) = q.where_parts();
        let count_sql = format!("SELECT COUNT(*) FROM ({}{})", q.from, where_sql);
        let total = self.count(&count_sql, &where_params)?;

        q.order_by("BOOK_METADATA_AGGREGATION.RELEASE_DATE DESC");
        let (sql, params) = q.paged_sql(page);
        // NULL release dates are counted in the total but dropped from the items, as in komga
        let items = self.fetch_map(&sql, params, |row| row.get::<_, Option<i64>>(0))?;
        let items = items
            .into_iter()
            .flatten()
            .map(|year| year.to_string())
            .collect();

        Ok(DtoPage {
            items,
            total,
            sorted: true,
        })
    }

    // endregion

    // region find_generic

    /// `ReferentialDao.findGeneric`: DISTINCT select over a referential table with
    /// restriction/library/filter conditions resolved through BOOK / SERIES joins.
    #[allow(clippy::too_many_arguments)]
    fn find_generic<T>(
        &self,
        ctx: &SearchContext,
        search: Option<&str>,
        filter_by: Option<&FilterBy>,
        page: &PageRequest,
        table: &str,
        searchable_field: Option<&str>,
        series_id_field: Option<&str>,
        book_id_field: Option<&str>,
        extra_fields: &[&str],
        extra_condition: Option<(String, Vec<Value>)>,
        sort_field: Option<&str>,
        mut map: impl FnMut(&Row<'_>) -> rusqlite::Result<Option<T>>,
    ) -> Result<DtoPage<T>> {
        assert!(
            series_id_field.is_some() || book_id_field.is_some(),
            "at least one of series_id_field/book_id_field is required"
        );
        let restriction = content_restrictions_condition(&ctx.restrictions);

        let series_id_required = matches!(
            filter_by.map(|f| f.type_),
            Some(FilterByEntity::Series | FilterByEntity::Collection | FilterByEntity::Library)
        ) || restriction.joins.contains(&RequiredJoin::SeriesMetadata)
            || ctx.library_ids.as_ref().is_some_and(|ids| !ids.is_empty());
        let book_id_required = filter_by.map(|f| f.type_) == Some(FilterByEntity::ReadList);

        let effective_series_id_field = series_id_field.unwrap_or("BOOK.SERIES_ID");
        let effective_book_id_field = book_id_field.unwrap_or("BOOK.ID");

        let mut fields: Vec<&str> = Vec::new();
        if let Some(f) = searchable_field {
            fields.push(f);
        }
        fields.extend_from_slice(extra_fields);
        let mut q = Query::new(&format!(
            "SELECT DISTINCT {} FROM {table}",
            fields.join(", ")
        ));

        if series_id_required && series_id_field.is_none() {
            q.from.push_str(&format!(
                " INNER JOIN BOOK ON {effective_book_id_field} = BOOK.ID"
            ));
        }
        if book_id_required && book_id_field.is_none() {
            q.from.push_str(&format!(
                " INNER JOIN BOOK ON {} = BOOK.SERIES_ID",
                series_id_field.unwrap()
            ));
        }
        if restriction.joins.contains(&RequiredJoin::SeriesMetadata) && table != "SERIES_METADATA" {
            q.from
            .push_str(&format!(" INNER JOIN SERIES_METADATA ON {effective_series_id_field} = SERIES_METADATA.SERIES_ID"));
        }
        if ctx.library_ids.as_ref().is_some_and(|ids| !ids.is_empty())
            || filter_by.map(|f| f.type_) == Some(FilterByEntity::Library)
        {
            q.from.push_str(&format!(
                " LEFT JOIN SERIES ON {effective_series_id_field} = SERIES.ID"
            ));
        }
        if filter_by.map(|f| f.type_) == Some(FilterByEntity::Collection) {
            q.from
            .push_str(&format!(" LEFT JOIN COLLECTION_SERIES ON {effective_series_id_field} = COLLECTION_SERIES.SERIES_ID"));
        }
        if filter_by.map(|f| f.type_) == Some(FilterByEntity::ReadList) {
            q.from.push_str(&format!(
                " LEFT JOIN READLIST_BOOK ON {effective_book_id_field} = READLIST_BOOK.BOOK_ID"
            ));
        }

        if !restriction.sql.is_empty() {
            q.wheres.push(restriction.sql);
            q.params.extend(restriction.params);
        }
        if let Some((sql, params)) = extra_condition {
            q.wheres.push(sql);
            q.params.extend(params);
        }
        if let (Some(search), Some(field)) = (search, searchable_field) {
            q.where_contains(field, search);
        }
        if let Some(ids) = &ctx.library_ids {
            let (sql, params) = in_clause("SERIES.LIBRARY_ID", ids);
            q.wheres.push(sql);
            q.params.extend(params);
        }
        if let Some(fb) = filter_by {
            let field = match fb.type_ {
                FilterByEntity::Library => "SERIES.LIBRARY_ID",
                FilterByEntity::Collection => "COLLECTION_SERIES.COLLECTION_ID",
                FilterByEntity::Series => effective_series_id_field,
                FilterByEntity::ReadList => "READLIST_BOOK.READLIST_ID",
            };
            let (sql, params) = in_clause(field, &fb.ids);
            q.wheres.push(sql);
            q.params.extend(params);
        }

        let (where_sql, where_params) = q.where_parts();
        let count_sql = format!("SELECT COUNT(*) FROM ({}{})", q.from, where_sql);
        let total = self.count(&count_sql, &where_params)?;

        let order = sort_field
            .map(str::to_string)
            .or_else(|| searchable_field.map(|f| format!("{f} {U3}")));
        if let Some(order) = order {
            q.order_by(&order);
        }
        let (sql, params) = q.paged_sql(page);

        let mut items = Vec::new();
        let conn = self.db.ro();
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        while let Some(row) = rows.next()? {
            if let Some(item) = map(row)? {
                items.push(item);
            }
        }

        // every v2 caller passes an explicit Sort in komga, so the page is always sorted
        Ok(DtoPage {
            items,
            total,
            sorted: true,
        })
    }

    // endregion

    // region row fetch helpers

    fn fetch_authors(&self, q: Query) -> Result<Vec<Author>> {
        let (sql, params) = q.into_sql();
        self.fetch_map(&sql, params, |row| {
            let name: String = row.get(0)?;
            let role: String = row.get(1)?;
            Ok(Author::new(&name, &role))
        })
    }

    fn fetch_strings(&self, q: Query) -> Result<Vec<String>> {
        let (sql, params) = q.into_sql();
        self.fetch_map(&sql, params, |row| row.get::<_, String>(0))
    }

    /// Plain (non-DISTINCT) selects whose duplicates are removed like jOOQ's `fetchSet`,
    /// keeping the SQL ORDER BY
    fn fetch_strings_dedup(&self, q: Query) -> Result<Vec<String>> {
        let (sql, params) = q.into_sql();
        let rows = self.fetch_map(&sql, params, |row| row.get::<_, String>(0))?;
        let mut seen = BTreeSet::new();
        Ok(rows
            .into_iter()
            .filter(|s| seen.insert(s.clone()))
            .collect())
    }

    /// UNION tag queries: jOOQ `fetchSet` then a stable sort by stripAccents + lowercase
    fn fetch_sorted_tags(&self, sql: &str, params: Vec<Value>) -> Result<Vec<String>> {
        let mut tags = self.fetch_map(sql, params, |row| row.get::<_, String>(0))?;
        tags.sort_by_key(|t| strip_accents(t).to_lowercase());
        Ok(tags)
    }

    fn fetch_ints(&self, q: Query) -> Result<Vec<Option<i32>>> {
        let (sql, params) = q.into_sql();
        self.fetch_map(&sql, params, |row| row.get::<_, Option<i32>>(0))
    }

    fn fetch_dates(&self, q: Query) -> Result<Vec<Date>> {
        let (sql, params) = q.into_sql();
        self.fetch_map(&sql, params, |row| {
            let s: String = row.get(0)?;
            komga_core::time_codec::parse_date(&s)
                .ok_or_else(|| crate::dao::invalid_column(0, "date", &s))
        })
    }

    fn fetch_map<T>(
        &self,
        sql: &str,
        params: Vec<Value>,
        mut map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
    ) -> Result<Vec<T>> {
        let conn = self.db.ro();
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map(row)?);
        }
        Ok(out)
    }

    fn count(&self, sql: &str, params: &[Value]) -> Result<i64> {
        let conn = self.db.ro();
        let n = conn.query_row(sql, rusqlite::params_from_iter(params), |r| {
            r.get::<_, i64>(0)
        })?;
        Ok(n)
    }

    // endregion
}

/// Kotlin `require(filterBy.type in setOf(LIBRARY, COLLECTION))`: unreachable from the v2
/// controllers, which only ever build those two filter types.
fn require_library_or_collection(filter_by: Option<&FilterBy>) {
    assert!(
        filter_by.is_none()
            || matches!(
                filter_by.map(|f| f.type_),
                Some(FilterByEntity::Library | FilterByEntity::Collection)
            ),
        "filterBy type must be LIBRARY or COLLECTION"
    );
}

/// Incremental SQL builder for the referential queries: FROM with joins, AND-ed WHEREs, ORDER BY.
struct Query {
    from: String,
    wheres: Vec<String>,
    params: Vec<Value>,
    order: String,
}

impl Query {
    fn new(select_from: &str) -> Self {
        Self {
            from: select_from.to_string(),
            wheres: vec![],
            params: vec![],
            order: String::new(),
        }
    }

    /// `field LIKE ('%' || ? || '%') ESCAPE '!'` over the accent-stripped field (jOOQ `contains`)
    fn where_contains(&mut self, field: &str, search: &str) {
        self.wheres.push(format!(
            "UDF_STRIP_ACCENTS({field}) LIKE ('%' || ? || '%') ESCAPE '!'"
        ));
        self.params
            .push(Value::Text(escape_like(&strip_accents(search))));
    }

    fn where_eq(&mut self, field: &str, value: &str) {
        self.wheres.push(format!("{field} = ?"));
        self.params.push(Value::Text(value.to_string()));
    }

    fn where_in(&mut self, field: &str, ids: &BTreeSet<String>) {
        let (sql, params) = in_clause(field, ids);
        self.wheres.push(sql);
        self.params.extend(params);
    }

    fn where_raw(&mut self, sql: &str) {
        self.wheres.push(sql.to_string());
    }

    fn order_by(&mut self, order: &str) {
        self.order = format!(" ORDER BY {order}");
    }

    fn where_parts(&self) -> (String, Vec<Value>) {
        let sql = if self.wheres.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.wheres.join(" AND "))
        };
        (sql, self.params.clone())
    }

    fn into_sql(self) -> (String, Vec<Value>) {
        let (where_sql, params) = self.where_parts();
        (format!("{}{}{}", self.from, where_sql, self.order), params)
    }

    /// LIMIT/OFFSET is appended only for paged requests, matching jOOQ
    fn paged_sql(mut self, page: &PageRequest) -> (String, Vec<Value>) {
        if !page.unpaged {
            self.order
                .push_str(&format!(" LIMIT {} OFFSET {}", page.size, page.offset()));
        }
        self.into_sql()
    }
}

fn union_of(a: Query, b: Query) -> (String, Vec<Value>) {
    let (a_sql, a_params) = a.into_sql();
    let (b_sql, b_params) = b.into_sql();
    (
        format!("{a_sql} UNION {b_sql}"),
        [a_params, b_params].concat(),
    )
}

/// jOOQ `Field.in`: an empty collection renders the false condition `1 = 0`
fn in_clause(field: &str, ids: &BTreeSet<String>) -> (String, Vec<Value>) {
    if ids.is_empty() {
        return ("1 = 0".to_string(), vec![]);
    }
    let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    (
        format!("{field} IN ({ph})"),
        ids.iter().map(|i| Value::Text(i.clone())).collect(),
    )
}

/// jOOQ escapes LIKE patterns with `!`
fn escape_like(value: &str) -> String {
    value
        .replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto_dao::SortOrder;
    use crate::migrate::Migrator;
    use crate::{main_migrations, Placeholders};
    use komga_core::model::user::{AgeRestriction, AllowExclude, ContentRestrictions};

    fn db() -> Database {
        let db = Database::open_in_memory(true).unwrap();
        let migrations = main_migrations();
        Migrator::new(&migrations, Placeholders::default())
            .migrate(&db.rw())
            .unwrap();
        db
    }

    fn exec(db: &Database, sql: &str, params: impl rusqlite::Params) {
        db.rw().execute(sql, params).unwrap();
    }

    fn library(db: &Database, id: &str) {
        exec(
            db,
            "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?, 'lib', 'file:/data/')",
            [id],
        );
    }

    fn series(db: &Database, id: &str, library_id: &str) {
        exec(
            db,
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) \
             VALUES (?, ?, 'file:/data/x/', '2024-01-01 00:00:00.0', ?)",
            rusqlite::params![id, id, library_id],
        );
    }

    fn series_metadata(
        db: &Database,
        series_id: &str,
        publisher: &str,
        language: &str,
        age_rating: Option<i32>,
    ) {
        exec(
            db,
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, PUBLISHER, LANGUAGE, AGE_RATING) \
             VALUES (?, 'ONGOING', ?, ?, ?, ?, ?)",
            rusqlite::params![series_id, series_id, series_id, publisher, language, age_rating],
        );
    }

    fn book(db: &Database, id: &str, series_id: &str, library_id: &str) {
        exec(
            db,
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
             VALUES (?, ?, 'file:/data/x.cbz', '2024-01-01 00:00:00.0', ?, ?)",
            rusqlite::params![id, id, series_id, library_id],
        );
    }

    fn book_metadata(db: &Database, book_id: &str, number_sort: f32, release_date: Option<&str>) {
        exec(
            db,
            "INSERT INTO BOOK_METADATA (BOOK_ID, TITLE, NUMBER, NUMBER_SORT, RELEASE_DATE) VALUES (?, ?, '', ?, ?)",
            rusqlite::params![book_id, book_id, number_sort, release_date],
        );
    }

    fn collection(db: &Database, id: &str, series_ids: &[&str]) {
        exec(
            db,
            "INSERT INTO COLLECTION (ID, NAME, ORDERED, SERIES_COUNT) VALUES (?, ?, 1, ?)",
            rusqlite::params![id, id, series_ids.len() as i32],
        );
        for (i, sid) in series_ids.iter().enumerate() {
            exec(
                db,
                "INSERT INTO COLLECTION_SERIES (COLLECTION_ID, SERIES_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, sid, i as i32],
            );
        }
    }

    fn readlist(db: &Database, id: &str, book_ids: &[&str]) {
        exec(
            db,
            "INSERT INTO READLIST (ID, NAME, BOOK_COUNT) VALUES (?, ?, ?)",
            rusqlite::params![id, id, book_ids.len() as i32],
        );
        for (i, bid) in book_ids.iter().enumerate() {
            exec(
                db,
                "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, bid, i as i32],
            );
        }
    }

    /// Base dataset:
    /// - l1: s1 (publisher P1, lang en, age 18, genre action, series tag st1, sharing kid)
    ///   books b1 (author Miura/writer, tag bt1), b2
    /// - l2: s2 (publisher P2, lang ja, no age, genre drama, series tag st2, sharing adult)
    ///   book b3 (author Stan/writer, tag bt2)
    /// - collection c1: [s1]; readlist r1: [b1, b3]
    fn seed(db: &Database) {
        library(db, "l1");
        library(db, "l2");
        series(db, "s1", "l1");
        series(db, "s2", "l2");
        series_metadata(db, "s1", "P1", "en", Some(18));
        series_metadata(db, "s2", "P2", "ja", None);

        exec(db, "INSERT INTO SERIES_METADATA_GENRE (SERIES_ID, GENRE) VALUES ('s1', 'action'), ('s2', 'drama')", []);
        exec(
            db,
            "INSERT INTO SERIES_METADATA_TAG (SERIES_ID, TAG) VALUES ('s1', 'st1'), ('s2', 'st2')",
            [],
        );
        exec(db, "INSERT INTO SERIES_METADATA_SHARING (SERIES_ID, LABEL) VALUES ('s1', 'kid'), ('s2', 'adult')", []);

        book(db, "b1", "s1", "l1");
        book(db, "b2", "s1", "l1");
        book(db, "b3", "s2", "l2");
        book_metadata(db, "b1", 1.0, Some("2020-05-01"));
        book_metadata(db, "b2", 2.0, Some("2021-06-01"));
        book_metadata(db, "b3", 1.0, Some("2023-07-01"));

        exec(db, "INSERT INTO BOOK_METADATA_AUTHOR (BOOK_ID, NAME, ROLE) VALUES ('b1', 'Miura', 'writer'), ('b3', 'Stan', 'writer')", []);
        exec(
            db,
            "INSERT INTO BOOK_METADATA_TAG (BOOK_ID, TAG) VALUES ('b1', 'bt1'), ('b3', 'bt2')",
            [],
        );
        exec(db, "INSERT INTO BOOK_METADATA_AGGREGATION (SERIES_ID, RELEASE_DATE) VALUES ('s1', '2021-06-01'), ('s2', '2023-07-01')", []);
        exec(db, "INSERT INTO BOOK_METADATA_AGGREGATION_AUTHOR (SERIES_ID, NAME, ROLE) VALUES ('s1', 'Miura', 'writer'), ('s2', 'Stan', 'writer')", []);
        exec(db, "INSERT INTO BOOK_METADATA_AGGREGATION_TAG (SERIES_ID, TAG) VALUES ('s1', 'bt1'), ('s2', 'bt2')", []);

        collection(db, "c1", &["s1"]);
        readlist(db, "r1", &["b1", "b3"]);
    }

    fn ctx_all(user_id: Option<&str>) -> SearchContext {
        SearchContext {
            user_id: user_id.map(String::from),
            restrictions: ContentRestrictions::default(),
            library_ids: None,
        }
    }

    fn ids(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    fn page_req() -> PageRequest {
        PageRequest {
            page: 0,
            size: 20,
            unpaged: false,
            sort: vec![],
        }
    }

    // region v1 tests

    #[test]
    fn v1_authors() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        let all = dao.find_all_authors_by_name("", None).unwrap();
        assert_eq!(
            all,
            vec![
                Author::new("Miura", "writer"),
                Author::new("Stan", "writer")
            ]
        );

        let filtered = dao.find_all_authors_by_name("miu", None).unwrap();
        assert_eq!(filtered, vec![Author::new("Miura", "writer")]);

        // author names are normalized on construction (trim + lowercase role)
        exec(&db, "INSERT INTO BOOK_METADATA_AUTHOR (BOOK_ID, NAME, ROLE) VALUES ('b2', '  X  ', 'ARTIST')", []);
        let all = dao.find_all_authors_by_name("", None).unwrap();
        assert!(all.contains(&Author::new("X", "ARTIST")));

        // by library: aggregated authors of that library's series
        let by_lib = dao
            .find_all_authors_by_name_and_library("", "l1", None)
            .unwrap();
        assert_eq!(by_lib, vec![Author::new("Miura", "writer")]);

        // by collection
        let by_col = dao
            .find_all_authors_by_name_and_collection("", "c1", None)
            .unwrap();
        assert_eq!(by_col, vec![Author::new("Miura", "writer")]);

        // by series
        let by_series = dao
            .find_all_authors_by_name_and_series("", "s2", None)
            .unwrap();
        assert_eq!(by_series, vec![Author::new("Stan", "writer")]);

        // authorized-library filter: l2 only sees its own authors
        let l2 = ids(&["l2"]);
        let all = dao.find_all_authors_by_name("", Some(&l2)).unwrap();
        assert_eq!(all, vec![Author::new("Stan", "writer")]);
        let by_lib = dao
            .find_all_authors_by_name_and_library("", "l1", Some(&l2))
            .unwrap();
        assert!(by_lib.is_empty());
        let by_col = dao
            .find_all_authors_by_name_and_collection("", "c1", Some(&l2))
            .unwrap();
        assert!(by_col.is_empty());
        let by_series = dao
            .find_all_authors_by_name_and_series("", "s1", Some(&l2))
            .unwrap();
        assert!(by_series.is_empty());
    }

    #[test]
    fn v1_authors_names_and_roles() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        let names = dao.find_all_authors_names_by_name("", None).unwrap();
        assert_eq!(names, vec!["Miura", "Stan"]);
        let names = dao.find_all_authors_names_by_name("sta", None).unwrap();
        assert_eq!(names, vec!["Stan"]);
        let l1 = ids(&["l1"]);
        let names = dao.find_all_authors_names_by_name("", Some(&l1)).unwrap();
        assert_eq!(names, vec!["Miura"]);

        exec(
            &db,
            "INSERT INTO BOOK_METADATA_AUTHOR (BOOK_ID, NAME, ROLE) VALUES ('b2', 'Y', 'artist')",
            [],
        );
        let roles = dao.find_all_authors_roles(None).unwrap();
        assert_eq!(roles, vec!["artist", "writer"]);
        let roles = dao.find_all_authors_roles(Some(&l1)).unwrap();
        assert_eq!(roles, vec!["artist", "writer"]);
        let l2 = ids(&["l2"]);
        let roles = dao.find_all_authors_roles(Some(&l2)).unwrap();
        assert_eq!(roles, vec!["writer"]);
    }

    #[test]
    fn v1_genres() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        assert_eq!(dao.find_all_genres(None).unwrap(), vec!["action", "drama"]);
        assert_eq!(
            dao.find_all_genres_by_libraries(&ids(&["l1"]), None)
                .unwrap(),
            vec!["action"]
        );
        assert_eq!(
            dao.find_all_genres_by_collection("c1", None).unwrap(),
            vec!["action"]
        );
        let l2 = ids(&["l2"]);
        assert_eq!(dao.find_all_genres(Some(&l2)).unwrap(), vec!["drama"]);
        assert!(dao
            .find_all_genres_by_collection("c1", Some(&l2))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn v1_series_and_book_tags() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        // union of book tags and series tags, sorted by stripAccents+lowercase
        assert_eq!(
            dao.find_all_series_and_book_tags(None).unwrap(),
            vec!["bt1", "bt2", "st1", "st2"]
        );
        assert_eq!(
            dao.find_all_series_and_book_tags_by_libraries(&ids(&["l1"]), None)
                .unwrap(),
            vec!["bt1", "st1"]
        );
        // by collection: book side goes through the aggregation tag table
        assert_eq!(
            dao.find_all_series_and_book_tags_by_collection("c1", None)
                .unwrap(),
            vec!["bt1", "st1"]
        );
        let l2 = ids(&["l2"]);
        assert_eq!(
            dao.find_all_series_and_book_tags(Some(&l2)).unwrap(),
            vec!["bt2", "st2"]
        );
    }

    #[test]
    fn v1_series_tags() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        assert_eq!(dao.find_all_series_tags(None).unwrap(), vec!["st1", "st2"]);
        assert_eq!(
            dao.find_all_series_tags_by_library("l1", None).unwrap(),
            vec!["st1"]
        );
        assert_eq!(
            dao.find_all_series_tags_by_collection("c1", None).unwrap(),
            vec!["st1"]
        );
        let l2 = ids(&["l2"]);
        assert_eq!(dao.find_all_series_tags(Some(&l2)).unwrap(), vec!["st2"]);
    }

    #[test]
    fn v1_book_tags() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        assert_eq!(dao.find_all_book_tags(None).unwrap(), vec!["bt1", "bt2"]);
        assert_eq!(
            dao.find_all_book_tags_by_series("s1", None).unwrap(),
            vec!["bt1"]
        );
        // readlist r1 contains b1 and b3: both tags
        assert_eq!(
            dao.find_all_book_tags_by_readlist("r1", None).unwrap(),
            vec!["bt1", "bt2"]
        );
        let l1 = ids(&["l1"]);
        assert_eq!(
            dao.find_all_book_tags_by_readlist("r1", Some(&l1)).unwrap(),
            vec!["bt1"]
        );
        assert_eq!(dao.find_all_book_tags(Some(&l1)).unwrap(), vec!["bt1"]);
    }

    #[test]
    fn v1_languages() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        assert_eq!(dao.find_all_languages(None).unwrap(), vec!["en", "ja"]);
        assert_eq!(
            dao.find_all_languages_by_libraries(&ids(&["l2"]), None)
                .unwrap(),
            vec!["ja"]
        );
        assert_eq!(
            dao.find_all_languages_by_collection("c1", None).unwrap(),
            vec!["en"]
        );
        let l2 = ids(&["l2"]);
        assert_eq!(dao.find_all_languages(Some(&l2)).unwrap(), vec!["ja"]);
    }

    #[test]
    fn v1_publishers() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        assert_eq!(dao.find_all_publishers(None).unwrap(), vec!["P1", "P2"]);
        assert_eq!(
            dao.find_all_publishers_by_libraries(&ids(&["l2"]), None)
                .unwrap(),
            vec!["P2"]
        );
        assert_eq!(
            dao.find_all_publishers_by_collection("c1", None).unwrap(),
            vec!["P1"]
        );
        let l2 = ids(&["l2"]);
        assert_eq!(dao.find_all_publishers(Some(&l2)).unwrap(), vec!["P2"]);
    }

    #[test]
    fn v1_age_ratings() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        // NULL sorts first, and is kept as-is (the v1 controller maps it to "None")
        assert_eq!(
            dao.find_all_age_ratings(None).unwrap(),
            vec![None, Some(18)]
        );
        assert_eq!(
            dao.find_all_age_ratings_by_libraries(&ids(&["l1"]), None)
                .unwrap(),
            vec![Some(18)]
        );
        assert_eq!(
            dao.find_all_age_ratings_by_collection("c1", None).unwrap(),
            vec![Some(18)]
        );
    }

    #[test]
    fn v1_series_release_dates() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        let dates = dao.find_all_series_release_dates(None).unwrap();
        assert_eq!(
            dates,
            vec![
                komga_core::time_codec::parse_date("2023-07-01").unwrap(),
                komga_core::time_codec::parse_date("2021-06-01").unwrap(),
            ]
        );
        assert_eq!(
            dao.find_all_series_release_dates_by_libraries(&ids(&["l1"]), None)
                .unwrap(),
            vec![komga_core::time_codec::parse_date("2021-06-01").unwrap()]
        );
        assert_eq!(
            dao.find_all_series_release_dates_by_collection("c1", None)
                .unwrap(),
            vec![komga_core::time_codec::parse_date("2021-06-01").unwrap()]
        );
    }

    #[test]
    fn v1_sharing_labels() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        assert_eq!(
            dao.find_all_sharing_labels(None).unwrap(),
            vec!["adult", "kid"]
        );
        assert_eq!(
            dao.find_all_sharing_labels_by_libraries(&ids(&["l1"]), None)
                .unwrap(),
            vec!["kid"]
        );
        assert_eq!(
            dao.find_all_sharing_labels_by_collection("c1", None)
                .unwrap(),
            vec!["kid"]
        );
    }

    // endregion

    // region v2 tests

    #[test]
    fn v2_authors_filters() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());
        let ctx = ctx_all(None);

        let page = dao
            .find_authors(&ctx, None, None, None, &page_req())
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(
            page.items,
            vec![
                AuthorDto::from(&Author::new("Miura", "writer")),
                AuthorDto::from(&Author::new("Stan", "writer")),
            ]
        );

        // search on name (accent-stripped contains)
        let page = dao
            .find_authors(&ctx, Some("miu"), None, None, &page_req())
            .unwrap();
        assert_eq!(page.items.len(), 1);

        // role filter
        let page = dao
            .find_authors(&ctx, None, Some("colorist"), None, &page_req())
            .unwrap();
        assert_eq!(page.total, 0);

        // filter by series
        let fb = FilterBy {
            type_: FilterByEntity::Series,
            ids: ids(&["s1"]),
        };
        let page = dao
            .find_authors(&ctx, None, None, Some(&fb), &page_req())
            .unwrap();
        assert_eq!(
            page.items,
            vec![AuthorDto::from(&Author::new("Miura", "writer"))]
        );

        // filter by readlist (b1 + b3)
        let fb = FilterBy {
            type_: FilterByEntity::ReadList,
            ids: ids(&["r1"]),
        };
        let page = dao
            .find_authors(&ctx, None, None, Some(&fb), &page_req())
            .unwrap();
        assert_eq!(page.total, 2);

        // filter by collection
        let fb = FilterBy {
            type_: FilterByEntity::Collection,
            ids: ids(&["c1"]),
        };
        let page = dao
            .find_authors(&ctx, None, None, Some(&fb), &page_req())
            .unwrap();
        assert_eq!(
            page.items,
            vec![AuthorDto::from(&Author::new("Miura", "writer"))]
        );

        // names with role filter
        let page = dao
            .find_authors_names(&ctx, None, Some("writer"), None, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["Miura", "Stan"]);
        let page = dao
            .find_authors_names(&ctx, None, Some("artist"), None, &page_req())
            .unwrap();
        assert_eq!(page.total, 0);

        // roles
        let page = dao.find_authors_roles(&ctx, None, &page_req()).unwrap();
        assert_eq!(page.items, vec!["writer"]);
    }

    #[test]
    fn v2_genres_sharing_languages_publishers() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());
        let ctx = ctx_all(None);

        let page = dao.find_genres(&ctx, None, None, &page_req()).unwrap();
        assert_eq!(page.items, vec!["action", "drama"]);

        let fb = FilterBy {
            type_: FilterByEntity::Library,
            ids: ids(&["l1"]),
        };
        let page = dao.find_genres(&ctx, None, Some(&fb), &page_req()).unwrap();
        assert_eq!(page.items, vec!["action"]);

        let fb = FilterBy {
            type_: FilterByEntity::Collection,
            ids: ids(&["c1"]),
        };
        let page = dao
            .find_sharing_labels(&ctx, None, Some(&fb), &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["kid"]);

        let page = dao.find_languages(&ctx, None, None, &page_req()).unwrap();
        assert_eq!(page.items, vec!["en", "ja"]);
        let page = dao
            .find_languages(&ctx, Some("e"), None, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["en"]);

        let page = dao.find_publishers(&ctx, None, None, &page_req()).unwrap();
        assert_eq!(page.items, vec!["P1", "P2"]);
    }

    #[test]
    fn v2_tags_include_modes() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());
        let ctx = ctx_all(None);

        let page = dao
            .find_tags(&ctx, None, None, FilterTags::Series, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["st1", "st2"]);

        let page = dao
            .find_tags(&ctx, None, None, FilterTags::Book, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["bt1", "bt2"]);

        // BOTH reads through the SERIES_AND_BOOK_TAG view (aggregation tags + series tags)
        let page = dao
            .find_tags(&ctx, None, None, FilterTags::Both, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["bt1", "bt2", "st1", "st2"]);

        // filter by readlist works only for the book side (series_id is resolved through BOOK)
        let fb = FilterBy {
            type_: FilterByEntity::ReadList,
            ids: ids(&["r1"]),
        };
        let page = dao
            .find_tags(&ctx, None, Some(&fb), FilterTags::Book, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["bt1", "bt2"]);
    }

    #[test]
    fn v2_release_years() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());
        let ctx = ctx_all(None);

        let page = dao
            .find_series_release_years(&ctx, None, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["2023", "2021"]);
        assert_eq!(page.total, 2);

        let fb = FilterBy {
            type_: FilterByEntity::Library,
            ids: ids(&["l1"]),
        };
        let page = dao
            .find_series_release_years(&ctx, Some(&fb), &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["2021"]);
    }

    #[test]
    fn v2_release_years_null_counts_but_is_not_listed() {
        let db = db();
        seed(&db);
        // a series with aggregation row but no release date
        series(&db, "s2-null", "l2");
        exec(
            &db,
            "INSERT INTO BOOK_METADATA_AGGREGATION (SERIES_ID) VALUES ('s2-null')",
            [],
        );
        let dao = ReferentialDao::new(db.clone());
        let ctx = ctx_all(None);

        let page = dao
            .find_series_release_years(&ctx, None, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["2023", "2021"]);
        // the NULL year is a distinct row: counted, not listed
        assert_eq!(page.total, 3);
    }

    #[test]
    fn v2_age_ratings() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());
        let ctx = ctx_all(None);

        let page = dao.find_age_ratings(&ctx, None, &page_req()).unwrap();
        assert_eq!(page.items, vec![18]);
        // NULL is a distinct value: counted, not listed
        assert_eq!(page.total, 2);
    }

    #[test]
    fn v2_content_restrictions_apply() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        // allow only age <= 12: s1 (18) is excluded, and s2 (no age rating) is not allowed either
        let ctx = SearchContext {
            user_id: None,
            restrictions: ContentRestrictions::new(
                Some(AgeRestriction {
                    age: 12,
                    restriction: AllowExclude::AllowOnly,
                }),
                BTreeSet::new(),
                BTreeSet::new(),
            ),
            library_ids: None,
        };
        assert_eq!(
            dao.find_genres(&ctx, None, None, &page_req())
                .unwrap()
                .total,
            0
        );

        // exclude age >= 15: s1 (18) is filtered out, s2 (no age rating) stays
        let ctx = SearchContext {
            user_id: None,
            restrictions: ContentRestrictions::new(
                Some(AgeRestriction {
                    age: 15,
                    restriction: AllowExclude::Exclude,
                }),
                BTreeSet::new(),
                BTreeSet::new(),
            ),
            library_ids: None,
        };
        let page = dao.find_genres(&ctx, None, None, &page_req()).unwrap();
        assert_eq!(page.items, vec!["drama"]);
        let page = dao.find_publishers(&ctx, None, None, &page_req()).unwrap();
        assert_eq!(page.items, vec!["P2"]);
        let page = dao
            .find_series_release_years(&ctx, None, &page_req())
            .unwrap();
        assert_eq!(page.items, vec!["2023"]);
    }

    #[test]
    fn v2_authorized_libraries_apply() {
        let db = db();
        seed(&db);
        let dao = ReferentialDao::new(db.clone());

        let mut ctx = ctx_all(None);
        ctx.library_ids = Some(ids(&["l2"]));
        let page = dao.find_genres(&ctx, None, None, &page_req()).unwrap();
        assert_eq!(page.items, vec!["drama"]);
        let page = dao
            .find_authors(&ctx, None, None, None, &page_req())
            .unwrap();
        assert_eq!(
            page.items,
            vec![AuthorDto::from(&Author::new("Stan", "writer"))]
        );

        // an empty authorization set matches nothing
        ctx.library_ids = Some(BTreeSet::new());
        let page = dao.find_genres(&ctx, None, None, &page_req()).unwrap();
        assert_eq!(page.total, 0);
    }

    #[test]
    fn v2_pagination() {
        let db = db();
        seed(&db);
        for i in 0..25 {
            exec(
                &db,
                "INSERT INTO BOOK_METADATA_AUTHOR (BOOK_ID, NAME, ROLE) VALUES ('b1', ?, 'writer')",
                [format!("Author{i:02}")],
            );
        }
        let dao = ReferentialDao::new(db.clone());
        let ctx = ctx_all(None);

        let paged = PageRequest {
            page: 1,
            size: 10,
            unpaged: false,
            sort: vec![SortOrder {
                property: "name".into(),
                descending: false,
            }],
        };
        let page = dao.find_authors(&ctx, None, None, None, &paged).unwrap();
        assert_eq!(page.total, 27);
        assert_eq!(page.items.len(), 10);
        assert_eq!(page.items[0].name, "Author10");
        assert!(page.sorted);

        // unpaged returns everything in one page
        let unpaged = PageRequest {
            page: 0,
            size: 10,
            unpaged: true,
            sort: vec![],
        };
        let page = dao.find_authors(&ctx, None, None, None, &unpaged).unwrap();
        assert_eq!(page.items.len(), 27);
    }

    // endregion

    // region read progress tests (in read_progress.rs, but the DAO shares this seed)

    // endregion
}
