//! Domain services (komga's `domain/service` equivalents): lifecycle logic that spans DAOs,
//! the filesystem, the task queue, and the event bus.

pub mod book;
pub mod collection;
pub mod convert;
pub mod import;
pub mod kepub;
pub mod kobo_proxy;
pub mod komf;
pub mod library;
pub mod library_content;
pub mod maintenance;
pub mod metadata;
pub mod metrics;
pub mod page_hash;
pub mod processor;
pub mod reading_stats;
pub mod readlist;
pub mod scheduler;
pub mod series;
pub mod smart_list;
pub mod strm;
pub mod strm_metadata;
pub mod sync_point;
pub mod tasks;
pub mod tracker_sync;
pub mod transient_book;
pub mod update_check;
pub mod user;
pub use tasks::{TaskEmitter, TaskNotify};

pub(crate) fn io_error(
    action: &str,
    path: &std::path::Path,
    error: std::io::Error,
) -> komga_db::Error {
    let kind = error.kind();
    komga_db::Error::Io(std::io::Error::new(
        kind,
        format!("{action} {}: {error}", path.display()),
    ))
}

pub(crate) fn remove_file_if_exists(
    action: &str,
    path: &std::path::Path,
) -> komga_db::Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_error(action, path, e)),
    }
}

pub(crate) fn remove_dir_if_exists(action: &str, path: &std::path::Path) -> komga_db::Result<bool> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_error(action, path, e)),
    }
}

pub(crate) fn read_dir_if_exists(
    action: &str,
    path: &std::path::Path,
) -> komga_db::Result<Option<std::fs::ReadDir>> {
    match std::fs::read_dir(path) {
        Ok(entries) => Ok(Some(entries)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(action, path, e)),
    }
}
