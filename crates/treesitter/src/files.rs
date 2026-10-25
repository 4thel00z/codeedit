use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::edit::{Change, Edit, apply};
use crate::error::Error;
use crate::language::Language;
use crate::parse::parse;
use crate::pattern::{self, Found, Pattern, Rule};
use crate::query::{Match, compile, matches};
use crate::rewrite;

#[derive(Debug, Clone, Serialize)]
pub struct FileMatches {
    pub file: PathBuf,
    pub language: String,
    pub matches: Vec<Match>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub file: PathBuf,
    #[serde(flatten)]
    pub change: Change,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleMatch {
    pub rule: usize,
    #[serde(flatten)]
    pub found: Found,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilePatternMatches {
    pub file: PathBuf,
    pub language: String,
    pub matches: Vec<RuleMatch>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileEdits {
    pub file: PathBuf,
    #[serde(default)]
    pub language: Option<String>,
    pub edits: Vec<Edit>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub path: PathBuf,
    pub language: Language,
    pub explicit: bool,
}

pub fn collect(paths: &[PathBuf], language: Option<Language>) -> Result<Vec<Target>, Error> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_file() {
            let detected = language.map_or_else(|| Language::from_path(path), Ok)?;
            found.push(Target {
                path: path.clone(),
                language: detected,
                explicit: true,
            });
            continue;
        }
        for entry in ignore::WalkBuilder::new(path).build() {
            let entry = entry.map_err(|error| Error::Io(std::io::Error::other(error)))?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let Ok(detected) = Language::from_path(entry.path()) else {
                continue;
            };
            if language.is_some_and(|wanted| wanted != detected) {
                continue;
            }
            found.push(Target {
                path: entry.into_path(),
                language: detected,
                explicit: false,
            });
        }
    }
    found.sort();
    found.dedup_by(|second, first| second.path == first.path);
    Ok(found)
}

pub(crate) fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|error| Error::from(error).at(path))
}

pub(crate) fn read_target(target: &Target) -> Result<Option<String>, Error> {
    let error = match std::fs::read_to_string(&target.path) {
        Ok(source) => return Ok(Some(source)),
        Err(error) => error,
    };
    if !target.explicit && error.kind() == std::io::ErrorKind::InvalidData {
        return Ok(None);
    }
    Err(Error::from(error).at(&target.path))
}

pub(crate) fn commit(changes: &[FileChange], dry_run: bool) -> Result<(), Error> {
    if dry_run {
        return Ok(());
    }
    for change in changes {
        std::fs::write(&change.file, &change.change.source).map_err(|error| Error::from(error).at(&change.file))?;
    }
    Ok(())
}

fn languages(files: &[Target]) -> Vec<Language> {
    let mut found: Vec<Language> = files.iter().map(|target| target.language).collect();
    found.sort();
    found.dedup();
    found
}

fn compile_each<T>(
    languages: &[Language],
    strict: bool,
    build: impl Fn(Language) -> Result<T, Error>,
) -> Result<HashMap<Language, T>, Error> {
    let mut compiled = HashMap::new();
    let mut failures = Vec::new();
    for language in languages {
        match build(*language) {
            Ok(built) => {
                compiled.insert(*language, built);
            }
            Err(error) if strict => return Err(error),
            Err(error) => failures.push((*language, error)),
        }
    }
    if !compiled.is_empty() || failures.is_empty() {
        return Ok(compiled);
    }
    let names: Vec<&str> = failures.iter().map(|(language, _)| language.name()).collect();
    let Some((_, error)) = failures.into_iter().next() else {
        return Ok(compiled);
    };
    Err(Error::Pattern(format!("does not apply to any language found ({}): {error}", names.join(", "))))
}

fn queries(files: &[Target], language: Option<Language>, pattern: &str) -> Result<HashMap<Language, tree_sitter::Query>, Error> {
    compile_each(&languages(files), language.is_some(), |language| compile(language, pattern))
}

fn search(target: &Target, query: &tree_sitter::Query) -> Result<Option<(String, Vec<Match>)>, Error> {
    let Some(source) = read_target(target)? else {
        return Ok(None);
    };
    let tree = parse(source.as_bytes(), target.language).map_err(|error| error.at(&target.path))?;
    let found = matches(query, &tree, source.as_bytes());
    Ok(Some((source, found)))
}

pub fn query(paths: &[PathBuf], language: Option<Language>, pattern: &str) -> Result<Vec<FileMatches>, Error> {
    let files = collect(paths, language)?;
    let compiled = queries(&files, language, pattern)?;
    let results: Result<Vec<Option<FileMatches>>, Error> = files
        .par_iter()
        .map(|target| {
            let Some(query) = compiled.get(&target.language) else {
                return Ok(None);
            };
            let Some((_, found)) = search(target, query)? else {
                return Ok(None);
            };
            Ok((!found.is_empty()).then(|| FileMatches {
                file: target.path.clone(),
                language: target.language.name().to_owned(),
                matches: found,
            }))
        })
        .collect();
    Ok(results?.into_iter().flatten().collect())
}

pub(crate) fn changed(target: &Target, source: &str, edits: &[Edit], allow_errors: bool) -> Result<Option<FileChange>, Error> {
    if edits.is_empty() {
        return Ok(None);
    }
    let change = apply(source, target.language, edits, &target.path.display().to_string(), allow_errors)
        .map_err(|error| error.at(&target.path))?;
    Ok((change.source != source).then(|| FileChange {
        file: target.path.clone(),
        change,
    }))
}

pub(crate) fn finish(results: Result<Vec<Option<FileChange>>, Error>, dry_run: bool) -> Result<Vec<FileChange>, Error> {
    let changes: Vec<FileChange> = results?.into_iter().flatten().collect();
    commit(&changes, dry_run)?;
    Ok(changes)
}

pub fn rewrite_query(
    paths: &[PathBuf],
    language: Option<Language>,
    pattern: &str,
    replacements: &BTreeMap<String, String>,
    dry_run: bool,
    allow_errors: bool,
) -> Result<Vec<FileChange>, Error> {
    let files = collect(paths, language)?;
    let compiled = queries(&files, language, pattern)?;
    for query in compiled.values() {
        rewrite::validate(query, replacements)?;
    }
    let results = files
        .par_iter()
        .map(|target| {
            let Some(query) = compiled.get(&target.language) else {
                return Ok(None);
            };
            let Some((source, found)) = search(target, query)? else {
                return Ok(None);
            };
            changed(target, &source, &rewrite::edits(&found, replacements), allow_errors)
        })
        .collect();
    finish(results, dry_run)
}

fn grouped(requests: &[FileEdits]) -> Result<Vec<(PathBuf, Language, Vec<Edit>)>, Error> {
    let mut groups: BTreeMap<PathBuf, (Language, Vec<Edit>)> = BTreeMap::new();
    for request in requests {
        let path = &request.file;
        let language = match &request.language {
            Some(name) => name.parse(),
            None => Language::from_path(path),
        }
        .map_err(|error| error.at(path))?;
        groups
            .entry(path.clone())
            .or_insert_with(|| (language, Vec::new()))
            .1
            .extend(request.edits.iter().cloned());
    }
    Ok(groups.into_iter().map(|(path, (language, edits))| (path, language, edits)).collect())
}

pub fn apply_edits(requests: &[FileEdits], dry_run: bool, allow_errors: bool) -> Result<Vec<FileChange>, Error> {
    let results: Result<Vec<FileChange>, Error> = grouped(requests)?
        .par_iter()
        .map(|(path, language, edits)| {
            let source = read(path)?;
            let change = apply(&source, *language, edits, &path.display().to_string(), allow_errors)
                .map_err(|error| error.at(path))?;
            Ok(FileChange {
                file: path.clone(),
                change,
            })
        })
        .collect();
    let changes = results?;
    commit(&changes, dry_run)?;
    Ok(changes)
}

type Compiled = HashMap<(usize, Language), Pattern>;

fn patterns(paths: &[PathBuf], language: Option<Language>, rules: &[Rule]) -> Result<(Vec<Target>, Compiled), Error> {
    let files = collect(paths, language)?;
    let present = languages(&files);
    let mut compiled = HashMap::new();
    for (index, rule) in rules.iter().enumerate() {
        let forced: Option<Language> = rule.language.as_deref().map(str::parse).transpose()?;
        let wanted = forced.or(language);
        let candidates: Vec<Language> = present
            .iter()
            .copied()
            .filter(|detected| wanted.is_none_or(|wanted| wanted == *detected))
            .collect();
        let built = compile_each(&candidates, wanted.is_some(), |detected| Pattern::new(rule, detected))?;
        compiled.extend(built.into_iter().map(|(detected, pattern)| ((index, detected), pattern)));
    }
    Ok((files, compiled))
}

fn find_in(target: &Target, rules: &[Rule], compiled: &Compiled) -> Result<Option<(String, Vec<RuleMatch>)>, Error> {
    let Some(source) = read_target(target)? else {
        return Ok(None);
    };
    let tree = parse(source.as_bytes(), target.language).map_err(|error| error.at(&target.path))?;
    let found = (0..rules.len())
        .filter_map(|index| compiled.get(&(index, target.language)).map(|pattern| (index, pattern)))
        .flat_map(|(rule, pattern)| {
            pattern
                .find(&tree, source.as_bytes())
                .into_iter()
                .map(move |found| RuleMatch { rule, found })
        })
        .collect();
    Ok(Some((source, found)))
}

pub fn find(paths: &[PathBuf], language: Option<Language>, rules: &[Rule]) -> Result<Vec<FilePatternMatches>, Error> {
    let (files, compiled) = patterns(paths, language, rules)?;
    let results: Result<Vec<Option<FilePatternMatches>>, Error> = files
        .par_iter()
        .map(|target| {
            let Some((_, matches)) = find_in(target, rules, &compiled)? else {
                return Ok(None);
            };
            Ok((!matches.is_empty()).then(|| FilePatternMatches {
                file: target.path.clone(),
                language: target.language.name().to_owned(),
                matches,
            }))
        })
        .collect();
    Ok(results?.into_iter().flatten().collect())
}

pub fn rewrite(
    paths: &[PathBuf],
    language: Option<Language>,
    rules: &[Rule],
    dry_run: bool,
    allow_errors: bool,
) -> Result<Vec<FileChange>, Error> {
    if rules.iter().any(|rule| rule.rewrite.is_none()) {
        return Err(Error::MissingRewrite);
    }
    let (files, compiled) = patterns(paths, language, rules)?;
    let results = files
        .par_iter()
        .map(|target| {
            let Some((source, matches)) = find_in(target, rules, &compiled)? else {
                return Ok(None);
            };
            let planned: Vec<pattern::Planned<'_>> = matches
                .iter()
                .filter_map(|each| {
                    let template = rules.get(each.rule)?.rewrite.as_deref()?;
                    Some(pattern::Planned {
                        found: &each.found,
                        template,
                    })
                })
                .collect();
            changed(target, &source, &pattern::edits(&planned, &source)?, allow_errors)
        })
        .collect();
    finish(results, dry_run)
}
