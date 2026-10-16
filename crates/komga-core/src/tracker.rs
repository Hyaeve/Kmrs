//! Tracker progress primitives for the komf-backed reading-status sync (kmrs
//! enhancement, no Java equivalent). Two concerns live here, both pure and
//! side-effect free:
//!
//! - book-name classification: which progress number (volume or chapter) a book
//!   filename carries. The regex sets are ported from komf-rs's `BookNameParser`
//!   (which serves metadata matching), but the composition differs on purpose:
//!   komf tries volume patterns first because volume numbers drive provider
//!   matching, while reading progress treats the chapter as the primary unit —
//!   a name carrying both (`Vol.03 ch.12`) resolves to the chapter.
//!   Unrecognized names fall back to the scanner's `number_sort`, and a
//!   series with no volume/chapter signal at all behaves as chapter-mode.
//! - push decisions: given the platform-side state (totals, current progress,
//!   status) and the read set, decide what to push. The completed check
//!   compares against the new (about-to-be-pushed) value, so finishing the
//!   final chapter marks completed on that very event.

use std::sync::OnceLock;

/// How a series' read progress maps onto tracker fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackMode {
    /// Decide from the series' book names (komf recognition chain, chapter-first).
    #[default]
    Auto,
    /// Push `last_read_chapter`.
    Chapter,
    /// Push `last_read_volume`.
    Volume,
}

impl TrackMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Chapter => "chapter",
            Self::Volume => "volume",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "chapter" => Some(Self::Chapter),
            "volume" => Some(Self::Volume),
            _ => None,
        }
    }
}

/// Inclusive number range parsed from a book name (`Vol.1-2`, `001-100话`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BookRange {
    pub start: f64,
    pub end: f64,
}

impl BookRange {
    fn single(value: f64) -> Self {
        Self {
            start: value,
            end: value,
        }
    }

    fn with_end(mut self, end: f64) -> Self {
        self.end = end;
        self
    }

    /// The furthest point the range covers: finishing `Vol.1-2` means volume 2
    /// has been read.
    pub fn end_value(&self) -> f64 {
        self.end
    }
}

/// Fullwidth ASCII (U+FF01..=U+FF5E) and the ideographic space fold to their
/// ASCII equivalents, so `第５巻` and `第5巻` classify identically. Same rule as
/// komf-rs's `replace_fullwidth_chars`.
fn replace_fullwidth_chars(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            '！'..='～' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '　' => ' ',
            other => other,
        })
        .collect()
}

/// Volume/chapter/book-number recognition ported from komf-rs
/// (`crates/komf-core/src/util/book_name_parser.rs`); the regex sets are kept
/// in sync with that file.
pub struct BookNameParser;

static VOLUME_REGEXES: OnceLock<Vec<regex::Regex>> = OnceLock::new();
static CHAPTER_REGEXES: OnceLock<Vec<regex::Regex>> = OnceLock::new();
static BOOK_NUMBER_REGEXES: OnceLock<Vec<regex::Regex>> = OnceLock::new();

fn volume_regexes() -> &'static [regex::Regex] {
    VOLUME_REGEXES.get_or_init(|| {
        vec![
            regex::Regex::new(r"(?i)(?:^|,?\s)\(?volume\s(?<volumeStart>[0-9]+)(,?\s?[0-9]+,)+(?<volumeEnd>\s?[0-9]+)\)?").unwrap(),
            // vol./vols./volume prefix, space optional: scanner output is often
            // "Vol.01" without a space
            regex::Regex::new(r"(?i)(?:^|,?\s)\(?([vtT]|vols\.\s?|vol\.\s?|volume\s?)(?<volumeStart>[0-9]+([.x#][0-9]+)?)(?<volumeEnd>-[0-9]+([.x#][0-9]+)?)?\)?").unwrap(),
            regex::Regex::new(r".*第\s*(?<volumeStart>\d+)\s*-?\s*(?<volumeEnd>\d+)?\s*[巻卷册冊集]").unwrap(),
            // "5巻"/"12卷"/"1-3冊" suffix without 「第」; ordered after the 第-form
            // so the first match wins there
            regex::Regex::new(r"(?:^|.*[^\d第-])(?<volumeStart>\d+(?:\.\d+)?)\s*-?\s*(?<volumeEnd>\d+(?:\.\d+)?)?\s*[巻卷册冊集]\s*$").unwrap(),
            // "巻5"/"巻1-2" prefix form
            regex::Regex::new(r"(?:^|.*[^\d])(?:[巻卷册冊集])\s*(?<volumeStart>\d+(?:\.\d+)?)\s*-?\s*(?<volumeEnd>\d+(?:\.\d+)?)?\s*$").unwrap(),
            regex::Regex::new(r".*年(?:[0-9]+月)?(?:[0-9]+日)?(?<volumeStart>\d+)-?(?<volumeEnd>\d+)?号").unwrap(),
        ]
    })
}

fn chapter_regexes() -> &'static [regex::Regex] {
    CHAPTER_REGEXES.get_or_init(|| {
        vec![
            // c/ch./chap./chapter/ep., space optional ("Chap.001", "ch.12")
            regex::Regex::new(r"(?i)(?:^|\s?)(c|ch\.\s?|chap\.\s?|chapter\s?|ep\.\s?)(?<start>[0-9]+([.x#][0-9]+)?)(?<end>-[0-9]+([.x#][0-9]+)?)?").unwrap(),
            regex::Regex::new(r".*第\s*(?<start>\d+(?:\.\d+)?)\s*-?\s*(?<end>\d+(?:\.\d+)?)?\s*[話话章节回]").unwrap(),
            regex::Regex::new(r"(?:^|.*[^\d第-])(?<start>\d+(?:\.\d+)?)\s*-?\s*(?<end>\d+(?:\.\d+)?)?\s*[話话章节回]\s*$").unwrap(),
        ]
    })
}

fn book_number_regexes() -> &'static [regex::Regex] {
    BOOK_NUMBER_REGEXES.get_or_init(|| {
        vec![
            regex::Regex::new(r"(?i)(?:\s|#|no\.)(?<start>[0-9]+[AB]?([.x#][0-9]+)?)(?<end>-[0-9]+([.x#][0-9]+)?)?(?:\s\(.*\)\s*)*$").unwrap(),
            regex::Regex::new(r"Issue (?<start>[0-9]+[AB]?([.x#][0-9]+)?)(?<end>-[0-9]+([.x#][0-9]+)?)?").unwrap(),
            regex::Regex::new(r"Volume (?<start>[0-9]+[AB]?([.x#][0-9]+)?)(?<end>-[0-9]+([.x#][0-9]+)?)?").unwrap(),
        ]
    })
}

fn parse_number(raw: &str) -> Option<f64> {
    raw.replace(['x', '#'], ".").parse().ok()
}

fn captures_to_range(captures: &regex::Captures<'_>) -> Option<BookRange> {
    let start = captures
        .name("start")
        .or_else(|| captures.name("volumeStart"))
        .and_then(|m| parse_number(m.as_str()));
    let end = captures
        .name("end")
        .or_else(|| captures.name("volumeEnd"))
        .and_then(|m| parse_number(&m.as_str().replace('-', "")));
    match (start, end) {
        (Some(start), Some(end)) => Some(BookRange::single(start).with_end(end)),
        (Some(start), None) => Some(BookRange::single(start)),
        _ => None,
    }
}

fn first_match(regexes: &'static [regex::Regex], name: &str) -> Option<BookRange> {
    let name = replace_fullwidth_chars(name);
    regexes
        .iter()
        .find_map(|regex| regex.captures(&name).as_ref().and_then(captures_to_range))
}

/// First regex with any match wins, then its last capture — matching komf's
/// `get_book_number_from` (`findAll(name).lastOrNull()` per regex in order).
fn last_match(regexes: &'static [regex::Regex], name: &str) -> Option<BookRange> {
    let name = replace_fullwidth_chars(name);
    regexes.iter().find_map(|regex| {
        regex
            .captures_iter(&name)
            .last()
            .as_ref()
            .and_then(captures_to_range)
    })
}

impl BookNameParser {
    pub fn get_volumes(name: &str) -> Option<BookRange> {
        first_match(volume_regexes(), name)
    }

    /// Last match wins, matching komf's `findAll(name).lastOrNull()`.
    pub fn get_chapters(name: &str) -> Option<BookRange> {
        let name = replace_fullwidth_chars(name);
        for regex in chapter_regexes() {
            let last = regex.captures_iter(&name).last();
            if let Some(range) = last.as_ref().and_then(captures_to_range) {
                return Some(range);
            }
        }
        None
    }

    pub fn get_book_number(name: &str) -> Option<BookRange> {
        last_match(book_number_regexes(), name)
    }
}

/// Which progress signal one book's name carries. When volume and chapter
/// patterns both match, the chapter wins (the chapter sequence is the
/// continuous reading unit); the volume component is kept for volume-mode
/// aggregation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BookTrackClass {
    Both { chapter: f64, volume: f64 },
    Chapter(f64),
    Volume(f64),
    Unknown,
}

pub fn classify_book(name: &str) -> BookTrackClass {
    let volume = BookNameParser::get_volumes(name);
    let chapter = BookNameParser::get_chapters(name);
    match (volume, chapter) {
        (Some(v), Some(c)) => BookTrackClass::Both {
            chapter: c.end_value(),
            volume: v.end_value(),
        },
        (None, Some(c)) => BookTrackClass::Chapter(c.end_value()),
        (Some(v), None) => BookTrackClass::Volume(v.end_value()),
        (None, None) => BookNameParser::get_book_number(name)
            .map(|b| BookTrackClass::Chapter(b.end_value()))
            .unwrap_or(BookTrackClass::Unknown),
    }
}

impl BookTrackClass {
    /// Contribution to a chapter-mode push value: the chapter signal, the
    /// volume number when no chapter signal exists (volume-as-chapter
    /// fallback), or the scanner's number as last resort.
    fn chapter_value(&self, number_sort: f64) -> f64 {
        match *self {
            Self::Both { chapter, .. } => chapter,
            Self::Chapter(c) => c,
            Self::Volume(v) => v,
            Self::Unknown => number_sort,
        }
    }

    /// Contribution to a volume-mode push value: volume signal, chapter floor
    /// as fallback (chapter-as-volume fallback), number_sort floor
    /// otherwise. No chapter offset is applied to volumes (offsets only shift
    /// chapter numbers).
    fn volume_value(&self, number_sort: f64) -> f64 {
        match *self {
            Self::Both { volume, .. } => volume,
            Self::Volume(v) => v,
            Self::Chapter(c) => c.floor(),
            Self::Unknown => number_sort.floor(),
        }
    }
}

/// One book's contribution to the series aggregate.
pub struct BookTrackInput<'a> {
    pub name: &'a str,
    pub number_sort: f32,
    pub read: bool,
}

/// Resolve `Auto` against the whole series: any chapter signal (including
/// names carrying both) makes it a chapter series, else any volume signal
/// makes it a volume series, else the chapter fallback for names carrying
/// no recognizable signal.
pub fn resolve_series_mode(inputs: &[BookTrackInput<'_>]) -> TrackMode {
    let classes = inputs
        .iter()
        .map(|b| classify_book(b.name))
        .collect::<Vec<_>>();
    if classes
        .iter()
        .any(|c| matches!(c, BookTrackClass::Chapter(_) | BookTrackClass::Both { .. }))
    {
        TrackMode::Chapter
    } else if classes
        .iter()
        .any(|c| matches!(c, BookTrackClass::Volume(_)))
    {
        TrackMode::Volume
    } else {
        TrackMode::Chapter
    }
}

/// The effective mode of one binding: `Auto` resolves against the series.
pub fn effective_mode(mode: TrackMode, inputs: &[BookTrackInput<'_>]) -> TrackMode {
    match mode {
        TrackMode::Auto => resolve_series_mode(inputs),
        explicit => explicit,
    }
}

/// Aggregated read progress of a series: both signals are computed, the
/// caller pushes only the one matching the effective mode.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProgressValue {
    pub chapter: Option<f64>,
    pub volume: Option<i32>,
}

pub fn aggregate(inputs: &[BookTrackInput<'_>]) -> ProgressValue {
    let mut value = ProgressValue::default();
    for book in inputs.iter().filter(|b| b.read) {
        let class = classify_book(book.name);
        let chapter = class.chapter_value(f64::from(book.number_sort));
        let volume = class.volume_value(f64::from(book.number_sort));
        value.chapter = Some(value.chapter.map_or(chapter, |c: f64| c.max(chapter)));
        value.volume = Some(
            value
                .volume
                .map_or(volume as i32, |v: i32| v.max(volume as i32)),
        );
    }
    value
}

/// Unified reading status (7 states); wire format matches komf's lowercase
/// strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackStatus {
    Reading,
    Planning,
    Completed,
    Paused,
    Dropped,
    Rereading,
}

impl TrackStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reading => "reading",
            Self::Planning => "planning",
            Self::Completed => "completed",
            Self::Paused => "paused",
            Self::Dropped => "dropped",
            Self::Rereading => "rereading",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "reading" => Some(Self::Reading),
            "planning" => Some(Self::Planning),
            "completed" => Some(Self::Completed),
            "paused" => Some(Self::Paused),
            "dropped" => Some(Self::Dropped),
            "rereading" => Some(Self::Rereading),
            _ => None,
        }
    }
}

/// komf `/api/tracker/{provider}/state` response (komf's `TrackState` schema,
/// camelCase).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RemoteState {
    pub status: Option<TrackStatus>,
    pub last_read_chapter: Option<f32>,
    pub last_read_volume: Option<i32>,
    pub total_chapters: Option<i32>,
    pub total_volumes: Option<i32>,
    pub start_read_date: Option<String>,
    pub finish_read_date: Option<String>,
}

/// The push payload: komf's `TrackUpdate` (camelCase on the wire).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackUpdate {
    pub status: Option<TrackStatus>,
    pub last_read_chapter: Option<f32>,
    pub last_read_volume: Option<i32>,
    pub start_read_date: Option<String>,
    pub finish_read_date: Option<String>,
}

/// Decide what to push for one binding, or `None` when there is nothing worth
/// pushing (no read book, or local progress does not advance the remote one).
/// `today` is the local date used for start/finish dates. `mode` must already
/// be resolved (`effective_mode`) — `Auto` panics.
pub fn decide(
    mode: TrackMode,
    chapter_offset: i32,
    remote: &RemoteState,
    inputs: &[BookTrackInput<'_>],
    today: time::Date,
) -> Option<TrackUpdate> {
    let progress = aggregate(inputs);
    let today = today.to_string();

    // only push when local progress strictly advances the remote value
    let (new_chapter, new_volume) = match mode {
        TrackMode::Chapter => {
            let new = progress.chapter?;
            let adjusted = apply_chapter_offset(new, chapter_offset, remote.total_chapters);
            if adjusted <= f64::from(remote.last_read_chapter.unwrap_or(0.0)) {
                return None;
            }
            (Some(adjusted as f32), None)
        }
        TrackMode::Volume => {
            let new = progress.volume?;
            if Some(new) <= remote.last_read_volume {
                return None;
            }
            (None, Some(new))
        }
        TrackMode::Auto => unreachable!("callers resolve auto first"),
    };

    let completed = match mode {
        TrackMode::Chapter => new_chapter
            .zip(remote.total_chapters)
            .is_some_and(|(c, total)| f64::from(c).floor() == f64::from(total)),
        TrackMode::Volume => new_volume.is_some() && new_volume == remote.total_volumes,
        TrackMode::Auto => false,
    };

    let mut update = TrackUpdate {
        last_read_chapter: new_chapter,
        last_read_volume: new_volume,
        ..TrackUpdate::default()
    };
    if completed {
        if remote.finish_read_date.is_none() {
            update.finish_read_date = Some(today.clone());
        }
        update.status = Some(TrackStatus::Completed);
    } else if remote.status != Some(TrackStatus::Reading)
        && remote.status != Some(TrackStatus::Rereading)
    {
        // not currently reading: opening a book starts the entry, and
        // re-reading a completed entry switches to rereading
        if remote.start_read_date.is_none()
            && matches!(remote.status, None | Some(TrackStatus::Planning))
        {
            update.start_read_date = Some(today);
        }
        update.status = Some(if remote.status == Some(TrackStatus::Completed) {
            TrackStatus::Rereading
        } else {
            TrackStatus::Reading
        });
    }
    Some(update)
}

/// `chapter + offset` clamped to [0, total].
fn apply_chapter_offset(chapter: f64, offset: i32, total: Option<i32>) -> f64 {
    let adjusted = (chapter + f64::from(offset)).max(0.0);
    match total {
        Some(total) => adjusted.min(f64::from(total)),
        None => adjusted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(name: &str, number_sort: f32, read: bool) -> BookTrackInput<'_> {
        BookTrackInput {
            name,
            number_sort,
            read,
        }
    }

    fn day() -> time::Date {
        time::Date::from_calendar_date(2026, time::Month::October, 8).unwrap()
    }

    // komf-rs BookNameParser parity: the ported regex sets must keep matching
    // komf's own test vectors (crates/komf-core/src/util/book_name_parser.rs).
    #[test]
    fn parser_matches_komf_test_vectors() {
        assert_eq!(
            BookNameParser::get_volumes("My Series v1"),
            Some(BookRange {
                start: 1.0,
                end: 1.0
            })
        );
        assert_eq!(
            BookNameParser::get_volumes("My Series Vol. 1-2"),
            Some(BookRange {
                start: 1.0,
                end: 2.0
            })
        );
        assert_eq!(
            BookNameParser::get_volumes("他国日记 Vol.01"),
            Some(BookRange {
                start: 1.0,
                end: 1.0
            })
        );
        assert_eq!(
            BookNameParser::get_volumes("僕のヒーローアカデミア 第５巻"),
            Some(BookRange {
                start: 5.0,
                end: 5.0
            })
        );
        assert_eq!(
            BookNameParser::get_volumes("标题 1-3冊"),
            Some(BookRange {
                start: 1.0,
                end: 3.0
            })
        );
        assert_eq!(
            BookNameParser::get_chapters("Some Series c10"),
            Some(BookRange {
                start: 10.0,
                end: 10.0
            })
        );
        assert_eq!(
            BookNameParser::get_chapters("Some Series Chapter 10-12"),
            Some(BookRange {
                start: 10.0,
                end: 12.0
            })
        );
        assert_eq!(
            BookNameParser::get_chapters("[王牌御史] 001-100话"),
            Some(BookRange {
                start: 1.0,
                end: 100.0
            })
        );
        assert_eq!(BookNameParser::get_chapters("12話5"), None);
        assert_eq!(BookNameParser::get_volumes("12巻5"), None);
        assert_eq!(
            BookNameParser::get_book_number("Some Series #5"),
            Some(BookRange {
                start: 5.0,
                end: 5.0
            })
        );
    }

    // generic book numbers also take the last match (komf's findAll().lastOrNull())
    #[test]
    fn book_number_takes_the_last_match() {
        assert_eq!(
            BookNameParser::get_book_number("Issue 5 Issue 6"),
            Some(BookRange {
                start: 6.0,
                end: 6.0
            })
        );
        assert_eq!(
            BookNameParser::get_book_number("Some Series #5 #6"),
            Some(BookRange {
                start: 6.0,
                end: 6.0
            })
        );
    }

    // The agreed classification contract: both-in-name resolves to the chapter,
    // ranges take the end, the generic book number counts as a chapter.
    #[test]
    fn classification_contract() {
        assert_eq!(classify_book("Series Vol.03"), BookTrackClass::Volume(3.0));
        assert_eq!(
            classify_book("Series ch.012"),
            BookTrackClass::Chapter(12.0)
        );
        assert_eq!(
            classify_book("Series Vol.03 ch.12"),
            BookTrackClass::Both {
                chapter: 12.0,
                volume: 3.0
            }
        );
        assert_eq!(
            classify_book("第3卷 第12话"),
            BookTrackClass::Both {
                chapter: 12.0,
                volume: 3.0
            }
        );
        assert_eq!(classify_book("Series #5"), BookTrackClass::Chapter(5.0));
        assert_eq!(
            classify_book("Vol.1-2 ch.10-12"),
            BookTrackClass::Both {
                chapter: 12.0,
                volume: 2.0
            }
        );
        assert_eq!(classify_book("Some Book"), BookTrackClass::Unknown);
    }

    #[test]
    fn auto_mode_resolves_chapter_first_then_volume_then_chapter_fallback() {
        // pure tankobon naming → volume mode
        let volumes = vec![
            book("Series Vol.01", 1.0, false),
            book("Series Vol.02", 2.0, false),
        ];
        assert_eq!(resolve_series_mode(&volumes), TrackMode::Volume);
        // any chapter signal (incl. mixed both) → chapter mode
        let mixed = vec![
            book("Series Vol.03", 3.0, false),
            book("Series ch.012", 12.0, false),
        ];
        assert_eq!(resolve_series_mode(&mixed), TrackMode::Chapter);
        let both = vec![book("Series Vol.03 ch.12", 12.0, false)];
        assert_eq!(resolve_series_mode(&both), TrackMode::Chapter);
        // nothing recognizable → chapter fallback
        let unknown = vec![book("Some Book", 1.0, false)];
        assert_eq!(resolve_series_mode(&unknown), TrackMode::Chapter);
        assert_eq!(
            effective_mode(TrackMode::Auto, &unknown),
            TrackMode::Chapter
        );
        assert_eq!(effective_mode(TrackMode::Volume, &mixed), TrackMode::Volume);
    }

    #[test]
    fn aggregate_uses_fallbacks_per_book() {
        let books = vec![
            book("Series Vol.03", 3.0, true),  // chapter fallback → 3
            book("Series ch.12", 12.0, true),  // chapter 12, volume floor 12
            book("Some Book", 4.5, true),      // unknown → number_sort
            book("Series ch.99", 99.0, false), // unread contributes nothing
        ];
        let value = aggregate(&books);
        assert_eq!(value.chapter, Some(12.0));
        assert_eq!(value.volume, Some(12));
    }

    fn empty_remote() -> RemoteState {
        RemoteState::default()
    }

    #[test]
    fn no_push_when_nothing_read_or_no_advance() {
        let books = vec![book("Series ch.10", 10.0, false)];
        assert_eq!(
            decide(TrackMode::Chapter, 0, &empty_remote(), &books, day()),
            None
        );

        let read = vec![book("Series ch.10", 10.0, true)];
        let ahead = RemoteState {
            last_read_chapter: Some(10.0),
            ..empty_remote()
        };
        assert_eq!(decide(TrackMode::Chapter, 0, &ahead, &read, day()), None);
    }

    #[test]
    fn planning_entry_starts_reading_with_start_date() {
        let remote = RemoteState {
            status: Some(TrackStatus::Planning),
            total_chapters: Some(20),
            ..empty_remote()
        };
        let books = vec![book("Series ch.10", 10.0, true)];
        let update = decide(TrackMode::Chapter, 0, &remote, &books, day()).unwrap();
        assert_eq!(update.last_read_chapter, Some(10.0));
        assert_eq!(update.status, Some(TrackStatus::Reading));
        assert_eq!(update.start_read_date.as_deref(), Some("2026-10-08"));
        assert_eq!(update.finish_read_date, None);
    }

    #[test]
    fn finishing_last_marks_completed_immediately_with_finish_date() {
        // comparing the pre-push value would lag completion by one event; the new
        // value lands completion on this very event.
        let remote = RemoteState {
            status: Some(TrackStatus::Reading),
            last_read_chapter: Some(19.0),
            start_read_date: Some("2026-10-01".into()),
            total_chapters: Some(20),
            ..empty_remote()
        };
        let books = vec![book("Series ch.20", 20.0, true)];
        let update = decide(TrackMode::Chapter, 0, &remote, &books, day()).unwrap();
        assert_eq!(update.last_read_chapter, Some(20.0));
        assert_eq!(update.status, Some(TrackStatus::Completed));
        assert_eq!(update.finish_read_date.as_deref(), Some("2026-10-08"));
        // existing start date is kept (never overwritten)
        assert_eq!(update.start_read_date, None);
    }

    #[test]
    fn completed_entry_reopened_becomes_rereading() {
        let remote = RemoteState {
            status: Some(TrackStatus::Completed),
            last_read_chapter: Some(20.0),
            total_chapters: Some(30),
            start_read_date: Some("2026-09-01".into()),
            finish_read_date: Some("2026-09-30".into()),
            ..empty_remote()
        };
        let books = vec![book("Series ch.21", 21.0, true)];
        let update = decide(TrackMode::Chapter, 0, &remote, &books, day()).unwrap();
        assert_eq!(update.status, Some(TrackStatus::Rereading));
        // dates of the original run are left untouched
        assert_eq!(update.start_read_date, None);
        assert_eq!(update.finish_read_date, None);
    }

    #[test]
    fn unknown_totals_never_auto_complete() {
        let remote = RemoteState {
            status: Some(TrackStatus::Reading),
            last_read_chapter: Some(99.0),
            ..empty_remote()
        };
        let books = vec![book("Series ch.100", 100.0, true)];
        let update = decide(TrackMode::Chapter, 0, &remote, &books, day()).unwrap();
        assert_eq!(update.status, None);
        assert_eq!(update.finish_read_date, None);
    }

    #[test]
    fn chapter_offset_applies_and_clamps_to_total() {
        let remote = RemoteState {
            status: Some(TrackStatus::Reading),
            total_chapters: Some(100),
            ..empty_remote()
        };
        let books = vec![book("Series ch.10", 10.0, true)];
        let update = decide(TrackMode::Chapter, 5, &remote, &books, day()).unwrap();
        assert_eq!(update.last_read_chapter, Some(15.0));

        // overshoot clamps to the platform total instead of exceeding it
        let update = decide(TrackMode::Chapter, 500, &remote, &books, day()).unwrap();
        assert_eq!(update.last_read_chapter, Some(100.0));
        assert_eq!(update.status, Some(TrackStatus::Completed));

        // a negative offset floors at 0 and may then not advance the remote
        let remote = RemoteState {
            status: Some(TrackStatus::Reading),
            ..empty_remote()
        };
        let books = vec![book("Series ch.3", 3.0, true)];
        assert_eq!(decide(TrackMode::Chapter, -5, &remote, &books, day()), None);
    }

    #[test]
    fn volume_mode_ignores_offset_and_completes_against_total_volumes() {
        let remote = RemoteState {
            status: Some(TrackStatus::Reading),
            total_volumes: Some(3),
            ..empty_remote()
        };
        // chapter-named book falls back to floor(chapter) as volume
        let books = vec![book("Series ch.3", 3.0, true)];
        let update = decide(TrackMode::Volume, 0, &remote, &books, day()).unwrap();
        assert_eq!(update.last_read_volume, Some(3));
        assert_eq!(update.last_read_chapter, None);
        assert_eq!(update.status, Some(TrackStatus::Completed));

        // volume values never receive the chapter offset
        let books = vec![book("Series Vol.02", 2.0, true)];
        let update = decide(TrackMode::Volume, 100, &remote, &books, day()).unwrap();
        assert_eq!(update.last_read_volume, Some(2));
    }

    #[test]
    fn volume_mode_waits_for_a_volume_signal_in_volume_named_series() {
        let remote = RemoteState {
            status: Some(TrackStatus::Reading),
            total_volumes: Some(5),
            ..empty_remote()
        };
        let books = vec![book("Series Vol.03", 3.0, true)];
        let update = decide(TrackMode::Volume, 0, &remote, &books, day()).unwrap();
        assert_eq!(update.last_read_volume, Some(3));
        assert_eq!(update.status, None); // 3 != 5, still reading
    }
}
