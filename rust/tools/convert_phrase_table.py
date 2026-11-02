#!/usr/bin/env python3
"""Convert the approved fallback phrase table into etv-core's fallback_phrases.json format.

Verifies the SHA-256 of the source file before reading, preserves all phrases,
none_phrase, subject, and key_order byte-for-byte without editing any wording,
and maps:
  "+" -> "pos"
  "-" -> "neg"
  none_phrase -> phrases["none"]["neutral"]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

EXPECTED_SOURCE_SHA256 = (
    "ef29eecf13ba985708d21ad9f002ab4a2de5af4aa70dc914f8c1135f4bf12a22"
)
DEFAULT_SOURCE_PATH = Path(
    "E:/AI/rnd-local/projects/etv/phrase-table/PHRASE-TABLE-DRAFT-2-2026-11-02.json"
)
DEFAULT_OUTPUT_PATH = (
    Path(__file__).resolve().parent.parent / "etv-core" / "content" / "fallback_phrases.json"
)


def convert_source_bytes(raw_bytes: bytes, verify_sha: bool = True) -> dict:
    actual_sha = hashlib.sha256(raw_bytes).hexdigest()
    if verify_sha and actual_sha != EXPECTED_SOURCE_SHA256:
        raise ValueError(
            f"source SHA-256 mismatch: expected {EXPECTED_SOURCE_SHA256}, got {actual_sha}"
        )

    src = json.loads(raw_bytes.decode("utf-8"))
    subject = src["subject"]
    none_phrase = src["none_phrase"]
    raw_phrases = src["phrases"]

    key_order: list[str] = []
    phrases_map: dict[str, dict[str, str]] = {}

    for entry in raw_phrases:
        key = entry["key"]
        raw_dir = entry["dir"]
        phrase = entry["phrase"]
        if key not in key_order:
            key_order.append(key)
            phrases_map[key] = {}
        if raw_dir == "+":
            mapped_dir = "pos"
        elif raw_dir == "-":
            mapped_dir = "neg"
        else:
            raise ValueError(f"unexpected dir {raw_dir!r} for key {key!r}")
        phrases_map[key][mapped_dir] = phrase

    phrases_map["none"] = {"neutral": none_phrase}

    ordered_phrases: dict[str, dict[str, str]] = {}
    for k in sorted(phrases_map.keys()):
        inner = phrases_map[k]
        ordered_phrases[k] = {dk: inner[dk] for dk in sorted(inner.keys())}

    return {
        "schema_version": 1,
        "subject": subject,
        "key_order": key_order,
        "phrases": ordered_phrases,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=Path,
        default=DEFAULT_SOURCE_PATH,
        help="Path to source phrase table JSON",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT_PATH,
        help="Path to write converted fallback_phrases.json",
    )
    args = parser.parse_args(argv)

    raw_bytes = args.source.read_bytes()
    converted = convert_source_bytes(raw_bytes, verify_sha=True)
    rendered = json.dumps(converted, indent=2, ensure_ascii=False) + "\n"
    args.output.write_bytes(rendered.encode("utf-8"))
    out_sha = hashlib.sha256(rendered.encode("utf-8")).hexdigest()
    print(f"source_sha256={hashlib.sha256(raw_bytes).hexdigest()}")
    print(f"output_sha256={out_sha}")
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
