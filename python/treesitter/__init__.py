import json
from collections.abc import Iterable, Mapping, Sequence
from os import PathLike, fspath
from typing import Any

from treesitter import native
from treesitter.native import languages, render, render_file

__all__ = [
    "add_import",
    "apply",
    "apply_source",
    "delete_symbol",
    "edit",
    "edit_source",
    "find",
    "find_source",
    "get",
    "insert",
    "languages",
    "outline",
    "outline_source",
    "parse",
    "parse_file",
    "query",
    "query_source",
    "render",
    "rename",
    "render_file",
    "replace_body",
    "replace_symbol",
    "rewrite",
    "rewrite_query",
    "rewrite_source",
]

type Path = str | PathLike[str]
type Rules = Sequence[Mapping[str, Any]]
type Operations = Sequence[Mapping[str, Any]]


def encode(source: str | bytes) -> bytes:
    return source.encode() if isinstance(source, str) else source


def paths(targets: Path | Iterable[Path]) -> list[str]:
    if isinstance(targets, (str, PathLike)):
        return [fspath(targets)]
    return [fspath(target) for target in targets]


def parse(source: str | bytes, language: str) -> dict[str, Any]:
    return json.loads(render(encode(source), language))


def parse_file(path: Path, language: str | None = None) -> dict[str, Any]:
    return json.loads(render_file(path, language))


def query(targets: Path | Iterable[Path], pattern: str, language: str | None = None) -> list[dict[str, Any]]:
    return json.loads(native.query(paths(targets), pattern, language))


def query_source(source: str | bytes, language: str, pattern: str) -> list[dict[str, Any]]:
    return json.loads(native.query_source(encode(source), language, pattern))


def rewrite_query(
    targets: Path | Iterable[Path],
    pattern: str,
    replacements: Mapping[str, str],
    language: str | None = None,
    dry_run: bool = False,
    allow_errors: bool = False,
) -> list[dict[str, Any]]:
    return json.loads(native.rewrite_query(paths(targets), pattern, dict(replacements), language, dry_run, allow_errors))


def apply(requests: Sequence[Mapping[str, Any]], dry_run: bool = False, allow_errors: bool = False) -> list[dict[str, Any]]:
    serializable = [{**request, "file": fspath(request["file"])} for request in requests]
    return json.loads(native.apply(json.dumps(serializable), dry_run, allow_errors))


def apply_source(
    source: str, language: str, edits: Sequence[Mapping[str, Any]], allow_errors: bool = False
) -> dict[str, Any]:
    return json.loads(native.apply_source(source, language, json.dumps(list(edits)), allow_errors))


def rule_list(
    pattern: str | None,
    rewrite: str | None,
    where: Mapping[str, Mapping[str, str]] | None,
    rules: Rules | None,
) -> str:
    if rules is not None:
        return json.dumps(list(rules))
    if pattern is None:
        raise ValueError("pass a pattern or rules")
    single: dict[str, Any] = {"pattern": pattern}
    if rewrite is not None:
        single["rewrite"] = rewrite
    if where:
        single["where"] = dict(where)
    return json.dumps([single])


def find(
    targets: Path | Iterable[Path],
    pattern: str | None = None,
    where: Mapping[str, Mapping[str, str]] | None = None,
    rules: Rules | None = None,
    language: str | None = None,
) -> list[dict[str, Any]]:
    return json.loads(native.find(paths(targets), rule_list(pattern, None, where, rules), language))


def find_source(
    source: str | bytes,
    language: str,
    pattern: str | None = None,
    where: Mapping[str, Mapping[str, str]] | None = None,
    rules: Rules | None = None,
) -> list[dict[str, Any]]:
    return json.loads(native.find_source(encode(source), language, rule_list(pattern, None, where, rules)))


def rewrite(
    targets: Path | Iterable[Path],
    pattern: str | None = None,
    rewrite: str | None = None,
    where: Mapping[str, Mapping[str, str]] | None = None,
    rules: Rules | None = None,
    language: str | None = None,
    dry_run: bool = False,
    allow_errors: bool = False,
) -> list[dict[str, Any]]:
    encoded = rule_list(pattern, rewrite, where, rules)
    return json.loads(native.rewrite(paths(targets), encoded, language, dry_run, allow_errors))


def rewrite_source(
    source: str,
    language: str,
    pattern: str | None = None,
    rewrite: str | None = None,
    where: Mapping[str, Mapping[str, str]] | None = None,
    rules: Rules | None = None,
    allow_errors: bool = False,
) -> dict[str, Any]:
    encoded = rule_list(pattern, rewrite, where, rules)
    return json.loads(native.rewrite_source(source, language, encoded, allow_errors))


def outline(path: Path, language: str | None = None) -> list[dict[str, Any]]:
    return json.loads(native.outline(path, language))


def outline_source(source: str | bytes, language: str) -> list[dict[str, Any]]:
    return json.loads(native.outline_source(encode(source), language))


def get(path: Path, symbol: str, language: str | None = None) -> dict[str, Any]:
    return json.loads(native.get(path, symbol, language))


def edit(operations: Operations, dry_run: bool = False, allow_errors: bool = False) -> list[dict[str, Any]]:
    serializable = [{**operation, "file": fspath(operation["file"])} for operation in operations]
    return json.loads(native.edit(json.dumps(serializable), dry_run, allow_errors))


def edit_source(source: str, language: str, operations: Operations, allow_errors: bool = False) -> dict[str, Any]:
    return json.loads(native.edit_source(source, language, json.dumps(list(operations)), allow_errors))


def single(path: Path, operation: dict[str, Any], dry_run: bool, allow_errors: bool) -> dict[str, Any]:
    [change] = edit([{**operation, "file": path}], dry_run, allow_errors)
    return change


def replace_symbol(path: Path, symbol: str, code: str, dry_run: bool = False, allow_errors: bool = False) -> dict[str, Any]:
    return single(path, {"op": "replace", "symbol": symbol, "code": code}, dry_run, allow_errors)


def replace_body(path: Path, symbol: str, code: str, dry_run: bool = False, allow_errors: bool = False) -> dict[str, Any]:
    return single(path, {"op": "replace_body", "symbol": symbol, "code": code}, dry_run, allow_errors)


def insert(
    path: Path, symbol: str, code: str, position: str, dry_run: bool = False, allow_errors: bool = False
) -> dict[str, Any]:
    return single(path, {"op": "insert", "symbol": symbol, "code": code, "position": position}, dry_run, allow_errors)


def delete_symbol(path: Path, symbol: str, dry_run: bool = False, allow_errors: bool = False) -> dict[str, Any]:
    return single(path, {"op": "delete", "symbol": symbol}, dry_run, allow_errors)


def add_import(path: Path, import_: str, dry_run: bool = False, allow_errors: bool = False) -> dict[str, Any]:
    return single(path, {"op": "add_import", "import": import_}, dry_run, allow_errors)


def rename(
    targets: Path | Iterable[Path],
    old: str,
    new: str,
    language: str | None = None,
    dry_run: bool = False,
    allow_errors: bool = False,
) -> list[dict[str, Any]]:
    return json.loads(native.rename(paths(targets), old, new, language, dry_run, allow_errors))
