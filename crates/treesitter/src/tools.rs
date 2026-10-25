use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Tree};

use crate::edit::{Change, Edit, apply, splice};
use crate::error::Error;
use crate::files::{FileChange, Parsed, agreed, canonical, changed, collect, commit, finish, read, read_target};
use crate::language::Language;
use crate::parse::parse;
use crate::symbols::{Symbol, lookup, outline};
use crate::walk::preorder;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Before,
    After,
    Start,
    End,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Replace { symbol: String, code: String },
    ReplaceBody { symbol: String, code: String },
    Insert { symbol: String, code: String, position: Position },
    Delete { symbol: String },
    AddImport { import: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "serde_json::Map<String, serde_json::Value>")]
pub struct Request {
    pub file: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(flatten)]
    pub operation: Operation,
}

impl TryFrom<serde_json::Map<String, serde_json::Value>> for Request {
    type Error = serde_json::Error;

    fn try_from(mut fields: serde_json::Map<String, serde_json::Value>) -> Result<Self, Self::Error> {
        let file = fields.remove("file").ok_or_else(|| serde::de::Error::missing_field("file"))?;
        let language = fields.remove("language").map(serde_json::from_value).transpose()?;
        Ok(Self {
            file: serde_json::from_value(file)?,
            language,
            operation: serde_json::from_value(serde_json::Value::Object(fields))?,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolSource {
    #[serde(flatten)]
    pub symbol: Symbol,
    pub text: String,
}

fn line_start(source: &str, position: usize) -> usize {
    source.get(..position).and_then(|head| head.rfind('\n')).map_or(0, |index| index + 1)
}

fn line_end(source: &str, position: usize) -> usize {
    let end = source
        .get(position..)
        .and_then(|tail| tail.find('\n'))
        .map_or(source.len(), |index| position + index);
    if end > 0 && source.get(end - 1..end) == Some("\r") {
        return end - 1;
    }
    end
}

fn next_line(source: &str, position: usize) -> usize {
    source
        .get(position..)
        .and_then(|tail| tail.find('\n'))
        .map_or(source.len(), |index| position + index + 1)
}

pub(crate) fn indent_at(source: &str, position: usize) -> String {
    let start = line_start(source, position);
    let line = source.get(start..line_end(source, start)).unwrap_or_default();
    line.chars().take_while(|character| *character == ' ' || *character == '\t').collect()
}

fn indent_unit(language: Language, source: &str) -> &'static str {
    if language == Language::Go || source.lines().any(|line| line.starts_with('\t')) {
        return "\t";
    }
    "    "
}

fn end_of_line(source: &str, end: usize) -> usize {
    line_end(source, end.saturating_sub(1))
}

fn literal(kind: &str) -> bool {
    kind.contains("string") || kind.contains("heredoc")
}

fn protected(code: &str, language: Language) -> Vec<bool> {
    let mut rows = vec![false; code.split('\n').count()];
    if !code.contains('\n') {
        return rows;
    }
    let Ok(tree) = parse(code.as_bytes(), language) else {
        return rows;
    };
    preorder(tree.root_node(), |node| {
        let [start, end] = [node.start_position().row, node.end_position().row];
        if literal(node.kind()) && end > start {
            rows.iter_mut().take(end + 1).skip(start + 1).for_each(|row| *row = true);
        }
        true
    });
    rows
}

fn width(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

pub fn reindent(code: &str, indent: &str, language: Language) -> String {
    let code = code.trim_matches('\n');
    let kept = protected(code, language);
    let lines: Vec<&str> = code.split('\n').collect();
    let common = lines
        .iter()
        .zip(&kept)
        .filter(|(line, kept)| !**kept && !line.trim().is_empty())
        .map(|(line, _)| width(line))
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .zip(&kept)
        .map(|(line, kept)| {
            if *kept {
                return (*line).to_owned();
            }
            if line.trim().is_empty() {
                return String::new();
            }
            format!("{indent}{}", line.get(common..).unwrap_or(line).trim_end())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Braces,
    Block,
    Inline,
    Expression,
}

struct Body {
    shape: Shape,
    inner: [usize; 2],
    indent: String,
    closing: String,
}

fn opening(text: &str, language: Language) -> Option<usize> {
    if language == Language::Python || !text.ends_with('}') {
        return None;
    }
    let open = text.find('{')?;
    let prefix = text.get(..open).unwrap_or_default();
    let keyword = prefix
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | ' ' | '\t' | '(' | ')'));
    keyword.then_some(open)
}

fn body(source: &str, language: Language, symbol: &Symbol) -> Result<Body, Error> {
    let [start, end] = symbol
        .body
        .ok_or_else(|| Error::Unsupported(format!("{} {} has no body", symbol.kind, symbol.path)))?;
    let text = source.get(start..end).unwrap_or_default();
    let symbol_indent = indent_at(source, symbol.bytes[0]);
    let nested = format!("{symbol_indent}{}", indent_unit(language, source));
    let line = line_start(source, start);
    let inline = !blank(source, [line, start]);
    let Some(open) = opening(text, language) else {
        let head = source.get(line..start).unwrap_or_default();
        let shape = match (language, inline) {
            (Language::Python, true) => Shape::Inline,
            (Language::Python, false) => Shape::Block,
            _ => Shape::Expression,
        };
        let inner = match shape {
            Shape::Inline => [line + head.trim_end().len(), end],
            _ => [start, end],
        };
        let indent = match shape {
            Shape::Block => indent_at(source, start),
            _ => nested,
        };
        return Ok(Body {
            shape,
            inner,
            indent,
            closing: symbol_indent,
        });
    };
    let inner = [start + open + 1, end - 1];
    let content = source.get(inner[0]..inner[1]).unwrap_or_default();
    let first = content.lines().skip(1).find(|line| !line.trim().is_empty());
    let indent = first.map_or(nested, |line| line.chars().take_while(|character| character.is_whitespace()).collect());
    Ok(Body {
        shape: Shape::Braces,
        inner,
        indent,
        closing: indent_at(source, inner[1]),
    })
}

fn blank(source: &str, range: [usize; 2]) -> bool {
    source.get(range[0]..range[1]).is_some_and(|content| content.trim().is_empty())
}

fn trailing_comment(tree: &Tree, source: &str, position: usize) -> bool {
    let tail = source.get(position..line_end(source, position)).unwrap_or_default();
    let start = position + horizontal(tail);
    let mut current = tree.root_node().descendant_for_byte_range(start, start);
    while let Some(node) = current.filter(|node| node.start_byte() == start) {
        if node.kind().contains("comment") {
            return node.end_byte() <= next_line(source, start);
        }
        current = node.parent();
    }
    false
}

fn owns_lines(tree: &Tree, source: &str, symbol: &Symbol) -> bool {
    let [start, end] = symbol.bytes;
    let rest = [end, end_of_line(source, end).max(end)];
    blank(source, [line_start(source, start), start]) && (blank(source, rest) || trailing_comment(tree, source, end))
}

fn replace(symbol: &Symbol, source: &str, language: Language, code: &str) -> Edit {
    let start = line_start(source, symbol.bytes[0]);
    let replacement = reindent(code, &indent_at(source, symbol.bytes[0]), language);
    if !blank(source, [start, symbol.bytes[0]]) {
        return Edit {
            bytes: symbol.bytes,
            expect: source.get(symbol.bytes[0]..symbol.bytes[1]).map(str::to_owned),
            replace: replacement.trim_start().to_owned(),
        };
    }
    Edit {
        bytes: [start, symbol.bytes[1]],
        expect: source.get(start..symbol.bytes[1]).map(str::to_owned),
        replace: replacement,
    }
}

fn wrap_braces(body: &Body, code: &str, language: Language) -> String {
    format!("\n{}\n{}", reindent(code, &body.indent, language), body.closing)
}

fn body_replacement(body: &Body, code: &str, language: Language) -> String {
    match body.shape {
        Shape::Braces => wrap_braces(body, code, language),
        Shape::Inline => format!("\n{}", reindent(code, &body.indent, language)),
        Shape::Block | Shape::Expression => reindent(code, &body.indent, language).trim_start().to_owned(),
    }
}

fn replace_body(symbol: &Symbol, source: &str, language: Language, code: &str) -> Result<Edit, Error> {
    let body = body(source, language, symbol)?;
    Ok(Edit {
        bytes: body.inner,
        expect: source.get(body.inner[0]..body.inner[1]).map(str::to_owned),
        replace: body_replacement(&body, code, language),
    })
}

fn insertion(position: usize, text: String) -> Edit {
    Edit {
        bytes: [position, position],
        expect: None,
        replace: text,
    }
}

fn gap(code: &str) -> &'static str {
    if code.contains('\n') {
        return "\n";
    }
    ""
}

fn docstring(tree: &Tree, body: &Body) -> Option<usize> {
    let block = tree.root_node().named_descendant_for_byte_range(body.inner[0], body.inner[1])?;
    let first = block.named_child(0)?;
    let string = first.kind() == "expression_statement" && first.named_child(0).is_some_and(|child| child.kind() == "string");
    string.then(|| first.end_byte())
}

fn inline(symbol: &Symbol, source: &str, language: Language, code: &str, position: Position) -> Result<Edit, Error> {
    let body = body(source, language, symbol)?;
    let existing = source.get(body.inner[0]..body.inner[1]).unwrap_or_default().trim();
    let combined = match position {
        Position::Start => format!("{code}\n{existing}"),
        _ => format!("{existing}\n{code}"),
    };
    Ok(Edit {
        bytes: body.inner,
        expect: source.get(body.inner[0]..body.inner[1]).map(str::to_owned),
        replace: format!("\n{}", reindent(&combined, &body.indent, language)),
    })
}

fn beside(tree: &Tree, symbol: &Symbol, source: &str, language: Language, code: &str, position: Position) -> Edit {
    let indent = indent_at(source, symbol.bytes[0]);
    let code = reindent(code, &indent, language);
    if owns_lines(tree, source, symbol) {
        return match position {
            Position::Before => insertion(line_start(source, symbol.bytes[0]), format!("{code}\n\n")),
            _ => insertion(end_of_line(source, symbol.bytes[1]), format!("\n\n{code}")),
        };
    }
    match position {
        Position::Before => insertion(symbol.bytes[0], format!("{} ", code.trim_start())),
        _ => insertion(symbol.bytes[1], format!(" {}", code.trim_start())),
    }
}

fn insert(tree: &Tree, symbol: &Symbol, source: &str, language: Language, code: &str, position: Position) -> Result<Edit, Error> {
    if matches!(position, Position::Before | Position::After) {
        return Ok(beside(tree, symbol, source, language, code, position));
    }
    let body = body(source, language, symbol)?;
    if body.shape == Shape::Inline {
        return inline(symbol, source, language, code, position);
    }
    if body.shape == Shape::Expression {
        return Err(Error::Unsupported(format!("{} {} has an expression body; use replace_body", symbol.kind, symbol.path)));
    }
    let code = reindent(code, &body.indent, language);
    let braces = body.shape == Shape::Braces;
    if braces && blank(source, body.inner) {
        return Ok(Edit {
            bytes: body.inner,
            expect: source.get(body.inner[0]..body.inner[1]).map(str::to_owned),
            replace: format!("\n{code}\n{}", body.closing),
        });
    }
    let gap = gap(&code);
    let documented = (position == Position::Start && !braces).then(|| docstring(tree, &body)).flatten();
    if let Some(end) = documented {
        return Ok(insertion(end_of_line(source, end), format!("\n{code}{gap}")));
    }
    let edit = match (position, braces) {
        (Position::Start, true) => insertion(body.inner[0], format!("\n{code}{gap}")),
        (Position::Start, false) => insertion(line_start(source, body.inner[0]), format!("{code}\n{gap}")),
        (_, true) if blank(source, [line_start(source, body.inner[1]), body.inner[1]]) => {
            insertion(line_start(source, body.inner[1]), format!("{gap}{code}\n"))
        }
        (_, true) => insertion(body.inner[1], format!("\n{code}\n{}", body.closing)),
        (_, false) => insertion(end_of_line(source, body.inner[1]), format!("\n{gap}{code}")),
    };
    Ok(edit)
}

fn horizontal(text: &str) -> usize {
    text.len() - text.trim_start_matches([' ', '\t']).len()
}

fn delete_inline(symbol: &Symbol, source: &str) -> Edit {
    let [mut start, mut end] = symbol.bytes;
    let after = source.get(end..).unwrap_or_default();
    end += horizontal(after);
    let separated = source.get(end..).is_some_and(|rest| rest.starts_with(','));
    if separated {
        end += 1 + horizontal(source.get(end + 1..).unwrap_or_default());
    }
    let before = source.get(..start).unwrap_or_default().trim_end_matches([' ', '\t']);
    if !separated && before.ends_with(',') {
        start = before.len() - 1;
        end = symbol.bytes[1];
    }
    Edit {
        bytes: [start, end],
        expect: source.get(start..end).map(str::to_owned),
        replace: String::new(),
    }
}

fn listed<'tree>(node: Node<'tree>, field: &str) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.children_by_field_name(field, &mut cursor).collect()
}

fn drop_item(items: &[Node<'_>], index: usize, source: &str) -> Option<Edit> {
    let item = items.get(index)?;
    let previous = index.checked_sub(1).and_then(|before| items.get(before));
    let [start, end] = match (items.get(index + 1), previous) {
        (Some(next), _) => [item.start_byte(), next.start_byte()],
        (None, Some(previous)) => [previous.end_byte(), item.end_byte()],
        (None, None) => return None,
    };
    Some(Edit {
        bytes: [start, end],
        expect: source.get(start..end).map(str::to_owned),
        replace: String::new(),
    })
}

fn delete_name(tree: &Tree, symbol: &Symbol, source: &str) -> Option<Vec<Edit>> {
    let declaration = tree.root_node().descendant_for_byte_range(symbol.bytes[0], symbol.bytes[1])?;
    let mut spec = None;
    preorder(declaration, |node| {
        if matches!(node.kind(), "var_spec" | "const_spec") {
            spec = spec.or(Some(node));
        }
        spec.is_none()
    });
    let spec = spec?;
    let names = listed(spec, "name");
    let index = names.iter().position(|name| source.get(name.byte_range()) == Some(symbol.name.as_str()))?;
    let mut edits = vec![drop_item(&names, index, source)?];
    let values: Vec<Node<'_>> = spec
        .child_by_field_name("value")
        .map(|list| {
            let mut cursor = list.walk();
            list.named_children(&mut cursor).collect()
        })
        .unwrap_or_default();
    edits.extend(drop_item(&values, index, source));
    Some(edits)
}

fn delete_symbol(tree: &Tree, symbols: &[Symbol], wanted: &str, source: &str) -> Result<Vec<Edit>, Error> {
    let refused = match sole(symbols, wanted) {
        Ok(symbol) => return Ok(vec![delete(tree, symbol, source)]),
        Err(error) => error,
    };
    let symbol = lookup(symbols, wanted)?;
    delete_name(tree, symbol, source).ok_or(refused)
}

fn delete(tree: &Tree, symbol: &Symbol, source: &str) -> Edit {
    if !owns_lines(tree, source, symbol) {
        return delete_inline(symbol, source);
    }
    let line = line_start(source, symbol.bytes[0]);
    let mut start = line;
    while start > 0 {
        let previous = line_start(source, start - 1);
        if !blank(source, [previous, start]) {
            break;
        }
        start = previous;
    }
    let mut end = next_line(source, symbol.bytes[1].saturating_sub(1).max(symbol.bytes[0]));
    while start == 0 && end < source.len() {
        let next = next_line(source, end);
        if !blank(source, [end, next]) {
            break;
        }
        end = next;
    }
    Edit {
        bytes: [start, end],
        expect: source.get(start..end).map(str::to_owned),
        replace: String::new(),
    }
}

fn top_level<'tree>(tree: &'tree Tree, kinds: &[&str]) -> Vec<Node<'tree>> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor).filter(|node| kinds.contains(&node.kind())).collect()
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(';').to_owned()
}

fn already_imported(nodes: &[Node<'_>], source: &str, import: &str) -> bool {
    let wanted = normalized(import);
    nodes
        .iter()
        .any(|node| normalized(source.get(node.byte_range()).unwrap_or_default()) == wanted)
}

fn preamble_end(tree: &Tree, source: &str, skipped: &[&str]) -> usize {
    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut end = 0;
    for node in root.named_children(&mut cursor) {
        let docstring = node.kind() == "expression_statement" && node.named_child(0).is_some_and(|child| child.kind() == "string");
        if !(skipped.contains(&node.kind()) || docstring) {
            break;
        }
        end = end_of_line(source, node.end_byte());
    }
    end
}

fn after_last(nodes: &[Node<'_>], source: &str, import: &str) -> Option<Edit> {
    let last = nodes.last()?;
    Some(insertion(end_of_line(source, last.end_byte()), format!("\n{import}")))
}

fn blank_lines_after(source: &str, position: usize) -> usize {
    let mut count = 0;
    let mut line = next_line(source, position);
    while line < source.len() && blank(source, [line, line_end(source, line)]) {
        count += 1;
        line = next_line(source, line);
    }
    count
}

fn definitions_follow(tree: &Tree, language: Language, position: usize) -> bool {
    let root = tree.root_node();
    let mut cursor = root.walk();
    let next = root
        .named_children(&mut cursor)
        .find(|node| node.start_byte() >= position && node.kind() != "comment");
    language == Language::Python
        && next.is_some_and(|node| matches!(node.kind(), "function_definition" | "class_definition" | "decorated_definition"))
}

fn at_top(tree: &Tree, source: &str, language: Language, position: usize, import: &str) -> Edit {
    let wanted = 1 + usize::from(definitions_follow(tree, language, position));
    if position == 0 {
        return insertion(0, format!("{import}\n{}", "\n".repeat(wanted)));
    }
    let missing = wanted.saturating_sub(blank_lines_after(source, position));
    insertion(position, format!("\n\n{import}{}", "\n".repeat(missing)))
}

fn go_spec(import: &str) -> String {
    let trimmed = import.trim();
    let spec = trimmed
        .strip_prefix("import")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .unwrap_or(trimmed)
        .trim();
    if spec.ends_with('"') || spec.ends_with('`') {
        return spec.to_owned();
    }
    format!("\"{spec}\"")
}

fn spread(list: Node<'_>, source: &str, spec: &str) -> Edit {
    let mut cursor = list.walk();
    let existing: Vec<String> = list
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "import_spec")
        .filter_map(|node| source.get(node.byte_range()).map(|text| format!("\t{text}")))
        .collect();
    let lines = existing.into_iter().chain(std::iter::once(format!("\t{spec}"))).collect::<Vec<_>>().join("\n");
    Edit {
        bytes: [list.start_byte(), list.end_byte()],
        expect: source.get(list.byte_range()).map(str::to_owned),
        replace: format!("(\n{lines}\n)"),
    }
}

fn add_go_import(tree: &Tree, source: &str, import: &str) -> Option<Edit> {
    let spec = go_spec(import);
    let declarations = top_level(tree, &["import_declaration"]);
    let mut specs = Vec::new();
    for declaration in &declarations {
        let mut cursor = declaration.walk();
        for child in declaration.named_children(&mut cursor) {
            if child.kind() == "import_spec" {
                specs.push(child);
            }
            if child.kind() == "import_spec_list" {
                let mut inner = child.walk();
                specs.extend(child.named_children(&mut inner).filter(|node| node.kind() == "import_spec"));
            }
        }
    }
    if already_imported(&specs, source, &spec) {
        return None;
    }
    let list = declarations.iter().find_map(|declaration| {
        let mut cursor = declaration.walk();
        declaration.named_children(&mut cursor).find(|child| child.kind() == "import_spec_list")
    });
    if let Some(list) = list {
        let close = list.end_byte().saturating_sub(1);
        if line_start(source, close) == line_start(source, list.start_byte()) {
            return Some(spread(list, source, &spec));
        }
        return Some(insertion(line_start(source, close), format!("\t{spec}\n")));
    }
    if let Some(edit) = after_last(&declarations, source, &format!("import {spec}")) {
        return Some(edit);
    }
    let package = top_level(tree, &["package_clause"]);
    let position = package.first().map_or(0, |node| end_of_line(source, node.end_byte()));
    Some(at_top(tree, source, Language::Go, position, &format!("import {spec}")))
}

fn zig_imports<'tree>(tree: &'tree Tree, source: &str) -> Vec<Node<'tree>> {
    top_level(tree, &["variable_declaration"])
        .into_iter()
        .filter(|node| source.get(node.byte_range()).is_some_and(|text| text.contains("@import(")))
        .collect()
}

pub fn add_import(tree: &Tree, source: &str, language: Language, import: &str) -> Result<Option<Edit>, Error> {
    let import = import.trim();
    let (imports, preamble): (Vec<Node<'_>>, &[&str]) = match language {
        Language::Go => return Ok(add_go_import(tree, source, import)),
        Language::Python => (
            top_level(tree, &["import_statement", "import_from_statement", "future_import_statement"]),
            &["comment"],
        ),
        Language::Rust => (top_level(tree, &["use_declaration", "extern_crate_declaration"]), &["line_comment", "block_comment", "inner_attribute_item"]),
        Language::JavaScript | Language::TypeScript | Language::Tsx => (top_level(tree, &["import_statement"]), &["comment", "hash_bang_line"]),
        Language::Zig => (zig_imports(tree, source), &["comment"]),
        _ => return Err(Error::Unsupported(format!("add_import is not available for {language}"))),
    };
    if already_imported(&imports, source, import) {
        return Ok(None);
    }
    if let Some(edit) = after_last(&imports, source, import) {
        return Ok(Some(edit));
    }
    Ok(Some(at_top(tree, source, language, preamble_end(tree, source, preamble), import)))
}

fn sole<'symbols>(symbols: &'symbols [Symbol], wanted: &str) -> Result<&'symbols Symbol, Error> {
    let symbol = lookup(symbols, wanted)?;
    let siblings: Vec<&str> = symbols
        .iter()
        .filter(|other| other.bytes == symbol.bytes && other.path != symbol.path)
        .map(|other| other.path.as_str())
        .collect();
    if siblings.is_empty() {
        return Ok(symbol);
    }
    Err(Error::Conflict(format!(
        "{} shares its declaration with {}; edit the declaration with apply instead",
        symbol.path,
        siblings.join(", ")
    )))
}

pub fn operation_edits(tree: &Tree, source: &str, language: Language, symbols: &[Symbol], operation: &Operation) -> Result<Vec<Edit>, Error> {
    let edit = match operation {
        Operation::Replace { symbol, code } => replace(sole(symbols, symbol)?, source, language, code),
        Operation::ReplaceBody { symbol, code } => replace_body(lookup(symbols, symbol)?, source, language, code)?,
        Operation::Insert { symbol, code, position } => insert(tree, lookup(symbols, symbol)?, source, language, code, *position)?,
        Operation::Delete { symbol } => return delete_symbol(tree, symbols, symbol, source),
        Operation::AddImport { import } => return Ok(add_import(tree, source, language, import)?.into_iter().collect()),
    };
    Ok(vec![edit])
}

fn resolve(path: &Path, language: Option<&str>) -> Result<Language, Error> {
    match language {
        Some(name) => name.parse(),
        None => Language::from_path(path),
    }
}

pub fn file_outline(path: &Path, language: Option<Language>) -> Result<Vec<Symbol>, Error> {
    let language = language.map_or_else(|| Language::from_path(path), Ok)?;
    let source = read(path)?;
    crate::symbols::outline_source(source.as_bytes(), language).map_err(|error| error.at(path))
}

pub(crate) fn dedent(text: &str, indent: &str, language: Language) -> String {
    let kept = protected(text, language);
    text.split('\n')
        .zip(kept)
        .enumerate()
        .map(|(index, (line, kept))| match (index, kept) {
            (0, _) | (_, true) => line,
            _ => line.strip_prefix(indent).unwrap_or(line),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn indent(text: &str, prefix: &str, language: Language) -> String {
    let kept = protected(text, language);
    text.split('\n')
        .zip(kept)
        .enumerate()
        .map(|(index, (line, kept))| match (index, kept) {
            (0, _) | (_, true) => line.to_owned(),
            _ if line.trim().is_empty() => line.to_owned(),
            _ => format!("{prefix}{line}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn get(path: &Path, language: Option<Language>, wanted: &str) -> Result<SymbolSource, Error> {
    let language = language.map_or_else(|| Language::from_path(path), Ok)?;
    let source = read(path)?;
    let tree = parse(source.as_bytes(), language)?;
    let symbols = outline(&tree, source.as_bytes(), language).map_err(|error| error.at(path))?;
    let symbol = lookup(&symbols, wanted).map_err(|error| error.at(path))?.clone();
    let raw = source.get(symbol.bytes[0]..symbol.bytes[1]).unwrap_or_default();
    let text = dedent(raw, &indent_at(&source, symbol.bytes[0]), language);
    Ok(SymbolSource { symbol, text })
}

fn boundary(text: &str, mut position: usize) -> usize {
    while !text.is_char_boundary(position) {
        position -= 1;
    }
    position
}

fn difference(old: &str, new: &str) -> Edit {
    let prefix = old.bytes().zip(new.bytes()).take_while(|(first, second)| first == second).count();
    let prefix = boundary(old, prefix);
    let (old_tail, new_tail) = (old.get(prefix..).unwrap_or_default(), new.get(prefix..).unwrap_or_default());
    let suffix = old_tail.bytes().rev().zip(new_tail.bytes().rev()).take_while(|(first, second)| first == second).count();
    let end = old.len() - suffix;
    let end = boundary(old, end).max(prefix);
    let replaced = new.len() - (old.len() - end);
    Edit {
        bytes: [prefix, end],
        expect: None,
        replace: new.get(prefix..replaced).unwrap_or_default().to_owned(),
    }
}

fn step(source: &str, language: Language, operation: &Operation) -> Result<(String, usize), Error> {
    let tree = parse(source.as_bytes(), language)?;
    let symbols = match operation {
        Operation::AddImport { .. } => Vec::new(),
        _ => outline(&tree, source.as_bytes(), language)?,
    };
    let edits = operation_edits(&tree, source, language, &symbols, operation)?;
    splice(source, &edits)
}

pub fn edit_source(source: &str, language: Language, operations: &[&Operation], label: &str, allow_errors: bool) -> Result<Change, Error> {
    let mut current = source.to_owned();
    let mut count = 0;
    for operation in operations {
        let (next, applied) = step(&current, language, operation)?;
        current = next;
        count += applied;
    }
    let mut change = apply(source, language, &[difference(source, &current)], label, allow_errors)?;
    change.edits = count;
    Ok(change)
}

fn edit_file(path: &Path, language: Language, operations: &[&Operation], allow_errors: bool) -> Result<FileChange, Error> {
    let source = read(path)?;
    let change = edit_source(&source, language, operations, &path.display().to_string(), allow_errors)?;
    Ok(FileChange {
        file: path.to_path_buf(),
        change,
    })
}

struct Group<'request> {
    file: &'request Path,
    language: Option<&'request str>,
    operations: Vec<&'request Operation>,
}

pub fn run(requests: &[Request], dry_run: bool, allow_errors: bool) -> Result<Vec<FileChange>, Error> {
    let mut groups: BTreeMap<PathBuf, Group<'_>> = BTreeMap::new();
    for request in requests {
        let key = canonical(&request.file)?;
        let group = groups.entry(key).or_insert_with(|| Group {
            file: &request.file,
            language: None,
            operations: Vec::new(),
        });
        group.language = agreed(group.language, request.language.as_deref(), &request.file)?;
        group.operations.push(&request.operation);
    }
    let results: Result<Vec<FileChange>, Error> = groups
        .into_par_iter()
        .map(|(_, group)| {
            let language = resolve(group.file, group.language).map_err(|error| error.at(group.file))?;
            edit_file(group.file, language, &group.operations, allow_errors).map_err(|error| error.at(group.file))
        })
        .collect();
    let changes: Vec<FileChange> = results?.into_iter().filter(|each| each.change.source != each.change.original).collect();
    commit(&changes, dry_run)?;
    Ok(changes)
}

fn identifier(node: Node<'_>) -> bool {
    let kind = node.kind();
    node.child_count() == 0
        && (kind.ends_with("identifier") || kind.ends_with("identifier_pattern") || matches!(kind, "word" | "variable_name"))
}

pub fn rename_edits(tree: &Tree, source: &str, old: &str, new: &str) -> Vec<Edit> {
    let mut edits = Vec::new();
    preorder(tree.root_node(), |node| {
        if identifier(node) && source.get(node.byte_range()) == Some(old) {
            edits.push(Edit {
                bytes: [node.start_byte(), node.end_byte()],
                expect: Some(old.to_owned()),
                replace: new.to_owned(),
            });
        }
        true
    });
    edits
}

pub fn rename(
    paths: &[PathBuf],
    language: Option<Language>,
    old: &str,
    new: &str,
    dry_run: bool,
    allow_errors: bool,
) -> Result<Vec<FileChange>, Error> {
    let files = collect(paths, language)?;
    let results = files
        .par_iter()
        .map(|target| {
            let Some(source) = read_target(target)? else {
                return Ok(None);
            };
            if !source.contains(old) {
                return Ok(None);
            }
            let tree = parse(source.as_bytes(), target.language).map_err(|error| error.at(&target.path))?;
            let file = Parsed { source, tree };
            changed(target, &file, &rename_edits(&file.tree, &file.source, old, new), allow_errors)
        })
        .collect();
    finish(results, dry_run)
}
