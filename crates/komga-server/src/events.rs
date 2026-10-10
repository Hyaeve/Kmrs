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
/// SSE/webhook/stats consumers tolerate; the search indexer cannot, so every publish
/// is additionally queued on a dedicated lossless channel handed to it at startup.
/// That channel is unbounded because publishers are synchronous code that cannot
/// await backpressure.
#[derive(Clone)]
pub struct EventBus {
    fanout: tokio::sync::broadcast::Sender<DomainEvent>,
    index: tokio::sync::mpsc::UnboundedSender<DomainEvent>,
}

impl EventBus {
    // the Err variant is boxed: it carries the event back, which exceeds
    // clippy's result_large_err threshold
    pub fn send(
        &self,
        event: DomainEvent,
    ) -> Result<usize, Box<tokio::sync::broadcast::error::SendError<DomainEvent>>> {
        // indexing is the one consumer that must not miss anything, so it is fed
        // even when the broadcast side has no receivers (or errors)
        let _ = self.index.send(event.clone());
        self.fanout.send(event).map_err(Box::new)
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DomainEvent> {
        self.fanout.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.fanout.receiver_count()
    }
}

const EVENT_BUS_CAPACITY: usize = 1024;

pub fn event_bus() -> (EventBus, tokio::sync::mpsc::UnboundedReceiver<DomainEvent>) {
    let (index, index_rx) = tokio::sync::mpsc::unbounded_channel();
    (
        EventBus {
            fanout: tokio::sync::broadcast::channel(EVENT_BUS_CAPACITY).0,
            index,
        },
        index_rx,
    )
}
