# Search in kmrs

kmrs ports komga's Lucene search to [tantivy](https://github.com/quickwit-oss/tantivy): the query syntax (`title:berserk`, `tag:seinen AND status:ONGOING`, prefixes, wildcards, phrases) and the multilingual analyzer chain behave like the Java version. This document describes the analyzer chain and the recall extensions kmrs adds on top of it.

## The analyzer chain

Both the index side and the query side share one pipeline (komga's `MultiLingualAnalyzer`); the index side appends an n-gram filter (komga's `MultiLingualNGramAnalyzer`, minGram 3, maxGram 10, preserveOriginal), which is what makes substring matching work:

1. **t2s** — traditional → simplified Chinese conversion (kmrs extension, see below)
2. **standard tokenize** — UAX#29 word segmentation: Han/Hiragana characters one per token, katakana runs stay together, Latin/digit runs
3. **CJK width** — fullwidth ASCII → ASCII, halfwidth katakana → fullwidth
4. **lowercase**
5. **CJK bigram** — sliding bigrams over CJK runs, with boundary unigrams (kmrs extension, see below)
6. **ASCII fold** — accents stripped (`café` → `cafe`)

Prefix and wildcard query terms skip the bigram step, like Java's `MultiLingualAnalyzer.normalize`.

## Extensions over the Java version

These change recall only: the index format is versioned (see below), and nothing in the data directory or the API is affected.

### CJK boundary unigrams

Java's `CJKBigramFilter` indexes a CJK run as sliding bigrams plus a trailing unigram, so a character that only ever appears inside a bigram cannot be searched for. `3月` therefore cannot match `3月的狮子` in the Java version: 月 exists in the index only inside the bigram 月的. kmrs additionally emits the first character of a CJK run that follows a non-CJK token as a unigram, so `3月` matches `3月的狮子`, and single-character queries like `王` find `狮子王` via the trailing unigram.

### Simplified ↔ traditional cross-search

Index and query text are both normalized to simplified Chinese (OpenCC phrase dictionaries, via [opencc-jieba-rs](https://crates.io/crates/opencc-jieba-rs)) before tokenization, so the two scripts cross-match in both directions: a traditional query `名偵探柯南` finds the simplified title `名侦探柯南`, and a simplified query finds traditional titles. Prefix and wildcard queries are covered too.

Two properties of the mapping are worth knowing:

- It is many-to-one (`乾`/`幹` → `干`, `髮`/`發` → `发`), so a few titles can over-merge; phrase-level rules keep the classic cases intact (`乾隆` stays `乾隆`, while `乾燥` → `干燥`).
- Japanese shinjitai kanji collapse to the simplified forms as well (`東京` → `东京`), so Chinese queries also match the kanji part of Japanese titles; kana is untouched.

## Index versioning and rebuilds

The index directory carries an analyzer-version marker (`.kmrs-search-analyzer-version`). On startup:

- version matches — the index is opened as-is;
- version mismatch or missing marker — the index is wiped and rebuilt automatically in the background;
- a Java Lucene index is found — it is wiped and rebuilt the same way.

Files that do not belong to a search index are never deleted, so pointing kmrs at a directory with other content is safe (it just builds the index alongside them).

## Known deviations from the Java version

- Lucene fuzzy (`~`) and phrase-slop (`~N`) queries are unsupported and yield empty results.
- `komga.lucene.index-analyzer.*` and `komga.lucene.commit-delay` are ignored; the analyzer is fixed to the chain described above.
