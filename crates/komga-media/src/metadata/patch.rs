//! Patch model (`BookMetadataPatch.kt` / `SeriesMetadataPatch.kt`), `MetadataApplier.kt`,
//! `MetadataAggregator.kt`, and `mostFrequent`.

use komga_core::model::book::{BookMetadata, WebLink};
use komga_core::model::library::Library;
use komga_core::model::media::Media;
use komga_core::model::series::{ReadingDirection, SeriesMetadata, SeriesStatus};
use komga_core::task::BookMetadataPatchCapability;
use std::collections::BTreeSet;
use std::path::Path;
use time::Date;

// re-exported so providers can name the domain types through the patch module
pub use komga_core::model::common::Author;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataPatchTarget {
    Book,
    Series,
    ReadList,
    Collection,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BookMetadataPatch {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub number: Option<String>,
    pub number_sort: Option<f32>,
    pub release_date: Option<Date>,
    pub authors: Option<Vec<Author>>,
    pub isbn: Option<String>,
    pub links: Option<Vec<WebLink>>,
    /// Kotlin `BookMetadata.tags` is a Set; the Rust domain model stores it as Vec
    pub tags: Option<Vec<String>>,
    pub read_lists: Vec<ReadListEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadListEntry {
    pub name: String,
    pub number: Option<i32>,
}

impl ReadListEntry {
    pub fn new(name: impl Into<String>, number: Option<i32>) -> Self {
        Self {
            name: name.into(),
            number,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeriesMetadataPatch {
    pub title: Option<String>,
    pub title_sort: Option<String>,
    pub status: Option<SeriesStatus>,
    pub summary: Option<String>,
    pub reading_direction: Option<ReadingDirection>,
    pub publisher: Option<String>,
    pub age_rating: Option<i32>,
    pub language: Option<String>,
    pub genres: Option<BTreeSet<String>>,
    pub total_book_count: Option<i32>,
    pub collections: BTreeSet<String>,
}

/// `MetadataApplier.apply(patch, metadata)`: a patch field wins only when present and unlocked.
pub fn apply_book_patch(patch: &BookMetadataPatch, metadata: &BookMetadata) -> BookMetadata {
    fn get<T: Clone>(original: &T, patched: &Option<T>, lock: bool) -> T {
        match patched {
            Some(value) if !lock => value.clone(),
            _ => original.clone(),
        }
    }
    fn get_opt<T: Clone>(original: &Option<T>, patched: &Option<T>, lock: bool) -> Option<T> {
        match patched {
            Some(value) if !lock => Some(value.clone()),
            _ => original.clone(),
        }
    }
    BookMetadata {
        title: get(&metadata.title, &patch.title, metadata.title_lock),
        summary: get(&metadata.summary, &patch.summary, metadata.summary_lock),
        number: get(&metadata.number, &patch.number, metadata.number_lock),
        number_sort: get(
            &metadata.number_sort,
            &patch.number_sort,
            metadata.number_sort_lock,
        ),
        release_date: get_opt(
            &metadata.release_date,
            &patch.release_date,
            metadata.release_date_lock,
        ),
        authors: get(&metadata.authors, &patch.authors, metadata.authors_lock),
        isbn: get(&metadata.isbn, &patch.isbn, metadata.isbn_lock),
        links: get(&metadata.links, &patch.links, metadata.links_lock),
        tags: get(&metadata.tags, &patch.tags, metadata.tags_lock),
        ..metadata.clone()
    }
}

/// `MetadataApplier.apply(patch, metadata)` for series (collections are applied separately
/// by the lifecycle, not here).
pub fn apply_series_patch(
    patch: &SeriesMetadataPatch,
    metadata: &SeriesMetadata,
) -> SeriesMetadata {
    fn get<T: Clone>(original: &T, patched: &Option<T>, lock: bool) -> T {
        match patched {
            Some(value) if !lock => value.clone(),
            _ => original.clone(),
        }
    }
    fn get_opt<T: Clone>(original: &Option<T>, patched: &Option<T>, lock: bool) -> Option<T> {
        match patched {
            Some(value) if !lock => Some(value.clone()),
            _ => original.clone(),
        }
    }
    SeriesMetadata {
        status: get(&metadata.status, &patch.status, metadata.status_lock),
        title: get(&metadata.title, &patch.title, metadata.title_lock),
        title_sort: get(
            &metadata.title_sort,
            &patch.title_sort,
            metadata.title_sort_lock,
        ),
        summary: get(&metadata.summary, &patch.summary, metadata.summary_lock),
        reading_direction: get_opt(
            &metadata.reading_direction,
            &patch.reading_direction,
            metadata.reading_direction_lock,
        ),
        age_rating: get_opt(
            &metadata.age_rating,
            &patch.age_rating,
            metadata.age_rating_lock,
        ),
        publisher: get(
            &metadata.publisher,
            &patch.publisher,
            metadata.publisher_lock,
        ),
        language: get(&metadata.language, &patch.language, metadata.language_lock),
        genres: get(&metadata.genres, &patch.genres, metadata.genres_lock),
        total_book_count: get_opt(
            &metadata.total_book_count,
            &patch.total_book_count,
            metadata.total_book_count_lock,
        ),
        ..metadata.clone()
    }
}

/// The parts of `BookMetadataAggregation` that `MetadataAggregator.aggregate` computes
/// (seriesId is filled in by the caller).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AggregationParts {
    pub authors: Vec<Author>,
    pub tags: BTreeSet<String>,
    pub release_date: Option<Date>,
    pub summary: String,
    pub summary_number: String,
}

/// `MetadataAggregator.aggregate`.
pub fn aggregate(metadatas: &[BookMetadata]) -> AggregationParts {
    let mut authors: Vec<Author> = vec![];
    for author in metadatas.iter().flat_map(|m| m.authors.iter()) {
        if !authors.iter().any(|a| {
            format!("{}__{}", a.role, a.name) == format!("{}__{}", author.role, author.name)
        }) {
            authors.push(author.clone());
        }
    }
    let tags: BTreeSet<String> = metadatas
        .iter()
        .flat_map(|m| m.tags.iter().cloned())
        .collect();
    let (summary, summary_number) = metadatas
        .iter()
        .min_by(|a, b| {
            a.number_sort
                .partial_cmp(&b.number_sort)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .filter(|m| !m.summary.is_empty())
        .map(|m| (m.summary.clone(), m.number.clone()))
        .unwrap_or_default();
    let release_date = metadatas.iter().filter_map(|m| m.release_date).min();
    AggregationParts {
        authors,
        tags,
        release_date,
        summary,
        summary_number,
    }
}

/// `LanguageUtils.mostFrequent`: the value with the highest count; on ties, the one whose
/// first occurrence comes first (Kotlin `groupingBy.eachCount().maxByOrNull`).
pub fn most_frequent<T, R: Eq>(
    items: impl IntoIterator<Item = T>,
    transform: impl Fn(&T) -> Option<R>,
) -> Option<R> {
    let mut order: Vec<R> = vec![];
    let mut counts: Vec<usize> = vec![];
    for item in items {
        if let Some(value) = transform(&item) {
            match order.iter().position(|v| *v == value) {
                Some(index) => counts[index] += 1,
                None => {
                    order.push(value);
                    counts.push(1);
                }
            }
        }
    }
    // Rust's max_by_key returns the last maximum; Kotlin's maxByOrNull keeps the first
    let mut best: Option<usize> = None;
    for (index, count) in counts.iter().enumerate() {
        if best.is_none_or(|b| *count > counts[b]) {
            best = Some(index);
        }
    }
    best.map(|index| order.swap_remove(index))
}

/// ISO 639-1 + ISO 639-2/T (Debian iso-codes iso_639-2.json) + ISO 639-2/B variants
/// + deprecated aliases: the membership set behind `BCP47TagValidator.isValid`.
pub const ISO_LANGUAGES: &[&str] = &[
    "aa", "aar", "ab", "abk", "ace", "ach", "ada", "ady", "ae", "af", "afa", "afh", "afr", "ain",
    "ak", "aka", "akk", "alb", "ale", "alg", "alt", "am", "amh", "an", "ang", "anp", "apa", "ar",
    "ara", "arc", "arg", "arm", "arn", "arp", "art", "arw", "as", "asm", "ast", "ath", "aus", "av",
    "ava", "ave", "awa", "ay", "aym", "az", "aze", "ba", "bad", "bai", "bak", "bal", "bam", "ban",
    "baq", "bas", "bat", "be", "bej", "bel", "bem", "ben", "ber", "bg", "bho", "bi", "bik", "bin",
    "bis", "bla", "bm", "bn", "bnt", "bo", "bod", "bos", "br", "bra", "bre", "bs", "btk", "bua",
    "bug", "bul", "bur", "byn", "ca", "cad", "cai", "car", "cat", "cau", "ceb", "cel", "ces", "ch",
    "cha", "chb", "che", "chg", "chi", "chk", "chm", "chn", "cho", "chp", "chr", "chu", "chv",
    "chy", "ckt", "cmc", "cnr", "co", "cop", "cor", "cos", "cpe", "cpf", "cpp", "cr", "crh", "crp",
    "cs", "csb", "cu", "cus", "cv", "cy", "cym", "cze", "da", "dak", "dan", "dar", "day", "de",
    "del", "den", "deu", "dgr", "din", "div", "doi", "dra", "dsb", "dua", "dum", "dut", "dv",
    "dyu", "dz", "dzo", "ee", "efi", "egl", "egy", "eka", "el", "ell", "elx", "en", "eng", "enm",
    "eo", "epo", "es", "est", "et", "eu", "eus", "eve", "ewo", "fa", "fan", "fao", "fas", "fat",
    "fi", "fij", "fil", "fin", "fiu", "fj", "fo", "fon", "fr", "fra", "fre", "frm", "fro", "frr",
    "frs", "fry", "ful", "fur", "fy", "ga", "gaa", "gai", "gal", "gay", "gba", "gd", "gem", "geo",
    "ger", "gez", "gil", "gl", "gla", "gle", "glg", "glv", "gmh", "gn", "gnd", "gon", "gor", "got",
    "grb", "grc", "gre", "gsw", "gu", "guj", "gwi", "ha", "hai", "hat", "hau", "haw", "he", "heb",
    "her", "hi", "hil", "him", "hin", "hit", "hmn", "hmo", "ho", "hr", "hrv", "hsb", "ht", "hu",
    "hun", "hup", "hy", "hye", "hz", "ia", "iba", "ibo", "ice", "id", "ido", "ie", "ig", "iii",
    "ijo", "ik", "iku", "ile", "ilo", "in", "ina", "inc", "ind", "ine", "inh", "io", "ipk", "ira",
    "iro", "is", "isl", "it", "ita", "iu", "iw", "ja", "jam", "jav", "jbo", "ji", "jpn", "jpr",
    "jrb", "jv", "ka", "kaa", "kab", "kac", "kal", "kam", "kan", "kar", "kas", "kat", "kau", "kaw",
    "kaz", "kbd", "kg", "kga", "kha", "khi", "khm", "kho", "ki", "kik", "kin", "kir", "kj", "kk",
    "kl", "kmb", "kmr", "kn", "ko", "kok", "kom", "kon", "kor", "kos", "kpe", "krc", "krl", "kro",
    "kru", "ks", "ku", "kua", "kum", "kur", "kut", "kv", "kw", "ky", "la", "lad", "lah", "lak",
    "lam", "lan", "lao", "lat", "lav", "lb", "lez", "lg", "li", "lim", "lin", "lit", "ln", "lo",
    "lol", "loz", "lt", "ltz", "lu", "lua", "lub", "lug", "lui", "lun", "luo", "lus", "lv", "ma",
    "mac", "mad", "mag", "mah", "mai", "mak", "mal", "man", "mao", "map", "mar", "mas", "may",
    "mdf", "mdr", "men", "mga", "mh", "mic", "min", "mis", "mk", "mkd", "mkh", "ml", "mlg", "mlt",
    "mn", "mnc", "mni", "mno", "mo", "moh", "mon", "mos", "mr", "mri", "ms", "msa", "mt", "mul",
    "mun", "mus", "mwl", "mwr", "my", "mya", "myn", "myv", "na", "nah", "nai", "nap", "nau", "nav",
    "nb", "nbl", "nd", "nde", "ndl", "ndo", "nds", "ne", "nep", "new", "ng", "nia", "nic", "niu",
    "nl", "nld", "nn", "nno", "no", "nob", "nog", "non", "nor", "nqo", "nso", "nub", "nwc", "ny",
    "nya", "nym", "nyn", "nyo", "nzi", "oc", "oci", "oj", "oji", "om", "or", "ora", "ori", "orm",
    "os", "osa", "oss", "ota", "oto", "pa", "paa", "pag", "pal", "pam", "pan", "pap", "pau", "peo",
    "per", "phi", "phn", "pi", "pl", "pli", "pol", "pon", "por", "pra", "pro", "ps", "pt", "pus",
    "qu", "que", "raj", "rap", "rar", "rcf", "rej", "rm", "rn", "ro", "roa", "rom", "ron", "ru",
    "rue", "rug", "run", "rup", "rus", "rw", "sa", "sad", "sag", "sah", "sai", "sal", "sam", "san",
    "sas", "sat", "sc", "scn", "sco", "sd", "se", "sel", "sem", "sg", "sga", "sgn", "shn", "shp",
    "si", "sid", "sin", "sio", "sit", "sk", "sl", "sla", "slk", "slo", "slv", "sm", "sma", "sme",
    "smi", "smj", "smn", "smo", "sms", "sn", "sna", "snd", "snk", "so", "sog", "som", "son", "sot",
    "spa", "sq", "sqi", "sr", "srd", "srn", "srp", "srr", "ss", "ssa", "ssw", "st", "su", "suk",
    "sun", "sus", "sux", "sv", "sw", "swa", "swe", "syc", "syr", "ta", "tah", "tai", "tam", "tat",
    "te", "tel", "tem", "ter", "tet", "tg", "tgk", "tgl", "th", "tha", "ti", "tib", "tig", "tir",
    "tiv", "tk", "tkl", "tl", "tlh", "tli", "tmh", "tn", "to", "tog", "ton", "tpi", "tr", "tsi",
    "tsn", "tso", "tt", "tuk", "tum", "tup", "tur", "tut", "tvl", "tw", "twi", "ty", "tyv", "udm",
    "ug", "uga", "uig", "uk", "ukr", "umb", "und", "ur", "urd", "uz", "uzb", "vai", "ve", "ven",
    "vi", "vie", "vo", "vol", "vot", "wa", "wak", "wal", "war", "was", "wel", "wen", "wln", "wo",
    "wol", "xal", "xh", "xho", "yao", "yap", "yi", "yid", "yo", "yor", "ypk", "za", "zap", "zbl",
    "zen", "zgh", "zh", "zha", "zho", "zu", "zul", "zun", "zxx", "zza",
];

/// Language aliases applied by `BCP47TagValidator.normalize`: ISO 639-2/T and /B forms and
/// deprecated codes mapped to the ISO 639-1 form when one exists.
pub const LANGUAGE_ALIASES: &[(&str, &str)] = &[
    ("aar", "aa"),
    ("abk", "ab"),
    ("afr", "af"),
    ("aka", "ak"),
    ("alb", "sq"),
    ("amh", "am"),
    ("ara", "ar"),
    ("arg", "an"),
    ("arm", "hy"),
    ("asm", "as"),
    ("ava", "av"),
    ("ave", "ae"),
    ("aym", "ay"),
    ("aze", "az"),
    ("bak", "ba"),
    ("bam", "bm"),
    ("baq", "eu"),
    ("bel", "be"),
    ("ben", "bn"),
    ("bis", "bi"),
    ("bod", "bo"),
    ("bos", "bs"),
    ("bre", "br"),
    ("bul", "bg"),
    ("bur", "my"),
    ("cat", "ca"),
    ("ces", "cs"),
    ("cha", "ch"),
    ("che", "ce"),
    ("chi", "zh"),
    ("chu", "cu"),
    ("chv", "cv"),
    ("cor", "kw"),
    ("cos", "co"),
    ("cym", "cy"),
    ("cze", "cs"),
    ("dan", "da"),
    ("deu", "de"),
    ("div", "dv"),
    ("dut", "nl"),
    ("dzo", "dz"),
    ("ell", "el"),
    ("eng", "en"),
    ("epo", "eo"),
    ("est", "et"),
    ("eus", "eu"),
    ("fao", "fo"),
    ("fas", "fa"),
    ("fij", "fj"),
    ("fin", "fi"),
    ("fra", "fr"),
    ("fre", "fr"),
    ("fry", "fy"),
    ("ful", "ff"),
    ("geo", "ka"),
    ("ger", "de"),
    ("gla", "gd"),
    ("gle", "ga"),
    ("glg", "gl"),
    ("glv", "gv"),
    ("gre", "el"),
    ("grn", "gn"),
    ("guj", "gu"),
    ("hat", "ht"),
    ("hau", "ha"),
    ("heb", "he"),
    ("her", "hz"),
    ("hin", "hi"),
    ("hmo", "ho"),
    ("hrv", "hr"),
    ("hun", "hu"),
    ("hye", "hy"),
    ("ibo", "ig"),
    ("ice", "is"),
    ("ido", "io"),
    ("iii", "ii"),
    ("iku", "iu"),
    ("ile", "ie"),
    ("ina", "ia"),
    ("in", "id"),
    ("ind", "id"),
    ("ipk", "ik"),
    ("isl", "is"),
    ("ita", "it"),
    ("iw", "he"),
    ("jav", "jv"),
    ("ji", "yi"),
    ("jpn", "ja"),
    ("jw", "jv"),
    ("kal", "kl"),
    ("kan", "kn"),
    ("kas", "ks"),
    ("kat", "ka"),
    ("kau", "kr"),
    ("kaz", "kk"),
    ("khm", "km"),
    ("kik", "ki"),
    ("kin", "rw"),
    ("kir", "ky"),
    ("kom", "kv"),
    ("kon", "kg"),
    ("kor", "ko"),
    ("kua", "kj"),
    ("kur", "ku"),
    ("lao", "lo"),
    ("lat", "la"),
    ("lav", "lv"),
    ("lim", "li"),
    ("lin", "ln"),
    ("lit", "lt"),
    ("ltz", "lb"),
    ("lub", "lu"),
    ("lug", "lg"),
    ("mac", "mk"),
    ("mah", "mh"),
    ("mal", "ml"),
    ("man", "gv"),
    ("mao", "mi"),
    ("mar", "mr"),
    ("may", "ms"),
    ("mkd", "mk"),
    ("mlg", "mg"),
    ("mlt", "mt"),
    ("mo", "ro"),
    ("mon", "mn"),
    ("mri", "mi"),
    ("msa", "ms"),
    ("mya", "my"),
    ("nau", "na"),
    ("nav", "nv"),
    ("nbl", "nr"),
    ("nde", "nd"),
    ("ndo", "ng"),
    ("nep", "ne"),
    ("nld", "nl"),
    ("nno", "nn"),
    ("nob", "nb"),
    ("nor", "no"),
    ("nya", "ny"),
    ("oci", "oc"),
    ("oji", "oj"),
    ("ori", "or"),
    ("orm", "om"),
    ("oss", "os"),
    ("pan", "pa"),
    ("per", "fa"),
    ("pli", "pi"),
    ("pol", "pl"),
    ("por", "pt"),
    ("pus", "ps"),
    ("que", "qu"),
    ("roh", "rm"),
    ("ron", "ro"),
    ("run", "rn"),
    ("rus", "ru"),
    ("sag", "sg"),
    ("san", "sa"),
    ("sh", "sr"),
    ("sin", "si"),
    ("slk", "sk"),
    ("slo", "sl"),
    ("slv", "sl"),
    ("sme", "se"),
    ("smo", "sm"),
    ("sna", "sn"),
    ("snd", "sd"),
    ("som", "so"),
    ("sot", "st"),
    ("spa", "es"),
    ("sqi", "sq"),
    ("srd", "sc"),
    ("srp", "sr"),
    ("ssw", "ss"),
    ("sun", "su"),
    ("swa", "sw"),
    ("swe", "sv"),
    ("tah", "ty"),
    ("tam", "ta"),
    ("tat", "tt"),
    ("tel", "te"),
    ("tgk", "tg"),
    ("tgl", "tl"),
    ("tha", "th"),
    ("tib", "bo"),
    ("tir", "ti"),
    ("ton", "to"),
    ("tsn", "tn"),
    ("tso", "ts"),
    ("tuk", "tk"),
    ("tur", "tr"),
    ("twi", "tw"),
    ("uig", "ug"),
    ("ukr", "uk"),
    ("urd", "ur"),
    ("uzb", "uz"),
    ("ven", "ve"),
    ("vie", "vi"),
    ("vol", "vo"),
    ("wel", "cy"),
    ("wln", "wa"),
    ("wol", "wo"),
    ("xho", "xh"),
    ("yid", "yi"),
    ("yor", "yo"),
    ("zha", "za"),
    ("zho", "zh"),
    ("zul", "zu"),
];

/// `BCP47TagValidator`: ICU `ULocale.getISOLanguages()` membership check.
pub mod bcp47 {
    /// `isValid`: parses as a language tag and requires the primary language subtag to be a
    /// known ISO language.
    pub fn is_valid(value: &str) -> bool {
        use std::str::FromStr;
        let Ok(locale) = icu_locale::Locale::from_str(value) else {
            return false;
        };
        let language = locale.id.language.as_str();
        !language.is_empty() && super::ISO_LANGUAGES.contains(&language)
    }

    /// `normalize`: canonical BCP47 form (shortest language alias, title-case script,
    /// upper-case region). Parse failure yields "".
    pub fn normalize(value: &str) -> String {
        use std::str::FromStr;
        if value.trim().is_empty() {
            return String::new();
        }
        let Ok(locale) = icu_locale::Locale::from_str(value) else {
            return String::new();
        };
        let id = &locale.id;
        let mut language = id.language.as_str().to_lowercase();
        if let Some((_, alias)) = super::LANGUAGE_ALIASES
            .iter()
            .find(|(from, _)| *from == language)
        {
            language = (*alias).to_string();
        }
        let mut out = language;
        if let Some(script) = id.script {
            let script = script.as_str();
            out.push('-');
            out.push_str(&script[..1].to_uppercase());
            out.push_str(&script[1..].to_lowercase());
        }
        if let Some(region) = id.region {
            out.push('-');
            out.push_str(&region.as_str().to_uppercase());
        }
        for variant in id.variants.iter() {
            out.push('-');
            out.push_str(&variant.as_str().to_lowercase());
        }
        out
    }
}

/// `BCP47TagValidator.isValid`.
pub fn bcp47_is_valid(value: &str) -> bool {
    bcp47::is_valid(value)
}

/// `BCP47TagValidator.normalize`.
pub fn bcp47_normalize(value: &str) -> String {
    bcp47::normalize(value)
}

/// commons-validator `ISBNValidator.validate`: accepts ISBN-10/ISBN-13 (separators allowed),
/// returns the normalized (separator-free) form when the check digit passes.
pub fn isbn_validate(gtin: &str) -> Option<String> {
    let code: String = gtin.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    if code.len() == 10 {
        let mut sum = 0u32;
        for (index, c) in code.chars().enumerate() {
            let digit = match c {
                '0'..='9' => c as u32 - '0' as u32,
                'X' | 'x' if index == 9 => 10,
                _ => return None,
            };
            sum += (10 - index as u32) * digit;
        }
        if sum.is_multiple_of(11) {
            return Some(code.to_uppercase());
        }
        return None;
    }
    if code.len() == 13 && (code.starts_with("978") || code.starts_with("979")) {
        let mut sum = 0u32;
        for (index, c) in code.chars().enumerate() {
            let digit = c.to_digit(10)?;
            sum += if index % 2 == 0 { digit } else { 3 * digit };
        }
        if sum.is_multiple_of(10) {
            return Some(code);
        }
    }
    None
}

/// Provider traits (`MetadataProvider.kt` hierarchy).
pub trait MetadataProvider {
    fn should_library_handle_patch(&self, library: &Library, target: MetadataPatchTarget) -> bool;
}

pub trait BookMetadataProvider: MetadataProvider {
    fn capabilities(&self) -> &BTreeSet<BookMetadataPatchCapability>;
    fn get_book_metadata_from_book(
        &self,
        book_path: &Path,
        media: &Media,
    ) -> Option<BookMetadataPatch>;

    /// Like `get_book_metadata_from_book`, but may reuse the raw metadata documents captured
    /// during analysis (ComicInfo.xml / EPUB OPF bytes) instead of re-opening the book file.
    /// The default implementation ignores the sources and reads the file; providers that
    /// support the handoff override it and fall back to the file when a document is absent.
    fn get_book_metadata_from_book_with_sources(
        &self,
        book_path: &Path,
        media: &Media,
        sources: Option<&crate::CapturedMetadataSources>,
    ) -> Option<BookMetadataPatch> {
        let _ = sources;
        self.get_book_metadata_from_book(book_path, media)
    }
}

pub trait SeriesMetadataFromBookProvider: MetadataProvider {
    fn supports_append_volume(&self) -> bool;
    fn get_series_metadata_from_book(
        &self,
        book_path: &Path,
        media: &Media,
        append_volume_to_title: bool,
    ) -> Option<SeriesMetadataPatch>;
}

pub trait SeriesMetadataProvider: MetadataProvider {
    fn get_series_metadata(&self, series_path: &Path, oneshot: bool)
        -> Option<SeriesMetadataPatch>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::time_codec::{now_utc, parse_date};

    fn book_metadata() -> BookMetadata {
        BookMetadata {
            book_id: "b1".into(),
            title: "old".into(),
            summary: "old summary".into(),
            number: "1".into(),
            number_sort: 1.0,
            release_date: None,
            authors: vec![],
            tags: vec![],
            isbn: String::new(),
            links: vec![],
            title_lock: true,
            summary_lock: false,
            number_lock: false,
            number_sort_lock: false,
            release_date_lock: false,
            authors_lock: false,
            tags_lock: false,
            isbn_lock: false,
            links_lock: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn apply_respects_locks_and_nulls() {
        let metadata = book_metadata();
        let patch = BookMetadataPatch {
            title: Some("new".into()),
            summary: Some("new summary".into()),
            number: None,
            number_sort: Some(2.5),
            ..Default::default()
        };
        let applied = apply_book_patch(&patch, &metadata);
        assert_eq!(applied.title, "old"); // locked
        assert_eq!(applied.summary, "new summary");
        assert_eq!(applied.number, "1"); // null patch never wins
        assert_eq!(applied.number_sort, 2.5);
    }

    #[test]
    fn apply_series_locks() {
        let metadata = SeriesMetadata {
            series_id: "s1".into(),
            status: SeriesStatus::Ongoing,
            title: "old".into(),
            title_sort: "old".into(),
            summary: String::new(),
            reading_direction: None,
            publisher: String::new(),
            age_rating: None,
            language: String::new(),
            genres: BTreeSet::new(),
            tags: BTreeSet::new(),
            total_book_count: None,
            sharing_labels: BTreeSet::new(),
            links: vec![],
            alternate_titles: vec![],
            status_lock: true,
            title_lock: false,
            title_sort_lock: false,
            summary_lock: false,
            reading_direction_lock: false,
            publisher_lock: false,
            age_rating_lock: false,
            language_lock: false,
            genres_lock: false,
            tags_lock: false,
            total_book_count_lock: false,
            sharing_labels_lock: false,
            links_lock: false,
            alternate_titles_lock: false,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        };
        let patch = SeriesMetadataPatch {
            status: Some(SeriesStatus::Ended),
            title: Some("new".into()),
            age_rating: Some(15),
            ..Default::default()
        };
        let applied = apply_series_patch(&patch, &metadata);
        assert_eq!(applied.status, SeriesStatus::Ongoing); // locked
        assert_eq!(applied.title, "new");
        assert_eq!(applied.age_rating, Some(15));
    }

    #[test]
    fn aggregate_picks_and_merges() {
        let mut a = book_metadata();
        a.summary = "first summary".into();
        a.number_sort = 2.0;
        a.number = "2".into();
        a.release_date = parse_date("2020-05-01");
        a.authors = vec![Author::new("A", "writer"), Author::new("B", "artist")];
        a.tags = vec!["x".into()];
        let mut b = book_metadata();
        b.summary = "second".into();
        b.number_sort = 1.0;
        b.number = "1".into();
        b.release_date = parse_date("2019-01-01");
        b.authors = vec![Author::new("A", "writer")];
        b.tags = vec!["y".into()];
        let parts = aggregate(&[a, b]);
        assert_eq!(parts.authors.len(), 2);
        assert_eq!(
            parts.tags,
            ["x".to_string(), "y".to_string()].into_iter().collect()
        );
        assert_eq!(parts.release_date, parse_date("2019-01-01"));
        // lowest numberSort wins the summary
        assert_eq!(parts.summary, "second");
        assert_eq!(parts.summary_number, "1");
    }

    #[test]
    fn aggregate_empty_summary_fallback() {
        let mut blank = book_metadata();
        blank.summary = String::new();
        let parts = aggregate(&[blank]);
        assert_eq!(parts.summary, "");
        assert_eq!(parts.summary_number, "");
    }

    #[test]
    fn most_frequent_tie_breaks_by_first_occurrence() {
        let items = vec!["b", "a", "b", "a"];
        assert_eq!(most_frequent(items, |s| Some(*s)), Some("b"));
        let items: Vec<Option<&str>> = vec![None, Some("x"), None, Some("y"), Some("x")];
        assert_eq!(most_frequent(items, |s| *s), Some("x"));
        assert_eq!(most_frequent(Vec::<i32>::new(), |_| Some(1)), None);
    }

    #[test]
    fn bcp47_is_valid_and_normalize() {
        assert!(bcp47::is_valid("en"));
        assert!(bcp47::is_valid("fra"));
        assert!(bcp47::is_valid("JA"));
        assert!(bcp47::is_valid("iw"));
        assert!(!bcp47::is_valid("japanese"));
        assert!(!bcp47::is_valid(""));
        assert!(!bcp47::is_valid("xx-notalanguage"));

        assert_eq!(bcp47::normalize("en"), "en");
        assert_eq!(bcp47::normalize("fra"), "fr");
        assert_eq!(bcp47::normalize("fra-be"), "fr-BE");
        assert_eq!(bcp47::normalize("JA"), "ja");
        assert_eq!(bcp47::normalize("en-us"), "en-US");
        assert_eq!(bcp47::normalize("iw"), "he");
        assert_eq!(bcp47::normalize(""), "");
    }

    #[test]
    fn isbn_validation() {
        assert_eq!(
            isbn_validate("9783440077894").as_deref(),
            Some("9783440077894")
        );
        assert_eq!(
            isbn_validate("978-3-16-148410-0").as_deref(),
            Some("9783161484100")
        );
        assert_eq!(
            isbn_validate("979-10-90636-07-1").as_deref(),
            Some("9791090636071")
        );
        assert_eq!(
            isbn_validate("0-306-40615-2").as_deref(),
            Some("0306406152")
        );
        assert_eq!(
            isbn_validate("0-8044-2957-X").as_deref(),
            Some("080442957X")
        );
        assert!(isbn_validate("9783440077895").is_none()); // bad check digit
        assert!(isbn_validate("1234567890").is_none()); // ISBN-10 with bad prefix/check
        assert!(isbn_validate("9783161484100X").is_none());
        assert!(isbn_validate("").is_none());
    }
}
