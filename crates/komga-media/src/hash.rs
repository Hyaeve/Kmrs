//! File hashing, ported from `infrastructure/hash/Hasher.kt` and `KoreaderHasher.kt`.

use std::io::Read;
use std::path::Path;

/// XXH3-128 with seed 0, rendered as 32 lowercase hex chars.
pub fn compute_hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = xxhash_rust::xxh3::Xxh3::with_seed(0);
    let mut buffer = [0u8; 8192];
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
        assert_eq!(
            compute_hash_bytes(b""),
            "99aa06d3014798d86001c324468d497f"
        );
        let dir = std::env::temp_dir().join("komga-rs-hasher-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("empty.bin");
        std::fs::write(&file, b"").unwrap();
        assert_eq!(compute_hash(&file).unwrap(), compute_hash_bytes(b""));
    }

    #[test]
    fn koreader_hash_matches_reference() {
        // Expected values computed with an independent Python reimplementation of the rule
        let dir = std::env::temp_dir().join("komga-rs-hasher-test");
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
}
