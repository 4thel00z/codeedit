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
    fn json_is_valid_for_every_language() {
        let samples: [(Language, &str); 13] = [
            (Language::Bash, "echo \"hi $USER\" | grep h\n"),
            (Language::Go, "package main\nfunc main() { println(1) }\n"),
            (Language::Html, "<div class=\"a\"><p>hi</p></div>\n"),
            (Language::JavaScript, "const App = () => <div>{x}</div>;\n"),
            (Language::Json, "{\"a\": [1, true, null]}\n"),
            (Language::Markdown, "# Title\n\n- item\n\n```py\nx = 1\n```\n"),
            (Language::Python, "class A:\n    def f(self) -> int:\n        return 1\n"),
            (Language::Rust, "fn main() { let x: u8 = 1; }\n"),
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
