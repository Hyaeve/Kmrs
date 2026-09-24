//! Configurable ICU collation for sort-heavy queries.
//!
//! - [`set_sort_locale`] must be called once at startup (before any collator is
//!   constructed); later calls are ignored (`OnceLock`).
//! - The default locale is `und` (UCA root), which reproduces the previous
//!   hard-coded `CollatorPreferences::default()` behavior, so the change is a
//!   no-op unless `KOMGA_SORT_LOCALE` (or `server.sort-locale`) is set.
//! - All comparisons end with a raw-string tie-break: ICU compares canonically
//!   equivalent strings (e.g. "á" vs "a\u{301}") as equal, and the raw string
//!   ordering keeps the result deterministic regardless of the input row order
//!   (important for `ORDER BY ... LIMIT` boundaries). The natural sort
//!   ([`compare_natural`]) also falls back to the raw strings when numeric or
//!   whitespace-normalized segments tie, so every comparison is a total order.

use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_locale::preferences::LocalePreferences;
use icu_locale::Locale;
use std::cmp::Ordering;
use std::str::FromStr;
use std::sync::OnceLock;

/// The two collators used by kmrs: PRIMARY (case/accent-insensitive matching)
/// and TERTIARY (case/accent-sensitive sorting), both built for the configured
/// sort locale.
pub struct SortCollators {
    primary: CollatorBorrowed<'static>,
    tertiary: CollatorBorrowed<'static>,
}

impl SortCollators {
    fn new(locale: &Locale) -> Self {
        let make = |strength: Strength| {
            let mut prefs = CollatorPreferences::default();
            prefs.locale_preferences = LocalePreferences::from(locale);
            let mut options = CollatorOptions::default();
            options.strength = Some(strength);
            Collator::try_new(prefs, options)
                .expect("ICU collator for the configured sort locale should construct")
        };
        Self {
            primary: make(Strength::Primary),
            tertiary: make(Strength::Tertiary),
        }
    }

    pub fn primary(&self) -> &CollatorBorrowed<'static> {
        &self.primary
    }

    pub fn tertiary(&self) -> &CollatorBorrowed<'static> {
        &self.tertiary
    }
}

static SORT_LOCALE_OVERRIDE: OnceLock<Option<Locale>> = OnceLock::new();
static COLLATORS: OnceLock<SortCollators> = OnceLock::new();

/// Configure the sort locale from a BCP47-ish string (e.g. `zh-CN`, `de_AT.UTF-8`).
/// Only the first call takes effect; the default is `und` (UCA root).
pub fn set_sort_locale(value: Option<String>) {
    SORT_LOCALE_OVERRIDE.get_or_init(|| {
        value.and_then(|v| {
            let locale = parse_locale(&v);
            if locale.is_none() {
                tracing::warn!(
                    "ignoring unparsable sort locale {v:?}; falling back to the UCA root (und)"
                );
            }
            locale
        })
    });
}

/// `zh_CN.UTF-8` -> `zh-CN` (charset suffix dropped, `_` normalized to `-`).
fn parse_locale(value: &str) -> Option<Locale> {
    let lang = value.split('.').next().unwrap_or(value).replace('_', "-");
    lang.parse().ok()
}

fn sort_collators() -> &'static SortCollators {
    COLLATORS.get_or_init(|| {
        let locale = SORT_LOCALE_OVERRIDE
            .get()
            .cloned()
            .flatten()
            .unwrap_or_else(|| Locale::from_str("und").expect("und is a valid locale"));
        SortCollators::new(&locale)
    })
}

/// The PRIMARY (case/accent-insensitive matching) collator for the configured
/// sort locale. Registered per SQLite connection as `COLLATION_UNICODE_1`.
pub fn primary_collator() -> &'static CollatorBorrowed<'static> {
    sort_collators().primary()
}

/// ICU TERTIARY comparison (sorting) with a raw-string tie-break for
/// canonical-equivalence ties. Text segments of [`compare_natural`] use this.
pub fn compare_tertiary(left: &str, right: &str) -> Ordering {
    sort_collators()
        .tertiary()
        .compare(left, right)
        .then_with(|| left.cmp(right))
}

// ---- ICU-based natural sort (numeric segments by value, text by ICU) ----
//
// Modeled on komga-rust's `compare_book_names`: the string is split into
// alternating text/numeric segments (numeric = runs of ASCII digits, a decimal
// point staying in text); numeric segments compare by value, text segments
// compare with the configured ICU tertiary collator (plus
// canonical-equivalence tie-break), and a numeric segment always sorts before
// text. Registered as the `COLLATION_UNICODE_3` SQLite collation and used for
// in-memory sorting, so "Page 2" sorts before "Page 10" while still following
// the configured sort locale for text.

enum Segment {
    Text(String),
    Number(String),
}

fn split_into_segments(value: &str) -> Vec<Segment> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return vec![Segment::Text(String::new())];
    }
    let mut segments = Vec::new();
    let mut chars = normalized.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_digit() {
            // Numeric segments are runs of ASCII digits only: a decimal point is
            // NOT part of a number ("1.5" vs "1.10" compares 5 < 10), matching
            // how Explorer/Finder and the grey-panther natural comparator sort
            // file names.
            let mut num_str = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    num_str.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            segments.push(Segment::Number(num_str));
        } else {
            let mut text = String::new();
            while let Some(&c) = chars.peek() {
                if !c.is_ascii_digit() {
                    text.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            segments.push(Segment::Text(text));
        }
    }
    merge_segments(segments)
}

/// Merges adjacent text segments. `split_into_segments` produces strictly
/// alternating, non-empty segments (the empty string returns before
/// segmenting), so no empty-text or empty-result fallback is needed.
fn merge_segments(segments: Vec<Segment>) -> Vec<Segment> {
    let mut result = Vec::new();
    for segment in segments {
        match segment {
            Segment::Text(text) => {
                if let Some(Segment::Text(prev)) = result.last_mut() {
                    prev.push_str(&text);
                } else {
                    result.push(Segment::Text(text));
                }
            }
            Segment::Number(num) => {
                result.push(Segment::Number(num));
            }
        }
    }
    result
}

/// Digit runs compare by value without integer parsing: leading zeros ignored,
/// then length, then lexicographic (no overflow).
fn compare_numeric_strings(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    let a = if a.is_empty() { "0" } else { a };
    let b = if b.is_empty() { "0" } else { b };
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn compare_segments(left: &[Segment], right: &[Segment]) -> Ordering {
    let mut left_iter = left.iter();
    let mut right_iter = right.iter();
    while let (Some(l), Some(r)) = (left_iter.next(), right_iter.next()) {
        let ordering = match (l, r) {
            (Segment::Number(nl), Segment::Number(nr)) => compare_numeric_strings(nl, nr),
            (Segment::Text(tl), Segment::Text(tr)) => compare_tertiary(tl, tr),
            (Segment::Number(_), Segment::Text(_)) => Ordering::Less,
            (Segment::Text(_), Segment::Number(_)) => Ordering::Greater,
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

/// ICU-based natural sort (numeric segments by value, text segments by the
/// configured ICU tertiary collator). This is the comparison registered as the
/// `COLLATION_UNICODE_3` SQLite collation.
///
/// A final raw-string tie-break on the original inputs makes this a total
/// order: numeric ties (`a01` vs `a1`) and whitespace-normalized ties
/// (`Page  2` vs `Page 2`) get a deterministic order, which SQLite's unstable
/// `ORDER BY` needs at pagination boundaries.
pub fn compare_natural(left: &str, right: &str) -> Ordering {
    compare_segments(&split_into_segments(left), &split_into_segments(right))
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collators_for(locale: &str) -> SortCollators {
        SortCollators::new(&Locale::from_str(locale).expect("test locale should parse"))
    }

    #[test]
    fn parse_locale_normalizes_charset_and_separator() {
        assert_eq!(
            parse_locale("zh_CN.UTF-8").map(|l| l.to_string()),
            Some("zh-CN".into())
        );
        assert_eq!(
            parse_locale("zh-Hans-CN").map(|l| l.to_string()),
            Some("zh-Hans-CN".into())
        );
        assert_eq!(
            parse_locale("de_AT").map(|l| l.to_string()),
            Some("de-AT".into())
        );
        assert_eq!(parse_locale("not a tag"), None);
    }

    #[test]
    fn default_collator_sorts_uca_tertiary() {
        // und tertiary: lowercase before uppercase on equal primary, base
        // letter before accented variant, letter groups ordered by primary.
        let collators = collators_for("und");
        let mut values = vec!["b", "é", "A", "a", "café", "cafe"];
        values.sort_by(|l, r| collators.tertiary().compare(l, r).then_with(|| l.cmp(r)));
        assert_eq!(values, vec!["a", "A", "b", "cafe", "café", "é"]);
    }

    #[test]
    fn tie_break_orders_canonical_equivalents_deterministically() {
        // "á" (U+00E1) and "a\u{301}" (a + combining acute) are canonically
        // equivalent and compare equal under ICU; the raw-string fallback
        // orders them deterministically regardless of the input row order.
        let collators = collators_for("und");
        let mut values = vec!["b", "a\u{301}", "á", "a"];
        values.sort_by(|l, r| collators.tertiary().compare(l, r).then_with(|| l.cmp(r)));
        assert_eq!(values, vec!["a", "a\u{301}", "á", "b"]);
        let cmp = |l: &str, r: &str| collators.tertiary().compare(l, r).then_with(|| l.cmp(r));
        assert_eq!(cmp("á", "a\u{301}"), Ordering::Greater);
        assert_eq!(cmp("a\u{301}", "á"), Ordering::Less);
    }

    #[test]
    fn zh_locale_collator_constructs() {
        // The zh collator builds and produces a deterministic (pinyin) order.
        let collators = collators_for("zh-CN");
        let cmp = |l: &str, r: &str| collators.tertiary().compare(l, r).then_with(|| l.cmp(r));
        assert_eq!(cmp("中文", "中文"), Ordering::Equal);
        assert_eq!(cmp("啊", "波"), Ordering::Less);
    }

    #[test]
    fn primary_strength_matches_case_and_accent_insensitively() {
        // PRIMARY: case/accent differences are not visible at the comparison
        // level; the raw-string tie-break still makes the result deterministic.
        let collators = collators_for("und");
        let cmp = |l: &str, r: &str| collators.primary().compare(l, r).then_with(|| l.cmp(r));
        // equal under ICU, raw tie-break decides (H < h)
        assert_eq!(cmp("hello", "HELLO"), Ordering::Greater);
        assert_eq!(cmp("HELLO", "hello"), Ordering::Less);
        // equal under ICU (é ~ e at primary), raw tie-break decides
        assert_eq!(cmp("héllo", "HELLO"), Ordering::Greater);
        assert_eq!(cmp("HELLO", "héllo"), Ordering::Less);
        // primary difference wins over the tie-break
        assert_eq!(cmp("hello", "hellz"), Ordering::Less);
        assert_eq!(cmp("hellz", "hello"), Ordering::Greater);
    }

    #[test]
    fn natural_sort_orders_numeric_segments_by_value() {
        assert_eq!(compare_natural("2", "10"), Ordering::Less);
        assert_eq!(compare_natural("10", "2"), Ordering::Greater);
        assert_eq!(compare_natural("Page 2", "Page 10"), Ordering::Less);
        assert_eq!(compare_natural("Vol 2", "Vol 10"), Ordering::Less);
        // a decimal point is not part of a number: "1.5" vs "1.10" orders by
        // the digit runs after the dot (5 < 10), matching Explorer/Finder and
        // the grey-panther natural comparator
        assert_eq!(compare_natural("Vol 1.5", "Vol 1.10"), Ordering::Less);
        assert_eq!(compare_natural("Vol 1.10", "Vol 1.5"), Ordering::Greater);
        // "1.50" vs "1.5": digit runs compare 50 > 5
        assert_eq!(compare_natural("Vol 1.5", "Vol 1.50"), Ordering::Less);
        assert_eq!(compare_natural("Vol 1.50", "Vol 1.5"), Ordering::Greater);
        // leading zeros are numerically equal; the raw fallback decides
        assert_eq!(compare_natural("a01", "a1"), Ordering::Less);
        assert_eq!(compare_natural("a1", "a01"), Ordering::Greater);
        assert_eq!(compare_natural("a1b", "a01c"), Ordering::Less);
        // numeric segments always sort before text
        assert_eq!(compare_natural("1", "a"), Ordering::Less);
        assert_eq!(compare_natural("a", "1"), Ordering::Greater);
        // whitespace is normalized before segmenting; equal values are ordered
        // by the raw string so the comparison is a total order
        assert_eq!(compare_natural("Page  2", "Page 2"), Ordering::Less);
        assert_eq!(compare_natural("Page 2", "Page  2"), Ordering::Greater);
    }

    #[test]
    fn natural_sort_text_segments_follow_icu_and_tie_break() {
        // text segments still use the configured ICU tertiary collator
        assert_eq!(compare_natural("cafe", "café"), Ordering::Less);
        // tertiary distinguishes case (lowercase first in UCA)
        // canonical-equivalence tie-break inside text segments
        assert_eq!(compare_natural("á", "a\u{301}"), Ordering::Greater);
        assert_eq!(compare_natural("a\u{301}", "á"), Ordering::Less);
        // mixed: text decides first, then numbers
        assert_eq!(compare_natural("a2", "b1"), Ordering::Less);
    }

    #[test]
    fn natural_sort_differs_from_plain_icu_on_numbers() {
        // plain tertiary is character order ("10" < "2"); the natural sort
        // compares the numeric segments by value ("2" < "10").
        assert_eq!(compare_tertiary("Page 10", "Page 2"), Ordering::Less);
        assert_eq!(compare_natural("Page 10", "Page 2"), Ordering::Greater);
    }
}
