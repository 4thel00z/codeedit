from pathlib import Path

import pytest

import treesitter


def test_find_source_bindings() -> None:
    found = treesitter.find_source("foo(1)\nfoo(a, b)\n", "python", "foo($$$ARGS)")
    assert [each["bindings"]["ARGS"]["text"] for each in found] == ["1", "a, b"]
    assert found[1]["rule"] == 0


def test_find_with_where(tmp_path: Path) -> None:
    (tmp_path / "a.py").write_text("log_info(1)\nprint(2)\n")
    [result] = treesitter.find(tmp_path, "$F($$$A)", where={"F": {"regex": "^log_"}})
    assert [each["text"] for each in result["matches"]] == ["log_info(1)"]


def test_rewrite_source_nested() -> None:
    change = treesitter.rewrite_source("foo(foo(x))\n", "python", "foo($A)", "bar($A)")
    assert change["source"] == "bar(bar(x))\n"


def test_rewrite_files_with_rules(tmp_path: Path) -> None:
    path = tmp_path / "main.rs"
    path.write_text("fn main() { let a = x.unwrap(); let b = y.expect(\"y\"); }\n")
    rules = [
        {"pattern": "$E.unwrap()", "rewrite": "$E?"},
        {"pattern": "$E.expect($MSG)", "rewrite": "$E.context($MSG)?"},
    ]
    [preview] = treesitter.rewrite(path, rules=rules, dry_run=True)
    assert preview["edits"] == 2
    treesitter.rewrite(path, rules=rules)
    assert path.read_text() == "fn main() { let a = x?; let b = y.context(\"y\")?; }\n"


def test_pattern_errors() -> None:
    with pytest.raises(ValueError, match="invalid pattern"):
        treesitter.find_source("x", "python", "def (")
    with pytest.raises(ValueError, match="pass a pattern or rules"):
        treesitter.find_source("x", "python")
