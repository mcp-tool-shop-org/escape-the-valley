"""Textual Pilot smoke test for the recommended play mode (TCD-B-05).

GM-off TUI is the documented, dependency-free way to play. This mounts the
real app against a real StepEngine and drives one travel action through the
actual key bindings + worker path, asserting the core layout exists and that
a step completes without freezing or crashing. It is a mount/layout + happy
-path smoke, not an exhaustive UI test.

The suite has no pytest-asyncio plugin, so each async scenario is driven
through asyncio.run() inside a plain sync test — no new test dependency.
"""

from __future__ import annotations

import asyncio

from escape_the_valley.gm import GMConfig
from escape_the_valley.step_engine import StepEngine
from escape_the_valley.tui_app import (
    EventBar,
    LedgerTrailApp,
    MapPanel,
    NarrationPanel,
    PartyPanel,
    StatusPanel,
    SuppliesPanel,
)
from escape_the_valley.worldgen import create_new_run


def _make_app(seed: int = 7) -> LedgerTrailApp:
    state = create_new_run(seed=seed)
    engine = StepEngine(state, GMConfig(enabled=False))
    return LedgerTrailApp(engine=engine)


def test_app_mounts_with_core_panels():
    """The app mounts and every core panel is present in the DOM."""

    async def scenario():
        app = _make_app()
        async with app.run_test() as pilot:
            await pilot.pause()
            assert app.query_one("#status", StatusPanel) is not None
            assert app.query_one("#supplies", SuppliesPanel) is not None
            assert app.query_one("#map", MapPanel) is not None
            assert app.query_one("#narration", NarrationPanel) is not None
            assert app.query_one("#party", PartyPanel) is not None
            assert app.query_one("#eventbar", EventBar) is not None

    asyncio.run(scenario())


def test_travel_action_advances_via_worker():
    """Pressing 't' drives a real travel step through the worker path.

    The engine starts on day 1, morning; after the travel step resolves (on a
    worker thread, then synced back) the trail has moved and the app is no
    longer 'in flight'. This is the live exercise of cli-tui-B-02 — proof the
    UI doesn't freeze and the worker completion lands cleanly.
    """

    async def scenario():
        app = _make_app(seed=7)
        async with app.run_test() as pilot:
            await pilot.pause()
            state = app._engine.state
            before = (state.day, state.distance_traveled, state.time_of_day)

            await pilot.press("t")
            await app.workers.wait_for_complete()
            await pilot.pause()

            after = (
                app._engine.state.day,
                app._engine.state.distance_traveled,
                app._engine.state.time_of_day,
            )
            assert after != before  # the trail moved
            assert app._in_flight is False  # busy state cleared

    asyncio.run(scenario())


def test_help_overlay_toggles():
    """'?' toggles the help overlay (legibility for new players)."""

    async def scenario():
        app = _make_app()
        async with app.run_test() as pilot:
            await pilot.pause()
            assert app.show_help is False
            await pilot.press("question_mark")
            await pilot.pause()
            assert app.show_help is True

    asyncio.run(scenario())


def test_parcel_input_does_not_steal_initial_focus():
    """cli-tui-B-03: the hidden parcel input must not grab mount focus.

    A focusable hidden Input would swallow the gameplay keys ('t' etc.). It
    starts non-focusable and is only enabled when the overlay is shown.
    """

    async def scenario():
        app = _make_app()
        async with app.run_test() as pilot:
            await pilot.pause()
            inp = app.query_one("#parcel_input")
            assert inp.can_focus is False
            assert app.focused is not inp

    asyncio.run(scenario())


def test_resume_notifies_run_and_day():
    """cli-tui-B-09: launching with resumed=True confirms run + day on mount."""

    async def scenario():
        state = create_new_run(seed=7)
        state.day = 4
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine, resumed=True)
        seen = []
        # Patch the instance before run_test() triggers on_mount.
        app.notify = lambda msg, *a, **k: seen.append(msg)
        async with app.run_test() as pilot:
            await pilot.pause()
        assert any("Resumed run" in m and "Day 4" in m for m in seen)

    asyncio.run(scenario())


def test_overlay_css_selectors_apply():
    """cli-tui-B-07: #parcel_notify and #send_parcel are styled hidden.

    Both overlays default to `display: none` in tui.tcss. If the selector id
    is wrong (the old #parcel_notice) or missing (#send_parcel had no rule),
    the widget would not be hidden — so an initially-hidden overlay proves the
    corrected selector actually matches.
    """

    async def scenario():
        app = _make_app()
        async with app.run_test() as pilot:
            await pilot.pause()
            assert app.query_one("#parcel_notify").display is False
            assert app.query_one("#send_parcel").display is False

    asyncio.run(scenario())


# ── _in_flight guards the synchronous overlay saves ─────────────────


class TestOverlaySavesRespectInFlight:
    """Stage-C re-verify: the overlay actions that _save() synchronously must
    drop a dispatch while a worker is in flight, exactly like the gameplay
    step/choose handlers already do. Without the guard, a disable/accept/
    refuse/dismiss could fire mid-worker and persist a torn snapshot.

    Driven via direct action calls (no live loop), matching the synchronous
    fallback the unit suite already uses. We spy on _save: with _in_flight
    True, none of the four actions may reach it.
    """

    def _guarded_app(self):
        from escape_the_valley.backpack_models import ParcelRecord

        state = create_new_run(seed=7)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        app._render_all = lambda: None
        app._sync_frame = lambda: None
        app._close_all_overlays = lambda: None
        app.notify = lambda *a, **k: None

        saves = {"n": 0}
        app._save = lambda: saves.__setitem__("n", saves["n"] + 1)

        # Give the parcel actions a real parcel so the ONLY thing that can
        # block them is the _in_flight guard, not the missing-parcel guard.
        parcel = ParcelRecord(
            parcel_id="rSender:FOD:5",
            sender="rSender",
            contents={"food": 5},
            accepted=False,
            day_received=1,
        )
        app._engine.state.backpack.parcels.append(parcel)
        app._current_parcel = parcel
        return app, saves

    def test_disable_dropped_in_flight(self):
        app, saves = self._guarded_app()
        app._engine.state.backpack.enabled = True
        app._in_flight = True
        app.action_ledger_disable()
        assert saves["n"] == 0
        # State untouched: the disable never ran.
        assert app._engine.state.backpack.enabled is True

    def test_accept_parcel_dropped_in_flight(self):
        app, saves = self._guarded_app()
        app._in_flight = True
        app.action_accept_parcel()
        assert saves["n"] == 0
        assert app._current_parcel.accepted is False

    def test_refuse_parcel_dropped_in_flight(self):
        app, saves = self._guarded_app()
        app._in_flight = True
        app.action_refuse_parcel()
        assert saves["n"] == 0
        assert not app._current_parcel.parcel_id.startswith("refused:")

    def test_nudge_dismiss_dropped_in_flight(self):
        app, saves = self._guarded_app()
        app._in_flight = True
        app.action_nudge_dismiss()
        assert saves["n"] == 0
        assert app._engine.state.backpack.nudge_dismissed is False

    def test_guards_release_when_not_in_flight(self):
        """Sanity: with _in_flight False the same actions DO reach _save."""
        app, saves = self._guarded_app()
        app._engine.state.backpack.enabled = True
        app._in_flight = False
        app.action_ledger_disable()
        assert saves["n"] == 1


# ── gm-B-06 consumer: TUI consumes a runtime voice failure ──────────


class _FakeVoiceBridge:
    """Stand-in for VoiceBridge exposing just the status() contract the TUI
    consumer reads (gm-B-06): available, last_error.
    """

    def __init__(self, *, available: bool, last_error: str | None):
        self._available = available
        self._last_error = last_error

    def status(self) -> dict:
        return {
            "installed": True,
            "available": self._available,
            "enabled": True,
            "last_error": self._last_error,
        }

    # _after_step also calls enqueue() when voice is enabled.
    def enqueue(self, event) -> None:
        pass


class TestVoiceRuntimeFailureConsumer:
    """gm-B-06 consumer: when the voice bridge self-disables on a runtime audio
    failure, the TUI must tell the player ONCE and stop claiming voice is on.

    voice.py fully implements the contract (status()['available'] flips False
    with a last_error); before this fix the TUI never read it, so _voice_enabled
    stayed stale True and the player heard silence with no explanation.
    """

    def _voice_app(self, bridge):
        state = create_new_run(seed=7)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        app._render_all = lambda: None
        app._sync_frame = lambda: None
        notes = []
        app.notify = lambda msg, *a, **k: notes.append(msg)
        app._voice_bridge = bridge
        app._voice_enabled = True
        return app, notes

    def test_runtime_failure_notifies_once_and_disables(self):
        bridge = _FakeVoiceBridge(
            available=False, last_error="no audio player found",
        )
        app, notes = self._voice_app(bridge)

        app._check_voice_health()
        assert app._voice_enabled is False
        assert len(notes) == 1
        assert "Voice unavailable" in notes[0]
        assert "no audio player found" in notes[0]

        # Second tick: already notified, no repeat nag.
        app._check_voice_health()
        assert len(notes) == 1

    def test_healthy_bridge_is_left_alone(self):
        bridge = _FakeVoiceBridge(available=True, last_error=None)
        app, notes = self._voice_app(bridge)
        app._check_voice_health()
        assert app._voice_enabled is True
        assert notes == []

    def test_after_step_invokes_the_consumer(self):
        """Lock the wiring: a normal _after_step picks up the failure.

        This is the real path — voice fails asynchronously on the bridge worker
        and the next engine step's _after_step is the tick that catches it.
        """
        bridge = _FakeVoiceBridge(
            available=False, last_error="voice playback failed: boom",
        )
        app, notes = self._voice_app(bridge)
        # Real frame so _after_step's narration extraction has warnings to read.
        app._sync_frame = lambda: None
        app._after_step()
        assert app._voice_enabled is False
        assert any("Voice unavailable" in m for m in notes)
