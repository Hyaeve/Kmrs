//! grey-panther natural-comparator 1.1's `CaseInsensitiveSimpleNaturalComparator`
//! replica, kept for its auxiliary functions:
//! - `strip_accents` backs the `UDF_STRIP_ACCENTS` SQL function
//!   (commons-lang3 behavior);
//! - `series_sort_key` is the Kotlin `SeriesLifecycle.sortBooks` sort key
//!   (trim → stripAccents → collapse whitespace), applied before the ICU
//!   comparison in `sort_books`.
//!
//! The comparator itself is deprecated in favor of `sort_locale::compare_natural`,
//! so page order and book numbering follow the same total order as the
//! `COLLATION_UNICODE_3` SQL collation.
//!
//! Semantics (checked against the `AbstractSimpleNaturalComparator.compare` source):
//! - Scans UTF-16 code unit by code unit; `isDigit` covers only ASCII 0-9.
//! - One side is a digit and the other is not → the digit side is smaller.
//! - Both sides are digits: the digit runs are parsed as long (wrapping on overflow); unequal values → return the unsigned comparison;
//!   equal values (leading zeros ignored) → do not return, keep comparing the following characters (`a01b` vs `a1c` goes on to compare `b`/`c`).
//! - Non-digit characters: `Character.toLowerCase(c1) - Character.toLowerCase(c2)` (per code unit).
//! - One side runs out first: the longer remainder is greater; both run out together → equal (`a1` and `a01` compare equal; this is not a total order).

use std::cmp::Ordering;

#[deprecated(note = "use sort_locale::compare_natural instead")]
pub fn compare(a: &str, b: &str) -> Ordering {
    let s1: Vec<u16> = a.encode_utf16().collect();
    let s2: Vec<u16> = b.encode_utf16().collect();
    let (mut i1, mut i2) = (0usize, 0usize);
    loop {
        if i1 >= s1.len() {
            return if i2 >= s2.len() {
                Ordering::Equal
            } else {
                Ordering::Less
            };
        }
        if i2 >= s2.len() {
            return Ordering::Greater;
        }
        let d1 = is_digit(s1[i1]);
        let d2 = is_digit(s2[i2]);
        match (d1, d2) {
            (true, true) => {
                let (n1, next1) = parse_digits(&s1, i1);
                let (n2, next2) = parse_digits(&s2, i2);
                if n1 != n2 {
                    return n1.cmp(&n2);
                }
                i1 = next1;
                i2 = next2;
            }
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {
                let l1 = lower(s1[i1]);
                let l2 = lower(s2[i2]);
                match l1.cmp(&l2) {
                    Ordering::Equal => {
                        i1 += 1;
                        i2 += 1;
                    }
                    ord => return ord,
                }
            }
        }
    }
}

fn is_digit(c: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&c)
}

/// Greedily parses a digit run with u64 wrapping (= the bit pattern of a Java long compared unsigned after overflow).
fn parse_digits(s: &[u16], start: usize) -> (u64, usize) {
    let mut n: u64 = 0;
    let mut i = start;
    while i < s.len() && is_digit(s[i]) {
        n = n.wrapping_mul(10).wrapping_add((s[i] - b'0' as u16) as u64);
        i += 1;
    }
    (n, i)
}

/// Equivalent of `Character.toLowerCase(char)`: BMP characters map to the first lowercase letter, surrogates are kept as-is.
fn lower(c: u16) -> u32 {
    char::from_u32(c as u32)
        .and_then(|ch| ch.to_lowercase().next())
        .map(|ch| ch as u32)
        .unwrap_or(c as u32)
}

/// commons-lang3 `StringUtils.stripAccents`: after NFD, removes the U+0300..=U+036F combining diacriticals block,
/// with special cases Ł→L and ł→l.
pub fn strip_accents(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd()
        .map(|c| match c {
            '\u{0141}' => 'L',
            '\u{0142}' => 'l',
            c => c,
        })
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
        .collect()
}

/// The sort key from komga's `SeriesLifecycle.sortBooks`: trim → stripAccents → collapse whitespace.
pub fn series_sort_key(name: &str) -> String {
    strip_accents(name.trim())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #![allow(deprecated)] // compare is kept as the Java-compat baseline
    use super::*;

    #[test]
    fn numeric_runs() {
        assert_eq!(compare("2", "10"), Ordering::Less);
        assert_eq!(compare("a2", "a10"), Ordering::Less);
        assert_eq!(compare("a10", "a2"), Ordering::Greater);
    }

    #[test]
    fn leading_zeros_equal_then_continue() {
        assert_eq!(compare("a01b", "a1c"), Ordering::Less);
        // Equal values that run out together → equal (not a total order)
        assert_eq!(compare("a1", "a01"), Ordering::Equal);
        assert_eq!(compare("a01", "a1b"), Ordering::Less);
    }

    #[test]
    fn digit_before_non_digit() {
        assert_eq!(compare("1", "a"), Ordering::Less);
        assert_eq!(compare("a", "1"), Ordering::Greater);
        assert_eq!(compare("1a", "a1"), Ordering::Less);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(compare("ABC", "abc"), Ordering::Equal);
        assert_eq!(compare("B", "a"), Ordering::Greater);
        assert_eq!(compare("Ä", "ä"), Ordering::Equal);
    }

    #[test]
    fn prefix_shorter_is_less() {
        assert_eq!(compare("a", "ab"), Ordering::Less);
        assert_eq!(compare("ab", "a"), Ordering::Greater);
        assert_eq!(compare("", ""), Ordering::Equal);
    }

    #[test]
    fn overflow_wraps_unsigned() {
        // u64::MAX vs a value wrapped to 0: unsigned comparison gives MAX > 0
        assert_eq!(
            compare("18446744073709551615", "18446744073709551616"),
            Ordering::Greater
        );
        // Does not fall back to string-length comparison
        assert_eq!(compare("18446744073709551616", "9"), Ordering::Less);
    }

    #[test]
    fn typical_comic_names() {
        let mut files = vec![
            "page 10.jpg",
            "page 2.jpg",
            "Page 1.jpg",
            "page 10a.jpg",
            "cover.jpg",
        ];
        files.sort_by(|a, b| compare(a, b));
        assert_eq!(
            files,
            vec![
                "cover.jpg",
                "Page 1.jpg",
                "page 2.jpg",
                "page 10.jpg",
                "page 10a.jpg"
            ]
        );
    }

    #[test]
    fn sort_key_strips_accents_and_whitespace() {
        assert_eq!(series_sort_key("  Héllo   Wörld "), "Hello World");
        assert_eq!(series_sort_key("Sôdan"), "Sodan");
        assert_eq!(strip_accents("Łódź"), "Lodz");
    }
}
