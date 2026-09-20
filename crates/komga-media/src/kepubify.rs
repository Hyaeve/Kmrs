//! Kepub conversion by shelling out to an external kepubify binary (`KepubConverter.kt`).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// `KepubConverter.isExecutable`: the path is executable, or names a command resolvable via
/// PATH that exits 0 within 3 seconds when run without arguments. Returns the path for later
/// use with [`convert`].
pub fn probe(path: &str) -> Option<PathBuf> {
    if is_executable_file(Path::new(path)) {
        return Some(PathBuf::from(path));
    }
    let mut child = Command::new(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    match wait_with_timeout(&mut child, Duration::from_secs(3)) {
        Ok(Some(status)) if status.success() => Some(PathBuf::from(path)),
        _ => None,
    }
}

/// `KepubConverter.convertEpubToKepubWithoutChecks`: `kepubify <epub> -o <dir>/<stem>.kepub.epub`
/// (kepubify only converts when the output name ends with `.kepub.epub`), 10s timeout.
pub fn convert(
    kepubify_path: &Path,
    epub: &Path,
    destination_dir: Option<&Path>,
) -> Option<PathBuf> {
    let dir = destination_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let stem = epub.file_stem()?.to_string_lossy();
    let destination = dir.join(format!("{stem}.kepub.epub"));
    let _ = std::fs::remove_file(&destination);

    let mut child = match Command::new(kepubify_path)
        .arg(epub)
        .arg("-o")
        .arg(&destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            tracing::error!("Failed to create kepubify process: {e}");
            return None;
        }
    };
    match wait_with_timeout(&mut child, Duration::from_secs(10)) {
        Ok(Some(status)) if status.success() => {}
        Ok(Some(status)) => {
            tracing::error!("Kepub conversion failed with {status}");
            return None;
        }
        Ok(None) => {
            tracing::error!("Kepub conversion timeout");
            return None;
        }
        Err(e) => {
            tracing::error!("Kepub conversion failed: {e}");
            return None;
        }
    }
    if destination.is_file() {
        Some(destination)
    } else {
        tracing::error!("Converted file not found: {}", destination.display());
        None
    }
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// `Process.waitFor(timeout)`: polls the child, killing it on timeout (Ok(None)).
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn probe_rejects_missing_and_non_executable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(probe(dir.path().join("nope").to_str().unwrap()).is_none());
        let not_exec = dir.path().join("plain.sh");
        std::fs::write(&not_exec, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        assert!(probe(not_exec.to_str().unwrap()).is_none());
    }

    #[test]
    fn probe_accepts_executable_file() {
        let dir = tempfile::tempdir().unwrap();
        let kepubify = script(dir.path(), "kepubify", "#!/bin/sh\nexit 0\n");
        assert_eq!(probe(kepubify.to_str().unwrap()), Some(kepubify));
    }

    #[test]
    fn convert_writes_stem_kepub_epub() {
        let dir = tempfile::tempdir().unwrap();
        let kepubify = script(dir.path(), "kepubify", "#!/bin/sh\ncp \"$1\" \"$3\"\n");
        let epub = dir.path().join("my book.epub");
        std::fs::write(&epub, b"epub-bytes").unwrap();
        let out = convert(&kepubify, &epub, Some(dir.path())).unwrap();
        assert_eq!(out, dir.path().join("my book.kepub.epub"));
        assert_eq!(std::fs::read(&out).unwrap(), b"epub-bytes");
    }

    #[test]
    fn convert_fails_on_nonzero_exit_and_missing_output() {
        let dir = tempfile::tempdir().unwrap();
        let epub = dir.path().join("a.epub");
        std::fs::write(&epub, b"x").unwrap();
        let failing = script(dir.path(), "kepubify-fail", "#!/bin/sh\nexit 1\n");
        assert!(convert(&failing, &epub, Some(dir.path())).is_none());
        let noop = script(dir.path(), "kepubify-noop", "#!/bin/sh\nexit 0\n");
        assert!(convert(&noop, &epub, Some(dir.path())).is_none());
    }
}
