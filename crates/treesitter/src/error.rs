#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown language: {0}")]
    UnknownLanguage(String),
    #[error("cannot detect language of {0}, pass a language explicitly")]
    UndetectedLanguage(String),
    #[error("unknown format: {0}")]
    UnknownFormat(String),
    #[error("grammar rejected by tree-sitter: {0}")]
    Grammar(tree_sitter::LanguageError),
    #[error("parser returned no tree")]
    Parse,
    #[error("invalid query: {0}")]
    Query(tree_sitter::QueryError),
    #[error("invalid pattern: {0}")]
    Pattern(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("no symbol {name:?}; available: {}", available.iter().take(30).cloned().collect::<Vec<_>>().join(", "))]
    NoSymbol { name: String, available: Vec<String> },
    #[error("symbol {name:?} is ambiguous: {}", candidates.join(", "))]
    Ambiguous { name: String, candidates: Vec<String> },
    #[error("rule has no rewrite")]
    MissingRewrite,
    #[error("query has no capture named @{0}")]
    UnknownCapture(String),
    #[error("edit range {start}..{end} is outside the source or splits a UTF-8 character (source is {length} bytes)")]
    Range { start: usize, end: usize, length: usize },
    #[error("stale edit at {start}..{end}: expected {expected:?}, found {found:?}")]
    Stale {
        start: usize,
        end: usize,
        expected: String,
        found: String,
    },
    #[error("edits {first:?} and {second:?} overlap")]
    Overlap { first: [usize; 2], second: [usize; 2] },
    #[error("edit introduces syntax errors ({before} before, {after} after); pass allow_errors to apply anyway")]
    Syntax { before: usize, after: usize },
    #[error("{path}: {error}")]
    File { path: String, error: Box<Error> },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<tree_sitter::LanguageError> for Error {
    fn from(error: tree_sitter::LanguageError) -> Self {
        Self::Grammar(error)
    }
}

impl From<tree_sitter::QueryError> for Error {
    fn from(error: tree_sitter::QueryError) -> Self {
        Self::Query(error)
    }
}

impl Error {
    pub fn at(self, path: &std::path::Path) -> Self {
        Self::File {
            path: path.display().to_string(),
            error: Box::new(self),
        }
    }

    pub fn root(&self) -> &Self {
        let Self::File { error, .. } = self else {
            return self;
        };
        error.root()
    }
}
