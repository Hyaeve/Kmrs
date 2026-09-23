//! File hashing, ported from `infrastructure/hash/Hasher.kt` and `KoreaderHasher.kt`.

use std::io::Read;
use std::path::Path;

/// XXH3-128 with seed 0, rendered as 32 lowercase hex chars.
pub fn compute_hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = xxhash_rust::xxh3::Xxh3::with_seed(0);
    // network mounts charge per read() call; an 8 KiB buffer collapses throughput there
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let len = file.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        hasher.update(&buffer[..len]);
    }
    Ok(format!("{:032x}", hasher.digest128()))
}

pub fn compute_hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = xxhash_rust::xxh3::Xxh3::with_seed(0);
    hasher.update(bytes);
    format!("{:032x}", hasher.digest128())
}

/// KOReader sample offsets, ascending: 0, 1K, 4K, 16K, 64K, 256K, 1M, 4M, 16M, 64M, 256M, 1G.
fn koreader_sample_offsets() -> impl Iterator<Item = u64> {
    (-1i64..=10).map(|i| 1024u64.wrapping_shl(((2 * i) & 63) as u32))
}

/// Compute both hashes in one file open and one sequential pass.
///
/// `want_file` requests the full-file XXH3-128 hash, `want_koreader` the KOReader
/// partial MD5. When both are requested the file is streamed once from start to end; the
/// KOReader sample points are captured from the sequential stream as it passes them
/// (they are ascending, so one monotonic pass covers every offset), reproducing the exact
/// JVM buffer-reuse semantics of `KoreaderHasher.kt`: the 1024-byte buffer is never
/// cleared, so a sample shorter than 1024 bytes (at EOF) is padded with the residue of
/// the previous sample, and samples at or beyond EOF are skipped without updating the
/// buffer. Network mounts may return partial buffers; the carry-over (`collected`)
/// reassembles a sample across reads, so the result matches a clean read.
/// Returns `(file_hash, koreader_hash)`, `None` for a hash that was not requested.
pub fn compute_hashes(
    path: &Path,
    want_file: bool,
    want_koreader: bool,
) -> std::io::Result<(Option<String>, Option<String>)> {
    if !want_file && !want_koreader {
        return Ok((None, None));
    }
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    compute_hashes_impl(&mut file, len, want_file, want_koreader)
}

/// Shared by `compute_hashes` and the short-read tests; streams `reader` once and captures
/// the KOReader samples as the pass crosses them.
fn compute_hashes_impl<R: Read>(
    mut reader: R,
    len: u64,
    want_file: bool,
    want_koreader: bool,
) -> std::io::Result<(Option<String>, Option<String>)> {
    // Only offsets strictly below EOF contribute; the rest are skipped (JVM `n == 0`).
    let sample_offsets: Vec<u64> = if want_koreader {
        koreader_sample_offsets().filter(|&o| o < len).collect()
    } else {
        Vec::new()
    };

    let mut file_hasher = xxhash_rust::xxh3::Xxh3::with_seed(0);
    let mut koreader_ctx = md5::Context::new();
    let mut koreader_buf = [0u8; 1024]; // JVM buffer, carries residue across samples
    let mut sample_idx = 0usize; // next sample offset to collect
    let mut collected = 0usize; // bytes collected so far for the current sample
    let mut buffer = vec![0u8; 1 << 20]; // network mounts charge per read() call
    let mut position: u64 = 0;

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        let chunk = &buffer[..n];
        let chunk_end = position + n as u64;
        if want_file {
            file_hasher.update(chunk);
        }
        while sample_idx < sample_offsets.len() {
            let off = sample_offsets[sample_idx];
            let target = ((len - off).min(1024)) as usize;
            if off >= chunk_end {
                break; // sample starts in a later chunk
            }
            // Copy as much of [off + collected, off + target) as this chunk provides.
            let take_start = (off + collected as u64).max(position);
            let take_end = (off + target as u64).min(chunk_end);
            if take_end > take_start {
                let s = (take_start - position) as usize;
                let e = (take_end - position) as usize;
                koreader_buf[collected..collected + (e - s)].copy_from_slice(&chunk[s..e]);
                collected += e - s;
            }
            if collected == target {
                // A read that returns anything consumes the whole 1024-byte buffer.
                koreader_ctx.consume(&koreader_buf[..]);
                sample_idx += 1;
                collected = 0;
            } else {
                break; // sample continues in a later chunk
            }
        }
        position = chunk_end;
        // koreader-only: once every sample is collected there is nothing left to read,
        // so stop instead of streaming the rest of a (possibly huge) file for nothing.
        if !want_file && sample_idx == sample_offsets.len() {
            break;
        }
    }

    let file_hash = want_file.then(|| format!("{:032x}", file_hasher.digest128()));
    let koreader_hash = want_koreader.then(|| {
        let digest = koreader_ctx.compute();
        format!("{digest:x}")
    });
    Ok((file_hash, koreader_hash))
}

/// KOReader's partial MD5, as ported by komga (`KoreaderHasher.kt`): samples at offsets
/// `1024 << (2i)` for i in -1..=10 **with JVM shift semantics** (`shl` masks the shift count to
/// 6 bits, so i=-1 becomes `1024 << 62` = 0 — the file head, not 256 as in KOReader's Lua).
/// The whole 1024-byte buffer is fed to MD5 whenever a read returns anything, including
/// leftovers from the previous read (or zeros on the first).
pub fn compute_koreader_hash(path: &Path) -> std::io::Result<String> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let mut context = md5::Context::new();
    let mut buffer = [0u8; 1024];
    for i in -1i64..=10 {
        let offset = 1024u64.wrapping_shl(((2 * i) & 63) as u32);
        file.seek(SeekFrom::Start(offset))?;
        let n = file.read(&mut buffer)?;
        if n > 0 {
            context.consume(buffer);
        }
    }
    let digest = context.compute();
    Ok(format!("{digest:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xxh3_vectors() {
        // XXH3-128("") = 99aa06d3014798d86001c324468d497f
        assert_eq!(compute_hash_bytes(b""), "99aa06d3014798d86001c324468d497f");
        let dir = std::env::temp_dir().join("kmrs-hasher-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("empty.bin");
        std::fs::write(&file, b"").unwrap();
        assert_eq!(compute_hash(&file).unwrap(), compute_hash_bytes(b""));
    }

    #[test]
    fn koreader_hash_matches_reference() {
        // Expected values computed with an independent Python reimplementation of the rule
        let dir = std::env::temp_dir().join("kmrs-hasher-test");
        std::fs::create_dir_all(&dir).unwrap();

        let file = dir.join("book.bin");
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&file, &data).unwrap();
        assert_eq!(
            compute_koreader_hash(&file).unwrap(),
            "c43e7af7c64be64ff8765e78ee771294"
        );

        let tiny = dir.join("tiny.bin");
        std::fs::write(&tiny, (0u8..100).collect::<Vec<_>>()).unwrap();
        assert_eq!(
            compute_koreader_hash(&tiny).unwrap(),
            "6ebd2f0c1acf7e8f7e0e205c671a86e1"
        );

        let empty = dir.join("empty2.bin");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(
            compute_koreader_hash(&empty).unwrap(),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
    }

    fn patterned(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn compute_hashes_matches_individual_functions() {
        let dir = std::env::temp_dir().join("kmrs-hasher-test");
        std::fs::create_dir_all(&dir).unwrap();
        // sizes exercise: empty, tiny, residue-at-sample-tail (2000), the 300000
        // reference vector, and a size whose 1M sample lies mid-file
        for (name, data) in [
            ("merge-empty.bin", Vec::new()),
            ("merge-tiny.bin", patterned(100)),
            ("merge-residue.bin", patterned(2000)),
            ("merge-300k.bin", patterned(300_000)),
            ("merge-5m.bin", patterned(5_000_000)),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, &data).unwrap();
            let (file, koreader) = compute_hashes(&path, true, true).unwrap();
            assert_eq!(
                file.as_deref(),
                Some(compute_hash(&path).unwrap().as_str()),
                "{name}"
            );
            assert_eq!(
                koreader.as_deref(),
                Some(compute_koreader_hash(&path).unwrap().as_str()),
                "{name}"
            );
        }
    }

    #[test]
    fn compute_hashes_partial_requests() {
        let dir = std::env::temp_dir().join("kmrs-hasher-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("merge-partial.bin");
        std::fs::write(&path, patterned(300_000)).unwrap();

        let (file, koreader) = compute_hashes(&path, true, false).unwrap();
        assert_eq!(file.as_deref(), Some(compute_hash(&path).unwrap().as_str()));
        assert!(koreader.is_none());

        let (file, koreader) = compute_hashes(&path, false, true).unwrap();
        assert!(file.is_none());
        assert_eq!(
            koreader.as_deref(),
            Some(compute_koreader_hash(&path).unwrap().as_str())
        );

        let (file, koreader) = compute_hashes(&path, false, false).unwrap();
        assert!(file.is_none() && koreader.is_none());
    }

    #[test]
    fn compute_hashes_sample_at_chunk_boundary() {
        // 1048576 (the 1M sample offset) is exactly 1 << 20: the sample starts at a 1 MiB
        // read-buffer boundary and lies entirely inside the second chunk. In fact none of
        // the 12 sample offsets straddles a 1 MiB boundary — all are 1024-aligned and
        // off % 1MiB is 0 or <= 256K, so off + 1024 never crosses — so the cross-chunk
        // carry-over is only reachable through short reads, exercised below by
        // compute_hashes_reassembles_samples_across_short_reads.
        let dir = std::env::temp_dir().join("kmrs-hasher-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("merge-chunk-boundary.bin");
        let data = patterned(2_500_000);
        std::fs::write(&path, &data).unwrap();
        let (_, koreader) = compute_hashes(&path, true, true).unwrap();
        assert_eq!(
            koreader.as_deref(),
            Some(compute_koreader_hash(&path).unwrap().as_str())
        );
    }

    /// A reader that returns at most `max` bytes per read, like a network mount serving
    /// partial buffers.
    struct ShortReader {
        inner: std::fs::File,
        max: usize,
        bytes_read: usize,
    }

    impl Read for ShortReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let cap = buf.len().min(self.max);
            let n = self.inner.read(&mut buf[..cap])?;
            self.bytes_read += n;
            Ok(n)
        }
    }

    #[test]
    fn compute_hashes_reassembles_samples_across_short_reads() {
        // 1024-byte samples cut by 1000-byte reads: every sample after offset 0 straddles
        // multiple reads, driving the carry-over reassembly path that is unreachable with
        // a full-buffer reader (no sample offset crosses a 1 MiB boundary). The result
        // must match the seek-based reference, i.e. short reads change nothing.
        let dir = std::env::temp_dir().join("kmrs-hasher-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("merge-short-reads.bin");
        let data = patterned(300_000);
        std::fs::write(&path, &data).unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let len = file.metadata().unwrap().len();
        let mut reader = ShortReader {
            inner: file,
            max: 1000,
            bytes_read: 0,
        };
        let (file_hash, koreader) = compute_hashes_impl(&mut reader, len, true, true).unwrap();
        assert_eq!(
            file_hash.as_deref(),
            Some(compute_hash(&path).unwrap().as_str())
        );
        assert_eq!(
            koreader.as_deref(),
            Some(compute_koreader_hash(&path).unwrap().as_str())
        );
    }

    #[test]
    fn compute_hashes_koreader_only_stops_after_last_sample() {
        // 7 MiB file: the last sample (4M) sits in chunk 4, so without the early exit the
        // koreader-only pass would stream the trailing ~2 MiB to EOF for nothing.
        let dir = std::env::temp_dir().join("kmrs-hasher-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("merge-koreader-early-exit.bin");
        let data = patterned(7_000_000);
        std::fs::write(&path, &data).unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let len = file.metadata().unwrap().len();
        let mut reader = ShortReader {
            inner: file,
            max: 1 << 20,
            bytes_read: 0,
        };
        let (file_hash, koreader) = compute_hashes_impl(&mut reader, len, false, true).unwrap();
        assert!(file_hash.is_none());
        assert_eq!(
            koreader.as_deref(),
            Some(compute_koreader_hash(&path).unwrap().as_str())
        );
        // the 1M and 4M samples land in chunks 1 and 4; the trailing 2 MiB is not read
        assert_eq!(reader.bytes_read, 5 * (1 << 20));
    }
}
