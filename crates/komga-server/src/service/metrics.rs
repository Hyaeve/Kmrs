//! Process-global metrics collectors. Task execution metrics back the `komga.tasks.execution`
//! Timer and `komga.tasks.failure` Counter of `MetricsPublisherController`: per task type
//! (`Task::simple_type`, the Java class simple name) execution count/time/max and failure
//! count. The process stats (start time, CPU, RSS) and row counts feed both the actuator
//! metrics endpoints and the kmrs-private stats endpoints.

use crate::state::AppState;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Default, Clone, Copy)]
pub struct TaskTypeMetrics {
    pub executions: u64,
    pub total: Duration,
    pub max: Duration,
    pub failures: u64,
}

static TASK_METRICS: Mutex<BTreeMap<&'static str, TaskTypeMetrics>> = Mutex::new(BTreeMap::new());

/// The timer records only successful executions; failures bump the counter (TaskHandler.kt).
pub fn record_task_execution(task_type: &'static str, elapsed: Duration, success: bool) {
    let mut metrics = TASK_METRICS.lock().unwrap();
    let entry = metrics.entry(task_type).or_default();
    if success {
        entry.executions += 1;
        entry.total += elapsed;
        entry.max = entry.max.max(elapsed);
    } else {
        entry.failures += 1;
    }
}

pub fn task_metrics() -> BTreeMap<&'static str, TaskTypeMetrics> {
    TASK_METRICS.lock().unwrap().clone()
}

pub(crate) fn process_start() -> &'static (std::time::Instant, f64) {
    static START: OnceLock<(std::time::Instant, f64)> = OnceLock::new();
    START.get_or_init(|| {
        let epoch_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        (std::time::Instant::now(), epoch_millis)
    })
}

pub(crate) fn count_of(state: &AppState, table: &str) -> i64 {
    let Ok(conn) = state.db.ro() else {
        tracing::warn!("read pool unavailable for metrics");
        return 0;
    };
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
        r.get::<_, i64>(0)
    })
    .unwrap_or(0)
}

/// One long-lived `System` for self-process stats: CPU usage is a diff between two
/// refreshes, so recreating the `System` would reset the baseline. Memory and CPU are
/// refreshed with disjoint `ProcessRefreshKind`s, so one metric never disturbs the
/// other's baseline.
fn process_sys() -> &'static Mutex<sysinfo::System> {
    static SYS: OnceLock<Mutex<sysinfo::System>> = OnceLock::new();
    SYS.get_or_init(|| Mutex::new(sysinfo::System::new()))
}

fn self_pid() -> sysinfo::Pid {
    sysinfo::Pid::from(std::process::id() as usize)
}

/// Recent CPU usage of this process in percent of total capacity (100 = every core busy,
/// the Java side's OperatingSystemMXBean semantics; sysinfo reports 100 per busy core).
/// Below MINIMUM_CPU_UPDATE_INTERVAL the last value is reused: a back-to-back poll would
/// otherwise shrink the diff window toward zero and read garbage.
pub(crate) fn cpu_usage_percent() -> f64 {
    static LAST: Mutex<Option<(std::time::Instant, f64)>> = Mutex::new(None);

    let mut last = LAST.lock().unwrap();
    if let Some((at, value)) = *last {
        if at.elapsed() < sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
            return value;
        }
    }
    let pid = self_pid();
    let mut sys = process_sys().lock().unwrap();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        false,
        sysinfo::ProcessRefreshKind::nothing().with_cpu(),
    );
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    let value = sys
        .process(pid)
        .map(|p| p.cpu_usage() as f64 / cores)
        .unwrap_or(0.0);
    *last = Some((std::time::Instant::now(), value));
    value
}

pub(crate) fn rss_bytes() -> i64 {
    let pid = self_pid();
    let mut sys = process_sys().lock().unwrap();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        false,
        sysinfo::ProcessRefreshKind::nothing().with_memory(),
    );
    sys.process(pid).map(|p| p.memory() as i64).unwrap_or(0)
}
