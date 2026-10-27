use std::fmt;
use std::path::Path;
use std::str::FromStr;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Language {
    Bash,
    C,
    CMake,
    CSharp,
    Cpp,
    Crystal,
    Css,
    Dart,
    Dockerfile,
    Elixir,
    Go,
    GraphQL,
    Haskell,
    Hcl,
    Html,
    Ini,
    Java,
    JavaScript,
    Json,
    Kotlin,
    Lua,
    Make,
    Markdown,
    Nix,
    OCaml,
    Php,
    Proto,
    Python,
    Ruby,
    Rust,
    Scala,
    Scss,
    Sql,
    Swift,
    Toml,
    Tsx,
    TypeScript,
    Xml,
    Yaml,
    Zig,
}

impl Language {
    pub const ALL: [Self; 40] = [
        Self::Bash,
        Self::C,
        Self::CMake,
        Self::CSharp,
        Self::Cpp,
        Self::Crystal,
        Self::Css,
        Self::Dart,
        Self::Dockerfile,
        Self::Elixir,
        Self::Go,
        Self::GraphQL,
        Self::Haskell,
        Self::Hcl,
        Self::Html,
        Self::Ini,
        Self::Java,
        Self::JavaScript,
        Self::Json,
        Self::Kotlin,
        Self::Lua,
        Self::Make,
        Self::Markdown,
        Self::Nix,
        Self::OCaml,
        Self::Php,
        Self::Proto,
        Self::Python,
        Self::Ruby,
        Self::Rust,
        Self::Scala,
        Self::Scss,
        Self::Sql,
        Self::Swift,
        Self::Toml,
        Self::Tsx,
        Self::TypeScript,
        Self::Xml,
        Self::Yaml,
        Self::Zig,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::C => "c",
            Self::CMake => "cmake",
            Self::CSharp => "csharp",
            Self::Cpp => "cpp",
            Self::Crystal => "crystal",
            Self::Css => "css",
            Self::Dart => "dart",
            Self::Dockerfile => "dockerfile",
            Self::Elixir => "elixir",
            Self::Go => "go",
            Self::GraphQL => "graphql",
            Self::Haskell => "haskell",
            Self::Hcl => "hcl",
            Self::Html => "html",
            Self::Ini => "ini",
            Self::Java => "java",
            Self::JavaScript => "javascript",
            Self::Json => "json",
            Self::Kotlin => "kotlin",
            Self::Lua => "lua",
            Self::Make => "make",
            Self::Markdown => "markdown",
            Self::Nix => "nix",
            Self::OCaml => "ocaml",
            Self::Php => "php",
            Self::Proto => "proto",
            Self::Python => "python",
            Self::Ruby => "ruby",
            Self::Rust => "rust",
            Self::Scala => "scala",
            Self::Scss => "scss",
            Self::Sql => "sql",
            Self::Swift => "swift",
            Self::Toml => "toml",
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
            Self::C => tree_sitter_c::LANGUAGE,
            Self::CMake => tree_sitter_cmake::LANGUAGE,
            Self::CSharp => tree_sitter_c_sharp::LANGUAGE,
            Self::Cpp => tree_sitter_cpp::LANGUAGE,
            Self::Crystal => tree_sitter_crystal::LANGUAGE,
            Self::Css => tree_sitter_css::LANGUAGE,
            Self::Dart => tree_sitter_dart::LANGUAGE,
            Self::Dockerfile => tree_sitter_dockerfile::LANGUAGE,
            Self::Elixir => tree_sitter_elixir::LANGUAGE,
            Self::Go => tree_sitter_go::LANGUAGE,
            Self::GraphQL => tree_sitter_graphql::LANGUAGE,
            Self::Haskell => tree_sitter_haskell::LANGUAGE,
            Self::Hcl => tree_sitter_hcl::LANGUAGE,
            Self::Html => tree_sitter_html::LANGUAGE,
            Self::Ini => tree_sitter_ini::LANGUAGE,
            Self::Java => tree_sitter_java::LANGUAGE,
            Self::JavaScript => tree_sitter_javascript::LANGUAGE,
            Self::Json => tree_sitter_json::LANGUAGE,
            Self::Kotlin => tree_sitter_kotlin_ng::LANGUAGE,
            Self::Lua => tree_sitter_lua::LANGUAGE,
            Self::Make => tree_sitter_make::LANGUAGE,
            Self::Markdown => tree_sitter_md::LANGUAGE,
            Self::Nix => tree_sitter_nix::LANGUAGE,
            Self::OCaml => tree_sitter_ocaml::LANGUAGE_OCAML,
            Self::Php => tree_sitter_php::LANGUAGE_PHP,
            Self::Proto => tree_sitter_proto::LANGUAGE,
            Self::Python => tree_sitter_python::LANGUAGE,
            Self::Ruby => tree_sitter_ruby::LANGUAGE,
            Self::Rust => tree_sitter_rust::LANGUAGE,
            Self::Scala => tree_sitter_scala::LANGUAGE,
            Self::Scss => tree_sitter_scss::LANGUAGE,
            Self::Sql => tree_sitter_sequel::LANGUAGE,
            Self::Swift => tree_sitter_swift::LANGUAGE,
            Self::Toml => tree_sitter_toml_ng::LANGUAGE,
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
            "c" | "h" => Self::C,
            "cmake" => Self::CMake,
            "cs" => Self::CSharp,
            "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" => Self::Cpp,
            "cr" => Self::Crystal,
            "css" => Self::Css,
            "dart" => Self::Dart,
            "dockerfile" => Self::Dockerfile,
            "ex" | "exs" => Self::Elixir,
            "go" => Self::Go,
            "graphql" | "gql" => Self::GraphQL,
            "hs" => Self::Haskell,
            "tf" | "tfvars" | "hcl" => Self::Hcl,
            "html" | "htm" => Self::Html,
            "ini" | "cfg" => Self::Ini,
            "java" => Self::Java,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "json" => Self::Json,
            "kt" | "kts" => Self::Kotlin,
            "lua" => Self::Lua,
            "mk" | "make" => Self::Make,
            "md" | "markdown" => Self::Markdown,
            "nix" => Self::Nix,
            "ml" => Self::OCaml,
            "php" => Self::Php,
            "proto" => Self::Proto,
            "py" | "pyi" => Self::Python,
            "rb" | "rake" | "gemspec" => Self::Ruby,
            "rs" => Self::Rust,
            "scala" | "sc" | "sbt" => Self::Scala,
            "scss" => Self::Scss,
            "sql" => Self::Sql,
            "swift" => Self::Swift,
            "toml" => Self::Toml,
            "tsx" => Self::Tsx,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "xml" | "svg" | "xsd" | "xsl" => Self::Xml,
            "yaml" | "yml" => Self::Yaml,
            "zig" | "zon" => Self::Zig,
            _ => return None,
        };
        Some(language)
    }

    pub fn from_file_name(name: &str) -> Option<Self> {
        let language = match name {
            "Makefile" | "makefile" | "GNUmakefile" => Self::Make,
            "CMakeLists.txt" => Self::CMake,
            "Gemfile" | "Rakefile" => Self::Ruby,
            "Dockerfile" | "Containerfile" => Self::Dockerfile,
            _ if name.starts_with("Dockerfile.") || name.starts_with("Containerfile.") => Self::Dockerfile,
            _ => return None,
        };
        Some(language)
    }

    pub fn from_path(path: &Path) -> Result<Self, Error> {
        let by_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(Self::from_file_name);
        let by_extension = || {
            path.extension()
                .and_then(|extension| extension.to_str())
                .and_then(Self::from_extension)
        };
        by_name
            .or_else(by_extension)
            .ok_or_else(|| Error::UndetectedLanguage(path.display().to_string()))
    }
}

impl FromStr for Language {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let language = match name.to_ascii_lowercase().as_str() {
            "bash" | "sh" | "shell" => Self::Bash,
            "c" => Self::C,
            "cmake" => Self::CMake,
            "csharp" | "cs" | "c#" => Self::CSharp,
            "cpp" | "c++" | "cxx" => Self::Cpp,
            "crystal" | "cr" => Self::Crystal,
            "css" => Self::Css,
            "dart" => Self::Dart,
            "dockerfile" | "docker" => Self::Dockerfile,
            "elixir" | "ex" => Self::Elixir,
            "go" | "golang" => Self::Go,
            "graphql" | "gql" => Self::GraphQL,
            "haskell" | "hs" => Self::Haskell,
            "hcl" | "terraform" | "tf" => Self::Hcl,
            "html" => Self::Html,
            "ini" => Self::Ini,
            "java" => Self::Java,
            "javascript" | "js" | "jsx" => Self::JavaScript,
            "json" => Self::Json,
            "kotlin" | "kt" => Self::Kotlin,
            "lua" => Self::Lua,
            "make" | "makefile" => Self::Make,
            "markdown" | "md" => Self::Markdown,
            "nix" => Self::Nix,
            "ocaml" | "ml" => Self::OCaml,
            "php" => Self::Php,
            "proto" | "protobuf" => Self::Proto,
            "python" | "py" => Self::Python,
            "ruby" | "rb" => Self::Ruby,
            "rust" | "rs" => Self::Rust,
            "scala" => Self::Scala,
            "scss" => Self::Scss,
            "sql" => Self::Sql,
            "swift" => Self::Swift,
            "toml" => Self::Toml,
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
