use std::collections::BTreeMap;
use std::path::PathBuf;

use pyo3::exceptions::{PyOSError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use serde::Serialize;
use treesitter::files::{self, FileEdits};
use treesitter::query::{compile, matches};
use treesitter::tools::{self, Operation, Request};
use treesitter::{Edit, Error, Format, Language, Rule};

fn to_py(error: Error) -> PyErr {
    let message = error.to_string();
    match error.root() {
        Error::Io(_) => PyOSError::new_err(message),
        Error::Grammar(_) | Error::Parse => PyRuntimeError::new_err(message),
        _ => PyValueError::new_err(message),
    }
}

fn json(value: &impl Serialize) -> Result<String, Error> {
    Ok(serde_json::to_string(value)?)
}

fn optional_language(language: Option<&str>) -> PyResult<Option<Language>> {
    language.map(str::parse).transpose().map_err(to_py)
}

fn render_source(source: &[u8], language: Language, format: &str) -> Result<String, Error> {
    let format: Format = format.parse()?;
    let tree = treesitter::parse(source, language)?;
    treesitter::render(&tree, source, format)
}

#[pyfunction]
#[pyo3(signature = (source, language, format = "json"))]
fn render(py: Python<'_>, source: &[u8], language: &str, format: &str) -> PyResult<String> {
    let language: Language = language.parse().map_err(to_py)?;
    py.detach(|| render_source(source, language, format)).map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (path, language = None, format = "json"))]
fn render_file(py: Python<'_>, path: PathBuf, language: Option<&str>, format: &str) -> PyResult<String> {
    let language = match language {
        Some(name) => name.parse(),
        None => Language::from_path(&path),
    }
    .map_err(to_py)?;
    py.detach(|| {
        let source = std::fs::read(&path)?;
        render_source(&source, language, format)
    })
    .map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (paths, pattern, language = None))]
fn query(py: Python<'_>, paths: Vec<PathBuf>, pattern: &str, language: Option<&str>) -> PyResult<String> {
    let language = optional_language(language)?;
    py.detach(|| json(&files::query(&paths, language, pattern)?)).map_err(to_py)
}

#[pyfunction]
fn query_source(py: Python<'_>, source: &[u8], language: &str, pattern: &str) -> PyResult<String> {
    let language: Language = language.parse().map_err(to_py)?;
    py.detach(|| {
        let query = compile(language, pattern)?;
        let tree = treesitter::parse(source, language)?;
        json(&matches(&query, &tree, source))
    })
    .map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (paths, pattern, replacements, language = None, dry_run = false, allow_errors = false))]
fn rewrite_query(
    py: Python<'_>,
    paths: Vec<PathBuf>,
    pattern: &str,
    replacements: BTreeMap<String, String>,
    language: Option<&str>,
    dry_run: bool,
    allow_errors: bool,
) -> PyResult<String> {
    let language = optional_language(language)?;
    py.detach(|| json(&files::rewrite_query(&paths, language, pattern, &replacements, dry_run, allow_errors)?))
        .map_err(to_py)
}

fn rules(raw: &str) -> Result<Vec<Rule>, Error> {
    Ok(serde_json::from_str(raw)?)
}

#[pyfunction]
#[pyo3(signature = (paths, rules_json, language = None))]
fn find(py: Python<'_>, paths: Vec<PathBuf>, rules_json: &str, language: Option<&str>) -> PyResult<String> {
    let language = optional_language(language)?;
    py.detach(|| json(&files::find(&paths, language, &rules(rules_json)?)?)).map_err(to_py)
}

#[pyfunction]
fn find_source(py: Python<'_>, source: &[u8], language: &str, rules_json: &str) -> PyResult<String> {
    let language: Language = language.parse().map_err(to_py)?;
    py.detach(|| {
        let tree = treesitter::parse(source, language)?;
        let found = rules(rules_json)?
            .iter()
            .enumerate()
            .map(|(index, rule)| Ok((index, treesitter::Pattern::new(rule, language)?.find(&tree, source))))
            .collect::<Result<Vec<_>, Error>>()?;
        let flat: Vec<files::RuleMatch> = found
            .into_iter()
            .flat_map(|(rule, matches)| matches.into_iter().map(move |found| files::RuleMatch { rule, found }))
            .collect();
        json(&flat)
    })
    .map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (paths, rules_json, language = None, dry_run = false, allow_errors = false))]
fn rewrite(
    py: Python<'_>,
    paths: Vec<PathBuf>,
    rules_json: &str,
    language: Option<&str>,
    dry_run: bool,
    allow_errors: bool,
) -> PyResult<String> {
    let language = optional_language(language)?;
    py.detach(|| json(&files::rewrite(&paths, language, &rules(rules_json)?, dry_run, allow_errors)?)).map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (source, language, rules_json, allow_errors = false))]
fn rewrite_source(py: Python<'_>, source: &str, language: &str, rules_json: &str, allow_errors: bool) -> PyResult<String> {
    let language: Language = language.parse().map_err(to_py)?;
    py.detach(|| {
        let tree = treesitter::parse(source.as_bytes(), language)?;
        let rules = rules(rules_json)?;
        let mut found = Vec::with_capacity(rules.len());
        for rule in &rules {
            let template = rule.rewrite.as_deref().ok_or(Error::MissingRewrite)?;
            found.push((treesitter::Pattern::new(rule, language)?.find(&tree, source.as_bytes()), template));
        }
        let planned: Vec<_> = found.iter().flat_map(|(found, template)| treesitter::pattern::plan(found, template)).collect();
        let edits = treesitter::pattern::edits(&planned, source);
        let change = treesitter::apply(source, language, &edits, "source", allow_errors)?;
        json(&SourceChange {
            source: change.source.clone(),
            change,
        })
    })
    .map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (requests, dry_run = false, allow_errors = false))]
fn apply(py: Python<'_>, requests: &str, dry_run: bool, allow_errors: bool) -> PyResult<String> {
    py.detach(|| {
        let requests: Vec<FileEdits> = serde_json::from_str(requests)?;
        json(&files::apply_edits(&requests, dry_run, allow_errors)?)
    })
    .map_err(to_py)
}

#[derive(Serialize)]
struct SourceChange {
    source: String,
    #[serde(flatten)]
    change: treesitter::Change,
}

#[pyfunction]
#[pyo3(signature = (source, language, edits, allow_errors = false))]
fn apply_source(py: Python<'_>, source: &str, language: &str, edits: &str, allow_errors: bool) -> PyResult<String> {
    let language: Language = language.parse().map_err(to_py)?;
    py.detach(|| {
        let edits: Vec<Edit> = serde_json::from_str(edits)?;
        let change = treesitter::apply(source, language, &edits, "source", allow_errors)?;
        json(&SourceChange {
            source: change.source.clone(),
            change,
        })
    })
    .map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (path, language = None))]
fn outline(py: Python<'_>, path: PathBuf, language: Option<&str>) -> PyResult<String> {
    let language = optional_language(language)?;
    py.detach(|| json(&tools::file_outline(&path, language)?)).map_err(to_py)
}

#[pyfunction]
fn outline_source(py: Python<'_>, source: &[u8], language: &str) -> PyResult<String> {
    let language: Language = language.parse().map_err(to_py)?;
    py.detach(|| {
        let tree = treesitter::parse(source, language)?;
        json(&treesitter::symbols::outline(&tree, source, language)?)
    })
    .map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (path, symbol, language = None))]
fn get(py: Python<'_>, path: PathBuf, symbol: &str, language: Option<&str>) -> PyResult<String> {
    let language = optional_language(language)?;
    py.detach(|| json(&tools::get(&path, language, symbol)?)).map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (requests, dry_run = false, allow_errors = false))]
fn edit(py: Python<'_>, requests: &str, dry_run: bool, allow_errors: bool) -> PyResult<String> {
    py.detach(|| {
        let requests: Vec<Request> = serde_json::from_str(requests)?;
        json(&tools::run(&requests, dry_run, allow_errors)?)
    })
    .map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (source, language, operations, allow_errors = false))]
fn edit_source(py: Python<'_>, source: &str, language: &str, operations: &str, allow_errors: bool) -> PyResult<String> {
    let language: Language = language.parse().map_err(to_py)?;
    py.detach(|| {
        let operations: Vec<Operation> = serde_json::from_str(operations)?;
        let tree = treesitter::parse(source.as_bytes(), language)?;
        let borrowed: Vec<&Operation> = operations.iter().collect();
        let edits = tools::operations_edits(&tree, source, language, &borrowed)?;
        let change = treesitter::apply(source, language, &edits, "source", allow_errors)?;
        json(&SourceChange {
            source: change.source.clone(),
            change,
        })
    })
    .map_err(to_py)
}

#[pyfunction]
#[pyo3(signature = (paths, old, new, language = None, dry_run = false, allow_errors = false))]
fn rename(
    py: Python<'_>,
    paths: Vec<PathBuf>,
    old: &str,
    new: &str,
    language: Option<&str>,
    dry_run: bool,
    allow_errors: bool,
) -> PyResult<String> {
    let language = optional_language(language)?;
    py.detach(|| json(&tools::rename(&paths, language, old, new, dry_run, allow_errors)?)).map_err(to_py)
}

#[pyfunction]
fn languages() -> Vec<&'static str> {
    Language::ALL.iter().map(|language| language.name()).collect()
}

#[pymodule]
fn native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(render, module)?)?;
    module.add_function(wrap_pyfunction!(render_file, module)?)?;
    module.add_function(wrap_pyfunction!(query, module)?)?;
    module.add_function(wrap_pyfunction!(query_source, module)?)?;
    module.add_function(wrap_pyfunction!(rewrite_query, module)?)?;
    module.add_function(wrap_pyfunction!(find, module)?)?;
    module.add_function(wrap_pyfunction!(find_source, module)?)?;
    module.add_function(wrap_pyfunction!(rewrite, module)?)?;
    module.add_function(wrap_pyfunction!(rewrite_source, module)?)?;
    module.add_function(wrap_pyfunction!(apply, module)?)?;
    module.add_function(wrap_pyfunction!(apply_source, module)?)?;
    module.add_function(wrap_pyfunction!(outline, module)?)?;
    module.add_function(wrap_pyfunction!(outline_source, module)?)?;
    module.add_function(wrap_pyfunction!(get, module)?)?;
    module.add_function(wrap_pyfunction!(edit, module)?)?;
    module.add_function(wrap_pyfunction!(edit_source, module)?)?;
    module.add_function(wrap_pyfunction!(rename, module)?)?;
    module.add_function(wrap_pyfunction!(languages, module)?)?;
    Ok(())
}
