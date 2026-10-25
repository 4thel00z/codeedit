use serde::{Deserialize, Serialize};
use similar::{Algorithm, udiff::unified_diff};
use tree_sitter::Tree;

use crate::error::Error;
use crate::language::Language;
use crate::parse::parse;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edit {
    pub bytes: [usize; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<String>,
    pub replace: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Change {
    #[serde(skip)]
    pub source: String,
    pub diff: String,
    pub edits: usize,
    pub errors_before: usize,
    pub errors_after: usize,
}

pub fn error_count(tree: &Tree) -> usize {
    if !tree.root_node().has_error() {
        return 0;
    }
    let mut cursor = tree.walk();
    let mut count = 0;
    loop {
        let node = cursor.node();
        count += usize::from(node.is_error() || node.is_missing());
        if node.has_error() && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return count;
            }
        }
    }
}

fn check(source: &str, edit: &Edit) -> Result<(), Error> {
    let [start, end] = edit.bytes;
    let Some(found) = source.get(start..end).filter(|_| start <= end) else {
        return Err(Error::Range {
            start,
            end,
            length: source.len(),
        });
    };
    let Some(expected) = &edit.expect else {
        return Ok(());
    };
    if expected == found {
        return Ok(());
    }
    Err(Error::Stale {
        start,
        end,
        expected: expected.clone(),
        found: found.to_owned(),
    })
}

fn ordered(edits: &[Edit]) -> Result<Vec<&Edit>, Error> {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|edit| edit.bytes);
    sorted.dedup();
    for pair in sorted.windows(2) {
        let [first, second] = pair else {
            continue;
        };
        if first.bytes[1] > second.bytes[0] || (first.bytes == second.bytes && first.bytes[0] != first.bytes[1]) {
            return Err(Error::Overlap {
                first: first.bytes,
                second: second.bytes,
            });
        }
    }
    Ok(sorted)
}

pub fn splice(source: &str, edits: &[Edit]) -> Result<(String, usize), Error> {
    let sorted = ordered(edits)?;
    for edit in &sorted {
        check(source, edit)?;
    }
    let mut output = String::with_capacity(source.len());
    let mut position = 0;
    for edit in &sorted {
        let [start, end] = edit.bytes;
        output.push_str(source.get(position..start).unwrap_or_default());
        output.push_str(&edit.replace);
        position = end;
    }
    output.push_str(source.get(position..).unwrap_or_default());
    Ok((output, sorted.len()))
}

fn labels(label: &str) -> (String, String) {
    if label.starts_with('/') {
        return (label.to_owned(), label.to_owned());
    }
    let relative = label.trim_start_matches("./");
    (format!("a/{relative}"), format!("b/{relative}"))
}

pub fn apply(source: &str, language: Language, edits: &[Edit], label: &str, allow_errors: bool) -> Result<Change, Error> {
    let errors_before = error_count(&parse(source.as_bytes(), language)?);
    let (output, count) = splice(source, edits)?;
    let errors_after = error_count(&parse(output.as_bytes(), language)?);
    if errors_after > errors_before && !allow_errors {
        return Err(Error::Syntax {
            before: errors_before,
            after: errors_after,
        });
    }
    let (old, new) = labels(label);
    let diff = unified_diff(Algorithm::Myers, source, &output, 3, Some((&old, &new)));
    Ok(Change {
        source: output,
        diff,
        edits: count,
        errors_before,
        errors_after,
    })
}
