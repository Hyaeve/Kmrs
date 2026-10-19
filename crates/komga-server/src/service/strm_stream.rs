//! HTTP Range transport for STRM content. Remote archives stay in memory and page
//! extraction fetches only the blocks needed by the ZIP reader.

use komga_db::dao::strm_source::StrmSource;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use reqwest::{StatusCode, Url};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;
use zip::ZipArchive;

const RANGE_BLOCK_BYTES: u64 = 256 * 1024;
const RANGE_BLOCKS_PER_ARCHIVE: usize = 16;
const OPEN_ARCHIVES: usize = 8;
const ARCHIVE_IDLE_SECS: u64 = 300;

type ZipArchives = VecDeque<(String, Instant, ZipArchive<RemoteRangeReader>)>;

#[derive(Debug, thiserror::Error)]
pub enum StrmStreamError {
    #[error("remote server does not support byte ranges")]
    RangeUnsupported,
    #[error("invalid remote range response: {0}")]
    InvalidRange(String),
    #[error("remote range request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("remote range request returned HTTP {0}")]
    Status(StatusCode),
    #[error("remote archive is invalid: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("remote archive entry failed: {0}")]
    Media(String),
    #[error("remote content length is unavailable")]
    MissingLength,
    #[error("remote range transport lock is poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone)]
pub struct RangeProbe {
    pub resolved_url: Url,
    pub content_length: u64,
    pub range_supported: bool,
}

struct RangeBlockCache {
    blocks: HashMap<u64, Vec<u8>>,
    order: VecDeque<u64>,
}

impl RangeBlockCache {
    fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, index: u64) -> Option<Vec<u8>> {
        let bytes = self.blocks.get(&index)?.clone();
        self.order.retain(|existing| *existing != index);
        self.order.push_back(index);
        Some(bytes)
    }

    fn insert(&mut self, index: u64, bytes: Vec<u8>) {
        if !self.blocks.contains_key(&index) {
            self.order.push_back(index);
        }
        self.blocks.insert(index, bytes);
        while self.blocks.len() > RANGE_BLOCKS_PER_ARCHIVE {
            if let Some(oldest) = self.order.pop_front() {
                self.blocks.remove(&oldest);
            }
        }
    }
}

pub struct RemoteRangeReader {
    url: Url,
    length: u64,
    pos: u64,
    cache: RangeBlockCache,
}

impl RemoteRangeReader {
    fn new(url: Url, length: u64) -> Self {
        Self {
            url,
            length,
            pos: 0,
            cache: RangeBlockCache::new(),
        }
    }

    fn load_block(&mut self, index: u64) -> Result<Vec<u8>, StrmStreamError> {
        if let Some(bytes) = self.cache.get(index) {
            return Ok(bytes);
        }
        let start = index.saturating_mul(RANGE_BLOCK_BYTES);
        if start >= self.length {
            return Ok(Vec::new());
        }
        let end = (start + RANGE_BLOCK_BYTES - 1).min(self.length - 1);
        let response = blocking_client()
            .get(self.url.clone())
            .header(RANGE, format!("bytes={start}-{end}"))
            .header(ACCEPT_ENCODING, "identity")
            .send()?;
        if response.status() == StatusCode::OK {
            return Err(StrmStreamError::RangeUnsupported);
        }
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(StrmStreamError::Status(response.status()));
        }
        let content_range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| StrmStreamError::InvalidRange("missing Content-Range".into()))?;
        let (actual_start, actual_end, total) = parse_content_range(content_range)
            .ok_or_else(|| StrmStreamError::InvalidRange(content_range.to_string()))?;
        if actual_start != start || actual_end != end || total != self.length {
            return Err(StrmStreamError::InvalidRange(format!(
                "expected {start}-{end}/{}, got {content_range}",
                self.length
            )));
        }
        let bytes = response.bytes()?.to_vec();
        if bytes.len() as u64 != end - start + 1 {
            return Err(StrmStreamError::InvalidRange(format!(
                "expected {} bytes, got {}",
                end - start + 1,
                bytes.len()
            )));
        }
        self.cache.insert(index, bytes.clone());
        Ok(bytes)
    }
}

impl Read for RemoteRangeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() || self.pos >= self.length {
            return Ok(0);
        }
        let block_index = self.pos / RANGE_BLOCK_BYTES;
        let block = self
            .load_block(block_index)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let offset = (self.pos % RANGE_BLOCK_BYTES) as usize;
        if offset >= block.len() {
            return Ok(0);
        }
        let available = block.len() - offset;
        let remaining = usize::try_from(self.length - self.pos).unwrap_or(usize::MAX);
        let count = buf.len().min(available).min(remaining);
        buf[..count].copy_from_slice(&block[offset..offset + count]);
        self.pos += count as u64;
        Ok(count)
    }
}

impl Seek for RemoteRangeReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let next = match position {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(delta) => i128::from(self.pos) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(self.length) + i128::from(delta),
        };
        if next < 0 || next > i128::from(self.length) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "remote range seek out of bounds",
            ));
        }
        self.pos = u64::try_from(next).unwrap();
        Ok(self.pos)
    }
}

fn blocking_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(format!("kmrs/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("valid blocking HTTP client")
    })
}

pub async fn probe_remote(url: Url) -> Result<RangeProbe, StrmStreamError> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(format!("kmrs/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("valid HTTP client")
    });
    let response = client
        .get(url)
        .header(RANGE, "bytes=0-0")
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .await?;
    let status = response.status();
    let resolved_url = response.url().clone();
    if status == StatusCode::PARTIAL_CONTENT {
        let content_range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| StrmStreamError::InvalidRange("missing Content-Range".into()))?;
        let (_, _, total) = parse_content_range(content_range)
            .ok_or_else(|| StrmStreamError::InvalidRange(content_range.to_string()))?;
        return Ok(RangeProbe {
            resolved_url,
            content_length: total,
            range_supported: true,
        });
    }
    if !status.is_success() {
        return Err(StrmStreamError::Status(status));
    }
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(StrmStreamError::MissingLength)?;
    Ok(RangeProbe {
        resolved_url,
        content_length,
        range_supported: false,
    })
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}

fn with_zip_archive<T>(
    source: &StrmSource,
    read: impl FnOnce(&mut ZipArchive<RemoteRangeReader>) -> Result<T, StrmStreamError>,
) -> Result<T, StrmStreamError> {
    let key = format!(
        "{}|{}|{}",
        source.book_id, source.source_url, source.file_size
    );
    let mut archives = archive_cache()
        .lock()
        .map_err(|_| StrmStreamError::LockPoisoned)?;
    archives.retain(|(_, last_used, _)| last_used.elapsed().as_secs() < ARCHIVE_IDLE_SECS);
    if let Some(index) = archives.iter().position(|(existing, last_used, _)| {
        existing == &key && last_used.elapsed().as_secs() < ARCHIVE_IDLE_SECS
    }) {
        let cached = archives.remove(index).expect("archive index exists");
        let (key, _, archive) = cached;
        archives.push_back((key, Instant::now(), archive));
        let archive = &mut archives.back_mut().expect("archive was pushed").2;
        return read(archive);
    }

    let reader = remote_reader(source)?;
    let mut archive = ZipArchive::new(reader)?;
    let value = read(&mut archive)?;
    while archives.len() >= OPEN_ARCHIVES {
        archives.pop_front();
    }
    archives.push_back((key, Instant::now(), archive));
    Ok(value)
}

pub fn remote_reader(source: &StrmSource) -> Result<RemoteRangeReader, StrmStreamError> {
    let url = Url::parse(&source.source_url)
        .map_err(|error| StrmStreamError::InvalidRange(error.to_string()))?;
    let length = u64::try_from(source.file_size)
        .ok()
        .filter(|length| *length > 0)
        .ok_or(StrmStreamError::MissingLength)?;
    Ok(RemoteRangeReader::new(url, length))
}

pub fn read_zip_entry(source: &StrmSource, entry_name: &str) -> Result<Vec<u8>, StrmStreamError> {
    with_zip_archive(source, |archive| {
        komga_media::zip::get_entry_bytes_from(archive, entry_name)
            .map_err(|error| StrmStreamError::Media(error.to_string()))
    })
}

pub fn analyze_zip(
    source: &StrmSource,
    analyzer: &komga_media::analyzer::Analyzer,
) -> Result<komga_media::analyzer::Analysis, StrmStreamError> {
    with_zip_archive(source, |archive| Ok(analyzer.analyze_zip_stream(archive)))
}

pub fn analyze_epub(
    source: &StrmSource,
    analyzer: &komga_media::analyzer::Analyzer,
) -> Result<komga_media::analyzer::Analysis, StrmStreamError> {
    Ok(analyzer.analyze_epub_stream(remote_reader(source)?))
}

pub fn analyze_pdf(
    source: &StrmSource,
    analyzer: &komga_media::analyzer::Analyzer,
) -> Result<komga_media::analyzer::Analysis, StrmStreamError> {
    Ok(analyzer.analyze_pdf_stream(remote_reader(source)?))
}

pub fn generate_epub_thumbnail(
    source: &StrmSource,
    analyzer: &komga_media::analyzer::Analyzer,
) -> Result<komga_media::analyzer::GeneratedThumbnail, StrmStreamError> {
    analyzer
        .generate_thumbnail_from_epub_reader(remote_reader(source)?)
        .map_err(|error| StrmStreamError::Media(error.to_string()))
}

pub fn render_pdf_page(
    source: &StrmSource,
    page_number: usize,
) -> Result<Vec<u8>, StrmStreamError> {
    komga_media::pdf::get_page_content_as_image_from_reader(remote_reader(source)?, page_number)
        .map_err(|error| StrmStreamError::Media(error.to_string()))
}

pub fn extract_pdf_page(
    source: &StrmSource,
    page_number: usize,
) -> Result<Vec<u8>, StrmStreamError> {
    komga_media::pdf::get_page_content_as_pdf_from_reader(remote_reader(source)?, page_number)
        .map_err(|error| StrmStreamError::Media(error.to_string()))
}

fn archive_cache() -> &'static Mutex<ZipArchives> {
    static ARCHIVES: OnceLock<Mutex<ZipArchives>> = OnceLock::new();
    ARCHIVES.get_or_init(|| Mutex::new(VecDeque::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_content_range() {
        assert_eq!(parse_content_range("bytes 0-0/12345"), Some((0, 0, 12345)));
        assert_eq!(
            parse_content_range("bytes 1024-2047/4096"),
            Some((1024, 2047, 4096))
        );
        assert_eq!(parse_content_range("invalid"), None);
    }
}
