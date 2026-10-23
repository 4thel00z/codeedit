use std::collections::BTreeMap;
use std::fs;

use treesitter::pattern::{Constraint, edits, plan};
use treesitter::{Error, Found, Language, Pattern, Rule, apply, parse};

fn rule(pattern: &str, rewrite: Option<&str>) -> Rule {
    Rule {
        pattern: pattern.to_owned(),
        rewrite: rewrite.map(str::to_owned),
        ..Rule::default()
    }
}

fn try_find(language: Language, rule: &Rule, source: &str) -> Result<Vec<Found>, Error> {
    let pattern = Pattern::new(rule, language)?;
    let tree = parse(source.as_bytes(), language)?;
    Ok(pattern.find(&tree, source.as_bytes()))
}

fn try_rewrite(language: Language, rule: &Rule, source: &str) -> Result<String, Error> {
    let found = try_find(language, rule, source)?;
    let edits = edits(&plan(&found, rule.rewrite.as_deref().ok_or(Error::MissingRewrite)?), source);
    Ok(apply(source, language, &edits, "x", false)?.source)
}

#[test]
fn single_and_multiple_metavariables() {
    let source = "foo(1)\nfoo(a, b)\nfoo()\nbar(1)\n";
    let found = try_find(Language::Python, &rule("foo($$$ARGS)", None), source).unwrap();
    let texts: Vec<&str> = found.iter().map(|each| each.bindings["ARGS"].text.as_str()).collect();
    assert_eq!(texts, ["1", "a, b", ""]);
    let single = try_find(Language::Python, &rule("foo($A)", None), source).unwrap();
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].bindings["A"].text, "1");
}

#[test]
fn operators_and_repeated_names_must_agree() {
    let source = "x = a + a\ny = a + b\nz = a - a\n";
    let found = try_find(Language::Python, &rule("$X + $X", None), source).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, "a + a");
}

#[test]
fn rewrites_nested_matches() {
    let source = "foo(foo(x), 2)\n";
    let result = try_rewrite(Language::Python, &rule("foo($$$A)", Some("bar($$$A)")), source).unwrap();
    assert_eq!(result, "bar(bar(x), 2)\n");
}

#[test]
fn rewrites_in_several_languages() {
    let rust = "fn main() { let v = x.unwrap(); }\n";
    assert_eq!(
        try_rewrite(Language::Rust, &rule("$E.unwrap()", Some("$E?")), rust).unwrap(),
        "fn main() { let v = x?; }\n"
    );
    let go = "package main\nfunc main() { fmt.Println(a, b) }\n";
    assert_eq!(
        try_rewrite(Language::Go, &rule("fmt.Println($$$A)", Some("log.Println($$$A)")), go).unwrap(),
        "package main\nfunc main() { log.Println(a, b) }\n"
    );
    let typescript = "const x = await fetch(url, { method: 'GET' });\n";
    assert_eq!(
        try_rewrite(Language::TypeScript, &rule("fetch($URL, $$$REST)", Some("http.get($URL)")), typescript).unwrap(),
        "const x = await http.get(url);\n"
    );
    let javascript = "console.log('a'); console.warn('b');\n";
    assert_eq!(
        try_rewrite(Language::JavaScript, &rule("console.log($$$A)", Some("logger.info($$$A)")), javascript).unwrap(),
        "logger.info('a'); console.warn('b');\n"
    );
}

#[test]
fn python_function_bodies() {
    let source = "def f(a, b):\n    x = 1\n    return x\n\ndef g():\n    pass\n";
    let found = try_find(Language::Python, &rule("def $NAME($$$PARAMS):\n    $$$BODY", None), source).unwrap();
    let names: Vec<&str> = found.iter().map(|each| each.bindings["NAME"].text.as_str()).collect();
    assert_eq!(names, ["f", "g"]);
    assert_eq!(found[0].bindings["BODY"].text, "x = 1\n    return x");
}

#[test]
fn constraints_filter_matches() {
    let mut constrained = rule("$F($$$A)", None);
    constrained.constraints = BTreeMap::from([("F".to_owned(), Constraint { regex: Some("^log_".to_owned()), kind: None })]);
    let found = try_find(Language::Python, &constrained, "log_info(1)\nprint(2)\nlog_warn(3)\n").unwrap();
    assert_eq!(found.len(), 2);

    let mut by_kind = rule("print($A)", None);
    by_kind.constraints = BTreeMap::from([("A".to_owned(), Constraint { regex: None, kind: Some("string".to_owned()) })]);
    let found = try_find(Language::Python, &by_kind, "print('a')\nprint(b)\n").unwrap();
    assert_eq!(found.len(), 1);
}

#[test]
fn context_and_selector() {
    let mut selected = rule("x: $T", None);
    selected.context = Some("class A:\n    x: $T\n".to_owned());
    selected.selector = Some("assignment".to_owned());
    let found = try_find(Language::Python, &selected, "class B:\n    x: int\n    y: str\n").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].bindings["T"].text, "int");
}

#[test]
fn invalid_patterns_are_rejected() {
    assert!(matches!(Pattern::new(&rule("def (", None), Language::Python), Err(Error::Pattern(_))));
    assert!(matches!(Pattern::new(&rule("$A", None), Language::Python), Err(Error::Pattern(_))));
}

#[test]
fn rewrite_files_with_several_rules() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("m.py");
    fs::write(&path, "print(a)\nassert x == 1\n").unwrap();
    let rules = [rule("print($$$A)", Some("logger.info($$$A)")), rule("assert $A == $B", Some("assert_equal($A, $B)"))];
    let changes = treesitter::files::rewrite(std::slice::from_ref(&path), None, &rules, false, false).unwrap();
    assert_eq!(changes[0].change.edits, 2);
    assert_eq!(fs::read_to_string(&path).unwrap(), "logger.info(a)\nassert_equal(x, 1)\n");
    let missing = treesitter::files::rewrite(&[path], None, &[rule("x", None)], true, false);
    assert!(matches!(missing, Err(Error::MissingRewrite)));
}

#[test]
fn nested_matches_from_different_rules() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("m.py");
    fs::write(&path, "print(open(p).read())\n").unwrap();
    let rules = [rule("print($$$A)", Some("logger.info($$$A)")), rule("open($P).read()", Some("Path($P).read_text()"))];
    treesitter::files::rewrite(std::slice::from_ref(&path), None, &rules, false, false).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "logger.info(Path(p).read_text())\n");
}

#[test]
fn mixed_language_directories_skip_languages_the_pattern_does_not_fit() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("a.py"), "print(1)\n").unwrap();
    fs::write(directory.path().join("b.json"), "{\"a\": 1}\n").unwrap();
    fs::write(directory.path().join("c.sh"), "echo hi\n").unwrap();
    fs::write(directory.path().join("d.py"), [0xff, 0xfe, 0x00]).unwrap();
    let paths = [directory.path().to_path_buf()];
    let found = treesitter::files::find(&paths, None, &[rule("print($$$A)", None)]).unwrap();
    assert_eq!(found.len(), 1);
    let queried = treesitter::files::query(&paths, None, "(call) @c").unwrap();
    assert_eq!(queried.len(), 1);
    let nothing = treesitter::files::find(&paths, None, &[rule("def (", None)]);
    assert!(matches!(nothing, Err(Error::Pattern(_))));
    let explicit = treesitter::files::find(&[directory.path().join("d.py")], None, &[rule("print($$$A)", None)]);
    assert!(explicit.is_err());
}
