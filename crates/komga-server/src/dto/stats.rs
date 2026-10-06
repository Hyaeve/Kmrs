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

/// `GET /api/v1/stats/libraries`: per-library content counts under the caller's visibility.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrariesStatsDto {
    pub libraries: Vec<LibraryStatsDto>,
    pub total: LibraryStatsTotalDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatsDto {
    pub library_id: String,
    pub name: String,
    pub series: i64,
    pub books: i64,
    pub file_size: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatsTotalDto {
    pub series: i64,
    pub books: i64,
    pub file_size: i64,
}

/// `GET /api/v1/stats/server`: admin-only server state snapshot.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatsDto {
    pub tasks: ServerTaskStatsDto,
    pub process: ServerProcessStatsDto,
    pub totals: ServerTotalsDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerTaskStatsDto {
    pub queue_size: i64,
    pub types: Vec<TaskTypeStatsDto>,
}

/// Execution metrics and queue depth merged by task type: a type shows up when it has
/// queued tasks, recorded executions, or both.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskTypeStatsDto {
    #[serde(rename = "type")]
    pub task_type: String,
    pub queued: i64,
    pub executions: u64,
    pub total_time_ms: i64,
    pub max_time_ms: i64,
    pub failures: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProcessStatsDto {
    #[serde(with = "dto_datetime")]
    pub start_time: time::OffsetDateTime,
    pub uptime_seconds: u64,
    /// Percent of total CPU capacity (100 = every core busy).
    pub cpu_usage: f64,
    pub memory_bytes: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerTotalsDto {
    pub libraries: i64,
    pub collections: i64,
    pub readlists: i64,
    pub sidecars: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingTimeSeriesPointDto {
    /// `yyyy-MM-dd`, UTC.
    pub date: String,
    pub pages_read: i64,
    pub books_completed: i64,
}
