#!/usr/bin/env python3
"""Post-build smoke test for the PyInstaller-frozen release binary.

Added for F-a4d8291c / F-a6fa29e0 (dogfood-swarm wave 2, ci-tooling domain).

release-binaries.yml previously went straight from `pyinstaller` to
`upload-artifact` with nothing that actually *ran* the binary it just built.
That let a 100%-reproducible startup crash (a package-relative import in
__main__.py -- fixed alongside this script) ship undetected through the
v1.1.0 and v1.1.1 GitHub Releases: the build step succeeded, the artifact
uploaded, and every status check went green.

A `--help`-only smoke test would not have caught the other half of that same
finding: PyInstaller was never told to bundle
`escape_the_valley/data/event_skeletons.json` (~200 of the game's ~260
events) or `escape_the_valley/tui.tcss` (the Textual stylesheet the `tui`
subcommand needs to start). A stylesheet-only fix ships a binary that starts
cleanly and silently runs a quarter-game -- so this script asserts the
*loaded* event count via __main__.py's ESCAPE_THE_VALLEY_SMOKE_EVENT_COUNT
hook, not just the process exit code of an argument-parsing path that never
touches game content.

Usage:
    python scripts/smoke_test_binary.py <path-to-binary>

Exits 0 on success, 1 on any smoke-test failure, 2 on usage error.
"""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

# 260 events exist today: 60 hardcoded in events.py + ~200 loaded from
# data/event_skeletons.json (build_event_library() combines both). This
# floor sits well above the 60-event hardcoded-only failure mode (missing or
# unbundled JSON) but below 260 so ordinary content growth/trimming of the
# JSON library doesn't require editing this script every time.
MIN_EXPECTED_EVENTS = 200

TIMEOUT_SECONDS = 60


def _run(binary: str, *args: str, env: dict | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [binary, *args],
        capture_output=True,
        text=True,
        timeout=TIMEOUT_SECONDS,
        env=env,
    )


def _fail(message: str, result: subprocess.CompletedProcess | None = None) -> int:
    print(f"SMOKE TEST FAILED: {message}", file=sys.stderr)
    if result is not None:
        print(f"--- exit code: {result.returncode} ---", file=sys.stderr)
        print("--- stdout ---", file=sys.stderr)
        print(result.stdout, file=sys.stderr)
        print("--- stderr ---", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: smoke_test_binary.py <path-to-binary>", file=sys.stderr)
        return 2

    binary = str(Path(sys.argv[1]).resolve())
    if not Path(binary).exists():
        print(f"SMOKE TEST FAILED: binary not found at {binary}", file=sys.stderr)
        return 1

    # 1. Does the binary even start? This alone would have caught F-a4d8291c's
    #    relative-import crash (ImportError on every invocation, including
    #    --help).
    print(f"[smoke] running: {binary} --help")
    result = _run(binary, "--help")
    if result.returncode != 0:
        return _fail(f"`--help` exited {result.returncode}", result)
    print("[smoke] --help OK")

    # 2. Does the event library actually load its full content? This is the
    #    check a startup-only smoke test would miss (F-a4d8291c part 2): a
    #    binary missing event_skeletons.json starts fine and answers --help
    #    fine, then silently plays a quarter-game.
    print("[smoke] running event-count self-test "
          "(ESCAPE_THE_VALLEY_SMOKE_EVENT_COUNT=1)")
    env = dict(os.environ)
    env["ESCAPE_THE_VALLEY_SMOKE_EVENT_COUNT"] = "1"
    result = _run(binary, env=env)
    if result.returncode != 0:
        return _fail(f"event-count self-test exited {result.returncode}", result)

    try:
        count = int(result.stdout.strip())
    except ValueError:
        return _fail(
            f"could not parse event count from stdout: {result.stdout!r}", result
        )

    print(f"[smoke] event library reports {count} events "
          f"(floor: {MIN_EXPECTED_EVENTS})")
    if count < MIN_EXPECTED_EVENTS:
        return _fail(
            f"only {count} events loaded (< {MIN_EXPECTED_EVENTS}). This is "
            "the quarter-game failure mode -- data/event_skeletons.json is "
            "likely missing from the bundle (check --add-data in "
            "release-binaries.yml's 'Build binary' step)."
        )

    print("[smoke] PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
