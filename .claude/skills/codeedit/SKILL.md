---
name: codeedit
description: Use when reading or editing source code by structure instead of by line or regex — replacing a function or method body, inserting or deleting a definition, adding an import, renaming an identifier across files, bulk rewrites like `$E.unwrap()` to `$E?`, finding call sites, listing a file's definitions, or dumping a syntax tree as JSON. Covers Rust, Go, Python, TypeScript, JavaScript, JSX, TSX, Zig, Bash, JSON, YAML, XML, HTML and Markdown. Use instead of sed, awk or hand-counted line ranges.
---

# codeedit

The `codeedit` CLI and the `treesitter` Python package (`import treesitter`), built on tree-sitter. Reads code as a syntax tree, edits it by symbol or pattern, and rejects any edit that is stale, overlapping, or adds syntax errors. Every editing command prints a unified diff; `-n` previews without writing, `--json` prints `{file, diff, edits, errors_before, errors_after}`.

Language comes from the file extension; force it with `-l python` (`apply` and `edit` take a `"language"` key instead). Run `codeedit <command> --help` for flags. JSON input with an unknown key is rejected, so a typo cannot silently drop a check. CRLF files stay CRLF.

## Read

```bash
codeedit outline src/app.py               # definitions: name, path, kind, start/end [row,col], bytes, signature
codeedit get src/app.py Greeter.greet     # one symbol with its full "text"
codeedit parse src/lib.rs                 # tree as JSON; -f sexp|xml|dot
cat x.json | codeedit parse - -l json -f sexp
```

Symbol paths: `Class.method`, Rust `impl Point` / `impl Display for Point` / `Point.new`, Go receiver methods `List.Push`. A bare name works when unique; an ambiguous one errors and lists candidates as `path:line`, which addresses one of them (`A.x:6` for a property setter). A symbol includes its decorators, attributes, `export` and doc comments. `get` returns `text` dedented to column 0, so it can be edited and passed back to `replace`; `bytes` is the exact range in the file.

## Edit one symbol

```bash
codeedit replace    src/app.py Greeter.greet --body -c 'return f"hi {self.name}"'
codeedit replace    src/app.py helper --code-file new_helper.py     # whole definition
codeedit insert     src/app.py Greeter.greet --position after -c 'def bye(self):
    return "bye"'
codeedit delete     src/app.py old_helper
codeedit add-import src/app.py 'from pathlib import Path'           # no-op if present
```

Positions: `before`/`after` the symbol, `start`/`end` of its body (`start` goes after a Python docstring). Write code unindented; it is reindented to fit, except lines inside multi-line strings and heredocs. A Python one-line body (`def f(): return 1`) becomes a block. An arrow function with an expression body only supports `replace --body`. A declaration naming several symbols (`var x, y = 1, 2`) cannot be replaced or deleted per name; use `apply`.

Several operations as JSON. Operations on one file apply in order, each seeing the result of the last. Every file is checked before any is written, every target is opened for writing first, and a write that still fails puts back the files already written:

```bash
codeedit edit - <<'EOF'
[{"op": "replace_body", "file": "a.py", "symbol": "f", "code": "return 1"},
 {"op": "insert", "file": "a.py", "symbol": "f", "position": "after", "code": "def g():\n    pass"},
 {"op": "add_import", "file": "a.py", "import": "import os"}]
EOF
```

Ops: `replace`, `replace_body`, `insert`, `delete`, `add_import`.

## Bulk rewrite with patterns

Write the pattern as code. `$A` matches one node, `$$$A` zero or more, `$_` / `$$$` match without binding. A name used twice must match the same text. Every node in the pattern must match; a comma or semicolon in the code that the pattern leaves out is skipped, nothing else (so `def $F` does not match `async def`). Write `\$` for a literal dollar, e.g. `cd \$HOME` in Bash (Python, Go, Rust, Bash, JS and TS). In a template, `$NAME_v2` uses the binding `NAME`. Write multi-line templates unindented: they take the indentation of the match, and multi-line bindings move with them. Whitespace inside a template is copied as written.

```bash
codeedit find    src -p 'print($$$ARGS)'                       # JSON matches with bindings
codeedit rewrite src -p '$E.unwrap()' -r '$E?' -n
codeedit rename  src --from old_name --to new_name             # by spelling, no type resolution
```

Rules file (`--rules FILE` or `-`) for constraints and multiple rules applied together:

```json
[{"pattern": "print($A)", "rewrite": "log($A)", "where": {"A": {"regex": "^msg", "kind": "identifier"}}},
 {"pattern": "$X: $T = $V", "context": "class C:\n    $X: $T = $V",
  "selector": "expression_statement", "rewrite": "$X: $T | None = $V"}]
```

Use `context` + `selector` when the pattern alone does not parse as the node you mean; the error says so. The context must contain the pattern text, and `selector` picks the enclosing node of that type. Rust match arms (`Command::Find { $$$A } => $B`) and impl items parse without it. A function pattern needs a block body: `fn $N($$$P) { $$$B }`, not `fn $N($$$P) $B`, and a metavariable cannot stand where the grammar needs structure, such as a struct field. `where` names must be metavariables of the pattern; `kind` accepts supertypes like `expression`.

## Raw tree-sitter queries and byte edits

```bash
codeedit query src -q '(call_expression function: (identifier) @f (#eq? @f "foo"))'
codeedit rewrite-query src -q '(call function: (identifier) @f (#eq? @f "foo"))' -r 'f=bar'
echo '[{"file": "a.py", "edits": [{"bytes": [10, 15], "expect": "hello", "replace": "world"}]}]' | codeedit apply -
```

Get byte ranges from `outline`, `get`, `find` or `parse`, and pass `expect` so a stale range is rejected. `rewrite-query` templates are inserted as written, without re-indentation; repeated captures (`(decorator)+ @dec`) each use their own text.

## Common mistakes

- In zsh, `echo '...\n...'` turns `\n` into a real newline and breaks JSON. Use a heredoc or `printf '%s'`.
- `rename` renames every identifier with that spelling, including unrelated ones. Preview with `-n`.
- `outline`/`get` do not support JSON, YAML, XML, HTML or Markdown; use `query` or `parse` there.
- Without `-l`, a directory walk skips files whose language the pattern does not parse in; it errors only if no language fits.
- Rust macro arguments (`assert!(...)`, `vec![...]`) are token trees, so a normal pattern does not see code inside them. Match the macro call itself: `assert!(edits($$$A).is_err())`.
- Existing syntax errors are tolerated; an edit is rejected only when it adds an error somewhere new. `--allow-errors` forces such an edit through.
