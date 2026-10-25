from pathlib import Path

import pytest

import codeedit

SOURCE = "import os\n\n\nclass Greeter:\n    def greet(self, name):\n        return name\n\n\ndef main():\n    Greeter().greet(\"x\")\n"


@pytest.fixture
def module(tmp_path: Path) -> Path:
    path = tmp_path / "m.py"
    path.write_text(SOURCE)
    return path


def test_outline_and_get(module: Path) -> None:
    assert [(symbol["path"], symbol["kind"]) for symbol in codeedit.outline(module)] == [
        ("Greeter", "class"),
        ("Greeter.greet", "method"),
        ("main", "function"),
    ]
    found = codeedit.get(module, "greet")
    assert found["text"] == "def greet(self, name):\n    return name"
    assert found["signature"] == "def greet(self, name)"


def test_single_operations(module: Path) -> None:
    codeedit.replace_body(module, "Greeter.greet", "return name.upper()")
    codeedit.insert(module, "Greeter", "def wave(self):\n    pass", "end")
    codeedit.add_import(module, "import sys")
    codeedit.delete_symbol(module, "main")
    assert module.read_text() == (
        "import os\nimport sys\n\n\nclass Greeter:\n    def greet(self, name):\n        return name.upper()\n"
        "\n    def wave(self):\n        pass\n"
    )


def test_replace_symbol_dry_run(module: Path) -> None:
    change = codeedit.replace_symbol(module, "main", "def main():\n    pass\n", dry_run=True)
    assert "+    pass" in change["diff"]
    assert module.read_text() == SOURCE


def test_batch_edit_is_atomic(module: Path) -> None:
    with pytest.raises(ValueError, match="no symbol"):
        codeedit.edit(
            [
                {"op": "add_import", "file": module, "import": "import sys"},
                {"op": "delete", "file": module, "symbol": "missing"},
            ]
        )
    assert module.read_text() == SOURCE


def test_rename_and_source_level(module: Path) -> None:
    [change] = codeedit.rename(module.parent, "Greeter", "Welcomer")
    assert change["edits"] == 2
    result = codeedit.edit_source(SOURCE, "python", [{"op": "replace_body", "symbol": "main", "code": "pass"}])
    assert result["source"].endswith("def main():\n    pass\n")
    assert [symbol["path"] for symbol in codeedit.outline_source("fn a() {}", "rust")] == ["a"]

def test_single_operations_take_a_language(tmp_path: Path) -> None:
    script = tmp_path / "tool"
    script.write_text("def f():\n    return 1\n")
    codeedit.replace_body(script, "f", "return 2", language="python")
    codeedit.add_import(script, "import os", language="python")
    assert codeedit.add_import(script, "import os", language="python") == {"file": str(script), "diff": "", "edits": 0, "errors_before": 0, "errors_after": 0}
    assert script.read_text() == "import os\n\n\ndef f():\n    return 2\n"
