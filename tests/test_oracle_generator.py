"""Tests for `rust/oracle/generate_vectors.py` pin enforcement."""

from __future__ import annotations

import gzip
import hashlib
import importlib.util
import json
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
GENERATOR_PATH = REPO_ROOT / "rust" / "oracle" / "generate_vectors.py"
VECTORS_DIR = REPO_ROOT / "rust" / "oracle" / "vectors"
EXPECTED_PIN_SHA = "87104824d1f5b8bfda5b0dc5c14bc5a8e3721407"
EXPECTED_GENERATOR_SHA256 = "3917d0889f28f949f63c07f832c8cab9c04593dc0a49009f1dc3b17be19627a8"
EXPECTED_VECTOR_CONTENT_SHA256: dict[str, str] = {
    "rng_s1_vectors.json.gz": "1f891b7b21ac17c33c9a88d3e01091a58c3c0a5b38d014a58ea82d5842586605",
    "worldgen_200.json.gz": "179944a803e73791d27b8831cc1b9f20eebb3584a02cd0b40068f240e7804003",
    "saves_20.json.gz": "cd5f10b46b5018ed3722b5c184087196549036223167e1ee3bc045476fcbfafa",
    "turns_1000_seeds.json.gz": "ce7feff96f7e7c22dd4a9925137c1fd10571afebd1bf9d50e1022c24fa375722",
    "turns_long_seeds.json.gz": "4c912f31e750ec3588db843ff5a54a593375e072033ab51ccec9cea16390237f",
    "resume_120_seeds.json.gz": "76fa1228de387ecaf689e08725f603bb7e95cf4253a5623099207a45801b8caf",
    "ledger_reference.json.gz": "bcbc4a6b30aed16fddbb66f301f1498b6a69e01424dd64cdacf9c7891773d964",
    "notices_reference.json.gz": "c56c3e2ee26d26dc3929624a36a81146face8546c605a85a7037dd1a8fcc59ee",
}
EXPECTED_GZIP_SHA256: dict[str, str] = {
    "rng_s1_vectors.json.gz": "0c4204946dac928343569b324742e1944605fc6cecae3ac5b9ba739ce470b7cc",
    "worldgen_200.json.gz": "125802c245e785acdcc19764eb313c4c47192072f965db4c4bdddc0f317ce527",
    "saves_20.json.gz": "5d2136bd93d2d5f7e9500b01247e98a0fa1a73251a022e10c8cab1607007cfab",
    "turns_1000_seeds.json.gz": "146a87f029ec6993d5848ef1f04cc1e47391a6b871bd31b3bb164bc613d0ff0a",
    "turns_long_seeds.json.gz": "48fa1e5575ea1c34a0d4351de971e7cf9df47f00e17572e813f481cc00480f30",
    "resume_120_seeds.json.gz": "5691dd6fff8d6730a0dff2b75b52ae222d156117bc0a7d12eec568f8b74ecbbc",
    "ledger_reference.json.gz": "ffe4b12dabeed12ec751469bb642bef353f5305c8da0d30d64d08d30756fd766",
    "notices_reference.json.gz": "be8eec30247e4a9cd95dc4f5b40ea1e7776bbdb6a86634b0b786f835b21075ca",
}


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
    assert meta["generator_sha256"] == EXPECTED_GENERATOR_SHA256


def test_vector_headers_record_generator_and_vector_sha256() -> None:
    mod = _load_generator_module()
    for filename, expected_gz_sha in EXPECTED_GZIP_SHA256.items():
        path = VECTORS_DIR / filename
        assert path.exists(), f"missing vector file {filename}"
        actual_gz_sha = hashlib.sha256(path.read_bytes()).hexdigest()
        assert actual_gz_sha == expected_gz_sha
        with gzip.open(path, "rt", encoding="utf-8") as f:
            payload = json.load(f)
        meta = payload["metadata"]
        assert meta["oracle_commit"] == EXPECTED_PIN_SHA
        assert meta["generator_sha256"] == EXPECTED_GENERATOR_SHA256
        assert meta["vector_sha256"] == EXPECTED_VECTOR_CONTENT_SHA256[filename]
        assert meta["vector_files_sha256"] == EXPECTED_VECTOR_CONTENT_SHA256
        assert mod.compute_vector_content_sha256(payload) == meta["vector_sha256"]


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
