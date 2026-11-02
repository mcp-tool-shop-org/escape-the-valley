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
CONTENT_LEGACY_DIR = REPO_ROOT / "rust" / "oracle" / "content-legacy"
EXPECTED_PIN_SHA = "87104824d1f5b8bfda5b0dc5c14bc5a8e3721407"
EXPECTED_GENERATOR_SHA256 = "b632c11aadbdbaa110c99c0b4062fcfb32f2daa82d22b8c3d129276fd0243639"
EXPECTED_CONTENT_LEGACY_MANIFEST_SHA256 = (
    "f84f98ea845bbfc9a0e0198ba9a29011104a2ed410d40867bb38a82dab342b9f"
)
EXPECTED_CONTENT_LEGACY_EVENT_SKELETONS_SHA256 = (
    "3402c6e1a1508fd55ecc60e2bfe819f60c219638f10d10e2881ca2d237a44576"
)
EXPECTED_CONTENT_LEGACY_FILES_SHA256: dict[str, str] = {
    "content_manifest.json": EXPECTED_CONTENT_LEGACY_MANIFEST_SHA256,
    "event_skeletons.json": EXPECTED_CONTENT_LEGACY_EVENT_SKELETONS_SHA256,
}
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
    "rng_s1_vectors.json.gz": "b4cec10f901eb4febb930d273d825bffc4fee512d29cf1aacb73996e6b4c698b",
    "worldgen_200.json.gz": "c7a51b2c06b84172391f85beaa519d75074475946eb51d86db534c3aac48b768",
    "saves_20.json.gz": "fe3f882d815a4c3a1aaa9d5185a999de12ac62f63b246d767b5efa8b4b47e1dc",
    "turns_1000_seeds.json.gz": "89abab8e3829f4bbe2d4ce49ebd08908aea008d159dc82a733c5b988d229a47c",
    "turns_long_seeds.json.gz": "df8ce43cd842fec6a48908a00a45b405c64cdb9ba9b8ac782d1081b3e0e9a865",
    "resume_120_seeds.json.gz": "6af98bd0175d8bdf95e59d2151629b59d00c0fc7aa89e0b8dd50f47fe63ea112",
    "ledger_reference.json.gz": "688cdc9491e03f5d44a50d792c836a1ed05cff99fede4b728e844716ad9bc743",
    "notices_reference.json.gz": "4d75fa318f29677255578b547d051b0984048b500e4d9dab2b221b05e133a242",
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

    for rel_name, expected_sha in EXPECTED_CONTENT_LEGACY_FILES_SHA256.items():
        snap_path = CONTENT_LEGACY_DIR / rel_name
        assert snap_path.exists(), f"missing {snap_path}"
        assert hashlib.sha256(snap_path.read_bytes()).hexdigest() == expected_sha

    meta = mod.get_oracle_metadata(REPO_ROOT)
    assert meta["oracle_commit"] == EXPECTED_PIN_SHA
    assert meta["generator_sha256"] == EXPECTED_GENERATOR_SHA256
    assert meta["content_legacy_manifest_sha256"] == EXPECTED_CONTENT_LEGACY_MANIFEST_SHA256
    assert (
        meta["content_legacy_event_skeletons_sha256"]
        == EXPECTED_CONTENT_LEGACY_EVENT_SKELETONS_SHA256
    )
    assert meta["content_legacy_files_sha256"] == EXPECTED_CONTENT_LEGACY_FILES_SHA256


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
        assert meta["content_legacy_manifest_sha256"] == EXPECTED_CONTENT_LEGACY_MANIFEST_SHA256
        assert (
            meta["content_legacy_event_skeletons_sha256"]
            == EXPECTED_CONTENT_LEGACY_EVENT_SKELETONS_SHA256
        )
        assert meta["content_legacy_files_sha256"] == EXPECTED_CONTENT_LEGACY_FILES_SHA256
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


def test_generator_refuses_when_content_legacy_snapshot_differs_from_pinned_sha() -> None:
    """`get_oracle_metadata()` must refuse to run if any file in `rust/oracle/content-legacy`
    differs from its pinned SHA-256."""
    mod = _load_generator_module()
    for rel_name in EXPECTED_CONTENT_LEGACY_FILES_SHA256:
        snap_path = CONTENT_LEGACY_DIR / rel_name
        orig_bytes = snap_path.read_bytes()
        try:
            snap_path.write_bytes(orig_bytes + b"\n")
            with pytest.raises(
                RuntimeError,
                match=rf"Refusing to generate oracle vectors: legacy content snapshot {rel_name}",
            ):
                mod.get_oracle_metadata(REPO_ROOT)
        finally:
            snap_path.write_bytes(orig_bytes)

    meta = mod.get_oracle_metadata(REPO_ROOT)
    assert meta["content_legacy_files_sha256"] == EXPECTED_CONTENT_LEGACY_FILES_SHA256
