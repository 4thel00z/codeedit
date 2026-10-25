from pathlib import Path

import pytest

import codeedit

PYTHON = "def foo(x):\n    return foo(x) + bar(foo)\n"
CALL_FOO = '(call function: (identifier) @f (#eq? @f "foo"))'


def test_query_source() -> None:
    [found] = codeedit.query_source(PYTHON, "python", CALL_FOO)
    assert found["captures"][0] == {"name": "f", "type": "identifier", "start": [1, 11], "end": [1, 14], "bytes": [23, 26], "text": "foo"}


def test_query_directory(tmp_path: Path) -> None:
    (tmp_path / "a.py").write_text(PYTHON)
    (tmp_path / "b.py").write_text("x = 1\n")
    [found] = codeedit.query(tmp_path, CALL_FOO)
    assert Path(found["file"]).name == "a.py"
    assert found["language"] == "python"


def test_rewrite_query(tmp_path: Path) -> None:
    path = tmp_path / "a.py"
    path.write_text(PYTHON)
    [preview] = codeedit.rewrite_query(path, CALL_FOO, {"f": "renamed_@f"}, dry_run=True)
    assert "+    return renamed_foo(x) + bar(foo)" in preview["diff"]
    assert path.read_text() == PYTHON
    codeedit.rewrite_query([path], CALL_FOO, {"@f": "renamed_@f"})
    assert path.read_text() == "def foo(x):\n    return renamed_foo(x) + bar(foo)\n"


def test_apply(tmp_path: Path) -> None:
    path = tmp_path / "a.py"
    path.write_text(PYTHON)
    [change] = codeedit.apply([{"file": path, "edits": [{"bytes": [4, 7], "expect": "foo", "replace": "baz"}]}])
    assert change["edits"] == 1
    assert path.read_text().startswith("def baz(x):")


def test_apply_source_and_rejections() -> None:
    change = codeedit.apply_source(PYTHON, "python", [{"bytes": [4, 7], "replace": "baz"}])
    assert change["source"].startswith("def baz(x):")
    with pytest.raises(ValueError, match="stale edit"):
        codeedit.apply_source(PYTHON, "python", [{"bytes": [4, 7], "expect": "nope", "replace": "baz"}])
    with pytest.raises(ValueError, match="introduces syntax errors"):
        codeedit.apply_source(PYTHON, "python", [{"bytes": [7, 8], "replace": ""}])
    with pytest.raises(ValueError, match="invalid query"):
        codeedit.query_source(PYTHON, "python", '(identifier) @i (#eq? @i "x")')
