"""Tests for `rust/oracle/generate_vectors.py` pin enforcement."""

from __future__ import annotations

import hashlib
import importlib.util
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
GENERATOR_PATH = REPO_ROOT / "rust" / "oracle" / "generate_vectors.py"
EXPECTED_PIN_SHA = "87104824d1f5b8bfda5b0dc5c14bc5a8e3721407"
EXPECTED_GENERATOR_SHA256 = "faf78ac8874346a496504e2d8b2495ad268231bbd1b7da664a84fd5ef783a4ee"


def _load_generator_module():
    spec = importlib.util.spec_from_file_location("etv_generate_vectors", GENERATOR_PATH)
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_generator_pinned_to_final_legacy_python_sha() -> None:
    mod = _load_generator_module()
    assert mod.PINNED_ORACLE_COMMIT == EXPECTED_PIN_SHA
    actual_sha = hashlib.sha256(GENERATOR_PATH.read_bytes()).hexdigest()
    assert actual_sha == EXPECTED_GENERATOR_SHA256

    meta = mod.get_oracle_metadata(REPO_ROOT)
    assert meta["oracle_commit"] == EXPECTED_PIN_SHA


def test_generator_refuses_when_escape_the_valley_differs_from_pinned_sha() -> None:
    """`get_oracle_metadata()` must refuse to run if `src/escape_the_valley`
    differs from `87104824d1f5b8bfda5b0dc5c14bc5a8e3721407` (either tracked diff
    or untracked file inside `src/escape_the_valley`)."""
    mod = _load_generator_module()
    pkg_init = REPO_ROOT / "src" / "escape_the_valley" / "__init__.py"
    orig_bytes = pkg_init.read_bytes()

    try:
        pkg_init.write_bytes(orig_bytes + b"\n# transient drift probe\n")
        with pytest.raises(
            RuntimeError,
            match=(
                r"Refusing to generate oracle vectors: src/escape_the_valley "
                r"differs from pinned commit " + EXPECTED_PIN_SHA
            ),
        ):
            mod.get_oracle_metadata(REPO_ROOT)
    finally:
        pkg_init.write_bytes(orig_bytes)

    untracked_probe = REPO_ROOT / "src" / "escape_the_valley" / "_untracked_probe.tmp"
    try:
        untracked_probe.write_text("probe\n", encoding="utf-8")
        with pytest.raises(
            RuntimeError,
            match=(
                r"Refusing to generate oracle vectors: dirty or untracked files "
                r"in src/escape_the_valley relative to " + EXPECTED_PIN_SHA
            ),
        ):
            mod.get_oracle_metadata(REPO_ROOT)
    finally:
        untracked_probe.unlink(missing_ok=True)

    # Restored tree succeeds again
    meta = mod.get_oracle_metadata(REPO_ROOT)
    assert meta["oracle_commit"] == EXPECTED_PIN_SHA
