use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Tree};

use crate::edit::{Edit, apply};
use crate::error::Error;
use crate::files::{FileChange, changed, collect, commit, finish, read, read_target};
use crate::language::Language;
use crate::parse::parse;
use crate::symbols::{Symbol, lookup, outline};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Before,
    After,
    Start,
    End,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    Replace { symbol: String, code: String },
    ReplaceBody { symbol: String, code: String },
    Insert { symbol: String, code: String, position: Position },
    Delete { symbol: String },
    AddImport { import: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Request {
    pub file: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(flatten)]
    pub operation: Operation,
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
    source
        .get(position..)
        .and_then(|tail| tail.find('\n'))
        .map_or(source.len(), |index| position + index)
}

fn indent_at(source: &str, position: usize) -> String {
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

pub fn reindent(code: &str, indent: &str) -> String {
    let lines: Vec<&str> = code.trim_matches('\n').lines().collect();
    let common = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                return String::new();
            }
            format!("{indent}{}", line.get(common..).unwrap_or(line).trim_end())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct Body {
    braces: bool,
    inner: [usize; 2],
    indent: String,
    closing: String,
}

fn block_indent(source: &str, symbol_start: usize, start: usize, nested: String) -> String {
    if line_start(source, start) == line_start(source, symbol_start) {
        return nested;
    }
    indent_at(source, start)
}

fn body(source: &str, language: Language, symbol: &Symbol) -> Result<Body, Error> {
    let [start, end] = symbol
        .body
        .ok_or_else(|| Error::Unsupported(format!("{} {} has no body", symbol.kind, symbol.path)))?;
    let text = source.get(start..end).unwrap_or_default();
    let symbol_indent = indent_at(source, symbol.bytes[0]);
    let nested = format!("{symbol_indent}{}", indent_unit(language, source));
    let (Some(open), Some(close)) = (text.find('{'), text.rfind('}')) else {
        return Ok(Body {
            braces: false,
            inner: [start, end],
            indent: block_indent(source, symbol.bytes[0], start, nested),
            closing: symbol_indent,
        });
    };
    let inner = [start + open + 1, start + close];
    let content = source.get(inner[0]..inner[1]).unwrap_or_default();
    let first = content.lines().skip(1).find(|line| !line.trim().is_empty());
    let indent = first.map_or(nested, |line| line.chars().take_while(|character| character.is_whitespace()).collect());
    Ok(Body {
        braces: true,
        inner,
        indent,
        closing: indent_at(source, inner[1]),
    })
}

fn blank(source: &str, range: [usize; 2]) -> bool {
    source.get(range[0]..range[1]).is_some_and(|content| content.trim().is_empty())
}

fn replace(symbol: &Symbol, source: &str, code: &str) -> Edit {
    let start = line_start(source, symbol.bytes[0]);
    let replacement = reindent(code, &indent_at(source, symbol.bytes[0]));
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

fn wrap_braces(body: &Body, code: &str) -> String {
    format!("\n{}\n{}", reindent(code, &body.indent), body.closing)
}

fn body_replacement(body: &Body, code: &str) -> String {
    if body.braces {
        return wrap_braces(body, code);
    }
    reindent(code, &body.indent).trim_start().to_owned()
}

fn replace_body(symbol: &Symbol, source: &str, language: Language, code: &str) -> Result<Edit, Error> {
    let body = body(source, language, symbol)?;
    Ok(Edit {
        bytes: body.inner,
        expect: source.get(body.inner[0]..body.inner[1]).map(str::to_owned),
        replace: body_replacement(&body, code),
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

fn insert(symbol: &Symbol, source: &str, language: Language, code: &str, position: Position) -> Result<Edit, Error> {
    let indent = indent_at(source, symbol.bytes[0]);
    match position {
        Position::Before => return Ok(insertion(line_start(source, symbol.bytes[0]), format!("{}\n\n", reindent(code, &indent)))),
        Position::After => return Ok(insertion(line_end(source, symbol.bytes[1]), format!("\n\n{}", reindent(code, &indent)))),
        Position::Start | Position::End => {}
    }
    let body = body(source, language, symbol)?;
    let code = reindent(code, &body.indent);
    if body.braces && blank(source, body.inner) {
        return Ok(Edit {
            bytes: body.inner,
            expect: source.get(body.inner[0]..body.inner[1]).map(str::to_owned),
            replace: format!("\n{code}\n{}", body.closing),
        });
    }
    let gap = gap(&code);
    let edit = match (position, body.braces) {
        (Position::Start, true) => insertion(body.inner[0], format!("\n{code}{gap}")),
        (Position::Start, false) => insertion(line_start(source, body.inner[0]), format!("{code}\n{gap}")),
        (_, true) if blank(source, [line_start(source, body.inner[1]), body.inner[1]]) => {
            insertion(line_start(source, body.inner[1]), format!("{gap}{code}\n"))
        }
        (_, true) => insertion(body.inner[1], format!("\n{code}\n{}", body.closing)),
        (_, false) => insertion(line_end(source, body.inner[1]), format!("\n{gap}{code}")),
    };
    Ok(edit)
}

fn delete(symbol: &Symbol, source: &str) -> Edit {
    let line = line_start(source, symbol.bytes[0]);
    if !blank(source, [line, symbol.bytes[0]]) {
        return Edit {
            bytes: symbol.bytes,
            expect: source.get(symbol.bytes[0]..symbol.bytes[1]).map(str::to_owned),
            replace: String::new(),
        };
    }
    let mut start = line;
    while start > 0 {
        let previous = line_start(source, start - 1);
        if !blank(source, [previous, start]) {
            break;
        }
        start = previous;
    }
    let mut end = (line_end(source, symbol.bytes[1]) + 1).min(source.len());
    while start == 0 && end < source.len() {
        let next = (line_end(source, end) + 1).min(source.len());
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
        end = line_end(source, node.end_byte());
    }
    end
}

fn after_last(nodes: &[Node<'_>], source: &str, import: &str) -> Option<Edit> {
    let last = nodes.last()?;
    Some(insertion(line_end(source, last.end_byte()), format!("\n{import}")))
}

fn at_top(position: usize, import: &str) -> Edit {
    if position == 0 {
        return insertion(0, format!("{import}\n\n"));
    }
    insertion(position, format!("\n\n{import}"))
}

fn go_spec(import: &str) -> String {
    let spec = import.trim().trim_start_matches("import").trim();
    if spec.ends_with('"') || spec.ends_with('`') {
        return spec.to_owned();
    }
    format!("\"{spec}\"")
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
        return Some(insertion(line_start(source, close), format!("\t{spec}\n")));
    }
    if let Some(edit) = after_last(&declarations, source, &format!("import {spec}")) {
        return Some(edit);
    }
    let package = top_level(tree, &["package_clause"]);
    let position = package.first().map_or(0, |node| line_end(source, node.end_byte()));
    Some(at_top(position, &format!("import {spec}")))
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
        Language::JavaScript | Language::TypeScript | Language::Tsx => (top_level(tree, &["import_statement"]), &["comment"]),
        Language::Zig => (zig_imports(tree, source), &["comment"]),
        _ => return Err(Error::Unsupported(format!("add_import is not available for {language}"))),
    };
    if already_imported(&imports, source, import) {
        return Ok(None);
    }
    if let Some(edit) = after_last(&imports, source, import) {
        return Ok(Some(edit));
    }
    Ok(Some(at_top(preamble_end(tree, source, preamble), import)))
}

pub fn operation_edits(tree: &Tree, source: &str, language: Language, symbols: &[Symbol], operation: &Operation) -> Result<Vec<Edit>, Error> {
    let edit = match operation {
        Operation::Replace { symbol, code } => replace(lookup(symbols, symbol)?, source, code),
        Operation::ReplaceBody { symbol, code } => replace_body(lookup(symbols, symbol)?, source, language, code)?,
        Operation::Insert { symbol, code, position } => insert(lookup(symbols, symbol)?, source, language, code, *position)?,
        Operation::Delete { symbol } => delete(lookup(symbols, symbol)?, source),
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
    let tree = parse(source.as_bytes(), language)?;
    outline(&tree, source.as_bytes(), language).map_err(|error| error.at(path))
}

pub fn get(path: &Path, language: Option<Language>, wanted: &str) -> Result<SymbolSource, Error> {
    let language = language.map_or_else(|| Language::from_path(path), Ok)?;
    let source = read(path)?;
    let tree = parse(source.as_bytes(), language)?;
    let symbols = outline(&tree, source.as_bytes(), language).map_err(|error| error.at(path))?;
    let symbol = lookup(&symbols, wanted).map_err(|error| error.at(path))?.clone();
    let text = source.get(symbol.bytes[0]..symbol.bytes[1]).unwrap_or_default().to_owned();
    Ok(SymbolSource { symbol, text })
}

pub fn operations_edits(tree: &Tree, source: &str, language: Language, operations: &[&Operation]) -> Result<Vec<Edit>, Error> {
    let needs_symbols = operations.iter().any(|operation| !matches!(operation, Operation::AddImport { .. }));
    let symbols = needs_symbols
        .then(|| outline(tree, source.as_bytes(), language))
        .transpose()?
        .unwrap_or_default();
    let mut edits = Vec::new();
    for operation in operations {
        edits.extend(operation_edits(tree, source, language, &symbols, operation)?);
    }
    Ok(edits)
}

fn edit_file(path: &Path, language: Language, operations: &[&Operation], allow_errors: bool) -> Result<FileChange, Error> {
    let source = read(path)?;
    let tree = parse(source.as_bytes(), language)?;
    let edits = operations_edits(&tree, &source, language, operations)?;
    let change = apply(&source, language, &edits, &path.display().to_string(), allow_errors)?;
    Ok(FileChange {
        file: path.to_path_buf(),
        change,
    })
}

pub fn run(requests: &[Request], dry_run: bool, allow_errors: bool) -> Result<Vec<FileChange>, Error> {
    let mut groups: BTreeMap<&Path, (Option<&str>, Vec<&Operation>)> = BTreeMap::new();
    for request in requests {
        let entry = groups.entry(request.file.as_path()).or_insert_with(|| (request.language.as_deref(), Vec::new()));
        entry.1.push(&request.operation);
    }
    let results: Result<Vec<FileChange>, Error> = groups
        .into_par_iter()
        .map(|(path, (language, operations))| {
            let language = resolve(path, language).map_err(|error| error.at(path))?;
            edit_file(path, language, &operations, allow_errors).map_err(|error| error.at(path))
        })
        .collect();
    let changes = results?;
    commit(&changes, dry_run)?;
    Ok(changes)
}

fn identifier(node: Node<'_>) -> bool {
    node.child_count() == 0 && (node.kind().ends_with("identifier") || node.kind() == "word")
}

pub fn rename_edits(tree: &Tree, source: &str, old: &str, new: &str) -> Vec<Edit> {
    let mut edits = Vec::new();
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        if identifier(node) && source.get(node.byte_range()) == Some(old) {
            edits.push(Edit {
                bytes: [node.start_byte(), node.end_byte()],
                expect: Some(old.to_owned()),
                replace: new.to_owned(),
            });
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return edits;
            }
        }
    }
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
            changed(target, &source, &rename_edits(&tree, &source, old, new), allow_errors)
        })
        .collect();
    finish(results, dry_run)
}
