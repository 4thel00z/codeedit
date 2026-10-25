use std::collections::BTreeMap;

use tree_sitter::Query;

use crate::edit::Edit;
use crate::error::Error;
use crate::language::Language;
use crate::pattern::trailing_indent;
use crate::query::{Capture, Match};
use crate::tools::{dedent, indent, indent_at};

pub fn validate(query: &Query, replacements: &BTreeMap<String, String>) -> Result<(), Error> {
    let names = query.capture_names();
    let unknown = replacements
        .keys()
        .map(|key| key.trim_start_matches('@'))
        .find(|key| !names.contains(key));
    let Some(name) = unknown else {
        return Ok(());
    };
    Err(Error::UnknownCapture(name.to_owned()))
}

fn capture_name(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
}

fn resolve<'found>(found: &'found Match, current: &'found Capture, name: &str) -> Option<&'found Capture> {
    if current.name == name {
        return Some(current);
    }
    found.capture(name)
}

pub struct Context<'source> {
    pub source: &'source str,
    pub language: Language,
}

pub fn expand(template: &str, found: &Match, current: &Capture, context: &Context<'_>) -> String {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some((head, after)) = rest.split_once('@') {
        output.push_str(head);
        let run = after.find(|character| !capture_name(character)).unwrap_or(after.len());
        let longest = (1..=run).rev().find_map(|length| {
            let (name, tail) = after.split_at_checked(length)?;
            Some((resolve(found, current, name)?, tail))
        });
        let Some((capture, tail)) = longest else {
            output.push('@');
            rest = after;
            continue;
        };
        let moved = dedent(&capture.text, &indent_at(context.source, capture.bytes[0]), context.language);
        output.push_str(&indent(&moved, &trailing_indent(&output), context.language));
        rest = tail;
    }
    output.push_str(rest);
    indent(&output, &indent_at(context.source, current.bytes[0]), context.language)
}

pub fn edits(found: &[Match], replacements: &BTreeMap<String, String>, source: &str, language: Language) -> Vec<Edit> {
    let context = Context { source, language };
    found
        .iter()
        .flat_map(|each| {
            let context = &context;
            replacements.iter().flat_map(move |(name, template)| {
                let name = name.trim_start_matches('@');
                each.captures
                    .iter()
                    .filter(move |capture| capture.name == name)
                    .map(move |capture| Edit {
                        bytes: capture.bytes,
                        expect: Some(capture.text.clone()),
                        replace: expand(template, each, capture, context),
                    })
            })
        })
        .collect()
}
