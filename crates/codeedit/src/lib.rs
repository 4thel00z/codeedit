pub mod edit;
pub mod error;
pub mod files;
pub mod format;
pub mod language;
pub mod parse;
pub mod pattern;
pub mod query;
pub mod rewrite;
pub mod symbols;
pub mod tools;
pub mod walk;

pub use edit::{Change, Edit, apply};
pub use error::Error;
pub use files::{FileChange, FileEdits, FileMatches};
pub use format::{Format, render, write};
pub use language::Language;
pub use parse::parse;
pub use pattern::{Found, Pattern, Rule};
pub use query::{Capture, Match};
pub use symbols::Symbol;
pub use tools::{Operation, Position, Request};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_loads() {
        for language in Language::ALL {
            let tree = parse(b"", language);
            assert!(tree.is_ok(), "{language}: {:?}", tree.err());
        }
    }

    #[test]
    fn json_is_valid_in_every_language() {
        let samples: [(Language, &str); Language::ALL.len()] = [
            (Language::Bash, "echo \"hi $USER\" | grep h\n"),
            (Language::C, "int main(void) { return 0; }\n"),
            (Language::CMake, "add_executable(app main.c)\n"),
            (Language::CSharp, "class A { int F() { return 1; } }\n"),
            (Language::Cpp, "namespace a { int f() { return 0; } }\n"),
            (Language::Crystal, "def f(x : Int32) : Int32\n  x\nend\n"),
            (Language::Css, ".a { color: red; }\n"),
            (Language::Dart, "void main() { print('x'); }\n"),
            (Language::Dockerfile, "FROM alpine AS base\nRUN echo hi\n"),
            (Language::Elixir, "defmodule A do\n  def f, do: 1\nend\n"),
            (Language::Go, "package main\nfunc main() { println(1) }\n"),
            (Language::GraphQL, "type Query { a: Int }\n"),
            (Language::Haskell, "main :: IO ()\nmain = pure ()\n"),
            (Language::Hcl, "resource \"a\" \"b\" {\n  x = 1\n}\n"),
            (Language::Html, "<div class=\"a\"><p>hi</p></div>\n"),
            (Language::Ini, "[a]\nb = 1\n"),
            (Language::Java, "class A { int f() { return 1; } }\n"),
            (Language::JavaScript, "const App = () => <div>{x}</div>;\n"),
            (Language::Json, "{\"a\": [1, true, null]}\n"),
            (Language::Kotlin, "fun f(x: Int): Int = x\n"),
            (Language::Lua, "local function f(x) return x end\n"),
            (Language::Make, "all: main.c\n\tcc main.c\n"),
            (Language::Markdown, "# Title\n\n- item\n\n```py\nx = 1\n```\n"),
            (Language::Nix, "{ a = 1; b = x: x; }\n"),
            (Language::OCaml, "let f x = x + 1\n"),
            (Language::Php, "<?php\nfunction f(int $x): int { return $x; }\n"),
            (Language::Proto, "syntax = \"proto3\";\nmessage A { int32 x = 1; }\n"),
            (Language::Python, "class A:\n    def f(self) -> int:\n        return 1\n"),
            (Language::Ruby, "class A\n  def f(x)\n    x\n  end\nend\n"),
            (Language::Rust, "fn main() { let x: u8 = 1; }\n"),
            (Language::Scala, "object A { def f(x: Int): Int = x }\n"),
            (Language::Scss, "$a: 1;\n.b { c: $a; }\n"),
            (Language::Sql, "SELECT a FROM b WHERE c = 1;\n"),
            (Language::Swift, "func f(_ x: Int) -> Int { return x }\n"),
            (Language::Toml, "[a]\nb = 1\n"),
            (Language::Tsx, "const A = (p: P) => <b>{p.x}</b>;\n"),
            (Language::TypeScript, "interface A { x: number }\n"),
            (Language::Xml, "<?xml version=\"1.0\"?><a b=\"c\">d</a>\n"),
            (Language::Yaml, "a:\n  - b: 1\n"),
            (Language::Zig, "pub fn main() void {}\n"),
        ];
        for (language, source) in samples {
            let tree = parse(source.as_bytes(), language).unwrap();
            assert!(!tree.root_node().has_error(), "{language} parsed with errors: {}", tree.root_node().to_sexp());
            let json = render(&tree, source.as_bytes(), Format::Json).unwrap();
            let value: serde_json::Value = serde_json::from_str(&json).unwrap_or_else(|error| panic!("{language}: {error}\n{json}"));
            assert_eq!(value["bytes"][1], tree.root_node().end_byte());
        }
    }

    #[test]
    fn json_shape() {
        let source = b"class Greeter:\n    def greet(self, name: str) -> str:\n        return name\n";
        let tree = parse(source, Language::Python).unwrap();
        let value: serde_json::Value = serde_json::from_str(&render(&tree, source, Format::Json).unwrap()).unwrap();
        let class = &value["children"][0];
        assert_eq!(class["type"], "class_definition");
        assert_eq!(class["children"][0]["field"], "name");
        assert_eq!(class["children"][0]["text"], "Greeter");
        assert_eq!(class["children"][1]["children"][0]["children"][0]["text"], "greet");
    }

    #[test]
    fn errors_are_flagged() {
        let missing = b"def f(:\n";
        let tree = parse(missing, Language::Python).unwrap();
        let json = render(&tree, missing, Format::Json).unwrap();
        assert!(json.contains("\"type\":\")\"") && json.contains("\"missing\":true"), "{json}");

        let broken = b"fn main() { let = ; }\n";
        let tree = parse(broken, Language::Rust).unwrap();
        let json = render(&tree, broken, Format::Json).unwrap();
        assert!(json.contains("\"error\":true"), "{json}");
    }

    #[test]
    fn other_formats_render() {
        let source = b"x = \"<a&b>\"\n";
        let tree = parse(source, Language::Python).unwrap();
        assert!(render(&tree, source, Format::Xml).unwrap().contains("&lt;a&amp;b&gt;"));
        assert!(render(&tree, source, Format::Dot).unwrap().starts_with("digraph"));
        assert!(render(&tree, source, Format::Sexp).unwrap().starts_with("(module"));
    }
}
