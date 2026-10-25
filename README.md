<p align="center">
  <img src="logo.svg" alt="codeedit" width="180">
</p>

<h1 align="center">codeedit</h1>

<p align="center">
  <strong>Structural code edits for coding agents: a better sed that reads the syntax tree.</strong>
</p>

<p align="center">
  <a href="https://github.com/4thel00z/codeedit/actions/workflows/ci.yml"><img src="https://github.com/4thel00z/codeedit/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/4thel00z/codeedit/actions/workflows/release-please.yml"><img src="https://github.com/4thel00z/codeedit/actions/workflows/release-please.yml/badge.svg" alt="release"></a>
  <a href="https://pypi.org/project/treesitter/"><img src="https://img.shields.io/pypi/v/treesitter?logo=pypi&logoColor=white" alt="PyPI"></a>
  <img src="https://img.shields.io/badge/python-3.13%2B-3776ab?logo=python&logoColor=white" alt="Python 3.13+">
  <img src="https://img.shields.io/badge/rust-2024-000000?logo=rust&logoColor=white" alt="Rust 2024">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT"></a>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#usage">Usage</a> ·
  <a href="#python">Python</a> ·
  <a href="#agent-skill">Agent skill</a> ·
  <a href="#performance">Performance</a>
</p>

---

Agents edit code with `sed` and hand-counted line ranges, and break it. codeedit parses the file with [tree-sitter](https://tree-sitter.github.io), finds code by symbol or by a pattern written as code, and applies the edit only if it is fresh, does not overlap another edit, and adds no syntax error. Every edit comes back as a unified diff. One Rust core sits behind the `codeedit` CLI and the `treesitter` Python package.

## Highlights

- **Edit by symbol**: `replace`, `insert`, `delete`, `rename` and `add-import` address `Greeter.greet`, `impl Point` or `List.Push`, with decorators, attributes and doc comments attached.
- **Edit by pattern**: `$E.unwrap()` → `$E?` across a directory; `$A` matches one node, `$$$A` any number, `where` filters by regex or node type.
- **Safe by default**: stale, overlapping or syntax-breaking edits are rejected; multi-file changes are written all or nothing.
- **Syntax tree as JSON**: every node with type, field, position, byte range and text; also S-expression, XML and Graphviz.
- **Raw tree-sitter queries** when patterns are not enough.
- **13 languages**: Rust, Go, Python, TypeScript, TSX, JavaScript/JSX, Zig, Bash, JSON, YAML, XML, HTML, Markdown.

## Install

```bash
pip install treesitter                                          # Python package, abi3 wheels for 3.13+
cargo install --git https://github.com/4thel00z/codeedit codeedit   # the `codeedit` binary
```

Prebuilt `codeedit` binaries for Linux, macOS and Windows are attached to every [release](https://github.com/4thel00z/codeedit/releases).

## Usage

```bash
codeedit outline src/app.py                          # classes, functions, methods as JSON
codeedit get     src/app.py Greeter.greet            # one symbol and its source
codeedit replace src/app.py Greeter.greet --body -c 'return f"hi {self.name}"'
codeedit insert  src/app.py Greeter.greet --position after -c 'def bye(self): ...'
codeedit delete  src/app.py old_helper
codeedit add-import src/app.py 'from pathlib import Path'
codeedit rename  src --from fetch --to load -n       # -n previews the diff

codeedit find    src -p 'print($$$ARGS)'             # matches and bindings as JSON
codeedit rewrite src -p '$E.unwrap()' -r '$E?'
codeedit query   src -q '(call_expression function: (identifier) @f)'
codeedit parse   src/lib.rs -f json                  # or sexp, xml, dot
```

Batches go in as JSON and are checked together before anything is written:

```bash
codeedit edit - <<'EOF'
[{"op": "replace_body", "file": "a.py", "symbol": "f", "code": "return 1"},
 {"op": "add_import", "file": "a.py", "import": "import os"}]
EOF
```

## Python

```python
import treesitter

tree = treesitter.parse("def f(): return 1", "python")          # syntax tree as dicts
treesitter.outline("src/app.py")                                # symbols
treesitter.replace_body("src/app.py", "Greeter.greet", "return 'hi'")
treesitter.rewrite("src", "$E.unwrap()", "$E?", dry_run=True)   # list of {file, diff, ...}
change = treesitter.rewrite_source("foo(1)\n", "python", "foo($A)", "bar($A)")
```

## Agent skill

[`.claude/skills/codeedit/SKILL.md`](.claude/skills/codeedit/SKILL.md) teaches a coding agent every command and its pitfalls. Link it into your agent's skill directory:

```bash
ln -s "$PWD/.claude/skills/codeedit" ~/.claude/skills/codeedit
```

## Performance

Release build on an Apple M3 Pro:

| Task | Time |
|---|---:|
| Parse a 4,159-line Rust file and print the tree as JSON | 20 ms |
| `find '$E.unwrap()'` over 120 files, 36k lines | 20 ms |
| `rewrite -n` of 4,025 call matches over the same files | 70 ms |

## Development

```bash
uv sync                                  # builds the extension into .venv
uv run pytest
cargo test --workspace --exclude treesitter-python
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs clippy, tests on Linux and macOS, pytest, a wheel and a binary smoke test. Releases are cut by [release-please](https://github.com/googleapis/release-please) from conventional commits: merging its release PR tags the version, attaches `codeedit` binaries, and publishes wheels to PyPI through trusted publishing.

## License

MIT. The vendored [tree-sitter](vendor/tree-sitter) runtime and all grammars are MIT as well; see [`vendor/tree-sitter/UPSTREAM`](vendor/tree-sitter/UPSTREAM) for the one local patch.
