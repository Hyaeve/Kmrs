//! Lucene classic query syntax subset, in `MultiFieldQueryParser` semantics with
//! `defaultOperator = AND`, as used by `LuceneHelper.searchEntitiesIds`.
//!
//! Supported: terms, `field:` prefixes, quoted phrases, trailing-`*` prefixes, inner wildcards,
//! `AND/OR/NOT` (and `&&/||/!`), `+`/`-` clauses, parentheses, `^` boosts, `*:*` match-all,
//! and `\` escapes. Leading wildcards are a parse error (Lucene's default). Fuzzy `~` and
//! slop `~N` are unsupported and also fail the whole query, matching Lucene's ParseException
//! behavior of yielding an empty result.

use komga_core::task::LuceneEntity;
use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, EmptyQuery, Occur, PhraseQuery, Query, RegexQuery,
    TermQuery,
};
use tantivy::schema::{Field, IndexRecordOption, Schema};
use tantivy::Term;

use crate::analyzer;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Term { field: Option<String>, text: String },
    Phrase { field: Option<String>, text: String },
    Prefix { field: Option<String>, text: String },
    Wildcard { field: Option<String>, text: String },
    MatchAll,
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
    Boost(Box<Node>, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(input: &str) -> Result<Node, ParseError> {
    let mut parser = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    let node = parser.parse_or()?;
    parser.skip_ws();
    if parser.pos < parser.chars.len() {
        return Err(ParseError(format!(
            "unexpected trailing input at {}",
            parser.pos
        )));
    }
    Ok(node)
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

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        let word_chars: Vec<char> = word.chars().collect();
        if self.chars.len() >= self.pos + word_chars.len()
            && self.chars[self.pos..self.pos + word_chars.len()] == word_chars[..]
        {
            // keyword boundary: the next character must not be a term character
            let after = self.chars.get(self.pos + word_chars.len());
            if after.is_none_or(|c| c.is_whitespace() || *c == '(' || *c == ')') {
                self.pos += word_chars.len();
                return true;
            }
        }
        false
    }

    fn parse_or(&mut self) -> Result<Node, ParseError> {
        let mut left = self.parse_and()?;
        loop {
            self.skip_ws();
            if self.eat_word("OR") || (self.eat('|') && self.eat('|')) {
                let right = self.parse_and()?;
                left = match left {
                    Node::Or(mut nodes) => {
                        nodes.push(right);
                        Node::Or(nodes)
                    }
                    _ => Node::Or(vec![left, right]),
                };
            } else {
                return Ok(left);
            }
        }
    }

    fn parse_and(&mut self) -> Result<Node, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_ws();
            if self.peek_word("OR") || self.peek_word("||") {
                return Ok(left);
            }
            if self.eat_word("AND") || (self.eat('&') && self.eat('&')) || self.at_clause_start() {
                let right = self.parse_unary()?;
                left = and_of(left, right);
            } else {
                return Ok(left);
            }
        }
    }

    /// `true` when the input ahead is exactly `word` followed by a keyword boundary
    fn peek_word(&self, word: &str) -> bool {
        let word_chars: Vec<char> = word.chars().collect();
        self.chars.len() >= self.pos + word_chars.len()
            && self.chars[self.pos..self.pos + word_chars.len()] == word_chars[..]
            && self
                .chars
                .get(self.pos + word_chars.len())
                .is_none_or(|c| c.is_whitespace() || *c == '(' || *c == ')')
    }

    fn at_clause_start(&self) -> bool {
        match self.peek() {
            Some('+' | '-' | '!' | '(' | '"') => true,
            Some(c) => !c.is_whitespace() && c != ')',
            None => false,
        }
    }

    fn parse_unary(&mut self) -> Result<Node, ParseError> {
        self.skip_ws();
        if self.eat('+') {
            return self.parse_unary();
        }
        if self.eat('-') || self.eat('!') || self.eat_word("NOT") {
            return Ok(Node::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_clause()
    }

    fn parse_clause(&mut self) -> Result<Node, ParseError> {
        self.skip_ws();
        if self.peek().is_none() {
            return Err(ParseError("unexpected end of query".into()));
        }

        // optional `field:` prefix (no whitespace between the name and the colon)
        let mut field = None;
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
        if !name.is_empty() && self.eat(':') {
            field = Some(name);
        } else {
            self.pos = save;
        }

        if self.eat('(') {
            let inner = self.parse_or()?;
            self.skip_ws();
            if !self.eat(')') {
                return Err(ParseError("unclosed group".into()));
            }
            let inner = match field {
                Some(f) => rescope(inner, &f),
                None => inner,
            };
            return self.parse_boost_tail(inner);
        }

        if self.peek() == Some('"') {
            self.pos += 1;
            let mut text = String::new();
            loop {
                match self.peek() {
                    None => return Err(ParseError("unclosed phrase".into())),
                    Some('\\') => {
                        self.pos += 1;
                        if let Some(c) = self.peek() {
                            text.push(c);
                            self.pos += 1;
                        }
                    }
                    Some('"') => {
                        self.pos += 1;
                        break;
                    }
                    Some(c) => {
                        text.push(c);
                        self.pos += 1;
                    }
                }
            }
            return self.parse_boost_tail(Node::Phrase { field, text });
        }

        // bare term
        let mut text = String::new();
        while let Some(c) = self.peek() {
            match c {
                '\\' => {
                    self.pos += 1;
                    if let Some(escaped) = self.peek() {
                        text.push(escaped);
                        self.pos += 1;
                    }
                }
                c if c.is_whitespace() || c == '(' || c == ')' || c == '"' => break,
                '^' | '~' => break,
                _ => {
                    text.push(c);
                    self.pos += 1;
                }
            }
        }
        if text.is_empty() {
            return Err(ParseError(format!("expected term at {}", self.pos)));
        }

        // fuzzy (`term~` / `term~0.8`) and phrase slop are unsupported
        if self.peek() == Some('~') {
            return Err(ParseError("fuzzy queries are not supported".into()));
        }

        let node = classify_term(field, text)?;
        self.parse_boost_tail(node)
    }

    /// `^` boost suffix; accepted and applied, scores only affect ordering
    fn parse_boost_tail(&mut self, node: Node) -> Result<Node, ParseError> {
        if self.peek() != Some('^') {
            return Ok(node);
        }
        self.pos += 1;
        let mut num = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                num.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        let boost: f32 = num
            .parse()
            .map_err(|_| ParseError(format!("invalid boost: {num}")))?;
        Ok(Node::Boost(Box::new(node), boost))
    }
}

fn classify_term(field: Option<String>, text: String) -> Result<Node, ParseError> {
    if text == "*:*" {
        return Ok(Node::MatchAll);
    }
    let stars = text.chars().filter(|&c| c == '*' || c == '?').count();
    if stars == 0 {
        return Ok(Node::Term { field, text });
    }
    if text == "*" {
        return Ok(Node::Wildcard { field, text });
    }
    if text.starts_with('*') || text.starts_with('?') {
        // Lucene's QueryParser rejects leading wildcards by default
        return Err(ParseError("leading wildcard".into()));
    }
    if text.ends_with('*') && text[..text.len() - 1].chars().all(|c| c != '*' && c != '?') {
        return Ok(Node::Prefix {
            field,
            text: text[..text.len() - 1].to_string(),
        });
    }
    Ok(Node::Wildcard { field, text })
}

/// `field:(a b c)` scopes the group to the field
fn rescope(node: Node, field: &str) -> Node {
    match node {
        Node::Term { text, .. } => Node::Term {
            field: Some(field.to_string()),
            text,
        },
        Node::Phrase { text, .. } => Node::Phrase {
            field: Some(field.to_string()),
            text,
        },
        Node::Prefix { text, .. } => Node::Prefix {
            field: Some(field.to_string()),
            text,
        },
        Node::Wildcard { text, .. } => Node::Wildcard {
            field: Some(field.to_string()),
            text,
        },
        Node::And(nodes) => Node::And(nodes.into_iter().map(|n| rescope(n, field)).collect()),
        Node::Or(nodes) => Node::Or(nodes.into_iter().map(|n| rescope(n, field)).collect()),
        Node::Not(inner) => Node::Not(Box::new(rescope(*inner, field))),
        other => other,
    }
}

fn and_of(left: Node, right: Node) -> Node {
    match left {
        Node::And(mut nodes) => {
            nodes.push(right);
            Node::And(nodes)
        }
        _ => Node::And(vec![left, right]),
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

/// Builds the tantivy query for a parsed AST, with `defaultFields` fan-out for
/// field-less clauses, search-side analysis for terms, and `normalize` for
/// prefix/wildcard terms (Lucene QueryParser semantics).
pub fn build_query(
    node: &Node,
    entity: LuceneEntity,
    schema: &Schema,
) -> Result<Box<dyn Query>, ParseError> {
    Ok(match node {
        Node::MatchAll => Box::new(AllQuery),
        Node::Term { field, text } => {
            let tokens = analyzer::search_analyze(text);
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
                                    Box::new(TermQuery::new(
                                        t,
                                        IndexRecordOption::WithFreqsAndPositions,
                                    )) as Box<dyn Query>,
                                )
                            })
                            .collect(),
                    )),
                }
            })?
        }
        Node::Phrase { field, text } => {
            let tokens = analyzer::search_analyze(text);
            per_field(field, entity, schema, |f| {
                let terms: Vec<Term> = tokens.iter().map(|t| Term::from_field_text(f, t)).collect();
                match terms.len() {
                    0 => Box::new(EmptyQuery) as Box<dyn Query>,
                    // Lucene turns a single-token phrase into a term query
                    1 => Box::new(TermQuery::new(
                        terms.into_iter().next().unwrap(),
                        IndexRecordOption::WithFreqsAndPositions,
                    )),
                    _ => Box::new(PhraseQuery::new(terms)),
                }
            })?
        }
        Node::Prefix { field, text } => {
            let normalized = analyzer::normalize(text);
            per_field(field, entity, schema, |f| {
                Box::new(
                    RegexQuery::from_pattern(&format!("{}.*", regex_escape(&normalized)), f)
                        .expect("escaped prefix is a valid regex"),
                ) as Box<dyn Query>
            })?
        }
        Node::Wildcard { field, text } => {
            let normalized = analyzer::normalize(text);
            let pattern = wildcard_to_regex(&normalized);
            per_field(field, entity, schema, |f| {
                Box::new(
                    RegexQuery::from_pattern(&pattern, f)
                        .expect("wildcard translation always yields a valid regex"),
                ) as Box<dyn Query>
            })?
        }
        Node::Boost(inner, boost) => {
            Box::new(BoostQuery::new(build_query(inner, entity, schema)?, *boost))
        }
        Node::And(nodes) => {
            let clauses = nodes
                .iter()
                .map(|n| Ok((Occur::Must, build_query(n, entity, schema)?)))
                .collect::<Result<Vec<_>, ParseError>>()?;
            Box::new(BooleanQuery::new(clauses))
        }
        Node::Or(nodes) => {
            let clauses = nodes
                .iter()
                .map(|n| Ok((Occur::Should, build_query(n, entity, schema)?)))
                .collect::<Result<Vec<_>, ParseError>>()?;
            Box::new(BooleanQuery::new(clauses))
        }
        Node::Not(inner) => {
            // Lucene rewrites pure-negative queries by adding a MatchAllDocsQuery
            Box::new(BooleanQuery::new(vec![
                (Occur::Must, Box::new(AllQuery)),
                (Occur::MustNot, build_query(inner, entity, schema)?),
            ]))
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
) -> Result<Box<dyn Query>, ParseError> {
    match field {
        Some(name) => {
            let f = schema
                .get_field(name)
                .map_err(|_| ParseError(format!("unknown field: {name}")))?;
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

fn regex_escape(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if "\\^$.|?*+()[]{}".contains(c) {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}

/// Lucene WildcardQuery to regex: `*` -> `.*`, `?` -> `.`, everything else escaped
fn wildcard_to_regex(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            '*' => vec!['.', '*'],
            '?' => vec!['.'],
            _ => regex_escape(&c.to_string()).chars().collect(),
        })
        .collect()
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
    fn single_and_multi_terms() {
        assert_eq!(parse("berserk").unwrap(), term(None, "berserk"));
        assert_eq!(
            parse("berserk vol").unwrap(),
            Node::And(vec![term(None, "berserk"), term(None, "vol")])
        );
    }

    #[test]
    fn field_prefix_and_group() {
        assert_eq!(
            parse("title:berserk").unwrap(),
            term(Some("title"), "berserk")
        );
        assert_eq!(
            parse("title:(a b)").unwrap(),
            Node::And(vec![term(Some("title"), "a"), term(Some("title"), "b")])
        );
    }

    #[test]
    fn phrases() {
        assert_eq!(
            parse("\"berserk vol\"").unwrap(),
            Node::Phrase {
                field: None,
                text: "berserk vol".into()
            }
        );
        assert_eq!(
            parse("title:\"a b\"").unwrap(),
            Node::Phrase {
                field: Some("title".into()),
                text: "a b".into()
            }
        );
    }

    #[test]
    fn prefix_and_wildcard() {
        assert_eq!(
            parse("ber*").unwrap(),
            Node::Prefix {
                field: None,
                text: "ber".into()
            }
        );
        assert_eq!(
            parse("b*rk").unwrap(),
            Node::Wildcard {
                field: None,
                text: "b*rk".into()
            }
        );
        assert!(parse("*foo").is_err());
        assert!(parse("?foo").is_err());
    }

    #[test]
    fn boolean_operators() {
        assert!(matches!(parse("a AND b").unwrap(), Node::And(_)));
        assert!(matches!(parse("a OR b").unwrap(), Node::Or(_)));
        assert!(matches!(parse("a && b").unwrap(), Node::And(_)));
        assert!(matches!(parse("a || b").unwrap(), Node::Or(_)));
        assert!(matches!(parse("a NOT b").unwrap(), Node::And(_)));
        assert!(matches!(parse("a -b").unwrap(), Node::And(_)));
        assert!(matches!(parse("NOT a").unwrap(), Node::Not(_)));
        assert!(matches!(parse("!a").unwrap(), Node::Not(_)));
    }

    #[test]
    fn match_all_and_boost_and_errors() {
        assert_eq!(parse("*:*").unwrap(), Node::MatchAll);
        assert!(matches!(parse("berserk^2").unwrap(), Node::Boost(_, _)));
        assert!(parse("foo~").is_err());
        assert!(parse("foo~0.8").is_err());
        assert!(parse("(a").is_err());
        assert!(parse("a)").is_err());
    }

    #[test]
    fn komga_wrapper() {
        // `LuceneHelper` searches with `"$searchTerm *:*"`
        let node = parse("berserk *:*").unwrap();
        match node {
            Node::And(nodes) => {
                assert_eq!(nodes.len(), 2);
                assert_eq!(nodes[1], Node::MatchAll);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn wildcard_regex_translation() {
        assert_eq!(wildcard_to_regex("b*rk"), "b.*rk");
        assert_eq!(wildcard_to_regex("a?c"), "a.c");
        assert_eq!(wildcard_to_regex("a.c"), "a\\.c");
    }
}
