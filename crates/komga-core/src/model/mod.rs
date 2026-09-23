//! Domain models, corresponding to komga's `domain/model`.

pub mod book;
pub mod book_projection;
pub mod collection;
pub mod common;
pub mod history;
pub mod library;
pub mod media;
pub mod page_hash;
pub mod read_progress;
pub mod readlist;
pub mod series;
pub mod settings;
pub mod sidecar;
pub mod sync_point;
pub mod thumbnail;
pub mod user;

pub use library::{Library, ScanInterval, SeriesCover};
