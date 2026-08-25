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


def _tui_check_and_exit() -> None:
    """CI smoke-test hook only -- not a public CLI surface.

    F-03194eb2: the two checks above (`--help` and
    ESCAPE_THE_VALLEY_SMOKE_EVENT_COUNT) were framed as covering *both*
    non-Python files F-a4d8291c named -- tui.tcss and
    data/event_skeletons.json -- but neither actually touches the stylesheet.
    `--help` is resolved by Click/Typer before any subcommand body runs, so
    it never constructs LedgerTrailApp; the event-count hook only calls
    build_event_library() directly. A binary missing tui.tcss (or built with
    a typo'd/dropped --add-data DEST for it) would pass both checks and only
    fail once a player actually opened the `tui` subcommand -- the exact
    "gate that cannot fail" pattern the smoke test exists to close, left open
    for one of the two files it claims to cover.

    release-binaries.yml's post-build step sets
    ESCAPE_THE_VALLEY_SMOKE_TUI_CHECK=1 and runs the frozen binary with no
    other arguments. This hook constructs LedgerTrailApp() -- the same class
    the real `tui` subcommand builds -- but stops short of .run(): a CI
    runner has no interactive terminal for Textual's event loop to attach to,
    and reaching that far isn't necessary anyway. Textual's App.__init__
    only *records* CSS_PATH as self.css_path; it does not read the file off
    disk until the app actually mounts (App.run() / App.run_async()). So
    resolution is forced explicitly here via the same Stylesheet.read_all()
    call Textual's own mount path makes, and any failure to read the file
    (StylesheetError -- missing, unreadable, or unparsable CSS) is caught and
    reported instead of surfacing as a bare traceback.

    Implemented here (in __main__.py) rather than in cli.py or tui_app.py so
    the whole fix stays inside the ci-tooling domain's owned files for this
    swarm wave -- touching either of those would be a cross-domain edit.
    """
    from textual.css.errors import StylesheetError

    from escape_the_valley.tui_app import LedgerTrailApp

    probe = LedgerTrailApp()
    try:
        probe.stylesheet.read_all(probe.css_path)
    except StylesheetError as exc:
        print(f"TUI_CHECK_FAILED: {exc}")
        raise SystemExit(1)
    print("TUI_CHECK_OK")


if os.environ.get("ESCAPE_THE_VALLEY_SMOKE_TUI_CHECK") == "1":
    _tui_check_and_exit()
elif os.environ.get("ESCAPE_THE_VALLEY_SMOKE_EVENT_COUNT") == "1":
    _print_event_count_and_exit()
else:
    from escape_the_valley.cli import app

    app()
