---
name: codeedit
description: Use when reading or editing source code by structure instead of by line or regex — replacing a function or method body, inserting or deleting a definition, adding an import, renaming an identifier across files, bulk rewrites like `$E.unwrap()` to `$E?`, finding call sites, listing a file's definitions, or dumping a syntax tree as JSON. Covers Rust, Go, Python, TypeScript, JavaScript, JSX, TSX, Java, Kotlin, Scala, Swift, C, C++, C#, Dart, Ruby, Crystal, PHP, Lua, Elixir, Haskell, OCaml, Zig, Bash, SQL, Terraform/HCL, Nix, Protobuf, GraphQL, CSS, SCSS, JSON, YAML, TOML, INI, Dockerfile, Makefile, CMake, XML, HTML and Markdown. Use instead of sed, awk or hand-counted line ranges.
---

# codeedit

The `codeedit` CLI and the `codeedit` Python package (`import codeedit`), built on tree-sitter. Reads code as a syntax tree, edits it by symbol or pattern, and rejects any edit that is stale, overlapping, or adds syntax errors. Every editing command prints a unified diff; `-n` previews without writing, `--json` prints `{file, diff, edits, errors_before, errors_after}`.

Language comes from the file extension or name (`Makefile`, `CMakeLists.txt`, `Dockerfile`, `Gemfile`, `.h` is C, `.tf` is HCL); force it with `-l python` (`apply` and `edit` take a `"language"` key instead). `codeedit languages` lists them. Run `codeedit <command> --help` for flags. JSON input with an unknown key is rejected, so a typo cannot silently drop a check. CRLF files stay CRLF.

## Read

```bash
codeedit outline src/app.py               # definitions: name, path, kind, start/end [row,col], bytes, signature
codeedit get src/app.py Greeter.greet     # one symbol with its full "text"
codeedit parse src/lib.rs                 # tree as JSON; -f sexp|xml|dot
cat x.json | codeedit parse - -l json -f sexp
```

Symbol paths: `Class.method`, Rust `impl Point` / `impl Display for Point` / `Point.new`, Go receiver methods `List.Push`, C++ out-of-class definitions `Greeter.greet`, Scala companions `object Greeter`, Swift `extension Greeter`, Haskell `instance Shape Point`, Terraform blocks `resource.aws_instance.web` and attributes `resource.aws_instance.web.ami`, SQL `users.id`, JSON/YAML/TOML/INI/Nix keys `a.b`, Markdown sections by heading `Intro.Install`, HTML/XML elements `div.p`, CSS rules by selector `.card:hover`. A bare name works when unique; an ambiguous one errors and lists candidates as `path:line`, which addresses one of them (`A.x:6` for a property setter). A symbol includes its decorators, attributes, `export` and doc comments. `get` returns `text` dedented to column 0, so it can be edited and passed back to `replace`; `bytes` is the exact range in the file.

## Edit one symbol

```bash
codeedit replace    src/app.py Greeter.greet --body -c 'return f"hi {self.name}"'
codeedit replace    src/app.py helper --code-file new_helper.py     # whole definition
codeedit insert     src/app.py Greeter.greet --position after -c 'def bye(self):
    return "bye"'
codeedit delete     src/app.py old_helper
codeedit add-import src/app.py 'from pathlib import Path'           # no-op if present
```

Positions: `before`/`after` the symbol, `start`/`end` of its body (`start` goes after a Python docstring). Write code unindented; it is reindented to fit, except lines inside multi-line strings and heredocs. A Python one-line body (`def f(): return 1`) becomes a block. An arrow function with an expression body only supports `replace --body`. `delete` removes one name (and its value) from `var x, y = 1, 2`; `replace` of such a name is refused. On a JSON or YAML key, `replace --body` replaces the value, and deleting the last JSON key also removes the comma before it.

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

Use `context` + `selector` when the pattern alone does not parse as the node you mean; the error says so. The context must contain the pattern text, and `selector` picks the enclosing node of that type. Rust match arms (`Command::Find { $$$A } => $B`) and impl items parse without it. A function pattern needs a block body: `fn $N($$$P) { $$$B }`, not `fn $N($$$P) $B`; `$$$F` may stand in for struct fields (`struct $S { $$$F }`). `where` names must be metavariables of the pattern; `kind` accepts supertypes like `expression`.

## Raw tree-sitter queries and byte edits

```bash
codeedit query src -q '(call_expression function: (identifier) @f (#eq? @f "foo"))'
codeedit rewrite-query src -q '(call function: (identifier) @f (#eq? @f "foo"))' -r 'f=bar'
echo '[{"file": "a.py", "edits": [{"bytes": [10, 15], "expect": "hello", "replace": "world"}]}]' | codeedit apply -
```

Get byte ranges from `outline`, `get`, `find` or `parse`, and pass `expect` so a stale range is rejected. `rewrite-query` templates take the capture's indentation like pattern templates; repeated captures (`(decorator)+ @dec`) each use their own text.

## Common mistakes

- In zsh, `echo '...\n...'` turns `\n` into a real newline and breaks JSON. Use a heredoc or `printf '%s'`.
- `rename` renames every identifier with that spelling, including unrelated ones. Preview with `-n`.
- Write `\$FOO` to match a literal PHP or SCSS variable. In PHP a metavariable cannot stand in for a variable name: `foo($A)` works, `$X = 1` does not parse.
- A CSS or SCSS declaration pattern ends with its semicolon (`color: $V;`), and the match includes it.
- A metavariable inside a single token binds that part of the token: `RUN apk add $PKGS` in a Dockerfile, `FROM $IMAGE:$TAG AS $STAGE`, `COPY --from=$STAGE $SRC $DST`, or `log("user $NAME")` matching the string content. The `$` must start a word (`a$B` is literal). Dockerfile `EXPOSE` and `USER` take literals only.
- A metavariable cannot stand in for a TOML or INI value (`name = $V` does not parse); spell the value out, or use `query` with `(pair (bare_key) @k (_) @v)`.
- In Elixir, `@doc`, `@spec`, `@impl`, `@deprecated`, `@since` and `@tag` belong to the function below them; every other attribute stays with the module.
- A Haskell function with several equations has no single body, so `replace` the whole symbol; `insert --position end` adds a declaration to a class or instance.
- `add-import` works in Python, Rust, Go, JS/TS, Zig, Ruby, Crystal, PHP, C, C++, Java, Kotlin, Swift, C#, Scala, Dart, Lua, Haskell, OCaml, Protobuf, CSS and SCSS; other languages report it as unsupported.
- Without `-l`, a directory walk skips files whose language the pattern does not parse in; it errors only if no language fits.
- Patterns see inside Rust macro calls whose arguments parse as expressions or statements (`assert!`, `vec!`, `format!`); other macro bodies are skipped.
- Existing syntax errors are tolerated; an edit is rejected only when it adds an error somewhere new. `--allow-errors` forces such an edit through.
