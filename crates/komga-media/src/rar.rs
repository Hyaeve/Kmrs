//! RAR entry extraction via libarchive (`compress-tools`), ported from `RarExtractor.kt`.
//!
//! junrar rejects encrypted and multi-volume archives up front with specific exceptions
//! (ERR_1002 / ERR_1004); libarchive does not expose those checks directly, so they are
//! mapped from its error strings.

use crate::error::{MediaError, Result};
use std::path::Path;

/// Returns the bytes of `entry_name` inside the RAR archive at `path`.
pub fn get_entry_bytes(path: &Path, entry_name: &str) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => MediaError::NoSuchFile(path.display().to_string()),
        _ => MediaError::Other(e.into()),
    })?;
    let mut buf = Vec::new();
    compress_tools::uncompress_archive_file(file, &mut buf, entry_name)
        .map_err(|e| map_archive_error(e, entry_name))?;
    Ok(buf)
}

/// Lists the file names in the archive; used to detect multi-volume and encrypted archives.
pub fn list_entries(path: &Path) -> Result<Vec<String>> {
    let file = std::fs::File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => MediaError::NoSuchFile(path.display().to_string()),
        _ => MediaError::Other(e.into()),
    })?;
    compress_tools::list_archive_files(file).map_err(|e| map_archive_error(e, ""))
}

fn map_archive_error(e: compress_tools::Error, entry_name: &str) -> MediaError {
    let details = e.to_string();
    let lower = details.to_lowercase();
    if lower.contains("encrypt") {
        return MediaError::unsupported_coded(
            "Encrypted RAR archives are not supported",
            "ERR_1002",
        );
    }
    if lower.contains("multi") || lower.contains("volume") {
        return MediaError::unsupported_coded(
            "Multi-Volume RAR archives are not supported",
            "ERR_1004",
        );
    }
    // a missing archive file is caught when opening it, so a NotFound from libarchive
    // can only mean the entry is absent
    if matches!(e, compress_tools::Error::Io(ref io) if io.kind() == std::io::ErrorKind::NotFound) {
        return MediaError::EntryNotFound(entry_name.to_string());
    }
    MediaError::unsupported(format!(
        "could not extract rar entry {entry_name}: {details}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archives() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../komga/komga/src/test/resources/archives")
    }

    #[test]
    fn extract_rar4_entry() {
        let bytes = get_entry_bytes(&archives().join("rar4.rar"), "komga-1.png").unwrap();
        assert_eq!(&bytes[0..4], b"\x89PNG");
        assert_eq!(list_entries(&archives().join("rar4.rar")).unwrap().len(), 3);
    }

    #[test]
    fn extract_rar5_entry() {
        let bytes = get_entry_bytes(&archives().join("rar5.rar"), "komga.png").unwrap();
        assert_eq!(&bytes[0..4], b"\x89PNG");
    }

    #[test]
    fn encrypted_rar_is_err_1002() {
        for name in ["rar4-encrypted.rar", "rar5-encrypted.rar"] {
            match get_entry_bytes(&archives().join(name), "komga-1.png") {
                Err(MediaError::Unsupported { code, .. }) => {
                    assert_eq!(code.as_deref(), Some("ERR_1002"), "{name}")
                }
                other => panic!("{name}: expected ERR_1002, got {other:?}"),
            }
        }
    }

    #[test]
    fn missing_entry_is_entry_not_found() {
        assert!(matches!(
            get_entry_bytes(&archives().join("rar4.rar"), "nope.png"),
            Err(MediaError::EntryNotFound(_))
        ));
    }
}
