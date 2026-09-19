//! `FileSystemScanner.kt` port: library filesystem walking with komga's series/book
//! detection and sidecar (artwork/metadata) recognition rules.
//!
//! Traversal follows symlinks (like `Files.walkFileTree` with `FOLLOW_LINKS`): directories are
//! visited through links, files behind a link are not. A hand-rolled recursion is used instead of
//! `walkdir` because the Kotlin logic needs both pre-visit and post-visit directory hooks.

use komga_core::model::book::Book;
use komga_core::model::series::Series;
use komga_core::model::sidecar::{SidecarSource, SidecarType};
use komga_core::time_codec;
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// `DirectoryNotFoundException`'s error code
pub const ERR_DIRECTORY_NOT_FOUND: &str = "ERR_1016";

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// `DirectoryNotFoundException("Folder is not accessible: $root", "ERR_1016")`
    #[error("Folder is not accessible: {0}")]
    DirectoryNotFound(String),
}

impl ScanError {
    pub fn code(&self) -> &'static str {
        ERR_DIRECTORY_NOT_FOUND
    }
}

pub type Result<T> = std::result::Result<T, ScanError>;

const CBX_EXTENSIONS: [&str; 4] = ["cbz", "zip", "cbr", "rar"];
const ARTWORK_EXTENSIONS: [&str; 6] = ["png", "jpeg", "jpg", "tbn", "webp", "gif"];
const ARTWORK_SERIES_FILES: [&str; 5] = ["cover", "default", "folder", "poster", "series"];
const MYLAR_SERIES_JSON: &str = "series.json";

/// `ScanResult.kt`
#[derive(Debug, Default)]
pub struct ScanResult {
    pub series: Vec<(Series, Vec<Book>)>,
    pub sidecars: Vec<Sidecar>,
}

/// `Sidecar.kt`: a sidecar file matched by a consumer.
#[derive(Debug, Clone, PartialEq)]
pub struct Sidecar {
    pub url: String,
    pub parent_url: String,
    pub last_modified_time: OffsetDateTime,
    pub type_: SidecarType,
    pub source: SidecarSource,
}

/// `SidecarBookConsumer`. On the Java side only LocalArtwork implements this interface today.
pub struct SidecarBookConsumer {
    type_: SidecarType,
    prefilter: Vec<Regex>,
}

impl SidecarBookConsumer {
    /// `LocalArtworkProvider`: ARTWORK, one `.*(-\d+)?\.{ext}` (case-insensitive) per extension
    pub fn local_artwork() -> Self {
        Self {
            type_: SidecarType::Artwork,
            prefilter: ARTWORK_EXTENSIONS
                .iter()
                .map(|ext| Regex::new(&format!("(?i).*(-[0-9]+)?\\.{ext}$")).unwrap())
                .collect(),
        }
    }

    pub fn type_(&self) -> SidecarType {
        self.type_
    }

    fn matches_prefilter(&self, name: &str) -> bool {
        self.prefilter.iter().any(|r| r.is_match(name))
    }

    /// `{basename}(-\d+)?` (case-insensitive, full match) against the sidecar's base name
    fn is_match(&self, basename: &str, sidecar: &str) -> bool {
        let pattern = format!("(?i)^{}(-[0-9]+)?$", regex::escape(basename));
        Regex::new(&pattern)
            .expect("escaped pattern is always valid")
            .is_match(&commons_get_base_name(sidecar))
    }
}

/// `SidecarSeriesConsumer`. On the Java side: LocalArtwork (ARTWORK) and Mylar (METADATA).
pub struct SidecarSeriesConsumer {
    type_: SidecarType,
    filenames: Vec<String>,
}

impl SidecarSeriesConsumer {
    /// `{cover,default,folder,poster,series} × {png,jpeg,jpg,tbn,webp,gif}`
    pub fn local_artwork() -> Self {
        Self {
            type_: SidecarType::Artwork,
            filenames: ARTWORK_SERIES_FILES
                .iter()
                .flat_map(|f| ARTWORK_EXTENSIONS.iter().map(move |e| format!("{f}.{e}")))
                .collect(),
        }
    }

    pub fn mylar() -> Self {
        Self {
            type_: SidecarType::Metadata,
            filenames: vec![MYLAR_SERIES_JSON.to_string()],
        }
    }

    pub fn type_(&self) -> SidecarType {
        self.type_
    }

    /// file names are matched exactly, ignoring case
    fn matches(&self, name: &str) -> bool {
        self.filenames.iter().any(|f| f.eq_ignore_ascii_case(name))
    }
}

/// Options for `Scanner::scan_root_folder` (the per-library scan settings).
#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub force_directory_modified_time: bool,
    pub oneshots_dir: Option<String>,
    pub scan_cbx: bool,
    pub scan_pdf: bool,
    pub scan_epub: bool,
    pub directory_exclusions: BTreeSet<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            force_directory_modified_time: false,
            oneshots_dir: None,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            directory_exclusions: BTreeSet::new(),
        }
    }
}

/// `FileSystemScanner`
pub struct Scanner {
    book_consumers: Vec<SidecarBookConsumer>,
    series_consumers: Vec<SidecarSeriesConsumer>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    /// The consumer set matches the current Java beans: LocalArtwork (book + series) and Mylar.
    pub fn new() -> Self {
        Self {
            book_consumers: vec![SidecarBookConsumer::local_artwork()],
            series_consumers: vec![
                SidecarSeriesConsumer::local_artwork(),
                SidecarSeriesConsumer::mylar(),
            ],
        }
    }

    pub fn scan_root_folder(&self, root: &Path, options: &ScanOptions) -> Result<ScanResult> {
        let mut extensions: Vec<String> = Vec::with_capacity(6);
        if options.scan_cbx {
            extensions.extend(CBX_EXTENSIONS.map(str::to_string));
        }
        if options.scan_pdf {
            extensions.push("pdf".to_string());
        }
        if options.scan_epub {
            extensions.push("epub".to_string());
        }
        tracing::info!("Scanning folder: {}", root.display());

        if !(root.is_dir() && std::fs::read_dir(root).is_ok()) {
            return Err(ScanError::DirectoryNotFound(
                root.to_string_lossy().into_owned(),
            ));
        }

        let mut ctx = WalkContext {
            extensions,
            force_directory_modified_time: options.force_directory_modified_time,
            oneshots_dir: options
                .oneshots_dir
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .cloned(),
            exclusions: options
                .directory_exclusions
                .iter()
                .map(|e| e.to_lowercase())
                .collect(),
            book_consumers: &self.book_consumers,
            series_consumers: &self.series_consumers,
            result: ScanResult::default(),
        };
        let mut ancestors = Vec::new();
        walk_dir(&mut ctx, root, &mut ancestors);

        tracing::info!(
            "Scanned {} series, {} books, and {} sidecars",
            ctx.result.series.len(),
            ctx.result
                .series
                .iter()
                .map(|(_, b)| b.len())
                .sum::<usize>(),
            ctx.result.sidecars.len()
        );
        Ok(ctx.result)
    }

    pub fn scan_file(&self, path: &Path) -> Option<Book> {
        if !path.exists() {
            return None;
        }
        let meta = std::fs::metadata(path).ok()?;
        Some(path_to_book(path, &meta))
    }

    /// Sidecars sitting next to a single book file. Unlike the main scan, `parentUrl` is the
    /// containing directory's URL (Kotlin behavior, kept for fidelity).
    pub fn scan_book_sidecars(&self, path: &Path) -> Vec<Sidecar> {
        let book_base_name = kotlin_name_without_extension(&entry_name(path));
        let Some(parent) = path.parent() else {
            return vec![];
        };
        let read_dir = match std::fs::read_dir(parent) {
            Ok(rd) => rd,
            Err(_) => {
                tracing::warn!("Could not access: {}", parent.display());
                return vec![];
            }
        };
        read_dir
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                self.book_consumers
                    .iter()
                    .any(|c| c.matches_prefilter(&name))
            })
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                self.book_consumers
                    .iter()
                    .find(|c| c.is_match(&book_base_name, &name))
                    .and_then(|consumer| {
                        let meta = std::fs::metadata(entry.path()).ok()?;
                        Some(Sidecar {
                            url: path_to_url(&entry.path()),
                            parent_url: path_to_url(parent),
                            last_modified_time: updated_time(&meta),
                            type_: consumer.type_(),
                            source: SidecarSource::Book,
                        })
                    })
            })
            .collect()
    }
}

struct TempSidecar {
    name: String,
    url: String,
    last_modified_time: OffsetDateTime,
}

struct WalkContext<'a> {
    extensions: Vec<String>,
    force_directory_modified_time: bool,
    oneshots_dir: Option<String>,
    exclusions: Vec<String>,
    book_consumers: &'a [SidecarBookConsumer],
    series_consumers: &'a [SidecarSeriesConsumer],
    result: ScanResult,
}

fn walk_dir(ctx: &mut WalkContext, dir: &Path, ancestors: &mut Vec<PathBuf>) {
    // preVisitDirectory: dot-prefixed or excluded directories are skipped as a whole subtree
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if name.starts_with('.')
        || ctx
            .exclusions
            .iter()
            .any(|ex| dir.to_string_lossy().to_lowercase().contains(ex))
    {
        return;
    }
    // Java's walkFileTree reports loops to visitFileFailed; warn and skip the subtree likewise
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if ancestors.contains(&canonical) {
        tracing::warn!("File system loop detected: {}", dir.display());
        return;
    }

    let dir_meta = match std::fs::metadata(dir) {
        Ok(m) => m,
        Err(_) => {
            tracing::warn!("Could not access: {}", dir.display());
            return;
        }
    };
    ancestors.push(canonical);

    let mut temp_series = Series {
        id: String::new(),
        name: if name.trim().is_empty() {
            dir.to_string_lossy().into_owned()
        } else {
            name.to_string()
        },
        url: path_to_url(dir),
        file_last_modified: updated_time(&dir_meta),
        library_id: String::new(),
        book_count: 0,
        deleted_date: None,
        oneshot: false,
        created_date: time_codec::now_utc(),
        last_modified_date: time_codec::now_utc(),
    };

    let mut books: Vec<Book> = vec![];
    let mut series_sidecars: Vec<Sidecar> = vec![];
    let mut sidecar_candidates: Vec<TempSidecar> = vec![];

    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
            tracing::warn!("Could not access: {}", dir.display());
            ancestors.pop();
            return;
        }
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                tracing::warn!("Could not access an entry of: {}", dir.display());
                continue;
            }
        };
        let path = entry.path();
        let link_meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                tracing::warn!("Could not access: {}", path.display());
                continue;
            }
        };

        if link_meta.file_type().is_symlink() {
            if path.is_dir() {
                walk_dir(ctx, &path, ancestors);
            }
            continue;
        }
        if link_meta.is_dir() {
            walk_dir(ctx, &path, ancestors);
            continue;
        }

        // visitFile: anything that is not a symlink and not a directory
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                tracing::warn!("Could not access: {}", path.display());
                continue;
            }
        };

        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ctx.extensions.contains(&ext) && !file_name.starts_with('.') {
            books.push(path_to_book(&path, &meta));
        }

        if let Some(consumer) = ctx.series_consumers.iter().find(|c| c.matches(&file_name)) {
            series_sidecars.push(Sidecar {
                url: path_to_url(&path),
                parent_url: path_to_url(dir),
                last_modified_time: updated_time(&meta),
                type_: consumer.type_(),
                source: SidecarSource::Series,
            });
        }

        if ctx
            .book_consumers
            .iter()
            .any(|c| c.matches_prefilter(&file_name))
        {
            sidecar_candidates.push(TempSidecar {
                name: file_name,
                url: path_to_url(&path),
                last_modified_time: updated_time(&meta),
            });
        }
    }

    // postVisitDirectory
    if !books.is_empty() {
        let oneshot = ctx
            .oneshots_dir
            .as_ref()
            .is_some_and(|o| contains_ignore_case(&dir.to_string_lossy(), o));
        if oneshot {
            for book in &books {
                let series = Series {
                    name: book.name.clone(),
                    url: book.url.clone(),
                    file_last_modified: book.file_last_modified,
                    oneshot: true,
                    ..temp_series.clone()
                };
                let mut oneshot_book = book.clone();
                oneshot_book.oneshot = true;
                ctx.result.series.push((series, vec![oneshot_book]));
            }
        } else {
            if ctx.force_directory_modified_time {
                if let Some(max_book_time) = books.iter().map(|b| b.file_last_modified).max() {
                    temp_series.file_last_modified =
                        temp_series.file_last_modified.max(max_book_time);
                }
            }
            ctx.result.series.push((temp_series, books.clone()));
            // series sidecars are only collected when the directory has books
            ctx.result.sidecars.append(&mut series_sidecars);
        }

        // match book sidecars against the directory's books; each candidate is consumed at most once
        for book in &books {
            let matched: Vec<(TempSidecar, SidecarType)> = sidecar_candidates
                .iter()
                .filter_map(|candidate| {
                    ctx.book_consumers
                        .iter()
                        .find(|c| c.is_match(&book.name, &candidate.name))
                        .map(|c| {
                            (
                                TempSidecar {
                                    name: candidate.name.clone(),
                                    url: candidate.url.clone(),
                                    last_modified_time: candidate.last_modified_time,
                                },
                                c.type_(),
                            )
                        })
                })
                .collect();
            if !matched.is_empty() {
                sidecar_candidates.retain(|c| !matched.iter().any(|(m, _)| m.url == c.url));
                for (candidate, type_) in matched {
                    ctx.result.sidecars.push(Sidecar {
                        url: candidate.url,
                        parent_url: book.url.clone(),
                        last_modified_time: candidate.last_modified_time,
                        type_,
                        source: SidecarSource::Book,
                    });
                }
            }
        }
    }

    ancestors.pop();
}

fn path_to_book(path: &Path, meta: &std::fs::Metadata) -> Book {
    Book {
        id: String::new(),
        name: kotlin_name_without_extension(&entry_name(path)),
        url: path_to_url(path),
        file_last_modified: updated_time(meta),
        series_id: String::new(),
        library_id: String::new(),
        file_size: meta.len() as i64,
        number: 0,
        file_hash: String::new(),
        file_hash_koreader: String::new(),
        deleted_date: None,
        oneshot: false,
        created_date: time_codec::now_utc(),
        last_modified_date: time_codec::now_utc(),
    }
}

fn entry_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `kotlin.io.path.nameWithoutExtension`: name up to the last `.` (or the whole name);
/// unlike Rust's `file_stem`, a leading dot counts as an extension separator (".cbz" → "").
fn kotlin_name_without_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(idx) => name[..idx].to_string(),
        None => name.to_string(),
    }
}

/// commons-io `FilenameUtils.getBaseName`: strips the last extension unless the name starts
/// with the only dot (dotfiles have no extension).
fn commons_get_base_name(name: &str) -> String {
    match name.rfind('.') {
        Some(0) | None => name.to_string(),
        Some(idx) => name[..idx].to_string(),
    }
}

/// `BasicFileAttributes.getUpdatedTime()`: max(creationTime, lastModifiedTime).
/// Birth time is unavailable on some platforms/filesystems; mtime is the fallback there.
fn updated_time(meta: &std::fs::Metadata) -> OffsetDateTime {
    let modified = meta.modified().ok();
    let created = meta.created().ok();
    let latest = match (created, modified) {
        (Some(c), Some(m)) => c.max(m),
        (Some(c), None) => c,
        (None, Some(m)) => m,
        (None, None) => std::time::SystemTime::UNIX_EPOCH,
    };
    OffsetDateTime::from(latest)
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn is_url_path_char(b: u8) -> bool {
    matches!(b,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' |
        b'-' | b'.' | b'_' | b'~' |
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' |
        b':' | b'@' | b'/')
}

/// `Path.toUri().toURL()`: `file:` + percent-encoded absolute path (RFC 2396 path characters
/// pass through, everything else is percent-encoded with uppercase hex), plus a trailing `/`
/// for real directories. Java checks `Files.isDirectory(path, NOFOLLOW_LINKS)`, so a symlink
/// to a directory gets no trailing slash.
pub fn path_to_url(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
    };
    let s = abs.to_string_lossy();
    let s = s.trim_end_matches('/');
    let mut out = String::from("file:");
    for &b in s.as_bytes() {
        if is_url_path_char(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    let is_real_dir = std::fs::symlink_metadata(&abs)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    if is_real_dir {
        out.push('/');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::dto::url_to_file_path;
    use std::collections::{BTreeMap, HashSet};
    use std::fs::{create_dir_all, File};
    use std::io::Write;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        File::create(&path).unwrap();
        path
    }

    /// tempfile's own directory is dot-prefixed (`.tmpXXXX`), which the scanner correctly
    /// treats as hidden; scans must be rooted at a non-hidden child
    fn test_root() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        create_dir_all(&root).unwrap();
        (tmp, root)
    }

    fn series_by_name(result: &ScanResult) -> BTreeMap<&str, &(Series, Vec<Book>)> {
        result
            .series
            .iter()
            .map(|entry| (entry.0.name.as_str(), entry))
            .collect()
    }

    fn sidecar_names(result: &ScanResult, source: SidecarSource) -> Vec<&str> {
        result
            .sidecars
            .iter()
            .filter(|s| s.source == source)
            .map(|s| s.url.rsplit('/').next().unwrap())
            .collect()
    }

    #[test]
    fn basic_scan_nested_series() {
        let (_tmp, root) = test_root();
        let a = root.join("seriesA");
        let b = root.join("seriesB");
        let c = root.join("sub").join("seriesC");
        let hidden = root.join(".hidden");
        let empty = root.join("empty");
        for d in [&a, &b, &c, &hidden, &empty] {
            create_dir_all(d).unwrap();
        }
        touch(&a, "v01.cbz");
        touch(&a, "v02.cbr");
        touch(&a, "cover.jpg");
        touch(&b, "v01.pdf");
        touch(&c, "v01.epub");
        touch(&hidden, "v01.cbz");

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();

        let by_name = series_by_name(&result);
        assert_eq!(by_name.len(), 3);
        assert_eq!(by_name["seriesA"].1.len(), 2);
        assert_eq!(by_name["seriesB"].1.len(), 1);
        assert!(
            !by_name.contains_key("sub"),
            "intermediate dirs are not series"
        );
        assert!(by_name["seriesA"].0.url.ends_with('/'));
        assert!(!by_name["seriesA"].1[0].url.ends_with('/'));
        assert!(!by_name["seriesA"].0.oneshot);
        assert_eq!(by_name["seriesA"].1[0].file_size, 0);

        // .hidden and empty produced nothing
        assert!(!by_name.contains_key(".hidden"));
        assert!(!by_name.contains_key("empty"));

        // seriesA's cover.jpg is an ARTWORK series sidecar pointing at the series dir
        assert_eq!(sidecar_names(&result, SidecarSource::Series), ["cover.jpg"]);
        let cover = &result.sidecars[0];
        assert_eq!(cover.type_, SidecarType::Artwork);
        assert_eq!(cover.parent_url, by_name["seriesA"].0.url);
    }

    #[test]
    fn scan_extension_switches() {
        let (_tmp, root) = test_root();
        create_dir_all(root.join("s")).unwrap();
        touch(&root.join("s"), "a.cbz");
        touch(&root.join("s"), "b.pdf");
        touch(&root.join("s"), "c.epub");
        touch(&root.join("s"), "d.txt");

        let all = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();
        assert_eq!(all.series[0].1.len(), 3);

        let no_cbx = Scanner::new()
            .scan_root_folder(
                &root,
                &ScanOptions {
                    scan_cbx: false,
                    ..ScanOptions::default()
                },
            )
            .unwrap();
        let names: HashSet<&str> = no_cbx.series[0].1.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["b", "c"].into_iter().collect());

        let only_pdf = Scanner::new()
            .scan_root_folder(
                &root,
                &ScanOptions {
                    scan_cbx: false,
                    scan_epub: false,
                    ..ScanOptions::default()
                },
            )
            .unwrap();
        assert_eq!(only_pdf.series[0].1.len(), 1);
        assert_eq!(only_pdf.series[0].1[0].name, "b");
    }

    #[test]
    fn dot_prefixed_dirs_and_files_are_skipped() {
        let (_tmp, root) = test_root();
        create_dir_all(root.join("seriesD")).unwrap();
        touch(&root, ".v01.cbz");
        touch(&root.join("seriesD"), ".v02.cbz");

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();
        assert!(result.series.is_empty());
    }

    #[test]
    fn directory_exclusions_match_case_insensitive_substring() {
        let (_tmp, root) = test_root();
        create_dir_all(root.join("pornography")).unwrap();
        create_dir_all(root.join("safe")).unwrap();
        touch(&root.join("pornography"), "v01.cbz");
        touch(&root.join("safe"), "v01.cbz");

        let exclusions = ["PORN".to_string()].into_iter().collect();
        let result = Scanner::new()
            .scan_root_folder(
                &root,
                &ScanOptions {
                    directory_exclusions: exclusions,
                    ..ScanOptions::default()
                },
            )
            .unwrap();
        let by_name = series_by_name(&result);
        assert_eq!(by_name.len(), 1);
        assert!(by_name.contains_key("safe"));
    }

    #[test]
    fn oneshot_directory_rule() {
        let (_tmp, root) = test_root();
        let oneshots = root.join("oneshots");
        create_dir_all(&oneshots).unwrap();
        create_dir_all(root.join("normal")).unwrap();
        touch(&oneshots, "a.cbz");
        touch(&oneshots, "b.cbz");
        touch(&oneshots, "cover.jpg");
        touch(&root.join("normal"), "v01.cbz");

        let result = Scanner::new()
            .scan_root_folder(
                &root,
                &ScanOptions {
                    oneshots_dir: Some("oneshots".to_string()),
                    ..ScanOptions::default()
                },
            )
            .unwrap();

        let by_name = series_by_name(&result);
        assert_eq!(by_name.len(), 3);
        for name in ["a", "b"] {
            let (series, books) = by_name[name];
            assert!(series.oneshot);
            assert_eq!(series.url, books[0].url);
            assert!(books[0].oneshot);
        }
        assert!(!by_name["normal"].0.oneshot);
        // no sidecars are collected from oneshot directories
        assert!(result.sidecars.is_empty());
    }

    #[test]
    fn force_directory_modified_time() {
        let (_tmp, root) = test_root();
        let dir = root.join("seriesF");
        create_dir_all(&dir).unwrap();
        let book = touch(&dir, "v01.cbz");
        std::thread::sleep(std::time::Duration::from_millis(50));
        // appending changes the book's mtime but not the directory's
        File::options()
            .append(true)
            .open(&book)
            .unwrap()
            .write_all(b"x")
            .unwrap();

        let scanner = Scanner::new();
        let plain = scanner
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();
        let forced = scanner
            .scan_root_folder(
                &root,
                &ScanOptions {
                    force_directory_modified_time: true,
                    ..ScanOptions::default()
                },
            )
            .unwrap();

        let book_mtime = plain.series[0].1[0].file_last_modified;
        assert!(plain.series[0].0.file_last_modified < book_mtime);
        assert_eq!(forced.series[0].0.file_last_modified, book_mtime);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_is_followed() {
        let (_tmp, root) = test_root();
        let real = root.join("real");
        create_dir_all(&real).unwrap();
        touch(&real, "v01.cbz");
        std::os::unix::fs::symlink(&real, root.join("link")).unwrap();

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();
        let by_name = series_by_name(&result);
        assert_eq!(by_name.len(), 2);
        assert!(by_name.contains_key("real"));
        assert!(by_name.contains_key("link"));
        // the linked series URL goes through the link path, like Java's walkFileTree;
        // a symlink to a directory gets no trailing slash (NOFOLLOW_LINKS, see the dedicated test)
        assert!(by_name["link"].0.url.ends_with("/link"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_file_is_skipped() {
        let (_tmp, root) = test_root();
        let dir = root.join("s2");
        create_dir_all(&dir).unwrap();
        let book = touch(&dir, "v01.cbz");
        std::os::unix::fs::symlink(&book, dir.join("v02.cbz")).unwrap();

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();
        assert_eq!(result.series.len(), 1);
        assert_eq!(result.series[0].1.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cycle_is_detected() {
        let (_tmp, root) = test_root();
        let dir = root.join("cyc");
        create_dir_all(&dir).unwrap();
        touch(&dir, "v01.cbz");
        std::os::unix::fs::symlink(&dir, dir.join("loop")).unwrap();

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();
        assert_eq!(result.series.len(), 1);
        assert_eq!(result.series[0].1.len(), 1);
    }

    #[test]
    fn series_sidecar_types_and_bookless_dirs() {
        let (_tmp, root) = test_root();
        let dir = root.join("s3");
        create_dir_all(&dir).unwrap();
        create_dir_all(root.join("empty2")).unwrap();
        touch(&dir, "v01.cbz");
        touch(&dir, "cover.jpg");
        touch(&dir, "series.json");
        touch(&root.join("empty2"), "cover.jpg");

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();

        let by_type: BTreeMap<&str, SidecarType> = result
            .sidecars
            .iter()
            .filter(|s| s.source == SidecarSource::Series)
            .map(|s| (s.url.rsplit('/').next().unwrap(), s.type_))
            .collect();
        assert_eq!(by_type.len(), 2);
        assert_eq!(by_type["cover.jpg"], SidecarType::Artwork);
        assert_eq!(by_type["series.json"], SidecarType::Metadata);
    }

    #[test]
    fn book_sidecar_matching() {
        let (_tmp, root) = test_root();
        let dir = root.join("s4");
        create_dir_all(&dir).unwrap();
        touch(&dir, "v01.cbz");
        touch(&dir, "v01.jpg");
        touch(&dir, "v01-2.png");
        touch(&dir, "v02.jpg");
        touch(&dir, "cover.jpg");

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();

        let mut book_sidecars: Vec<&str> = sidecar_names(&result, SidecarSource::Book);
        book_sidecars.sort_unstable();
        assert_eq!(book_sidecars, ["v01-2.png", "v01.jpg"]);
        let book_url = &result.series[0].1[0].url;
        for sidecar in result
            .sidecars
            .iter()
            .filter(|s| s.source == SidecarSource::Book)
        {
            assert_eq!(sidecar.type_, SidecarType::Artwork);
            assert_eq!(&sidecar.parent_url, book_url);
        }
        // cover.jpg is a series sidecar, v02.jpg matches nothing
        assert_eq!(sidecar_names(&result, SidecarSource::Series), ["cover.jpg"]);
    }

    #[test]
    fn a_sidecar_is_consumed_by_at_most_one_book() {
        let (_tmp, root) = test_root();
        let dir = root.join("s5");
        create_dir_all(&dir).unwrap();
        touch(&dir, "v01.cbz");
        touch(&dir, "v01-2.cbz");
        touch(&dir, "v01-2.jpg");

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();
        // both books' patterns match "v01-2.jpg" (v01(-\d+)? and v01\-2(-\d+)?),
        // but the file is consumed exactly once
        assert_eq!(sidecar_names(&result, SidecarSource::Book), ["v01-2.jpg"]);
    }

    #[test]
    fn url_roundtrip_with_spaces_and_non_ascii() {
        let (_tmp, root) = test_root();
        let dir = root.join("my comics 系列");
        create_dir_all(&dir).unwrap();
        touch(&dir, "v01.cbz");

        let result = Scanner::new()
            .scan_root_folder(&root, &ScanOptions::default())
            .unwrap();
        let series = &result.series[0].0;
        assert!(series.url.starts_with("file:/"));
        assert!(series.url.contains("my%20comics%20%E7%B3%BB%E5%88%97/"));
        assert_eq!(url_to_file_path(&series.url), dir.to_string_lossy());
    }

    #[test]
    fn root_not_accessible_returns_err_1016() {
        let err = Scanner::new()
            .scan_root_folder(
                Path::new("/nonexistent-komga-rs-test"),
                &ScanOptions::default(),
            )
            .unwrap_err();
        assert_eq!(err.code(), ERR_DIRECTORY_NOT_FOUND);
        assert_eq!(
            err.to_string(),
            "Folder is not accessible: /nonexistent-komga-rs-test"
        );
    }

    #[test]
    fn scan_file_and_scan_book_sidecars() {
        let (_tmp, root) = test_root();
        let dir = root.join("s6");
        create_dir_all(&dir).unwrap();
        let book_path = touch(&dir, "v01.cbz");
        touch(&dir, "v01.jpg");
        touch(&dir, "v02.jpg");

        let scanner = Scanner::new();
        let book = scanner.scan_file(&book_path).unwrap();
        assert_eq!(book.name, "v01");
        assert_eq!(book.file_size, 0);
        assert!(!book.url.ends_with('/'));
        assert!(scanner.scan_file(&dir.join("missing.cbz")).is_none());

        let sidecars = scanner.scan_book_sidecars(&book_path);
        assert_eq!(sidecars.len(), 1);
        assert_eq!(sidecars[0].type_, SidecarType::Artwork);
        assert_eq!(sidecars[0].source, SidecarSource::Book);
        // scanBookSidecars uses the containing directory as parentUrl (Kotlin fidelity)
        assert_eq!(sidecars[0].parent_url, path_to_url(&dir));
        assert!(sidecars[0].parent_url.ends_with('/'));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_dir_gets_no_trailing_slash() {
        let (_tmp, root) = test_root();
        let dir = root.join("s7");
        create_dir_all(&dir).unwrap();
        let link = root.join("slink");
        std::os::unix::fs::symlink(&dir, &link).unwrap();

        assert!(path_to_url(&dir).ends_with('/'));
        // Java's toUri checks isDirectory with NOFOLLOW_LINKS
        assert!(!path_to_url(&link).ends_with('/'));
    }

    #[test]
    fn name_helpers() {
        assert_eq!(kotlin_name_without_extension("foo.bar.cbz"), "foo.bar");
        assert_eq!(kotlin_name_without_extension(".cbz"), "");
        assert_eq!(kotlin_name_without_extension("foo"), "foo");
        assert_eq!(kotlin_name_without_extension("foo."), "foo");

        assert_eq!(commons_get_base_name(".foo"), ".foo");
        assert_eq!(commons_get_base_name(".foo.bar"), ".foo");
        assert_eq!(commons_get_base_name("foo."), "foo");
        assert_eq!(commons_get_base_name("foo.bar.jpg"), "foo.bar");
    }
}
