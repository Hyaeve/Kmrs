//! RAR entry extraction via the `unrar` crate, ported from `RarExtractor.kt`.
//!
//! junrar rejects encrypted and multi-volume archives up front with specific exceptions
//! (ERR_1002 / ERR_1004); unrar reports them through `is_multipart` and its error codes
//! (`MissingPassword` / `BadPassword`), mapped here. Corrupt archives stay generic errors,
//! so the analyzer files them under ERR_1008 like junrar's exceptions.

use crate::error::{MediaError, Result};
use std::path::Path;

/// Returns the bytes of `entry_name` inside the RAR archive at `path`.
pub fn get_entry_bytes(path: &Path, entry_name: &str) -> Result<Vec<u8>> {
    check_multipart(path)?;
    let mut archive = open(path)?;
    loop {
        let header = match archive.read_header() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(e) => return Err(map_unrar_error(e)),
        };
        let e = header.entry();
        if e.is_directory() {
            archive = header
                .skip()
                .map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
            continue;
        }
        if e.filename.to_string_lossy() == entry_name {
            let (bytes, _) = header.read().map_err(map_unrar_error)?;
            return Ok(bytes);
        }
        archive = header
            .skip()
            .map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
    }
    Err(MediaError::EntryNotFound(entry_name.to_string()))
}

/// Lists the file names in the archive; used to detect multi-volume and encrypted archives.
pub fn list_entries(path: &Path) -> Result<Vec<String>> {
    check_multipart(path)?;
    let mut archive = open(path)?;
    let mut names = vec![];
    loop {
        let header = match archive.read_header() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(e) => return Err(map_unrar_error(e)),
        };
        let e = header.entry();
        if !e.is_directory() {
            names.push(e.filename.to_string_lossy().to_string());
        }
        archive = header
            .skip()
            .map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
    }
    Ok(names)
}

fn check_multipart(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(MediaError::NoSuchFile(path.display().to_string()));
    }
    if unrar::Archive::new(path).is_multipart() {
        return Err(MediaError::unsupported_coded(
            "Multi-Volume RAR archives are not supported",
            "ERR_1004",
        ));
    }
    Ok(())
}

fn open(path: &Path) -> Result<unrar::OpenArchive<unrar::Process, unrar::CursorBeforeHeader>> {
    unrar::Archive::new(path)
        .open_for_processing()
        .map_err(map_unrar_error)
}

fn map_unrar_error(e: unrar::error::UnrarError) -> MediaError {
    use unrar::error::Code;
    match e.code {
        Code::MissingPassword | Code::BadPassword => {
            MediaError::unsupported_coded("Encrypted RAR archives are not supported", "ERR_1002")
        }
        // junrar has no coded exception for corrupt archives: the analyzer turns them into
        // ERR_1008, so these must stay uncoded
        Code::BadData | Code::BadArchive | Code::UnknownFormat => {
            MediaError::Other(anyhow::anyhow!("RAR archive is corrupt: {e}"))
        }
        // unrar reports a missing next volume as EOpen during processing
        Code::EOpen if e.when == unrar::error::When::Process => {
            MediaError::unsupported_coded("Multi-Volume RAR archives are not supported", "ERR_1004")
        }
        _ => MediaError::unsupported(format!("could not extract rar entry: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archives() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/archives")
    }

    #[test]
    fn extract_rar4_entry() {
        let bytes = get_entry_bytes(&archives().join("rar4.rar"), "komga-1.png").unwrap();
        assert_eq!(&bytes[0..4], b"\x89PNG");
        assert_eq!(list_entries(&archives().join("rar4.rar")).unwrap().len(), 3);
        // later entries work too (random access order)
        let bytes = get_entry_bytes(&archives().join("rar4.rar"), "komga-3.png").unwrap();
        assert_eq!(&bytes[0..4], b"\x89PNG");
    }

    #[test]
    fn extract_rar5_entry() {
        let bytes = get_entry_bytes(&archives().join("rar5.rar"), "komga.png").unwrap();
        assert_eq!(&bytes[0..4], b"\x89PNG");
        assert_eq!(
            list_entries(&archives().join("rar5.rar")).unwrap(),
            ["komga.png"]
        );
    }

    #[test]
    fn extract_solid_rar_entries() {
        for name in ["rar4-solid.rar", "rar5-solid.rar"] {
            let entries = list_entries(&archives().join(name)).unwrap();
            assert_eq!(entries.len(), 3, "{name}");
            for entry in ["komga-1.png", "komga-2.png", "komga-3.png"] {
                let bytes = get_entry_bytes(&archives().join(name), entry).unwrap();
                assert_eq!(&bytes[0..4], b"\x89PNG", "{name}:{entry}");
            }
        }
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
            match list_entries(&archives().join(name)) {
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

    #[test]
    fn missing_file_is_no_such_file() {
        assert!(matches!(
            get_entry_bytes(&archives().join("missing.rar"), "komga.png"),
            Err(MediaError::NoSuchFile(_))
        ));
        assert!(matches!(
            list_entries(&archives().join("missing.rar")),
            Err(MediaError::NoSuchFile(_))
        ));
    }
}
