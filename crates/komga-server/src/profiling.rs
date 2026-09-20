//! Heap profiling (cargo feature `profiling`): jemalloc as the global allocator with
//! allocation sampling, served pprof-style over HTTP and via signals. The release
//! binaries ship with this. Sampling is always on — the same model as Go's pprof, a
//! backtrace per ~512 KiB allocated — and SIGUSR1 toggles it off/on at runtime.
//!
//! - `GET /debug/pprof/heap` (ADMIN): the sampled live heap in jeprof format, as the
//!   response body. Symbolize with the published binary, which keeps its symbol table:
//!   `jeprof --svg <binary> <dump>` or `--collapsed` for flamegraph tools.
//! - `kill -USR2 <pid>`: same dump written to `$TMPDIR/kmrs.<pid>.<seq>.heap`.

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::response::{IntoResponse, Response};
use axum::{routing, Router};
use std::sync::atomic::{AtomicUsize, Ordering};

#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// jemalloc picks this weak-symbol override up as its boot config (source #1 in
/// `obtain_malloc_conf`), so profiling needs no MALLOC_CONF fiddling. Both our target
/// configs keep jemalloc's `_rjem_` prefix: Linux prefixed outright, macOS prefixed
/// with the malloc zone overridden behind it.
#[used]
#[no_mangle]
static _rjem_malloc_conf: MallocConf = MallocConf(c"prof:true,prof_active:true".as_ptr());

/// `const char *` payload: repr(transparent) keeps the symbol's layout a bare pointer.
#[repr(transparent)]
struct MallocConf(*const std::ffi::c_char);

// the pointer targets immutable static bytes, so sharing it across threads is fine
unsafe impl Sync for MallocConf {}

pub fn router() -> Router<AppState> {
    Router::new().route("/debug/pprof/heap", routing::get(get_heap_profile))
}

/// `GET /debug/pprof/heap`: dump the sampled heap to a temp file and stream it back.
async fn get_heap_profile(auth: RequireAuth) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    if !read_sampling() {
        return Err(ApiError::conflict(
            "heap sampling is off (SIGUSR1 toggles it)",
        ));
    }
    // flush per-thread cached stats so the dump sees all arena activity
    let _ = tikv_jemalloc_ctl::epoch::advance();
    let path = next_dump_path();
    dump_to(&path).map_err(|e| ApiError::Internal(e.to_string()))?;
    let body = std::fs::read(&path).map_err(|e| ApiError::Internal(e.to_string()))?;
    let _ = std::fs::remove_file(&path);
    let filename = path.file_name().unwrap().to_string_lossy().into_owned();
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/octet-stream".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}

pub fn spawn_heap_dump_listener() {
    match tikv_jemalloc_ctl::profiling::prof::read() {
        Ok(true) => {
            tracing::info!("jemalloc heap profiling active; GET /debug/pprof/heap dumps (SIGUSR1 toggles sampling, SIGUSR2 dumps to file)")
        }
        Ok(false) => tracing::warn!("jemalloc built without profiling; heap dumps will fail"),
        Err(e) => tracing::warn!("read jemalloc opt.prof: {e}"),
    }
    tokio::spawn(async {
        let mut toggles =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1()) {
                Ok(signals) => signals,
                Err(e) => {
                    tracing::warn!("install SIGUSR1 handler: {e}");
                    return;
                }
            };
        loop {
            toggles.recv().await;
            toggle_sampling();
        }
    });
    tokio::spawn(async {
        let mut dumps =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined2()) {
                Ok(signals) => signals,
                Err(e) => {
                    tracing::warn!("install SIGUSR2 handler: {e}");
                    return;
                }
            };
        loop {
            dumps.recv().await;
            dump_heap();
        }
    });
}

/// prof.active is a bool mallctl; the raw API is untyped, hence the unsafe.
fn read_sampling() -> bool {
    unsafe { tikv_jemalloc_ctl::raw::read::<bool>(c"prof.active".to_bytes_with_nul()) }
        .unwrap_or(false)
}

fn toggle_sampling() {
    let next = !read_sampling();
    // prof.active is a bool mallctl; the raw API is untyped, hence the unsafe
    let result = unsafe { tikv_jemalloc_ctl::raw::write(c"prof.active".to_bytes_with_nul(), next) };
    match result {
        Ok(()) => tracing::info!(
            "jemalloc sampling {}",
            if next { "enabled" } else { "disabled" }
        ),
        Err(e) => tracing::warn!("write jemalloc prof.active: {e}"),
    }
}

fn dump_heap() {
    if !read_sampling() {
        tracing::warn!("sampling is off; SIGUSR1 enables it, then SIGUSR2 dumps");
        return;
    }
    // flush per-thread cached stats so the dump sees all arena activity
    let _ = tikv_jemalloc_ctl::epoch::advance();
    let path = next_dump_path();
    match dump_to(&path) {
        Ok(()) => tracing::info!("heap profile written to {}", path.display()),
        Err(e) => tracing::warn!("heap profile dump failed: {e}"),
    }
}

fn next_dump_path() -> std::path::PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "kmrs.{}.{}.heap",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

fn dump_to(path: &std::path::Path) -> tikv_jemalloc_ctl::Result<()> {
    // mallctl copies the filename synchronously, but the safe API only takes &'static —
    // leak the few bytes, a manual dump is rare
    let bytes = std::ffi::CString::new(path.to_string_lossy().as_bytes())
        .expect("path has no NUL")
        .into_bytes_with_nul();
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    tikv_jemalloc_ctl::raw::write_str(c"prof.dump".to_bytes_with_nul(), bytes)
}
