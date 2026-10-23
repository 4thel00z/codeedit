use tree_sitter::{Parser, Tree};

use crate::error::Error;
use crate::language::Language;

pub fn parse(source: &[u8], language: Language) -> Result<Tree, Error> {
    let mut parser = Parser::new();
    parser.set_language(&language.grammar())?;
    parser.parse(source, None).ok_or(Error::Parse)
}
