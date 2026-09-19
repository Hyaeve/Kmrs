//! ZIP entry extraction, ported from `ZipFileUtils.kt` (`getZipEntryBytes`).
//!
//! The rust `zip` crate resolves names from the central directory only; the commons-compress
//! slow path that re-reads local file headers for unicode extra fields has no equivalent.
//! That only affects archives whose central directory names are mojibake, which is accepted.

use crate::error::{MediaError, Result};
use std::io::Read;
use std::path::Path;

/// Returns the bytes of `entry_name` inside the zip at `path`.
pub fn get_entry_bytes(path: &Path, entry_name: &str) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => MediaError::NoSuchFile(path.display().to_string()),
        _ => MediaError::Other(e.into()),
    })?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| MediaError::unsupported(format!("could not open zip archive: {e}")))?;
    let mut entry = match archive.by_name(entry_name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => {
            return Err(MediaError::EntryNotFound(entry_name.to_string()))
        }
        Err(e) => {
            return Err(MediaError::unsupported(format!(
                "could not read zip entry: {e}"
            )))
        }
    };
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf).map_err(|e| {
        MediaError::unsupported(format!("could not extract zip entry {entry_name}: {e}"))
    })?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archives() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/resources/archives")
    }

    #[test]
    fn extract_existing_entry() {
        let bytes = get_entry_bytes(&archives().join("zip.zip"), "komga.png").unwrap();
        assert_eq!(bytes.len(), 3108);
        assert_eq!(&bytes[0..4], b"\x89PNG");
    }

    #[test]
    fn missing_entry_is_entry_not_found() {
        assert!(matches!(
            get_entry_bytes(&archives().join("zip.zip"), "nope.png"),
            Err(MediaError::EntryNotFound(_))
        ));
    }

    #[test]
    fn missing_file_is_no_such_file() {
        assert!(matches!(
            get_entry_bytes(&archives().join("missing.zip"), "komga.png"),
            Err(MediaError::NoSuchFile(_))
        ));
    }

    #[test]
    fn stored_and_deflate_variants() {
        for name in ["zip-copy.zip", "zip.zip", "zip-bzip2.zip"] {
            let bytes = get_entry_bytes(&archives().join(name), "komga.png").unwrap();
            assert_eq!(bytes.len(), 3108, "{name}");
        }
    }
}
