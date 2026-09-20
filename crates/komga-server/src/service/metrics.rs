//! Task execution metrics backing the `komga.tasks.execution` Timer and `komga.tasks.failure`
//! Counter of `MetricsPublisherController`: per task type (`Task::simple_type`, the Java class
//! simple name) execution count/time/max and failure count. Process-global, like the meter registry.

use std::collections::BTreeMap;
use std::sync::Mutex;
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
