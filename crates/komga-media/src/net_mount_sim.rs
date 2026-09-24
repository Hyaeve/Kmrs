//! Before/after simulation for the network-mount analysis slowness (kmworks/kmrs#40).
//!
//! Counts the reads/seeks/bytes of the old and new analyze + hash paths against a synthetic
//! CBZ, then models wall time at various per-operation latencies: a network mount turns
//! every read()/seek into a round trip (the reporter saw ~3.9 MB/s on 8 KiB reads, i.e.
//! ~2 ms per call; cloud-drive backends are more like tens of ms).
//!
//! Run with: cargo test -p komga-media -- --ignored --nocapture net_mount

use crate::analyzer::zip_entries_from;
use crate::{detect, hash, image};
use komga_core::sort_locale::compare_natural;
use std::cell::RefCell;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

const PAGES: usize = 150;

#[derive(Default, Clone, Copy)]
struct Stats {
    reads: usize,
    /// reads issued right after the file position changed — each one is a fresh range
    /// request on a network mount; sequential reads are served from the mount's readahead
    scattered: usize,
    /// real position changes (no-op `stream_position` calls excluded)
    seeks: usize,
    bytes: usize,
}

impl Stats {
    fn ops(&self) -> usize {
        self.reads + self.seeks
    }
}

struct AccountedFile {
    inner: std::fs::File,
    pos: u64,
    last_read_end: u64,
    stats: Rc<RefCell<Stats>>,
}

impl Read for AccountedFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        let mut s = self.stats.borrow_mut();
        s.reads += 1;
        s.bytes += n;
        if self.pos != self.last_read_end {
            s.scattered += 1;
        }
        self.pos += n as u64;
        self.last_read_end = self.pos;
        Ok(n)
    }
}

impl Seek for AccountedFile {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new = self.inner.seek(pos)?;
        if new != self.pos {
            self.stats.borrow_mut().seeks += 1;
        }
        self.pos = new;
        Ok(new)
    }
}

fn accounted(path: &Path, stats: &Rc<RefCell<Stats>>) -> AccountedFile {
    AccountedFile {
        inner: std::fs::File::open(path).unwrap(),
        pos: 0,
        last_read_end: 0,
        stats: stats.clone(),
    }
}

fn noise_jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut img = ::image::RgbImage::new(w, h);
    let mut state = 0x9e3779b97f4a7c15u64;
    for px in img.pixels_mut() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let v = (state >> 33) as u8;
        *px = ::image::Rgb([v, v, v]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    ::image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, ::image::ImageFormat::Jpeg)
        .unwrap();
    out.into_inner()
}

fn fixture_cbz() -> PathBuf {
    let dir = std::env::temp_dir().join("kmrs-net-mount-sim");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let book = dir.join("book.cbz");
    let jpeg = noise_jpeg(1200, 900);
    let file = std::fs::File::create(&book).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    // stored keeps fixture building cheap; deflate would only push the old path's
    // read-call counts higher
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for i in 0..PAGES {
        writer.start_file(format!("p{i:03}.jpg"), options).unwrap();
        std::io::Write::write_all(&mut writer, &jpeg).unwrap();
    }
    writer.finish().unwrap();
    book
}

fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> usize {
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader.read(&mut buf[filled..]).unwrap();
        if n == 0 {
            break;
        }
        filled += n;
    }
    filled
}

/// Pre-fix `Hasher.computeHash`: 8 KiB read buffer
fn hash_old(path: &Path, stats: &Rc<RefCell<Stats>>) -> String {
    let mut file = accounted(path, stats);
    let mut hasher = xxhash_rust::xxh3::Xxh3::with_seed(0);
    let mut buffer = [0u8; 8192];
    loop {
        let len = file.read(&mut buffer).unwrap();
        if len == 0 {
            break;
        }
        hasher.update(&buffer[..len]);
    }
    format!("{:032x}", hasher.digest128())
}

/// Post-fix: 1 MiB buffer, mirrors `hash::compute_hash`
fn hash_new(path: &Path, stats: &Rc<RefCell<Stats>>) -> String {
    let mut file = accounted(path, stats);
    let mut hasher = xxhash_rust::xxh3::Xxh3::with_seed(0);
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let len = file.read(&mut buffer).unwrap();
        if len == 0 {
            break;
        }
        hasher.update(&buffer[..len]);
    }
    format!("{:032x}", hasher.digest128())
}

/// Pre-fix `ZipExtractor.getEntries`: every image entry read in full for its dimensions
fn analyze_old(path: &Path, stats: &Rc<RefCell<Stats>>) -> Vec<(String, Option<(u32, u32)>)> {
    let file = accounted(path, stats);
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entries = vec![];
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let file_size = entry.size() as usize;
        let mut head = vec![0u8; 65536.min(file_size)];
        let n = read_full(&mut entry, &mut head);
        head.truncate(n);
        let media_type = detect::detect_media_type(&head);
        let dimension = if detect::is_image(&media_type) {
            let mut bytes = head.clone();
            if file_size > head.len() && entry.read_to_end(&mut bytes).is_err() {
                bytes = head.clone();
            }
            image::get_dimension(&bytes)
        } else {
            None
        };
        entries.push((name, dimension));
    }
    entries.sort_by(|a, b| compare_natural(&a.0, &b.0));
    entries
}

/// Post-fix: the real `zip_entries_from`, head-first dimensions
fn analyze_new(path: &Path, stats: &Rc<RefCell<Stats>>) -> Vec<(String, Option<(i32, i32)>)> {
    let file = accounted(path, stats);
    let mut archive = zip::ZipArchive::new(file).unwrap();
    zip_entries_from(&mut archive, true)
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.dimension))
        .collect()
}

/// The cost of merely opening the archive (thumbnail/ComicInfo/barcode/page-hash tasks each
/// pay this per invocation)
fn open_only(path: &Path, stats: &Rc<RefCell<Stats>>) {
    let file = accounted(path, stats);
    let archive = zip::ZipArchive::new(file).unwrap();
    assert_eq!(archive.len(), PAGES);
}

/// Poster-style access: open the archive and read a single entry out
fn poster(path: &Path, stats: &Rc<RefCell<Stats>>) {
    let file = accounted(path, stats);
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name("p000.jpg").unwrap();
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf).unwrap();
    assert_eq!(image::get_dimension(&buf), Some((1200, 900)));
}

fn report(label: &str, stats: &Stats, wall: Duration) {
    let mib = stats.bytes as f64 / (1 << 20) as f64;
    println!(
        "{label:<26} reads={:>6} (scattered={:>5}) seeks={:>4} pulled={:>7.1} MiB wall={:>6.2}s | modeled: 2ms/op={:>6.1}s 50ms/op={:>7.1}s",
        stats.reads,
        stats.scattered,
        stats.seeks,
        mib,
        wall.as_secs_f64(),
        stats.ops() as f64 * 0.002,
        stats.ops() as f64 * 0.05,
    );
}

fn run(label: &str, book: &Path, f: impl FnOnce(&Path, &Rc<RefCell<Stats>>)) -> (Stats, Duration) {
    let stats = Rc::new(RefCell::new(Stats::default()));
    let t = Instant::now();
    f(book, &stats);
    let wall = t.elapsed();
    let stats = *stats.borrow();
    report(label, &stats, wall);
    (stats, wall)
}

#[test]
#[ignore = "heavy IO simulation, run explicitly"]
fn net_mount_before_after() {
    let book = fixture_cbz();
    let book_mib = std::fs::metadata(&book).unwrap().len() as f64 / (1 << 20) as f64;
    println!(
        "fixture: {} ({book_mib:.1} MiB, {PAGES} pages)\n",
        book.display()
    );

    // the real production entry points on the local disk, as a sanity check
    let t = Instant::now();
    let media = crate::analyzer::Analyzer::new(3, 300, 15, None)
        .analyze(&book, true)
        .media;
    println!(
        "real Analyzer::analyze: {:.2}s ({} pages, first page {}x{})",
        t.elapsed().as_secs_f64(),
        media.page_count,
        media.pages[0].width.unwrap_or(0),
        media.pages[0].height.unwrap_or(0),
    );
    let t = Instant::now();
    let real_hash = hash::compute_hash(&book).unwrap();
    println!(
        "real hash::compute_hash: {:.2}s\n",
        t.elapsed().as_secs_f64()
    );

    let (hash_old_stats, _) = run("hash OLD (8 KiB)", &book, |p, s| {
        assert_eq!(hash_old(p, s), real_hash);
    });
    let (hash_new_stats, _) = run("hash NEW (1 MiB)", &book, |p, s| {
        assert_eq!(hash_new(p, s), real_hash);
    });
    println!();

    let (analyze_old_stats, _) = run("analyze OLD (full reads)", &book, |p, s| {
        let entries = analyze_old(p, s);
        assert_eq!(entries.len(), PAGES);
        assert_eq!(entries[0].1, Some((1200, 900)));
    });
    let (analyze_new_stats, _) = run("analyze NEW (head-first)", &book, |p, s| {
        let entries = analyze_new(p, s);
        assert_eq!(entries.len(), PAGES);
        assert_eq!(entries[0].1, Some((1200, 900)));
    });
    println!();

    let (open_stats, _) = run("archive open only", &book, open_only);
    run("poster (open + 1 entry)", &book, poster);
    println!();

    // default pipeline opens per book: media-type detect + analyze + thumbnail + ComicInfo
    // + up to 6 barcode page tries (hashPages would add 6 more)
    let opens = 10;
    println!(
        "default pipeline opens/book: {opens} x {} scattered reads = {} scattered | 2ms/op={:.1}s 50ms/op={:.1}s\n",
        open_stats.scattered,
        opens * open_stats.scattered,
        opens as f64 * open_stats.scattered as f64 * 0.002,
        opens as f64 * open_stats.scattered as f64 * 0.05,
    );

    let pipeline = |a: &Stats, h: &Stats| Stats {
        reads: a.reads + h.reads,
        scattered: a.scattered + h.scattered,
        seeks: a.seeks + h.seeks,
        bytes: a.bytes + h.bytes,
    };
    println!("per-book pipeline (analyze + hash), modeled:");
    for (label, stats) in [
        ("OLD", pipeline(&analyze_old_stats, &hash_old_stats)),
        ("NEW", pipeline(&analyze_new_stats, &hash_new_stats)),
    ] {
        println!(
            "  {label}: {} ops ({} scattered), {:.1} MiB pulled | 2ms/op={:.1}s 50ms/op={:.1}s",
            stats.ops(),
            stats.scattered,
            stats.bytes as f64 / (1 << 20) as f64,
            stats.ops() as f64 * 0.002,
            stats.ops() as f64 * 0.05,
        );
    }
}
