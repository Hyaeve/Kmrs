//! ZIP entry extraction, ported from `ZipFileUtils.kt` (`getZipEntryBytes`).
//!
//! Entry names come from the central directory only and are decoded the way commons-compress
//! decodes them: UTF-8 with `?` replacement over the raw bytes (a unicode path extra field
//! supplies the UTF-8 bytes where present).

use crate::error::{MediaError, Result};
use std::io::Read;
use std::path::Path;

/// The commons-compress (`NioZipEncoding`) entry-name decoding: UTF-8, with a literal `?`
/// replacing each malformed unit (a whole surrogate sequence counts as one). Page names are
/// stored in MEDIA_PAGE in this exact form, so a Java komga and kmrs read each other's
/// databases; the `zip` crate's own CP437 fallback would not round-trip. Differentially
/// verified against OpenJDK's decoder on every 1-3 byte input plus a 4-byte edge grid.
pub(crate) fn java_entry_name(raw: &[u8]) -> String {
    fn is_cont(b: u8) -> bool {
        (b & 0xC0) == 0x80
    }

    let mut out = String::new();
    let mut i = 0;
    while i < raw.len() {
        let b1 = raw[i];
        if b1 < 0x80 {
            out.push(b1 as char);
            i += 1;
        } else if (0xC2..=0xDF).contains(&b1) {
            if i + 1 < raw.len() && is_cont(raw[i + 1]) {
                let c = ((b1 as u32 & 0x1F) << 6) | (raw[i + 1] as u32 & 0x3F);
                out.push(char::from_u32(c).unwrap());
                i += 2;
            } else {
                out.push('?');
                i += 1;
            }
        } else if (0xE0..=0xEF).contains(&b1) {
            if i + 1 >= raw.len() {
                out.push('?');
                i += 1;
                continue;
            }
            let b2 = raw[i + 1];
            if (b1 == 0xE0 && (b2 & 0xE0) == 0x80) || !is_cont(b2) {
                out.push('?');
                i += 1;
                continue;
            }
            if i + 2 >= raw.len() {
                out.push('?');
                i += 2;
                continue;
            }
            let b3 = raw[i + 2];
            if !is_cont(b3) {
                out.push('?');
                i += 2;
                continue;
            }
            let c = ((b1 as u32 & 0x0F) << 12) | ((b2 as u32 & 0x3F) << 6) | (b3 as u32 & 0x3F);
            if (0xD800..=0xDFFF).contains(&c) {
                out.push('?');
                i += 3;
            } else {
                out.push(char::from_u32(c).unwrap());
                i += 3;
            }
        } else if (0xF0..=0xF4).contains(&b1) {
            if i + 1 >= raw.len() {
                out.push('?');
                i += 1;
                continue;
            }
            let b2 = raw[i + 1];
            if (b1 == 0xF0 && !(0x90..=0xBF).contains(&b2))
                || (b1 == 0xF4 && (b2 & 0xF0) != 0x80)
                || !is_cont(b2)
            {
                out.push('?');
                i += 1;
                continue;
            }
            if i + 2 >= raw.len() {
                out.push('?');
                i += 2;
                continue;
            }
            let b3 = raw[i + 2];
            if !is_cont(b3) {
                out.push('?');
                i += 2;
                continue;
            }
            if i + 3 >= raw.len() {
                out.push('?');
                i += 3;
                continue;
            }
            let b4 = raw[i + 3];
            if !is_cont(b4) {
                out.push('?');
                i += 3;
                continue;
            }
            let uc = ((b1 as u32 & 0x07) << 18)
                | ((b2 as u32 & 0x3F) << 12)
                | ((b3 as u32 & 0x3F) << 6)
                | (b4 as u32 & 0x3F);
            if !(0x10000..=0x10FFFF).contains(&uc) {
                out.push('?');
                i += 3;
                continue;
            }
            out.push(char::from_u32(uc).unwrap());
            i += 4;
        } else {
            out.push('?');
            i += 1;
        }
    }
    out
}

/// A ZIP archive opened once, with per-entry reads on the same handle. Lazy: only the
/// requested entry is touched per call, so a scan can stop early without touching the
/// remaining candidates (network mounts charge per open, not per read).
pub struct ZipEntries {
    archive: zip::ZipArchive<std::fs::File>,
}

impl ZipEntries {
    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => MediaError::NoSuchFile(path.display().to_string()),
            _ => MediaError::Other(e.into()),
        })?;
        let archive = zip::ZipArchive::new(file)
            .map_err(|e| MediaError::unsupported(format!("could not open zip archive: {e}")))?;
        Ok(Self { archive })
    }

    /// Reads one entry on the held archive; repeated calls reuse the same handle.
    pub fn read(&mut self, entry_name: &str) -> Result<Vec<u8>> {
        read_entry(&mut self.archive, entry_name)
    }
}

/// Returns the bytes of `entry_name` inside the zip at `path`.
pub fn get_entry_bytes(path: &Path, entry_name: &str) -> Result<Vec<u8>> {
    ZipEntries::open(path)?.read(entry_name)
}

/// Reads several entries with a single archive open, in the order given. Network mounts
/// charge per open, so hashing the first and last pages must not reopen the archive for
/// every page.
pub fn get_entries_bytes(path: &Path, entry_names: &[&str]) -> Result<Vec<Vec<u8>>> {
    let mut entries = ZipEntries::open(path)?;
    entry_names.iter().map(|name| entries.read(name)).collect()
}

/// `ZipArchive::by_name` matches the raw central-directory bytes, which the stored name
/// forms do not round-trip to: also look entries up by their decoded names — first the
/// `zip` crate's own form (rows written before names moved to the commons-compress form),
/// then `java_entry_name`. A raw hit wins over a decoded-name collision; duplicate-name
/// archives are degenerate either way.
pub(crate) fn by_name_decoded<'a, R: std::io::Read + std::io::Seek>(
    archive: &'a mut zip::ZipArchive<R>,
    entry_name: &str,
) -> std::result::Result<zip::read::ZipFile<'a, R>, zip::result::ZipError> {
    let index = archive
        .index_for_name(entry_name)
        .or_else(|| (0..archive.len()).find(|&i| archive.name_for_index(i) == Some(entry_name)))
        .or_else(|| {
            (0..archive.len()).find(|&i| {
                archive
                    .by_index(i)
                    .is_ok_and(|e| java_entry_name(e.name_raw()) == entry_name)
            })
        });
    match index {
        Some(index) => archive.by_index(index),
        None => Err(zip::result::ZipError::FileNotFound),
    }
}

fn read_entry(archive: &mut zip::ZipArchive<std::fs::File>, entry_name: &str) -> Result<Vec<u8>> {
    let mut entry = match by_name_decoded(archive, entry_name) {
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
pub(crate) fn write_zip_raw(path: &Path, entries: &[(&[u8], &[u8])]) {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    // minimal stored zip: local headers, then the central directory, then EOCD; flags stay
    // 0, so non-ASCII raw names are written without the UTF-8 flag, like Windows tools do
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in entries {
        let crc = crc32(data);
        let offset = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[0; 4]);
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 12]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    std::fs::write(path, out).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archives() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/archives")
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

    #[test]
    fn get_entries_bytes_matches_individual_reads() {
        let path = archives().join("zip.zip");
        let batch = get_entries_bytes(&path, &["komga.png"]).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0], get_entry_bytes(&path, "komga.png").unwrap());

        // missing entries surface EntryNotFound like the single-entry function
        assert!(matches!(
            get_entries_bytes(&path, &["nope.png"]),
            Err(MediaError::EntryNotFound(_))
        ));
    }

    /// Expected values produced by commons-compress 1.28.0 (`NioZipEncoding`: UTF-8 with
    /// `?` replacement); the decoder itself is differentially verified against OpenJDK.
    #[test]
    fn java_entry_name_matches_commons_compress() {
        let cases: &[(&[u8], &str)] = &[
            // GBK 他国日记
            (&[0xCB, 0xFB, 0xB9, 0xFA, 0xC8, 0xD5, 0xBC, 0xC7], "?????ռ?"),
            // GBK 封面.jpg
            (
                &[0xB7, 0xE2, 0xC3, 0xE6, 0x2E, 0x6A, 0x70, 0x67],
                "????.jpg",
            ),
            (&[0xE4, 0xB8], "?"),
            (&[0x80, 0x41], "?A"),
            (&[0xC0, 0xAF], "??"),
            // a surrogate sequence is replaced as a single unit (unlike U+FFFD decoders)
            (&[0xED, 0xA0, 0x80], "?"),
            // valid UTF-8 封面
            (&[0xE5, 0xB0, 0x81, 0xE9, 0x9D, 0xA2], "封面"),
            (&[0xF0, 0x9F, 0x41], "?A"),
            (&[0xF0, 0x9F, 0x98, 0x80], "\u{1F600}"),
            // > U+10FFFF
            (&[0xF4, 0x90, 0xBF, 0x80], "????"),
        ];
        for (raw, expected) in cases {
            assert_eq!(java_entry_name(raw), *expected, "{raw:02X?}");
        }
    }

    /// GBK "封面.jpg": Windows tools write non-ASCII entry names without the UTF-8 flag;
    /// the central directory then decodes (CP437) to a mojibake string whose bytes differ
    /// from the raw ones. Readers look entries up by that decoded name (as stored in
    /// MEDIA_PAGE), like commons-compress's name map.
    #[test]
    fn read_by_decoded_name_when_raw_bytes_are_not_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.zip");
        let name_gbk: &[u8] = &[0xB7, 0xE2, 0xC3, 0xE6, 0x2E, 0x6A, 0x70, 0x67];
        write_zip_raw(&path, &[(name_gbk, b"jpeg-bytes")]);

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let decoded = archive.by_index(0).unwrap().name().to_string();
        assert_ne!(decoded.as_bytes(), name_gbk, "fixture must decode the name");

        assert_eq!(get_entry_bytes(&path, &decoded).unwrap(), b"jpeg-bytes");
        assert_eq!(
            get_entries_bytes(&path, &[decoded.as_str()]).unwrap(),
            vec![b"jpeg-bytes".to_vec()]
        );
        // the commons-compress form (what analysis stores and Java writes) resolves too
        assert_eq!(
            get_entry_bytes(&path, &java_entry_name(name_gbk)).unwrap(),
            b"jpeg-bytes"
        );
        // no charset guessing: the UTF-8 spelling of the same text is not a key
        assert!(matches!(
            get_entry_bytes(&path, "封面.jpg"),
            Err(MediaError::EntryNotFound(_))
        ));
    }

    /// UTF-8 name bytes without the UTF-8 flag (old/buggy writers): commons-compress still
    /// decodes them cleanly, so the proper name resolves the entry, as in Java komga.
    #[test]
    fn read_by_decoded_name_when_utf8_name_lacks_the_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf8-no-flag.zip");
        write_zip_raw(&path, &[("封面.jpg".as_bytes(), b"jpeg-bytes")]);

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let decoded = archive.by_index(0).unwrap().name().to_string();
        assert_ne!(decoded, "封面.jpg", "fixture must decode as CP437");

        assert_eq!(get_entry_bytes(&path, &decoded).unwrap(), b"jpeg-bytes");
        assert_eq!(get_entry_bytes(&path, "封面.jpg").unwrap(), b"jpeg-bytes");
    }
}
