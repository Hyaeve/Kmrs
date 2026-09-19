//! Domain services (komga's `domain/service` equivalents): lifecycle logic that spans DAOs,
//! the filesystem, the task queue, and the event bus.

pub mod book;
pub mod collection;
pub mod convert;
pub mod import;
pub mod library;
pub mod library_content;
pub mod metadata;
pub mod page_hash;
pub mod processor;
pub mod readlist;
pub mod scheduler;
pub mod series;
pub mod sync_point;
pub mod tasks;
pub mod transient_book;
pub use tasks::{TaskEmitter, TaskNotify};
