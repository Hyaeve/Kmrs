//! Domain services (komga's `domain/service` equivalents): lifecycle logic that spans DAOs,
//! the filesystem, the task queue, and the event bus.

pub mod book;
// wired up by the M4 task processor
#[allow(dead_code)]
pub mod library_content;
// wired up by the M4 task processor
#[allow(dead_code)]
pub mod series;
pub mod tasks;

// TaskNotify is consumed by the M4 task processor
#[allow(unused_imports)]
pub use tasks::{TaskEmitter, TaskNotify};
