"""#30 D1: the first-town backpack nudge must not stay open once play goes on,
or its E (enable) key shadows the E escape valve."""

from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import IntentAction, PlayerIntent
from escape_the_valley.step_engine import StepEngine
from escape_the_valley.tui_app import LedgerTrailApp
from escape_the_valley.worldgen import create_new_run


def _app():
    engine = StepEngine(create_new_run(seed=7), GMConfig(enabled=False))
    app = LedgerTrailApp(engine=engine)
    app._render_all = lambda: None
    app._sync_frame = lambda: None
    app._close_all_overlays = lambda: None
    app._save = lambda: None
    app.notify = lambda *a, **k: None
    return app


def test_nudge_closes_when_the_player_plays_on():
    app = _app()
    app.show_nudge = True
    # The nudge opening marks it shown (see _after_step), so it can't re-pop.
    app._engine.state.backpack.nudge_shown = True
    app._run_step(PlayerIntent(action=IntentAction.REST))
    assert app.show_nudge is False
    # One-time offer: closing it by playing does not dismiss it for good.
    assert app._engine.state.backpack.nudge_dismissed is False

