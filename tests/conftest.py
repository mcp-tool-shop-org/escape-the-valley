"""Shared pytest fixtures.

Autosave is CWD-relative. ``save.save_game`` resolves ``.trail/`` against
``Path(".")`` when no ``base_path`` is passed, and every autosave call site
passes none: ``StepEngine.step``, ``GameEngine.step``, the TUI's save action,
and the CLI's mutating subcommands. So any test that steps an engine, drives
the TUI, or invokes the CLI writes a real save file — plus the wallet-secrets
sidecar ``.trail/secrets.json`` — into whatever directory pytest was launched
from, which in practice is the repo root.

``.trail/`` is gitignored, so the mess never shows up in ``git status``. It
just sits in the checkout, ready to mask or clobber a real save the next time
someone plays the game from that directory.

The fix is suite-wide rather than per-test, because the leak is a property of
the default save path rather than of any one test: every test runs with its
CWD inside its own temp directory, and a guard fails the offending test if the
launch directory's ``.trail/`` is created or modified anyway.
"""

from __future__ import annotations

from pathlib import Path

import pytest

# Captured at collection time, before any fixture has moved the process — this
# is the directory pytest was actually launched from (the repo root, normally).
LAUNCH_DIR = Path.cwd()
_LAUNCH_TRAIL = LAUNCH_DIR / ".trail"


def _trail_snapshot() -> dict[str, int] | None:
    """Fingerprint the launch dir's ``.trail/``, or None if it isn't there."""
    if not _LAUNCH_TRAIL.exists():
        return None
    return {
        str(p.relative_to(_LAUNCH_TRAIL)): p.stat().st_mtime_ns
        for p in sorted(_LAUNCH_TRAIL.rglob("*"))
        if p.is_file()
    }


# Baseline is whatever was there before the suite ran. A pre-existing save the
# developer made by playing the game from the checkout is fine; creating or
# rewriting one is not.
_baseline = _trail_snapshot()


@pytest.fixture(autouse=True)
def _isolate_cwd(tmp_path, monkeypatch):
    """Run every test with its CWD inside a private temp directory.

    A subdirectory of ``tmp_path`` rather than ``tmp_path`` itself, so an
    unnoticed autosave can never land on top of a path a test is asserting
    against via its own ``tmp_path``.
    """
    cwd = tmp_path / "_cwd"
    cwd.mkdir()
    monkeypatch.chdir(cwd)


@pytest.fixture(autouse=True)
def _launch_dir_stays_clean(_isolate_cwd):
    """Fail the test that writes ``.trail/`` into the launch directory."""
    yield

    global _baseline
    after = _trail_snapshot()
    if after == _baseline:
        return

    before, _baseline = _baseline, after
    verb = "created" if before is None else "modified"
    pytest.fail(
        f"this test {verb} {_LAUNCH_TRAIL} — a save written outside its "
        f"temp dir. Pass base_path=tmp_path to save_game()/load_game(), or "
        f"monkeypatch escape_the_valley.save.SAVE_DIR to an absolute path, "
        f"for whatever bypassed the CWD isolation.",
        pytrace=False,
    )
