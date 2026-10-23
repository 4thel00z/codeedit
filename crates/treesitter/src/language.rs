use std::fmt;
use std::path::Path;
use std::str::FromStr;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Language {
    Bash,
    Go,
    Html,
    JavaScript,
    Json,
    Markdown,
    Python,
    Rust,
    Tsx,
    TypeScript,
    Xml,
    Yaml,
    Zig,
}

impl Language {
    pub const ALL: [Self; 13] = [
        Self::Bash,
        Self::Go,
        Self::Html,
        Self::JavaScript,
        Self::Json,
        Self::Markdown,
        Self::Python,
        Self::Rust,
        Self::Tsx,
        Self::TypeScript,
        Self::Xml,
        Self::Yaml,
        Self::Zig,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Go => "go",
            Self::Html => "html",
            Self::JavaScript => "javascript",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Tsx => "tsx",
            Self::TypeScript => "typescript",
            Self::Xml => "xml",
            Self::Yaml => "yaml",
            Self::Zig => "zig",
        }
    }

    pub fn grammar(self) -> tree_sitter::Language {
        let function = match self {
            Self::Bash => tree_sitter_bash::LANGUAGE,
            Self::Go => tree_sitter_go::LANGUAGE,
            Self::Html => tree_sitter_html::LANGUAGE,
            Self::JavaScript => tree_sitter_javascript::LANGUAGE,
            Self::Json => tree_sitter_json::LANGUAGE,
            Self::Markdown => tree_sitter_md::LANGUAGE,
            Self::Python => tree_sitter_python::LANGUAGE,
            Self::Rust => tree_sitter_rust::LANGUAGE,
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX,
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            Self::Xml => tree_sitter_xml::LANGUAGE_XML,
            Self::Yaml => tree_sitter_yaml::LANGUAGE,
            Self::Zig => tree_sitter_zig::LANGUAGE,
        };
        function.into()
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        let language = match extension.to_ascii_lowercase().as_str() {
            "sh" | "bash" | "zsh" => Self::Bash,
            "go" => Self::Go,
            "html" | "htm" => Self::Html,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "json" => Self::Json,
            "md" | "markdown" => Self::Markdown,
            "py" | "pyi" => Self::Python,
            "rs" => Self::Rust,
            "tsx" => Self::Tsx,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "xml" | "svg" | "xsd" | "xsl" => Self::Xml,
            "yaml" | "yml" => Self::Yaml,
            "zig" | "zon" => Self::Zig,
            _ => return None,
        };
        Some(language)
    }

    pub fn from_path(path: &Path) -> Result<Self, Error> {
        path.extension()
            .and_then(|extension| extension.to_str())
            .and_then(Self::from_extension)
            .ok_or_else(|| Error::UndetectedLanguage(path.display().to_string()))
    }
}

impl FromStr for Language {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let language = match name.to_ascii_lowercase().as_str() {
            "bash" | "sh" | "shell" => Self::Bash,
            "go" | "golang" => Self::Go,
            "html" => Self::Html,
            "javascript" | "js" | "jsx" => Self::JavaScript,
            "json" => Self::Json,
            "markdown" | "md" => Self::Markdown,
            "python" | "py" => Self::Python,
            "rust" | "rs" => Self::Rust,
            "tsx" => Self::Tsx,
            "typescript" | "ts" => Self::TypeScript,
            "xml" => Self::Xml,
            "yaml" | "yml" => Self::Yaml,
            "zig" => Self::Zig,
            _ => return Err(Error::UnknownLanguage(name.to_owned())),
        };
        Ok(language)
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}
