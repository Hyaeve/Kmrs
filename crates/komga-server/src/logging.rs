use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{InitError, RollingFileAppender};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init(logs_dir: &Path) -> Option<WorkerGuard> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "warn,kmrs=info,komga=info".into());
    let stdout = tracing_subscriber::fmt::layer();

    let appender = std::fs::create_dir_all(logs_dir)
        .map_err(|e| e.to_string())
        .and_then(|()| rolling_appender(logs_dir).map_err(|e| e.to_string()));

    match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let file = tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false);
            tracing_subscriber::registry()
                .with(filter)
                .with(stdout)
                .with(file)
                .init();
            Some(guard)
        }
        Err(e) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(stdout)
                .init();
            tracing::warn!("file logging disabled: {e}");
            None
        }
    }
}

// The Java version rotates komga.log by size; tracing-appender only rotates by
// time, so daily rotation with 7 kept files approximates its 7-day history.
fn rolling_appender(logs_dir: &Path) -> Result<RollingFileAppender, InitError> {
    tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("kmrs")
        .filename_suffix("log")
        .max_log_files(7)
        .build(logs_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rolling_appender_writes_dated_kmrs_log() {
        let dir = tempfile::tempdir().unwrap();
        let appender = rolling_appender(dir.path()).unwrap();
        let (mut writer, guard) = tracing_appender::non_blocking(appender);
        writeln!(writer, "hello kmrs").unwrap();
        drop(writer);
        drop(guard);

        let files: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files.len(), 1, "expected a single log file, got {files:?}");
        assert!(
            files[0].starts_with("kmrs.") && files[0].ends_with(".log"),
            "unexpected log file name: {}",
            files[0]
        );
        let content = std::fs::read_to_string(dir.path().join(&files[0])).unwrap();
        assert!(content.contains("hello kmrs"));
    }
}
