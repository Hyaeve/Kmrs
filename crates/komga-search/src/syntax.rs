//! kmrs full-text query syntax: whitespace-separated terms ANDed together, with
//! komga's `field:` scoping as the only operator. Everything else is literal
//! text — `~`, `*`, `^`, quotes and parens are analyzed like any other
//! characters, so a pasted title can never become a query error or an empty
//! result.
//!
//! - `field:term` scopes the term to one index field (`status:ongoing`);
//! - `field:(a b)` ANDs several terms under the field, `field:(a OR b)` ORs
//!   them, and `field:"a b"` is the spaced AND form — grouping exists only as
//!   part of field syntax;
//! - a term that analyzes to zero tokens (pure punctuation) is dropped from the
//!   query instead of failing it.

use komga_core::task::LuceneEntity;
use tantivy::query::{BooleanQuery, EmptyQuery, FuzzyTermQuery, Occur, Query, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, Schema};
use tantivy::Term;

use crate::analyzer;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Term { field: Option<String>, text: String },
    And(Vec<Node>),
    Or(Vec<Node>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnknownFieldError(pub String);

impl std::fmt::Display for UnknownFieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for UnknownFieldError {}

/// Parses the input into an AND of clauses. The parser never fails: anything
/// that is not a recognized `field:` form is literal text.
pub fn parse(input: &str) -> Node {
    let mut parser = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    let mut clauses = vec![];
    loop {
        parser.skip_ws();
        if parser.peek().is_none() {
            break;
        }
        let start = parser.pos;
        clauses.push(parser.parse_clause());
        // a `field:` prefix followed by whitespace consumes nothing after the
        // colon (`title: foo`) — step over it so the loop cannot stall
        if parser.pos == start {
            parser.pos += 1;
        }
    }
    Node::And(clauses)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn skip_ws(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn parse_clause(&mut self) -> Node {
        let field = self.try_field_prefix();
        // grouping exists only as part of field syntax; anywhere else parens
        // and quotes are literal word characters
        let Some(field) = field else {
            return Node::Term {
                field: None,
                text: self.read_word(),
            };
        };
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                self.parse_group(field)
            }
            Some('"') => {
                self.pos += 1;
                Node::Term {
                    field: Some(field),
                    text: self.read_quoted(),
                }
            }
            _ => Node::Term {
                field: Some(field),
                text: self.read_word(),
            },
        }
    }

    /// `name:` glued to the following element (no whitespace); the name is
    /// alphanumeric, `_` or `.`
    fn try_field_prefix(&mut self) -> Option<String> {
        let save = self.pos;
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '.' {
                name.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if !name.is_empty() && self.peek() == Some(':') {
            self.pos += 1;
            Some(name)
        } else {
            self.pos = save;
            None
        }
    }

    /// Group body: whitespace-separated words; `OR` between words makes the
    /// group a disjunction, `AND` is a plain separator. An unclosed group runs
    /// to the end of input.
    fn parse_group(&mut self, field: String) -> Node {
        let mut ors: Vec<Node> = vec![];
        let mut ands: Vec<Node> = vec![];
        loop {
            self.skip_ws();
            match self.peek() {
                None => break,
                Some(')') => {
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            if self.eat_keyword("OR") {
                if !ands.is_empty() {
                    ors.push(and_of(std::mem::take(&mut ands)));
                }
                continue;
            }
            if self.eat_keyword("AND") {
                continue;
            }
            let start = self.pos;
            let text = self.read_group_word();
            if text.is_empty() {
                if self.pos == start {
                    self.pos += 1;
                }
                continue;
            }
            ands.push(Node::Term {
                field: Some(field.clone()),
                text,
            });
        }
        if !ands.is_empty() {
            ors.push(and_of(ands));
        }
        or_of(ors)
    }

    /// Keyword bounded by whitespace or `)` (Lucene's uppercase operators)
    fn eat_keyword(&mut self, word: &str) -> bool {
        let word_chars: Vec<char> = word.chars().collect();
        if self.chars.len() >= self.pos + word_chars.len()
            && self.chars[self.pos..self.pos + word_chars.len()] == word_chars[..]
            && self
                .chars
                .get(self.pos + word_chars.len())
                .is_none_or(|c| c.is_whitespace() || *c == ')')
        {
            self.pos += word_chars.len();
            return true;
        }
        false
    }

    /// A double-quoted run; an unclosed quote takes the rest of the input
    fn read_quoted(&mut self) -> String {
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.pos += 1;
                break;
            }
            text.push(c);
            self.pos += 1;
        }
        text
    }

    /// Everything up to the next whitespace, fully literal
    fn read_word(&mut self) -> String {
        self.read_until(|c| c.is_whitespace())
    }

    /// Group words end at the group's closing paren as well
    fn read_group_word(&mut self) -> String {
        self.read_until(|c| c.is_whitespace() || c == ')')
    }

    fn read_until(&mut self, stop: impl Fn(char) -> bool) -> String {
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if stop(c) {
                break;
            }
            text.push(c);
            self.pos += 1;
        }
        text
    }
}

fn and_of(mut nodes: Vec<Node>) -> Node {
    match nodes.len() {
        1 => nodes.pop().unwrap(),
        _ => Node::And(nodes),
    }
}

fn or_of(mut nodes: Vec<Node>) -> Node {
    match nodes.len() {
        1 => nodes.pop().unwrap(),
        _ => Node::Or(nodes),
    }
}

pub fn default_fields(entity: LuceneEntity) -> &'static [&'static str] {
    match entity {
        LuceneEntity::Book => &["title", "isbn"],
        LuceneEntity::Series => &["title"],
        LuceneEntity::Collection => &["name"],
        LuceneEntity::ReadList => &["name"],
    }
}

/// The original text of a query made only of unqualified terms — the shape a
/// plain search-box string parses to. The recall fallbacks (prefix, fuzzy)
/// apply only to this shape; field-scoped queries stay exact.
pub(crate) fn unqualified_text(node: &Node) -> Option<String> {
    let Node::And(nodes) = node else {
        return None;
    };
    let mut text = String::new();
    for node in nodes {
        match node {
            Node::Term {
                field: None,
                text: t,
            } => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(t);
            }
            _ => return None,
        }
    }
    if text.is_empty() {
        return None;
    }
    Some(text)
}

/// Builds the tantivy query for a parsed AST, with `defaultFields` fan-out for
/// field-less clauses and search-side analysis for terms. A clause that
/// analyzes to zero tokens is dropped; `Ok(None)` means every clause was
/// dropped and there is nothing to search.
pub fn build_query(
    node: &Node,
    entity: LuceneEntity,
    schema: &Schema,
) -> Result<Option<Box<dyn Query>>, UnknownFieldError> {
    match node {
        Node::Term { field, text } => {
            let tokens = analyzer::search_analyze(text);
            if tokens.is_empty() {
                return Ok(None);
            }
            Ok(Some(build_term_query(field, entity, schema, &tokens)?))
        }
        Node::And(nodes) => build_boolean(nodes, Occur::Must, entity, schema),
        Node::Or(nodes) => build_boolean(nodes, Occur::Should, entity, schema),
    }
}

fn build_boolean(
    nodes: &[Node],
    occur: Occur,
    entity: LuceneEntity,
    schema: &Schema,
) -> Result<Option<Box<dyn Query>>, UnknownFieldError> {
    let mut clauses = vec![];
    for node in nodes {
        if let Some(query) = build_query(node, entity, schema)? {
            clauses.push((occur, query));
        }
    }
    Ok(match clauses.len() {
        0 => None,
        1 => Some(clauses.pop().unwrap().1),
        _ => Some(Box::new(BooleanQuery::new(clauses))),
    })
}

/// The fuzzy fallback for a bare search-box query that matched nothing: every
/// analyzed token becomes an ANDed clause, with fuzzy-eligible tokens relaxed
/// to `FuzzyTermQuery`. `None` when no token is fuzzy-eligible, so all-CJK and
/// all-short queries stay exact.
pub(crate) fn build_fuzzy_query(
    tokens: &[String],
    entity: LuceneEntity,
    schema: &Schema,
) -> Option<Box<dyn Query>> {
    if tokens.is_empty() || !tokens.iter().any(|t| analyzer::fuzzy_distance(t) > 0) {
        return None;
    }
    let mut clauses = vec![];
    for token in tokens {
        let distance = analyzer::fuzzy_distance(token);
        let query = per_field(&None, entity, schema, |f| {
            let term = Term::from_field_text(f, token);
            match distance {
                0 => Box::new(TermQuery::new(
                    term,
                    IndexRecordOption::WithFreqsAndPositions,
                )) as Box<dyn Query>,
                d => Box::new(FuzzyTermQuery::new(term, d, true)) as Box<dyn Query>,
            }
        })
        .expect("default fields always exist");
        clauses.push((Occur::Must, query));
    }
    Some(Box::new(BooleanQuery::new(clauses)))
}

/// Analyzed-term query for pre-tokenized input (`build_query` analyzes the raw text
/// itself; the recall fallbacks reuse this on token prefixes).
pub(crate) fn build_term_query(
    field: &Option<String>,
    entity: LuceneEntity,
    schema: &Schema,
    tokens: &[String],
) -> Result<Box<dyn Query>, UnknownFieldError> {
    per_field(field, entity, schema, |f| {
        let terms: Vec<Term> = tokens.iter().map(|t| Term::from_field_text(f, t)).collect();
        match terms.len() {
            0 => Box::new(EmptyQuery) as Box<dyn Query>,
            1 => Box::new(TermQuery::new(
                terms.into_iter().next().unwrap(),
                IndexRecordOption::WithFreqsAndPositions,
            )),
            _ => Box::new(BooleanQuery::new(
                terms
                    .into_iter()
                    .map(|t| {
                        (
                            Occur::Must,
                            Box::new(TermQuery::new(t, IndexRecordOption::WithFreqsAndPositions))
                                as Box<dyn Query>,
                        )
                    })
                    .collect(),
            )),
        }
    })
}

/// Fan-out over `defaultFields` for field-less clauses (Lucene MultiFieldQueryParser);
/// a present field scopes the query to it, without any existence check.
fn per_field(
    field: &Option<String>,
    entity: LuceneEntity,
    schema: &Schema,
    make: impl Fn(Field) -> Box<dyn Query>,
) -> Result<Box<dyn Query>, UnknownFieldError> {
    match field {
        Some(name) => {
            let f = schema
                .get_field(name)
                .map_err(|_| UnknownFieldError(format!("unknown field: {name}")))?;
            Ok(make(f))
        }
        None => {
            let fields = default_fields(entity);
            if fields.len() == 1 {
                let f = schema.get_field(fields[0]).expect("schema field");
                return Ok(make(f));
            }
            let clauses = fields
                .iter()
                .map(|name| {
                    (
                        Occur::Should,
                        make(schema.get_field(name).expect("schema field")),
                    )
                })
                .collect();
            Ok(Box::new(BooleanQuery::new(clauses)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(field: Option<&str>, text: &str) -> Node {
        Node::Term {
            field: field.map(str::to_string),
            text: text.to_string(),
        }
    }

    #[test]
    fn whitespace_terms_are_anded() {
        assert_eq!(parse("berserk"), Node::And(vec![term(None, "berserk")]));
        assert_eq!(
            parse("berserk vol"),
            Node::And(vec![term(None, "berserk"), term(None, "vol")])
        );
    }

    #[test]
    fn field_scoping() {
        assert_eq!(
            parse("title:berserk"),
            Node::And(vec![term(Some("title"), "berserk")])
        );
        assert_eq!(
            parse("author:(sean murphy)"),
            Node::And(vec![Node::And(vec![
                term(Some("author"), "sean"),
                term(Some("author"), "murphy")
            ])])
        );
        assert_eq!(
            parse("status:(error OR unsupported)"),
            Node::And(vec![Node::Or(vec![
                term(Some("status"), "error"),
                term(Some("status"), "unsupported")
            ])])
        );
        assert_eq!(
            parse("tag:\"double pages\""),
            Node::And(vec![term(Some("tag"), "double pages")])
        );
    }

    #[test]
    fn operators_are_literal_text() {
        for (input, text) in [
            ("berserk~", "berserk~"),
            ("ber*", "ber*"),
            ("berserk^2", "berserk^2"),
            ("title(2)", "title(2)"),
            ("a AND b", "a AND b"),
        ] {
            let expected: Vec<Node> = text.split(' ').map(|t| term(None, t)).collect();
            assert_eq!(parse(input), Node::And(expected), "input {input}");
        }
    }

    #[test]
    fn malformed_input_is_literal_text() {
        // parens and quotes without a field prefix are plain word characters
        assert_eq!(
            parse("(a b"),
            Node::And(vec![term(None, "(a"), term(None, "b")])
        );
        assert_eq!(
            parse("\"a b"),
            Node::And(vec![term(None, "\"a"), term(None, "b")])
        );
        // a field group left open runs to the end of input
        assert_eq!(parse("title:(a"), Node::And(vec![term(Some("title"), "a")]));
        // a stray closing paren is just a word
        assert_eq!(parse("a)"), Node::And(vec![term(None, "a)")]));
    }

    #[test]
    fn group_keywords_need_word_boundaries() {
        // OR glued to a word is part of the word
        assert_eq!(
            parse("status:(a ORb)"),
            Node::And(vec![Node::And(vec![
                term(Some("status"), "a"),
                term(Some("status"), "ORb")
            ])])
        );
        // lowercase or is not a keyword
        assert_eq!(
            parse("status:(a or b)"),
            Node::And(vec![Node::And(vec![
                term(Some("status"), "a"),
                term(Some("status"), "or"),
                term(Some("status"), "b")
            ])])
        );
    }

    #[test]
    fn unqualified_text_shape() {
        assert_eq!(
            unqualified_text(&parse("berserk vol")),
            Some("berserk vol".to_string())
        );
        // literal quotes and parens stay part of the words
        assert_eq!(
            unqualified_text(&parse("\"quoted words\"")),
            Some("\"quoted words\"".to_string())
        );
        assert_eq!(
            unqualified_text(&parse("(berserk vol)")),
            Some("(berserk vol)".to_string())
        );
        assert_eq!(unqualified_text(&parse("title:berserk")), None);
        assert_eq!(unqualified_text(&parse("berserk title:vol")), None);
        assert_eq!(unqualified_text(&parse("title:(a b)")), None);
    }
}
