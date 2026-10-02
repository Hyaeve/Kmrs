//! DTOs for the kmrs-private reading statistics endpoints.

use komga_core::dto::{dto_datetime, dto_datetime_opt};
use serde::Serialize;

/// `GET /api/v1/stats/reading/summary`: totals cards.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingSummaryDto {
    pub total_books: i64,
    pub books_started: i64,
    pub books_completed: i64,
    pub pages_read: i64,
    pub average_pages_per_book: i64,
    pub reading_days: i64,
    #[serde(with = "dto_datetime_opt")]
    pub last_read_at: Option<time::OffsetDateTime>,
    pub current_streak_days: i64,
    pub longest_streak_days: i64,
    pub status_distribution: Vec<NamedValueDto>,
    #[serde(with = "dto_datetime")]
    pub generated_at: time::OffsetDateTime,
}

/// `GET /api/v1/stats/reading/activity`: the time dimension.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingActivityDto {
    /// 7 counts, index 0 = Sunday .. 6 = Saturday.
    pub weekday_distribution: [i64; 7],
    /// 24 counts, index = hour of day.
    pub hourly_distribution: [i64; 24],
    pub reading_time_series: Vec<ReadingTimeSeriesPointDto>,
    #[serde(with = "dto_datetime")]
    pub generated_at: time::OffsetDateTime,
}

/// `GET /api/v1/stats/reading/tops`: content composition.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingTopsDto {
    pub top_authors: Vec<NamedValueDto>,
    pub top_genres: Vec<NamedValueDto>,
    pub top_tags: Vec<NamedValueDto>,
    pub genre_distribution: Vec<NamedValueDto>,
    pub tag_distribution: Vec<NamedValueDto>,
    #[serde(with = "dto_datetime")]
    pub generated_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedValueDto {
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingTimeSeriesPointDto {
    /// `yyyy-MM-dd`, UTC.
    pub date: String,
    pub pages_read: i64,
    pub books_completed: i64,
}
