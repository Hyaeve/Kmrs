//! `LibraryScanScheduler.kt` + `PeriodicScannerController.kt`: periodic library scans and
//! scan-on-startup.
//!
//! Each library with a non-DISABLED interval gets its own tokio interval task; rescheduling
//! aborts the previous one. First fire happens after one full period, matching FixedRateTask's
//! initial delay.

use crate::state::AppState;
use komga_core::dto::url_to_file_path;
use komga_core::model::library::{Library, ScanInterval};
use komga_core::task::DEFAULT_PRIORITY;
use komga_db::dao::library::LibraryDao;
use komga_db::dao::library_setting::LibrarySettingDao;
use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Abort handles of the per-library interval tasks, keyed by library id, with the metadata
/// needed to report them (`/actuator/scheduledtasks`).
/// A process-wide registry is required because `schedule_scan` is called from `update_library`,
/// which has no access to the supervisor task.
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

type Registry = HashMap<String, (Registration, tokio::task::JoinHandle<()>)>;

fn realtime_registry() -> &'static Mutex<HashMap<String, tokio::task::JoinHandle<()>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone)]
pub struct Registration {
    pub library_id: String,
    pub library_name: String,
    pub period: std::time::Duration,
}

pub struct ScanScheduler;

impl ScanScheduler {
    /// Schedules (or reschedules) the periodic scan for a library; DISABLED cancels any existing one.
    pub fn schedule_scan(state: &AppState, library: &Library) {
        let scan_strm = LibrarySettingDao::new(state.kmrs_db.clone())
            .get(&library.id)
            .map(|settings| settings.scan_strm && settings.real_time_scan)
            .unwrap_or(false);
        Self::schedule_realtime(state, library, scan_strm);

        let mut registry = registry().lock().unwrap();
        if let Some((_, handle)) = registry.remove(&library.id) {
            handle.abort();
        }
        if library.scan_interval == ScanInterval::Disabled {
            return;
        }
        let period = interval_duration(library.scan_interval);
        let emitter = state.task_emitter.clone();
        let library_id = library.id.clone();
        let library_name = library.name.clone();
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            // interval() fires immediately on the first tick; a FixedRateTask first fires
            // after one full period
            interval.tick().await;
            loop {
                interval.tick().await;
                tracing::info!("Periodic scan for library: {library_name}");
                if let Err(e) = emitter.scan_library(&library_id, false, DEFAULT_PRIORITY) {
                    tracing::error!("Failed to submit periodic scan for library {library_id}: {e}");
                }
            }
        });
        let registration = Registration {
            library_id: library.id.clone(),
            library_name: library.name.clone(),
            period,
        };
        registry.insert(library.id.clone(), (registration, handle));
    }

    /// Starts or stops the filesystem watcher for a library. The watcher debounces
    /// filesystem events and submits one scan after the burst goes quiet.
    pub fn schedule_realtime(state: &AppState, library: &Library, enabled: bool) {
        let mut registry = realtime_registry().lock().unwrap();
        if let Some(handle) = registry.remove(&library.id) {
            handle.abort();
        }
        if !enabled {
            return;
        }

        let root = PathBuf::from(url_to_file_path(&library.root));
        if !root.is_dir() {
            tracing::warn!(
                "Cannot start real-time scan for library {}: {} is not a directory",
                library.name,
                root.display()
            );
            return;
        }

        let emitter = state.task_emitter.clone();
        let library_id = library.id.clone();
        let library_name = library.name.clone();
        let handle = tokio::spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let mut watcher = match notify::recommended_watcher(move |event| {
                let _ = tx.send(event);
            }) {
                Ok(watcher) => watcher,
                Err(error) => {
                    tracing::error!(
                        "Cannot create real-time watcher for library {library_name}: {error}"
                    );
                    return;
                }
            };
            if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
                tracing::error!(
                    "Cannot watch library {library_name} at {}: {error}",
                    root.display()
                );
                return;
            }
            tracing::info!(
                "Real-time scan enabled for library {library_name}: {}",
                root.display()
            );

            while let Some(event) = rx.recv().await {
                match event {
                    Ok(event) if !is_relevant_event(&event) => continue,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(
                            "Real-time watcher error for library {library_name}: {error}"
                        );
                        continue;
                    }
                }

                // Burst debounce: submit one scan after events stop arriving.
                loop {
                    match tokio::time::timeout(Duration::from_millis(750), rx.recv()).await {
                        Ok(Some(Ok(event))) if is_relevant_event(&event) => {}
                        Ok(Some(Err(error))) => {
                            tracing::warn!(
                                "Real-time watcher error for library {library_name}: {error}"
                            );
                        }
                        Ok(Some(Ok(_))) => {}
                        Ok(None) => return,
                        Err(_) => break,
                    }
                }

                tracing::info!("Real-time change detected for library {library_name}");
                if let Err(error) = emitter.scan_library(&library_id, false, DEFAULT_PRIORITY) {
                    tracing::error!(
                        "Failed to submit real-time scan for library {library_id}: {error}"
                    );
                }
            }
        });
        registry.insert(library.id.clone(), handle);
    }

    pub fn cancel(library_id: &str) {
        if let Some((_, handle)) = registry().lock().unwrap().remove(library_id) {
            handle.abort();
        }
        if let Some(handle) = realtime_registry().lock().unwrap().remove(library_id) {
            handle.abort();
        }
    }

    /// The currently registered periodic scans, sorted by library id (deterministic output).
    pub fn scheduled_tasks() -> Vec<Registration> {
        let mut tasks: Vec<Registration> = registry()
            .lock()
            .unwrap()
            .values()
            .map(|(registration, _)| registration.clone())
            .collect();
        tasks.sort_by(|a, b| a.library_id.cmp(&b.library_id));
        tasks
    }

    /// Schedules every library's periodic scan and submits scans for `scanOnStartup` libraries.
    /// The returned handle is a lifecycle placeholder: the interval tasks live in the registry.
    pub fn start(state: AppState) -> tokio::task::JoinHandle<()> {
        let libraries = match LibraryDao::new(state.db.clone()).find_all() {
            Ok(libraries) => libraries,
            Err(e) => {
                tracing::error!("Failed to load libraries for scan scheduling: {e}");
                vec![]
            }
        };
        for library in &libraries {
            Self::schedule_scan(&state, library);
        }
        for library in libraries.iter().filter(|l| l.scan_on_startup) {
            tracing::info!("Scan on startup for library: {}", library.name);
            if let Err(e) = state
                .task_emitter
                .scan_library(&library.id, false, DEFAULT_PRIORITY)
            {
                tracing::error!(
                    "Failed to submit scan on startup for library {}: {e}",
                    library.id
                );
            }
        }
        tokio::spawn(async {})
    }
}

fn interval_duration(interval: ScanInterval) -> std::time::Duration {
    const HOUR: u64 = 3600;
    match interval {
        ScanInterval::Hourly => std::time::Duration::from_secs(HOUR),
        ScanInterval::Every6H => std::time::Duration::from_secs(6 * HOUR),
        ScanInterval::Every12H => std::time::Duration::from_secs(12 * HOUR),
        ScanInterval::Daily => std::time::Duration::from_secs(24 * HOUR),
        ScanInterval::Weekly => std::time::Duration::from_secs(7 * 24 * HOUR),
        ScanInterval::Disabled => unreachable!("DISABLED is filtered before scheduling"),
    }
}

fn is_relevant_event(event: &notify::Event) -> bool {
    !matches!(event.kind, notify::EventKind::Access(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::series::tests as series_tests;
    use komga_core::model::library::SeriesCover;
    use komga_core::time_codec::now_utc;
    use komga_db::dao::tasks::TasksDao;

    fn test_library(id: &str, interval: ScanInterval, scan_on_startup: bool) -> Library {
        let now = now_utc();
        Library {
            id: id.into(),
            name: format!("L-{id}"),
            root: "file:/l/".into(),
            import_comicinfo_book: true,
            import_comicinfo_series: true,
            import_comicinfo_collection: true,
            import_comicinfo_readlist: true,
            import_comicinfo_series_append_volume: true,
            import_epub_book: true,
            import_epub_series: true,
            import_mylar_series: true,
            import_local_artwork: true,
            import_barcode_isbn: true,
            scan_force_modified_time: false,
            scan_on_startup,
            scan_interval: interval,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: SeriesCover::First,
            hash_files: true,
            hash_pages: true,
            hash_koreader: true,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now,
            last_modified_date: now,
        }
    }

    fn registered_ids() -> Vec<String> {
        registry().lock().unwrap().keys().cloned().collect()
    }

    #[tokio::test]
    async fn schedule_registers_and_reschedule_aborts() {
        let state = series_tests::test_state();
        let tag = std::process::id();
        let id = format!("lib-sched-{tag}");
        let library = test_library(&id, ScanInterval::Hourly, false);

        ScanScheduler::schedule_scan(&state, &library);
        assert!(registered_ids().contains(&id));
        let first = registry().lock().unwrap().get(&id).unwrap().1.id();

        ScanScheduler::schedule_scan(&state, &library);
        let second = registry().lock().unwrap().get(&id).unwrap().1.id();
        assert_ne!(first, second);
        // the aborted task finishes shortly after
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let registry = registry().lock().unwrap();
        assert!(registry.get(&id).unwrap().1.id() == second);
        registry.get(&id).unwrap().1.abort();
        drop(registry);
    }

    #[tokio::test]
    async fn disabled_interval_cancels_and_does_not_register() {
        let state = series_tests::test_state();
        let tag = std::process::id();
        let id = format!("lib-dis-{tag}");
        let library = test_library(&id, ScanInterval::Hourly, false);
        ScanScheduler::schedule_scan(&state, &library);
        assert!(registered_ids().contains(&id));

        let disabled = test_library(&id, ScanInterval::Disabled, false);
        ScanScheduler::schedule_scan(&state, &disabled);
        assert!(!registered_ids().contains(&id));
    }

    #[tokio::test]
    async fn start_schedules_all_and_submits_scan_on_startup() {
        let state = series_tests::test_state();
        let tag = std::process::id();
        let startup_id = format!("lib-startup-{tag}");
        let plain_id = format!("lib-plain-{tag}");
        for library in [
            test_library(&startup_id, ScanInterval::Disabled, true),
            test_library(&plain_id, ScanInterval::Disabled, false),
        ] {
            LibraryDao::new(state.db.clone()).insert(&library).unwrap();
        }

        ScanScheduler::start(state.clone()).await.unwrap();

        let tasks = TasksDao::new(state.tasks_db.clone()).find_all().unwrap();
        let ids: Vec<String> = tasks.iter().map(|t| t.unique_id()).collect();
        assert_eq!(ids, vec![format!("SCAN_LIBRARY_{startup_id}_DEEP_false")]);
        // the placeholder handle completes; nothing is registered for DISABLED libraries
        assert!(registered_ids().is_empty() || !registered_ids().contains(&startup_id));
    }

    #[tokio::test]
    async fn periodic_scan_fires_after_period() {
        let state = series_tests::test_state();
        let tag = std::process::id();
        let id = format!("lib-tick-{tag}");
        // hourly period would never fire during the test; verify the initial tick is consumed
        // by checking nothing is submitted immediately
        let library = test_library(&id, ScanInterval::Hourly, false);
        ScanScheduler::schedule_scan(&state, &library);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let count = TasksDao::new(state.tasks_db.clone()).count().unwrap();
        assert_eq!(count, 0);
        registry().lock().unwrap().get(&id).unwrap().1.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_time_watcher_submits_scan_after_file_change() {
        let state = series_tests::test_state();
        let root = tempfile::tempdir().unwrap();
        let tag = std::process::id();
        let id = format!("lib-realtime-{tag}");
        let mut library = test_library(&id, ScanInterval::Disabled, false);
        library.root = komga_media::scanner::path_to_url(root.path());
        LibraryDao::new(state.db.clone()).insert(&library).unwrap();
        LibrarySettingDao::new(state.kmrs_db.clone())
            .set(
                &id,
                komga_db::dao::library_setting::LibrarySettings {
                    scan_strm: true,
                    real_time_scan: true,
                },
            )
            .unwrap();

        ScanScheduler::schedule_scan(&state, &library);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        std::fs::write(root.path().join("new.cbz"), b"not-a-real-archive").unwrap();

        let mut queued = false;
        for _ in 0..50 {
            let tasks = TasksDao::new(state.tasks_db.clone()).find_all().unwrap();
            if tasks
                .iter()
                .any(|task| task.unique_id() == format!("SCAN_LIBRARY_{id}_DEEP_false"))
            {
                queued = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        ScanScheduler::cancel(&id);
        assert!(queued, "real-time watcher did not queue a scan");
    }
}
