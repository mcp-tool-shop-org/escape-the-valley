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


class _RecordingNarration:
    """Stand-in for NarrationPanel capturing stream_reset/append calls."""

    def __init__(self):
        self.reset_count = 0
        self.text = ""

    def stream_reset(self):
        self.reset_count += 1
        self.text = ""

    def stream_append(self, delta):
        self.text += delta


class _RecordingEventBar:
    def __init__(self):
        self.last = None

    def update_from(self, frame, *, busy=False, streaming=False):
        self.last = {"busy": busy, "streaming": streaming}


class TestStreamingRender:
    """gm-feat-01: a streamed scene renders progressively.

    The GM scene/outcome narration arrives token-by-token on a worker thread;
    each delta is marshalled to the UI thread and appended to the narration
    panel so the player watches the storyteller write. We drive a fake on_token
    here (no live GM, no network) and assert the panel grows and the first
    token leaves the 'thinking' state behind.
    """

    def _app(self):
        state = create_new_run(seed=7)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        return app

    def test_tokens_grow_the_narration_panel(self):
        app = self._app()
        narr = _RecordingNarration()
        bar = _RecordingEventBar()

        def _query(sel, *a, **k):
            return narr if "narration" in sel else bar

        app.query_one = _query

        # First token: opens a fresh buffer and flips streaming on.
        assert app._streaming is False
        app._apply_narration_token("The ")
        assert app._streaming is True
        assert narr.reset_count == 1
        assert narr.text == "The "

        # Subsequent tokens append without re-resetting.
        app._apply_narration_token("river ")
        app._apply_narration_token("waits.")
        assert narr.reset_count == 1
        assert narr.text == "The river waits."

    def test_first_token_switches_busy_state_to_writing(self):
        app = self._app()
        narr = _RecordingNarration()
        bar = _RecordingEventBar()

        def _query(sel, *a, **k):
            return narr if "narration" in sel else bar

        app.query_one = _query
        app._apply_narration_token("Smoke on the ridge.")
        # The event bar was repainted in the streaming ('writing') state.
        assert bar.last == {"busy": True, "streaming": True}

    def test_worker_callback_marshals_to_ui_thread(self):
        """_on_narration_token hands the delta to call_from_thread (thread-safe)."""
        app = self._app()
        marshalled = []
        app.call_from_thread = lambda fn, *a: marshalled.append((fn, a))

        app._on_narration_token("a delta")
        assert len(marshalled) == 1
        fn, args = marshalled[0]
        assert fn == app._apply_narration_token
        assert args == ("a delta",)

    def test_empty_delta_is_ignored(self):
        app = self._app()
        called = []
        app.call_from_thread = lambda fn, *a: called.append(1)
        app._on_narration_token("")
        assert called == []

    def test_arm_stream_wraps_gm_and_injects_sink(self):
        """Arming installs a wrapper that threads on_token into the GM call."""
        app = self._app()

        captured = {}
        gm = app._engine.gm

        def _fake_scene(*a, on_token=None, **k):
            captured["scene_on_token"] = on_token
            return None

        def _fake_outcome(*a, on_token=None, **k):
            captured["outcome_on_token"] = on_token
            return None

        gm.generate_scene = _fake_scene
        gm.generate_outcome = _fake_outcome

        app._arm_stream()
        # The live sink is the app's token callback.
        assert app._token_sink == app._on_narration_token

        # Calling through the wrapper forwards the sink the engine never passes.
        app._engine.gm.generate_scene(app._engine.state, None, "clear")
        app._engine.gm.generate_outcome(
            app._engine.state, None, "t", "A", "label", {},
        )
        assert captured["scene_on_token"] == app._on_narration_token
        assert captured["outcome_on_token"] == app._on_narration_token

    def test_gm_off_path_does_not_stream(self):
        """With no live loop the step runs synchronously and never streams."""
        app = self._app()
        app._render_all = lambda: None
        app.notify = lambda *a, **k: None
        # No live runtime → synchronous fallback, _arm_stream never reached.
        assert app._has_worker_runtime() is False
        app.action_intent("TRAVEL")
        assert app._streaming is False
        assert app._token_sink is None


class TestStreamingPilot:
    """gm-feat-01 live exercise: streaming deltas marshalled from a worker
    thread under a live Textual loop land in the real NarrationPanel.

    A travel step is driven through the actual worker path (proving the GM
    wrapper is armed and the busy gate holds), then the streaming callback is
    fed from inside a thread worker — the true cross-thread path — and the real
    panel is asserted to carry the streamed prose.
    """

    def test_worker_path_arms_stream_and_completes(self):
        async def scenario():
            app = _make_app(seed=7)  # GM off
            async with app.run_test() as pilot:
                await pilot.pause()
                await pilot.press("t")
                await app.workers.wait_for_complete()
                await pilot.pause()
                # The travel step ran through the worker; the GM wrapper is
                # installed and we are no longer in flight.
                assert app._gm_wrapped is True
                assert app._in_flight is False

        asyncio.run(scenario())

    def test_streamed_delta_from_worker_reaches_real_panel(self):
        async def scenario():
            app = _make_app(seed=7)
            async with app.run_test() as pilot:
                await pilot.pause()
                app._streaming = False

                # Feed the streaming callback from inside a real thread worker —
                # the genuine cross-thread path on_token uses (call_from_thread).
                def _feed():
                    app._on_narration_token("Cold ")
                    app._on_narration_token("morning.")

                app.run_worker(_feed, thread=True)
                await app.workers.wait_for_complete()
                await pilot.pause()

                panel = app.query_one("#narration", NarrationPanel)
                assert getattr(panel, "_stream_text", "") == "Cold morning."
                assert app._streaming is True

        asyncio.run(scenario())


class TestEndScreen:
    """EC-04 / FEAT-CLITUI-01: the end screen renders the graded ending, the
    GM epilogue (fallback-safe), the diagnostics, and the trail ledger.
    """

    def _ended_state(self, *, victory=False, seed=7):
        """A finished run with a graded ending ready to render."""
        from escape_the_valley.step_engine import compute_ending

        state = create_new_run(seed=seed)
        state.game_over = True
        state.victory = victory
        if not victory:
            state.cause_of_death = "starvation"
            # Kill someone so 'the fallen' row populates.
            state.party.members[-1].health = 0
        state.ending = compute_ending(state)
        return state

    def _ended_app(self, *, victory=False):
        state = self._ended_state(victory=victory)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        return app

    def test_after_step_raises_end_screen_on_game_over(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        app = self._ended_app()
        # Neutralize the live-DOM render; we assert the state transition + the
        # epilogue computation, and render the widget directly elsewhere.
        app._render_all = lambda: None
        app.notify = lambda *a, **k: None

        app._after_step()

        # The run is over → the end screen is up and the frame carries the
        # graded ending the EndScreen widget will render.
        assert app.show_end is True
        assert app._frame.game_over is True
        assert "THE TRAIL" not in app._frame.ending_headline  # headline is prose
        assert any("Survivors" in label for label, _ in app._frame.ending_facts)
        # The epilogue (deterministic floor, GM off) was computed synchronously.
        assert app._frame.epilogue

    def test_end_screen_shows_deterministic_epilogue_with_gm_off(self):
        """The fallback-safe epilogue API fills the screen even with the GM off."""
        app = self._ended_app()
        ending = app._engine.state.ending

        # The synchronous (no live loop) path computes + applies the epilogue.
        text = app._compute_epilogue(ending)
        assert isinstance(text, str)
        assert text  # never empty — deterministic floor

        # And it matches the deterministic builder (GM off).
        from escape_the_valley.gm import build_deterministic_epilogue

        assert text == build_deterministic_epilogue(app._engine.state, ending)

    def test_finish_epilogue_repaints_with_text(self):
        app = self._ended_app()
        app._render_all = lambda: None
        app._frame.game_over = True
        app._finish_epilogue("The country kept its silence.")
        assert app._frame.epilogue == "The country kept its silence."
        assert app._epilogue_text == "The country kept its silence."

    def test_endscreen_widget_renders_all_sections(self):
        from escape_the_valley.adapter import populate_end_data
        from escape_the_valley.tui_app import EndScreen, FrameState

        state = self._ended_state(victory=True)
        frame = FrameState()
        populate_end_data(frame, state)
        frame.epilogue = "They had earned the quiet at the end."

        widget = EndScreen()
        captured = {}
        widget.update = lambda txt: captured.setdefault("t", txt)
        widget.update_from(frame)
        text = captured["t"]

        assert "THE VALLEY IS BEHIND YOU" in text
        assert frame.ending_headline in text
        assert "They had earned the quiet" in text
        assert "The reckoning" in text   # facts section
        assert "The run" in text          # diagnostics section
        assert "Trail ledger" in text     # postcard/ledger section

    def test_endscreen_hidden_for_live_run(self):
        from escape_the_valley.tui_app import EndScreen, FrameState

        widget = EndScreen()
        captured = {}
        widget.update = lambda txt: captured.setdefault("t", txt)
        widget.update_from(FrameState(game_over=False))
        # Live run → no content (the widget is hidden anyway).
        assert captured["t"] == ""

    def test_sync_frame_reapplies_epilogue_after_game_over(self, tmp_path, monkeypatch):
        """A frame re-sync after the epilogue landed must not blank it."""
        monkeypatch.chdir(tmp_path)
        app = self._ended_app()
        app._epilogue_text = "The vow held."
        app._sync_frame()
        assert app._frame.game_over is True
        assert app._frame.epilogue == "The vow held."

    def test_end_screen_pilot_renders_on_death(self):
        """Live-loop: a run driven to game-over raises the end screen."""

        async def scenario():
            from escape_the_valley.tui_app import EndScreen

            app = self._ended_app()
            async with app.run_test() as pilot:
                await pilot.pause()
                await app.workers.wait_for_complete()
                await pilot.pause()
                assert app.show_end is True
                assert app.query_one("#end_screen", EndScreen).display is True

        asyncio.run(scenario())


class TestEndScreenKeyGate:
    """v1.1 polish: gameplay keys are a clean no-op on the end screen.

    The gameplay BINDINGS (t/r/h/p, 1-7) are not gated on show_end, so before
    this fix pressing them on the end screen still dispatched action_intent /
    action_choose -> a step worker. Harmless today (StepEngine.step short
    -circuits at GAME_OVER) but it spins a wasted worker thread + a redundant
    re-render. The guard early-returns so the keypress dispatches no step.
    """

    def _ended_app(self, *, victory=False):
        from escape_the_valley.step_engine import compute_ending

        state = create_new_run(seed=7)
        state.game_over = True
        state.victory = victory
        if not victory:
            state.cause_of_death = "starvation"
        state.ending = compute_ending(state)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        app._render_all = lambda: None
        app.notify = lambda *a, **k: None
        app.show_end = True
        # Spy on the single step-dispatch point both handlers funnel into.
        dispatched = []
        app._run_step = lambda intent: dispatched.append(intent)
        return app, dispatched

    def test_intent_key_dispatches_no_step_on_end_screen(self):
        app, dispatched = self._ended_app()
        for key in ("TRAVEL", "REST", "HUNT", "REPAIR"):
            app.action_intent(key)
        assert dispatched == []

    def test_choose_key_dispatches_no_step_on_end_screen(self):
        app, dispatched = self._ended_app()
        for cid in ("A", "B", "C", "D", "E", "F", "G"):
            app.action_choose(cid)
        assert dispatched == []

    def test_gameplay_keys_still_dispatch_during_live_run(self):
        """Sanity: with the run live, the same keys DO reach the step dispatch."""
        state = create_new_run(seed=7)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        app._render_all = lambda: None
        app.notify = lambda *a, **k: None
        assert app.show_end is False
        dispatched = []
        app._run_step = lambda intent: dispatched.append(intent)
        app.action_intent("TRAVEL")
        assert len(dispatched) == 1

    def test_end_screen_pilot_t_key_runs_no_worker(self):
        """Live loop: pressing 't' on the end screen spawns no step worker."""

        async def scenario():
            app, dispatched = self._ended_app()
            # Use the real render path under the live loop.
            del app._render_all
            async with app.run_test() as pilot:
                await pilot.pause()
                await app.workers.wait_for_complete()
                await pilot.pause()
                assert app.show_end is True
                await pilot.press("t")
                await pilot.pause()
                # No step was dispatched behind the ending.
                assert dispatched == []

        asyncio.run(scenario())


class TestPostcardCopy:
    """FEAT-CLITUI-01: the end screen can export the postcard to a file and the
    end screen surfaces the postcard + run diagnostics.
    """

    def _ended_app_with_receipts(self, tmp_path, monkeypatch, *, receipts=False):
        from escape_the_valley.step_engine import compute_ending

        monkeypatch.chdir(tmp_path)
        state = create_new_run(seed=7)
        state.game_over = True
        state.victory = True
        if receipts:
            from escape_the_valley.backpack_models import SettlementRecord

            state.backpack.enabled = True
            state.backpack.wallet_address = "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh"
            state.backpack.settlements.append(
                SettlementRecord(
                    day=2, location="Millford", status="settled",
                    deltas={"food": -3}, txids=["DEADBEEF01234567"],
                ),
            )
        state.ending = compute_ending(state)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        app._render_all = lambda: None
        notes = []
        app.notify = lambda msg, *a, **k: notes.append(msg)
        app.show_end = True
        # Populate the frame's end data so the copy action has postcard lines.
        from escape_the_valley.adapter import populate_end_data

        populate_end_data(app._frame, state)
        return app, notes, state

    def test_copy_writes_file_and_notifies_path(self, tmp_path, monkeypatch):
        app, notes, state = self._ended_app_with_receipts(tmp_path, monkeypatch)
        app.action_copy_postcard()

        out_file = tmp_path / ".trail" / f"postcard-{state.run_id}.txt"
        assert out_file.exists()
        assert "TRAIL LEDGER" in out_file.read_text(encoding="utf-8")
        assert any("Postcard saved to" in m for m in notes)

    def test_copy_receipted_postcard(self, tmp_path, monkeypatch):
        app, notes, state = self._ended_app_with_receipts(
            tmp_path, monkeypatch, receipts=True,
        )
        assert app._frame.is_postcard is True
        app.action_copy_postcard()
        out_file = tmp_path / ".trail" / f"postcard-{state.run_id}.txt"
        text = out_file.read_text(encoding="utf-8")
        assert "RECEIPTS ON LEDGER" in text

    def test_copy_noop_without_end_screen(self, tmp_path, monkeypatch):
        app, notes, state = self._ended_app_with_receipts(tmp_path, monkeypatch)
        app.show_end = False
        app.action_copy_postcard()
        # No end screen → no write, no notification.
        trail_dir = tmp_path / ".trail"
        written = list(trail_dir.glob("postcard-*.txt")) if trail_dir.exists() else []
        assert written == []
        assert notes == []

    def test_copy_reports_write_failure(self, tmp_path, monkeypatch):
        app, notes, state = self._ended_app_with_receipts(tmp_path, monkeypatch)

        import escape_the_valley.cli as cli_mod

        def _boom(*a, **k):
            raise OSError("disk full")

        monkeypatch.setattr(cli_mod, "write_postcard_file", _boom)
        app.action_copy_postcard()
        assert any("Could not write postcard" in m for m in notes)

    def test_end_screen_surfaces_postcard_and_stats(self, tmp_path, monkeypatch):
        """The diagnostics (CLI `stats` data) and postcard appear on the screen."""
        from escape_the_valley.tui_app import EndScreen

        app, notes, state = self._ended_app_with_receipts(
            tmp_path, monkeypatch, receipts=True,
        )
        widget = EndScreen()
        captured = {}
        widget.update = lambda txt: captured.setdefault("t", txt)
        widget.update_from(app._frame)
        text = captured["t"]
        # Stats data the CLI computes is on the screen.
        assert "The run" in text
        assert f"seed {state.seed}" in text
        assert "Journal" in text
        # The receipted postcard heading + receipts line.
        assert "Postcard (on-ledger)" in text
        assert "RECEIPTS ON LEDGER" in text
        # And the copy hint.
        assert "Press C to copy" in text

    def test_pilot_c_key_copies_postcard(self, tmp_path, monkeypatch):
        """Live loop: pressing 'c' on the end screen writes the postcard file."""

        async def scenario():
            from escape_the_valley.step_engine import compute_ending

            monkeypatch.chdir(tmp_path)
            state = create_new_run(seed=7)
            state.game_over = True
            state.victory = True
            state.ending = compute_ending(state)
            engine = StepEngine(state, GMConfig(enabled=False))
            app = LedgerTrailApp(engine=engine)
            seen = []
            app.notify = lambda msg, *a, **k: seen.append(msg)
            async with app.run_test() as pilot:
                await pilot.pause()
                await app.workers.wait_for_complete()
                await pilot.pause()
                assert app.show_end is True
                await pilot.press("c")
                await pilot.pause()
            out_file = tmp_path / ".trail" / f"postcard-{state.run_id}.txt"
            assert out_file.exists()
            assert any("Postcard saved to" in m for m in seen)

        asyncio.run(scenario())


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
