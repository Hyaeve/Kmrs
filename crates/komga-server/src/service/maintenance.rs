//! Periodic maintenance jobs unrelated to library scanning: daily history and
//! authentication-activity cleanups, and the thumbnail orphan sweep.

use crate::state::AppState;

pub struct MaintenanceScheduler;

impl MaintenanceScheduler {
    /// History retention: every day, delete events older than `history-retention-days`
    /// (0 = keep forever). Runs on the task write pool like the auth-activity cleanup.
    pub fn start_history_cleanup(state: AppState) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                interval.tick().await;
                let retention_days = state.config.history_retention_days;
                if retention_days == 0 {
                    continue;
                }
                let cutoff =
                    komga_core::time_codec::now_utc() - time::Duration::days(retention_days.into());
                match komga_db::dao::history::HistoricalEventDao::new(state.task_db.clone())
                    .delete_older_than(cutoff)
                {
                    Ok(n) if n > 0 => tracing::info!(
                        "Removed {n} historical events older than {retention_days} days"
                    ),
                    Err(e) => tracing::error!("Failed to cleanup historical events: {e}"),
                    _ => {}
                }
            }
        })
    }

    /// `AuthenticationActivityCleanupController`: every day, delete activity older than 1 month.
    pub fn start_auth_activity_cleanup(state: AppState) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                interval.tick().await;
                let cutoff = komga_core::time_codec::now_utc() - time::Duration::days(30);
                tracing::info!(
                    "Remove authentication activity older than {}",
                    komga_core::time_codec::format_datetime(cutoff)
                );
                // authentication-activity cleanup runs on the task write pool
                match komga_db::dao::user::UserDao::new(state.task_db.clone())
                    .delete_activity_older_than(cutoff)
                {
                    Ok(n) if n > 0 => tracing::info!("Removed {n} old authentication activities"),
                    Err(e) => tracing::error!("Failed to cleanup authentication activity: {e}"),
                    _ => {}
                }
            }
        })
    }

    /// Thumbnail file storage: sweeps orphaned files under `<config-dir>/thumbnails` at
    /// startup and every day. Runs in both storage modes; a missing directory is a no-op.
    pub fn start_thumbnail_sweep(state: AppState) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                interval.tick().await;
                let state = state.clone();
                match tokio::task::spawn_blocking(move || {
                    crate::thumbnails::sweep_orphan_files(&state)
                })
                .await
                {
                    Ok(Ok(n)) if n > 0 => tracing::info!("Removed {n} orphaned thumbnail files"),
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => tracing::error!("Failed to sweep orphaned thumbnail files: {e}"),
                    Err(e) => tracing::error!("Thumbnail sweep task failed: {e}"),
                }
            }
        })
    }
}
