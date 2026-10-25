use tree_sitter::{Node, Parser, Tree};

use crate::error::Error;
use crate::language::Language;

pub fn parse(source: &[u8], language: Language) -> Result<Tree, Error> {
    let mut parser = Parser::new();
    parser.set_language(&language.grammar())?;
    parser.parse(source, None).ok_or(Error::Parse)
}

pub fn parse_inline(source: &[u8], node: Node<'_>) -> Result<Tree, Error> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_md::INLINE_LANGUAGE.into())?;
    parser.set_included_ranges(&[node.range()]).map_err(|_| Error::Parse)?;
    parser.parse(source, None).ok_or(Error::Parse)
}
