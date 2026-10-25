from pathlib import Path

import pytest

import codeedit

SAMPLE = 'class Greeter:\n    def greet(self, name: str) -> str:\n        return f"hi {name}"\n'


def test_languages() -> None:
    assert {"python", "rust", "go", "typescript", "tsx", "javascript", "zig", "bash", "json", "xml", "yaml", "markdown", "html"} <= set(codeedit.languages())


def test_parse_returns_tree() -> None:
    tree = codeedit.parse(SAMPLE, "python")
    method = tree["children"][0]["children"][1]["children"][0]
    assert tree["type"] == "module"
    assert method["type"] == "function_definition"
    assert method["children"][0] == {"type": "identifier", "field": "name", "start": [1, 8], "end": [1, 13], "bytes": [23, 28], "text": "greet"}


def test_parse_file_detects_language(tmp_path: Path) -> None:
    path = tmp_path / "main.go"
    path.write_text("package main\nfunc main() {}\n")
    assert codeedit.parse_file(path)["type"] == "source_file"


def test_render_formats() -> None:
    assert codeedit.render(SAMPLE.encode(), "python", "sexp").startswith("(module")
    assert codeedit.render(SAMPLE.encode(), "python", "xml").startswith("<?xml")


def test_errors() -> None:
    with pytest.raises(ValueError, match="unknown language"):
        codeedit.parse("x", "cobol")
    with pytest.raises(ValueError, match="cannot detect language"):
        codeedit.parse_file("README")
    with pytest.raises(OSError):
        codeedit.parse_file("missing.py")
