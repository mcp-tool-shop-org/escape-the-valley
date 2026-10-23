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
    verify_pbs_archive_accounting,
    verify_toc_against_notices,
    write_third_party_notices,
)


def main(argv: list[str] | None = None) -> int:
    raw_args = list(sys.argv[1:] if argv is None else argv)
    release = False
    target: str | None = None
    archive_path: str | None = None
    verify_archive_sha256 = True
    args: list[str] = []
    i = 0
    while i < len(raw_args):
        a = raw_args[i]
        if a == "--release":
            release = True
        elif a == "--target" and i + 1 < len(raw_args):
            target = raw_args[i + 1]
            i += 1
        elif a == "--archive-path" and i + 1 < len(raw_args):
            archive_path = raw_args[i + 1]
            i += 1
        elif a == "--no-verify-archive-sha256":
            verify_archive_sha256 = False
        else:
            args.append(a)
        i += 1

    if args and args[0] == "--verify-pbs-archive":
        try:
            verified = verify_pbs_archive_accounting(
                target=target,
                archive_path=archive_path,
                verify_archive_sha256=verify_archive_sha256,
            )
        except Exception as exc:
            print(f"PBS ARCHIVE VERIFICATION FAILED: {exc}", file=sys.stderr)
            return 1
        print(
            f"[pbs-archive-check] Verified vendored _licenses/ and SOURCES.json "
            f"against python-build-standalone archive for: {', '.join(verified)}"
        )
        return 0

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
            prune_kwargs: dict = {"release": release}
            if target is not None:
                prune_kwargs["target"] = target
            bundled = prune_notices_to_toc(
                build_dir, notices_path, **prune_kwargs
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
    write_kwargs: dict = {"release": release}
    if target is not None:
        write_kwargs["target"] = target
    path = write_third_party_notices(out, **write_kwargs)
    print(f"Wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
