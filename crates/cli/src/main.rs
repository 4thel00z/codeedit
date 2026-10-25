use std::collections::BTreeMap;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use codeedit::files::{self, FileChange, FileEdits};
use codeedit::tools::{self, Operation, Position, Request};
use codeedit::{Format, Language, Rule};

#[derive(Parser)]
#[command(name = "codeedit", version, about = "Parse, query and edit code in any language")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a file (or `-` for stdin) and print its syntax tree
    Parse {
        file: PathBuf,
        #[arg(long, short)]
        lang: Option<String>,
        #[arg(long, short, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    /// Run a tree-sitter query over files and directories, print matches as JSON
    Query {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long, short)]
        query: String,
        #[arg(long, short)]
        lang: Option<String>,
    },
    /// Replace query captures: `-r name=template`, where `@capture` in a template inserts that capture's text
    RewriteQuery {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long, short)]
        query: String,
        #[arg(long = "replace", short, value_parser = name_template, required = true)]
        replacements: Vec<(String, String)>,
        #[arg(long, short)]
        lang: Option<String>,
        #[command(flatten)]
        options: EditOptions,
    },
    /// Find code matching a pattern such as `foo($$$ARGS)`, print matches and bindings as JSON
    Find {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[command(flatten)]
        rules: RuleArgs,
        #[arg(long, short)]
        lang: Option<String>,
    },
    /// Rewrite code matching a pattern: `-p 'foo($$$A)' -r 'bar($$$A)'`
    Rewrite {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[command(flatten)]
        rules: RuleArgs,
        #[arg(long, short)]
        lang: Option<String>,
        #[command(flatten)]
        options: EditOptions,
    },
    /// Apply JSON edits read from a file or `-` for stdin: [{"file", "edits": [{"bytes", "expect", "replace"}]}]
    Apply {
        #[arg(default_value = "-")]
        input: PathBuf,
        #[command(flatten)]
        options: EditOptions,
    },
    /// List classes, functions, methods and other definitions in a file as JSON
    Outline {
        file: PathBuf,
        #[arg(long, short)]
        lang: Option<String>,
    },
    /// Print one symbol (e.g. `Greeter.greet`) with its source as JSON
    Get {
        file: PathBuf,
        symbol: String,
        #[arg(long, short)]
        lang: Option<String>,
    },
    /// Replace a whole symbol, or only its body with --body
    Replace {
        file: PathBuf,
        #[arg(long, short)]
        lang: Option<String>,
        symbol: String,
        #[command(flatten)]
        code: CodeArgs,
        #[arg(long)]
        body: bool,
        #[command(flatten)]
        options: EditOptions,
    },
    /// Insert code before/after a symbol or at the start/end of its body
    Insert {
        file: PathBuf,
        #[arg(long, short)]
        lang: Option<String>,
        symbol: String,
        #[command(flatten)]
        code: CodeArgs,
        #[arg(long, value_enum)]
        position: InsertPosition,
        #[command(flatten)]
        options: EditOptions,
    },
    /// Delete a symbol together with its decorators or attributes
    Delete {
        file: PathBuf,
        #[arg(long, short)]
        lang: Option<String>,
        symbol: String,
        #[command(flatten)]
        options: EditOptions,
    },
    /// Rename every identifier spelled OLD to NEW (syntax only, no type resolution)
    Rename {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, short)]
        lang: Option<String>,
        #[command(flatten)]
        options: EditOptions,
    },
    /// Add an import unless it is already present
    AddImport {
        file: PathBuf,
        #[arg(long, short)]
        lang: Option<String>,
        import: String,
        #[command(flatten)]
        options: EditOptions,
    },
    /// Run a JSON batch of operations from a file or `-`: [{"op": "replace_body", "file", "symbol", "code"}, ...]
    Edit {
        #[arg(default_value = "-")]
        input: PathBuf,
        #[command(flatten)]
        options: EditOptions,
    },
    /// List supported languages
    Languages,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct CodeArgs {
    /// Code to insert or replace with
    #[arg(long, short)]
    code: Option<String>,
    /// Read the code from a file or `-` for stdin
    #[arg(long)]
    code_file: Option<PathBuf>,
}

impl CodeArgs {
    fn load(self) -> Result<String> {
        if let Some(code) = self.code {
            return Ok(code);
        }
        let Some(file) = self.code_file else {
            bail!("pass --code or --code-file");
        };
        Ok(String::from_utf8(input(&file)?)?)
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum InsertPosition {
    Before,
    After,
    Start,
    End,
}

impl From<InsertPosition> for Position {
    fn from(position: InsertPosition) -> Self {
        match position {
            InsertPosition::Before => Self::Before,
            InsertPosition::After => Self::After,
            InsertPosition::Start => Self::Start,
            InsertPosition::End => Self::End,
        }
    }
}

#[derive(Args)]
#[group(required = true, multiple = true)]
struct RuleArgs {
    /// Code pattern; `$NAME` matches one node, `$$$NAME` matches zero or more
    #[arg(long, short, conflicts_with = "rules")]
    pattern: Option<String>,
    /// Replacement template using the pattern's metavariables
    #[arg(long, short, requires = "pattern")]
    rewrite: Option<String>,
    /// JSON rule or list of rules from a file or `-`: [{"pattern", "rewrite", "language", "where", "context", "selector"}]
    #[arg(long)]
    rules: Option<PathBuf>,
}

impl RuleArgs {
    fn load(self) -> Result<Vec<Rule>> {
        if let Some(pattern) = self.pattern {
            return Ok(vec![Rule {
                pattern,
                rewrite: self.rewrite,
                ..Rule::default()
            }]);
        }
        let Some(file) = self.rules else {
            bail!("pass --pattern or --rules");
        };
        one_or_many(&input(&file)?, "rules")
    }
}

#[derive(Args)]
struct EditOptions {
    /// Print the diff without writing files
    #[arg(long, short = 'n')]
    dry_run: bool,
    /// Apply even if the edit adds syntax errors
    #[arg(long)]
    allow_errors: bool,
    /// Print results as JSON instead of a unified diff
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Sexp,
    Xml,
    Dot,
}

impl From<OutputFormat> for Format {
    fn from(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Json => Self::Json,
            OutputFormat::Sexp => Self::Sexp,
            OutputFormat::Xml => Self::Xml,
            OutputFormat::Dot => Self::Dot,
        }
    }
}

fn name_template(argument: &str) -> Result<(String, String), String> {
    let Some((name, template)) = argument.split_once('=') else {
        return Err("expected name=template".to_owned());
    };
    Ok((name.to_owned(), template.to_owned()))
}

fn language(file: &Path, lang: Option<&str>) -> Result<Language> {
    if let Some(name) = lang {
        return Ok(name.parse()?);
    }
    if file.as_os_str() == "-" {
        bail!("reading from stdin requires --lang");
    }
    Ok(Language::from_path(file)?)
}

fn optional_language(lang: Option<&str>) -> Result<Option<Language>> {
    Ok(lang.map(str::parse).transpose()?)
}

fn input(file: &Path) -> Result<Vec<u8>> {
    if file.as_os_str() != "-" {
        return std::fs::read(file).with_context(|| format!("reading {}", file.display()));
    }
    let mut buffer = Vec::new();
    io::stdin().read_to_end(&mut buffer)?;
    Ok(buffer)
}

fn one_or_many<T: serde::de::DeserializeOwned>(raw: &[u8], what: &str) -> Result<Vec<T>> {
    let value: serde_json::Value = serde_json::from_slice(raw).with_context(|| format!("{what} are not valid JSON"))?;
    if value.is_array() {
        return Ok(serde_json::from_value(value)?);
    }
    Ok(vec![serde_json::from_value(value)?])
}

fn report(changes: &[FileChange], options: &EditOptions, out: &mut impl Write) -> Result<()> {
    if options.json {
        serde_json::to_writer(&mut *out, changes)?;
        writeln!(out)?;
        return Ok(());
    }
    for change in changes {
        out.write_all(change.change.diff.as_bytes())?;
    }
    Ok(())
}

fn replacement(symbol: String, code: String, body: bool) -> Operation {
    if body {
        return Operation::ReplaceBody { symbol, code };
    }
    Operation::Replace { symbol, code }
}

fn single(file: PathBuf, lang: Option<&str>, operation: Operation, options: &EditOptions, out: &mut impl Write) -> Result<()> {
    let request = Request {
        file,
        language: lang.map(str::to_owned),
        operation,
    };
    let changes = tools::run(&[request], options.dry_run, options.allow_errors)?;
    report(&changes, options, out)
}

fn run(cli: Cli) -> Result<()> {
    let mut out = BufWriter::new(io::stdout().lock());
    match cli.command {
        Command::Parse { file, lang, format } => {
            let language = language(&file, lang.as_deref())?;
            let source = input(&file)?;
            let tree = codeedit::parse(&source, language)?;
            codeedit::write(&tree, &source, format.into(), &mut out)?;
        }
        Command::Query { paths, query, lang } => {
            let found = files::query(&paths, optional_language(lang.as_deref())?, &query)?;
            serde_json::to_writer(&mut out, &found)?;
            writeln!(out)?;
        }
        Command::RewriteQuery {
            paths,
            query,
            replacements,
            lang,
            options,
        } => {
            let replacements: BTreeMap<String, String> = replacements.into_iter().collect();
            let language = optional_language(lang.as_deref())?;
            let changes = files::rewrite_query(&paths, language, &query, &replacements, options.dry_run, options.allow_errors)?;
            report(&changes, &options, &mut out)?;
        }
        Command::Find { paths, rules, lang } => {
            if rules.rewrite.is_some() {
                bail!("find does not rewrite; use `codeedit rewrite`");
            }
            let found = files::find(&paths, optional_language(lang.as_deref())?, &rules.load()?)?;
            serde_json::to_writer(&mut out, &found)?;
            writeln!(out)?;
        }
        Command::Rewrite {
            paths,
            rules,
            lang,
            options,
        } => {
            let language = optional_language(lang.as_deref())?;
            let changes = files::rewrite(&paths, language, &rules.load()?, options.dry_run, options.allow_errors)?;
            report(&changes, &options, &mut out)?;
        }
        Command::Apply { input: file, options } => {
            let requests: Vec<FileEdits> = one_or_many(&input(&file)?, "edits")?;
            let changes = files::apply_edits(&requests, options.dry_run, options.allow_errors)?;
            report(&changes, &options, &mut out)?;
        }
        Command::Outline { file, lang } => {
            let symbols = tools::file_outline(&file, optional_language(lang.as_deref())?)?;
            serde_json::to_writer(&mut out, &symbols)?;
            writeln!(out)?;
        }
        Command::Get { file, symbol, lang } => {
            let found = tools::get(&file, optional_language(lang.as_deref())?, &symbol)?;
            serde_json::to_writer(&mut out, &found)?;
            writeln!(out)?;
        }
        Command::Replace {
            file,
            lang,
            symbol,
            code,
            body,
            options
        } => {
            single(file, lang.as_deref(), replacement(symbol, code.load()?, body), &options, &mut out)?;
        },
        Command::Insert {
            file,
            lang,
            symbol,
            code,
            position,
            options
        } => {
            let operation = Operation::Insert {
                symbol,
                code: code.load()?,
                position: position.into(),
            };
            single(file, lang.as_deref(), operation, &options, &mut out)?;
        },
        Command::Delete { file, lang, symbol, options } => single(file, lang.as_deref(), Operation::Delete { symbol }, &options, &mut out)?,
        Command::AddImport { file, lang, import, options } => single(file, lang.as_deref(), Operation::AddImport { import }, &options, &mut out)?,
        Command::Rename {
            paths,
            from,
            to,
            lang,
            options,
        } => {
            let language = optional_language(lang.as_deref())?;
            let changes = tools::rename(&paths, language, &from, &to, options.dry_run, options.allow_errors)?;
            report(&changes, &options, &mut out)?;
        }
        Command::Edit { input: file, options } => {
            let requests: Vec<Request> = one_or_many(&input(&file)?, "operations")?;
            let changes = tools::run(&requests, options.dry_run, options.allow_errors)?;
            report(&changes, &options, &mut out)?;
        }
        Command::Languages => {
            for language in Language::ALL {
                writeln!(out, "{language}")?;
            }
        }
    }
    out.flush()?;
    Ok(())
}

fn main() -> ExitCode {
    let Err(error) = run(Cli::parse()) else {
        return ExitCode::SUCCESS;
    };
    eprintln!("error: {error:#}");
    ExitCode::FAILURE
}
