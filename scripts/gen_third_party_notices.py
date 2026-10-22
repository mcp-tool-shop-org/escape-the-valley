#!/usr/bin/env python3
"""Generate THIRD_PARTY_NOTICES.txt from the active Python environment (issue #28).

Usage:
    python scripts/gen_third_party_notices.py [output_path]
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

from escape_the_valley.notices import write_third_party_notices  # noqa: E402


def main() -> int:
    out = sys.argv[1] if len(sys.argv) > 1 else "THIRD_PARTY_NOTICES.txt"
    path = write_third_party_notices(out)
    print(f"Wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
