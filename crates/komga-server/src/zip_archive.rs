//! Zip archive writer reproducing the byte layout of Apache Commons Compress
//! `ZipArchiveOutputStream` with `DEFLATED`/`NO_COMPRESSION`/`Zip64Mode.Always`
//! (komga's series/readlist downloads and book conversion):
//! - entries are deflate streams of 65535-byte stored blocks
//! - local headers carry placeholder sizes (0xFFFFFFFF) and a zeroed Zip64 extra field,
//!   followed by a signature-prefixed data descriptor with 8-byte sizes
//! - central entries carry the real values inside the Zip64 extra field
//! - the archive ends with a Zip64 EOCD, its locator, and a classic EOCD
//! - entry times are the current local time at DOS 2-second precision

use std::io::{self, Read, Write};

const LOCAL_SIG: u32 = 0x04034b50;
const DATA_DESCRIPTOR_SIG: u32 = 0x08074b50;
const CENTRAL_SIG: u32 = 0x02014b50;
const ZIP64_EOCD_SIG: u32 = 0x06064b50;
const ZIP64_LOCATOR_SIG: u32 = 0x07064b50;
const EOCD_SIG: u32 = 0x06054b50;

const VERSION_ZIP64: u16 = 45;
const FLAGS: u16 = 0x0808; // data descriptor + EFS (UTF-8), as Commons Compress writes
const METHOD_DEFLATE: u16 = 8;
const ZIP64_TAG: u16 = 0x0001;
const U32_MAX: u32 = 0xFFFFFFFF;

struct CentralRecord {
    name: Vec<u8>,
    dos_time: u16,
    dos_date: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
}

pub struct ZipWriter<W: Write> {
    out: W,
    offset: u64,
    entries: Vec<CentralRecord>,
}

impl<W: Write> ZipWriter<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            offset: 0,
            entries: Vec::new(),
        }
    }

    /// Adds one entry, streaming the content through in 64 KiB chunks.
    pub fn add_entry(&mut self, name: &str, reader: impl Read) -> io::Result<()> {
        let (dos_time, dos_date) = dos_now();
        let local_header_offset = self.offset;

        self.write_all(&LOCAL_SIG.to_le_bytes())?;
        self.write_all(&VERSION_ZIP64.to_le_bytes())?;
        self.write_all(&FLAGS.to_le_bytes())?;
        self.write_all(&METHOD_DEFLATE.to_le_bytes())?;
        self.write_all(&dos_time.to_le_bytes())?;
        self.write_all(&dos_date.to_le_bytes())?;
        self.write_all(&0u32.to_le_bytes())?; // crc in the data descriptor
        self.write_all(&U32_MAX.to_le_bytes())?;
        self.write_all(&U32_MAX.to_le_bytes())?;
        self.write_all(&(name.len() as u16).to_le_bytes())?;
        self.write_all(&20u16.to_le_bytes())?; // zip64 extra: tag + 16 bytes of zeros
        self.write_all(name.as_bytes())?;
        self.write_all(&ZIP64_TAG.to_le_bytes())?;
        self.write_all(&16u16.to_le_bytes())?;
        self.write_all(&[0u8; 16])?;

        let (crc32, compressed_size, uncompressed_size) = self.write_deflate_stored(reader)?;

        self.write_all(&DATA_DESCRIPTOR_SIG.to_le_bytes())?;
        self.write_all(&crc32.to_le_bytes())?;
        self.write_all(&compressed_size.to_le_bytes())?;
        self.write_all(&uncompressed_size.to_le_bytes())?;

        self.entries.push(CentralRecord {
            name: name.as_bytes().to_vec(),
            dos_time,
            dos_date,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
        });
        Ok(())
    }

    /// Deflate level 0: stored blocks of at most 65535 bytes, final block flagged BFINAL.
    fn write_deflate_stored(&mut self, mut reader: impl Read) -> io::Result<(u32, u64, u64)> {
        let mut hasher = crc32fast::Hasher::new();
        let mut buf = vec![0u8; 65535];
        let mut filled = 0usize;
        let mut compressed = 0u64;
        let mut uncompressed = 0u64;
        loop {
            let n = reader.read(&mut buf[filled..])?;
            filled += n;
            if filled == buf.len() {
                self.write_stored_block(&buf, false)?;
                hasher.update(&buf);
                compressed += 5 + filled as u64;
                uncompressed += filled as u64;
                filled = 0;
            }
            if n == 0 {
                break;
            }
        }
        self.write_stored_block(&buf[..filled], true)?;
        hasher.update(&buf[..filled]);
        compressed += 5 + filled as u64;
        uncompressed += filled as u64;
        Ok((hasher.finalize(), compressed, uncompressed))
    }

    fn write_stored_block(&mut self, data: &[u8], last: bool) -> io::Result<()> {
        let len = data.len() as u16;
        self.write_all(&[last as u8])?;
        self.write_all(&len.to_le_bytes())?;
        self.write_all(&(!len).to_le_bytes())?;
        self.write_all(data)
    }

    pub fn finish(mut self) -> io::Result<W> {
        let cd_offset = self.offset;
        for i in 0..self.entries.len() {
            let entry = &self.entries[i];
            let mut buf = Vec::with_capacity(46 + entry.name.len() + 32);
            let w = &mut buf;
            w.write_all(&CENTRAL_SIG.to_le_bytes())?;
            w.write_all(&VERSION_ZIP64.to_le_bytes())?; // version made by (platform FAT)
            w.write_all(&VERSION_ZIP64.to_le_bytes())?; // version needed
            w.write_all(&FLAGS.to_le_bytes())?;
            w.write_all(&METHOD_DEFLATE.to_le_bytes())?;
            w.write_all(&entry.dos_time.to_le_bytes())?;
            w.write_all(&entry.dos_date.to_le_bytes())?;
            w.write_all(&entry.crc32.to_le_bytes())?;
            w.write_all(&U32_MAX.to_le_bytes())?;
            w.write_all(&U32_MAX.to_le_bytes())?;
            w.write_all(&(entry.name.len() as u16).to_le_bytes())?;
            w.write_all(&32u16.to_le_bytes())?; // zip64 extra: tag + 28 bytes
            w.write_all(&0u16.to_le_bytes())?; // comment length
            w.write_all(&0u16.to_le_bytes())?; // disk number
            w.write_all(&0u16.to_le_bytes())?; // internal attributes
            w.write_all(&0u32.to_le_bytes())?; // external attributes
            w.write_all(&U32_MAX.to_le_bytes())?; // local header offset (in the zip64 extra)
            w.write_all(&entry.name)?;
            w.write_all(&ZIP64_TAG.to_le_bytes())?;
            w.write_all(&28u16.to_le_bytes())?;
            w.write_all(&entry.uncompressed_size.to_le_bytes())?;
            w.write_all(&entry.compressed_size.to_le_bytes())?;
            w.write_all(&entry.local_header_offset.to_le_bytes())?;
            w.write_all(&0u32.to_le_bytes())?;
            self.write_all(&buf)?;
        }
        let cd_size = self.offset - cd_offset;

        let zip64_eocd_offset = self.offset;
        self.write_all(&ZIP64_EOCD_SIG.to_le_bytes())?;
        self.write_all(&44u64.to_le_bytes())?;
        self.write_all(&VERSION_ZIP64.to_le_bytes())?;
        self.write_all(&VERSION_ZIP64.to_le_bytes())?;
        self.write_all(&0u32.to_le_bytes())?;
        self.write_all(&0u32.to_le_bytes())?;
        self.write_all(&(self.entries.len() as u64).to_le_bytes())?;
        self.write_all(&(self.entries.len() as u64).to_le_bytes())?;
        self.write_all(&cd_size.to_le_bytes())?;
        self.write_all(&cd_offset.to_le_bytes())?;

        self.write_all(&ZIP64_LOCATOR_SIG.to_le_bytes())?;
        self.write_all(&0u32.to_le_bytes())?;
        self.write_all(&zip64_eocd_offset.to_le_bytes())?;
        self.write_all(&1u32.to_le_bytes())?;

        self.write_all(&EOCD_SIG.to_le_bytes())?;
        self.write_all(&0u16.to_le_bytes())?;
        self.write_all(&0u16.to_le_bytes())?;
        self.write_all(&(self.entries.len() as u16).to_le_bytes())?;
        self.write_all(&(self.entries.len() as u16).to_le_bytes())?;
        self.write_all(&(cd_size as u32).to_le_bytes())?;
        self.write_all(&(cd_offset as u32).to_le_bytes())?;
        self.write_all(&0u16.to_le_bytes())?;
        Ok(self.out)
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.offset += bytes.len() as u64;
        Ok(())
    }
}

/// DOS date/time of the current local time (2-second precision), like `new ZipArchiveEntry(name)`.
fn dos_now() -> (u16, u16) {
    let now = komga_core::time_codec::now_utc().to_offset(komga_core::time_codec::system_offset());
    let dos_time =
        ((now.hour() as u16) << 11) | ((now.minute() as u16) << 5) | (now.second() as u16 / 2);
    let dos_date =
        (((now.year() as u16) - 1980) << 9) | ((now.month() as u16) << 5) | now.day() as u16;
    (dos_time, dos_date)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Vec::new());
        for (name, data) in entries {
            zip.add_entry(name, *data).unwrap();
        }
        zip.finish().unwrap()
    }

    #[test]
    fn roundtrip_and_layout() {
        let content = vec![7u8; 70000]; // two stored blocks
        let bytes = build(&[("a.bin", &content[..]), ("b.bin", b"")]);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(archive.len(), 2);
        let (name, method, crc, size, year) = {
            let first = archive.by_index(0).unwrap();
            (
                first.name().to_string(),
                first.compression(),
                first.crc32(),
                first.size(),
                first.last_modified().unwrap().year(),
            )
        };
        assert_eq!(name, "a.bin");
        assert_eq!(method, zip::CompressionMethod::Deflated);
        assert_eq!(crc, crc32fast::hash(&content));
        assert_eq!(size, 70000);
        assert!(year >= 2024);
        let second = archive.by_index(1).unwrap();
        assert_eq!(second.size(), 0);
        // local header of the first entry: zip64 placeholder sizes and zeroed extra
        assert_eq!(&bytes[0..4], &LOCAL_SIG.to_le_bytes());
        assert_eq!(&bytes[18..26], &U32_MAX.to_le_bytes()[..4].repeat(2));
    }

    #[test]
    fn stored_blocks_chunk_at_65535() {
        let content = vec![1u8; 65536];
        let bytes = build(&[("x", &content[..])]);
        // 65535-byte block + 1-byte final block: (65535+5) + (1+5)
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let entry = archive.by_index(0).unwrap();
        assert_eq!(entry.compressed_size(), 65536 + 10);

        // an exact multiple gets a trailing empty final block: (65535+5)*2 + (0+5)
        let content = vec![1u8; 65535 * 2];
        let bytes = build(&[("x", &content[..])]);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let entry = archive.by_index(0).unwrap();
        assert_eq!(entry.compressed_size(), 65535 * 2 + 15);
    }
}
