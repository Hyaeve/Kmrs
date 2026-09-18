//! Translation of the structured search DSL (`komga_core::search`) into SQLite WHERE fragments.
//!
//! Ported from komga's `SeriesSearchHelper.kt`, `BookSearchHelper.kt`,
//! `ContentRestrictionsSearchHelper.kt`, `SearchOperatorUtils.kt`, and the restriction part of
//! `Utils.kt`. Column references use the same table names as the jOOQ generated tables
//! (`SERIES`, `BOOK`, `SERIES_METADATA`, ...), which match the physical schema.
//!
//! Composition mirrors jOOQ: an empty fragment is `noCondition()` and disappears from
//! AND/OR compositions; `falseCondition()` is rendered `1 = 0`.

use komga_core::model::user::{AllowExclude, ContentRestrictions};
use komga_core::natural_sort::strip_accents;
use komga_core::search::*;
use komga_core::time_codec;
use rusqlite::types::Value;
use std::collections::BTreeSet;

/// Tables that must be added to the query for a condition to work (`RequiredJoin.kt`).
/// Only the read-list and collection joins are actually dynamic; the rest are always
/// joined by the DTO queries and are kept for parity with the Kotlin model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequiredJoin {
    BookMetadata,
    Media,
    ReadProgress(String),
    ReadList(String),
    Collection(String),
    BookMetadataAggregation,
    SeriesMetadata,
}

/// A WHERE fragment plus its bind parameters, in placeholder order.
#[derive(Debug, Default)]
pub struct SqlWhere {
    pub sql: String,
    pub params: Vec<Value>,
    pub joins: BTreeSet<RequiredJoin>,
}

impl SqlWhere {
    pub fn no_condition() -> Self {
        Self::default()
    }

    pub fn false_condition() -> Self {
        Self {
            sql: "1 = 0".to_string(),
            params: vec![],
            joins: BTreeSet::new(),
        }
    }

    fn raw(sql: impl Into<String>, joins: BTreeSet<RequiredJoin>) -> Self {
        Self {
            sql: sql.into(),
            params: vec![],
            joins,
        }
    }

    fn bind(sql: impl Into<String>, params: Vec<Value>) -> Self {
        Self {
            sql: sql.into(),
            params,
            joins: BTreeSet::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.sql.is_empty()
    }

    /// jOOQ `Condition.and`: noCondition is absorbed
    pub fn and(self, other: SqlWhere) -> SqlWhere {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => SqlWhere::no_condition(),
            (true, false) => other,
            (false, true) => self,
            (false, false) => SqlWhere {
                sql: format!("({}) AND ({})", self.sql, other.sql),
                params: [self.params, other.params].concat(),
                joins: self.joins.into_iter().chain(other.joins).collect(),
            },
        }
    }

    /// jOOQ `Condition.or`: noCondition is absorbed
    pub fn or(self, other: SqlWhere) -> SqlWhere {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => SqlWhere::no_condition(),
            (true, false) => other,
            (false, true) => self,
            (false, false) => SqlWhere {
                sql: format!("({}) OR ({})", self.sql, other.sql),
                params: [self.params, other.params].concat(),
                joins: self.joins.into_iter().chain(other.joins).collect(),
            },
        }
    }

    fn not(self) -> SqlWhere {
        if self.is_empty() {
            return self;
        }
        SqlWhere {
            sql: format!("NOT ({})", self.sql),
            params: self.params,
            joins: self.joins,
        }
    }
}

fn unicode1(field: &str) -> String {
    format!("{field} COLLATE COLLATION_UNICODE_1")
}

fn strip(field: &str) -> String {
    format!("UDF_STRIP_ACCENTS({field})")
}

/// jOOQ escapes LIKE patterns with `!`
fn escape_like(value: &str) -> String {
    value
        .replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_")
}

fn join_set(joins: &[RequiredJoin]) -> BTreeSet<RequiredJoin> {
    joins.iter().cloned().collect()
}

// region operator translations (`SearchOperatorUtils.kt`)

fn equality_string(field: &str, op: &Equality<String>, ignore_case: bool) -> SqlWhere {
    let field = if ignore_case {
        unicode1(field)
    } else {
        field.to_string()
    };
    match op {
        Equality::Is { value } => SqlWhere::bind(
            format!("{field} = ?"),
            vec![Value::Text(value.clone())],
        ),
        Equality::IsNot { value } => SqlWhere::bind(
            format!("{field} <> ?"),
            vec![Value::Text(value.clone())],
        ),
    }
}

/// Enum-valued equality: the bound parameter is the enum's `name()`
fn equality_named<T: AsRef<str>>(field: &str, op: &Equality<T>) -> SqlWhere {
    match op {
        Equality::Is { value } => SqlWhere::bind(
            format!("{field} = ?"),
            vec![Value::Text(value.as_ref().to_string())],
        ),
        Equality::IsNot { value } => SqlWhere::bind(
            format!("{field} <> ?"),
            vec![Value::Text(value.as_ref().to_string())],
        ),
    }
}

fn string_op(field: &str, op: &StringOp) -> SqlWhere {
    let stripped = strip(field);
    let like = |pattern: String| {
        SqlWhere::bind(
            format!("LOWER({stripped}) LIKE LOWER(?) ESCAPE '!'"),
            vec![Value::Text(pattern)],
        )
    };
    match op {
        StringOp::BeginsWith { value } => like(format!("{}%", escape_like(&strip_accents(value)))),
        StringOp::DoesNotBeginWith { value } => {
            like(format!("{}%", escape_like(&strip_accents(value)))).not()
        }
        StringOp::Contains { value } => like(format!("%{}%", escape_like(&strip_accents(value)))),
        StringOp::DoesNotContain { value } => {
            like(format!("%{}%", escape_like(&strip_accents(value)))).not()
        }
        StringOp::EndsWith { value } => like(format!("%{}", escape_like(&strip_accents(value)))),
        StringOp::DoesNotEndWith { value } => {
            like(format!("%{}", escape_like(&strip_accents(value)))).not()
        }
        StringOp::Is { value } => SqlWhere::bind(
            format!("{} = ?", unicode1(field)),
            vec![Value::Text(value.clone())],
        ),
        StringOp::IsNot { value } => SqlWhere::bind(
            format!("{} <> ?", unicode1(field)),
            vec![Value::Text(value.clone())],
        ),
    }
}

fn date_op(field: &str, op: &DateOp) -> SqlWhere {
    // `dateTime.withZoneSameInstant(ZoneOffset.UTC).toLocalDate()`
    let utc_date = |dt: &time::OffsetDateTime| {
        time_codec::format_date(dt.to_offset(time::UtcOffset::UTC).date())
    };
    match op {
        DateOp::After { date_time } => SqlWhere::bind(
            format!("{field} > ?"),
            vec![Value::Text(utc_date(date_time))],
        ),
        DateOp::Before { date_time } => SqlWhere::bind(
            format!("{field} < ?"),
            vec![Value::Text(utc_date(date_time))],
        ),
        DateOp::IsInTheLast { duration } => {
            let threshold = time_codec::now_utc().date() - time::Duration::days(duration.to_days());
            SqlWhere::bind(
                format!("{field} > ?"),
                vec![Value::Text(time_codec::format_date(threshold))],
            )
        }
        DateOp::IsNotInTheLast { duration } => {
            let threshold = time_codec::now_utc().date() - time::Duration::days(duration.to_days());
            SqlWhere::bind(
                format!("{field} < ?"),
                vec![Value::Text(time_codec::format_date(threshold))],
            )
        }
        DateOp::IsNull => SqlWhere::raw(format!("{field} IS NULL"), BTreeSet::new()),
        DateOp::IsNotNull => SqlWhere::raw(format!("{field} IS NOT NULL"), BTreeSet::new()),
    }
}

fn numeric_nullable_i32(field: &str, op: &NumericNullable<i32>) -> SqlWhere {
    match op {
        NumericNullable::Is { value } => {
            SqlWhere::bind(format!("{field} = ?"), vec![Value::Integer(*value as i64)])
        }
        NumericNullable::IsNot { value } => SqlWhere::bind(
            format!("({field} <> ? OR {field} IS NULL)"),
            vec![Value::Integer(*value as i64)],
        ),
        NumericNullable::GreaterThan { value } => SqlWhere::bind(
            format!("{field} >= ?"),
            vec![Value::Integer(*value as i64)],
        ),
        NumericNullable::LessThan { value } => SqlWhere::bind(
            format!("{field} <= ?"),
            vec![Value::Integer(*value as i64)],
        ),
        NumericNullable::IsNull => SqlWhere::raw(format!("{field} IS NULL"), BTreeSet::new()),
        NumericNullable::IsNotNull => {
            SqlWhere::raw(format!("{field} IS NOT NULL"), BTreeSet::new())
        }
    }
}

fn numeric_f32(field: &str, op: &Numeric<f32>) -> SqlWhere {
    match op {
        Numeric::Is { value } => SqlWhere::bind(
            format!("{field} = ?"),
            vec![Value::Real(*value as f64)],
        ),
        Numeric::IsNot { value } => SqlWhere::bind(
            format!("{field} <> ?"),
            vec![Value::Real(*value as f64)],
        ),
        Numeric::GreaterThan { value } => SqlWhere::bind(
            format!("{field} >= ?"),
            vec![Value::Real(*value as f64)],
        ),
        Numeric::LessThan { value } => SqlWhere::bind(
            format!("{field} <= ?"),
            vec![Value::Real(*value as f64)],
        ),
    }
}

fn boolean_op(field: &str, op: &BooleanOp) -> SqlWhere {
    match op {
        BooleanOp::IsTrue => SqlWhere::raw(format!("{field} = 1"), BTreeSet::new()),
        BooleanOp::IsFalse => SqlWhere::raw(format!("{field} = 0"), BTreeSet::new()),
    }
}

fn equality_nullable(field_id: &str, inner_equals: SqlWhere, inner_any: SqlWhere, op: &EqualityNullable<String>) -> SqlWhere {
    let in_sub = |sub: SqlWhere, negated: bool| {
        let kw = if negated { "NOT IN" } else { "IN" };
        SqlWhere {
            sql: format!("{field_id} {kw} ({})", sub.sql),
            params: sub.params,
            joins: sub.joins,
        }
    };
    match op {
        EqualityNullable::Is { .. } => in_sub(inner_equals, false),
        EqualityNullable::IsNot { .. } => in_sub(inner_equals, true),
        EqualityNullable::IsNull => in_sub(inner_any, true),
        EqualityNullable::IsNotNull => in_sub(inner_any, false),
    }
}

// endregion

/// `ContentRestrictions.toCondition()` / `ContentRestrictionsSearchHelper.kt`.
/// The returned join set contains `SeriesMetadata` when restricted (the sibling queries use it
/// to decide whether to join SERIES_METADATA).
pub fn content_restrictions_condition(restrictions: &ContentRestrictions) -> SqlWhere {
    let age_allowed = match &restrictions.age_restriction {
        Some(ar) if ar.restriction == AllowExclude::AllowOnly => SqlWhere::bind(
            "(SERIES_METADATA.AGE_RATING IS NOT NULL AND SERIES_METADATA.AGE_RATING <= ?)",
            vec![Value::Integer(ar.age as i64)],
        ),
        _ => SqlWhere::no_condition(),
    };

    let label_allowed = if !restrictions.labels_allow.is_empty() {
        let (ph, params) = placeholders(&restrictions.labels_allow);
        SqlWhere {
            sql: format!(
                "SERIES_METADATA.SERIES_ID IN (SELECT SERIES_ID FROM SERIES_METADATA_SHARING WHERE LABEL IN ({ph}))"
            ),
            params,
            joins: BTreeSet::new(),
        }
    } else {
        SqlWhere::no_condition()
    };

    let age_denied = match &restrictions.age_restriction {
        Some(ar) if ar.restriction == AllowExclude::Exclude => SqlWhere::bind(
            "(SERIES_METADATA.AGE_RATING IS NULL OR SERIES_METADATA.AGE_RATING < ?)",
            vec![Value::Integer(ar.age as i64)],
        ),
        _ => SqlWhere::no_condition(),
    };

    let label_denied = if !restrictions.labels_exclude.is_empty() {
        let (ph, params) = placeholders(&restrictions.labels_exclude);
        SqlWhere {
            sql: format!(
                "SERIES_METADATA.SERIES_ID NOT IN (SELECT SERIES_ID FROM SERIES_METADATA_SHARING WHERE LABEL IN ({ph}))"
            ),
            params,
            joins: BTreeSet::new(),
        }
    } else {
        SqlWhere::no_condition()
    };

    let restricted = restrictions.is_restricted();
    let mut out = age_allowed
        .or(label_allowed)
        .and(age_denied.and(label_denied));
    if restricted {
        out.joins.insert(RequiredJoin::SeriesMetadata);
    }
    out
}

fn placeholders(values: &BTreeSet<String>) -> (String, Vec<Value>) {
    let ph = values.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let params = values.iter().map(|v| Value::Text(v.clone())).collect();
    (ph, params)
}

fn library_ids_condition(table: &str, library_ids: Option<&BTreeSet<String>>) -> SqlWhere {
    match library_ids {
        None => SqlWhere::no_condition(),
        Some(ids) if ids.is_empty() => SqlWhere::false_condition(),
        Some(ids) => {
            let (ph, params) = placeholders(ids);
            SqlWhere::bind(format!("{table}.LIBRARY_ID IN ({ph})"), params)
        }
    }
}

/// `SeriesSearchHelper.toCondition(searchCondition)`: search condition AND base restrictions.
pub fn series_condition(condition: Option<&SearchConditionSeries>, ctx: &SearchContext) -> SqlWhere {
    let base = content_restrictions_condition(&ctx.restrictions)
        .and(library_ids_condition("SERIES", ctx.library_ids.as_ref()));
    series_condition_internal(condition, ctx).and(base)
}

fn series_condition_internal(
    condition: Option<&SearchConditionSeries>,
    ctx: &SearchContext,
) -> SqlWhere {
    let Some(condition) = condition else {
        return SqlWhere::no_condition();
    };
    match condition {
        SearchConditionSeries::AllOf { conditions } => conditions.iter().fold(
            SqlWhere::no_condition(),
            |acc, c| acc.and(series_condition_internal(Some(c), ctx)),
        ),
        SearchConditionSeries::AnyOf { conditions } => conditions.iter().fold(
            SqlWhere::no_condition(),
            |acc, c| acc.or(series_condition_internal(Some(c), ctx)),
        ),
        SearchConditionSeries::LibraryId { operator } => {
            equality_string("SERIES.LIBRARY_ID", operator, false)
        }
        SearchConditionSeries::Deleted { deleted } => match deleted {
            BooleanOp::IsFalse => SqlWhere::raw("SERIES.DELETED_DATE IS NULL", BTreeSet::new()),
            BooleanOp::IsTrue => SqlWhere::raw("SERIES.DELETED_DATE IS NOT NULL", BTreeSet::new()),
        },
        SearchConditionSeries::ReleaseDate { operator } => {
            let mut w = date_op("BOOK_METADATA_AGGREGATION.RELEASE_DATE", operator);
            w.joins.insert(RequiredJoin::BookMetadataAggregation);
            w
        }
        SearchConditionSeries::ReadStatus { operator } => match &ctx.user_id {
            None => SqlWhere::false_condition(),
            Some(user_id) => {
                let field = "READ_PROGRESS_SERIES.READ_COUNT";
                let book_count = "SERIES.BOOK_COUNT";
                let w = match operator {
                    Equality::Is { value } => match value {
                        ReadStatus::Unread => SqlWhere::raw(format!("{field} IS NULL"), BTreeSet::new()),
                        ReadStatus::Read => SqlWhere::raw(format!("{field} = {book_count}"), BTreeSet::new()),
                        ReadStatus::InProgress => {
                            SqlWhere::raw(format!("{field} <> {book_count}"), BTreeSet::new())
                        }
                    },
                    Equality::IsNot { value } => match value {
                        ReadStatus::Unread => {
                            SqlWhere::raw(format!("{field} IS NOT NULL"), BTreeSet::new())
                        }
                        ReadStatus::Read => SqlWhere::raw(
                            format!("({field} <> {book_count} OR {field} IS NULL)"),
                            BTreeSet::new(),
                        ),
                        ReadStatus::InProgress => SqlWhere::raw(
                            format!("({field} = {book_count} OR {field} IS NULL)"),
                            BTreeSet::new(),
                        ),
                    },
                };
                SqlWhere {
                    joins: join_set(&[RequiredJoin::ReadProgress(user_id.clone())]),
                    ..w
                }
            }
        },
        SearchConditionSeries::SeriesStatus { operator } => {
            let mut w = equality_named("SERIES_METADATA.STATUS", &map_named(operator, |s| s.as_str()));
            w.joins.insert(RequiredJoin::SeriesMetadata);
            w
        }
        SearchConditionSeries::Tag { tag } => {
            let inner_equals = |value: &str| SqlWhere::bind(
                format!(
                    "SELECT SERIES_ID FROM SERIES_METADATA_TAG WHERE {} = ? \
                     UNION \
                     SELECT SERIES_ID FROM BOOK_METADATA_AGGREGATION_TAG WHERE {} = ?",
                    unicode1("SERIES_METADATA_TAG.TAG"),
                    unicode1("BOOK_METADATA_AGGREGATION_TAG.TAG"),
                ),
                vec![Value::Text(value.to_string()), Value::Text(value.to_string())],
            );
            let inner_any = || SqlWhere::raw(
                "SELECT SERIES_ID FROM SERIES_METADATA_TAG WHERE TAG IS NOT NULL \
                 UNION \
                 SELECT SERIES_ID FROM BOOK_METADATA_AGGREGATION_TAG WHERE TAG IS NOT NULL",
                BTreeSet::new(),
            );
            let (eq, any) = match tag {
                EqualityNullable::Is { value } | EqualityNullable::IsNot { value } => {
                    (inner_equals(value), SqlWhere::no_condition())
                }
                _ => (SqlWhere::no_condition(), inner_any()),
            };
            equality_nullable("SERIES.ID", eq, any, tag)
        }
        SearchConditionSeries::Author { author } => {
            let (Equality::Is { value } | Equality::IsNot { value }) = author;
            if value.name.is_none() && value.role.is_none() {
                return SqlWhere::no_condition();
            }
            let mut sql = "SELECT SERIES_ID FROM BOOK_METADATA_AGGREGATION_AUTHOR WHERE 1 = 1".to_string();
            let mut params = vec![];
            if let Some(name) = &value.name {
                sql.push_str(&format!(" AND {} = ?", unicode1("BOOK_METADATA_AGGREGATION_AUTHOR.NAME")));
                params.push(Value::Text(name.clone()));
            }
            if let Some(role) = &value.role {
                sql.push_str(&format!(" AND {} = ?", unicode1("BOOK_METADATA_AGGREGATION_AUTHOR.ROLE")));
                params.push(Value::Text(role.clone()));
            }
            let kw = match author {
                Equality::Is { .. } => "IN",
                Equality::IsNot { .. } => "NOT IN",
            };
            SqlWhere::bind(format!("SERIES.ID {kw} ({sql})"), params)
        }
        SearchConditionSeries::OneShot { operator } => boolean_op("SERIES.ONESHOT", operator),
        SearchConditionSeries::AgeRating { operator } => {
            let mut w = numeric_nullable_i32("SERIES_METADATA.AGE_RATING", operator);
            w.joins.insert(RequiredJoin::SeriesMetadata);
            w
        }
        SearchConditionSeries::CollectionId { operator } => match operator {
            Equality::Is { value } => {
                let alias = collection_alias(value);
                SqlWhere {
                    sql: format!("{alias}.COLLECTION_ID = ?"),
                    params: vec![Value::Text(value.clone())],
                    joins: join_set(&[RequiredJoin::Collection(value.clone())]),
                }
            }
            Equality::IsNot { value } => SqlWhere::bind(
                "SERIES.ID NOT IN (SELECT SERIES_ID FROM COLLECTION_SERIES WHERE COLLECTION_ID = ?)",
                vec![Value::Text(value.clone())],
            ),
        },
        SearchConditionSeries::Complete { complete } => {
            let field = "SERIES_METADATA.TOTAL_BOOK_COUNT";
            let mut w = match complete {
                BooleanOp::IsTrue => SqlWhere::raw(
                    format!("({field} IS NOT NULL AND {field} = SERIES.BOOK_COUNT)"),
                    BTreeSet::new(),
                ),
                BooleanOp::IsFalse => SqlWhere::raw(
                    format!("({field} IS NOT NULL AND {field} <> SERIES.BOOK_COUNT)"),
                    BTreeSet::new(),
                ),
            };
            w.joins.insert(RequiredJoin::SeriesMetadata);
            w
        }
        SearchConditionSeries::Genre { genre } => {
            let inner_equals = |value: &str| SqlWhere::bind(
                format!(
                    "SELECT SERIES_ID FROM SERIES_METADATA_GENRE WHERE {} = ?",
                    unicode1("SERIES_METADATA_GENRE.GENRE"),
                ),
                vec![Value::Text(value.to_string())],
            );
            let inner_any = || SqlWhere::raw(
                "SELECT SERIES_ID FROM SERIES_METADATA_GENRE WHERE GENRE IS NOT NULL",
                BTreeSet::new(),
            );
            let (eq, any) = match genre {
                EqualityNullable::Is { value } | EqualityNullable::IsNot { value } => {
                    (inner_equals(value), SqlWhere::no_condition())
                }
                _ => (SqlWhere::no_condition(), inner_any()),
            };
            equality_nullable("SERIES.ID", eq, any, genre)
        }
        SearchConditionSeries::Language { language } => {
            let mut w = equality_string("SERIES_METADATA.LANGUAGE", language, true);
            w.joins.insert(RequiredJoin::SeriesMetadata);
            w
        }
        SearchConditionSeries::Publisher { publisher } => {
            let mut w = equality_string("SERIES_METADATA.PUBLISHER", publisher, true);
            w.joins.insert(RequiredJoin::SeriesMetadata);
            w
        }
        SearchConditionSeries::SharingLabel { operator: sharing_label } => {
            let inner_equals = |value: &str| SqlWhere::bind(
                format!(
                    "SELECT SERIES_ID FROM SERIES_METADATA_SHARING WHERE {} = ?",
                    unicode1("SERIES_METADATA_SHARING.LABEL"),
                ),
                vec![Value::Text(value.to_string())],
            );
            let inner_any = || SqlWhere::raw(
                "SELECT SERIES_ID FROM SERIES_METADATA_SHARING WHERE LABEL IS NOT NULL",
                BTreeSet::new(),
            );
            let (eq, any) = match sharing_label {
                EqualityNullable::Is { value } | EqualityNullable::IsNot { value } => {
                    (inner_equals(value), SqlWhere::no_condition())
                }
                _ => (SqlWhere::no_condition(), inner_any()),
            };
            equality_nullable("SERIES.ID", eq, any, sharing_label)
        }
        SearchConditionSeries::Title { title } => {
            let mut w = string_op("SERIES_METADATA.TITLE", title);
            w.joins.insert(RequiredJoin::SeriesMetadata);
            w
        }
        SearchConditionSeries::TitleSort { operator } => {
            let mut w = string_op("SERIES_METADATA.TITLE_SORT", operator);
            w.joins.insert(RequiredJoin::SeriesMetadata);
            w
        }
    }
}

/// Deprecated `search_regex` query parameter: `SearchField.toColumn().likeRegex(regex)`
pub fn series_regex_condition(regex: &str, field: SearchField) -> SqlWhere {
    let column = match field {
        SearchField::Title => "SERIES_METADATA.TITLE",
        SearchField::TitleSort => "SERIES_METADATA.TITLE_SORT",
    };
    SqlWhere::bind(format!("{column} REGEXP ?"), vec![Value::Text(regex.to_string())])
}

pub fn collection_alias(collection_id: &str) -> String {
    format!("CS_{collection_id}")
}

pub fn readlist_alias(readlist_id: &str) -> String {
    format!("RLB_{readlist_id}")
}

/// `BookSearchHelper.toCondition(searchCondition)`: search condition AND base restrictions.
pub fn book_condition(condition: Option<&SearchConditionBook>, ctx: &SearchContext) -> SqlWhere {
    let base = content_restrictions_condition(&ctx.restrictions)
        .and(library_ids_condition("BOOK", ctx.library_ids.as_ref()));
    book_condition_internal(condition, ctx).and(base)
}

fn book_condition_internal(condition: Option<&SearchConditionBook>, ctx: &SearchContext) -> SqlWhere {
    let Some(condition) = condition else {
        return SqlWhere::no_condition();
    };
    match condition {
        SearchConditionBook::AllOf { conditions } => conditions.iter().fold(
            SqlWhere::no_condition(),
            |acc, c| acc.and(book_condition_internal(Some(c), ctx)),
        ),
        SearchConditionBook::AnyOf { conditions } => conditions.iter().fold(
            SqlWhere::no_condition(),
            |acc, c| acc.or(book_condition_internal(Some(c), ctx)),
        ),
        SearchConditionBook::LibraryId { operator } => {
            equality_string("BOOK.LIBRARY_ID", operator, false)
        }
        SearchConditionBook::SeriesId { operator } => {
            equality_string("BOOK.SERIES_ID", operator, false)
        }
        SearchConditionBook::ReadListId { operator } => match operator {
            Equality::Is { value } => {
                let alias = readlist_alias(value);
                SqlWhere {
                    sql: format!("{alias}.READLIST_ID = ?"),
                    params: vec![Value::Text(value.clone())],
                    joins: join_set(&[RequiredJoin::ReadList(value.clone())]),
                }
            }
            Equality::IsNot { value } => SqlWhere::bind(
                "BOOK.ID NOT IN (SELECT BOOK_ID FROM READLIST_BOOK WHERE READLIST_ID = ?)",
                vec![Value::Text(value.clone())],
            ),
        },
        SearchConditionBook::Title { title } => {
            let mut w = string_op("BOOK_METADATA.TITLE", title);
            w.joins.insert(RequiredJoin::BookMetadata);
            w
        }
        SearchConditionBook::Deleted { deleted } => match deleted {
            BooleanOp::IsFalse => SqlWhere::raw("BOOK.DELETED_DATE IS NULL", BTreeSet::new()),
            BooleanOp::IsTrue => SqlWhere::raw("BOOK.DELETED_DATE IS NOT NULL", BTreeSet::new()),
        },
        SearchConditionBook::ReleaseDate { operator } => {
            let mut w = date_op("BOOK_METADATA.RELEASE_DATE", operator);
            w.joins.insert(RequiredJoin::BookMetadata);
            w
        }
        SearchConditionBook::NumberSort { operator } => {
            let mut w = numeric_f32("BOOK_METADATA.NUMBER_SORT", operator);
            w.joins.insert(RequiredJoin::BookMetadata);
            w
        }
        SearchConditionBook::ReadStatus { operator } => match &ctx.user_id {
            None => SqlWhere::false_condition(),
            Some(user_id) => {
                let field = "READ_PROGRESS.COMPLETED";
                let w = match operator {
                    Equality::Is { value } => match value {
                        ReadStatus::Unread => SqlWhere::raw(format!("{field} IS NULL"), BTreeSet::new()),
                        ReadStatus::Read => SqlWhere::raw(format!("{field} = 1"), BTreeSet::new()),
                        ReadStatus::InProgress => {
                            SqlWhere::raw(format!("{field} = 0"), BTreeSet::new())
                        }
                    },
                    Equality::IsNot { value } => match value {
                        ReadStatus::Unread => {
                            SqlWhere::raw(format!("{field} IS NOT NULL"), BTreeSet::new())
                        }
                        ReadStatus::Read => SqlWhere::raw(
                            format!("({field} IS NULL OR {field} = 0)"),
                            BTreeSet::new(),
                        ),
                        ReadStatus::InProgress => SqlWhere::raw(
                            format!("({field} = 1 OR {field} IS NULL)"),
                            BTreeSet::new(),
                        ),
                    },
                };
                SqlWhere {
                    joins: join_set(&[RequiredJoin::ReadProgress(user_id.clone())]),
                    ..w
                }
            }
        },
        SearchConditionBook::MediaStatus { operator } => {
            let mut w = equality_named("MEDIA.STATUS", &map_named(operator, |s| s.as_str()));
            w.joins.insert(RequiredJoin::Media);
            w
        }
        SearchConditionBook::MediaProfile { operator } => {
            let types: &[&str] = match operator {
                Equality::Is { value } | Equality::IsNot { value } => match value {
                    MediaProfile::Divina => &[
                        "application/zip",
                        "application/x-rar-compressed",
                        "application/x-rar-compressed; version=4",
                        "application/x-rar-compressed; version=5",
                    ],
                    MediaProfile::Pdf => &["application/pdf"],
                    MediaProfile::Epub => &["application/epub+zip"],
                },
            };
            let ph = types.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let params = types.iter().map(|t| Value::Text(t.to_string())).collect();
            let kw = match operator {
                Equality::Is { .. } => "IN",
                Equality::IsNot { .. } => "NOT IN",
            };
            let mut w = SqlWhere::bind(format!("MEDIA.MEDIA_TYPE {kw} ({ph})"), params);
            w.joins.insert(RequiredJoin::Media);
            w
        }
        SearchConditionBook::Tag { tag } => {
            let inner_equals = |value: &str| SqlWhere::bind(
                format!(
                    "SELECT BOOK_ID FROM BOOK_METADATA_TAG WHERE {} = ?",
                    unicode1("BOOK_METADATA_TAG.TAG"),
                ),
                vec![Value::Text(value.to_string())],
            );
            let inner_any = || SqlWhere::raw(
                "SELECT BOOK_ID FROM BOOK_METADATA_TAG WHERE TAG IS NOT NULL",
                BTreeSet::new(),
            );
            let (eq, any) = match tag {
                EqualityNullable::Is { value } | EqualityNullable::IsNot { value } => {
                    (inner_equals(value), SqlWhere::no_condition())
                }
                _ => (SqlWhere::no_condition(), inner_any()),
            };
            equality_nullable("BOOK.ID", eq, any, tag)
        }
        SearchConditionBook::Author { author } => {
            let (Equality::Is { value } | Equality::IsNot { value }) = author;
            if value.name.is_none() && value.role.is_none() {
                return SqlWhere::no_condition();
            }
            let mut sql = "SELECT BOOK_ID FROM BOOK_METADATA_AUTHOR WHERE 1 = 1".to_string();
            let mut params = vec![];
            if let Some(name) = &value.name {
                sql.push_str(&format!(" AND {} = ?", unicode1("BOOK_METADATA_AUTHOR.NAME")));
                params.push(Value::Text(name.clone()));
            }
            if let Some(role) = &value.role {
                sql.push_str(&format!(" AND {} = ?", unicode1("BOOK_METADATA_AUTHOR.ROLE")));
                params.push(Value::Text(role.clone()));
            }
            let kw = match author {
                Equality::Is { .. } => "IN",
                Equality::IsNot { .. } => "NOT IN",
            };
            SqlWhere::bind(format!("BOOK.ID {kw} ({sql})"), params)
        }
        SearchConditionBook::Poster { poster } => {
            let (Equality::Is { value } | Equality::IsNot { value }) = poster;
            if value.type_.is_none() && value.selected.is_none() {
                return SqlWhere::no_condition();
            }
            let mut sql = "SELECT BOOK_ID FROM THUMBNAIL_BOOK WHERE 1 = 1".to_string();
            let mut params = vec![];
            if let Some(type_) = &value.type_ {
                // jOOQ equalIgnoreCase
                sql.push_str(" AND LOWER(THUMBNAIL_BOOK.TYPE) = LOWER(?)");
                params.push(Value::Text(type_.as_str().to_string()));
            }
            if let Some(selected) = value.selected {
                sql.push_str(if selected {
                    " AND THUMBNAIL_BOOK.SELECTED = 1"
                } else {
                    " AND THUMBNAIL_BOOK.SELECTED = 0"
                });
            }
            let kw = match poster {
                Equality::Is { .. } => "IN",
                Equality::IsNot { .. } => "NOT IN",
            };
            SqlWhere::bind(format!("BOOK.ID {kw} ({sql})"), params)
        }
        SearchConditionBook::OneShot { operator } => boolean_op("BOOK.ONESHOT", operator),
    }
}

fn map_named<T, U: AsRef<str>>(op: &Equality<T>, f: impl Fn(&T) -> U) -> Equality<String> {
    match op {
        Equality::Is { value } => Equality::Is {
            value: f(value).as_ref().to_string(),
        },
        Equality::IsNot { value } => Equality::IsNot {
            value: f(value).as_ref().to_string(),
        },
    }
}

/// `Field<String>.inOrNoCondition`: None → noCondition, empty → falseCondition, else IN
pub fn id_in_or_no_condition(field: &str, ids: Option<&[String]>) -> SqlWhere {
    match ids {
        None => SqlWhere::no_condition(),
        Some(ids) if ids.is_empty() => SqlWhere::false_condition(),
        Some(ids) => {
            let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            SqlWhere::bind(
                format!("{field} IN ({ph})"),
                ids.iter().map(|i| Value::Text(i.clone())).collect(),
            )
        }
    }
}

/// `Field<String>.sortByValues`: used for relevance ordering
pub fn sort_by_values(field: &str, values: &[String], asc: bool) -> (String, Vec<Value>) {
    let multiplier = if asc { 1 } else { -1 };
    let mut sql = format!("CASE {field}");
    let mut params = vec![];
    for (index, value) in values.iter().enumerate() {
        sql.push_str(&format!(" WHEN ? THEN {}", index as i64 * multiplier));
        params.push(Value::Text(value.clone()));
    }
    sql.push_str(" ELSE 2147483647 END");
    (sql, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::user::{AgeRestriction, ContentRestrictions};

    fn ctx(user_id: Option<&str>) -> SearchContext {
        SearchContext {
            user_id: user_id.map(String::from),
            restrictions: ContentRestrictions::default(),
            library_ids: None,
        }
    }

    fn parse_book(json: &str) -> SearchConditionBook {
        serde_json::from_str(json).unwrap()
    }

    fn parse_series(json: &str) -> SearchConditionSeries {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn and_or_absorbs_no_condition() {
        let c = SqlWhere::no_condition().and(SqlWhere::raw("a = 1", BTreeSet::new()));
        assert_eq!(c.sql, "a = 1");
        let c = SqlWhere::raw("a = 1", BTreeSet::new()).or(SqlWhere::no_condition());
        assert_eq!(c.sql, "a = 1");
        let c = SqlWhere::raw("a = 1", BTreeSet::new()).and(SqlWhere::raw("b = 2", BTreeSet::new()));
        assert_eq!(c.sql, "(a = 1) AND (b = 2)");
    }

    #[test]
    fn book_read_status() {
        let w = book_condition(
            Some(&parse_book(r#"{"readStatus":{"operator":"is","value":"UNREAD"}}"#)),
            &ctx(Some("u1")),
        );
        assert_eq!(w.sql, "READ_PROGRESS.COMPLETED IS NULL");
        assert!(w.joins.contains(&RequiredJoin::ReadProgress("u1".into())));

        let w = book_condition(
            Some(&parse_book(r#"{"readStatus":{"operator":"isNot","value":"READ"}}"#)),
            &ctx(Some("u1")),
        );
        assert_eq!(w.sql, "(READ_PROGRESS.COMPLETED IS NULL OR READ_PROGRESS.COMPLETED = 0)");

        // no user in context: false condition
        let w = book_condition(
            Some(&parse_book(r#"{"readStatus":{"operator":"is","value":"READ"}}"#)),
            &ctx(None),
        );
        assert_eq!(w.sql, "1 = 0");
    }

    #[test]
    fn series_read_status() {
        let w = series_condition(
            Some(&parse_series(r#"{"readStatus":{"operator":"is","value":"READ"}}"#)),
            &ctx(Some("u1")),
        );
        assert_eq!(w.sql, "READ_PROGRESS_SERIES.READ_COUNT = SERIES.BOOK_COUNT");

        let w = series_condition(
            Some(&parse_series(
                r#"{"readStatus":{"operator":"isNot","value":"IN_PROGRESS"}}"#,
            )),
            &ctx(Some("u1")),
        );
        assert_eq!(
            w.sql,
            "(READ_PROGRESS_SERIES.READ_COUNT = SERIES.BOOK_COUNT OR READ_PROGRESS_SERIES.READ_COUNT IS NULL)"
        );
    }

    #[test]
    fn string_operators() {
        let w = series_condition(
            Some(&parse_series(r#"{"title":{"operator":"contains","value":"50%"}}"#)),
            &ctx(None),
        );
        assert_eq!(
            w.sql,
            "LOWER(UDF_STRIP_ACCENTS(SERIES_METADATA.TITLE)) LIKE LOWER(?) ESCAPE '!'"
        );
        assert_eq!(w.params, vec![Value::Text("%50!%%".to_string())]);

        let w = series_condition(
            Some(&parse_series(r#"{"titleSort":{"operator":"is","value":"Berserk"}}"#)),
            &ctx(None),
        );
        assert_eq!(w.sql, "SERIES_METADATA.TITLE_SORT COLLATE COLLATION_UNICODE_1 = ?");
    }

    #[test]
    fn collection_id_is_requires_join() {
        let w = series_condition(
            Some(&parse_series(r#"{"collectionId":{"operator":"is","value":"c1"}}"#)),
            &ctx(None),
        );
        assert_eq!(w.sql, "CS_c1.COLLECTION_ID = ?");
        assert!(w.joins.contains(&RequiredJoin::Collection("c1".into())));

        let w = series_condition(
            Some(&parse_series(r#"{"collectionId":{"operator":"isNot","value":"c1"}}"#)),
            &ctx(None),
        );
        assert_eq!(
            w.sql,
            "SERIES.ID NOT IN (SELECT SERIES_ID FROM COLLECTION_SERIES WHERE COLLECTION_ID = ?)"
        );
        assert!(w.joins.is_empty());
    }

    #[test]
    fn readlist_id_is_requires_join() {
        let w = book_condition(
            Some(&parse_book(r#"{"readListId":{"operator":"is","value":"r1"}}"#)),
            &ctx(None),
        );
        assert_eq!(w.sql, "RLB_r1.READLIST_ID = ?");
        assert!(w.joins.contains(&RequiredJoin::ReadList("r1".into())));
    }

    #[test]
    fn media_profile_maps_types() {
        let w = book_condition(
            Some(&parse_book(r#"{"mediaProfile":{"operator":"is","value":"DIVINA"}}"#)),
            &ctx(None),
        );
        assert_eq!(
            w.sql,
            "MEDIA.MEDIA_TYPE IN (?, ?, ?, ?)"
        );
        assert_eq!(w.params.len(), 4);
        assert_eq!(w.params[1], Value::Text("application/x-rar-compressed".to_string()));
    }

    #[test]
    fn content_restrictions_allow_only_age() {
        let restrictions = ContentRestrictions::new(
            Some(AgeRestriction {
                age: 15,
                restriction: AllowExclude::AllowOnly,
            }),
            BTreeSet::new(),
            BTreeSet::new(),
        );
        let w = content_restrictions_condition(&restrictions);
        assert_eq!(
            w.sql,
            "(SERIES_METADATA.AGE_RATING IS NOT NULL AND SERIES_METADATA.AGE_RATING <= ?)"
        );
        assert!(w.joins.contains(&RequiredJoin::SeriesMetadata));
    }

    #[test]
    fn content_restrictions_allow_or_then_deny() {
        let restrictions = ContentRestrictions::new(
            Some(AgeRestriction {
                age: 15,
                restriction: AllowExclude::AllowOnly,
            }),
            ["kids".to_string()].into_iter().collect(),
            ["horror".to_string()].into_iter().collect(),
        );
        let w = content_restrictions_condition(&restrictions);
        assert_eq!(
            w.sql,
            "(((SERIES_METADATA.AGE_RATING IS NOT NULL AND SERIES_METADATA.AGE_RATING <= ?)) OR \
             (SERIES_METADATA.SERIES_ID IN (SELECT SERIES_ID FROM SERIES_METADATA_SHARING WHERE LABEL IN (?)))) AND \
             (SERIES_METADATA.SERIES_ID NOT IN (SELECT SERIES_ID FROM SERIES_METADATA_SHARING WHERE LABEL IN (?)))"
        );
        assert_eq!(
            w.params,
            vec![
                Value::Integer(15),
                Value::Text("kids".to_string()),
                Value::Text("horror".to_string()),
            ]
        );
    }

    #[test]
    fn authorized_libraries_in_context() {
        let mut c = ctx(None);
        c.library_ids = Some(["l1".to_string(), "l2".to_string()].into_iter().collect());
        let w = series_condition(None, &c);
        assert_eq!(w.sql, "SERIES.LIBRARY_ID IN (?, ?)");

        c.library_ids = Some(BTreeSet::new());
        let w = series_condition(None, &c);
        assert_eq!(w.sql, "1 = 0");
    }

    #[test]
    fn tag_unions_series_and_book_aggregation() {
        let w = series_condition(
            Some(&parse_series(r#"{"tag":{"operator":"is","value":"seinen"}}"#)),
            &ctx(None),
        );
        assert!(w.sql.contains("UNION"));
        assert!(w.sql.starts_with("SERIES.ID IN ("));
        assert_eq!(w.params.len(), 2);

        let w = series_condition(
            Some(&parse_series(r#"{"tag":{"operator":"isNull"}}"#)),
            &ctx(None),
        );
        assert!(w.sql.starts_with("SERIES.ID NOT IN ("));
        assert!(w.params.is_empty());
    }

    #[test]
    fn author_without_name_and_role_is_no_condition() {
        let w = book_condition(
            Some(&parse_book(r#"{"author":{"operator":"is","value":{}}}"#)),
            &ctx(None),
        );
        assert!(w.is_empty());

        let w = book_condition(
            Some(&parse_book(
                r#"{"author":{"operator":"is","value":{"name":"Miura"}}}"#,
            )),
            &ctx(None),
        );
        assert_eq!(
            w.sql,
            "BOOK.ID IN (SELECT BOOK_ID FROM BOOK_METADATA_AUTHOR WHERE 1 = 1 AND BOOK_METADATA_AUTHOR.NAME COLLATE COLLATION_UNICODE_1 = ?)"
        );
    }

    #[test]
    fn date_operators_use_utc_date() {
        let w = series_condition(
            Some(&parse_series(
                r#"{"releaseDate":{"operator":"after","dateTime":"2024-12-31T12:00:00+14:00"}}"#,
            )),
            &ctx(None),
        );
        assert_eq!(w.sql, "BOOK_METADATA_AGGREGATION.RELEASE_DATE > ?");
        // 2024-12-31T12:00:00+14:00 is 2024-12-30 22:00 UTC
        assert_eq!(w.params, vec![Value::Text("2024-12-30".to_string())]);
    }

    #[test]
    fn sort_by_values_case() {
        let (sql, params) = sort_by_values("BOOK.ID", &["a".into(), "b".into()], true);
        assert_eq!(sql, "CASE BOOK.ID WHEN ? THEN 0 WHEN ? THEN 1 ELSE 2147483647 END");
        assert_eq!(params.len(), 2);

        let (sql, _) = sort_by_values("BOOK.ID", &["a".into()], false);
        assert_eq!(sql, "CASE BOOK.ID WHEN ? THEN 0 ELSE 2147483647 END");
    }

    #[test]
    fn complete_and_deleted() {
        let w = series_condition(
            Some(&parse_series(r#"{"complete":{"operator":"isTrue"}}"#)),
            &ctx(None),
        );
        assert_eq!(
            w.sql,
            "(SERIES_METADATA.TOTAL_BOOK_COUNT IS NOT NULL AND SERIES_METADATA.TOTAL_BOOK_COUNT = SERIES.BOOK_COUNT)"
        );

        let w = book_condition(
            Some(&parse_book(r#"{"deleted":{"operator":"isTrue"}}"#)),
            &ctx(None),
        );
        assert_eq!(w.sql, "BOOK.DELETED_DATE IS NOT NULL");
    }

    #[test]
    fn regex_condition() {
        let w = series_regex_condition("^ber", SearchField::Title);
        assert_eq!(w.sql, "SERIES_METADATA.TITLE REGEXP ?");
        let w = series_regex_condition("^ber", SearchField::TitleSort);
        assert_eq!(w.sql, "SERIES_METADATA.TITLE_SORT REGEXP ?");
    }
}
