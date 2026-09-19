//! Domain events, ported from `domain/model/DomainEvent.kt`, plus the broadcast bus used to
//! fan them out (Spring's `ApplicationEventPublisher` equivalent).

use komga_core::model::book::Book;
use komga_core::model::collection::SeriesCollection;
use komga_core::model::library::Library;
use komga_core::model::read_progress::ReadProgress;
use komga_core::model::readlist::ReadList;
use komga_core::model::series::Series;
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
}

/// Process-wide event bus. Lagging receivers skip ahead (broadcast semantics).
pub type EventBus = tokio::sync::broadcast::Sender<DomainEvent>;

pub fn event_bus() -> EventBus {
    tokio::sync::broadcast::channel(1024).0
}
