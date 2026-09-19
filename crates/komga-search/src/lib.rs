//! Search: tantivy index and Lucene syntax compatibility layer.
//!
//! Ports komga's `LuceneHelper`/`LuceneEntity`/`LuceneConfiguration` to tantivy 0.26:
//! one index for all four entity kinds, a `type` keyword field distinguishing them,
//! stored id fields, and an `index_version` marker document.

pub mod analyzer;
pub mod syntax;

use komga_core::task::LuceneEntity;
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, TermQuery};
use tantivy::schema::document::Value as _;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, STORED, STRING,
};
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument, Term};

pub use analyzer::KomgaIndexTokenizer;

const TYPE_FIELD: &str = "type";
const INDEX_VERSION_FIELD: &str = "index_version";
const INDEX_VERSION_TYPE: &str = "index_version";
const MAX_RESULTS: usize = 1000;
const INDEX_TOKENIZER: &str = "komga";

/// komga's `LuceneCommitter` is synchronous: every write is committed and becomes
/// searchable immediately.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Tantivy(#[from] tantivy::TantivyError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn entity_type_str(entity: LuceneEntity) -> &'static str {
    match entity {
        LuceneEntity::Book => "book",
        LuceneEntity::Series => "series",
        LuceneEntity::Collection => "collection",
        LuceneEntity::ReadList => "readlist",
    }
}

pub fn entity_id_field(entity: LuceneEntity) -> &'static str {
    match entity {
        LuceneEntity::Book => "book_id",
        LuceneEntity::Series => "series_id",
        LuceneEntity::Collection => "collection_id",
        LuceneEntity::ReadList => "readlist_id",
    }
}

/// Text fields of the entity documents (`LuceneEntity.kt` toDocument), including the
/// ComicInfo author roles, which Lucene indexes under the role's own field name.
const TEXT_FIELDS: &[&str] = &[
    "title",
    "isbn",
    "name",
    "tag",
    "series_tag",
    "book_tag",
    "author",
    "writer",
    "penciller",
    "inker",
    "colorist",
    "letterer",
    "cover",
    "editor",
    "translator",
    "publisher",
    "status",
    "reading_direction",
    "age_rating",
    "language",
    "genre",
    "sharing_label",
    "total_book_count",
    "book_count",
    "release_date",
    "deleted",
    "oneshot",
    "complete",
];

fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    let text_options = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(INDEX_TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    for name in TEXT_FIELDS {
        builder.add_text_field(name, text_options.clone());
    }
    // keyword fields: indexed raw, not analyzed
    builder.add_text_field(TYPE_FIELD, STRING);
    builder.add_text_field(INDEX_VERSION_FIELD, STRING | STORED);
    builder.add_text_field("book_id", STRING | STORED);
    builder.add_text_field("series_id", STRING | STORED);
    builder.add_text_field("collection_id", STRING | STORED);
    builder.add_text_field("readlist_id", STRING | STORED);
    builder.build()
}

/// A document to index; `fields` keeps insertion order and allows repeated (multi-valued) entries.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityDoc {
    pub entity: LuceneEntity,
    pub id: String,
    pub fields: Vec<(String, String)>,
}

impl EntityDoc {
    fn to_tantivy(&self, schema: &Schema) -> TantivyDocument {
        let mut doc = TantivyDocument::new();
        for (name, value) in &self.fields {
            match schema.get_field(name) {
                Ok(field) => doc.add_text(field, value),
                // Lucene accepts dynamic fields (e.g. arbitrary author roles); tantivy's
                // static schema cannot, so unknown fields are dropped with a warning
                Err(_) => {
                    tracing::warn!(
                        "skipping unindexed field {name} of {} {}",
                        entity_type_str(self.entity),
                        self.id
                    )
                }
            }
        }
        doc.add_text(
            schema.get_field(TYPE_FIELD).unwrap(),
            entity_type_str(self.entity),
        );
        doc.add_text(
            schema.get_field(entity_id_field(self.entity)).unwrap(),
            &self.id,
        );
        doc
    }
}

/// The dto_dao-facing search contract (`komga_db::dto_dao::EntitySearcher` has the same shape;
/// the server adapts between them).
pub trait EntitySearcher: Send + Sync {
    /// `None` means no filtering (blank term); `Some(ids)` filters, possibly to nothing.
    fn search_entity_ids(&self, term: Option<&str>, entity: LuceneEntity) -> Option<Vec<String>>;
}

pub struct SearchIndex {
    index: Index,
    /// `IndexWriter` is not `Clone` in tantivy 0.26, and `commit` needs `&mut`
    writer: std::sync::Mutex<IndexWriter>,
    reader: IndexReader,
}

impl SearchIndex {
    /// Opens an existing index or creates it (the directory is created when missing).
    pub fn open(dir: &Path) -> Result<Self> {
        let index = match Index::open_in_dir(dir) {
            Ok(index) => index,
            Err(_) => {
                std::fs::create_dir_all(dir)?;
                Index::create_in_dir(dir, build_schema())?
            }
        };
        index
            .tokenizers()
            .register(INDEX_TOKENIZER, KomgaIndexTokenizer);
        let writer = std::sync::Mutex::new(index.writer(50_000_000)?);
        let reader = index.reader()?;
        Ok(Self {
            index,
            writer,
            reader,
        })
    }

    /// `DirectoryReader.indexExists`
    pub fn exists(dir: &Path) -> bool {
        Index::open_in_dir(dir).is_ok()
    }

    /// Version stored in the `index_version` marker document; defaults to 1 like the Java side.
    pub fn index_version(&self) -> i32 {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.field(TYPE_FIELD), INDEX_VERSION_TYPE),
            IndexRecordOption::WithFreqs,
        );
        searcher
            .search(&query, &TopDocs::with_limit(1).order_by_score())
            .ok()
            .and_then(|top| top.into_iter().next())
            .and_then(|(_, addr)| searcher.doc::<TantivyDocument>(addr).ok())
            .and_then(|doc| {
                doc.get_first(self.field(INDEX_VERSION_FIELD))
                    .and_then(|v| v.as_value().as_str().map(str::to_string))
            })
            .and_then(|s| s.parse().ok())
            .unwrap_or(1)
    }

    /// `setIndexVersion`: replace the marker document
    pub fn set_index_version(&self, version: i32) -> Result<()> {
        self.writer
            .lock()
            .unwrap()
            .delete_term(Term::from_field_text(
                self.field(TYPE_FIELD),
                INDEX_VERSION_TYPE,
            ));
        let mut doc = TantivyDocument::new();
        doc.add_text(self.field(TYPE_FIELD), INDEX_VERSION_TYPE);
        doc.add_text(self.field(INDEX_VERSION_FIELD), version.to_string());
        self.writer.lock().unwrap().add_document(doc)?;
        self.commit_and_reload()
    }

    /// `searchEntitiesIds`: parse `"<term> *:*"` in Lucene syntax, require the entity type,
    /// return up to 1000 ids in score order. Blank terms mean "no filtering" (None);
    /// parse failures yield an empty list, like Lucene's ParseException path.
    pub fn search_entity_ids(
        &self,
        term: Option<&str>,
        entity: LuceneEntity,
    ) -> Option<Vec<String>> {
        let term = term.filter(|t| !t.trim().is_empty())?;
        let Ok(ast) = syntax::parse(&format!("{term} *:*")) else {
            return Some(vec![]);
        };
        let fields_query = match syntax::build_query(&ast, entity, &self.index.schema()) {
            Ok(query) => query,
            Err(_) => return Some(vec![]),
        };
        let type_query = TermQuery::new(
            Term::from_field_text(self.field(TYPE_FIELD), entity_type_str(entity)),
            IndexRecordOption::WithFreqs,
        );
        let boolean = BooleanQuery::new(vec![
            (Occur::Must, fields_query),
            (Occur::Must, Box::new(type_query)),
        ]);
        let searcher = self.reader.searcher();
        let id_field = self.field(entity_id_field(entity));
        let top =
            match searcher.search(&boolean, &TopDocs::with_limit(MAX_RESULTS).order_by_score()) {
                Ok(top) => top,
                Err(e) => {
                    tracing::error!("error fetching entities from index: {e}");
                    return Some(vec![]);
                }
            };
        Some(
            top.into_iter()
                .filter_map(|(_, addr)| searcher.doc::<TantivyDocument>(addr).ok())
                .filter_map(|doc| {
                    doc.get_first(id_field)
                        .and_then(|v| v.as_value().as_str().map(str::to_string))
                })
                .collect(),
        )
    }

    pub fn add_documents(&self, docs: Vec<EntityDoc>) -> Result<()> {
        let schema = self.index.schema();
        for doc in docs {
            self.writer
                .lock()
                .unwrap()
                .add_document(doc.to_tantivy(&schema))?;
        }
        self.commit_and_reload()
    }

    /// Lucene's `updateDocument(term, doc)`: delete by id, then add, in one commit
    pub fn update_document(&self, entity: LuceneEntity, id: &str, doc: EntityDoc) -> Result<()> {
        self.writer
            .lock()
            .unwrap()
            .delete_term(Term::from_field_text(
                self.field(entity_id_field(entity)),
                id,
            ));
        self.writer
            .lock()
            .unwrap()
            .add_document(doc.to_tantivy(&self.index.schema()))?;
        self.commit_and_reload()
    }

    pub fn delete_documents(&self, entity: LuceneEntity, id: &str) -> Result<()> {
        self.writer
            .lock()
            .unwrap()
            .delete_term(Term::from_field_text(
                self.field(entity_id_field(entity)),
                id,
            ));
        self.commit_and_reload()
    }

    /// `rebuildIndex` first wipes every document of the entity type
    pub fn delete_entity_type(&self, entity: LuceneEntity) -> Result<()> {
        self.writer
            .lock()
            .unwrap()
            .delete_term(Term::from_field_text(
                self.field(TYPE_FIELD),
                entity_type_str(entity),
            ));
        self.commit_and_reload()
    }

    /// Lucene's `IndexUpgrader` upgrades the on-disk codec; tantivy has no such concept
    /// (format changes are handled by reindexing), so this is a no-op.
    pub fn upgrade(&self) {
        tracing::info!("tantivy index requires no codec upgrade");
    }

    fn commit_and_reload(&self) -> Result<()> {
        self.writer.lock().unwrap().commit()?;
        self.reader.reload()?;
        Ok(())
    }

    fn field(&self, name: &str) -> Field {
        self.index.schema().get_field(name).expect("schema field")
    }
}

impl EntitySearcher for SearchIndex {
    fn search_entity_ids(&self, term: Option<&str>, entity: LuceneEntity) -> Option<Vec<String>> {
        self.search_entity_ids(term, entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> (tempfile::TempDir, SearchIndex) {
        let dir = tempfile::tempdir().unwrap();
        let index = SearchIndex::open(dir.path()).unwrap();
        (dir, index)
    }

    fn book(id: &str, fields: &[(&str, &str)]) -> EntityDoc {
        EntityDoc {
            entity: LuceneEntity::Book,
            id: id.to_string(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    fn seed(index: &SearchIndex) {
        index
            .add_documents(vec![
                book(
                    "b1",
                    &[
                        ("title", "Berserk Volume 1"),
                        ("isbn", "9781593070205"),
                        ("author", "Kentaro Miura"),
                        ("writer", "Kentaro Miura"),
                        ("tag", "seinen"),
                    ],
                ),
                book(
                    "b2",
                    &[
                        ("title", "Solo Leveling"),
                        ("isbn", "9781975319278"),
                        ("author", "Chugong"),
                        ("writer", "Chugong"),
                    ],
                ),
                book("b3", &[("title", "東京クライシス"), ("author", "誰か")]),
            ])
            .unwrap();
    }

    #[test]
    fn search_by_default_fields() {
        let (_dir, index) = index();
        seed(&index);
        let ids = index
            .search_entity_ids(Some("berserk"), LuceneEntity::Book)
            .unwrap();
        assert_eq!(ids, vec!["b1"]);
        // isbn is a default field for books
        let ids = index
            .search_entity_ids(Some("9781593070205"), LuceneEntity::Book)
            .unwrap();
        assert_eq!(ids, vec!["b1"]);
        // author is NOT a default field: a bare term does not reach it
        let ids = index
            .search_entity_ids(Some("miura"), LuceneEntity::Book)
            .unwrap();
        assert!(ids.is_empty());
        // ...but it is reachable when named
        let ids = index
            .search_entity_ids(Some("author:miura"), LuceneEntity::Book)
            .unwrap();
        assert_eq!(ids, vec!["b1"]);
    }

    #[test]
    fn search_prefix_phrase_wildcard() {
        let (_dir, index) = index();
        seed(&index);
        let ids = index
            .search_entity_ids(Some("ber*"), LuceneEntity::Book)
            .unwrap();
        assert_eq!(ids, vec!["b1"]);
        // phrase positions advance per emitted n-gram (Lucene NGramTokenFilter semantics),
        // so a multi-word phrase on a title does not match — same as komga
        assert!(index
            .search_entity_ids(Some("\"berserk volume\""), LuceneEntity::Book)
            .unwrap()
            .is_empty());
        // a CJK phrase aligned with the document's bigram positions matches
        let ids = index
            .search_entity_ids(Some("\"クライシス\""), LuceneEntity::Book)
            .unwrap();
        assert_eq!(ids, vec!["b3"]);
        let ids = index
            .search_entity_ids(Some("b*rk"), LuceneEntity::Book)
            .unwrap();
        assert_eq!(ids, vec!["b1"]);
    }

    #[test]
    fn search_cjk() {
        let (_dir, index) = index();
        seed(&index);
        // the trailing unigram of the bigram chain becomes an AND operand and is not
        // indexed, so single-word CJK queries do not match — same as Lucene
        assert!(index
            .search_entity_ids(Some("東京"), LuceneEntity::Book)
            .unwrap()
            .is_empty());
        let ids = index
            .search_entity_ids(Some("クライシス"), LuceneEntity::Book)
            .unwrap();
        assert_eq!(ids, vec!["b3"]);
    }

    #[test]
    fn blank_term_and_parse_error() {
        let (_dir, index) = index();
        seed(&index);
        assert!(index.search_entity_ids(None, LuceneEntity::Book).is_none());
        assert!(index
            .search_entity_ids(Some("  "), LuceneEntity::Book)
            .is_none());
        assert_eq!(
            index
                .search_entity_ids(Some("*foo"), LuceneEntity::Book)
                .unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn entity_type_isolation() {
        let (_dir, index) = index();
        seed(&index);
        index
            .add_documents(vec![EntityDoc {
                entity: LuceneEntity::Collection,
                id: "c1".into(),
                fields: vec![("name".into(), "Berserk".into())],
            }])
            .unwrap();
        assert_eq!(
            index
                .search_entity_ids(Some("berserk"), LuceneEntity::Collection)
                .unwrap(),
            vec!["c1"]
        );
        assert_eq!(
            index
                .search_entity_ids(Some("berserk"), LuceneEntity::ReadList)
                .unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn update_and_delete() {
        let (_dir, index) = index();
        seed(&index);
        index
            .update_document(
                LuceneEntity::Book,
                "b1",
                book("b1", &[("title", "Berserk Deluxe")]),
            )
            .unwrap();
        assert!(index
            .search_entity_ids(Some("volume"), LuceneEntity::Book)
            .unwrap()
            .is_empty());
        assert_eq!(
            index
                .search_entity_ids(Some("deluxe"), LuceneEntity::Book)
                .unwrap(),
            vec!["b1"]
        );
        index.delete_documents(LuceneEntity::Book, "b1").unwrap();
        assert!(index
            .search_entity_ids(Some("deluxe"), LuceneEntity::Book)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn delete_entity_type_for_rebuild() {
        let (_dir, index) = index();
        seed(&index);
        index.delete_entity_type(LuceneEntity::Book).unwrap();
        assert!(index
            .search_entity_ids(Some("berserk"), LuceneEntity::Book)
            .unwrap()
            .is_empty());
        index
            .add_documents(vec![book("b9", &[("title", "New Berserk")])])
            .unwrap();
        assert_eq!(
            index
                .search_entity_ids(Some("berserk"), LuceneEntity::Book)
                .unwrap(),
            vec!["b9"]
        );
    }

    #[test]
    fn version_document() {
        let (_dir, index) = index();
        assert_eq!(index.index_version(), 1);
        index.set_index_version(8).unwrap();
        assert_eq!(index.index_version(), 8);
        // the marker is not an entity and does not leak into entity searches
        assert!(index
            .search_entity_ids(Some("index_version"), LuceneEntity::Book)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn exists_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert!(!SearchIndex::exists(&missing));
        let _index = SearchIndex::open(&missing).unwrap();
        assert!(SearchIndex::exists(&missing));
    }
}
