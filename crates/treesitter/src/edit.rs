use serde::{Deserialize, Serialize};
use similar::{Algorithm, udiff::unified_diff};
use tree_sitter::Tree;

use crate::error::Error;
use crate::language::Language;
use crate::parse::parse;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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
    #[serde(skip)]
    pub original: String,
    pub diff: String,
    pub edits: usize,
    pub errors_before: usize,
    pub errors_after: usize,
}

pub fn errors(tree: &Tree) -> Vec<[usize; 2]> {
    if !tree.root_node().has_error() {
        return Vec::new();
    }
    let mut cursor = tree.walk();
    let mut found = Vec::new();
    loop {
        let node = cursor.node();
        let broken = node.is_error() || node.is_missing();
        if broken {
            found.push([node.start_byte(), node.end_byte()]);
        }
        if !broken && node.has_error() && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return found;
            }
        }
    }
}

pub fn error_count(tree: &Tree) -> usize {
    errors(tree).len()
}

fn shifted(range: [usize; 2], edits: &[&Edit]) -> Option<[usize; 2]> {
    let mut added = 0;
    let mut removed = 0;
    for edit in edits {
        let [start, end] = edit.bytes;
        if start >= range[1] && start > range[0] {
            break;
        }
        if end > range[0] {
            return None;
        }
        added += edit.replace.len();
        removed += end - start;
    }
    Some([(range[0] + added).checked_sub(removed)?, (range[1] + added).checked_sub(removed)?])
}

fn introduced(before: &[[usize; 2]], after: &[[usize; 2]], edits: &[&Edit]) -> bool {
    let kept: Vec<[usize; 2]> = before.iter().filter_map(|range| shifted(*range, edits)).collect();
    after
        .iter()
        .any(|range| !kept.iter().any(|old| old[0] <= range[1] && range[0] <= old[1]))
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
    sorted.dedup_by(|second, first| second == first && first.bytes[0] != first.bytes[1]);
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

fn line_endings(source: &str, edits: &[Edit]) -> Vec<Edit> {
    if !source.contains("\r\n") {
        return edits.to_vec();
    }
    edits
        .iter()
        .map(|edit| Edit {
            replace: edit.replace.replace("\r\n", "\n").replace('\n', "\r\n"),
            ..edit.clone()
        })
        .collect()
}

pub fn apply(source: &str, language: Language, edits: &[Edit], label: &str, allow_errors: bool) -> Result<Change, Error> {
    let before = errors(&parse(source.as_bytes(), language)?);
    let edits = line_endings(source, edits);
    let (output, count) = splice(source, &edits)?;
    let after = errors(&parse(output.as_bytes(), language)?);
    if !allow_errors && introduced(&before, &after, &ordered(&edits)?) {
        return Err(Error::Syntax {
            before: before.len(),
            after: after.len(),
        });
    }
    let (old, new) = labels(label);
    let diff = unified_diff(Algorithm::Myers, source, &output, 3, Some((&old, &new)));
    Ok(Change {
        source: output,
        original: source.to_owned(),
        diff,
        edits: count,
        errors_before: before.len(),
        errors_after: after.len(),
    })
}
