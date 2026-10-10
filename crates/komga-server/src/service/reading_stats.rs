//! Reading statistics, a kmrs-private enhancement with no Java equivalent. Every
//! `ReadProgressChanged` event is appended to the kmrs-side READING_EVENT log: the main
//! database only keeps the latest position per book, so per-day page counts would
//! otherwise be lost. The per-user aggregation itself lives in `api::stats`.

use crate::events::DomainEvent;
use crate::state::AppState;
use komga_core::model::read_progress::ReadProgress;
use komga_db::dao::book::BookDao;
use komga_db::dao::reading_event::{NewReadingEvent, ReadingEvent, ReadingEventDao};
use std::collections::BTreeMap;
use time::Date;

/// Feeds on a lossless tap of the event bus and appends one READING_EVENT per
/// progress change: the log is append-only with no rebuild path, so a dropped
/// event would undercount the day's pages forever. `ReadProgressDeleted` records
/// nothing: the pages of a deleted book were already counted on the days they
/// were read.
pub fn consume_events(
    state: AppState,
    mut events: tokio::sync::mpsc::UnboundedReceiver<DomainEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Some(DomainEvent::ReadProgressChanged(progress)) => {
                    let state = state.clone();
                    let result =
                        tokio::task::spawn_blocking(move || record(&state, &progress)).await;
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => tracing::warn!("failed to record reading event: {e}"),
                        Err(e) => tracing::error!("reading stats event task failed: {e}"),
                    }
                }
                Some(_) => {}
                // all senders are gone (shutdown)
                None => break,
            }
        }
    })
}

fn record(state: &AppState, progress: &ReadProgress) -> komga_db::Result<()> {
    // the series id is denormalized onto the event so the aggregation never joins back
    // to the main database; a book already gone here (deleted between the progress
    // write and the event) simply leaves no event
    let Some(book) = BookDao::new(state.db.clone()).find_by_id(&progress.book_id)? else {
        return Ok(());
    };
    ReadingEventDao::new(state.kmrs_db.clone()).insert(&NewReadingEvent {
        user_id: progress.user_id.clone(),
        book_id: progress.book_id.clone(),
        series_id: book.series_id,
        page: progress.page,
        // read_date is caller-reported (Readium/Kobo can sync offline reading long
        // after the fact); the consume time would attribute those to the sync day
        created_date: progress.read_date,
    })
}

/// Current and longest day-streak over the two activity date lists merged and deduped.
/// The current streak is still alive when its last day is yesterday: today's reading
/// may simply not have happened yet.
pub(crate) fn streaks(progress_dates: &[Date], event_dates: &[Date], today: Date) -> (i64, i64) {
    let mut dates: Vec<Date> = progress_dates.iter().chain(event_dates).copied().collect();
    dates.sort_unstable();
    dates.dedup();

    let mut longest = 0i64;
    let mut run = 0i64;
    let mut previous: Option<Date> = None;
    for date in &dates {
        run = match previous {
            Some(p) if p.next_day() == Some(*date) => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        previous = Some(*date);
    }

    // after the loop `run` is the length of the trailing run
    let current = match dates.last() {
        Some(&last) if last >= today - time::Duration::days(1) => run,
        _ => 0,
    };
    (current, longest)
}

/// Pages read per event date over the full event history. Each event contributes its
/// forward page delta against the previous event of the same book (the first one starts
/// from 0); a page decrease means the book was restarted and contributes nothing.
/// `events` must be ordered by (book, time, id), which is
/// `ReadingEventDao::find_all_by_user`'s output contract.
pub(crate) fn pages_by_day(events: &[ReadingEvent]) -> BTreeMap<Date, i64> {
    let mut by_book: BTreeMap<&str, Vec<&ReadingEvent>> = BTreeMap::new();
    for event in events {
        by_book
            .entry(event.book_id.as_str())
            .or_default()
            .push(event);
    }
    let mut pages: BTreeMap<Date, i64> = BTreeMap::new();
    for book_events in by_book.values() {
        let mut previous_page = 0;
        for event in book_events {
            let delta = i64::from((event.page - previous_page).max(0));
            previous_page = event.page;
            if delta > 0 {
                *pages.entry(event.created_date.date()).or_default() += delta;
            }
        }
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests as shared;
    use komga_core::time_codec::{parse_date, parse_datetime_utc};

    fn day(s: &str) -> Date {
        parse_date(s).unwrap()
    }

    fn days(dates: &[&str]) -> Vec<Date> {
        dates.iter().map(|s| day(s)).collect()
    }

    #[test]
    fn streaks_empty_and_single_day() {
        let today = day("2026-10-02");
        assert_eq!(streaks(&[], &[], today), (0, 0));
        assert_eq!(streaks(&days(&["2026-10-02"]), &[], today), (1, 1));
    }

    #[test]
    fn streaks_current_may_end_today_or_yesterday() {
        let today = day("2026-10-02");
        let run = days(&["2026-09-30", "2026-10-01", "2026-10-02"]);
        assert_eq!(streaks(&run, &[], today), (3, 3));

        let ending_yesterday = days(&["2026-09-30", "2026-10-01"]);
        assert_eq!(streaks(&ending_yesterday, &[], today), (2, 2));

        // a streak that ended the day before yesterday is dead
        let stale = days(&["2026-09-29", "2026-09-30"]);
        assert_eq!(streaks(&stale, &[], today), (0, 2));
    }

    #[test]
    fn streaks_merges_both_sources_and_dedupes() {
        let today = day("2026-10-02");
        // neither source alone is consecutive; together they form a 4-day run
        let progress = days(&["2026-09-29", "2026-10-01"]);
        let events = days(&["2026-09-30", "2026-10-01", "2026-10-02"]);
        assert_eq!(streaks(&progress, &events, today), (4, 4));

        // the longest run can be an older one
        let progress = days(&[
            "2026-09-01",
            "2026-09-02",
            "2026-09-03",
            "2026-09-04",
            "2026-09-05",
            "2026-10-02",
        ]);
        assert_eq!(streaks(&progress, &[], today), (1, 5));
    }

    fn event(book_id: &str, page: i32, created_date: &str) -> ReadingEvent {
        ReadingEvent {
            id: 0,
            user_id: "u1".into(),
            book_id: book_id.into(),
            series_id: "s1".into(),
            page,
            created_date: parse_datetime_utc(created_date).unwrap(),
        }
    }

    #[test]
    fn pages_by_day_deltas_and_restarts() {
        let events = vec![
            // b1: 0 -> 25 -> 40 on day 1, restart to 5 (counts 0), then 12 on day 2
            event("b1", 25, "2026-10-01 10:00:00"),
            event("b1", 40, "2026-10-01 11:00:00"),
            event("b1", 5, "2026-10-01 12:00:00"),
            event("b1", 12, "2026-10-02 09:00:00"),
            // b2: first event counts from 0
            event("b2", 8, "2026-10-01 08:00:00"),
        ];
        let pages = pages_by_day(&events);
        assert_eq!(
            pages.into_iter().collect::<Vec<_>>(),
            [(day("2026-10-01"), 25 + 15 + 8), (day("2026-10-02"), 7)]
        );
    }

    #[test]
    fn pages_by_day_equal_pages_count_nothing() {
        let events = vec![
            event("b1", 10, "2026-10-01 10:00:00"),
            event("b1", 10, "2026-10-01 11:00:00"),
            event("b1", 15, "2026-10-01 12:00:00"),
        ];
        let pages = pages_by_day(&events);
        // only the initial 0 -> 10 and 10 -> 15 deltas count
        assert_eq!(
            pages.into_iter().collect::<Vec<_>>(),
            [(day("2026-10-01"), 15)]
        );
    }

    fn progress(book_id: &str, page: i32) -> ReadProgress {
        ReadProgress {
            book_id: book_id.into(),
            user_id: "u1".into(),
            page,
            completed: false,
            read_date: parse_datetime_utc("2026-10-01 10:00:00").unwrap(),
            device_id: String::new(),
            device_name: String::new(),
            locator: None,
            created_date: parse_datetime_utc("2026-10-01 10:00:00").unwrap(),
            last_modified_date: parse_datetime_utc("2026-10-01 10:00:00").unwrap(),
        }
    }

    #[tokio::test]
    async fn consumer_records_every_progress_event_of_an_overflow_burst() {
        let state = shared::test_state();
        let events = state.events.tap();
        shared::seed_base(&state.db);
        let handle = consume_events(state.clone(), events);
        // the three real events sit at the front of a burst far beyond the
        // broadcast capacity: a broadcast receiver would skip ahead past them,
        // the tap queues every one of them
        for page in [10, 20, 30] {
            let _ = state
                .events
                .send(DomainEvent::ReadProgressChanged(progress("b1", page)));
        }
        for i in 0..3000 {
            let _ = state.events.send(DomainEvent::ReadProgressSeriesChanged {
                series_id: format!("s{i}"),
                user_id: "u1".into(),
            });
        }
        let recorded = || {
            ReadingEventDao::new(state.kmrs_db.clone())
                .find_all_by_user("u1")
                .unwrap()
                .len()
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let found = loop {
            if recorded() == 3 {
                break true;
            }
            if std::time::Instant::now() > deadline {
                break false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        handle.abort();
        assert!(found, "every progress event of the burst must be recorded");
    }
}
