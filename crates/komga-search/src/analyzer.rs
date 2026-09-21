//! Analysis chain aligned with komga's Lucene analyzers:
//! - search side (`MultiLingualAnalyzer`): standard tokenize -> CJK width -> lowercase ->
//!   CJK bigram -> ASCII fold
//! - index side (`MultiLingualNGramAnalyzer`): the same plus NGram(3, 10,
//!   preserveOriginal = true) before folding
//! - `MultiLingualAnalyzer.normalize` (used for prefix/wildcard query terms): CJK width ->
//!   lowercase -> ASCII fold, without bigramming

use tantivy::tokenizer::{Token, TokenStream, Tokenizer};
use unicode_normalization::UnicodeNormalization;

/// Lucene `StandardTokenizer` (UAX#29 word segmentation, komga-relevant subset):
/// Han/Hiragana characters are emitted one per token, katakana runs stay together, and
/// other letters/digits form maximal runs (with `.`/`:` joining inside and `,`/`;` between digits).
pub fn standard_tokenize(text: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Cls {
        ALetter,
        Numeric,
        Katakana,
        Extend,
    }

    fn classify(c: char) -> Option<Cls> {
        if is_ideographic(c) {
            return None; // emitted per character, outside the run machinery
        }
        if is_katakana(c) {
            return Some(Cls::Katakana);
        }
        if c.is_alphabetic() {
            return Some(Cls::ALetter);
        }
        if c.is_numeric() {
            return Some(Cls::Numeric);
        }
        if is_extend(c as u32) {
            return Some(Cls::Extend);
        }
        None
    }

    let chars: Vec<char> = text.chars().collect();
    let mut tokens = vec![];
    let mut buf = String::new();
    let mut buf_cls: Option<Cls> = None;

    fn flush(buf: &mut String, tokens: &mut Vec<String>) {
        if !buf.is_empty() {
            tokens.push(std::mem::take(buf));
        }
    }

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_ideographic(c) {
            flush(&mut buf, &mut tokens);
            buf_cls = None;
            tokens.push(c.to_string());
            i += 1;
            continue;
        }
        // `.`/`:` join inside an alphanumeric run when both neighbors are alphanumeric;
        // `,`/`;` join only between digits (UAX#29 WB6/7/11/12)
        if c == '.' || c == ':' || c == ',' || c == ';' {
            let prev_alnum = buf_cls.is_some();
            let next = chars.get(i + 1);
            let next_alnum = next.is_some_and(|n| n.is_alphanumeric());
            let joins = if c == ',' || c == ';' {
                buf_cls == Some(Cls::Numeric) && next.is_some_and(|n| n.is_numeric())
            } else {
                prev_alnum && next_alnum
            };
            if joins {
                buf.push(c);
            } else {
                flush(&mut buf, &mut tokens);
                buf_cls = None;
            }
            i += 1;
            continue;
        }
        let cls = match classify(c) {
            Some(cls) => cls,
            // punctuation and symbols act as separators
            None => {
                flush(&mut buf, &mut tokens);
                buf_cls = None;
                i += 1;
                continue;
            }
        };
        match (buf_cls, cls) {
            (None, _) => {
                buf.push(c);
                if cls != Cls::Extend {
                    buf_cls = Some(cls);
                }
            }
            (Some(_), Cls::Extend) => buf.push(c),
            (Some(a), b) if a == b => buf.push(c),
            (Some(Cls::ALetter), Cls::Numeric) | (Some(Cls::Numeric), Cls::ALetter) => buf.push(c),
            (Some(_), _) => {
                flush(&mut buf, &mut tokens);
                buf.push(c);
                buf_cls = Some(cls);
            }
        }
        i += 1;
    }
    flush(&mut buf, &mut tokens);
    tokens
}

/// UAX#29 Extend: combining marks and ZWJ
fn is_extend(u: u32) -> bool {
    (0x0300..=0x036F).contains(&u)
        || (0x1AB0..=0x1AFF).contains(&u)
        || (0x1DC0..=0x1DFF).contains(&u)
        || (0x20D0..=0x20FF).contains(&u)
        || (0xFE00..=0xFE0F).contains(&u)
        || (0xFE20..=0xFE2F).contains(&u)
        || u == 0x200D
        || (0xE0100..=0xE01EF).contains(&u)
}

/// Han ideographs and Hiragana: emitted as single-character tokens (UAX#29)
fn is_ideographic(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u)
        || (0x3400..=0x4DBF).contains(&u)
        || (0x20000..=0x2A6DF).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0x3040..=0x309F).contains(&u)
}

fn is_katakana(c: char) -> bool {
    let u = c as u32;
    (0x30A0..=0x30FF).contains(&u)
        || (0x31F0..=0x31FF).contains(&u)
        || (0xFF61..=0xFF9F).contains(&u)
}

/// Lucene `CJKBigramFilter` character ranges (Han/Hiragana/Katakana; Hangul excluded)
fn is_cjk_char(c: char) -> bool {
    is_ideographic(c) || is_katakana(c)
}

const HALFWIDTH_KATAKANA: [char; 0x3D] = [
    '。', '「', '」', '、', '・', 'ヲ', 'ァ', 'ィ', 'ゥ', 'ェ', 'ォ', 'ャ', 'ュ', 'ョ', 'ッ', 'ー',
    'ア', 'イ', 'ウ', 'エ', 'オ', 'カ', 'キ', 'ク', 'ケ', 'コ', 'サ', 'シ', 'ス', 'セ', 'ソ', 'タ',
    'チ', 'ツ', 'テ', 'ト', 'ナ', 'ニ', 'ヌ', 'ネ', 'ノ', 'ハ', 'ヒ', 'フ', 'ヘ', 'ホ', 'マ', 'ミ',
    'ム', 'メ', 'モ', 'ヤ', 'ユ', 'ヨ', 'ラ', 'リ', 'ル', 'レ', 'ロ', 'ワ', 'ン',
];

fn dakuten(c: char) -> Option<char> {
    Some(match c {
        'ウ' => 'ヴ',
        'カ' => 'ガ',
        'キ' => 'ギ',
        'ク' => 'グ',
        'ケ' => 'ゲ',
        'コ' => 'ゴ',
        'サ' => 'ザ',
        'シ' => 'ジ',
        'ス' => 'ズ',
        'セ' => 'ゼ',
        'ソ' => 'ゾ',
        'タ' => 'ダ',
        'チ' => 'ヂ',
        'ツ' => 'ヅ',
        'テ' => 'デ',
        'ト' => 'ド',
        'ハ' => 'バ',
        'ヒ' => 'ビ',
        'フ' => 'ブ',
        'ヘ' => 'ベ',
        'ホ' => 'ボ',
        _ => return None,
    })
}

fn handakuten(c: char) -> Option<char> {
    Some(match c {
        'ハ' => 'パ',
        'ヒ' => 'ピ',
        'フ' => 'プ',
        'ヘ' => 'ペ',
        'ホ' => 'ポ',
        _ => return None,
    })
}

/// Lucene `CJKWidthFilter`: fullwidth ASCII (U+FF01-U+FF5E) to ASCII, halfwidth katakana
/// (U+FF61-U+FF9F) to fullwidth katakana with dakuten/handakuten combination.
pub fn cjk_width_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut iter = text.chars().peekable();
    while let Some(c) = iter.next() {
        let u = c as u32;
        if (0xFF01..=0xFF5E).contains(&u) {
            out.push(char::from_u32(u - 0xFEE0).unwrap());
        } else if (0xFF61..=0xFF9D).contains(&u) {
            let full = HALFWIDTH_KATAKANA[(u - 0xFF61) as usize];
            match iter.peek() {
                Some('ﾞ') if dakuten(full).is_some() => {
                    out.push(dakuten(full).unwrap());
                    iter.next();
                }
                Some('ﾟ') if handakuten(full).is_some() => {
                    out.push(handakuten(full).unwrap());
                    iter.next();
                }
                _ => out.push(full),
            }
        } else if u == 0xFF9E {
            out.push('゛');
        } else if u == 0xFF9F {
            out.push('゜');
        } else {
            out.push(c);
        }
    }
    out
}

pub fn cjk_width(tokens: Vec<String>) -> Vec<String> {
    tokens.into_iter().map(|t| cjk_width_str(&t)).collect()
}

pub fn lowercase(tokens: Vec<String>) -> Vec<String> {
    tokens.into_iter().map(|t| t.to_lowercase()).collect()
}

/// Lucene `CJKBigramFilter` with one recall deviation: sliding bigrams over the CJK
/// character stream; a trailing lone CJK character is emitted as a unigram, and the first
/// character of a CJK run following a non-CJK token is emitted as a unigram too — without
/// it a query like "3月" cannot match "3月的狮子", where 月 only exists inside the bigram 月的.
pub fn cjk_bigram(tokens: Vec<String>) -> Vec<String> {
    let mut out = vec![];
    // pending CJK character awaiting a possible bigram; the flag marks it as already
    // emitted via the left-boundary rule so a one-character run is not emitted twice
    let mut prev: Option<(String, bool)> = None;
    let mut after_non_cjk = false;
    for token in tokens {
        if token.chars().all(is_cjk_char) {
            for c in token.chars() {
                let c = c.to_string();
                let next = match prev.take() {
                    Some((p, _)) => {
                        out.push(format!("{p}{c}"));
                        (c, false)
                    }
                    None if after_non_cjk => {
                        out.push(c.clone());
                        (c, true)
                    }
                    None => (c, false),
                };
                prev = Some(next);
            }
        } else {
            if let Some((p, emitted)) = prev.take() {
                if !emitted {
                    out.push(p);
                }
            }
            out.push(token);
        }
        after_non_cjk = prev.is_none();
    }
    if let Some((p, emitted)) = prev.take() {
        if !emitted {
            out.push(p);
        }
    }
    out
}

/// Lucene `NGramTokenFilter`: all substrings of length min..=max per token; with
/// `preserve_original` the whole token is emitted as well.
pub fn ngram(tokens: Vec<String>, min: usize, max: usize, preserve_original: bool) -> Vec<String> {
    let mut out = vec![];
    for token in tokens {
        let chars: Vec<char> = token.chars().collect();
        let len = chars.len();
        for size in min..=max.min(len) {
            for i in 0..=len - size {
                out.push(chars[i..i + size].iter().collect());
            }
        }
        if preserve_original {
            out.push(token);
        }
    }
    out
}

/// Lucene `ASCIIFoldingFilter` (komga-relevant subset): NFD decomposition with combining
/// marks stripped, plus the explicit expansions that decomposition cannot produce.
/// Greek/Cyrillic/CJK pass through unchanged, as in Lucene.
pub fn ascii_fold_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ß' => out.push_str("ss"),
            'ẞ' => out.push_str("SS"),
            'æ' => out.push_str("ae"),
            'Æ' => out.push_str("AE"),
            'œ' => out.push_str("oe"),
            'Œ' => out.push_str("OE"),
            'ø' => out.push('o'),
            'Ø' => out.push('O'),
            'đ' | 'ð' => out.push('d'),
            'Đ' | 'Ð' => out.push('D'),
            'þ' => out.push_str("th"),
            'Þ' => out.push_str("TH"),
            'ł' => out.push('l'),
            'Ł' => out.push('L'),
            'ı' => out.push('i'),
            'ŋ' => out.push('n'),
            'Ŋ' => out.push('N'),
            'ħ' => out.push('h'),
            'Ĥ' => out.push('H'),
            'ƒ' => out.push('f'),
            'ĳ' => out.push_str("ij"),
            'Ĳ' => out.push_str("IJ"),
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            'ﬅ' | 'ﬆ' => out.push_str("st"),
            _ => {
                for d in c.to_string().nfd() {
                    if !is_extend(d as u32) {
                        out.push(d);
                    }
                }
            }
        }
    }
    out
}

pub fn ascii_fold(tokens: Vec<String>) -> Vec<String> {
    tokens.into_iter().map(|t| ascii_fold_str(&t)).collect()
}

/// Search-side chain (`MultiLingualAnalyzer`)
pub fn search_analyze(text: &str) -> Vec<String> {
    ascii_fold(cjk_bigram(lowercase(cjk_width(standard_tokenize(text)))))
}

/// Index-side chain (`MultiLingualNGramAnalyzer` with minGram=3, maxGram=10, preserveOriginal)
pub fn index_analyze(text: &str) -> Vec<String> {
    ascii_fold(ngram(
        cjk_bigram(lowercase(cjk_width(standard_tokenize(text)))),
        3,
        10,
        true,
    ))
}

/// `MultiLingualAnalyzer.normalize`, used for prefix/wildcard query terms
pub fn normalize(text: &str) -> String {
    ascii_fold_str(&cjk_width_str(text).to_lowercase())
}

/// Eager token stream over a precomputed token list.
pub struct VecTokenStream {
    tokens: Vec<Token>,
    pos: usize,
}

impl VecTokenStream {
    pub fn new(tokens: Vec<String>) -> Self {
        let tokens = tokens
            .into_iter()
            .enumerate()
            .map(|(position, text)| Token {
                position,
                text,
                ..Default::default()
            })
            .collect();
        Self { tokens, pos: 0 }
    }
}

impl TokenStream for VecTokenStream {
    fn advance(&mut self) -> bool {
        if self.pos < self.tokens.len() {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.tokens[self.pos - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.pos - 1]
    }
}

/// The index-side analyzer registered in the tantivy schema.
#[derive(Clone, Default)]
pub struct KomgaIndexTokenizer;

impl Tokenizer for KomgaIndexTokenizer {
    type TokenStream<'a> = VecTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        VecTokenStream::new(index_analyze(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_tokenize_latin_and_digits() {
        assert_eq!(standard_tokenize("Berserk"), vec!["Berserk"]);
        assert_eq!(standard_tokenize("v01"), vec!["v01"]);
        assert_eq!(standard_tokenize("9781593070205"), vec!["9781593070205"]);
        assert_eq!(standard_tokenize("hello world"), vec!["hello", "world"]);
        assert_eq!(standard_tokenize("foo.bar"), vec!["foo.bar"]);
        assert_eq!(standard_tokenize("a,b"), vec!["a", "b"]);
        assert_eq!(standard_tokenize("1,5"), vec!["1,5"]);
    }

    #[test]
    fn standard_tokenize_cjk() {
        assert_eq!(standard_tokenize("東京タワー"), vec!["東", "京", "タワー"]);
        assert_eq!(standard_tokenize("ひらがな"), vec!["ひ", "ら", "が", "な"]);
        assert_eq!(standard_tokenize("カタカナ"), vec!["カタカナ"]);
        assert_eq!(
            standard_tokenize("one 東京 two"),
            vec!["one", "東", "京", "two"]
        );
    }

    #[test]
    fn cjk_width_fullwidth_ascii_and_halfwidth_katakana() {
        assert_eq!(cjk_width_str("Ｈｅｌｌｏ"), "Hello");
        assert_eq!(cjk_width_str("１２３"), "123");
        assert_eq!(cjk_width_str("ｶﾀｶﾅ"), "カタカナ");
        assert_eq!(cjk_width_str("ｶﾞｷ"), "ガキ");
        assert_eq!(cjk_width_str("ﾊﾟﾋﾟ"), "パピ");
        assert_eq!(cjk_width_str("ｳﾞ"), "ヴ");
    }

    #[test]
    fn cjk_bigram_sequence() {
        assert_eq!(
            cjk_bigram(standard_tokenize("東京タワー")),
            vec!["東京", "京タ", "タワ", "ワー", "ー"]
        );
        assert_eq!(
            cjk_bigram(standard_tokenize("ひらがな")),
            vec!["ひら", "らが", "がな", "な"]
        );
        // non-CJK tokens flush the pending character, and a run following one emits
        // its first character as a boundary unigram
        assert_eq!(
            cjk_bigram(vec!["abc".into(), "東".into(), "京".into(), "def".into()]),
            vec!["abc", "東", "東京", "京", "def"]
        );
    }

    #[test]
    fn cjk_bigram_boundary_unigrams() {
        // the left-boundary 月 is what lets the query "3月" match this title
        assert_eq!(
            search_analyze("3月的狮子"),
            vec!["3", "月", "月的", "的狮", "狮子", "子"]
        );
        assert_eq!(search_analyze("3月"), vec!["3", "月"]);
        // a run at the very start of the text gets no left-boundary unigram
        assert_eq!(search_analyze("犬夜叉2"), vec!["犬夜", "夜叉", "叉", "2"]);
        // a run sandwiched between non-CJK tokens emits both boundary unigrams
        assert_eq!(search_analyze("A月的B"), vec!["a", "月", "月的", "的", "b"]);
        // the rule looks at the token stream, so a run after a whitespace-separated
        // Latin word gains the unigram too
        assert_eq!(
            search_analyze("Batman 東京"),
            vec!["batman", "東", "東京", "京"]
        );
    }

    #[test]
    fn ngram_3_to_10_preserve_original() {
        let grams = ngram(vec!["berserk".to_string()], 3, 10, true);
        for expected in [
            "ber", "ers", "rse", "ser", "erk", "bers", "erse", "rser", "serk", "berserk",
        ] {
            assert!(grams.contains(&expected.to_string()), "missing {expected}");
        }
        // length 2 token: only the original survives (minGram=3)
        let grams = ngram(vec!["東京".to_string()], 3, 10, true);
        assert_eq!(grams, vec!["東京"]);
    }

    #[test]
    fn ascii_folding() {
        assert_eq!(ascii_fold_str("café"), "cafe");
        assert_eq!(ascii_fold_str("straße"), "strasse");
        assert_eq!(ascii_fold_str("œuvre"), "oeuvre");
        assert_eq!(ascii_fold_str("ﬁle"), "file");
        assert_eq!(ascii_fold_str("naïve"), "naive");
        assert_eq!(ascii_fold_str("Łódź"), "Lodz");
        assert_eq!(ascii_fold_str("東京"), "東京");
    }

    #[test]
    fn search_chain() {
        assert_eq!(search_analyze("Ｈｅｌｌｏ"), vec!["hello"]);
        assert_eq!(
            search_analyze("東京タワー"),
            vec!["東京", "京タ", "タワ", "ワー", "ー"]
        );
    }

    #[test]
    fn normalize_chain() {
        assert_eq!(normalize("Ｈｅｌｌｏ"), "hello");
        // no bigramming for prefix/wildcard terms
        assert_eq!(normalize("東京"), "東京");
    }

    #[test]
    fn index_tokenizer_stream() {
        let mut tokenizer = KomgaIndexTokenizer;
        let mut stream = tokenizer.token_stream("Berserk");
        let mut texts = vec![];
        while stream.advance() {
            texts.push(stream.token().text.clone());
        }
        assert!(texts.contains(&"berserk".to_string()));
        assert!(texts.contains(&"ber".to_string()));
    }
}
