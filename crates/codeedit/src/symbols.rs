use std::collections::HashSet;
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
(variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)]) @definition.function
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
(variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)]) @definition.function
";

const ZIG: &str = r#"
(function_declaration name: (identifier) @name) @definition.function
(variable_declaration (identifier) @name [(struct_declaration) (enum_declaration) (union_declaration) (opaque_declaration)]) @definition.struct
(test_declaration (string) @name) @definition.test
"#;

const BASH: &str = r"
(function_definition name: (word) @name) @definition.function
";

const JSON: &str = r"
(pair key: (string (string_content) @name)) @definition.key
";

const YAML: &str = r"
(block_mapping_pair key: (flow_node) @name) @definition.key
(flow_pair key: (flow_node) @name) @definition.key
";

const MARKDOWN: &str = r"
(section . (atx_heading heading_content: (inline) @name)) @definition.section
(section . (setext_heading (paragraph) @name)) @definition.section
";

const HTML: &str = r"
(element (start_tag (tag_name) @name)) @definition.element
(element (self_closing_tag (tag_name) @name)) @definition.element
(script_element (start_tag (tag_name) @name)) @definition.element
(style_element (start_tag (tag_name) @name)) @definition.element
";

const XML: &str = r"
(element (STag (Name) @name)) @definition.element
(element (EmptyElemTag (Name) @name)) @definition.element
";

const RUBY: &str = r"
(class name: (_) @name) @definition.class
(module name: (_) @name) @definition.module
(method name: (_) @name) @definition.method
(singleton_method name: (_) @name) @definition.method
(assignment left: (constant) @name) @definition.constant
";

const CRYSTAL: &str = r"
(class_def name: (_) @name) @definition.class
(struct_def name: (_) @name) @definition.struct
(module_def name: (_) @name) @definition.module
(enum_def name: (_) @name) @definition.enum
(lib_def name: (_) @name) @definition.module
(annotation_def name: (_) @name) @definition.class
(method_def name: (_) @name) @definition.method
(macro_def name: (_) @name) @definition.macro
(fun_def name: (_) @name) @definition.function
(const_assign lhs: (_) @name) @definition.constant
";

const PHP: &str = r"
(class_declaration name: (_) @name) @definition.class
(interface_declaration name: (_) @name) @definition.interface
(trait_declaration name: (_) @name) @definition.trait
(enum_declaration name: (_) @name) @definition.enum
(namespace_definition name: (_) @name) @definition.module
(function_definition name: (_) @name) @definition.function
(method_declaration name: (_) @name) @definition.method
(const_declaration (const_element (name) @name)) @definition.constant
(property_declaration (property_element name: (_) @name)) @definition.field
(enum_case name: (_) @name) @definition.constant
";

const C: &str = r"
(function_definition declarator: (_) @declarator) @definition.function
(struct_specifier name: (_) @name body: (_)) @definition.struct
(union_specifier name: (_) @name body: (_)) @definition.union
(enum_specifier name: (_) @name body: (_)) @definition.enum
(type_definition declarator: (type_identifier) @name) @definition.type
(preproc_def name: (_) @name) @definition.macro
(preproc_function_def name: (_) @name) @definition.macro
";

const CPP: &str = r"
(function_definition declarator: (_) @declarator) @definition.function
(field_declaration_list (field_declaration declarator: (function_declarator) @declarator) @definition.method)
(field_declaration_list (declaration declarator: (function_declarator) @declarator) @definition.method)
(class_specifier name: (_) @name body: (_)) @definition.class
(struct_specifier name: (_) @name body: (_)) @definition.struct
(union_specifier name: (_) @name body: (_)) @definition.union
(enum_specifier name: (_) @name body: (_)) @definition.enum
(namespace_definition name: (_) @name) @definition.module
(type_definition declarator: (type_identifier) @name) @definition.type
(alias_declaration name: (_) @name) @definition.type
(concept_definition name: (_) @name) @definition.concept
(preproc_def name: (_) @name) @definition.macro
(preproc_function_def name: (_) @name) @definition.macro
";

const JAVA: &str = r"
(class_declaration name: (_) @name) @definition.class
(interface_declaration name: (_) @name) @definition.interface
(enum_declaration name: (_) @name) @definition.enum
(record_declaration name: (_) @name) @definition.record
(annotation_type_declaration name: (_) @name) @definition.annotation
(method_declaration name: (_) @name) @definition.method
(constructor_declaration name: (_) @name) @definition.constructor
(field_declaration declarator: (variable_declarator name: (_) @name)) @definition.field
(enum_constant name: (_) @name) @definition.constant
";

const KOTLIN: &str = r#"
(class_declaration "class" name: (_) @name) @definition.class
(class_declaration "interface" name: (_) @name) @definition.interface
(object_declaration name: (_) @name) @definition.object
(companion_object name: (_) @name) @definition.object
(function_declaration name: (_) @name) @definition.function
(property_declaration (variable_declaration (identifier) @name)) @definition.property
(type_alias type: (_) @name) @definition.type
"#;

const SWIFT: &str = r#"
(class_declaration "class" name: (_) @name) @definition.class
(class_declaration "actor" name: (_) @name) @definition.class
(class_declaration "struct" name: (_) @name) @definition.struct
(class_declaration "enum" name: (_) @name) @definition.enum
(class_declaration "extension" name: (_) @name) @definition.extension
(protocol_declaration name: (_) @name) @definition.protocol
(function_declaration name: (simple_identifier) @name) @definition.function
(protocol_function_declaration name: (simple_identifier) @name) @definition.method
(init_declaration "init" @name) @definition.constructor
(deinit_declaration "deinit" @name) @definition.method
(property_declaration name: (pattern bound_identifier: (simple_identifier) @name)) @definition.property
(typealias_declaration name: (type_identifier) @name) @definition.type
"#;

const CSHARP: &str = r"
(class_declaration name: (_) @name) @definition.class
(struct_declaration name: (_) @name) @definition.struct
(interface_declaration name: (_) @name) @definition.interface
(enum_declaration name: (_) @name) @definition.enum
(record_declaration name: (_) @name) @definition.record
(namespace_declaration name: (_) @name) @definition.module
(file_scoped_namespace_declaration name: (_) @name) @definition.module
(method_declaration name: (_) @name) @definition.method
(constructor_declaration name: (_) @name) @definition.constructor
(destructor_declaration name: (_) @name) @definition.method
(local_function_statement name: (_) @name) @definition.function
(property_declaration name: (_) @name) @definition.property
(field_declaration (variable_declaration (variable_declarator name: (_) @name))) @definition.field
(event_field_declaration (variable_declaration (variable_declarator name: (_) @name))) @definition.field
(delegate_declaration name: (_) @name) @definition.type
(enum_member_declaration name: (_) @name) @definition.constant
";

const SCALA: &str = r"
(class_definition name: (_) @name) @definition.class
(object_definition name: (_) @name) @definition.object
(trait_definition name: (_) @name) @definition.trait
(enum_definition name: (_) @name) @definition.enum
(type_definition name: (_) @name) @definition.type
(function_definition name: (_) @name) @definition.function
(function_declaration name: (_) @name) @definition.function
(val_definition pattern: (identifier) @name) @definition.variable
(var_definition pattern: (identifier) @name) @definition.variable
(val_declaration name: (identifier) @name) @definition.variable
(var_declaration name: (identifier) @name) @definition.variable
";

const DART: &str = r"
(class_declaration name: (_) @name) @definition.class
(mixin_declaration name: (_) @name) @definition.mixin
(enum_declaration name: (_) @name) @definition.enum
(extension_declaration name: (_) @name) @definition.extension
(function_declaration signature: (function_signature name: (identifier) @name)) @definition.function
(method_declaration signature: (method_signature [(function_signature name: (identifier) @name) (getter_signature name: (identifier) @name) (setter_signature name: (identifier) @name)])) @definition.method
(declaration (constructor_signature name: (identifier) @name)) @definition.constructor
(declaration (initialized_identifier_list (initialized_identifier name: (identifier) @name))) @definition.field
(declaration (static_final_declaration_list (static_final_declaration name: (identifier) @name))) @definition.field
(top_level_variable_declaration (static_final_declaration_list (static_final_declaration name: (identifier) @name))) @definition.variable
(top_level_variable_declaration (initialized_identifier_list (initialized_identifier name: (identifier) @name))) @definition.variable
(type_alias (type_identifier) @name) @definition.type
(enum_entry name: (identifier) @name) @definition.constant
";

const DOCKERFILE: &str = r"
(from_instruction (image_alias) @name) @definition.stage
(arg_instruction name: (_) @name) @definition.variable
(env_pair name: (_) @name) @definition.variable
";

const LUA: &str = r"
(function_declaration name: (_) @name) @definition.function
(chunk (variable_declaration (assignment_statement (variable_list name: (identifier) @name))) @definition.variable)
(chunk (assignment_statement (variable_list name: (_) @name)) @definition.variable)
";

const ELIXIR: &str = r#"
((call target: (identifier) @keyword (arguments (alias) @name) (do_block)) @definition.module (#match? @keyword "^(defmodule|defprotocol|defimpl)$"))
((call target: (identifier) @keyword (arguments . [(call target: (identifier) @name) (identifier) @name])) @definition.function (#match? @keyword "^(def|defp|defmacro|defmacrop|defguard|defguardp|defdelegate)$"))
"#;

const HASKELL: &str = r"
(signature name: (variable) @name) @definition.signature
(function name: (variable) @name) @definition.function
(bind name: (variable) @name) @definition.function
(data_type name: (name) @name) @definition.type
(newtype name: (name) @name) @definition.type
(type_synonym name: (name) @name) @definition.type
(type_family name: (name) @name) @definition.type
(class name: (name) @name) @definition.class
(instance name: (name) @name patterns: (_) @patterns) @definition.instance
";

const OCAML: &str = r"
(value_definition (let_binding pattern: (value_name) @name (parameter))) @definition.function
(value_definition (let_binding pattern: (value_name) @name . body: (_))) @definition.variable
(type_definition (type_binding name: (type_constructor) @name)) @definition.type
(module_definition (module_binding (module_name) @name)) @definition.module
(module_type_definition (module_type_name) @name) @definition.module
(exception_definition (constructor_declaration (constructor_name) @name)) @definition.exception
(external (value_name) @name) @definition.function
";

const NIX: &str = r"
(binding attrpath: (attrpath) @name) @definition.key
";

const PROTO: &str = r"
(message (message_name (identifier) @name)) @definition.message
(enum (enum_name (identifier) @name)) @definition.enum
(service (service_name (identifier) @name)) @definition.service
(rpc (rpc_name (identifier) @name)) @definition.method
(field (identifier) @name) @definition.field
(enum_field (identifier) @name) @definition.constant
";

const GRAPHQL: &str = r"
(object_type_definition (name) @name) @definition.type
(interface_type_definition (name) @name) @definition.interface
(enum_type_definition (name) @name) @definition.enum
(input_object_type_definition (name) @name) @definition.type
(union_type_definition (name) @name) @definition.union
(scalar_type_definition (name) @name) @definition.type
(directive_definition (name) @name) @definition.directive
(operation_definition (name) @name) @definition.operation
(fragment_definition (fragment_name (name) @name)) @definition.fragment
(field_definition (name) @name) @definition.field
";

const INI: &str = r"
(section (section_name (text) @name)) @definition.section
(setting (setting_name) @name) @definition.key
";

const TOML: &str = r"
(table . [(bare_key) (dotted_key) (quoted_key)] @name) @definition.table
(table_array_element . [(bare_key) (dotted_key) (quoted_key)] @name) @definition.table
(pair . [(bare_key) (dotted_key) (quoted_key)] @name) @definition.key
";

const HCL: &str = r"
(block . (identifier) @name (string_lit)* @label) @definition.block
(attribute . (identifier) @name) @definition.key
";

const MAKE: &str = r"
(rule (targets) @name) @definition.target
(variable_assignment name: (_) @name) @definition.variable
(define_directive name: (_) @name) @definition.variable
";

const CMAKE: &str = r#"
(function_def (function_command (argument_list . (argument) @name))) @definition.function
(macro_def (macro_command (argument_list . (argument) @name))) @definition.macro
((normal_command (identifier) @command (argument_list . (argument) @name)) @definition.variable (#match? @command "^(?i)(set|option)$"))
"#;

const CSS: &str = r"
(rule_set (selectors) @name) @definition.rule
(media_statement) @definition.media
(supports_statement) @definition.supports
(keyframes_statement (keyframes_name) @name) @definition.keyframes
";

const SCSS: &str = r"
(rule_set (selectors) @name) @definition.rule
(media_statement) @definition.media
(keyframes_statement (keyframes_name) @name) @definition.keyframes
(mixin_statement name: (_) @name) @definition.mixin
(function_statement name: (_) @name) @definition.function
(stylesheet (declaration (property_name) @name) @definition.variable)
";

const SQL: &str = r"
(create_table (object_reference name: (identifier) @name)) @definition.table
(create_view (object_reference name: (identifier) @name)) @definition.view
(create_materialized_view (object_reference name: (identifier) @name)) @definition.view
(create_index column: (identifier) @name) @definition.index
(create_function (object_reference name: (identifier) @name)) @definition.function
(create_type (object_reference name: (identifier) @name)) @definition.type
(create_schema (identifier) @name) @definition.schema
(create_sequence (object_reference name: (identifier) @name)) @definition.sequence
(column_definition name: (identifier) @name) @definition.column
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

fn definitions(language: Language) -> &'static str {
    match language {
        Language::Python => PYTHON,
        Language::Rust => RUST,
        Language::Go => GO,
        Language::JavaScript => JAVASCRIPT,
        Language::TypeScript | Language::Tsx => TYPESCRIPT,
        Language::Zig => ZIG,
        Language::Bash => BASH,
        Language::Json => JSON,
        Language::Yaml => YAML,
        Language::Markdown => MARKDOWN,
        Language::Html => HTML,
        Language::Xml => XML,
        Language::Ruby => RUBY,
        Language::Crystal => CRYSTAL,
        Language::Php => PHP,
        Language::C => C,
        Language::Cpp => CPP,
        Language::Java => JAVA,
        Language::Kotlin => KOTLIN,
        Language::Swift => SWIFT,
        Language::CSharp => CSHARP,
        Language::Scala => SCALA,
        Language::Dart => DART,
        Language::Dockerfile => DOCKERFILE,
        Language::Lua => LUA,
        Language::Elixir => ELIXIR,
        Language::Haskell => HASKELL,
        Language::OCaml => OCAML,
        Language::Nix => NIX,
        Language::Proto => PROTO,
        Language::GraphQL => GRAPHQL,
        Language::Ini => INI,
        Language::Toml => TOML,
        Language::Hcl => HCL,
        Language::Make => MAKE,
        Language::CMake => CMAKE,
        Language::Css => CSS,
        Language::Scss => SCSS,
        Language::Sql => SQL,
    }
}

pub(crate) fn block_bodies(language: Language) -> bool {
    matches!(
        language,
        Language::Python
            | Language::Ruby
            | Language::Crystal
            | Language::Lua
            | Language::Elixir
            | Language::Make
            | Language::CMake
            | Language::Hcl
    )
}

const BODY_KINDS: &[&str] = &[
    "class_body",
    "enum_class_body",
    "enum_body",
    "interface_body",
    "annotation_type_body",
    "constructor_body",
    "declaration_list",
    "enum_declaration_list",
    "field_declaration_list",
    "enumerator_list",
    "template_body",
    "protocol_body",
    "extension_body",
    "body_statement",
    "do_block",
    "expressions",
    "message_body",
    "service_body",
    "fields_definition",
    "enum_values_definition",
    "input_fields_definition",
    "selection_set",
    "keyframe_block_list",
    "recipe",
    "block",
    "compound_statement",
    "function_body",
    "body",
    "statements",
];

fn named_body(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| BODY_KINDS.contains(&child.kind()))
}

fn inner_range(node: Node<'_>, source: &[u8]) -> Option<[usize; 2]> {
    if node.kind() == "do_block" {
        let first = node.named_child(0)?;
        let mut cursor = node.walk();
        let last = node.named_children(&mut cursor).last()?;
        return Some([first.start_byte(), last.end_byte()]);
    }
    if node.kind() == "function_body" {
        let mut cursor = node.walk();
        if let Some(block) = node
            .named_children(&mut cursor)
            .find(|child| child.kind() == "block")
        {
            return Some([block.start_byte(), block.end_byte()]);
        }
        if !text(node, source).starts_with('{') && node.named_child_count() == 1 {
            let child = node.named_child(0)?;
            return Some([child.start_byte(), child.end_byte()]);
        }
    }
    Some([node.start_byte(), node.end_byte()])
}

fn trimmed(range: [usize; 2], source: &[u8]) -> Option<[usize; 2]> {
    let content = source.get(range[0]..range[1])?;
    let leading = content
        .iter()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count();
    let trailing = content
        .iter()
        .rev()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count();
    if leading + trailing >= content.len() {
        return Some([range[0], range[0]]);
    }
    Some([range[0] + leading, range[1] - trailing])
}

fn key_value(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(value) = node
        .child_by_field_name("value")
        .or_else(|| node.child_by_field_name("expression"))
    {
        return Some(value);
    }
    if node.named_child_count() < 2 {
        return None;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).last()
}

fn definition_body(
    language: Language,
    kind: &str,
    node: Node<'_>,
    source: &[u8],
) -> Option<[usize; 2]> {
    let found = match kind {
        "constant" | "variable" | "section" | "element" | "field" | "property" | "table"
        | "column" | "index" | "sequence" | "signature" => return None,
        "key" => key_value(node)?,
        _ if language == Language::Haskell => node
            .child_by_field_name("match")
            .and_then(|found| found.child_by_field_name("expression"))?,
        _ => node
            .child_by_field_name("body")
            .or_else(|| named_body(node))
            .or_else(|| body(node, 3))?,
    };
    trimmed(inner_range(found, source)?, source)
}

fn query(language: Language) -> Result<&'static Query, Error> {
    static QUERIES: [OnceLock<Result<Query, String>>; Language::ALL.len()] =
        [const { OnceLock::new() }; Language::ALL.len()];
    let unsupported = || Error::Unsupported(format!("symbols are not available for {language}"));
    let source = definitions(language);
    let index = Language::ALL
        .iter()
        .position(|each| *each == language)
        .ok_or_else(unsupported)?;
    let slot = QUERIES.get(index).ok_or_else(unsupported)?;
    let compiled = slot
        .get_or_init(|| Query::new(&language.grammar(), source).map_err(|error| error.to_string()));
    compiled
        .as_ref()
        .map_err(|message| Error::Pattern(message.clone()))
}

fn wrapper(language: Language, node: Node<'_>) -> bool {
    let single = node.named_child_count() == 1;
    match language {
        Language::Python => node.kind() == "decorated_definition",
        Language::JavaScript | Language::TypeScript | Language::Tsx => match node.kind() {
            "export_statement" | "ambient_declaration" => true,
            "lexical_declaration" | "variable_declaration" => single,
            _ => false,
        },
        Language::Go => {
            matches!(
                node.kind(),
                "type_declaration" | "const_declaration" | "var_declaration"
            ) && single
        }
        Language::Cpp => node.kind() == "template_declaration",
        Language::Dart => node.kind() == "member_declaration",
        Language::Sql => node.kind() == "statement",
        _ => false,
    }
}

fn own_line(node: Node<'_>, source: &[u8]) -> bool {
    let head = source.get(..node.start_byte()).unwrap_or_default();
    let line = head
        .rsplit(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    line.iter().all(u8::is_ascii_whitespace)
}

fn doc_comment(language: Language, node: Node<'_>, source: &[u8]) -> bool {
    if !own_line(node, source) {
        return false;
    }
    let content = text(node, source);
    let javadoc = content.starts_with("/**") && !content.starts_with("/***");
    let triple =
        (content.starts_with("///") && !content.starts_with("////")) || content.starts_with("//!");
    match (language, node.kind()) {
        (Language::Rust, "line_comment" | "block_comment") => triple || javadoc,
        (Language::Zig, "comment") => content.starts_with("///"),
        (Language::Go | Language::Ruby | Language::Crystal, "comment") => true,
        (
            Language::JavaScript
            | Language::TypeScript
            | Language::Tsx
            | Language::Php
            | Language::Kotlin
            | Language::Scala
            | Language::Java,
            "comment" | "block_comment",
        ) => javadoc,
        (
            Language::C | Language::Cpp | Language::Swift | Language::CSharp,
            "comment" | "multiline_comment",
        ) => javadoc || triple,
        (Language::Dart, "documentation_comment") => true,
        (Language::Haskell, "haddock") => true,
        (Language::OCaml, "comment") => content.starts_with("(**") && !content.starts_with("(***"),
        (Language::Lua, "comment") => content.starts_with("---"),
        _ => false,
    }
}

fn attribute(language: Language, node: Node<'_>, source: &[u8]) -> bool {
    match language {
        Language::Rust => node.kind() == "attribute_item",
        Language::JavaScript | Language::TypeScript | Language::Tsx => node.kind() == "decorator",
        Language::Elixir => node.kind() == "unary_operator" && text(node, source).starts_with('@'),
        _ => false,
    }
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
        let is_attribute = attribute(language, sibling, source);
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
    if matches!(
        node.kind(),
        "struct_declaration" | "enum_declaration" | "union_declaration" | "opaque_declaration"
    ) {
        return Some(node);
    }
    if depth == 0 {
        return None;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    children
        .into_iter()
        .find_map(|child| body(child, depth - 1))
}

fn clean_name(kind: &str, raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches('*').trim_matches('"');
    if !matches!(kind, "impl" | "method") {
        return trimmed.to_owned();
    }
    let generic = trimmed.find(['<', '[']).filter(|index| *index > 0);
    generic
        .and_then(|index| trimmed.get(..index))
        .unwrap_or(trimmed)
        .trim()
        .to_owned()
}

fn head_text(node: Node<'_>, source: &[u8]) -> String {
    let content = text(node, source);
    let head = content.split(['{', '\n']).next().unwrap_or_default();
    head.trim().to_owned()
}

struct Declared {
    receiver: Option<String>,
    name: String,
}

fn declared(node: Node<'_>, source: &[u8]) -> Declared {
    let mut current = node;
    loop {
        let inner = match current.kind() {
            "parenthesized_declarator" => current.named_child(0),
            _ => current.child_by_field_name("declarator"),
        };
        let Some(inner) = inner else {
            break;
        };
        current = inner;
    }
    if current.kind() != "qualified_identifier" {
        return Declared {
            receiver: None,
            name: text(current, source).into_owned(),
        };
    }
    let scope = current
        .child_by_field_name("scope")
        .map(|scope| clean_name("method", &text(scope, source)).replace("::", "."));
    let name = current.child_by_field_name("name").map_or_else(
        || text(current, source).into_owned(),
        |name| declared(name, source).name,
    );
    Declared {
        receiver: scope,
        name,
    }
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

fn title(
    language: Language,
    kind: &str,
    name: &str,
    node: Node<'_>,
    source: &[u8],
) -> Option<String> {
    match (language, kind) {
        (_, "impl") => {
            let implemented = node
                .child_by_field_name("trait")
                .map(|node| clean_name("impl", &text(node, source)));
            Some(implemented.map_or_else(
                || format!("impl {name}"),
                |implemented| format!("impl {implemented} for {name}"),
            ))
        }
        (Language::Swift, "extension") => Some(format!("extension {name}")),
        (Language::Scala, "object") => Some(format!("object {name}")),
        _ => None,
    }
}

pub fn outline_source(source: &[u8], language: Language) -> Result<Vec<Symbol>, Error> {
    let tree = crate::parse::parse(source, language)?;
    outline(&tree, source, language)
}

pub fn outline(tree: &Tree, source: &[u8], language: Language) -> Result<Vec<Symbol>, Error> {
    let query = query(language)?;
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut found: Vec<Definition> = Vec::new();
    let mut seen: HashSet<([usize; 2], String)> = HashSet::new();
    let mut iterator = cursor.matches(query, tree.root_node(), source);
    while let Some(each) = iterator.next() {
        let mut definition = None;
        let mut name = None;
        let mut receiver = None;
        let mut labels = Vec::new();
        for capture in each.captures() {
            let label = names
                .get(capture.index as usize)
                .copied()
                .unwrap_or_default();
            match label {
                "name" => name = Some(text(capture.node, source).into_owned()),
                "receiver" => receiver = Some(clean_name("method", &text(capture.node, source))),
                "declarator" => {
                    let Declared {
                        receiver: scope,
                        name: declared,
                    } = declared(capture.node, source);
                    receiver = scope;
                    name = Some(declared);
                }
                "label" | "patterns" => labels.push(clean_name("", &text(capture.node, source))),
                _ => {
                    if let Some(kind) = label.strip_prefix("definition.") {
                        definition = Some((kind, capture.node));
                    }
                }
            }
        }
        let Some((kind, node)) = definition else {
            continue;
        };
        let name = clean_name(kind, &name.unwrap_or_else(|| head_text(node, source)));
        let Extent {
            first,
            signature,
            last,
        } = extent(language, node, source);
        let bytes = [first.start_byte(), last.end_byte()];
        let (name, title) = match kind {
            "instance" => (
                labels.join(" "),
                Some(format!("instance {name} {}", labels.join(" "))),
            ),
            _ => {
                let name = std::iter::once(name)
                    .chain(labels)
                    .collect::<Vec<_>>()
                    .join(".");
                let title = title(language, kind, &name, node, source);
                (name, title)
            }
        };
        if !seen.insert((bytes, name.clone())) {
            continue;
        }
        let start = first.start_position();
        let end = last.end_position();
        found.push(Definition {
            signature_start: signature.start_byte(),
            title,
            name,
            kind: kind.to_owned(),
            receiver,
            bytes,
            body: definition_body(language, kind, node, source),
            start: [start.row, start.column],
            end: [end.row, end.column],
        });
    }
    found.sort_by_key(|definition| (definition.bytes[0], std::cmp::Reverse(definition.bytes[1])));
    if language == Language::Haskell {
        found = merged_equations(found);
    }
    Ok(nest(found, source))
}

fn merged_equations(found: Vec<Definition>) -> Vec<Definition> {
    let mut merged: Vec<Definition> = Vec::with_capacity(found.len());
    for definition in found {
        let equation = matches!(definition.kind.as_str(), "signature" | "function");
        let previous = merged.last_mut().filter(|previous| {
            equation
                && previous.name == definition.name
                && matches!(previous.kind.as_str(), "signature" | "function")
        });
        let Some(previous) = previous else {
            merged.push(definition);
            continue;
        };
        previous.bytes[1] = definition.bytes[1];
        previous.end = definition.end;
        previous.kind = "function".to_owned();
        previous.body = None;
    }
    for definition in &mut merged {
        if definition.kind == "signature" {
            definition.kind = "function".to_owned();
        }
    }
    merged
}

const OWNERS: &[&str] = &[
    "class",
    "impl",
    "trait",
    "struct",
    "extension",
    "interface",
    "object",
    "protocol",
    "enum",
    "record",
    "mixin",
    "annotation",
];

fn nest(found: Vec<Definition>, source: &[u8]) -> Vec<Symbol> {
    let mut symbols: Vec<Symbol> = Vec::with_capacity(found.len());
    let mut scopes: Vec<String> = Vec::with_capacity(found.len());
    let mut stack: Vec<usize> = Vec::new();
    for definition in found {
        while stack
            .last()
            .and_then(|index| symbols.get(*index))
            .is_some_and(|parent| {
                parent.bytes[1] <= definition.bytes[0] || parent.bytes == definition.bytes
            })
        {
            stack.pop();
        }
        let parent = stack.last().and_then(|index| symbols.get(*index));
        if parent.is_some_and(|parent| parent.kind == "type" && parent.name == definition.name) {
            continue;
        }
        let mut kind = definition.kind;
        let owned = definition.receiver.is_some()
            || parent.is_some_and(|parent| OWNERS.contains(&parent.kind.as_str()));
        if kind == "function" && owned {
            kind = "method".to_owned();
        }
        let scope = stack.last().and_then(|index| scopes.get(*index)).cloned();
        let prefix = definition.receiver.or(scope);
        let scoped = prefix.map_or_else(
            || definition.name.clone(),
            |prefix| format!("{prefix}.{}", definition.name),
        );
        scopes.push(scoped.clone());
        let path = definition.title.unwrap_or(scoped);
        let signature_end = definition.body.map_or(definition.bytes[1], |body| body[0]);
        let signature = String::from_utf8_lossy(
            source
                .get(definition.signature_start..signature_end)
                .unwrap_or_default(),
        );
        let signature = signature
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
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
    let preferred: Vec<&Symbol> = candidates.iter().copied().filter(|symbol| !matches!(symbol.kind.as_str(), "impl" | "extension" | "instance")).collect();
    if preferred.is_empty() {
        return candidates;
    }
    preferred
}

fn at_line(wanted: &str) -> Option<(&str, usize)> {
    let (path, line) = wanted.rsplit_once(':')?;
    Some((path, line.parse().ok()?))
}

fn located<'symbols>(symbols: &'symbols [Symbol], wanted: &str) -> Vec<&'symbols Symbol> {
    let chosen = without_impls(candidates(symbols, wanted));
    if !chosen.is_empty() {
        return chosen;
    }
    let Some((path, line)) = at_line(wanted) else {
        return chosen;
    };
    candidates(symbols, path).into_iter().filter(|symbol| symbol.start[0] + 1 == line).collect()
}

pub fn lookup<'symbols>(symbols: &'symbols [Symbol], wanted: &str) -> Result<&'symbols Symbol, Error> {
    let chosen = located(symbols, wanted);
    match chosen[..] {
        [only] => Ok(only),
        [] => Err(Error::NoSymbol {
            name: wanted.to_owned(),
            available: symbols.iter().map(|symbol| symbol.path.clone()).collect(),
        }),
        _ => Err(Error::Ambiguous {
            name: wanted.to_owned(),
            candidates: chosen.iter().map(|symbol| format!("{}:{} ({})", symbol.path, symbol.start[0] + 1, symbol.kind)).collect(),
        }),
    }
}
