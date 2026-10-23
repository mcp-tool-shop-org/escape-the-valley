#!/usr/bin/env python3
"""Generate THIRD_PARTY_NOTICES.txt and prune/verify against PyInstaller TOC/bundle (issue #28).

Usage:
    python scripts/gen_third_party_notices.py [--release] [output_path]
    python scripts/gen_third_party_notices.py [--release] --prune-toc <build_dir> [notices_path]
    python scripts/gen_third_party_notices.py --verify-toc <build_dir> [notices_path]
    python scripts/gen_third_party_notices.py --verify-bundle <binary_path>
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

from escape_the_valley.notices import (  # noqa: E402
    prune_notices_to_toc,
    verify_bundle_no_direct_url,
    verify_toc_against_notices,
    write_third_party_notices,
)


def main(argv: list[str] | None = None) -> int:
    raw_args = list(sys.argv[1:] if argv is None else argv)
    release = False
    args: list[str] = []
    for a in raw_args:
        if a == "--release":
            release = True
        else:
            args.append(a)

    if args and args[0] == "--prune-toc":
        if len(args) < 2:
            print(
                "usage: gen_third_party_notices.py [--release] "
                "--prune-toc <build_dir> [notices_path]",
                file=sys.stderr,
            )
            return 2
        build_dir = args[1]
        notices_path = args[2] if len(args) > 2 else "THIRD_PARTY_NOTICES.txt"
        try:
            bundled = prune_notices_to_toc(
                build_dir, notices_path, release=True
            )
        except Exception as exc:
            print(f"TOC PRUNE FAILED: {exc}", file=sys.stderr)
            return 1
        out_file = Path(notices_path)
        print(
            f"[toc-prune] Pruned {out_file} ({out_file.stat().st_size} bytes) "
            f"to {len(bundled)} bundled site-packages distributions: "
            f"{', '.join(bundled)}"
        )
        return 0

    if args and args[0] == "--verify-toc":
        if len(args) < 2:
            print(
                "usage: gen_third_party_notices.py --verify-toc <build_dir> [notices_path]",
                file=sys.stderr,
            )
            return 2
        build_dir = args[1]
        notices_path = args[2] if len(args) > 2 else "THIRD_PARTY_NOTICES.txt"
        try:
            bundled = verify_toc_against_notices(build_dir, notices_path)
        except Exception as exc:
            print(f"TOC VERIFICATION FAILED: {exc}", file=sys.stderr)
            return 1
        print(
            f"[toc-check] Verified {len(bundled)} bundled site-packages "
            f"distributions and excluded extensions against {notices_path}: "
            f"{', '.join(bundled)}"
        )
        return 0

    if args and args[0] == "--verify-bundle":
        if len(args) < 2:
            print(
                "usage: gen_third_party_notices.py --verify-bundle <binary_path>",
                file=sys.stderr,
            )
            return 2
        binary_path = args[1]
        try:
            entries = verify_bundle_no_direct_url(binary_path)
        except Exception as exc:
            print(f"BUNDLE VERIFICATION FAILED: {exc}", file=sys.stderr)
            return 1
        print(
            f"[bundle-check] Verified {len(entries)} archive entries in "
            f"{binary_path}: no direct_url.json or _sysconfigdata home paths present"
        )
        return 0

    out = args[0] if args else "THIRD_PARTY_NOTICES.txt"
    path = write_third_party_notices(out, release=release)
    print(f"Wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
