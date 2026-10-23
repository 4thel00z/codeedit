use std::collections::BTreeMap;

use tree_sitter::Query;

use crate::edit::Edit;
use crate::error::Error;
use crate::query::Match;

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

pub fn expand(template: &str, found: &Match) -> String {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some((head, after)) = rest.split_once('@') {
        output.push_str(head);
        let length = after.find(|character| !capture_name(character)).unwrap_or(after.len());
        let (name, tail) = after.split_at_checked(length).unwrap_or((after, ""));
        let capture = (!name.is_empty()).then(|| found.capture(name)).flatten();
        let Some(capture) = capture else {
            output.push('@');
            rest = after;
            continue;
        };
        output.push_str(&capture.text);
        rest = tail;
    }
    output.push_str(rest);
    output
}

pub fn edits(found: &[Match], replacements: &BTreeMap<String, String>) -> Vec<Edit> {
    found
        .iter()
        .flat_map(|each| {
            replacements.iter().flat_map(move |(name, template)| {
                let name = name.trim_start_matches('@');
                each.captures
                    .iter()
                    .filter(move |capture| capture.name == name)
                    .map(move |capture| Edit {
                        bytes: capture.bytes,
                        expect: Some(capture.text.clone()),
                        replace: expand(template, each),
                    })
            })
        })
        .collect()
}
