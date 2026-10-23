#!/usr/bin/env python3
"""Generate THIRD_PARTY_NOTICES.txt and prune/verify against PyInstaller TOC (issue #28).

Usage:
    python scripts/gen_third_party_notices.py [output_path]
    python scripts/gen_third_party_notices.py --prune-toc <build_dir> [notices_path]
    python scripts/gen_third_party_notices.py --verify-toc <build_dir> [notices_path]
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

from escape_the_valley.notices import (  # noqa: E402
    prune_notices_to_toc,
    verify_toc_against_notices,
    write_third_party_notices,
)


def main() -> int:
    if len(sys.argv) >= 2 and sys.argv[1] == "--prune-toc":
        if len(sys.argv) < 3:
            print(
                "usage: gen_third_party_notices.py --prune-toc <build_dir> [notices_path]",
                file=sys.stderr,
            )
            return 2
        build_dir = sys.argv[2]
        notices_path = sys.argv[3] if len(sys.argv) > 3 else "THIRD_PARTY_NOTICES.txt"
        try:
            bundled = prune_notices_to_toc(build_dir, notices_path)
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

    if len(sys.argv) >= 2 and sys.argv[1] == "--verify-toc":
        if len(sys.argv) < 3:
            print(
                "usage: gen_third_party_notices.py --verify-toc <build_dir> [notices_path]",
                file=sys.stderr,
            )
            return 2
        build_dir = sys.argv[2]
        notices_path = sys.argv[3] if len(sys.argv) > 3 else "THIRD_PARTY_NOTICES.txt"
        try:
            bundled = verify_toc_against_notices(build_dir, notices_path)
        except Exception as exc:
            print(f"TOC VERIFICATION FAILED: {exc}", file=sys.stderr)
            return 1
        print(
            f"[toc-check] Verified {len(bundled)} bundled site-packages "
            f"distributions against {notices_path}: {', '.join(bundled)}"
        )
        return 0

    out = sys.argv[1] if len(sys.argv) > 1 else "THIRD_PARTY_NOTICES.txt"
    path = write_third_party_notices(out)
    print(f"Wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
