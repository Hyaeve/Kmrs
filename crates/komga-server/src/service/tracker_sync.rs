//! Tracker sync, a kmrs-private enhancement with no Java equivalent: pushes
//! reading progress to komf's per-user tracker accounts (AniList / MAL /
//! Bangumi / MangaBaka) whenever a book is read. Every decision rule lives in
//! `komga_core::tracker` (pure, unit-tested); this module only wires events,
//! database reads, and komf calls together.
//!
//! Progress only ever moves forward: unread/delete events never regress the
//! platform entry. Bindings belong to one user and only that user's reading
//! advances them.

use crate::events::DomainEvent;
use crate::service::komf::{self, KomfClient};
use crate::state::AppState;
use komga_core::tracker::{self, BookTrackInput, RemoteState, TrackStatus, TrackUpdate};
use komga_db::dao::book::{BookDao, BookMetadataDao};
use komga_db::dao::komf_integration::{KomfIntegrationDao, KomfIntegrationState};
use komga_db::dao::read_progress::ReadProgressDao;
use komga_db::dao::tracker_link::{TrackerLink, TrackerLinkDao};

/// komf's `TrackState` response shape (camelCase); komf owns the schema.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct KomfTrackStateDto {
    status: Option<String>,
    last_read_chapter: Option<f32>,
    last_read_volume: Option<i32>,
    total_chapters: Option<i32>,
    total_volumes: Option<i32>,
    start_read_date: Option<String>,
    finish_read_date: Option<String>,
}

impl From<KomfTrackStateDto> for RemoteState {
    fn from(dto: KomfTrackStateDto) -> Self {
        RemoteState {
            status: dto.status.as_deref().and_then(TrackStatus::parse),
            last_read_chapter: dto.last_read_chapter,
            last_read_volume: dto.last_read_volume,
            total_chapters: dto.total_chapters,
            total_volumes: dto.total_volumes,
            start_read_date: dto.start_read_date,
            finish_read_date: dto.finish_read_date,
        }
    }
}

/// Wire shape of `POST /api/tracker/{provider}/update`; all-None fields are
/// omitted so komf keeps the platform value ("keep" semantics).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct KomfTrackUpdateBody<'a> {
    track_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_read_chapter: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_read_volume: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_read_date: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_read_date: Option<&'a str>,
}

impl<'a> From<&'a TrackUpdate> for KomfTrackUpdateBody<'a> {
    fn from(update: &'a TrackUpdate) -> Self {
        KomfTrackUpdateBody {
            track_id: "", // filled by the caller, which knows the binding
            status: update.status.map(|s| s.as_str()),
            last_read_chapter: update.last_read_chapter,
            last_read_volume: update.last_read_volume,
            start_read_date: update.start_read_date.as_deref(),
            finish_read_date: update.finish_read_date.as_deref(),
        }
    }
}

/// Serializes concurrent syncs of the same (series, user) pair. Pushing is a
/// GET → decide → POST sequence against komf, and overlapping sequences
/// reorder: two chapters completing in quick succession would both read the
/// same remote progress and the earlier chapter could POST last, regressing
/// the remote value. Holding one lock per key for the whole sync keeps every
/// sequence atomic; distinct pairs still run concurrently.
type SyncKey = (String, String);

fn sync_locks() -> &'static std::sync::Mutex<
    std::collections::HashMap<SyncKey, std::sync::Arc<tokio::sync::Mutex<()>>>,
> {
    static LOCKS: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<SyncKey, std::sync::Arc<tokio::sync::Mutex<()>>>,
        >,
    > = std::sync::OnceLock::new();
    LOCKS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

async fn run_serialized<R>(key: &SyncKey, fut: impl std::future::Future<Output = R>) -> R {
    let slot = {
        let mut locks = sync_locks().lock().unwrap_or_else(|e| e.into_inner());
        locks.entry(key.clone()).or_default().clone()
    };
    let _guard = slot.lock().await;
    let result = fut.await;
    drop(_guard);
    // drop the slot when nobody else holds it so the registry can't grow with
    // one entry per synced series
    let mut locks = sync_locks().lock().unwrap_or_else(|e| e.into_inner());
    if std::sync::Arc::strong_count(&slot) == 2 {
        locks.remove(key);
    }
    result
}

/// Subscribes to the domain event bus and syncs series with tracker bindings
/// when their read progress changes (same consumer shape as
/// `reading_stats::consume_events`). `ReadProgressDeleted`/series-deleted
/// events intentionally do nothing: platform progress never regresses.
pub fn consume_events(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut receiver = state.events.subscribe();
        loop {
            match receiver.recv().await {
                Ok(DomainEvent::ReadProgressChanged(progress)) if progress.completed => {
                    let state = state.clone();
                    tokio::spawn(async move {
                        let Some(series_id) = book_series_id(&state, &progress.book_id).await
                        else {
                            return;
                        };
                        run_serialized(
                            &(series_id.clone(), progress.user_id.clone()),
                            sync_series(&state, series_id, progress.user_id),
                        )
                        .await;
                    });
                }
                Ok(DomainEvent::ReadProgressSeriesChanged { series_id, user_id }) => {
                    let state = state.clone();
                    tokio::spawn(async move {
                        run_serialized(
                            &(series_id.clone(), user_id.clone()),
                            sync_series(&state, series_id, user_id),
                        )
                        .await;
                    });
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("tracker sync event consumer lagged by {n} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

async fn book_series_id(state: &AppState, book_id: &str) -> Option<String> {
    let book_id = book_id.to_string();
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        BookDao::new(state.db.clone())
            .get_series_id_or_null(&book_id)
            .ok()
            .flatten()
    })
    .await
    .ok()
    .flatten()
}

/// Everything the decision needs, gathered in one blocking pass over the main
/// database. Book inputs own their names so the snapshot can outlive the DAO
/// rows; `BookTrackInput` views are built per decision call.
struct SeriesSnapshot {
    links: Vec<TrackerLink>,
    books: Vec<OwnedBookInput>,
}

struct OwnedBookInput {
    name: String,
    number_sort: f32,
    read: bool,
}

fn gather(
    state: &AppState,
    series_id: &str,
    user_id: &str,
) -> komga_db::Result<Option<SeriesSnapshot>> {
    let links =
        TrackerLinkDao::new(state.kmrs_db.clone()).list_by_series_and_user(series_id, user_id)?;
    if links.is_empty() {
        return Ok(None);
    }
    let book_dao = BookDao::new(state.db.clone());
    let books = book_dao.find_by_series_id(series_id)?;
    if books.is_empty() {
        return Ok(None);
    }
    let book_ids: Vec<String> = books.iter().map(|b| b.id.clone()).collect();
    let number_sorts: std::collections::HashMap<String, f32> =
        BookMetadataDao::new(state.db.clone())
            .find_number_sort_by_book_ids(&book_ids)?
            .into_iter()
            .collect();
    let read: std::collections::HashSet<String> = ReadProgressDao::new(state.db.clone())
        .find_by_books_and_user(&book_ids, user_id)?
        .into_iter()
        .filter(|p| p.completed)
        .map(|p| p.book_id)
        .collect();
    let books = books
        .into_iter()
        .map(|b| OwnedBookInput {
            name: b.name,
            number_sort: number_sorts.get(&b.id).copied().unwrap_or(0.0),
            read: read.contains(&b.id),
        })
        .collect();
    Ok(Some(SeriesSnapshot { links, books }))
}

/// Syncs every tracker binding of one (series, user) pair. Failures are logged
/// and never propagated: reading must never fail because a tracker is down.
async fn sync_series(state: &AppState, series_id: String, user_id: String) {
    let snapshot = match tokio::task::spawn_blocking({
        let state = state.clone();
        let series_id = series_id.clone();
        let user_id = user_id.clone();
        move || gather(&state, &series_id, &user_id)
    })
    .await
    {
        Ok(Ok(Some(snapshot))) => snapshot,
        Ok(Ok(None)) => return,
        Ok(Err(e)) => {
            tracing::warn!("tracker sync: failed to gather series {series_id}: {e}");
            return;
        }
        Err(e) => {
            tracing::warn!("tracker sync: gather task failed: {e}");
            return;
        }
    };

    let integration = match KomfIntegrationDao::new(state.kmrs_db.clone()).get() {
        Ok(Some(row)) => row,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("tracker sync: failed to read komf integration: {e}");
            return;
        }
    };
    if integration.state != KomfIntegrationState::Connected {
        return; // nothing useful to report until an admin connects komf
    }
    let client = komf::client(state, &integration);

    let today = komga_core::time_codec::now_utc().date();
    let books = snapshot
        .books
        .iter()
        .map(|b| BookTrackInput {
            name: &b.name,
            number_sort: b.number_sort,
            read: b.read,
        })
        .collect::<Vec<_>>();
    for link in &snapshot.links {
        if let Err(e) = push_link(&client, link, &books, today).await {
            tracing::warn!(
                "tracker sync: {}:{} for user {} failed: {e:#}",
                link.provider,
                link.track_id,
                link.user_id
            );
        }
    }
}

/// Fetches the platform state, decides, and pushes when there is something to
/// push. `Err` covers transport and komf failure statuses; "nothing to push"
/// is `Ok(())`.
async fn push_link(
    client: &KomfClient,
    link: &TrackerLink,
    books: &[BookTrackInput<'_>],
    today: time::Date,
) -> anyhow::Result<()> {
    let state_response = client
        .proxy_tracker(
            reqwest::Method::GET,
            &format!("/api/tracker/{}/state", link.provider),
            Some(&format!("trackId={}", urlencoding_encode(&link.track_id))),
            None,
            &link.user_id,
        )
        .await?;
    if state_response.status() == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!("komf reports the tracker login is lost; re-login required");
    }
    if !state_response.status().is_success() {
        anyhow::bail!("komf state query failed: HTTP {}", state_response.status());
    }
    let remote: RemoteState = state_response
        .json::<KomfTrackStateDto>()
        .await
        .map(RemoteState::from)?;
    let mode = tracker::effective_mode(link.track_mode, books);
    let Some(update) = tracker::decide(mode, link.chapter_offset, &remote, books, today) else {
        return Ok(());
    };
    let mut body = KomfTrackUpdateBody::from(&update);
    body.track_id = &link.track_id;
    let response = client
        .proxy_tracker(
            reqwest::Method::POST,
            &format!("/api/tracker/{}/update", link.provider),
            None,
            Some(&serde_json::to_value(&body)?),
            &link.user_id,
        )
        .await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!("komf reports the tracker login is lost; re-login required");
    }
    if !response.status().is_success() {
        anyhow::bail!("komf update failed: HTTP {}", response.status());
    }
    Ok(())
}

/// Query-encodes one parameter value without pulling in a crate for a single
/// use; track ids are platform entry ids (digits, or provider-safe slugs).
fn urlencoding_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    /// Two pushes racing on the same (series, user) must stay ordered: each
    /// task's GET happens strictly before its POST, otherwise the earlier
    /// chapter could POST last and regress the remote value. The sleep stands
    /// in for the komf round trip and forces the interleaving.
    #[tokio::test]
    async fn same_key_sequences_never_interleave() {
        let events: Arc<StdMutex<Vec<(&'static str, &'static str)>>> =
            Arc::new(StdMutex::new(Vec::new()));
        let key = ("series-1".to_string(), "user-1".to_string());
        let run = |tag: &'static str| {
            let events = events.clone();
            let key = key.clone();
            async move {
                run_serialized(&key, async move {
                    events.lock().unwrap().push(("get", tag));
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    events.lock().unwrap().push(("post", tag));
                })
                .await
            }
        };
        tokio::join!(run("one"), run("two"));
        let order: Vec<_> = events.lock().unwrap().iter().map(|(op, _)| *op).collect();
        assert_eq!(order, vec!["get", "post", "get", "post"]);
    }

    /// Distinct (series, user) pairs must not block each other. The barrier
    /// forces both GETs before either POST; a global lock would deadlock
    /// them into get,post,get,post instead.
    #[tokio::test]
    async fn distinct_keys_run_concurrently() {
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let events: Arc<StdMutex<Vec<&'static str>>> = Arc::new(StdMutex::new(Vec::new()));
        let run = |key: SyncKey, barrier: Arc<tokio::sync::Barrier>| {
            let events = events.clone();
            async move {
                run_serialized(&key, async move {
                    events.lock().unwrap().push("get");
                    barrier.wait().await;
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    events.lock().unwrap().push("post");
                })
                .await
            }
        };
        let key_a = ("series-1".to_string(), "user-1".to_string());
        let key_b = ("series-2".to_string(), "user-1".to_string());
        tokio::join!(run(key_a, barrier.clone()), run(key_b, barrier));
        assert_eq!(*events.lock().unwrap(), vec!["get", "get", "post", "post"]);
    }

    /// Finished slots are evicted so the registry can't grow with one entry
    /// per synced series.
    #[tokio::test]
    async fn finished_slots_are_evicted() {
        let key = ("evict-series".to_string(), "evict-user".to_string());
        run_serialized(&key, async {}).await;
        let locks = sync_locks().lock().unwrap_or_else(|e| e.into_inner());
        assert!(!locks.contains_key(&key));
    }

    #[test]
    fn state_dto_maps_komf_shape() {
        let dto: KomfTrackStateDto = serde_json::from_str(
            r#"{
                "score": 8,
                "status": "reading",
                "lastReadChapter": 12.0,
                "lastReadVolume": 1,
                "totalChapters": 20,
                "totalVolumes": null,
                "startReadDate": "2026-10-01",
                "finishReadDate": null
            }"#,
        )
        .unwrap();
        let remote = RemoteState::from(dto);
        assert_eq!(remote.status, Some(TrackStatus::Reading));
        assert_eq!(remote.last_read_chapter, Some(12.0));
        assert_eq!(remote.last_read_volume, Some(1));
        assert_eq!(remote.total_chapters, Some(20));
        assert_eq!(remote.total_volumes, None);
        assert_eq!(remote.start_read_date.as_deref(), Some("2026-10-01"));
        assert_eq!(remote.finish_read_date, None);
    }

    #[test]
    fn state_dto_defaults_for_empty_platform_entry() {
        let remote = RemoteState::from(serde_json::from_str::<KomfTrackStateDto>("{}").unwrap());
        assert_eq!(remote, RemoteState::default());
    }

    #[test]
    fn update_body_omits_none_fields_and_serializes_camel_case() {
        let update = TrackUpdate {
            status: Some(TrackStatus::Reading),
            last_read_chapter: Some(12.0),
            ..TrackUpdate::default()
        };
        let mut body = KomfTrackUpdateBody::from(&update);
        body.track_id = "42";
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "trackId": "42",
                "status": "reading",
                "lastReadChapter": 12.0
            })
        );
    }

    #[test]
    fn update_body_full_shape() {
        let update = TrackUpdate {
            status: Some(TrackStatus::Completed),
            last_read_chapter: None,
            last_read_volume: Some(3),
            start_read_date: Some("2026-10-01".into()),
            finish_read_date: Some("2026-10-08".into()),
        };
        let mut body = KomfTrackUpdateBody::from(&update);
        body.track_id = "7";
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "trackId": "7",
                "status": "completed",
                "lastReadVolume": 3,
                "startReadDate": "2026-10-01",
                "finishReadDate": "2026-10-08"
            })
        );
    }

    #[test]
    fn url_encoding_encodes_only_the_value() {
        assert_eq!(urlencoding_encode("12345"), "12345");
        assert_eq!(urlencoding_encode("a b/c"), "a%20b%2Fc");
    }
}
