//! Domain services (komga's `domain/service` equivalents): lifecycle logic that spans DAOs,
//! the filesystem, the task queue, and the event bus.

pub mod book;
pub mod library;
pub mod library_content;
pub mod processor;
pub mod scheduler;
pub mod series;
pub mod tasks;

pub use tasks::{TaskEmitter, TaskNotify};
