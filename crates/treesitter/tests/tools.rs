use std::fs;

use treesitter::symbols::outline;
use treesitter::tools::{Operation, Position, Request, edit_source, get, operation_edits, rename, rename_edits, run};
use treesitter::{Error, Language, apply, parse};

const PYTHON: &str = "import os\n\n\nclass Greeter:\n    @staticmethod\n    def greet(name):\n        return name\n\n    def wave(self):\n        pass\n\n\ndef main():\n    Greeter.greet(\"x\")\n";

const RUST: &str = "use std::fmt;\n\n#[derive(Debug)]\nstruct Point {\n    x: i32,\n}\n\nimpl Point {\n    fn len(&self) -> i32 {\n        self.x\n    }\n}\n\nfn main() {}\n";

const GO: &str = "package main\n\nimport (\n\t\"os\"\n)\n\ntype List[T any] struct{}\n\nfunc (l *List[T]) Push(v T) {\n\tos.Exit(0)\n}\n\nfunc main() {}\n";

const TYPESCRIPT: &str = "import { a } from \"a\";\n\nexport class Store {\n  get(key: string): string {\n    return key;\n  }\n}\n\nexport const handler = async (event: Event) => {\n  return event;\n};\n\ninterface Options {\n  x: number;\n}\n";

fn paths(language: Language, source: &str) -> Result<Vec<(String, String)>, Error> {
    let tree = parse(source.as_bytes(), language)?;
    let symbols = outline(&tree, source.as_bytes(), language)?;
    Ok(symbols.into_iter().map(|symbol| (symbol.path, symbol.kind)).collect())
}

fn edit(language: Language, source: &str, operation: Operation) -> Result<String, Error> {
    let tree = parse(source.as_bytes(), language)?;
    let symbols = outline(&tree, source.as_bytes(), language)?;
    let edits = operation_edits(&tree, source, language, &symbols, &operation)?;
    Ok(apply(source, language, &edits, "x", false)?.source)
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(path, kind)| ((*path).to_owned(), (*kind).to_owned())).collect()
}

#[test]
fn outlines() {
    assert_eq!(paths(Language::Python, PYTHON).unwrap(), pairs(&[("Greeter", "class"), ("Greeter.greet", "method"), ("Greeter.wave", "method"), ("main", "function")]));
    assert_eq!(paths(Language::Rust, RUST).unwrap(), pairs(&[("Point", "struct"), ("impl Point", "impl"), ("Point.len", "method"), ("main", "function")]));
    assert_eq!(paths(Language::Go, GO).unwrap(), pairs(&[("List", "type"), ("List.Push", "method"), ("main", "function")]));
    assert_eq!(
        paths(Language::TypeScript, TYPESCRIPT).unwrap(),
        pairs(&[("Store", "class"), ("Store.get", "method"), ("handler", "function"), ("Options", "interface")])
    );
    let zig = "const Point = struct {\n    pub fn len(self: Point) i32 { return 0; }\n};\npub fn main() void {}\n";
    assert_eq!(paths(Language::Zig, zig).unwrap(), pairs(&[("Point", "struct"), ("Point.len", "function"), ("main", "function")]));
    assert_eq!(paths(Language::Bash, "build() {\n  make\n}\n").unwrap(), pairs(&[("build", "function")]));
    assert!(matches!(paths(Language::Json, "{}"), Err(Error::Unsupported(_))));
}

#[test]
fn symbol_extent_covers_decorators_and_attributes() {
    let tree = parse(PYTHON.as_bytes(), Language::Python).unwrap();
    let symbols = outline(&tree, PYTHON.as_bytes(), Language::Python).unwrap();
    let greet = symbols.iter().find(|symbol| symbol.path == "Greeter.greet").unwrap();
    assert!(PYTHON[greet.bytes[0]..greet.bytes[1]].starts_with("@staticmethod"));
    assert_eq!(greet.signature, "@staticmethod def greet(name)");
    let tree = parse(RUST.as_bytes(), Language::Rust).unwrap();
    let symbols = outline(&tree, RUST.as_bytes(), Language::Rust).unwrap();
    assert!(RUST[symbols[0].bytes[0]..].starts_with("#[derive(Debug)]"));
}

#[test]
fn replace_and_replace_body() {
    let replaced = edit(Language::Python, PYTHON, Operation::Replace { symbol: "wave".into(), code: "def wave(self, n):\n    return n\n".into() }).unwrap();
    assert!(replaced.contains("    def wave(self, n):\n        return n\n\n\ndef main"), "{replaced}");
    let body = edit(Language::Python, PYTHON, Operation::ReplaceBody { symbol: "Greeter.greet".into(), code: "x = name.upper()\nreturn x".into() }).unwrap();
    assert!(body.contains("    def greet(name):\n        x = name.upper()\n        return x\n"), "{body}");
    let rust = edit(Language::Rust, RUST, Operation::ReplaceBody { symbol: "Point.len".into(), code: "self.x * 2".into() }).unwrap();
    assert!(rust.contains("    fn len(&self) -> i32 {\n        self.x * 2\n    }\n"), "{rust}");
    let empty = edit(Language::Rust, RUST, Operation::ReplaceBody { symbol: "main".into(), code: "println!(\"hi\");".into() }).unwrap();
    assert!(empty.ends_with("fn main() {\n    println!(\"hi\");\n}\n"), "{empty}");
    let go = edit(Language::Go, GO, Operation::ReplaceBody { symbol: "Push".into(), code: "_ = v".into() }).unwrap();
    assert!(go.contains("func (l *List[T]) Push(v T) {\n\t_ = v\n}"), "{go}");
}

#[test]
fn insert_positions() {
    let start = edit(Language::Python, PYTHON, Operation::Insert { symbol: "Greeter".into(), code: "count = 0\n".into(), position: Position::Start }).unwrap();
    assert!(start.contains("class Greeter:\n    count = 0\n    @staticmethod"), "{start}");
    let end = edit(Language::Rust, RUST, Operation::Insert { symbol: "impl Point".into(), code: "fn zero() -> Self {\n    Point { x: 0 }\n}".into(), position: Position::End }).unwrap();
    assert!(end.contains("        self.x\n    }\n\n    fn zero() -> Self {\n        Point { x: 0 }\n    }\n}"), "{end}");
    let after = edit(Language::TypeScript, TYPESCRIPT, Operation::Insert { symbol: "Store.get".into(), code: "set(key: string) {}".into(), position: Position::After }).unwrap();
    assert!(after.contains("    return key;\n  }\n\n  set(key: string) {}\n}"), "{after}");
    let before = edit(Language::Go, GO, Operation::Insert { symbol: "main".into(), code: "// main runs\nfunc helper() {}".into(), position: Position::Before }).unwrap();
    assert!(before.contains("func helper() {}\n\nfunc main() {}"), "{before}");
}

#[test]
fn delete_symbols() {
    let python = edit(Language::Python, PYTHON, Operation::Delete { symbol: "wave".into() }).unwrap();
    assert!(python.contains("        return name\n\n\ndef main"), "{python}");
    assert!(!python.contains("wave"));
    let rust = edit(Language::Rust, RUST, Operation::Delete { symbol: "Point".into() }).unwrap();
    assert!(rust.starts_with("use std::fmt;\n\nimpl Point"), "{rust}");
    let exported = edit(Language::TypeScript, TYPESCRIPT, Operation::Delete { symbol: "handler".into() }).unwrap();
    assert!(!exported.contains("export const handler") && exported.contains("interface Options"), "{exported}");
}

#[test]
fn add_imports() {
    let python = edit(Language::Python, PYTHON, Operation::AddImport { import: "import sys".into() }).unwrap();
    assert!(python.starts_with("import os\nimport sys\n"), "{python}");
    assert_eq!(edit(Language::Python, PYTHON, Operation::AddImport { import: "import  os".into() }).unwrap(), PYTHON);
    let rust = edit(Language::Rust, RUST, Operation::AddImport { import: "use std::io;".into() }).unwrap();
    assert!(rust.starts_with("use std::fmt;\nuse std::io;\n"), "{rust}");
    let go = edit(Language::Go, GO, Operation::AddImport { import: "fmt".into() }).unwrap();
    assert!(go.contains("import (\n\t\"os\"\n\t\"fmt\"\n)"), "{go}");
    let bare = edit(Language::Go, "package main\n\nfunc main() {}\n", Operation::AddImport { import: "\"fmt\"".into() }).unwrap();
    assert_eq!(bare, "package main\n\nimport \"fmt\"\n\nfunc main() {}\n");
    let typescript = edit(Language::TypeScript, TYPESCRIPT, Operation::AddImport { import: "import { b } from \"b\";".into() }).unwrap();
    assert!(typescript.starts_with("import { a } from \"a\";\nimport { b } from \"b\";\n"), "{typescript}");
    let fresh = edit(Language::Python, "\"\"\"Doc.\"\"\"\nx = 1\n", Operation::AddImport { import: "import os".into() }).unwrap();
    assert_eq!(fresh, "\"\"\"Doc.\"\"\"\n\nimport os\n\nx = 1\n");
}

#[test]
fn trait_impls_get_their_own_path() {
    let source = "struct P;\nimpl P { fn a(&self) {} }\nimpl std::fmt::Display for P { fn fmt(&self) {} }\n";
    let found = paths(Language::Rust, source).unwrap();
    assert_eq!(found, pairs(&[("P", "struct"), ("impl P", "impl"), ("P.a", "method"), ("impl std::fmt::Display for P", "impl"), ("P.fmt", "method")]));
}

#[test]
fn lookup_errors() {
    let missing = edit(Language::Python, PYTHON, Operation::Delete { symbol: "nope".into() });
    assert!(matches!(missing, Err(Error::NoSymbol { .. })));
    let ambiguous = "class A:\n    def f(self): pass\nclass B:\n    def f(self): pass\n";
    assert!(matches!(edit(Language::Python, ambiguous, Operation::Delete { symbol: "f".into() }), Err(Error::Ambiguous { .. })));
    assert!(edit(Language::Python, ambiguous, Operation::Delete { symbol: "B.f".into() }).is_ok());
}

#[test]
fn get_text_round_trips_through_replace() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("m.rs");
    fs::write(&path, RUST).unwrap();
    let symbol = get(&path, None, "Point.len").unwrap();
    assert_eq!(symbol.text, "fn len(&self) -> i32 {\n    self.x\n}");
    let operation = Operation::Replace { symbol: "Point.len".to_owned(), code: symbol.text };
    assert_eq!(edit(Language::Rust, RUST, operation).unwrap(), RUST);
}

#[test]
fn batch_and_rename_on_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("m.py");
    fs::write(&path, PYTHON).unwrap();
    let requests = vec![
        Request { file: path.clone(), language: None, operation: Operation::AddImport { import: "import sys".into() } },
        Request { file: path.clone(), language: None, operation: Operation::ReplaceBody { symbol: "main".into(), code: "sys.exit(0)".into() } },
    ];
    let changes = run(&requests, false, false).unwrap();
    assert_eq!(changes[0].change.edits, 2);
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.starts_with("import os\nimport sys\n") && written.ends_with("def main():\n    sys.exit(0)\n"), "{written}");

    let renamed = rename(&[directory.path().to_path_buf()], None, "Greeter", "Welcomer", false, false).unwrap();
    assert_eq!(renamed[0].change.edits, 1);
    fs::write(&path, PYTHON).unwrap();
    rename(&[directory.path().to_path_buf()], None, "Greeter", "Welcomer", false, false).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("class Welcomer:") && written.contains("Welcomer.greet(\"x\")"), "{written}");
}

#[test]
fn doc_comments_travel_with_their_symbol() {
    let rust = "use x;\n\n/// Adds.\n#[inline]\nfn add() {}\n\n/// Main.\nfn main() {}\n";
    let deleted = edit(Language::Rust, rust, Operation::Delete { symbol: "add".into() }).unwrap();
    assert_eq!(deleted, "use x;\n\n/// Main.\nfn main() {}\n");
    let before = edit(Language::Rust, rust, Operation::Insert { symbol: "main".into(), code: "fn other() {}".into(), position: Position::Before }).unwrap();
    assert!(before.contains("fn other() {}\n\n/// Main.\nfn main() {}"), "{before}");
    let go = "package p\n\n// Run runs.\n// Twice.\nfunc Run() {}\n\n// Stop stops.\nfunc Stop() {}\n";
    let deleted = edit(Language::Go, go, Operation::Delete { symbol: "Run".into() }).unwrap();
    assert_eq!(deleted, "package p\n\n// Stop stops.\nfunc Stop() {}\n");
    let typescript = "/** A. */\nexport function a() {}\n\n// plain\n\nfunction b() {}\n";
    let deleted = edit(Language::TypeScript, typescript, Operation::Delete { symbol: "a".into() }).unwrap();
    assert_eq!(deleted, "// plain\n\nfunction b() {}\n");
    let tree = parse(rust.as_bytes(), Language::Rust).unwrap();
    let symbols = outline(&tree, rust.as_bytes(), Language::Rust).unwrap();
    assert_eq!(symbols[0].signature, "#[inline] fn add()");
}

fn batch(language: Language, source: &str, operations: &[Operation]) -> Result<String, Error> {
    let borrowed: Vec<&Operation> = operations.iter().collect();
    Ok(edit_source(source, language, &borrowed, "x", false)?.source)
}

fn body(symbol: &str, code: &str) -> Operation {
    Operation::ReplaceBody { symbol: symbol.to_owned(), code: code.to_owned() }
}

fn insert(symbol: &str, code: &str, position: Position) -> Operation {
    Operation::Insert { symbol: symbol.to_owned(), code: code.to_owned(), position }
}

fn delete(symbol: &str) -> Operation {
    Operation::Delete { symbol: symbol.to_owned() }
}

fn import(text: &str) -> Operation {
    Operation::AddImport { import: text.to_owned() }
}

#[test]
fn same_file_named_two_ways_keeps_both_edits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("m.py");
    fs::write(&path, "def f():\n    return 1\n\n\ndef g():\n    return 2\n").unwrap();
    let alias = directory.path().join(".").join("m.py");
    let requests = [
        Request { file: path.clone(), language: None, operation: body("f", "return 10") },
        Request { file: alias, language: None, operation: body("g", "return 20") },
    ];
    let changes = run(&requests, false, false).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), "def f():\n    return 10\n\n\ndef g():\n    return 20\n");
}

#[test]
fn brace_free_bodies_ignore_braces_in_literals() {
    let dictionary = "def f(d):\n    d[\"k\"] = {}\n    return d\n";
    assert_eq!(
        batch(Language::Python, dictionary, &[insert("f", "print(d)", Position::End)]).unwrap(),
        "def f(d):\n    d[\"k\"] = {}\n    return d\n    print(d)\n"
    );
    let docstring = "def f():\n    \"\"\"Return {x}.\"\"\"\n    return 1\n";
    assert_eq!(batch(Language::Python, docstring, &[body("f", "return 2")]).unwrap(), "def f():\n    return 2\n");
    let arrow = "const f = (xs) => xs.map((y) => { return y; });\n";
    assert_eq!(batch(Language::JavaScript, arrow, &[body("f", "xs")]).unwrap(), "const f = (xs) => xs;\n");
}

#[test]
fn symbols_sharing_a_line_are_edited_alone() {
    let rust = "fn a() {} fn b() {}\nfn main() { b() }\n";
    assert_eq!(batch(Language::Rust, rust, &[delete("a")]).unwrap(), "fn b() {}\nfn main() { b() }\n");
    let inline = "impl P { fn a(&self) {} fn b(&self) {} }\n";
    assert_eq!(
        batch(Language::Rust, inline, &[insert("P.b", "fn c(&self) {}", Position::Before)]).unwrap(),
        "impl P { fn a(&self) {} fn c(&self) {} fn b(&self) {} }\n"
    );
    assert_eq!(
        batch(Language::Rust, inline, &[insert("P.a", "fn c(&self) {}", Position::After)]).unwrap(),
        "impl P { fn a(&self) {} fn c(&self) {} fn b(&self) {} }\n"
    );
}

#[test]
fn python_one_line_bodies_become_blocks() {
    let one_line = "def f(): return 1\n";
    assert_eq!(batch(Language::Python, one_line, &[insert("f", "x = 1", Position::Start)]).unwrap(), "def f():\n    x = 1\n    return 1\n");
    assert_eq!(batch(Language::Python, one_line, &[insert("f", "x = 1", Position::End)]).unwrap(), "def f():\n    return 1\n    x = 1\n");
    assert_eq!(batch(Language::Python, one_line, &[body("f", "x = 1\nreturn x")]).unwrap(), "def f():\n    x = 1\n    return x\n");
    let decorated = "@dec\ndef f(): return 1\n";
    assert_eq!(batch(Language::Python, decorated, &[body("f", "x = 1\nreturn x")]).unwrap(), "@dec\ndef f():\n    x = 1\n    return x\n");
    let documented = "def f():\n    \"\"\"Doc.\"\"\"\n    return 1\n";
    assert_eq!(
        batch(Language::Python, documented, &[insert("f", "x = 1", Position::Start)]).unwrap(),
        "def f():\n    \"\"\"Doc.\"\"\"\n    x = 1\n    return 1\n"
    );
}

#[test]
fn multi_line_strings_keep_their_content() {
    let python = "class Q:\n    def sql(self):\n        return 1\n";
    assert_eq!(
        batch(Language::Python, python, &[body("Q.sql", "return \"\"\"\nSELECT *\nFROM t\n\"\"\"")]).unwrap(),
        "class Q:\n    def sql(self):\n        return \"\"\"\nSELECT *\nFROM t\n\"\"\"\n"
    );
    let bash = "f() {\n  echo hi\n}\n";
    assert_eq!(
        batch(Language::Bash, bash, &[body("f", "cat <<EOF\nhello\nEOF\necho done")]).unwrap(),
        "f() {\n  cat <<EOF\nhello\nEOF\n  echo done\n}\n"
    );
}

#[test]
fn rename_covers_destructuring_and_shell_variables() {
    let javascript = "const { total } = load();\nconsole.log(total + 1);\n";
    let tree = parse(javascript.as_bytes(), Language::JavaScript).unwrap();
    let renamed = apply(javascript, Language::JavaScript, &rename_edits(&tree, javascript, "total", "sum"), "x", false).unwrap();
    assert_eq!(renamed.source, "const { sum } = load();\nconsole.log(sum + 1);\n");
    let bash = "COUNT=1\necho $COUNT\n";
    let tree = parse(bash.as_bytes(), Language::Bash).unwrap();
    let renamed = apply(bash, Language::Bash, &rename_edits(&tree, bash, "COUNT", "N"), "x", false).unwrap();
    assert_eq!(renamed.source, "N=1\necho $N\n");
}

#[test]
fn imports_go_after_file_level_comments() {
    let rust = "//! Small CLI.\nfn main() {\n    let s = String::new();\n}\n";
    assert_eq!(
        batch(Language::Rust, rust, &[import("use std::io;")]).unwrap(),
        "//! Small CLI.\n\nuse std::io;\n\nfn main() {\n    let s = String::new();\n}\n"
    );
    let javascript = "#!/usr/bin/env node\nrun();\n";
    assert_eq!(
        batch(Language::JavaScript, javascript, &[import("import fs from \"fs\";")]).unwrap(),
        "#!/usr/bin/env node\n\nimport fs from \"fs\";\n\nrun();\n"
    );
}

#[test]
fn batches_apply_operations_in_order() {
    let python = "import os\n\n\ndef f():\n    pass\n";
    assert_eq!(
        batch(Language::Python, python, &[import("import sys"), import("import sys")]).unwrap(),
        "import os\nimport sys\n\n\ndef f():\n    pass\n"
    );
    let rust = "fn f() {}\n";
    assert_eq!(
        batch(Language::Rust, rust, &[insert("f", "a();", Position::Start), insert("f", "b();", Position::End)]).unwrap(),
        "fn f() {\n    a();\n    b();\n}\n"
    );
}

#[test]
fn go_imports_keep_their_spelling() {
    let go = "package main\n\nimport \"os\"\n";
    assert_eq!(batch(Language::Go, go, &[import("importer")]).unwrap(), "package main\n\nimport \"os\"\nimport \"importer\"\n");
    let grouped = "package main\n\nimport (\"os\")\n";
    assert_eq!(batch(Language::Go, grouped, &[import("fmt")]).unwrap(), "package main\n\nimport (\n\t\"os\"\n\t\"fmt\"\n)\n");
}

#[test]
fn same_path_symbols_are_addressable_by_line() {
    let python = "class A:\n    @property\n    def x(self):\n        return 1\n\n    @x.setter\n    def x(self, value):\n        pass\n";
    assert!(matches!(batch(Language::Python, python, &[delete("A.x")]), Err(Error::Ambiguous { .. })));
    assert_eq!(batch(Language::Python, python, &[delete("A.x:6")]).unwrap(), "class A:\n    @property\n    def x(self):\n        return 1\n");
}

#[test]
fn trailing_comments_stay_with_their_line() {
    let go = "package m\n\nconst (\n\tA = 1 // a\n\tB = 2 // b\n\tC = 3 // c\n)\n";
    let replaced = batch(Language::Go, go, &[Operation::Replace { symbol: "C".to_owned(), code: "C = 4".to_owned() }]).unwrap();
    assert_eq!(replaced, "package m\n\nconst (\n\tA = 1 // a\n\tB = 2 // b\n\tC = 4 // c\n)\n");
    let after_code = "package m\n\nvar x = 1 // x is one\nfunc b() {}\n";
    let replaced = batch(Language::Go, after_code, &[Operation::Replace { symbol: "b".to_owned(), code: "func b() { return }".to_owned() }]).unwrap();
    assert_eq!(replaced, "package m\n\nvar x = 1 // x is one\nfunc b() { return }\n");
}

#[test]
fn typescript_decorators_and_declare_belong_to_their_symbol() {
    let decorated = "class A {\n  @HostListener(\"click\")\n  onClick() {}\n\n  other() {}\n}\n";
    assert_eq!(batch(Language::TypeScript, decorated, &[delete("A.onClick")]).unwrap(), "class A {\n\n  other() {}\n}\n");
    let ambient = "export declare function legacy(): void;\nexport function next() {}\n";
    assert_eq!(batch(Language::TypeScript, ambient, &[delete("legacy")]).unwrap(), "export function next() {}\n");
}

#[test]
fn multi_name_declarations_keep_their_siblings() {
    let javascript = "const a = () => 1, b = () => 2;\nb();\n";
    assert_eq!(paths(Language::JavaScript, javascript).unwrap(), pairs(&[("a", "function"), ("b", "function")]));
    assert_eq!(batch(Language::JavaScript, javascript, &[delete("a")]).unwrap(), "const b = () => 2;\nb();\n");
    let go = "package m\n\nvar x, y = 1, 2\n";
    assert_eq!(paths(Language::Go, go).unwrap(), pairs(&[("x", "variable"), ("y", "variable")]));
    assert!(batch(Language::Go, go, &[delete("x")]).unwrap() == "package m\n\nvar y = 2\n");
}

#[test]
fn computed_method_names_and_constant_values() {
    let typescript = "class A {\n  [Symbol.iterator]() {}\n}\n";
    assert_eq!(paths(Language::TypeScript, typescript).unwrap(), pairs(&[("A", "class"), ("A.[Symbol.iterator]", "method")]));
    let rust = "const ORIGIN: P = P { x: 1 };\n";
    assert!(matches!(batch(Language::Rust, rust, &[body("ORIGIN", "x: 2")]), Err(Error::Unsupported(_))));
}

#[test]
fn symbol_edits_work_in_files_broken_elsewhere() {
    let broken = "def f(:\n    pass\n\n\ndef g():\n    return 1\n";
    assert_eq!(batch(Language::Python, broken, &[body("g", "return 2")]).unwrap(), "def f(:\n    pass\n\n\ndef g():\n    return 2\n");
}

#[test]
fn one_name_of_a_go_declaration_can_be_deleted() {
    let go = "package m\n\nvar x, y = 1, 2\n";
    assert_eq!(batch(Language::Go, go, &[delete("x")]).unwrap(), "package m\n\nvar y = 2\n");
    assert_eq!(batch(Language::Go, go, &[delete("y")]).unwrap(), "package m\n\nvar x = 1\n");
    let typed = "package m\n\nvar a, b, c int\n";
    assert_eq!(batch(Language::Go, typed, &[delete("b")]).unwrap(), "package m\n\nvar a, c int\n");
    assert!(matches!(batch(Language::Go, go, &[Operation::Replace { symbol: "x".to_owned(), code: "var x = 3".to_owned() }]), Err(Error::Conflict(_))));
}

#[test]
fn python_imports_keep_two_blank_lines_before_definitions() {
    let python = "class A:\n    pass\n";
    assert_eq!(batch(Language::Python, python, &[import("import os")]).unwrap(), "import os\n\n\nclass A:\n    pass\n");
    let script = "x = 1\n";
    assert_eq!(batch(Language::Python, script, &[import("import os")]).unwrap(), "import os\n\nx = 1\n");
}

#[test]
fn trailing_comments_on_the_last_line_go_with_the_symbol() {
    let python = "class A:\n    def f(self):\n        return 1  # cached\n\n    def g(self):\n        pass\n";
    assert_eq!(batch(Language::Python, python, &[delete("A.f")]).unwrap(), "class A:\n\n    def g(self):\n        pass\n");
    assert_eq!(
        batch(Language::Python, python, &[insert("A.f", "def h(self):\n    pass", Position::After)]).unwrap(),
        "class A:\n    def f(self):\n        return 1  # cached\n\n    def h(self):\n        pass\n\n    def g(self):\n        pass\n"
    );
    let rust = "fn a() {}  // a\nfn b() {}\n";
    assert_eq!(batch(Language::Rust, rust, &[delete("a")]).unwrap(), "fn b() {}\n");
    assert_eq!(batch(Language::Rust, rust, &[insert("a", "fn c() {}", Position::After)]).unwrap(), "fn a() {}  // a\n\nfn c() {}\nfn b() {}\n");
}
