"""Entry point for PyInstaller and `python -m escape_the_valley`.

F-a4d8291c: this MUST be an absolute import, not `from .cli import app`.
PyInstaller runs this file as a bare top-level `__main__` module with no
parent package, so a package-relative import raises `ImportError: attempted
relative import with no known parent package` on every invocation --
including `--help` -- before any application code runs. The absolute form
below works identically for `python -m escape_the_valley` (where
`escape_the_valley` is on sys.path as a real importable package either way).
"""

from __future__ import annotations

import os


def _print_event_count_and_exit() -> None:
    """CI smoke-test hook only -- not a public CLI surface.

    release-binaries.yml's post-build step (scripts/smoke_test_binary.py)
    sets ESCAPE_THE_VALLEY_SMOKE_EVENT_COUNT=1 and runs the frozen binary
    with no other arguments. It prints len(build_event_library()) and exits,
    so the workflow can assert the *loaded* event count (260 today: 60
    hardcoded in events.py + ~200 from data/event_skeletons.json) instead of
    only checking that the process starts (F-a6fa29e0). A startup-only smoke
    test would go green even if event_skeletons.json were missing from the
    bundle -- event_loader.load_json_events() silently returns [] when the
    file is absent, shipping a binary that runs a quarter-game with no error.

    Implemented here (in __main__.py) rather than as a `trail` subcommand in
    cli.py so the whole fix stays inside the ci-tooling domain's owned files
    for this swarm wave -- adding it to cli.py would be a cross-domain edit.
    """
    from escape_the_valley.events import build_event_library

    print(len(build_event_library()))


if os.environ.get("ESCAPE_THE_VALLEY_SMOKE_EVENT_COUNT") == "1":
    _print_event_count_and_exit()
else:
    from escape_the_valley.cli import app

    app()
