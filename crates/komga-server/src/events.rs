//! Domain events, ported from `domain/model/DomainEvent.kt`, plus the broadcast bus used to
//! fan them out (Spring's `ApplicationEventPublisher` equivalent).

use komga_core::model::book::Book;
use komga_core::model::collection::SeriesCollection;
use komga_core::model::library::Library;
use komga_core::model::read_progress::ReadProgress;
use komga_core::model::readlist::ReadList;
use komga_core::model::series::Series;
use komga_core::model::smart_list::SmartList;
use komga_core::model::thumbnail::{
    ThumbnailBook, ThumbnailReadList, ThumbnailSeries, ThumbnailSeriesCollection,
};
use komga_core::model::user::KomgaUser;

#[derive(Debug, Clone)]
pub enum DomainEvent {
    LibraryAdded(Library),
    LibraryUpdated(Library),
    LibraryDeleted(Library),
    LibraryScanned(Library),

    SeriesAdded(Series),
    SeriesUpdated(Series),
    SeriesDeleted(Series),

    BookAdded(Book),
    BookUpdated(Book),
    BookDeleted(Book),
    BookImported {
        book: Option<Book>,
        source_file: String,
        success: bool,
        message: Option<String>,
    },

    ReadListAdded(ReadList),
    ReadListUpdated(ReadList),
    ReadListDeleted(ReadList),

    CollectionAdded(SeriesCollection),
    CollectionUpdated(SeriesCollection),
    CollectionDeleted(SeriesCollection),

    ReadProgressChanged(ReadProgress),
    ReadProgressDeleted(ReadProgress),
    ReadProgressSeriesChanged {
        series_id: String,
        user_id: String,
    },
    ReadProgressSeriesDeleted {
        series_id: String,
        user_id: String,
    },

    ThumbnailBookAdded(ThumbnailBook),
    ThumbnailBookDeleted(ThumbnailBook),
    ThumbnailSeriesAdded(ThumbnailSeries),
    ThumbnailSeriesDeleted(ThumbnailSeries),
    ThumbnailSeriesCollectionAdded(ThumbnailSeriesCollection),
    ThumbnailSeriesCollectionDeleted(ThumbnailSeriesCollection),
    ThumbnailReadListAdded(ThumbnailReadList),
    ThumbnailReadListDeleted(ThumbnailReadList),

    UserUpdated {
        user: KomgaUser,
        expire_session: bool,
    },
    UserDeleted(KomgaUser),

    SmartListAdded(SmartListEvent),
    SmartListUpdated(SmartListEvent),
    SmartListDeleted(SmartListEvent),
    SmartListThumbnailChanged(SmartListEvent),

    FileDeletionCompleted(crate::service::tasks::TaskOutcome),
}

/// A smart list with its share scope resolved at publish time: consumers see a deletion
/// only after `SMART_LIST_SHARE` is already emptied, so the audience cannot be
/// reconstructed at consume time.
#[derive(Debug, Clone)]
pub struct SmartListEvent {
    pub smart_list: SmartList,
    pub shared_with_user_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RecordedEvent {
    pub id: String,
    sequence: u64,
    pub event: DomainEvent,
}

#[derive(Debug)]
pub enum Resume {
    Live,
    Replay(Vec<RecordedEvent>),
}

pub struct DomainReceiver(tokio::sync::broadcast::Receiver<RecordedEvent>);

impl DomainReceiver {
    pub async fn recv(&mut self) -> Result<DomainEvent, tokio::sync::broadcast::error::RecvError> {
        self.0.recv().await.map(|recorded| recorded.event)
    }

    pub fn try_recv(&mut self) -> Result<DomainEvent, tokio::sync::broadcast::error::TryRecvError> {
        self.0.try_recv().map(|recorded| recorded.event)
    }
}

/// Process-wide event bus. Broadcast receivers skip ahead when they lag, which the
/// webhook consumers and one-shot event waiters tolerate. SSE replays from the bounded
/// history when continuity can be proven and starts live otherwise. Consumers that must
/// not miss events (the search indexer, reading stats, tracker sync) register a `tap`: an
/// unbounded queue fed on every publish — unbounded because publishers are synchronous
/// code that cannot await backpressure.
#[derive(Clone)]
pub struct EventBus {
    fanout: tokio::sync::broadcast::Sender<RecordedEvent>,
    history: std::sync::Arc<std::sync::Mutex<EventHistory>>,
    taps: std::sync::Arc<std::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<DomainEvent>>>>,
}

struct EventHistory {
    boot_id: String,
    latest_sequence: u64,
    events: std::collections::VecDeque<RecordedEvent>,
}

impl EventHistory {
    fn id(&self, sequence: u64) -> String {
        format!("{}:{sequence}", self.boot_id)
    }

    fn sequence(&self, id: &str) -> Option<u64> {
        let (boot_id, sequence) = id.split_once(':')?;
        if boot_id != self.boot_id {
            return None;
        }
        sequence.parse().ok()
    }

    fn record(&mut self, event: DomainEvent) -> RecordedEvent {
        self.latest_sequence += 1;
        let sequence = self.latest_sequence;
        let recorded = RecordedEvent {
            id: self.id(sequence),
            sequence,
            event,
        };
        if self.events.len() == EVENT_HISTORY_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(recorded.clone());
        recorded
    }

    fn resume(&self, last_event_id: Option<&str>) -> Resume {
        let Some(last_event_id) = last_event_id else {
            return Resume::Live;
        };
        let Some(sequence) = self.sequence(last_event_id) else {
            return Resume::Live;
        };
        if sequence > self.latest_sequence {
            return Resume::Live;
        }
        if sequence == self.latest_sequence {
            return Resume::Replay(Vec::new());
        }
        let continuous = self
            .events
            .front()
            .is_some_and(|oldest| sequence >= oldest.sequence.saturating_sub(1));
        if !continuous {
            return Resume::Live;
        }
        Resume::Replay(
            self.events
                .iter()
                .filter(|recorded| recorded.sequence > sequence)
                .cloned()
                .collect(),
        )
    }
}

impl EventBus {
    // the Err variant is boxed: it carries the event back, which exceeds
    // clippy's result_large_err threshold
    pub fn send(
        &self,
        event: DomainEvent,
    ) -> Result<usize, Box<tokio::sync::broadcast::error::SendError<RecordedEvent>>> {
        // taps are fed even when the broadcast side has no receivers (or errors):
        // their consumers must not miss anything
        for tap in self.taps.lock().unwrap().iter() {
            let _ = tap.send(event.clone());
        }
        let result = {
            let mut history = self.history.lock().unwrap();
            let recorded = history.record(event);
            self.fanout.send(recorded)
        };
        result.map_err(Box::new)
    }

    pub fn subscribe(&self) -> DomainReceiver {
        DomainReceiver(self.fanout.subscribe())
    }

    /// The receiver and resume decision are taken under one lock so events published
    /// during connection setup cannot fall between the replay and live streams.
    pub fn subscribe_with_replay(
        &self,
        last_event_id: Option<&str>,
    ) -> (tokio::sync::broadcast::Receiver<RecordedEvent>, Resume) {
        let history = self.history.lock().unwrap();
        let receiver = self.fanout.subscribe();
        (receiver, history.resume(last_event_id))
    }

    /// Registers a lossless queue; every event published after this call is queued
    /// for the returned receiver.
    pub fn tap(&self) -> tokio::sync::mpsc::UnboundedReceiver<DomainEvent> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.taps.lock().unwrap().push(tx);
        rx
    }

    pub fn receiver_count(&self) -> usize {
        self.fanout.receiver_count()
    }
}

const EVENT_BUS_CAPACITY: usize = 1024;
const EVENT_HISTORY_CAPACITY: usize = 1024;

pub fn event_bus() -> EventBus {
    EventBus {
        fanout: tokio::sync::broadcast::channel(EVENT_BUS_CAPACITY).0,
        history: std::sync::Arc::new(std::sync::Mutex::new(EventHistory {
            boot_id: uuid::Uuid::new_v4().to_string(),
            latest_sequence: 0,
            events: std::collections::VecDeque::with_capacity(EVENT_HISTORY_CAPACITY),
        })),
        taps: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(i: usize) -> DomainEvent {
        DomainEvent::ReadProgressSeriesChanged {
            series_id: format!("s{i}"),
            user_id: "u1".into(),
        }
    }

    fn assert_series_id(event: &DomainEvent, expected: String) {
        match event {
            DomainEvent::ReadProgressSeriesChanged { series_id, .. } => {
                assert_eq!(*series_id, expected)
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn taps_receive_every_event_even_past_broadcast_capacity() {
        let bus = event_bus();
        let mut tap_a = bus.tap();
        let mut tap_b = bus.tap();
        for i in 0..EVENT_BUS_CAPACITY * 3 {
            let _ = bus.send(event(i));
        }
        // bounded so a broken delivery fails instead of hanging on recv
        let drained = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            for i in 0..EVENT_BUS_CAPACITY * 3 {
                assert_series_id(&tap_a.recv().await.unwrap(), format!("s{i}"));
                assert_series_id(&tap_b.recv().await.unwrap(), format!("s{i}"));
            }
        });
        assert!(drained.await.is_ok(), "every event must reach both taps");
    }

    fn expect_replay(resume: Resume) -> Vec<RecordedEvent> {
        match resume {
            Resume::Replay(events) => events,
            other => panic!("expected replay, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn replay_includes_events_published_without_receivers() {
        let bus = event_bus();
        let (mut initial, resume) = bus.subscribe_with_replay(None);
        assert!(matches!(resume, Resume::Live));
        bus.send(event(1)).unwrap();
        let first = initial.recv().await.unwrap();
        drop(initial);

        let _ = bus.send(event(2));
        let (mut resumed, resume) = bus.subscribe_with_replay(Some(&first.id));
        let replay = expect_replay(resume);
        assert_eq!(replay.len(), 1);
        assert!(replay[0].sequence > first.sequence);
        assert_series_id(&replay[0].event, "s2".into());

        bus.send(event(3)).unwrap();
        let live = resumed.recv().await.unwrap();
        assert!(live.sequence > replay[0].sequence);
        assert_series_id(&live.event, "s3".into());
    }

    #[tokio::test]
    async fn unusable_cursor_starts_live() {
        let bus = event_bus();
        let (mut initial, resume) = bus.subscribe_with_replay(None);
        assert!(matches!(resume, Resume::Live));
        bus.send(event(1)).unwrap();
        let first = initial.recv().await.unwrap();
        drop(initial);
        let (boot_id, _) = first.id.split_once(':').unwrap();
        let future = format!("{boot_id}:{}", u64::MAX);

        for invalid in ["not-an-event-id", future.as_str()] {
            let (_receiver, resume) = bus.subscribe_with_replay(Some(invalid));
            assert!(matches!(resume, Resume::Live));
        }
        let (_receiver, resume) = bus.subscribe_with_replay(Some(&first.id));
        assert!(expect_replay(resume).is_empty());
    }

    #[tokio::test]
    async fn foreign_boot_id_starts_live() {
        let previous = event_bus();
        let (mut initial, resume) = previous.subscribe_with_replay(None);
        assert!(matches!(resume, Resume::Live));
        previous.send(event(1)).unwrap();
        let previous_id = initial.recv().await.unwrap().id;
        drop(initial);

        let current = event_bus();
        let (_receiver, resume) = current.subscribe_with_replay(Some(&previous_id));
        assert!(matches!(resume, Resume::Live));
    }

    #[tokio::test]
    async fn expired_cursor_starts_live_only_when_history_is_discontinuous() {
        let bus = event_bus();
        let (mut initial, resume) = bus.subscribe_with_replay(None);
        assert!(matches!(resume, Resume::Live));
        bus.send(event(1)).unwrap();
        let first = initial.recv().await.unwrap();
        drop(initial);
        for i in 2..=EVENT_HISTORY_CAPACITY + 1 {
            let _ = bus.send(event(i));
        }

        let replay = {
            let (_receiver, resume) = bus.subscribe_with_replay(Some(&first.id));
            expect_replay(resume)
        };
        assert_eq!(replay.len(), EVENT_HISTORY_CAPACITY);
        assert_series_id(&replay.first().unwrap().event, "s2".into());
        assert_series_id(
            &replay.last().unwrap().event,
            format!("s{}", EVENT_HISTORY_CAPACITY + 1),
        );

        let _ = bus.send(event(EVENT_HISTORY_CAPACITY + 2));
        let (_receiver, resume) = bus.subscribe_with_replay(Some(&first.id));
        assert!(matches!(resume, Resume::Live));
    }

    #[tokio::test]
    async fn broadcast_still_skips_ahead_when_lagging() {
        let bus = event_bus();
        let mut fanout = bus.subscribe();
        for i in 0..EVENT_BUS_CAPACITY * 2 {
            let _ = bus.send(event(i));
        }
        assert!(matches!(
            fanout.recv().await,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
        ));
    }
}
