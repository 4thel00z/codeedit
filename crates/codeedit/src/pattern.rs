use std::collections::{BTreeMap, HashSet};

use regex::Regex;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Tree};

use crate::edit::{Change, Edit, apply};
use crate::error::Error;
use crate::format::text;
use crate::language::Language;
use crate::parse::parse;
use crate::walk::preorder;
use crate::tools::{dedent, indent, indent_at};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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
    kind: Option<Vec<u16>>,
}

pub struct Pattern {
    source: String,
    tree: Tree,
    root: usize,
    sigil: &'static str,
    language: Language,
    checks: BTreeMap<String, Check>,
    needles: Vec<String>,
}

fn needles(root: Node<'_>, source: &str, sigil: &str) -> Vec<String> {
    let mut found = Vec::new();
    preorder(root, |node| {
        let word = text(node, source.as_bytes());
        let literal = node.child_count() == 0 && !word.contains(sigil) && word.chars().any(char::is_alphanumeric);
        if literal {
            found.push(word.into_owned());
        }
        true
    });
    found
}

const MACRO_WRAPPERS: [(&str, &str); 2] = [("fn µwrap() { µwrap(", "); }"), ("fn µwrap() {\n", "\n}")];

fn explained(tree: &Tree, source: &str, sigil: &str) -> bool {
    let mut clean = true;
    preorder(tree.root_node(), |node| {
        let stray = node.is_error() && variable(text(node, source.as_bytes()).trim(), sigil).is_none();
        clean &= !(node.is_missing() || stray);
        clean && !node.is_error()
    });
    clean
}

fn point(source: &[u8], byte: usize) -> [usize; 2] {
    let head = source.get(..byte).unwrap_or_default();
    let row = head.iter().filter(|character| **character == b'\n').count();
    let column = head.iter().rev().take_while(|character| **character != b'\n').count();
    [row, column]
}

fn relocate(found: Found, from: usize, to: usize, source: &[u8]) -> Found {
    let moved = |byte: usize| byte.saturating_sub(from) + to;
    let bytes = found.bytes.map(moved);
    let bindings = found
        .bindings
        .into_iter()
        .map(|(name, binding)| {
            let bytes = binding.bytes.map(moved);
            (name, Binding { bytes, ..binding })
        })
        .collect();
    Found {
        start: point(source, bytes[0]),
        end: point(source, bytes[1]),
        bytes,
        bindings,
        ..found
    }
}

const fn sigil(language: Language) -> &'static str {
    match language {
        Language::Go
        | Language::Python
        | Language::Rust
        | Language::Bash
        | Language::JavaScript
        | Language::TypeScript
        | Language::Tsx
        | Language::Ruby
        | Language::Crystal
        | Language::Php
        | Language::C
        | Language::Cpp
        | Language::Java
        | Language::Kotlin
        | Language::Swift
        | Language::CSharp
        | Language::Scala
        | Language::Elixir
        | Language::Haskell
        | Language::Hcl => "µ",
        Language::Zig
        | Language::Lua
        | Language::OCaml
        | Language::Nix
        | Language::Sql
        | Language::Css
        | Language::Scss
        | Language::Make
        | Language::CMake
        | Language::Ini
        | Language::GraphQL
        | Language::Proto
        | Language::Dockerfile => "_0",
        _ => "$",
    }
}

fn marker(variable: bool, sigil: &'static str) -> &'static str {
    if variable {
        return sigil;
    }
    "$"
}

fn metavariables(pattern: &str, sigil: &'static str) -> String {
    let mut output = String::with_capacity(pattern.len());
    let mut dollars = 0;
    for character in pattern.chars() {
        if character == '$' {
            dollars += 1;
            continue;
        }
        let variable = character.is_ascii_uppercase() || character == '_' || dollars == 3;
        output.push_str(&marker(variable, sigil).repeat(dollars));
        dollars = 0;
        output.push(character);
    }
    output.push_str(&marker(dollars == 3, sigil).repeat(dollars));
    output
}

fn substitute(pattern: &str, sigil: &'static str) -> String {
    pattern
        .split("\\$")
        .map(|piece| metavariables(piece, sigil))
        .collect::<Vec<_>>()
        .join("$")
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

fn variable(value: &str, sigil: &str) -> Option<Variable> {
    let triple = sigil.repeat(3);
    if let Some(rest) = value.strip_prefix(&triple) {
        return name(rest).map(Variable::Multiple);
    }
    let rest = value.strip_prefix(sigil)?;
    if rest.is_empty() {
        return None;
    }
    name(rest).map(Variable::Single)
}

type Failed = (usize, usize, Vec<Option<[usize; 2]>>);

enum Segment {
    Literal(String),
    Variable { name: Option<String>, multiple: bool },
}

fn variable_name(rest: &str) -> Option<(Option<String>, usize)> {
    let mut characters = rest.chars();
    let first = characters.next().filter(|character| character.is_ascii_uppercase() || *character == '_')?;
    let tail: String = characters
        .take_while(|character| character.is_ascii_uppercase() || character.is_ascii_digit() || *character == '_')
        .collect();
    let taken = first.len_utf8() + tail.len();
    let name = format!("{first}{tail}");
    let bound = (name != "_").then_some(name);
    Some((bound, taken))
}

fn sigil_run(text: &str, sigil: &str) -> usize {
    let mut count = 0;
    let mut rest = text;
    while let Some(after) = rest.strip_prefix(sigil) {
        count += 1;
        rest = after;
    }
    count
}

fn segments(text: &str, sigil: &str) -> Option<Vec<Segment>> {
    let mut found = Vec::new();
    let mut literal = String::new();
    let mut previous: Option<char> = None;
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let word_start = previous.is_none_or(|previous| !previous.is_alphanumeric() && previous != '_');
        if word_start && rest.starts_with(sigil) {
            let multiple = sigil_run(rest, sigil) >= 3;
            let skipped = if multiple { 3 } else { 1 } * sigil.len();
            if let Some((name, taken)) = rest.get(skipped..).and_then(variable_name) {
                if !literal.is_empty() {
                    found.push(Segment::Literal(std::mem::take(&mut literal)));
                }
                found.push(Segment::Variable { name, multiple });
                previous = Some('_');
                rest = rest.get(skipped + taken..).unwrap_or_default();
                continue;
            }
        }
        literal.push(character);
        previous = Some(character);
        rest = rest.get(character.len_utf8()..).unwrap_or_default();
    }
    if !found.iter().any(|segment| matches!(segment, Segment::Variable { .. })) {
        return None;
    }
    if !literal.is_empty() {
        found.push(Segment::Literal(literal));
    }
    Some(found)
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(|child| !child.is_extra() || child.is_error()).collect()
}

fn gap(range: [usize; 2], source: &[u8]) -> Option<[usize; 2]> {
    crate::symbols::trimmed(range, source).filter(|found| found[0] != found[1])
}

fn gaps(node: Node<'_>, source: &[u8]) -> Vec<[usize; 2]> {
    let mut found = Vec::new();
    let mut cursor = node.start_byte();
    for child in children(node) {
        found.extend(gap([cursor, child.start_byte()], source));
        cursor = child.end_byte();
    }
    found.extend(gap([cursor, node.end_byte()], source));
    found
}

fn skippable(node: Node<'_>) -> bool {
    !node.is_named() && matches!(node.kind(), "," | ";")
}

fn enclosing<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut current = Some(node);
    while let Some(each) = current {
        if each.kind() == kind {
            return Some(each);
        }
        current = each.parent();
    }
    None
}

fn kinds(grammar: &tree_sitter::Language, id: u16) -> Vec<u16> {
    if !grammar.node_kind_is_supertype(id) {
        return vec![id];
    }
    grammar.subtypes_for_supertype(id).iter().flat_map(|subtype| kinds(grammar, *subtype)).collect()
}

fn names(text: &str, sigil: &str) -> Vec<String> {
    text.split(sigil)
        .skip(1)
        .map(|piece| {
            piece
                .chars()
                .take_while(|character| character.is_ascii_uppercase() || character.is_ascii_digit() || *character == '_')
                .collect::<String>()
        })
        .filter(|name| !name.is_empty() && name != "_")
        .collect()
}

fn wrappers(language: Language) -> &'static [(&'static str, &'static str)] {
    match language {
        Language::Rust => &[("", ""), ("fn µwrap() {\n", "\n}"), ("impl µWrap {\n", "\n}"), ("fn µwrap() { match µwrap {\n", "\n} }")],
        Language::Go => &[
            ("", ""),
            ("package µwrap\n", ""),
            ("package µwrap\nfunc µwrap() {\n", "\n}"),
        ],
        Language::Zig => &[("", ""), ("fn wrap() void {\n", "\n}"), ("fn wrap() void {\n", ";\n}")],
        Language::JavaScript | Language::TypeScript | Language::Tsx => &[("", ""), ("class Wrap {\n", "\n}")],
        Language::C => &[("", ""), ("void µwrap() {\n", "\n}"), ("void µwrap() {\n", ";\n}")],
        Language::Cpp => &[("", ""), ("void µwrap() {\n", "\n}"), ("void µwrap() {\n", ";\n}"), ("struct µWrap {\n", "\n};")],
        Language::Java | Language::CSharp => &[
            ("", ""),
            ("class µWrap {\n", "\n}"),
            ("class µWrap { void µwrap() {\n", "\n} }"),
            ("class µWrap { void µwrap() {\n", ";\n} }"),
        ],
        Language::Kotlin => &[("", ""), ("fun µwrap() {\n", "\n}"), ("class µWrap {\n", "\n}")],
        Language::Swift => &[("", ""), ("func µwrap() {\n", "\n}"), ("class µWrap {\n", "\n}")],
        Language::Scala => &[("", ""), ("object µWrap {\n", "\n}"), ("def µwrap = {\n", "\n}")],
        Language::Dart => &[("", ""), ("void wrap() {\n", "\n}"), ("void wrap() {\n", ";\n}"), ("class Wrap {\n", "\n}")],
        Language::Php => &[("<?php\n", ""), ("<?php\n", ";"), ("<?php\nclass µWrap {\n", "\n}")],
        Language::Haskell => &[("", ""), ("µwrap = ", "")],
        Language::OCaml => &[("", ""), ("let wrap = ", "")],
        Language::Css | Language::Scss => &[("", ""), ("wrap {\n", "\n}")],
        Language::Nix => &[("", ""), ("{\n", "\n}"), ("let\n", "\nin null")],
        Language::Proto => &[("", ""), ("message Wrap {\n", "\n}"), ("service Wrap {\n", "\n}"), ("enum Wrap {\n", "\n}")],
        Language::GraphQL => &[("", ""), ("{\n", "\n}"), ("type Wrap {\n", "\n}")],
        Language::Make => &[("", ""), ("wrap = ", "")],
        Language::Dockerfile => &[("", "\n")],
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
        let spanned = tree
            .root_node()
            .named_descendant_for_byte_range(span[0], span[1])
            .ok_or_else(|| Error::Pattern(format!("{:?} is not a single syntax node; pick one with `selector`", rule.pattern)))?;
        let root = match &rule.selector {
            Some(kind) => enclosing(spanned, kind)
                .ok_or_else(|| Error::Pattern(format!("selector {kind:?} does not enclose the pattern")))?,
            None => Some(spanned)
                .filter(|node| node.start_byte() == span[0] && node.end_byte() == span[1])
                .ok_or_else(|| Error::Pattern(format!("{:?} is not a single syntax node; pick one with `selector`", rule.pattern)))?,
        };
        if variable(&text(root, source.as_bytes()), sigil).is_some() {
            return Err(Error::Pattern("a pattern must contain more than a single metavariable".to_owned()));
        }
        let known = names(&text(root, source.as_bytes()), sigil);
        let grammar = language.grammar();
        let checks = rule
            .constraints
            .iter()
            .map(|(name, constraint)| {
                let name = name.trim_start_matches('$').to_owned();
                if !known.contains(&name) {
                    return Err(Error::Pattern(format!("where.{name}: the pattern has no metavariable ${name}")));
                }
                let regex = constraint.regex.as_deref().map(Regex::new).transpose();
                let regex = regex.map_err(|error| Error::Pattern(format!("where.{name}.regex: {error}")))?;
                let kind = constraint.kind.as_deref().map(|kind| grammar.id_for_node_kind(kind, true));
                if kind == Some(0) {
                    return Err(Error::Pattern(format!("where.{name}.kind: {language} has no node type {:?}", constraint.kind)));
                }
                let kind = kind.map(|kind| kinds(&grammar, kind));
                Ok((name, Check { regex, kind }))
            })
            .collect::<Result<_, Error>>()?;
        
        let needles = needles(root, &source, sigil);
        let root = root.id();
        Ok(Self {
            needles,
            source,
            tree,
            root,
            sigil,
            language,
            checks,
        })
    }

    fn span(prefix: &str, pattern: &str) -> [usize; 2] {
        let leading = pattern.len() - pattern.trim_start().len();
        [prefix.len() + leading, prefix.len() + pattern.trim_end().len()]
    }

    fn wrapped(pattern: &str, language: Language) -> Result<(String, Tree, [usize; 2]), Error> {
        let mut parsed = Vec::with_capacity(wrappers(language).len());
        for (prefix, suffix) in wrappers(language) {
            let source = format!("{prefix}{pattern}{suffix}");
            let tree = parse(source.as_bytes(), language)?;
            if !tree.root_node().has_error() {
                return Ok((source, tree, Self::span(prefix, pattern)));
            }
            parsed.push((source, tree, Self::span(prefix, pattern)));
        }
        let sigil = sigil(language);
        parsed
            .into_iter()
            .find(|(source, tree, _)| explained(tree, source, sigil))
            .ok_or_else(|| Error::Pattern(format!("{pattern:?} does not parse as {language}; give it a `context` and pick the node with `selector`")))
    }

    fn in_context(context: &str, pattern: &str, language: Language) -> Result<(String, Tree, [usize; 2]), Error> {
        let tree = parse(context.as_bytes(), language)?;
        if tree.root_node().has_error() && !explained(&tree, context, sigil(language)) {
            return Err(Error::Pattern(format!("context does not parse as {language}")));
        }
        let Some(offset) = context.find(pattern.trim()) else {
            return Err(Error::Pattern(format!("context does not contain the pattern {:?}", pattern.trim())));
        };
        let prefix = context.get(..offset).unwrap_or_default();
        let span = Self::span(prefix, pattern.trim());
        Ok((context.to_owned(), tree, span))
    }

    fn root(&self) -> Option<Node<'_>> {
        let mut found = None;
        preorder(self.tree.root_node(), |node| {
            if node.id() == self.root {
                found = Some(node);
            }
            found.is_none()
        });
        found
    }

    pub fn find(&self, tree: &Tree, source: &[u8]) -> Vec<Found> {
        let Some(root) = self.root() else {
            return Vec::new();
        };
        let macro_body = (self.language == Language::Rust).then(|| self.tree.language().id_for_node_kind("token_tree", true));
        let mut found = Vec::new();
        let mut bodies = Vec::new();
        preorder(tree.root_node(), |node| {
            let body = Some(node.kind_id()) == macro_body && node.parent().is_some_and(|parent| parent.kind() == "macro_invocation");
            if body {
                bodies.push(node);
                return false;
            }
            if node.kind_id() != root.kind_id() {
                return true;
            }
            let mut matcher = Matcher {
                pattern: self,
                source,
                bindings: Vec::new(),
            };
            if matcher.node(root, node) {
                found.push(matcher.found(node));
            }
            true
        });
        if bodies.is_empty() {
            return found;
        }
        found.extend(bodies.into_iter().flat_map(|body| self.in_macro(body, source)));
        found.sort_by_key(|each| (each.bytes[0], std::cmp::Reverse(each.bytes[1])));
        found
    }

    fn in_macro(&self, body: Node<'_>, source: &[u8]) -> Vec<Found> {
        let inner = [body.start_byte() + 1, body.end_byte().saturating_sub(1).max(body.start_byte() + 1)];
        let Some(text) = source.get(inner[0]..inner[1]).and_then(|bytes| std::str::from_utf8(bytes).ok()) else {
            return Vec::new();
        };
        if !self.needles.iter().all(|needle| text.contains(needle.as_str())) {
            return Vec::new();
        }
        for (prefix, suffix) in MACRO_WRAPPERS {
            let wrapped = format!("{prefix}{text}{suffix}");
            let Ok(tree) = parse(wrapped.as_bytes(), Language::Rust) else {
                continue;
            };
            if tree.root_node().has_error() {
                continue;
            }
            let span = [prefix.len(), prefix.len() + text.len()];
            return self
                .find(&tree, wrapped.as_bytes())
                .into_iter()
                .filter(|found| span[0] <= found.bytes[0] && found.bytes[1] <= span[1])
                .map(|found| relocate(found, span[0], inner[0], source))
                .collect();
        }
        Vec::new()
    }
}

struct Bound {
    name: String,
    range: [usize; 2],
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
        if !self.allowed(&name, range, kind) {
            return false;
        }
        self.bindings.push(Bound { name, range });
        true
    }

    fn allowed(&self, name: &str, range: [usize; 2], kind: Option<u16>) -> bool {
        let Some(check) = self.pattern.checks.get(name) else {
            return true;
        };
        let value = String::from_utf8_lossy(self.slice(range));
        let regex = check.regex.as_ref().is_none_or(|regex| regex.is_match(&value));
        let kinds = check.kind.as_ref().is_none_or(|kinds| kind.is_some_and(|kind| kinds.contains(&kind)));
        regex && kinds
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
            return self.leaf(pattern, target);
        }
        self.sequence(&expected, &children(target), target.start_byte()) && self.hidden(pattern, target)
    }

    fn hidden(&mut self, pattern: Node<'_>, target: Node<'_>) -> bool {
        let expected = gaps(pattern, self.pattern.source.as_bytes());
        if expected.is_empty() {
            return true;
        }
        let actual = gaps(target, self.source);
        if expected.len() != actual.len() {
            return false;
        }
        for (wanted, range) in expected.into_iter().zip(actual) {
            let wanted = self.pattern.source.get(wanted[0]..wanted[1]).unwrap_or_default().to_owned();
            let Ok(found) = std::str::from_utf8(self.slice(range)).map(str::to_owned) else {
                return false;
            };
            if wanted == found {
                continue;
            }
            let Some(segments) = segments(&wanted, self.pattern.sigil) else {
                return false;
            };
            if !self.embedded(&segments, &found, range[0], target.kind_id()) {
                return false;
            }
        }
        true
    }

    fn leaf(&mut self, pattern: Node<'_>, target: Node<'_>) -> bool {
        let expected = self.pattern_text(pattern);
        let actual = self.slice([target.start_byte(), target.end_byte()]);
        if expected.as_bytes() == actual {
            return true;
        }
        let Some(segments) = segments(&expected, self.pattern.sigil) else {
            return false;
        };
        let Ok(actual) = std::str::from_utf8(actual).map(str::to_owned) else {
            return false;
        };
        self.embedded(&segments, &actual, target.start_byte(), target.kind_id())
    }

    fn embedded(&mut self, segments: &[Segment], actual: &str, at: usize, kind: u16) -> bool {
        let mut failed = HashSet::new();
        self.embedded_from(segments, 0, actual, at, kind, &mut failed)
    }

    fn later_bindings(&self, segments: &[Segment], index: usize) -> Vec<Option<[usize; 2]>> {
        segments
            .get(index..)
            .unwrap_or_default()
            .iter()
            .map(|segment| match segment {
                Segment::Variable { name: Some(name), .. } => self.bindings.iter().find(|bound| bound.name == *name).map(|bound| bound.range),
                _ => None,
            })
            .collect()
    }

    fn embedded_from(&mut self, segments: &[Segment], index: usize, actual: &str, at: usize, kind: u16, failed: &mut HashSet<Failed>) -> bool {
        let Some(first) = segments.get(index) else {
            return actual.is_empty();
        };
        let key = (index, at, self.later_bindings(segments, index));
        if failed.contains(&key) {
            return false;
        }
        let matched = match first {
            Segment::Literal(literal) => actual
                .strip_prefix(literal.as_str())
                .is_some_and(|remaining| self.embedded_from(segments, index + 1, remaining, at + literal.len(), kind, failed)),
            Segment::Variable { name, multiple } => {
                let checkpoint = self.bindings.len();
                let minimum = usize::from(!multiple);
                let ends: Vec<usize> = actual.char_indices().map(|(index, _)| index).chain(std::iter::once(actual.len())).collect();
                let mut matched = false;
                for end in ends.into_iter().rev() {
                    if end < minimum {
                        break;
                    }
                    let remaining = actual.get(end..).unwrap_or_default();
                    if self.bind(name.clone(), [at, at + end], Some(kind)) && self.embedded_from(segments, index + 1, remaining, at + end, kind, failed) {
                        matched = true;
                        break;
                    }
                    self.bindings.truncate(checkpoint);
                }
                matched
            }
        };
        if !matched {
            failed.insert(key);
        }
        matched
    }

    fn sequence(&mut self, expected: &[Node<'_>], actual: &[Node<'_>], position: usize) -> bool {
        let Some((first, rest)) = expected.split_first() else {
            return actual.iter().all(|node| skippable(*node));
        };
        let checkpoint = self.bindings.len();
        if let Some(Variable::Multiple(name)) = variable(&self.pattern_text(*first), self.pattern.sigil) {
            let required = rest.iter().filter(|node| !matches!(variable(&self.pattern_text(**node), self.pattern.sigil), Some(Variable::Multiple(_)))).count();
            for taken in 0..=actual.len().saturating_sub(required) {
                let (span, remaining) = actual.split_at(taken);
                let (range, kind) = match span {
                    [only] => ([only.start_byte(), only.end_byte()], Some(only.kind_id())),
                    [first, .., last] => ([first.start_byte(), last.end_byte()], None),
                    [] => {
                        let at = remaining.first().map_or(position, Node::start_byte);
                        ([at, at], None)
                    }
                };
                let next = span.last().map_or(position, Node::end_byte);
                if self.bind(name.clone(), range, kind) && self.sequence(rest, remaining, next) {
                    return true;
                }
                self.bindings.truncate(checkpoint);
            }
            return false;
        }
        let Some((target, remaining)) = actual.split_first() else {
            return false;
        };
        if self.node(*first, *target) && self.sequence(rest, remaining, target.end_byte()) {
            return true;
        }
        self.bindings.truncate(checkpoint);
        if !skippable(*target) {
            return false;
        }
        self.sequence(expected, remaining, target.end_byte())
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

pub(crate) fn trailing_indent(text: &str) -> String {
    let line = text.rsplit('\n').next().unwrap_or_default();
    line.chars().take_while(|character| *character == ' ' || *character == '\t').collect()
}

struct Rewriter<'found> {
    planned: Vec<Planned<'found>>,
    source: &'found str,
    language: Language,
}

impl<'found> Rewriter<'found> {
    fn outermost(&self, range: [usize; 2], exclude: Option<[usize; 2]>) -> Vec<Planned<'found>> {
        let first = self.planned.partition_point(|each| each.found.bytes[0] < range[0]);
        let mut kept: Vec<Planned<'found>> = Vec::new();
        for each in self.planned.iter().skip(first) {
            let bytes = each.found.bytes;
            if bytes[0] > range[1] {
                break;
            }
            let inside = bytes[1] <= range[1] && Some(bytes) != exclude;
            let free = kept.last().is_none_or(|last| bytes[0] >= last.found.bytes[1]);
            if inside && free {
                kept.push(*each);
            }
        }
        kept
    }

    fn render(&self, range: [usize; 2], exclude: [usize; 2], depth: usize) -> Result<String, Error> {
        let mut output = String::new();
        let mut position = range[0];
        for each in self.outermost(range, Some(exclude)) {
            output.push_str(self.source.get(position..each.found.bytes[0]).unwrap_or_default());
            output.push_str(&self.expand(each, depth + 1)?);
            position = each.found.bytes[1];
        }
        output.push_str(self.source.get(position..range[1]).unwrap_or_default());
        Ok(output)
    }

    fn expand(&self, planned: Planned<'_>, depth: usize) -> Result<String, Error> {
        if depth > NESTING {
            return Err(Error::Pattern(format!("matches nest deeper than {NESTING} levels; rewrite in smaller steps")));
        }
        let found = planned.found;
        let mut output = String::with_capacity(planned.template.len());
        let mut rest = planned.template;
        while let Some((head, after)) = rest.split_once('$') {
            if let Some(literal) = head.strip_suffix('\\') {
                output.push_str(literal);
                output.push('$');
                rest = after;
                continue;
            }
            output.push_str(head);
            let dollars = 1 + 2 * usize::from(after.starts_with("$$"));
            let after = after.get(dollars - 1..).unwrap_or_default();
            let run = after
                .find(|character: char| !(character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'))
                .unwrap_or(after.len());
            let longest = (1..=run).rev().find(|length| after.get(..*length).is_some_and(|name| found.bindings.contains_key(name)));
            let Some((binding, tail)) = longest.and_then(|length| {
                let (name, tail) = after.split_at_checked(length)?;
                Some((found.bindings.get(name)?, tail))
            }) else {
                output.push_str(&"$".repeat(dollars));
                rest = after;
                continue;
            };
            let rendered = dedent(&self.render(binding.bytes, found.bytes, depth)?, &indent_at(self.source, binding.bytes[0]), self.language);
            output.push_str(&indent(&rendered, &trailing_indent(&output), self.language));
            rest = tail;
        }
        output.push_str(rest);
        Ok(indent(&output, &indent_at(self.source, found.bytes[0]), self.language))
    }
}

const NESTING: usize = 256;

pub fn edits(planned: &[Planned<'_>], source: &str, language: Language) -> Result<Vec<Edit>, Error> {
    let mut ordered: Vec<Planned<'_>> = planned.to_vec();
    ordered.sort_by_key(|each| (each.found.bytes[0], std::cmp::Reverse(each.found.bytes[1])));
    let rewriter = Rewriter { planned: ordered, source, language };
    rewriter
        .outermost([0, source.len()], None)
        .into_iter()
        .map(|each| {
            Ok(Edit {
                bytes: each.found.bytes,
                expect: Some(each.found.text.clone()),
                replace: rewriter.expand(each, 0)?,
            })
        })
        .collect()
}

pub fn plan<'found>(found: &'found [Found], template: &'found str) -> Vec<Planned<'found>> {
    found.iter().map(|found| Planned { found, template }).collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleMatch {
    pub rule: usize,
    #[serde(flatten)]
    pub found: Found,
}

fn applies(rule: &Rule, language: Language) -> Result<bool, Error> {
    let Some(name) = rule.language.as_deref() else {
        return Ok(true);
    };
    Ok(name.parse::<Language>()? == language)
}

pub fn compile_rules(rules: &[Rule], language: Language) -> Result<Vec<(usize, Pattern)>, Error> {
    let mut compiled = Vec::with_capacity(rules.len());
    for (index, rule) in rules.iter().enumerate() {
        if !applies(rule, language)? {
            continue;
        }
        compiled.push((index, Pattern::new(rule, language)?));
    }
    Ok(compiled)
}

pub fn search<'pattern>(patterns: impl IntoIterator<Item = (usize, &'pattern Pattern)>, tree: &Tree, source: &[u8]) -> Vec<RuleMatch> {
    patterns
        .into_iter()
        .flat_map(|(rule, pattern)| pattern.find(tree, source).into_iter().map(move |found| RuleMatch { rule, found }))
        .collect()
}

pub fn planned<'found>(matches: &'found [RuleMatch], rules: &'found [Rule]) -> Vec<Planned<'found>> {
    matches
        .iter()
        .filter_map(|each| {
            let template = rules.get(each.rule)?.rewrite.as_deref()?;
            Some(Planned {
                found: &each.found,
                template,
            })
        })
        .collect()
}

pub fn find_source(source: &[u8], language: Language, rules: &[Rule]) -> Result<Vec<RuleMatch>, Error> {
    let tree = parse(source, language)?;
    let compiled = compile_rules(rules, language)?;
    Ok(search(compiled.iter().map(|(rule, pattern)| (*rule, pattern)), &tree, source))
}

pub fn rewrite_source(source: &str, language: Language, rules: &[Rule], label: &str, allow_errors: bool) -> Result<Change, Error> {
    if rules.iter().any(|rule| rule.rewrite.is_none()) {
        return Err(Error::MissingRewrite);
    }
    let matches = find_source(source.as_bytes(), language, rules)?;
    let edits = edits(&planned(&matches, rules), source, language)?;
    apply(source, language, &edits, label, allow_errors)
}
