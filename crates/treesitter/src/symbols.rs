use std::sync::OnceLock;

use serde::Serialize;
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::Error;
use crate::format::text;
use crate::language::Language;

const PYTHON: &str = r"
(class_definition name: (identifier) @name) @definition.class
(function_definition name: (identifier) @name) @definition.function
";

const RUST: &str = r"
(function_item name: (identifier) @name) @definition.function
(function_signature_item name: (identifier) @name) @definition.function
(struct_item name: (type_identifier) @name) @definition.struct
(enum_item name: (type_identifier) @name) @definition.enum
(union_item name: (type_identifier) @name) @definition.union
(trait_item name: (type_identifier) @name) @definition.trait
(impl_item type: (_) @name) @definition.impl
(mod_item name: (identifier) @name) @definition.module
(type_item name: (type_identifier) @name) @definition.type
(const_item name: (identifier) @name) @definition.constant
(static_item name: (identifier) @name) @definition.constant
(macro_definition name: (identifier) @name) @definition.macro
";

const GO: &str = r"
(function_declaration name: (identifier) @name) @definition.function
(method_declaration receiver: (parameter_list (parameter_declaration type: (_) @receiver)) name: (field_identifier) @name) @definition.method
(type_spec name: (type_identifier) @name) @definition.type
(type_alias name: (type_identifier) @name) @definition.type
(const_spec name: (identifier) @name) @definition.constant
(var_spec name: (identifier) @name) @definition.variable
";

const JAVASCRIPT: &str = r"
(function_declaration name: (identifier) @name) @definition.function
(generator_function_declaration name: (identifier) @name) @definition.function
(class_declaration name: (_) @name) @definition.class
(method_definition name: (_) @name) @definition.method
(lexical_declaration (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @definition.function
";

const TYPESCRIPT: &str = r"
(function_declaration name: (identifier) @name) @definition.function
(generator_function_declaration name: (identifier) @name) @definition.function
(function_signature name: (identifier) @name) @definition.function
(class_declaration name: (_) @name) @definition.class
(abstract_class_declaration name: (_) @name) @definition.class
(method_definition name: (_) @name) @definition.method
(abstract_method_signature name: (_) @name) @definition.method
(interface_declaration name: (_) @name) @definition.interface
(type_alias_declaration name: (_) @name) @definition.type
(enum_declaration name: (_) @name) @definition.enum
(lexical_declaration (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @definition.function
";

const ZIG: &str = r#"
(function_declaration name: (identifier) @name) @definition.function
(variable_declaration (identifier) @name [(struct_declaration) (enum_declaration) (union_declaration) (opaque_declaration)]) @definition.struct
(test_declaration (string) @name) @definition.test
"#;

const BASH: &str = r"
(function_definition name: (word) @name) @definition.function
";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub start: [usize; 2],
    pub end: [usize; 2],
    pub bytes: [usize; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<[usize; 2]>,
    pub signature: String,
}

fn definitions(language: Language) -> Option<&'static str> {
    match language {
        Language::Python => Some(PYTHON),
        Language::Rust => Some(RUST),
        Language::Go => Some(GO),
        Language::JavaScript => Some(JAVASCRIPT),
        Language::TypeScript | Language::Tsx => Some(TYPESCRIPT),
        Language::Zig => Some(ZIG),
        Language::Bash => Some(BASH),
        _ => None,
    }
}

fn query(language: Language) -> Result<&'static Query, Error> {
    static QUERIES: [OnceLock<Result<Query, String>>; Language::ALL.len()] = [const { OnceLock::new() }; Language::ALL.len()];
    let unsupported = || Error::Unsupported(format!("symbols are not available for {language}"));
    let source = definitions(language).ok_or_else(unsupported)?;
    let index = Language::ALL.iter().position(|each| *each == language).ok_or_else(unsupported)?;
    let slot = QUERIES.get(index).ok_or_else(unsupported)?;
    let compiled = slot.get_or_init(|| Query::new(&language.grammar(), source).map_err(|error| error.to_string()));
    compiled.as_ref().map_err(|message| Error::Pattern(message.clone()))
}

fn wrapper(language: Language, node: Node<'_>) -> bool {
    match language {
        Language::Python => node.kind() == "decorated_definition",
        Language::JavaScript | Language::TypeScript | Language::Tsx => node.kind() == "export_statement",
        Language::Go => {
            matches!(node.kind(), "type_declaration" | "const_declaration" | "var_declaration") && node.named_child_count() == 1
        }
        _ => false,
    }
}

fn doc_comment(language: Language, node: Node<'_>, source: &[u8]) -> bool {
    let content = text(node, source);
    match (language, node.kind()) {
        (Language::Rust, "line_comment" | "block_comment") => {
            (content.starts_with("///") && !content.starts_with("////")) || content.starts_with("/**")
        }
        (Language::Zig, "comment") => content.starts_with("///"),
        (Language::Go, "comment") => true,
        (Language::JavaScript | Language::TypeScript | Language::Tsx, "comment") => content.starts_with("/**"),
        _ => false,
    }
}

fn attribute(language: Language, node: Node<'_>) -> bool {
    language == Language::Rust && node.kind() == "attribute_item"
}

struct Extent<'tree> {
    first: Node<'tree>,
    signature: Node<'tree>,
    last: Node<'tree>,
}

fn extent<'tree>(language: Language, node: Node<'tree>, source: &[u8]) -> Extent<'tree> {
    let mut outer = node;
    while let Some(parent) = outer.parent().filter(|parent| wrapper(language, *parent)) {
        outer = parent;
    }
    let mut first = outer;
    let mut signature = outer;
    let mut previous = outer.prev_sibling();
    while let Some(sibling) = previous {
        let adjacent = sibling.end_position().row + 1 >= first.start_position().row;
        let is_attribute = attribute(language, sibling);
        if !adjacent || !(is_attribute || doc_comment(language, sibling, source)) {
            break;
        }
        first = sibling;
        if is_attribute {
            signature = sibling;
        }
        previous = sibling.prev_sibling();
    }
    Extent {
        first,
        signature,
        last: outer,
    }
}

fn body(node: Node<'_>, depth: usize) -> Option<Node<'_>> {
    if let Some(found) = node.child_by_field_name("body") {
        return Some(found);
    }
    if matches!(node.kind(), "struct_declaration" | "enum_declaration" | "union_declaration" | "opaque_declaration") {
        return Some(node);
    }
    if depth == 0 {
        return None;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    children.into_iter().find_map(|child| body(child, depth - 1))
}

fn clean_name(kind: &str, raw: &str) -> String {
    let trimmed = raw.trim_start_matches('*').trim_matches('"');
    if !matches!(kind, "impl" | "method") {
        return trimmed.to_owned();
    }
    trimmed.split(['<', '[']).next().unwrap_or(trimmed).trim().to_owned()
}

struct Definition {
    signature_start: usize,
    name: String,
    title: Option<String>,
    kind: String,
    receiver: Option<String>,
    bytes: [usize; 2],
    body: Option<[usize; 2]>,
    start: [usize; 2],
    end: [usize; 2],
}

pub fn outline(tree: &Tree, source: &[u8], language: Language) -> Result<Vec<Symbol>, Error> {
    let query = query(language)?;
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut found: Vec<Definition> = Vec::new();
    let mut iterator = cursor.matches(query, tree.root_node(), source);
    while let Some(each) = iterator.next() {
        let mut definition = None;
        let mut name = None;
        let mut receiver = None;
        for capture in each.captures() {
            let label = names.get(capture.index as usize).copied().unwrap_or_default();
            match label {
                "name" => name = Some(text(capture.node, source).into_owned()),
                "receiver" => receiver = Some(clean_name("method", &text(capture.node, source))),
                _ => definition = label.strip_prefix("definition.").map(|kind| (kind, capture.node)),
            }
        }
        let (Some((kind, node)), Some(name)) = (definition, name) else {
            continue;
        };
        let Extent { first, signature, last } = extent(language, node, source);
        let bytes = [first.start_byte(), last.end_byte()];
        if found.iter().any(|existing| existing.bytes == bytes) {
            continue;
        }
        let start = first.start_position();
        let end = last.end_position();
        let name = clean_name(kind, &name);
        let title = (kind == "impl").then(|| {
            let implemented = node.child_by_field_name("trait").map(|node| clean_name("impl", &text(node, source)));
            implemented.map_or_else(|| format!("impl {name}"), |implemented| format!("impl {implemented} for {name}"))
        });
        found.push(Definition {
            signature_start: signature.start_byte(),
            name,
            title,
            kind: kind.to_owned(),
            receiver,
            bytes,
            body: body(node, 3).map(|body| [body.start_byte(), body.end_byte()]),
            start: [start.row, start.column],
            end: [end.row, end.column],
        });
    }
    found.sort_by_key(|definition| (definition.bytes[0], std::cmp::Reverse(definition.bytes[1])));
    Ok(nest(found, source))
}

fn nest(found: Vec<Definition>, source: &[u8]) -> Vec<Symbol> {
    let mut symbols: Vec<Symbol> = Vec::with_capacity(found.len());
    let mut scopes: Vec<String> = Vec::with_capacity(found.len());
    let mut stack: Vec<usize> = Vec::new();
    for definition in found {
        while stack
            .last()
            .and_then(|index| symbols.get(*index))
            .is_some_and(|parent| parent.bytes[1] <= definition.bytes[0])
        {
            stack.pop();
        }
        let parent = stack.last().and_then(|index| symbols.get(*index));
        let mut kind = definition.kind;
        if kind == "function" && parent.is_some_and(|parent| matches!(parent.kind.as_str(), "class" | "impl" | "trait")) {
            kind = "method".to_owned();
        }
        let scope = stack.last().and_then(|index| scopes.get(*index)).cloned();
        let prefix = definition.receiver.or(scope);
        let scoped = prefix.map_or_else(|| definition.name.clone(), |prefix| format!("{prefix}.{}", definition.name));
        scopes.push(scoped.clone());
        let path = definition.title.unwrap_or(scoped);
        let signature_end = definition.body.map_or(definition.bytes[1], |body| body[0]);
        let signature = String::from_utf8_lossy(source.get(definition.signature_start..signature_end).unwrap_or_default());
        let signature = signature.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ");
        stack.push(symbols.len());
        symbols.push(Symbol {
            name: definition.name,
            path,
            kind,
            start: definition.start,
            end: definition.end,
            bytes: definition.bytes,
            body: definition.body,
            signature: signature.trim_end_matches(['{', ':']).trim().to_owned(),
        });
    }
    symbols
}

fn candidates<'symbols>(symbols: &'symbols [Symbol], wanted: &str) -> Vec<&'symbols Symbol> {
    let exact: Vec<&Symbol> = symbols.iter().filter(|symbol| symbol.path == wanted).collect();
    if !exact.is_empty() {
        return exact;
    }
    let suffix = format!(".{wanted}");
    symbols
        .iter()
        .filter(|symbol| symbol.name == wanted || symbol.path.ends_with(&suffix))
        .collect()
}

fn without_impls(candidates: Vec<&Symbol>) -> Vec<&Symbol> {
    let preferred: Vec<&Symbol> = candidates.iter().copied().filter(|symbol| symbol.kind != "impl").collect();
    if preferred.is_empty() {
        return candidates;
    }
    preferred
}

pub fn lookup<'symbols>(symbols: &'symbols [Symbol], wanted: &str) -> Result<&'symbols Symbol, Error> {
    let chosen = without_impls(candidates(symbols, wanted));
    match chosen[..] {
        [only] => Ok(only),
        [] => Err(Error::NoSymbol {
            name: wanted.to_owned(),
            available: symbols.iter().map(|symbol| symbol.path.clone()).collect(),
        }),
        _ => Err(Error::Ambiguous {
            name: wanted.to_owned(),
            candidates: chosen.iter().map(|symbol| format!("{} ({}:{})", symbol.path, symbol.kind, symbol.start[0] + 1)).collect(),
        }),
    }
}
