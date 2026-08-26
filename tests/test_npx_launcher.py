"""npx launcher pin must match the shipped version (F-05786299)."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _package_version() -> str:
    return json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]


def _init_version() -> str:
    text = (ROOT / "src" / "escape_the_valley" / "__init__.py").read_text(encoding="utf-8")
    match = re.search(r'__version__ = "([^"]+)"', text)
    assert match, "missing __version__"
    return match.group(1)


def _pyproject_version() -> str:
    text = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
    match = re.search(r'(?m)^version = "([^"]+)"', text)
    assert match, "missing pyproject version"
    return match.group(1)


def test_python_and_npm_versions_match():
    assert _package_version() == _init_version() == _pyproject_version()


def test_npx_launcher_pins_current_github_release():
    """Wrapper must not keep downloading a dead GitHub tag."""
    ver = _package_version()
    js = (ROOT / "bin" / "escape-the-valley.js").read_text(encoding="utf-8")
    assert f'version: "{ver}"' in js
    assert f'tag: "v{ver}"' in js
    assert 'version: "1.1.1"' not in js
