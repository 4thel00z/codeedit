use std::collections::BTreeMap;
use std::fs;

use treesitter::files::{apply_edits, query, rewrite_query};
use treesitter::query::{compile, matches};
use treesitter::{Edit, Error, FileEdits, Language, Request, Rule, apply, parse};

const PYTHON: &str = "def foo(x):\n    return foo(x) + bar(foo)\n";

fn edit(start: usize, end: usize, expect: Option<&str>, replace: &str) -> Edit {
    Edit {
        bytes: [start, end],
        expect: expect.map(str::to_owned),
        replace: replace.to_owned(),
    }
}

#[test]
fn apply_replaces_and_diffs() {
    let change = apply(PYTHON, Language::Python, &[edit(4, 7, Some("foo"), "baz")], "m.py", false).unwrap();
    assert_eq!(change.source, "def baz(x):\n    return foo(x) + bar(foo)\n");
    assert!(change.diff.contains("-def foo(x):") && change.diff.contains("+def baz(x):"), "{}", change.diff);
    assert_eq!(change.edits, 1);
}

#[test]
fn diff_labels_drop_a_leading_dot_slash() {
    let change = apply(PYTHON, Language::Python, &[edit(4, 7, None, "baz")], "./m.py", false).unwrap();
    assert!(change.diff.starts_with("--- a/m.py\n+++ b/m.py\n"), "{}", change.diff);
}

#[test]
fn apply_handles_many_edits_in_any_order() {
    let edits = [edit(23, 26, None, "qux"), edit(4, 7, None, "qux"), edit(0, 0, None, "# head\n")];
    let change = apply(PYTHON, Language::Python, &edits, "m.py", false).unwrap();
    assert_eq!(change.source, "# head\ndef qux(x):\n    return qux(x) + bar(foo)\n");
}

#[test]
fn apply_rejects_stale_overlap_range_and_syntax() {
    let stale = apply(PYTHON, Language::Python, &[edit(4, 7, Some("bar"), "baz")], "m.py", false);
    assert!(matches!(stale, Err(Error::Stale { .. })));
    let overlap = apply(PYTHON, Language::Python, &[edit(4, 7, None, "a"), edit(5, 9, None, "b")], "m.py", false);
    assert!(matches!(overlap, Err(Error::Overlap { .. })));
    let range = apply(PYTHON, Language::Python, &[edit(4, 400, None, "a")], "m.py", false);
    assert!(matches!(range, Err(Error::Range { .. })));
    let utf8 = apply("x = 'é'\n", Language::Python, &[edit(6, 7, None, "a")], "m.py", false);
    assert!(matches!(utf8, Err(Error::Range { .. })));
    let syntax = apply(PYTHON, Language::Python, &[edit(7, 8, None, "")], "m.py", false);
    assert!(matches!(syntax, Err(Error::Syntax { before: 0, .. })));
    let allowed = apply(PYTHON, Language::Python, &[edit(7, 8, None, "")], "m.py", true).unwrap();
    assert!(allowed.errors_after > 0);
}

#[test]
fn apply_tolerates_existing_errors() {
    let broken = "def f(:\n    pass\nx = 1\n";
    let change = apply(broken, Language::Python, &[edit(17, 18, Some("x"), "y")], "m.py", false).unwrap();
    assert!(change.errors_before > 0 && change.errors_after == change.errors_before);
}

#[test]
fn query_returns_captures() {
    let query = compile(Language::Python, "(call function: (identifier) @name (#eq? @name \"foo\"))").unwrap();
    let tree = parse(PYTHON.as_bytes(), Language::Python).unwrap();
    let found = matches(&query, &tree, PYTHON.as_bytes());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].captures[0].text, "foo");
    assert_eq!(found[0].captures[0].bytes, [23, 26]);
    assert!(matches!(compile(Language::Python, "(nonsense"), Err(Error::Query(_))));
    assert!(matches!(compile(Language::Python, "(identifier) @i (#eq? @i \"foo\")"), Err(Error::Query(_))));
    assert!(compile(Language::Python, "((identifier) @i (#eq? @i \"foo\"))").is_ok());
}

#[test]
fn repeated_captures_keep_their_own_text() {
    let source = "@cache\n@trace\ndef f():\n    pass\n";
    let query = compile(Language::Python, "(decorated_definition (decorator)+ @dec definition: (function_definition))").unwrap();
    let tree = parse(source.as_bytes(), Language::Python).unwrap();
    let found = matches(&query, &tree, source.as_bytes());
    let replacements = BTreeMap::from([("dec".to_owned(), "@dec  # audited".to_owned())]);
    let change = apply(source, Language::Python, &treesitter::rewrite::edits(&found, &replacements), "m.py", false).unwrap();
    assert_eq!(change.source, "@cache  # audited\n@trace  # audited\ndef f():\n    pass\n");
}

#[test]
fn capture_names_stop_at_the_longest_capture() {
    let source = "x = 1\n";
    let query = compile(Language::Python, "(identifier) @x").unwrap();
    let tree = parse(source.as_bytes(), Language::Python).unwrap();
    let found = matches(&query, &tree, source.as_bytes());
    let replacements = BTreeMap::from([("x".to_owned(), "@x.value".to_owned())]);
    let change = apply(source, Language::Python, &treesitter::rewrite::edits(&found, &replacements), "m.py", false).unwrap();
    assert_eq!(change.source, "x.value = 1\n");
}

#[test]
fn rewrite_query_across_a_directory() {
    let directory = tempfile::tempdir().unwrap();
    let python = directory.path().join("a.py");
    let rust = directory.path().join("b.rs");
    let ignored = directory.path().join("notes.txt");
    fs::write(&python, PYTHON).unwrap();
    fs::write(&rust, "fn main() { foo(1); }\n").unwrap();
    fs::write(&ignored, "foo(1)").unwrap();
    let paths = [directory.path().to_path_buf()];

    let pattern = "(call function: (identifier) @f (#eq? @f \"foo\") arguments: (argument_list) @args)";
    let replacements = BTreeMap::from([("f".to_owned(), "renamed".to_owned()), ("@args".to_owned(), "(@args, 2)".to_owned())]);

    let preview = rewrite_query(&paths, Some(Language::Python), pattern, &replacements, true, false).unwrap();
    assert_eq!(preview.len(), 1);
    assert_eq!(fs::read_to_string(&python).unwrap(), PYTHON);

    rewrite_query(&paths, Some(Language::Python), pattern, &replacements, false, false).unwrap();
    assert_eq!(fs::read_to_string(&python).unwrap(), "def foo(x):\n    return renamed((x), 2) + bar(foo)\n");
    assert_eq!(fs::read_to_string(&rust).unwrap(), "fn main() { foo(1); }\n");

    let unknown = BTreeMap::from([("nope".to_owned(), "x".to_owned())]);
    assert!(matches!(rewrite_query(&paths, Some(Language::Python), pattern, &unknown, true, false), Err(Error::UnknownCapture(_))));

    let found = query(&paths, Some(Language::Rust), "(call_expression function: (identifier) @f)").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].matches[0].captures[0].text, "foo");
}

#[test]
fn apply_edits_is_all_or_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a.py");
    let second = directory.path().join("b.py");
    fs::write(&first, PYTHON).unwrap();
    fs::write(&second, PYTHON).unwrap();
    let requests = [
        FileEdits { file: first.clone(), language: None, edits: vec![edit(4, 7, Some("foo"), "ok")] },
        FileEdits { file: second.clone(), language: None, edits: vec![edit(4, 7, Some("stale"), "no")] },
    ];
    let result = apply_edits(&requests, false, false);
    assert!(matches!(result.as_ref().map_err(Error::root), Err(Error::Stale { .. })));
    assert_eq!(fs::read_to_string(&first).unwrap(), PYTHON);

    let merged = [
        FileEdits { file: first.clone(), language: None, edits: vec![edit(4, 7, None, "one")] },
        FileEdits { file: first.clone(), language: None, edits: vec![edit(23, 26, None, "two")] },
    ];
    apply_edits(&merged, false, false).unwrap();
    assert_eq!(fs::read_to_string(&first).unwrap(), "def one(x):\n    return two(x) + bar(foo)\n");
}

#[test]
fn apply_edits_merges_one_file_named_two_ways() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("m.py");
    fs::write(&path, PYTHON).unwrap();
    let alias = directory.path().join(".").join("m.py");
    let requests = [
        FileEdits { file: path.clone(), language: None, edits: vec![edit(4, 7, None, "bar")] },
        FileEdits { file: alias, language: None, edits: vec![edit(5, 6, None, "x")] },
    ];
    assert!(matches!(apply_edits(&requests, false, false), Err(Error::File { .. } | Error::Overlap { .. })));
    assert_eq!(fs::read_to_string(&path).unwrap(), PYTHON);
}

#[test]
fn unknown_fields_are_rejected() {
    assert!(serde_json::from_str::<Edit>(r#"{"bytes": [4, 7], "expected": "zzz", "replace": "bar"}"#).is_err());
    assert!(serde_json::from_str::<Rule>(r#"{"pattern": "f($A)", "where": {"A": {"regexp": "^x"}}}"#).is_err());
    assert!(serde_json::from_str::<Rule>(r#"{"pattern": "f($A)", "rewrites": "g($A)"}"#).is_err());
    assert!(serde_json::from_str::<FileEdits>(r#"{"file": "a.py", "edit": []}"#).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"op": "replace", "file": "a.py", "symbol": "f", "code": "x", "langauge": "python"}"#).is_err());
    let request: Request = serde_json::from_str(r#"{"op": "delete", "file": "a.py", "language": "python", "symbol": "f"}"#).unwrap();
    assert_eq!(request.language.as_deref(), Some("python"));
}

#[test]
fn identical_inserts_both_apply() {
    let change = apply(PYTHON, Language::Python, &[edit(0, 0, None, "# a\n"), edit(0, 0, None, "# a\n")], "m.py", false).unwrap();
    assert!(change.source.starts_with("# a\n# a\ndef foo"));
}

#[test]
fn new_errors_are_rejected_even_when_old_ones_disappear() {
    let broken = "def f(:\n    pass\n\nx = 1\n";
    let fixes_one_breaks_another = [edit(6, 7, Some(":"), "):"), edit(18, 23, Some("x = 1"), "x = (")];
    assert!(matches!(apply(broken, Language::Python, &fixes_one_breaks_another, "m.py", false), Err(Error::Syntax { .. })));
}

#[test]
fn crlf_sources_keep_crlf() {
    let source = "def f():\r\n    return 1\r\n";
    let change = apply(source, Language::Python, &[edit(22, 22, None, "\n    x = 1")], "m.py", false).unwrap();
    assert_eq!(change.source, "def f():\r\n    return 1\r\n    x = 1\r\n");
}

#[test]
fn writes_are_all_or_nothing_across_files() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a.py");
    let second = directory.path().join("b.py");
    fs::write(&first, "foo(1)\n").unwrap();
    fs::write(&second, "foo(2)\n").unwrap();
    let mut permissions = fs::metadata(&second).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&second, permissions).unwrap();
    let rules = [Rule { pattern: "foo($A)".to_owned(), rewrite: Some("bar($A)".to_owned()), ..Rule::default() }];
    assert!(treesitter::files::rewrite(&[directory.path().to_path_buf()], None, &rules, false, false).is_err());
    assert_eq!(fs::read_to_string(&first).unwrap(), "foo(1)\n");
    let unchanged = [Rule { pattern: "baz($A)".to_owned(), rewrite: Some("qux($A)".to_owned()), ..Rule::default() }];
    assert!(treesitter::files::rewrite(std::slice::from_ref(&second), None, &unchanged, false, false).unwrap().is_empty());
}

#[test]
fn dot_and_xml_output() {
    let tree = parse(b"x = \"a\"\ndef f(:\n", Language::Python).unwrap();
    let dot = treesitter::format::render(&tree, b"x = \"a\"\ndef f(:\n", treesitter::Format::Dot).unwrap();
    assert!(!dot.contains("\\\\\""), "{dot}");
    let xml = treesitter::format::render(&tree, b"x = \"a\"\ndef f(:\n", treesitter::Format::Xml).unwrap();
    assert!(xml.contains("missing=\"true\""), "{xml}");
}
