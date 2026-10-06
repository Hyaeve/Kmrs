//! kmrs-private statistics, not part of the Komga API surface (kept out of the OpenAPI
//! spec): per-library content counts (`/libraries`) scoped to the caller's visibility,
//! the admin-only server snapshot (`/server`), and per-user reading statistics
//! (`/reading/*`, visible books only).

use crate::auth::RequireAuth;
use crate::dto::stats::{
    LibrariesStatsDto, LibraryStatsDto, LibraryStatsTotalDto, NamedValueDto, ReadingActivityDto,
    ReadingSummaryDto, ReadingTimeSeriesPointDto, ReadingTopsDto, ServerProcessStatsDto,
    ServerStatsDto, ServerTaskStatsDto, ServerTotalsDto, TaskTypeStatsDto,
};
use crate::error::ApiError;
use crate::service::metrics::TaskTypeMetrics;
use crate::service::reading_stats;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::{routing, Json, Router};
use komga_core::model::user::KomgaUser;
use komga_core::time_codec;
use komga_db::dao::reading_event::{ReadingEvent, ReadingEventDao};
use komga_db::dao::tasks::TasksDao;
use komga_db::dto_dao::library_stats::LibraryStatsDtoDao;
use komga_db::dto_dao::reading_stats::{ReadingStatsDtoDao, ReadingTotals};
use komga_db::search_sql::{content_restrictions_condition, library_ids_condition, SqlWhere};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use time::{Date, OffsetDateTime};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/stats/libraries", routing::get(libraries_stats))
        .route("/api/v1/stats/server", routing::get(server_stats))
        .route(
            "/api/v1/stats/reading/summary",
            routing::get(reading_summary),
        )
        .route(
            "/api/v1/stats/reading/activity",
            routing::get(reading_activity),
        )
        .route("/api/v1/stats/reading/tops", routing::get(reading_tops))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryParam {
    library_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivityParams {
    library_id: Option<String>,
    tz_offset_minutes: Option<String>,
}

/// The (book-level, series-level) visibility pair for the requesting user: library
/// sharing intersected with the optional libraryId filter, plus content restrictions —
/// the same composition the book search WHERE clause uses.
fn visibility(user: &KomgaUser, library_id: Option<&str>) -> (SqlWhere, SqlWhere) {
    let requested = library_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| BTreeSet::from([id.to_string()]));
    let authorized = user.get_authorized_library_ids(requested.as_ref());
    let book = content_restrictions_condition(&user.restrictions)
        .and(library_ids_condition("BOOK", authorized.as_ref()));
    let series = content_restrictions_condition(&user.restrictions)
        .and(library_ids_condition("SERIES", authorized.as_ref()));
    (book, series)
}

/// Per-library content counts (series/books/filesize) plus their total, all under the
/// caller's visibility: sharing and content restrictions apply exactly like search, so
/// the numbers match what the user can actually browse.
async fn libraries_stats(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<LibrariesStatsDto>, ApiError> {
    let user = &auth.0.user;
    let (book_visibility, series_visibility) = visibility(user, None);
    let authorized = user.get_authorized_library_ids(None);
    let dao = LibraryStatsDtoDao::new(state.db.clone());
    let rows = dao.per_library(authorized.as_ref(), &series_visibility, &book_visibility)?;
    let mut total = LibraryStatsTotalDto::default();
    let libraries = rows
        .into_iter()
        .map(|row| {
            total.series += row.series;
            total.books += row.books;
            total.file_size += row.filesize;
            LibraryStatsDto {
                library_id: row.library_id,
                name: row.library_name,
                series: row.series,
                books: row.books,
                file_size: row.filesize,
                readlists: row.readlists,
                collections: row.collections,
            }
        })
        .collect();
    // spanning lists count once per touched library, so the total is a distinct count, not a row sum
    let membership = dao.membership_totals(&series_visibility, &book_visibility)?;
    total.readlists = membership.readlists;
    total.collections = membership.collections;
    Ok(Json(LibrariesStatsDto { libraries, total }))
}

/// The admin server snapshot: task queue depth merged with per-type execution metrics,
/// process stats, and global content totals. ADMIN only, like the actuator metrics.
async fn server_stats(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<ServerStatsDto>, ApiError> {
    auth.0.require_admin()?;

    let queue = TasksDao::new(state.tasks_db.clone());
    let queue_size = queue.count()?;
    let mut by_type: BTreeMap<String, (i64, TaskTypeMetrics)> = queue
        .count_by_simple_type()?
        .into_iter()
        .map(|(task_type, queued)| (task_type, (queued, TaskTypeMetrics::default())))
        .collect();
    for (task_type, executed) in crate::service::metrics::task_metrics() {
        by_type.entry(task_type.to_string()).or_default().1 = executed;
    }
    let types = by_type
        .into_iter()
        .map(|(task_type, (queued, executed))| TaskTypeStatsDto {
            task_type,
            queued,
            executions: executed.executions,
            total_time_ms: executed.total.as_millis() as i64,
            max_time_ms: executed.max.as_millis() as i64,
            failures: executed.failures,
        })
        .collect();

    let process = ServerProcessStatsDto {
        start_time: OffsetDateTime::from_unix_timestamp_nanos(
            (crate::service::metrics::process_start().1 * 1_000_000.0) as i128,
        )
        .unwrap(),
        uptime_seconds: crate::service::metrics::process_start()
            .0
            .elapsed()
            .as_secs(),
        cpu_usage: crate::service::metrics::cpu_usage_percent(),
        memory_bytes: crate::service::metrics::rss_bytes(),
    };

    let totals = ServerTotalsDto {
        libraries: crate::service::metrics::count_of(&state, "LIBRARY"),
        collections: crate::service::metrics::count_of(&state, "COLLECTION"),
        readlists: crate::service::metrics::count_of(&state, "READLIST"),
        sidecars: crate::service::metrics::count_of(&state, "SIDECAR"),
    };

    Ok(Json(ServerStatsDto {
        tasks: ServerTaskStatsDto { queue_size, types },
        process,
        totals,
    }))
}

/// The user's full READING_EVENT history filtered to visible series. Visibility is
/// series-level, so the denormalized SERIES_ID filters the kmrs-side event log without
/// joining back to the main database.
fn visible_events(
    state: &AppState,
    user_id: &str,
    series_visibility: &SqlWhere,
) -> Result<Vec<ReadingEvent>, ApiError> {
    let visible_series =
        ReadingStatsDtoDao::new(state.db.clone()).visible_series_ids(series_visibility)?;
    let events = ReadingEventDao::new(state.kmrs_db.clone())
        .find_all_by_user(user_id)?
        .into_iter()
        .filter(|e| visible_series.contains(&e.series_id))
        .collect();
    Ok(events)
}

async fn reading_summary(
    State(state): State<AppState>,
    auth: RequireAuth,
    Query(params): Query<LibraryParam>,
) -> Result<Json<ReadingSummaryDto>, ApiError> {
    let user = &auth.0.user;
    let (book_visibility, series_visibility) = visibility(user, params.library_id.as_deref());
    let dao = ReadingStatsDtoDao::new(state.db.clone());
    let user_id = &user.id;
    let totals = dao.totals(user_id, &book_visibility)?;
    let read_dates = dao.read_dates(user_id, &book_visibility)?;
    let last_read_at = dao.last_read_date(user_id, &book_visibility)?;

    let today = time_codec::now_utc().date();
    let visible_series = dao.visible_series_ids(&series_visibility)?;
    let event_dates = ReadingEventDao::new(state.kmrs_db.clone())
        .find_activity_dates(user_id, &visible_series)?;
    let (current_streak_days, longest_streak_days) =
        reading_stats::streaks(&read_dates, &event_dates, today);
    let reading_days = read_dates
        .iter()
        .chain(&event_dates)
        .copied()
        .collect::<BTreeSet<_>>()
        .len() as i64;

    Ok(Json(ReadingSummaryDto {
        total_books: totals.total_books,
        books_started: totals.books_started,
        books_completed: totals.books_completed,
        pages_read: totals.pages_read,
        average_pages_per_book: average_pages_per_book(
            totals.completed_pages_read,
            totals.books_completed,
        ),
        reading_days,
        last_read_at,
        current_streak_days,
        longest_streak_days,
        status_distribution: status_distribution(&totals),
        generated_at: time_codec::now_utc(),
    }))
}

async fn reading_activity(
    State(state): State<AppState>,
    auth: RequireAuth,
    Query(params): Query<ActivityParams>,
) -> Result<Json<ReadingActivityDto>, ApiError> {
    let tz_offset_minutes = parse_tz_offset(params.tz_offset_minutes.as_deref())?;
    let user = &auth.0.user;
    let (book_visibility, series_visibility) = visibility(user, params.library_id.as_deref());
    let dao = ReadingStatsDtoDao::new(state.db.clone());
    let user_id = &user.id;
    let completions = dao.completions_by_day(user_id, &book_visibility)?;
    let progress_activity = dao.progress_activity(user_id, &book_visibility)?;
    let events = visible_events(&state, user_id, &series_visibility)?;

    // completed books without any event get their whole page count on the completion
    // day, so histories predating the event log are not blank
    let mut pages = reading_stats::pages_by_day(&events);
    let event_books: HashSet<&str> = events.iter().map(|e| e.book_id.as_str()).collect();
    for book in dao.completed_book_page_counts(user_id, &book_visibility)? {
        if !event_books.contains(book.book_id.as_str()) {
            *pages.entry(book.read_day).or_default() += book.page_count;
        }
    }
    let completions: BTreeMap<Date, i64> = completions.into_iter().collect();
    let (weekday_distribution, hourly_distribution) =
        activity_buckets(&events, &progress_activity, &event_books, tz_offset_minutes);

    Ok(Json(ReadingActivityDto {
        weekday_distribution,
        hourly_distribution,
        reading_time_series: time_series(&pages, &completions),
        generated_at: time_codec::now_utc(),
    }))
}

async fn reading_tops(
    State(state): State<AppState>,
    auth: RequireAuth,
    Query(params): Query<LibraryParam>,
) -> Result<Json<ReadingTopsDto>, ApiError> {
    let user = &auth.0.user;
    let (book_visibility, _) = visibility(user, params.library_id.as_deref());
    let dao = ReadingStatsDtoDao::new(state.db.clone());
    let user_id = &user.id;
    let genres = dao.genre_counts(user_id, &book_visibility)?;
    let tags = dao.tag_counts(user_id, &book_visibility)?;
    let authors = dao.author_counts(user_id, &book_visibility)?;

    Ok(Json(ReadingTopsDto {
        top_authors: top(&authors, 10),
        top_genres: top(&genres, 10),
        top_tags: top(&tags, 10),
        genre_distribution: with_other(&genres, 17),
        tag_distribution: with_other(&tags, 17),
        generated_at: time_codec::now_utc(),
    }))
}

/// -720..=840 covers every real-world UTC offset; anything else is a client bug.
fn parse_tz_offset(raw: Option<&str>) -> Result<i32, ApiError> {
    let Some(raw) = raw else { return Ok(0) };
    let value: i32 = raw.trim().parse().map_err(|_| {
        ApiError::bad_request("invalid 'tzOffsetMinutes', expected integer minutes")
    })?;
    if !(-720..=840).contains(&value) {
        return Err(ApiError::bad_request(
            "'tzOffsetMinutes' must be between -720 and 840",
        ));
    }
    Ok(value)
}

fn average_pages_per_book(completed_pages_read: i64, books_completed: i64) -> i64 {
    if books_completed == 0 {
        0
    } else {
        (completed_pages_read as f64 / books_completed as f64).round() as i64
    }
}

/// The order is fixed read → inProgress → unread; zero values stay in (the client
/// decides whether to hide them).
fn status_distribution(totals: &ReadingTotals) -> Vec<NamedValueDto> {
    [
        ("read", totals.books_completed),
        ("inProgress", totals.books_started - totals.books_completed),
        ("unread", totals.total_books - totals.books_started),
    ]
    .into_iter()
    .map(|(name, value)| NamedValueDto {
        name: name.to_string(),
        value,
    })
    .collect()
}

/// Activity timestamps bucketed by weekday and hour in the client's timezone. Every
/// event of books that have an event log counts, plus the READ_DATE of books without
/// one — the same per-book attribution as the time series.
fn activity_buckets(
    events: &[ReadingEvent],
    progress: &[(String, OffsetDateTime)],
    event_books: &HashSet<&str>,
    tz_offset_minutes: i32,
) -> ([i64; 7], [i64; 24]) {
    // parse_tz_offset already bounded the minutes to a range UtcOffset always accepts
    let offset = time::UtcOffset::from_whole_seconds(tz_offset_minutes * 60).unwrap();
    let mut weekday = [0i64; 7];
    let mut hourly = [0i64; 24];
    let mut count = |ts: OffsetDateTime| {
        let local = ts.to_offset(offset);
        weekday[local.weekday().number_days_from_sunday() as usize] += 1;
        hourly[local.hour() as usize] += 1;
    };
    for event in events {
        count(event.created_date);
    }
    for (book_id, read_date) in progress {
        if !event_books.contains(book_id.as_str()) {
            count(*read_date);
        }
    }
    (weekday, hourly)
}

/// Only days with activity get a point (the client zero-fills when rendering); the
/// series covers the full history.
fn time_series(
    pages: &BTreeMap<Date, i64>,
    completions: &BTreeMap<Date, i64>,
) -> Vec<ReadingTimeSeriesPointDto> {
    pages
        .keys()
        .chain(completions.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|day| ReadingTimeSeriesPointDto {
            date: time_codec::format_date(day),
            pages_read: pages.get(&day).copied().unwrap_or(0),
            books_completed: completions.get(&day).copied().unwrap_or(0),
        })
        .collect()
}

fn top(counts: &[(String, i64)], limit: usize) -> Vec<NamedValueDto> {
    counts
        .iter()
        .take(limit)
        .map(|(name, value)| NamedValueDto {
            name: name.clone(),
            value: *value,
        })
        .collect()
}

/// The full sorted list, or the first `limit` entries plus an "Other" bucket summing
/// the rest, so the payload stays bounded no matter how varied the library is.
fn with_other(counts: &[(String, i64)], limit: usize) -> Vec<NamedValueDto> {
    if counts.len() <= limit {
        return top(counts, limit);
    }
    let mut out = top(counts, limit);
    out.push(NamedValueDto {
        name: "Other".to_string(),
        value: counts[limit..].iter().map(|(_, value)| value).sum(),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::{insert_api_key, insert_user, TestApp};
    use axum::http::StatusCode;
    use komga_core::model::read_progress::ReadProgress;
    use komga_core::model::user::{AgeRestriction, AllowExclude, ContentRestrictions};
    use komga_core::time_codec::now_utc;
    use komga_db::dao::reading_event::NewReadingEvent;
    use komga_db::pool::Database;

    fn exec(db: &Database, sql: &str, params: impl rusqlite::Params) {
        db.rw().unwrap().execute(sql, params).unwrap();
    }

    fn at(date: Date, hour: u8) -> OffsetDateTime {
        date.with_hms(hour, 0, 0).unwrap().assume_utc()
    }

    fn seed_library(db: &Database, id: &str) {
        exec(
            db,
            "INSERT OR IGNORE INTO LIBRARY (ID, NAME, ROOT) VALUES (?, ?, 'file:/data/')",
            [id, id],
        );
    }

    fn seed_series(db: &Database, id: &str, library_id: &str, age_rating: Option<i32>) {
        seed_library(db, library_id);
        exec(
            db,
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID) \
             VALUES (?, ?, 'file:/data/s/', '2024-01-01 00:00:00.0', ?)",
            [id, id, library_id],
        );
        exec(
            db,
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, AGE_RATING) \
             VALUES (?, 'ONGOING', ?, ?, ?)",
            rusqlite::params![id, id, id, age_rating],
        );
    }

    fn seed_book(db: &Database, book_id: &str, series_id: &str, library_id: &str, page_count: i64) {
        exec(
            db,
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID) \
             VALUES (?, 'b', 'file:/data/s/b.cbz', '2024-01-01 00:00:00.0', ?, ?)",
            rusqlite::params![book_id, series_id, library_id],
        );
        exec(
            db,
            "INSERT INTO MEDIA (BOOK_ID, STATUS, PAGE_COUNT) VALUES (?, 'READY', ?)",
            rusqlite::params![book_id, page_count],
        );
    }

    fn seed_progress(
        db: &Database,
        book_id: &str,
        user_id: &str,
        page: i32,
        completed: bool,
        read_date: OffsetDateTime,
    ) {
        exec(
            db,
            "INSERT INTO READ_PROGRESS (BOOK_ID, USER_ID, PAGE, COMPLETED, READ_DATE) \
             VALUES (?,?,?,?,?)",
            rusqlite::params![
                book_id,
                user_id,
                page,
                completed,
                time_codec::format_datetime(read_date)
            ],
        );
    }

    fn seed_event(
        db: &Database,
        user_id: &str,
        book_id: &str,
        series_id: &str,
        page: i32,
        created_date: OffsetDateTime,
    ) {
        ReadingEventDao::new(db.clone())
            .insert(&NewReadingEvent {
                user_id: user_id.into(),
                book_id: book_id.into(),
                series_id: series_id.into(),
                page,
                created_date,
            })
            .unwrap();
    }

    fn user_app() -> (TestApp, String) {
        let app = TestApp::new(router());
        let user = insert_user(&app.state.db, "user@x.c", false, true, &[]);
        insert_api_key(&app.state.db, &user, "k");
        (app, user)
    }

    fn named_values(body: &serde_json::Value, key: &str) -> Vec<(String, i64)> {
        body[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["name"].as_str().unwrap().to_string(),
                    e["value"].as_i64().unwrap(),
                )
            })
            .collect()
    }

    /// s1 (l1): b1 completed with events, b2 in progress. s2 (l1): b3 completed without
    /// events, b4 unread.
    fn seed_full(db: &Database, kmrs_db: &Database, user: &str, today: Date) {
        seed_series(db, "s1", "l1", None);
        seed_series(db, "s2", "l1", None);
        seed_book(db, "b1", "s1", "l1", 100);
        seed_book(db, "b2", "s1", "l1", 80);
        seed_book(db, "b3", "s2", "l1", 50);
        seed_book(db, "b4", "s2", "l1", 60);
        let days_ago = |n: i64, hour: u8| at(today - time::Duration::days(n), hour);
        seed_progress(db, "b1", user, 100, true, days_ago(1, 15));
        seed_progress(db, "b2", user, 30, false, days_ago(2, 9));
        seed_progress(db, "b3", user, 50, true, days_ago(0, 8));
        seed_event(kmrs_db, user, "b1", "s1", 40, days_ago(3, 10));
        seed_event(kmrs_db, user, "b1", "s1", 70, days_ago(1, 11));
        seed_event(kmrs_db, user, "b1", "s1", 100, days_ago(1, 12));
        exec(db, "INSERT INTO SERIES_METADATA_GENRE (SERIES_ID, GENRE) VALUES ('s1', 'action'), ('s2', 'drama')", []);
        exec(
            db,
            "INSERT INTO SERIES_METADATA_TAG (SERIES_ID, TAG) VALUES ('s1', 'favorite')",
            [],
        );
        exec(
            db,
            "INSERT INTO BOOK_METADATA_TAG (BOOK_ID, TAG) VALUES ('b1', 'classic')",
            [],
        );
        exec(db, "INSERT INTO BOOK_METADATA_AGGREGATION_AUTHOR (SERIES_ID, NAME, ROLE) VALUES ('s1', 'author-a', 'writer')", []);
        exec(db, "INSERT INTO BOOK_METADATA_AUTHOR (BOOK_ID, NAME, ROLE) VALUES ('b3', 'author-c', 'artist')", []);
    }

    #[tokio::test]
    async fn summary_assembles_totals_status_and_streaks() {
        let (app, user) = user_app();
        let today = now_utc().date();
        seed_full(&app.state.db, &app.state.kmrs_db, &user, today);

        let (status, body) = app.get_json("/api/v1/stats/reading/summary", "k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalBooks"], 4);
        assert_eq!(body["booksStarted"], 3);
        assert_eq!(body["booksCompleted"], 2);
        // completed books count their page count, the in-progress one its current page
        assert_eq!(body["pagesRead"], 150 + 30);
        // the average only divides completed pages over completed books
        assert_eq!(body["averagePagesPerBook"], 75);
        // read dates today-2..today plus the event-only day today-3
        assert_eq!(body["readingDays"], 4);
        assert_eq!(
            body["lastReadAt"],
            time_codec::format_dto_datetime(at(today, 8))
        );
        assert_eq!(body["currentStreakDays"], 4);
        assert_eq!(body["longestStreakDays"], 4);
        assert_eq!(
            named_values(&body, "statusDistribution"),
            [
                ("read".to_string(), 2),
                ("inProgress".to_string(), 1),
                ("unread".to_string(), 1)
            ]
        );
        assert!(body["generatedAt"].as_str().unwrap().ends_with('Z'));
    }

    #[tokio::test]
    async fn summary_is_zeroed_for_a_fresh_user() {
        let (app, _) = user_app();
        let (status, body) = app.get_json("/api/v1/stats/reading/summary", "k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalBooks"], 0);
        assert_eq!(body["booksStarted"], 0);
        assert_eq!(body["booksCompleted"], 0);
        assert_eq!(body["pagesRead"], 0);
        assert_eq!(body["averagePagesPerBook"], 0);
        assert_eq!(body["readingDays"], 0);
        assert_eq!(body["currentStreakDays"], 0);
        assert_eq!(body["longestStreakDays"], 0);
        assert_eq!(body["lastReadAt"], serde_json::Value::Null);
        // all three statuses stay in, zero values included
        assert_eq!(
            named_values(&body, "statusDistribution"),
            [
                ("read".to_string(), 0),
                ("inProgress".to_string(), 0),
                ("unread".to_string(), 0)
            ]
        );
    }

    #[tokio::test]
    async fn activity_buckets_time_and_builds_a_sparse_series() {
        let (app, user) = user_app();
        let today = now_utc().date();
        seed_full(&app.state.db, &app.state.kmrs_db, &user, today);

        let (status, body) = app.get_json("/api/v1/stats/reading/activity", "k").await;
        assert_eq!(status, StatusCode::OK);

        // activity: b1's events (today-3 10:00, today-1 11:00 and 12:00), b2's read_date
        // (today-2 09:00) and b3's read_date (today 08:00) — b1 has events, so its
        // read_date does not count
        let weekday = |d: Date| d.weekday().number_days_from_sunday() as usize;
        let daily = body["weekdayDistribution"].as_array().unwrap();
        assert_eq!(daily.len(), 7);
        assert_eq!(daily[weekday(today - time::Duration::days(3))], 1);
        assert_eq!(daily[weekday(today - time::Duration::days(2))], 1);
        assert_eq!(daily[weekday(today - time::Duration::days(1))], 2);
        assert_eq!(daily[weekday(today)], 1);
        let hourly = body["hourlyDistribution"].as_array().unwrap();
        assert_eq!(hourly.len(), 24);
        for (hour, count) in [(8, 1), (9, 1), (10, 1), (11, 1), (12, 1)] {
            assert_eq!(hourly[hour], count, "hour {hour}");
        }

        // sparse series: only days with activity get a point
        let series = body["readingTimeSeries"].as_array().unwrap();
        let expected: Vec<(Date, i64, i64)> = vec![
            (today - time::Duration::days(3), 40, 0),
            (today - time::Duration::days(1), 60, 1),
            (today, 50, 1),
        ];
        assert_eq!(series.len(), expected.len());
        for (point, (date, pages, completed)) in series.iter().zip(expected) {
            assert_eq!(point["date"], time_codec::format_date(date));
            assert_eq!(point["pagesRead"], pages);
            assert_eq!(point["booksCompleted"], completed);
        }
        assert!(body["generatedAt"].as_str().unwrap().ends_with('Z'));
    }

    #[tokio::test]
    async fn activity_is_zeroed_for_a_fresh_user() {
        let (app, _) = user_app();
        let (status, body) = app.get_json("/api/v1/stats/reading/activity", "k").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["weekdayDistribution"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d == 0));
        assert!(body["hourlyDistribution"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d == 0));
        assert_eq!(body["readingTimeSeries"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn tops_count_each_name_once_per_series() {
        let (app, user) = user_app();
        let today = now_utc().date();
        seed_full(&app.state.db, &app.state.kmrs_db, &user, today);

        let (status, body) = app.get_json("/api/v1/stats/reading/tops", "k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            named_values(&body, "topGenres"),
            [("action".to_string(), 1), ("drama".to_string(), 1)]
        );
        assert_eq!(
            named_values(&body, "topTags"),
            [("classic".to_string(), 1), ("favorite".to_string(), 1)]
        );
        assert_eq!(
            named_values(&body, "topAuthors"),
            [("author-a".to_string(), 1), ("author-c".to_string(), 1)]
        );
        assert_eq!(
            named_values(&body, "genreDistribution"),
            named_values(&body, "topGenres")
        );
        assert_eq!(
            named_values(&body, "tagDistribution"),
            named_values(&body, "topTags")
        );
        assert!(body["generatedAt"].as_str().unwrap().ends_with('Z'));
    }

    #[tokio::test]
    async fn tops_distribution_caps_at_17_plus_other() {
        let (app, user) = user_app();
        let today = now_utc().date();
        for i in 0..19 {
            let series = format!("s{i:02}");
            let book = format!("b{i:02}");
            let genre = format!("g{i:02}");
            seed_series(&app.state.db, &series, "l1", None);
            seed_book(&app.state.db, &book, &series, "l1", 10);
            seed_progress(&app.state.db, &book, &user, 10, true, at(today, 8));
            exec(
                &app.state.db,
                "INSERT INTO SERIES_METADATA_GENRE (SERIES_ID, GENRE) VALUES (?, ?)",
                rusqlite::params![series, genre],
            );
        }

        let (status, body) = app.get_json("/api/v1/stats/reading/tops", "k").await;
        assert_eq!(status, StatusCode::OK);
        let top = named_values(&body, "topGenres");
        assert_eq!(top.len(), 10);
        assert_eq!(top[0], ("g00".to_string(), 1));
        assert_eq!(top[9], ("g09".to_string(), 1));
        let distribution = named_values(&body, "genreDistribution");
        assert_eq!(distribution.len(), 18);
        assert_eq!(distribution[16], ("g16".to_string(), 1));
        assert_eq!(distribution[17], ("Other".to_string(), 2));
    }

    #[tokio::test]
    async fn visibility_restricts_every_endpoint() {
        let app = TestApp::new(router());
        let today = now_utc().date();
        // the shared library must exist before the user row references it
        seed_library(&app.state.db, "l1");
        // the user only shares l1 and excludes 18+ series: s2 (l2) and s3 (age 21) are invisible
        let user = crate::api::collections::tests::insert_user(
            &app.state.db,
            "kid@x.c",
            &[],
            &["l1"],
            ContentRestrictions::new(
                Some(AgeRestriction {
                    age: 18,
                    restriction: AllowExclude::Exclude,
                }),
                Default::default(),
                Default::default(),
            ),
            "k",
        );
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book(&app.state.db, "b1", "s1", "l1", 100);
        seed_series(&app.state.db, "s2", "l2", None);
        seed_book(&app.state.db, "b2", "s2", "l2", 50);
        seed_series(&app.state.db, "s3", "l1", Some(21));
        seed_book(&app.state.db, "b3", "s3", "l1", 60);
        let days_ago = |n: i64, hour: u8| at(today - time::Duration::days(n), hour);
        seed_progress(&app.state.db, "b1", &user, 100, true, days_ago(1, 15));
        seed_progress(&app.state.db, "b2", &user, 50, true, days_ago(1, 16));
        seed_progress(&app.state.db, "b3", &user, 60, true, days_ago(0, 9));
        seed_event(&app.state.kmrs_db, &user, "b1", "s1", 40, days_ago(1, 10));
        seed_event(&app.state.kmrs_db, &user, "b2", "s2", 30, days_ago(0, 10));
        exec(&app.state.db, "INSERT INTO SERIES_METADATA_GENRE (SERIES_ID, GENRE) VALUES ('s1', 'action'), ('s2', 'drama'), ('s3', 'horror')", []);

        let (status, body) = app.get_json("/api/v1/stats/reading/summary", "k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalBooks"], 1);
        assert_eq!(body["booksCompleted"], 1);
        assert_eq!(body["pagesRead"], 100);
        assert_eq!(
            body["lastReadAt"],
            time_codec::format_dto_datetime(days_ago(1, 15))
        );
        // invisible completions and events leave no trace in the streaks either
        assert_eq!(body["currentStreakDays"], 1);
        assert_eq!(body["readingDays"], 1);

        let (status, body) = app.get_json("/api/v1/stats/reading/activity", "k").await;
        assert_eq!(status, StatusCode::OK);
        // b1's event at today-1 10:00 is the only visible activity
        let weekday = |d: Date| d.weekday().number_days_from_sunday() as usize;
        let daily = body["weekdayDistribution"].as_array().unwrap();
        assert_eq!(daily[weekday(today - time::Duration::days(1))], 1);
        assert_eq!(daily[weekday(today)], 0);
        assert_eq!(body["hourlyDistribution"].as_array().unwrap()[10], 1);
        let series = body["readingTimeSeries"].as_array().unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(
            series[0]["date"],
            time_codec::format_date(today - time::Duration::days(1))
        );
        assert_eq!(series[0]["pagesRead"], 40);
        assert_eq!(series[0]["booksCompleted"], 1);

        let (status, body) = app.get_json("/api/v1/stats/reading/tops", "k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            named_values(&body, "topGenres"),
            [("action".to_string(), 1)]
        );
    }

    #[tokio::test]
    async fn library_id_filter_intersects_with_the_authorized_set() {
        let (app, user) = user_app();
        let today = now_utc().date();
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book(&app.state.db, "b1", "s1", "l1", 100);
        seed_series(&app.state.db, "s2", "l2", None);
        seed_book(&app.state.db, "b2", "s2", "l2", 50);
        seed_progress(&app.state.db, "b1", &user, 100, true, at(today, 10));
        seed_progress(&app.state.db, "b2", &user, 50, true, at(today, 11));

        let (status, body) = app
            .get_json("/api/v1/stats/reading/summary?libraryId=l1", "k")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalBooks"], 1);
        assert_eq!(body["pagesRead"], 100);

        // a library the user cannot see intersects to the empty set, like search
        let (status, body) = app
            .get_json("/api/v1/stats/reading/summary?libraryId=unknown", "k")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalBooks"], 0);
        let (status, body) = app
            .get_json("/api/v1/stats/reading/activity?libraryId=unknown", "k")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["readingTimeSeries"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn tz_offset_shifts_only_weekday_and_hour_buckets() {
        let (app, user) = user_app();
        let today = now_utc().date();
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book(&app.state.db, "b1", "s1", "l1", 100);
        // completed yesterday at 23:30 UTC
        seed_progress(
            &app.state.db,
            "b1",
            &user,
            100,
            true,
            at(today - time::Duration::days(1), 23) + time::Duration::minutes(30),
        );

        let weekday = |d: Date| d.weekday().number_days_from_sunday() as usize;
        let (_, body) = app.get_json("/api/v1/stats/reading/activity", "k").await;
        let daily = body["weekdayDistribution"].as_array().unwrap();
        let hourly = body["hourlyDistribution"].as_array().unwrap();
        assert_eq!(daily[weekday(today - time::Duration::days(1))], 1);
        assert_eq!(hourly[23], 1);
        // the time series day itself stays UTC
        let series = body["readingTimeSeries"].as_array().unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(
            series[0]["date"],
            time_codec::format_date(today - time::Duration::days(1))
        );

        let (_, body) = app
            .get_json("/api/v1/stats/reading/activity?tzOffsetMinutes=60", "k")
            .await;
        let daily = body["weekdayDistribution"].as_array().unwrap();
        let hourly = body["hourlyDistribution"].as_array().unwrap();
        // 23:30 UTC + 60 minutes lands at 00:30 local on the next day
        assert_eq!(daily[weekday(today)], 1);
        assert_eq!(hourly[0], 1);

        for uri in [
            "/api/v1/stats/reading/activity?tzOffsetMinutes=abc",
            "/api/v1/stats/reading/activity?tzOffsetMinutes=1000",
            "/api/v1/stats/reading/activity?tzOffsetMinutes=-721",
        ] {
            let (status, _) = app.get_json(uri, "k").await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "uri {uri}");
        }
    }

    #[tokio::test]
    async fn endpoints_require_authentication() {
        let (app, _) = user_app();
        let raw = router()
            .layer(axum::middleware::from_fn_with_state(
                app.state.clone(),
                crate::auth::auth_middleware,
            ))
            .with_state(app.state.clone());
        for uri in [
            "/api/v1/stats/libraries",
            "/api/v1/stats/server",
            "/api/v1/stats/reading/summary",
            "/api/v1/stats/reading/activity",
            "/api/v1/stats/reading/tops",
        ] {
            let response = tower::ServiceExt::oneshot(
                raw.clone(),
                axum::http::Request::get(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "uri {uri}");
        }
    }

    fn seed_book_sized(
        db: &Database,
        book_id: &str,
        series_id: &str,
        library_id: &str,
        file_size: i64,
    ) {
        exec(
            db,
            "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, FILE_SIZE) \
             VALUES (?, 'b', 'file:/data/s/b.cbz', '2024-01-01 00:00:00.0', ?, ?, ?)",
            rusqlite::params![book_id, series_id, library_id, file_size],
        );
    }

    fn seed_sidecar(db: &Database, url: &str, library_id: &str) {
        exec(
            db,
            "INSERT INTO SIDECAR (URL, PARENT_URL, LAST_MODIFIED_TIME, LIBRARY_ID) \
             VALUES (?, 'file:/data/s/b.cbz', '2024-01-01', ?)",
            rusqlite::params![url, library_id],
        );
    }

    fn admin_app() -> TestApp {
        let app = TestApp::new(router());
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k");
        app
    }

    #[tokio::test]
    async fn libraries_stats_scope_to_sharing_and_restrictions() {
        let app = TestApp::new(router());
        // the shared library must exist before the user row references it
        seed_library(&app.state.db, "l1");
        crate::api::collections::tests::insert_user(
            &app.state.db,
            "kid@x.c",
            &[],
            &["l1"],
            ContentRestrictions::new(
                Some(AgeRestriction {
                    age: 18,
                    restriction: AllowExclude::Exclude,
                }),
                Default::default(),
                Default::default(),
            ),
            "k",
        );
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book_sized(&app.state.db, "b1", "s1", "l1", 100);
        seed_series(&app.state.db, "s3", "l1", Some(21));
        seed_book_sized(&app.state.db, "b3", "s3", "l1", 300);
        seed_series(&app.state.db, "s2", "l2", None);
        seed_book_sized(&app.state.db, "b2", "s2", "l2", 200);

        let (status, body) = app.get_json("/api/v1/stats/libraries", "k").await;
        assert_eq!(status, StatusCode::OK);
        let libraries = body["libraries"].as_array().unwrap();
        assert_eq!(libraries.len(), 1);
        assert_eq!(libraries[0]["libraryId"], "l1");
        assert_eq!(libraries[0]["name"], "l1");
        // s3 (21+) and its book are invisible, so they leave no trace in the counts
        assert_eq!(libraries[0]["series"], 1);
        assert_eq!(libraries[0]["books"], 1);
        assert_eq!(libraries[0]["fileSize"], 100);
        assert_eq!(
            body["total"],
            serde_json::json!({"series": 1, "books": 1, "fileSize": 100, "readlists": 0, "collections": 0})
        );
    }

    #[tokio::test]
    async fn libraries_stats_empty_for_user_without_libraries() {
        let app = TestApp::new(router());
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book_sized(&app.state.db, "b1", "s1", "l1", 100);
        let user = insert_user(&app.state.db, "none@x.c", false, false, &[]);
        insert_api_key(&app.state.db, &user, "k");

        let (status, body) = app.get_json("/api/v1/stats/libraries", "k").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["libraries"], serde_json::json!([]));
        assert_eq!(
            body["total"],
            serde_json::json!({"series": 0, "books": 0, "fileSize": 0, "readlists": 0, "collections": 0})
        );
    }

    #[tokio::test]
    async fn libraries_stats_admin_gets_every_library_zero_filled() {
        let app = admin_app();
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book_sized(&app.state.db, "b1", "s1", "l1", 100);
        seed_library(&app.state.db, "l2");

        let (status, body) = app.get_json("/api/v1/stats/libraries", "k").await;
        assert_eq!(status, StatusCode::OK);
        let libraries = body["libraries"].as_array().unwrap();
        assert_eq!(libraries.len(), 2);
        // the empty library reports zeros instead of vanishing like the actuator MultiGauge
        assert_eq!(libraries[1]["libraryId"], "l2");
        assert_eq!(libraries[1]["series"], 0);
        assert_eq!(libraries[1]["books"], 0);
        assert_eq!(libraries[1]["fileSize"], 0);
        assert_eq!(
            body["total"],
            serde_json::json!({"series": 1, "books": 1, "fileSize": 100, "readlists": 0, "collections": 0})
        );
    }

    fn seed_readlist(db: &Database, id: &str, book_ids: &[&str]) {
        exec(
            db,
            "INSERT INTO READLIST (ID, NAME, BOOK_COUNT) VALUES (?, ?, ?)",
            rusqlite::params![id, id, book_ids.len() as i64],
        );
        for (number, book_id) in book_ids.iter().enumerate() {
            exec(
                db,
                "INSERT INTO READLIST_BOOK (READLIST_ID, BOOK_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, book_id, number as i64],
            );
        }
    }

    fn seed_collection(db: &Database, id: &str, series_ids: &[&str]) {
        exec(
            db,
            "INSERT INTO COLLECTION (ID, NAME, SERIES_COUNT) VALUES (?, ?, ?)",
            rusqlite::params![id, id, series_ids.len() as i64],
        );
        for (number, series_id) in series_ids.iter().enumerate() {
            exec(
                db,
                "INSERT INTO COLLECTION_SERIES (COLLECTION_ID, SERIES_ID, NUMBER) VALUES (?, ?, ?)",
                rusqlite::params![id, series_id, number as i64],
            );
        }
    }

    /// r1/c1 span l1+l2; r2/c2 hold only restricted members; r3 is empty and belongs to nothing.
    fn seed_lists_fixture(db: &Database) {
        seed_series(db, "s1", "l1", None);
        seed_book_sized(db, "b1", "s1", "l1", 100);
        seed_series(db, "s3", "l1", Some(21));
        seed_book_sized(db, "b3", "s3", "l1", 300);
        seed_series(db, "s2", "l2", None);
        seed_book_sized(db, "b2", "s2", "l2", 200);
        seed_readlist(db, "r1", &["b1", "b2"]);
        seed_readlist(db, "r2", &["b3"]);
        seed_readlist(db, "r3", &[]);
        seed_collection(db, "c1", &["s1", "s2"]);
        seed_collection(db, "c2", &["s3"]);
    }

    #[tokio::test]
    async fn libraries_stats_lists_belong_to_every_touched_library() {
        let app = admin_app();
        seed_lists_fixture(&app.state.db);

        let (status, body) = app.get_json("/api/v1/stats/libraries", "k").await;
        assert_eq!(status, StatusCode::OK);
        let libraries = body["libraries"].as_array().unwrap();
        assert_eq!(libraries[0]["libraryId"], "l1");
        assert_eq!(libraries[0]["readlists"], 2); // r1 via b1, r2 via b3
        assert_eq!(libraries[0]["collections"], 2); // c1 via s1, c2 via s3
        assert_eq!(libraries[1]["libraryId"], "l2");
        assert_eq!(libraries[1]["readlists"], 1); // r1 via b2
        assert_eq!(libraries[1]["collections"], 1); // c1 via s2

        // r1/c1 count in both libraries, so the total is the distinct count, not the row sum
        assert_eq!(
            body["total"],
            serde_json::json!({"series": 3, "books": 3, "fileSize": 600, "readlists": 2, "collections": 2})
        );
    }

    #[tokio::test]
    async fn libraries_stats_lists_follow_content_restrictions() {
        let app = TestApp::new(router());
        // the shared library must exist before the user row references it
        seed_library(&app.state.db, "l1");
        crate::api::collections::tests::insert_user(
            &app.state.db,
            "kid@x.c",
            &[],
            &["l1"],
            ContentRestrictions::new(
                Some(AgeRestriction {
                    age: 18,
                    restriction: AllowExclude::Exclude,
                }),
                Default::default(),
                Default::default(),
            ),
            "k",
        );
        seed_lists_fixture(&app.state.db);

        let (status, body) = app.get_json("/api/v1/stats/libraries", "k").await;
        assert_eq!(status, StatusCode::OK);
        let libraries = body["libraries"].as_array().unwrap();
        assert_eq!(libraries.len(), 1);
        // r2's only book is 21+ and c2's only series is 21+: neither list counts for the kid
        assert_eq!(libraries[0]["readlists"], 1);
        assert_eq!(libraries[0]["collections"], 1);
        assert_eq!(
            body["total"],
            serde_json::json!({"series": 1, "books": 1, "fileSize": 100, "readlists": 1, "collections": 1})
        );
    }

    #[tokio::test]
    async fn libraries_stats_lists_count_only_libraries_with_visible_members() {
        let app = TestApp::new(router());
        // the shared libraries must exist before the user row references them
        seed_library(&app.state.db, "l1");
        seed_library(&app.state.db, "l2");
        crate::api::collections::tests::insert_user(
            &app.state.db,
            "kid@x.c",
            &[],
            &["l1", "l2"],
            ContentRestrictions::new(
                Some(AgeRestriction {
                    age: 18,
                    restriction: AllowExclude::Exclude,
                }),
                Default::default(),
                Default::default(),
            ),
            "k",
        );
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book_sized(&app.state.db, "b1", "s1", "l1", 100);
        seed_series(&app.state.db, "s4", "l2", Some(21));
        seed_book_sized(&app.state.db, "b4", "s4", "l2", 400);
        seed_readlist(&app.state.db, "r1", &["b1", "b4"]);
        seed_collection(&app.state.db, "c1", &["s1", "s4"]);

        let (status, body) = app.get_json("/api/v1/stats/libraries", "k").await;
        assert_eq!(status, StatusCode::OK);
        let libraries = body["libraries"].as_array().unwrap();
        assert_eq!(libraries.len(), 2);
        // r1/c1 count in l1 via b1/s1; their only l2 members are 21+, so l2 gets nothing
        assert_eq!(libraries[0]["readlists"], 1);
        assert_eq!(libraries[0]["collections"], 1);
        assert_eq!(libraries[1]["readlists"], 0);
        assert_eq!(libraries[1]["collections"], 0);
        assert_eq!(
            body["total"],
            serde_json::json!({"series": 1, "books": 1, "fileSize": 100, "readlists": 1, "collections": 1})
        );
    }

    #[tokio::test]
    async fn server_stats_requires_admin() {
        let (app, _) = user_app();
        let (status, _) = app.get("/api/v1/stats/server", "k").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn server_stats_merges_queue_and_execution_by_type() {
        let app = admin_app();
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book_sized(&app.state.db, "b1", "s1", "l1", 100);
        seed_sidecar(&app.state.db, "file:/data/s/b.json", "l1");
        seed_library(&app.state.db, "l2");
        exec(
            &app.state.tasks_db,
            "INSERT INTO TASK (ID, PRIORITY, CLASS, SIMPLE_TYPE, PAYLOAD) VALUES \
             ('t1', 5, 'c', 'StatsExecTask', '{}'), \
             ('t2', 5, 'c', 'StatsExecTask', '{}'), \
             ('t3', 5, 'c', 'StatsQueueTask', '{}')",
            [],
        );
        // a made-up type keeps the assertions deterministic: other tests in this binary
        // record real task types into the same process-global registry
        crate::service::metrics::record_task_execution(
            "StatsExecTask",
            std::time::Duration::from_millis(120),
            true,
        );
        crate::service::metrics::record_task_execution(
            "StatsExecTask",
            std::time::Duration::ZERO,
            false,
        );

        let (status, body) = app.get_json("/api/v1/stats/server", "k").await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(body["tasks"]["queueSize"], 3);
        let types = body["tasks"]["types"].as_array().unwrap();
        let find = |name: &str| types.iter().find(|t| t["type"] == name).cloned().unwrap();
        let executed = find("StatsExecTask");
        assert_eq!(executed["queued"], 2);
        assert_eq!(executed["executions"], 1);
        assert_eq!(executed["totalTimeMs"], 120);
        assert_eq!(executed["maxTimeMs"], 120);
        assert_eq!(executed["failures"], 1);
        let queued_only = find("StatsQueueTask");
        assert_eq!(queued_only["queued"], 1);
        assert_eq!(queued_only["executions"], 0);
        assert_eq!(queued_only["failures"], 0);

        assert!(body["process"]["startTime"]
            .as_str()
            .unwrap()
            .ends_with('Z'));
        assert!(body["process"]["uptimeSeconds"].as_u64().is_some());
        assert!(body["process"]["cpuUsage"].as_f64().unwrap() >= 0.0);
        assert!(body["process"]["memoryBytes"].as_i64().unwrap() > 0);

        assert_eq!(body["totals"]["libraries"], 2);
        assert_eq!(body["totals"]["collections"], 0);
        assert_eq!(body["totals"]["readlists"], 0);
        assert_eq!(body["totals"]["sidecars"], 1);
    }

    fn progress(user_id: &str, book_id: &str, page: i32) -> ReadProgress {
        ReadProgress {
            book_id: book_id.into(),
            user_id: user_id.into(),
            page,
            completed: false,
            // fixed and distinct from now, so the consumer test proves the event keeps
            // the caller-reported read date instead of the consume time
            read_date: time_codec::parse_datetime_utc("2026-09-15 10:00:00").unwrap(),
            device_id: "dev-1".into(),
            device_name: "Tablet".into(),
            locator: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    /// The consumer subscribes inside its task; retry the send until a receiver exists.
    async fn send_progress(state: &AppState, user_id: &str, book_id: &str, page: i32) {
        for _ in 0..200 {
            if state
                .events
                .send(crate::events::DomainEvent::ReadProgressChanged(progress(
                    user_id, book_id, page,
                )))
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("consumer did not subscribe in time");
    }

    async fn wait_for_events(
        db: &Database,
        user_id: &str,
        want: usize,
    ) -> Vec<komga_db::dao::reading_event::ReadingEvent> {
        let dao = ReadingEventDao::new(db.clone());
        for _ in 0..200 {
            let rows = dao.find_all_by_user(user_id).unwrap();
            if rows.len() >= want {
                return rows;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {want} reading events");
    }

    #[tokio::test]
    async fn consume_events_records_progress_and_skips_missing_books() {
        let (app, user) = user_app();
        seed_series(&app.state.db, "s1", "l1", None);
        seed_book(&app.state.db, "b1", "s1", "l1", 100);
        let handle = reading_stats::consume_events(app.state.clone());

        send_progress(&app.state, &user, "b1", 42).await;
        let rows = wait_for_events(&app.state.kmrs_db, &user, 1).await;
        assert_eq!(rows[0].series_id, "s1");
        assert_eq!(rows[0].page, 42);
        assert_eq!(
            rows[0].created_date,
            time_codec::parse_datetime_utc("2026-09-15 10:00:00").unwrap()
        );

        // the consumer is serial: once the follow-up b1 event lands, the missing-book
        // event has already been processed (and skipped), so the total stays 2
        send_progress(&app.state, &user, "b-missing", 10).await;
        send_progress(&app.state, &user, "b1", 50).await;
        let rows = wait_for_events(&app.state.kmrs_db, &user, 2).await;
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|e| e.book_id == "b1"));
        assert_eq!(rows[1].page, 50);

        handle.abort();
    }
}
