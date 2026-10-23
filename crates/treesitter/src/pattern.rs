use std::collections::BTreeMap;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Tree};

use crate::edit::Edit;
use crate::error::Error;
use crate::format::text;
use crate::language::Language;
use crate::parse::parse;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Constraint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rule {
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewrite: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, rename = "where", skip_serializing_if = "BTreeMap::is_empty")]
    pub constraints: BTreeMap<String, Constraint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Binding {
    pub text: String,
    pub bytes: [usize; 2],
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Found {
    #[serde(rename = "type")]
    pub kind: String,
    pub start: [usize; 2],
    pub end: [usize; 2],
    pub bytes: [usize; 2],
    pub text: String,
    pub bindings: BTreeMap<String, Binding>,
}

enum Variable {
    Single(Option<String>),
    Multiple(Option<String>),
}

struct Check {
    regex: Option<Regex>,
    kind: Option<u16>,
}

pub struct Pattern {
    source: String,
    tree: Tree,
    root: usize,
    sigil: char,
    checks: BTreeMap<String, Check>,
}

const fn sigil(language: Language) -> char {
    match language {
        Language::Go | Language::Python | Language::Rust => 'µ',
        Language::Zig => '_',
        _ => '$',
    }
}

fn marker(variable: bool, sigil: char) -> char {
    if variable {
        return sigil;
    }
    '$'
}

fn substitute(pattern: &str, sigil: char) -> String {
    let mut output = String::with_capacity(pattern.len());
    let mut dollars = 0;
    for character in pattern.chars() {
        if character == '$' {
            dollars += 1;
            continue;
        }
        let variable = character.is_ascii_uppercase() || character == '_' || dollars == 3;
        output.extend(std::iter::repeat_n(marker(variable, sigil), dollars));
        dollars = 0;
        output.push(character);
    }
    output.extend(std::iter::repeat_n(marker(dollars == 3, sigil), dollars));
    output
}

fn name(rest: &str) -> Option<Option<String>> {
    if rest == "_" || rest.is_empty() {
        return Some(None);
    }
    let mut characters = rest.chars();
    let first = characters.next()?;
    let valid = (first.is_ascii_uppercase() || first == '_')
        && characters.all(|character| character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_');
    valid.then(|| Some(rest.to_owned()))
}

fn variable(value: &str, sigil: char) -> Option<Variable> {
    let triple: String = std::iter::repeat_n(sigil, 3).collect();
    if let Some(rest) = value.strip_prefix(&triple) {
        return name(rest).map(Variable::Multiple);
    }
    let rest = value.strip_prefix(sigil)?;
    if rest.is_empty() {
        return None;
    }
    name(rest).map(Variable::Single)
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(|child| !child.is_extra()).collect()
}

fn first_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    loop {
        let current = cursor.node();
        if current.kind() == kind {
            return Some(current);
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
}

fn wrappers(language: Language) -> &'static [(&'static str, &'static str)] {
    match language {
        Language::Rust => &[("", ""), ("fn µwrap() {\n", "\n}"), ("impl µWrap {\n", "\n}")],
        Language::Go => &[
            ("", ""),
            ("package µwrap\n", ""),
            ("package µwrap\nfunc µwrap() {\n", "\n}"),
        ],
        Language::Zig => &[("", ""), ("fn wrap() void {\n", "\n}"), ("fn wrap() void {\n", ";\n}")],
        Language::JavaScript | Language::TypeScript | Language::Tsx => &[("", ""), ("class Wrap {\n", "\n}")],
        _ => &[("", "")],
    }
}

impl Pattern {
    pub fn new(rule: &Rule, language: Language) -> Result<Self, Error> {
        let sigil = sigil(language);
        let pattern = substitute(&rule.pattern, sigil);
        let (source, tree, span) = match &rule.context {
            Some(context) => Self::in_context(&substitute(context, sigil), &pattern, language)?,
            None => Self::wrapped(&pattern, language)?,
        };
        let root = match &rule.selector {
            Some(kind) => first_of_kind(tree.root_node(), kind)
                .ok_or_else(|| Error::Pattern(format!("selector {kind:?} not found in the pattern")))?,
            None => tree
                .root_node()
                .named_descendant_for_byte_range(span[0], span[1])
                .filter(|node| node.start_byte() == span[0] && node.end_byte() == span[1])
                .ok_or_else(|| Error::Pattern(format!("{:?} is not a single syntax node; pick one with `selector`", rule.pattern)))?,
        };
        if variable(&text(root, source.as_bytes()), sigil).is_some() {
            return Err(Error::Pattern("a pattern must contain more than a single metavariable".to_owned()));
        }
        let grammar = language.grammar();
        let checks = rule
            .constraints
            .iter()
            .map(|(name, constraint)| {
                let regex = constraint.regex.as_deref().map(Regex::new).transpose();
                let regex = regex.map_err(|error| Error::Pattern(format!("where.{name}.regex: {error}")))?;
                let kind = constraint.kind.as_deref().map(|kind| grammar.id_for_node_kind(kind, true));
                if kind == Some(0) {
                    return Err(Error::Pattern(format!("where.{name}.kind: {language} has no node type {:?}", constraint.kind)));
                }
                let check = Check { regex, kind };
                Ok((name.trim_start_matches('$').to_owned(), check))
            })
            .collect::<Result<_, Error>>()?;
        let root = root.id();
        Ok(Self {
            source,
            tree,
            root,
            sigil,
            checks,
        })
    }

    fn span(prefix: &str, pattern: &str) -> [usize; 2] {
        let leading = pattern.len() - pattern.trim_start().len();
        [prefix.len() + leading, prefix.len() + pattern.trim_end().len()]
    }

    fn wrapped(pattern: &str, language: Language) -> Result<(String, Tree, [usize; 2]), Error> {
        for (prefix, suffix) in wrappers(language) {
            let source = format!("{prefix}{pattern}{suffix}");
            let tree = parse(source.as_bytes(), language)?;
            if !tree.root_node().has_error() {
                return Ok((source, tree, Self::span(prefix, pattern)));
            }
        }
        Err(Error::Pattern(format!(
            "{pattern:?} does not parse as {language}; give it a `context` and pick the node with `selector`"
        )))
    }

    fn in_context(context: &str, pattern: &str, language: Language) -> Result<(String, Tree, [usize; 2]), Error> {
        let tree = parse(context.as_bytes(), language)?;
        if tree.root_node().has_error() {
            return Err(Error::Pattern(format!("context does not parse as {language}")));
        }
        let offset = context.find(pattern.trim()).unwrap_or_default();
        let prefix = context.get(..offset).unwrap_or_default();
        let span = Self::span(prefix, pattern.trim());
        Ok((context.to_owned(), tree, span))
    }

    fn root(&self) -> Option<Node<'_>> {
        let mut cursor = self.tree.walk();
        loop {
            let node = cursor.node();
            if node.id() == self.root {
                return Some(node);
            }
            if cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return None;
                }
            }
        }
    }

    pub fn find(&self, tree: &Tree, source: &[u8]) -> Vec<Found> {
        let Some(root) = self.root() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut cursor = tree.walk();
        loop {
            let node = cursor.node();
            if node.kind_id() == root.kind_id() {
                let mut matcher = Matcher {
                    pattern: self,
                    source,
                    bindings: Vec::new(),
                };
                if matcher.node(root, node) && matcher.satisfied() {
                    found.push(matcher.found(node));
                }
            }
            if cursor.goto_first_child() {
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
}

struct Bound {
    name: String,
    range: [usize; 2],
    kind: Option<u16>,
}

struct Matcher<'pattern, 'source> {
    pattern: &'pattern Pattern,
    source: &'source [u8],
    bindings: Vec<Bound>,
}

impl Matcher<'_, '_> {
    fn pattern_text(&self, node: Node<'_>) -> String {
        text(node, self.pattern.source.as_bytes()).into_owned()
    }

    fn slice(&self, range: [usize; 2]) -> &[u8] {
        self.source.get(range[0]..range[1]).unwrap_or_default()
    }

    fn bind(&mut self, name: Option<String>, range: [usize; 2], kind: Option<u16>) -> bool {
        let Some(name) = name else {
            return true;
        };
        let previous = self.bindings.iter().find(|bound| bound.name == name).map(|bound| bound.range);
        if let Some(previous) = previous {
            return self.slice(previous) == self.slice(range);
        }
        self.bindings.push(Bound { name, range, kind });
        true
    }

    fn node(&mut self, pattern: Node<'_>, target: Node<'_>) -> bool {
        if let Some(Variable::Single(name)) = variable(&self.pattern_text(pattern), self.pattern.sigil) {
            return self.bind(name, [target.start_byte(), target.end_byte()], Some(target.kind_id()));
        }
        if pattern.kind_id() != target.kind_id() {
            return false;
        }
        let expected = children(pattern);
        if expected.is_empty() {
            return self.pattern_text(pattern).as_bytes() == self.slice([target.start_byte(), target.end_byte()]);
        }
        self.sequence(&expected, &children(target))
    }

    fn sequence(&mut self, expected: &[Node<'_>], actual: &[Node<'_>]) -> bool {
        let Some((first, rest)) = expected.split_first() else {
            return actual.is_empty();
        };
        let checkpoint = self.bindings.len();
        if let Some(Variable::Multiple(name)) = variable(&self.pattern_text(*first), self.pattern.sigil) {
            for taken in 0..=actual.len() {
                let (span, remaining) = actual.split_at(taken);
                let (range, kind) = match span {
                    [only] => ([only.start_byte(), only.end_byte()], Some(only.kind_id())),
                    [first, .., last] => ([first.start_byte(), last.end_byte()], None),
                    [] => {
                        let position = remaining
                            .first()
                            .map_or_else(|| actual.last().map_or(0, Node::end_byte), Node::start_byte);
                        ([position, position], None)
                    }
                };
                if self.bind(name.clone(), range, kind) && self.sequence(rest, remaining) {
                    return true;
                }
                self.bindings.truncate(checkpoint);
            }
            return false;
        }
        let Some((target, remaining)) = actual.split_first() else {
            return false;
        };
        if self.node(*first, *target) && self.sequence(rest, remaining) {
            return true;
        }
        self.bindings.truncate(checkpoint);
        false
    }

    fn satisfied(&self) -> bool {
        self.pattern.checks.iter().all(|(name, check)| {
            let Some(bound) = self.bindings.iter().find(|bound| bound.name == *name) else {
                return false;
            };
            let value = String::from_utf8_lossy(self.slice(bound.range));
            let regex = check.regex.as_ref().is_none_or(|regex| regex.is_match(&value));
            regex && check.kind.is_none_or(|kind| bound.kind == Some(kind))
        })
    }

    fn found(&self, node: Node<'_>) -> Found {
        let start = node.start_position();
        let end = node.end_position();
        let bindings = self
            .bindings
            .iter()
            .map(|bound| {
                let binding = Binding {
                    text: String::from_utf8_lossy(self.slice(bound.range)).into_owned(),
                    bytes: bound.range,
                };
                (bound.name.clone(), binding)
            })
            .collect();
        Found {
            kind: node.kind().to_owned(),
            start: [start.row, start.column],
            end: [end.row, end.column],
            bytes: [node.start_byte(), node.end_byte()],
            text: text(node, self.source).into_owned(),
            bindings,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Planned<'found> {
    pub found: &'found Found,
    pub template: &'found str,
}

struct Rewriter<'found> {
    planned: Vec<Planned<'found>>,
    source: &'found str,
}

impl<'found> Rewriter<'found> {
    fn outermost(&self, range: [usize; 2], exclude: Option<[usize; 2]>) -> Vec<Planned<'found>> {
        let mut kept: Vec<Planned<'found>> = Vec::new();
        for each in &self.planned {
            let bytes = each.found.bytes;
            let inside = range[0] <= bytes[0] && bytes[1] <= range[1] && Some(bytes) != exclude;
            let free = kept.last().is_none_or(|last| bytes[0] >= last.found.bytes[1]);
            if inside && free {
                kept.push(*each);
            }
        }
        kept
    }

    fn render(&self, range: [usize; 2], exclude: [usize; 2]) -> String {
        let mut output = String::new();
        let mut position = range[0];
        for each in self.outermost(range, Some(exclude)) {
            output.push_str(self.source.get(position..each.found.bytes[0]).unwrap_or_default());
            output.push_str(&self.expand(each));
            position = each.found.bytes[1];
        }
        output.push_str(self.source.get(position..range[1]).unwrap_or_default());
        output
    }

    fn expand(&self, planned: Planned<'_>) -> String {
        let found = planned.found;
        let mut output = String::with_capacity(planned.template.len());
        let mut rest = planned.template;
        while let Some((head, after)) = rest.split_once('$') {
            output.push_str(head);
            let dollars = 1 + 2 * usize::from(after.starts_with("$$"));
            let after = after.get(dollars - 1..).unwrap_or_default();
            let length = after
                .find(|character: char| !(character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'))
                .unwrap_or(after.len());
            let (name, tail) = after.split_at_checked(length).unwrap_or((after, ""));
            let Some(binding) = found.bindings.get(name) else {
                output.push_str(&"$".repeat(dollars));
                rest = after;
                continue;
            };
            output.push_str(&self.render(binding.bytes, found.bytes));
            rest = tail;
        }
        output.push_str(rest);
        output
    }
}

pub fn edits(planned: &[Planned<'_>], source: &str) -> Vec<Edit> {
    let mut ordered: Vec<Planned<'_>> = planned.to_vec();
    ordered.sort_by_key(|each| (each.found.bytes[0], std::cmp::Reverse(each.found.bytes[1])));
    let rewriter = Rewriter { planned: ordered, source };
    rewriter
        .outermost([0, source.len()], None)
        .into_iter()
        .map(|each| Edit {
            bytes: each.found.bytes,
            expect: Some(each.found.text.clone()),
            replace: rewriter.expand(each),
        })
        .collect()
}

pub fn plan<'found>(found: &'found [Found], template: &'found str) -> Vec<Planned<'found>> {
    found.iter().map(|found| Planned { found, template }).collect()
}
