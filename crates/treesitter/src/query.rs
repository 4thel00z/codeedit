use serde::Serialize;
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::Error;
use crate::format::text;
use crate::language::Language;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Capture {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub start: [usize; 2],
    pub end: [usize; 2],
    pub bytes: [usize; 2],
    pub text: String,
}

impl Capture {
    pub fn new(name: &str, node: Node<'_>, source: &[u8]) -> Self {
        let start = node.start_position();
        let end = node.end_position();
        Self {
            name: name.to_owned(),
            kind: node.kind().to_owned(),
            start: [start.row, start.column],
            end: [end.row, end.column],
            bytes: [node.start_byte(), node.end_byte()],
            text: text(node, source).into_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Match {
    pub pattern: usize,
    pub captures: Vec<Capture>,
}

impl Match {
    pub fn capture(&self, name: &str) -> Option<&Capture> {
        self.captures.iter().find(|capture| capture.name == name)
    }
}

pub fn compile(language: Language, pattern: &str) -> Result<Query, Error> {
    Ok(Query::new(&language.grammar(), pattern)?)
}

pub fn matches(query: &Query, tree: &Tree, source: &[u8]) -> Vec<Match> {
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut found = Vec::new();
    let mut iterator = cursor.matches(query, tree.root_node(), source);
    while let Some(each) = iterator.next() {
        let captures = each
            .captures()
            .iter()
            .filter_map(|capture| {
                let name = names.get(capture.index as usize)?;
                Some(Capture::new(name, capture.node, source))
            })
            .collect();
        found.push(Match {
            pattern: each.pattern_index,
            captures,
        });
    }
    found
}
