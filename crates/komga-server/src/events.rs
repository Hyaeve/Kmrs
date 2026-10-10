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

    SmartListAdded(SmartList),
    SmartListUpdated(SmartList),
    SmartListDeleted(SmartList),
    SmartListThumbnailChanged {
        smart_list_id: String,
        user_id: String,
    },
}

/// Process-wide event bus. Broadcast receivers skip ahead when they lag, which the
/// SSE/webhook consumers and the one-shot event waiters tolerate. Consumers that must
/// not miss events (the search indexer, reading stats, tracker sync) instead register
/// a `tap`: an unbounded queue fed on every publish — unbounded because publishers
/// are synchronous code that cannot await backpressure.
#[derive(Clone)]
pub struct EventBus {
    fanout: tokio::sync::broadcast::Sender<DomainEvent>,
    taps: std::sync::Arc<std::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<DomainEvent>>>>,
}

impl EventBus {
    // the Err variant is boxed: it carries the event back, which exceeds
    // clippy's result_large_err threshold
    pub fn send(
        &self,
        event: DomainEvent,
    ) -> Result<usize, Box<tokio::sync::broadcast::error::SendError<DomainEvent>>> {
        // taps are fed even when the broadcast side has no receivers (or errors):
        // their consumers must not miss anything
        for tap in self.taps.lock().unwrap().iter() {
            let _ = tap.send(event.clone());
        }
        self.fanout.send(event).map_err(Box::new)
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DomainEvent> {
        self.fanout.subscribe()
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

pub fn event_bus() -> EventBus {
    EventBus {
        fanout: tokio::sync::broadcast::channel(EVENT_BUS_CAPACITY).0,
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
