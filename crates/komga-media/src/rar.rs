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

/// Reads several entries with a single sequential pass over the archive. RAR cannot seek,
/// so solid archives are traversed in order and matching entries are extracted as they are
/// encountered. Output follows `entry_names`; when a requested name is absent from the
/// archive the whole archive is still scanned, then `EntryNotFound` is returned for the
/// first missing name. `entry_names` may repeat a name: each same-named archive entry
/// fills the next unfilled slot in request order.
pub fn get_entries_bytes(path: &Path, entry_names: &[&str]) -> Result<Vec<Vec<u8>>> {
    check_multipart(path)?;
    let mut archive = open(path)?;
    let mut out: Vec<Option<Vec<u8>>> = vec![None; entry_names.len()];
    loop {
        let header = match archive.read_header() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(e) => return Err(map_unrar_error(e)),
        };
        let e = header.entry();
        let name = e.filename.to_string_lossy();
        if e.is_directory() {
            archive = header
                .skip()
                .map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
            continue;
        }
        // `out` indexes follow `entry_names`; each entry maps to the first unfilled slot
        // whose name matches, so output order follows the request even when the archive
        // order differs (a repeated name lands in the next unfilled slot).
        if let Some(idx) = entry_names
            .iter()
            .enumerate()
            .find(|(i, &r)| r == name && out[*i].is_none())
            .map(|(i, _)| i)
        {
            let (bytes, next) = header.read().map_err(map_unrar_error)?;
            out[idx] = Some(bytes);
            archive = next;
            if out.iter().all(|o| o.is_some()) {
                break;
            }
        } else {
            archive = header
                .skip()
                .map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
        }
    }
    if let Some(pos) = out.iter().position(|o| o.is_none()) {
        return Err(MediaError::EntryNotFound(entry_names[pos].to_string()));
    }
    Ok(out.into_iter().map(|o| o.unwrap()).collect())
}

/// Like `get_entries_bytes`, but with per-entry outcomes: a missing or unreadable entry
/// fails only its own slot, so best-effort scans (barcode) keep trying the remaining
/// candidates. Still a single sequential pass. A payload read error consumes the archive
/// cursor (the unrar crate cannot continue after a failed extract), so the slots after it
/// degrade to `EntryNotFound`; missing entries at end-of-archive are filled the same way.
pub fn get_entries_bytes_tolerant(
    path: &Path,
    entry_names: &[&str],
) -> Result<Vec<Result<Vec<u8>>>> {
    check_multipart(path)?;
    let mut archive = open(path)?;
    let mut out: Vec<Option<Result<Vec<u8>>>> = std::iter::repeat_with(|| None)
        .take(entry_names.len())
        .collect();
    loop {
        let header = match archive.read_header() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(e) => return Err(map_unrar_error(e)),
        };
        let e = header.entry();
        let name = e.filename.to_string_lossy();
        if e.is_directory() {
            archive = header
                .skip()
                .map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
            continue;
        }
        if let Some(idx) = entry_names
            .iter()
            .enumerate()
            .find(|(i, &r)| r == name && out[*i].is_none())
            .map(|(i, _)| i)
        {
            match header.read() {
                Ok((bytes, next)) => {
                    out[idx] = Some(Ok(bytes));
                    archive = next;
                }
                Err(e) => {
                    out[idx] = Some(Err(map_unrar_error(e)));
                    break;
                }
            }
            if out.iter().all(|o| o.is_some()) {
                break;
            }
        } else {
            archive = header
                .skip()
                .map_err(|e| MediaError::Other(anyhow::anyhow!(e)))?;
        }
    }
    Ok(out
        .into_iter()
        .enumerate()
        .map(|(i, o)| {
            o.unwrap_or_else(|| Err(MediaError::EntryNotFound(entry_names[i].to_string())))
        })
        .collect())
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
    fn get_entries_bytes_single_pass_matches_individual_reads() {
        let path = archives().join("rar4.rar");
        let names = ["komga-1.png", "komga-3.png", "komga-2.png"];
        let batch = get_entries_bytes(&path, &names).unwrap();
        for (name, bytes) in names.iter().zip(batch.iter()) {
            assert_eq!(
                *bytes,
                get_entry_bytes(&path, name).unwrap(),
                "{name} (order preserved)"
            );
        }

        // a missing name fails after scanning the whole archive, like the single-entry read
        assert!(matches!(
            get_entries_bytes(&path, &["komga-1.png", "nope.png"]),
            Err(MediaError::EntryNotFound(_))
        ));
    }

    #[test]
    fn get_entries_bytes_solid_archives() {
        for name in ["rar4-solid.rar", "rar5-solid.rar"] {
            let path = archives().join(name);
            let batch =
                get_entries_bytes(&path, &["komga-1.png", "komga-2.png", "komga-3.png"]).unwrap();
            for (i, entry) in ["komga-1.png", "komga-2.png", "komga-3.png"]
                .iter()
                .enumerate()
            {
                assert_eq!(
                    batch[i],
                    get_entry_bytes(&path, entry).unwrap(),
                    "{name}:{entry}"
                );
            }
        }
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
