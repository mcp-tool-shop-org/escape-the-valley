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
from pathlib import Path

from escape_the_valley.gm import GMConfig
from escape_the_valley.step_engine import StepEngine
from escape_the_valley.tui_app import (
    Choice,
    EndScreen,
    EventBar,
    FrameState,
    JournalDrawer,
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
        # F-2f661eea: previously `widget.update = lambda txt: ...`, which
        # swaps out the real Static.update() -- the method that calls
        # Content.from_markup() and is the actual MarkupError crash site. A
        # lambda mock captures the string but can never observe that crash.
        # This calls the real (unmocked) update_from() and reads the
        # widget's own stored content back afterward.
        from escape_the_valley.adapter import populate_end_data
        from escape_the_valley.tui_app import EndScreen, FrameState

        state = self._ended_state(victory=True)
        frame = FrameState()
        populate_end_data(frame, state)
        frame.epilogue = "They had earned the quiet at the end."

        widget = EndScreen()
        widget.update_from(frame)
        text = widget.content

        assert "THE VALLEY IS BEHIND YOU" in text
        assert frame.ending_headline in text
        assert "They had earned the quiet" in text
        assert "The reckoning" in text   # facts section
        assert "The run" in text          # diagnostics section
        assert "Trail ledger" in text     # postcard/ledger section

    def test_endscreen_hidden_for_live_run(self):
        from escape_the_valley.tui_app import EndScreen, FrameState

        widget = EndScreen()
        widget.update_from(FrameState(game_over=False))
        # Live run → no content (the widget is hidden anyway).
        assert widget.content == ""

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
        # F-2f661eea: real (unmocked) update_from() -- see the comment on
        # test_endscreen_widget_renders_all_sections above for why the old
        # `widget.update = lambda ...` mock could not see a render crash.
        widget = EndScreen()
        widget.update_from(app._frame)
        text = widget.content
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


# ── F-133540bb: action_choose must not forward an unoffered choice_id ──


class TestPhantomChoiceGuard:
    """All seven choice bindings (digits 1-7 -> A-G) stay active regardless of
    game phase, but a typical EVENT/ROUTE frame only ever offers 2-4 choices.
    CAMP already guards this via camp_choice_intent (an ungated valve letter
    resolves to nothing and is ignored); this mirrors that guard for the
    non-CAMP branch so an id the current frame never displayed is a clean
    no-op instead of reaching the engine — which either raises internally or
    silently resolves to an option the player never chose.
    """

    def _app_offering(self, phase, choice_ids):
        from escape_the_valley.tui_app import Choice

        app = _make_app(seed=7)
        app._engine.phase = phase
        app._frame.choices = [Choice(cid, f"Option {cid}") for cid in choice_ids]
        dispatched = []
        app._run_step = lambda intent: dispatched.append(intent)
        return app, dispatched

    def test_unoffered_choices_ignored_in_event_phase(self):
        """The exact finding scenario: a 2-option event, digits 3-7 dead."""
        from escape_the_valley.intent import GamePhase

        app, dispatched = self._app_offering(GamePhase.EVENT, ["A", "B"])
        for cid in ("C", "D", "E", "F", "G"):
            app.action_choose(cid)
        assert dispatched == []

    def test_offered_choice_still_dispatches_in_event_phase(self):
        from escape_the_valley.intent import GamePhase, IntentAction

        app, dispatched = self._app_offering(GamePhase.EVENT, ["A", "B"])
        app.action_choose("B")
        assert len(dispatched) == 1
        assert dispatched[0].action == IntentAction.CHOOSE
        assert dispatched[0].choice_id == "B"

    def test_unoffered_choice_ignored_in_route_phase(self):
        from escape_the_valley.intent import GamePhase

        app, dispatched = self._app_offering(
            GamePhase.ROUTE, ["A", "B", "C"],
        )
        app.action_choose("D")
        assert dispatched == []

    def test_offered_choice_dispatches_in_route_phase(self):
        from escape_the_valley.intent import GamePhase

        app, dispatched = self._app_offering(
            GamePhase.ROUTE, ["A", "B", "C"],
        )
        app.action_choose("C")
        assert len(dispatched) == 1

    def test_hotkey_alias_beyond_offered_choices_is_a_clean_noop(self):
        """action_intent's t/r/h/p -> A/B/C/D alias also respects the guard.

        A 2-option event only offers A/B; 'h'/'p' alias to C/D and must not
        reach the engine either (they funnel through action_choose).
        """
        app, dispatched = self._app_offering(
            self._event_phase(), ["A", "B"],
        )
        for intent_str in ("HUNT", "REPAIR"):
            app.action_intent(intent_str)
        assert dispatched == []

    @staticmethod
    def _event_phase():
        from escape_the_valley.intent import GamePhase

        return GamePhase.EVENT


# ── F-2f661eea: every Static.update() site must survive a stray '[/...]' ──


class TestMarkupSafety:
    """F-2f661eea: wave 4 (F-82f88b31) set markup=False on all 16 notify()
    call sites, but every Static subclass in this file is *also* constructed
    with Textual's markup=True default and none override it -- so the same
    Content.from_markup()-raises-on-a-stray-'[/...]' mechanism notify() had
    is still live on every .update() call below. Verified directly against
    the installed textual: Static.update() always calls
    visualize(self, content, markup=self._render_markup) synchronously
    (textual.widgets._static.visualize / Static.update source), and
    Static.__init__ defaults markup=True; Markdown.update() takes a
    completely different path (MarkdownIt("gfm-like").parse(), never
    Content.from_markup()) so NarrationPanel is genuinely immune and is not
    covered here.

    Every test below calls the REAL (unmocked) update()/update_from() -- the
    exact anti-pattern this wave was told to fix in the six tests that used
    to replace widget.update with a capturing lambda (three in this file,
    three in test_adapter.py): a lambda mock records the string but can
    never observe Content.from_markup() raising, so it cannot see this crash
    class by construction. These tests read back widget.visual (the
    Content object Static.update() itself builds and caches) so a
    regression here fails loudly instead of passing silently.
    """

    # The exact stray-closing-tag shape from the finding's own repro
    # (matches F-82f88b31's notify() repro too): a GM-authored aside in
    # brackets with no matching open tag anywhere in the string.
    STRAY = "the guide mutters something [/uncanny] under her breath"

    def test_journal_drawer_survives_stray_closing_tag(self):
        """The brief's own worked example. Escaping must not blank or mangle
        the real message -- the player must still see what the GM wrote."""
        widget = JournalDrawer()
        frame = FrameState(journal=[f"Day 3 — {self.STRAY}: A"])

        widget.update_from(frame)  # real Static.update() -- raises pre-fix

        text = widget.visual.plain
        assert self.STRAY in text
        assert "Day 3" in text

    def test_journal_drawer_chrome_survives_as_real_formatting(self):
        """The '[b]Journal[/b]' header must stay a real bold span -- not
        degrade to literal '[b]Journal[/b]' text just because the dynamic
        line next to it needed escaping (the Director's chrome-vs-content
        split)."""
        widget = JournalDrawer()
        widget.update_from(FrameState(journal=[self.STRAY]))

        visual = widget.visual
        assert "[b]" not in visual.plain
        assert "[/b]" not in visual.plain
        bold_spans = [sp for sp in visual.spans if "b" in str(sp.style)]
        assert bold_spans, "expected a real bold span for the Journal header"
        assert visual.plain[bold_spans[0].start:bold_spans[0].end] == "Journal"
        # And the escaped dynamic content is still fully present as text.
        assert self.STRAY in visual.plain

    def test_eventbar_choice_and_prompt_fields_survive_stray_closing_tag(self):
        """EventChoiceInfo.label/risk_hint/cost_hint (step_engine.py's GM
        JSON) and prompt_title/prompt_text (scene.title/event_title) are the
        two confirmed GM-authored paths into EventBar; both must survive."""
        widget = EventBar()
        frame = FrameState(
            prompt_title=self.STRAY,
            prompt_text=self.STRAY,
            choices=[
                Choice(
                    id="A", label=self.STRAY,
                    risk_hint=self.STRAY, cost_hint=self.STRAY,
                ),
            ],
        )

        widget.update_from(frame)

        text = widget.visual.plain
        assert text.count(self.STRAY) >= 3  # title, prompt text, choice label

    def test_eventbar_choice_id_survives_stray_closing_tag(self):
        """F-d4a8ed17: EventChoiceInfo.id is GM-authored on the primary
        EVENT path -- step_engine.py's _maybe_trigger_event builds
        EventChoiceInfo(id=c.get('id', '?'), ...) straight from the GM's
        own scene.choices[] JSON (gm.py's _validate_scene only checks
        choice['id'] for truthiness, never that it's one of A-G), and
        adapter.py's _build_prompt passes that id straight through to
        Choice.id. That is the SAME untrusted source already established
        for label/risk_hint/cost_hint -- so id must survive exactly like
        its siblings, at BOTH interpolation points in EventBar.update_from:
        the "[b]{id}[/b])" choice line, and the choice_letters joined into
        the [i]...[/i]-wrapped hint_line. Reproduces the finding's own
        repro shape (Choice(id='A[/b]', ...) raised
        textual.markup.MarkupError pre-fix) using the shared STRAY
        fixture so it exercises the identical crash class as every other
        field in this class."""
        widget = EventBar()
        frame = FrameState(
            prompt_title="A Fork in the Weather",
            prompt_text="The wind picks up.",
            choices=[
                Choice(id=self.STRAY, label="Push on through the storm"),
            ],
        )

        widget.update_from(frame)  # real Static.update() -- raises pre-fix

        visual = widget.visual
        text = visual.plain
        # Escaping must not blank or mangle the real message -- the id
        # text is still fully present at both interpolation points (the
        # choice line and the folded-in pick_hint/hint_line).
        assert text.count(self.STRAY) >= 2
        assert "Push on through the storm" in text

        # Chrome survives: "[b]...[/b]" around the id is literal chrome
        # authored in EventBar and is never escaped -- only c.id itself
        # is. So even this adversarial id still renders as a real bold
        # span rather than degrading to literal bracket text.
        assert "[b]" not in text
        bold_texts = {
            visual.plain[sp.start:sp.end]
            for sp in visual.spans if "b" in str(sp.style)
        }
        assert self.STRAY in bold_texts

    def test_eventbar_chrome_survives_as_real_formatting(self):
        widget = EventBar()
        widget.update_from(
            FrameState(prompt_title="Camp", choices=[Choice(id="A", label="Travel")]),
        )

        visual = widget.visual
        assert "[b]" not in visual.plain
        bold_texts = {
            visual.plain[sp.start:sp.end]
            for sp in visual.spans if "b" in str(sp.style)
        }
        assert "Camp" in bold_texts       # prompt_title chrome
        assert "A" in bold_texts          # choice id chrome

    def test_end_screen_epilogue_survives_stray_closing_tag(self):
        """FrameState.epilogue is the once-per-run screen and the field
        _worker_failed's own recovery render depends on staying alive --
        the Director-set HIGH severity centers on this path."""
        widget = EndScreen()
        widget.update_from(FrameState(game_over=True, epilogue=self.STRAY))

        assert self.STRAY in widget.visual.plain

    def test_end_screen_chrome_survives_as_real_formatting(self):
        widget = EndScreen()
        widget.update_from(
            FrameState(game_over=True, victory=True, epilogue="fine."),
        )

        visual = widget.visual
        assert "[b]" not in visual.plain
        bold_texts = {
            visual.plain[sp.start:sp.end]
            for sp in visual.spans if "b" in str(sp.style)
        }
        assert "THE VALLEY IS BEHIND YOU" in bold_texts

    def test_other_static_panels_survive_stray_closing_tag(self):
        """StatusPanel/SuppliesPanel/MapPanel/PartyPanel were never traced to
        a confirmed GM-authored field, but all four are unescaped
        Static.update() sites with markup=True by the same construction as
        the three confirmed panels above -- so all four get the same
        defensive escape, and all four are proven here."""
        frame = FrameState(
            location=self.STRAY,
            next_stop=self.STRAY,
            weather=self.STRAY,
            biome=self.STRAY,
            pace=self.STRAY,
            party_summary=self.STRAY,
            wagon=self.STRAY,
            backpack_status=self.STRAY,
            route_ascii=self.STRAY,
            party_detail=[self.STRAY],
            warnings=[self.STRAY],
            supplies={self.STRAY: 1},
        )

        for widget in (StatusPanel(), SuppliesPanel(), MapPanel(), PartyPanel()):
            widget.update_from(frame)  # must not raise
            assert self.STRAY in widget.visual.plain, type(widget).__name__

    def test_render_all_survives_poisoned_journal_entry_live(self):
        """End-to-end reproduction matching the finding's own repro: a
        normal, successful step leaves behind a JournalEntry whose
        scene_title carries a stray '[/...]' shape. Pre-fix this raised
        MarkupError out of JournalDrawer.update_from inside _render_all on
        the SUCCESS path (_finish_step -> _after_step -> _render_all) --
        never touching _worker_failed at all, so TestWorkerFailureRecovery's
        tests could not have caught it."""
        from escape_the_valley.models import JournalEntry

        async def scenario():
            app = _make_app(seed=7)
            async with app.run_test() as pilot:
                await pilot.pause()

                app._engine.state.journal.append(
                    JournalEntry(
                        day=app._engine.state.day,
                        location="Millford",
                        event_id="ev-poison",
                        scene_title=self.STRAY,
                        narration="",
                        choice_made="A",
                        outcome="",
                    ),
                )

                # The exact call chain the finding traced -- must not raise.
                app._sync_frame()
                app._render_all()
                await pilot.pause()

                assert app.is_running is True
                journal_text = app.query_one("#journal", JournalDrawer).visual.plain
                assert self.STRAY in journal_text

        asyncio.run(scenario())


# ── F-9c0e7613: a worker exception must not freeze the input surface ───


class TestWorkerFailureRecovery:
    """None of the five @work(thread=True) workers used to catch an exception
    from their blocking call before invoking call_from_thread on their
    finish_* handler. Every mutating action handler is gated on
    'not self._in_flight', and that flag was only ever cleared from inside a
    finish_* handler — so a raise mid-call skipped the completion step and
    the whole input surface froze forever (compounded by Textual's default
    exit_on_error=True turning the same raise into a hard app crash). Both
    outcomes are worse than a plain, recoverable notification.

    F-82f88b31: the two live-Pilot tests below (step worker, enable worker)
    previously monkeypatched app.notify before triggering the failure, so
    they never exercised the real notify() -> Toast.render() ->
    Content.from_markup() path — which is exactly where _worker_failed's
    f-string-built message (a raw exception embedded with no markup=False)
    raised MarkupError on any '[/...]'-shaped substring (routine in
    HTTP/XRPL error text) and crashed the whole app. Both now run with
    notifications=True and an UNMOCKED app.notify, using an exception
    message containing a stray '[/...]' sequence, and capture the rendered
    notification from app._notifications afterward instead of replacing
    notify(). The other two tests below (on_worker_state_changed) still
    mock notify() because they never call run_test() at all — there is no
    live message pump or mounted screen for notify() to render into (a
    bare app.notify() call on an unstarted app is a silent no-op: no
    exception, but app._notifications stays empty), so mocking there isn't
    hiding any rendering risk.
    """

    def test_step_worker_exception_recovers_and_app_stays_playable(self):
        """A real threaded step that raises still clears _in_flight, notifies,
        and leaves the app interactive for the next (successful) step.

        F-82f88b31: notify() runs LIVE here (notifications=True, app.notify
        is never replaced) with an exception message shaped like an
        HTTP/XRPL error — a stray '[/...]' substring. Pre-fix, Toast.render()
        called Content.from_markup() on exactly this shape of string and
        raised MarkupError from inside Textual's own compositor, which took
        the whole app down (is_running went False right here, well before
        run_test()'s deliberate teardown). A suite that mocks notify() can't
        see that; this one doesn't mock it.
        """

        async def scenario():
            app = _make_app(seed=7)
            async with app.run_test(notifications=True) as pilot:
                await pilot.pause()

                real_step = app._engine.step

                def _boom(intent):
                    raise RuntimeError(
                        "disk full: GET [/api/v1/accounts/rHb9CJ] failed: "
                        "503 Service Unavailable"
                    )

                app._engine.step = _boom
                before = (
                    app._engine.state.day,
                    app._engine.state.distance_traveled,
                )

                await pilot.press("t")
                await app.workers.wait_for_complete()
                await pilot.pause()

                # The money assertion: rendering a '[/...]'-shaped message
                # through the REAL Toast pipeline must not crash the app.
                assert app.is_running is True

                # Cleared, not frozen — and the player was told plainly.
                # Captured from the live notification collection *after*
                # Toast.render() ran, not by replacing notify().
                messages = [n.message for n in app._notifications]
                assert app._in_flight is False
                assert any(
                    "disk full" in m and "last save is intact" in m
                    for m in messages
                )
                # A Toast for it actually mounted and rendered without
                # raising — the real reproduction of the bug, not a proxy.
                from textual.widgets._toast import Toast

                assert len(app.query(Toast)) >= 1

                # The failed attempt never touched engine state.
                assert (
                    app._engine.state.day,
                    app._engine.state.distance_traveled,
                ) == before

                # The app is still alive: restoring the real step and pressing
                # again completes normally, exactly like the happy-path smoke
                # test (proof this is recoverable, not a one-shot patch-over).
                app._engine.step = real_step
                await pilot.press("t")
                await app.workers.wait_for_complete()
                await pilot.pause()

                assert app._in_flight is False
                after = (
                    app._engine.state.day,
                    app._engine.state.distance_traveled,
                )
                assert after != before

        asyncio.run(scenario())

    def test_enable_worker_exception_recovers(self, monkeypatch):
        """The ledger-group worker path fails safe the same way as step.

        F-82f88b31: same live-notify proof as the step-worker test above,
        against the OTHER worker group (ledger, not step) — _worker_failed
        is shared code, so this confirms the markup=False fix isn't a
        one-group patch-over.
        """

        async def scenario():
            from escape_the_valley.backpack import BackpackManager

            def _boom(self, state):
                raise RuntimeError(
                    "xrpl testnet unreachable: [/accounts/rHb9CJ] 503"
                )

            monkeypatch.setattr(BackpackManager, "enable", _boom)

            app = _make_app(seed=7)
            async with app.run_test(notifications=True) as pilot:
                await pilot.pause()

                app.action_ledger_enable()
                await app.workers.wait_for_complete()
                await pilot.pause()

                # Live Toast render of a '[/...]'-shaped message must not
                # crash the app (the F-82f88b31 regression).
                assert app.is_running is True

                messages = [n.message for n in app._notifications]
                assert app._in_flight is False
                assert any("xrpl testnet unreachable" in m for m in messages)
                # The failure was caught before any state mutation landed.
                assert app._engine.state.backpack.enabled is False

        asyncio.run(scenario())

    def test_on_worker_state_changed_recovers_from_error(self):
        """Belt-and-suspenders net: an ERROR-state worker still unfreezes
        _in_flight and notifies, even one that forgot its own try/except."""
        from textual.worker import WorkerState

        app = _make_app(seed=7)
        app._render_all = lambda: None
        notified = []
        app.notify = lambda msg, *a, **k: notified.append(msg)
        app._in_flight = True

        class _FakeWorker:
            name = "_future_worker"
            error = RuntimeError("forgot the try/except")

        class _FakeEvent:
            state = WorkerState.ERROR
            worker = _FakeWorker()

        app.on_worker_state_changed(_FakeEvent())

        assert app._in_flight is False
        assert any("forgot the try/except" in m for m in notified)

    def test_on_worker_state_changed_ignores_non_error_states(self):
        """A cancelled/successful worker must not be treated as a failure."""
        from textual.worker import WorkerState

        app = _make_app(seed=7)
        app._render_all = lambda: None
        notified = []
        app.notify = lambda *a, **k: notified.append(a)
        app._in_flight = True

        class _FakeWorker:
            name = "_some_worker"
            error = None

        class _FakeEvent:
            state = WorkerState.SUCCESS
            worker = _FakeWorker()

        app.on_worker_state_changed(_FakeEvent())
        assert app._in_flight is True  # untouched
        assert notified == []

    def test_worker_failed_survives_render_all_raising(self):
        """F-2f661eea: _worker_failed's OWN _render_all() must never be
        allowed to re-crash this method. Per the finding's own trace: a
        worker exception routes here, notify() only *queues* a Notify via
        post_message() (it does not paint synchronously), and pre-fix the
        _render_all() call right after it could raise on a still-poisoned
        frame -- tearing the app down before that queued Notify was ever
        dispatched, so the player got zero information. This simulates a
        frame poisoned by something this wave's escaping did not anticipate
        by making _render_all itself raise, and asserts the method still
        completes: _in_flight cleared, notify() already called, no
        exception propagates.
        """
        app = _make_app(seed=7)
        app._in_flight = True
        notified = []
        app.notify = lambda msg, *a, **k: notified.append(msg)

        def _still_poisoned():
            raise RuntimeError("some field this wave didn't know to escape")

        app._render_all = _still_poisoned

        app._worker_failed("step", RuntimeError("original failure"))

        assert app._in_flight is False
        assert any("original failure" in m for m in notified)


# ── F-96d427ea: CSS_PATH must resolve inside a frozen PyInstaller bundle ─


class TestFrozenCssPathResolution:
    """The application half of the dead-binary defect.

    The release workflow's ``--add-data`` (out of this domain's scope) is the
    other half — it has to actually put tui.tcss somewhere under
    sys._MEIPASS for any of this to find. This covers _resolve_css_path's own
    search logic: unchanged behavior from source, and a search across the
    plausible frozen-bundle layouts instead of trusting Textual's
    inspect.getfile()-based default to land somewhere real.
    """

    def test_source_mode_matches_the_real_file(self):
        """Unfrozen: resolves to the real tui.tcss next to tui_app.py."""
        from escape_the_valley import tui_app as tui_app_module

        resolved = Path(tui_app_module._resolve_css_path())
        expected = (
            Path(tui_app_module.__file__).resolve().parent / "tui.tcss"
        )
        assert resolved == expected
        assert resolved.is_file()

    def test_frozen_mode_prefers_package_relative_layout(
        self, tmp_path, monkeypatch,
    ):
        """--add-data '...tui.tcss<sep>escape_the_valley' (the finding's own
        suggested flag) extracts under a package-named subdirectory."""
        from escape_the_valley import tui_app as tui_app_module

        pkg_dir = tmp_path / "escape_the_valley"
        pkg_dir.mkdir()
        (pkg_dir / "tui.tcss").write_text("Screen { background: black; }")

        monkeypatch.setattr(tui_app_module.sys, "frozen", True, raising=False)
        monkeypatch.setattr(
            tui_app_module.sys, "_MEIPASS", str(tmp_path), raising=False,
        )

        resolved = Path(tui_app_module._resolve_css_path())
        assert resolved == pkg_dir / "tui.tcss"

    def test_frozen_mode_falls_back_to_flat_layout(
        self, tmp_path, monkeypatch,
    ):
        """--add-data '...tui.tcss<sep>.' drops it flat at the bundle root."""
        from escape_the_valley import tui_app as tui_app_module

        (tmp_path / "tui.tcss").write_text("Screen { background: black; }")

        monkeypatch.setattr(tui_app_module.sys, "frozen", True, raising=False)
        monkeypatch.setattr(
            tui_app_module.sys, "_MEIPASS", str(tmp_path), raising=False,
        )

        resolved = Path(tui_app_module._resolve_css_path())
        assert resolved == tmp_path / "tui.tcss"

    def test_frozen_mode_with_nothing_bundled_falls_back_to_source_default(
        self, tmp_path, monkeypatch,
    ):
        """Neither candidate exists — degrade to the familiar source-relative
        path rather than silently pointing at an empty _MEIPASS guess."""
        from escape_the_valley import tui_app as tui_app_module

        monkeypatch.setattr(tui_app_module.sys, "frozen", True, raising=False)
        monkeypatch.setattr(
            tui_app_module.sys, "_MEIPASS", str(tmp_path), raising=False,
        )

        resolved = Path(tui_app_module._resolve_css_path())
        expected = (
            Path(tui_app_module.__file__).resolve().parent / "tui.tcss"
        )
        assert resolved == expected

    def test_not_frozen_ignores_stray_meipass(self, monkeypatch):
        """sys.frozen is the gate — a stray _MEIPASS alone must not divert."""
        from escape_the_valley import tui_app as tui_app_module

        monkeypatch.setattr(
            tui_app_module.sys, "_MEIPASS", "/nonexistent", raising=False,
        )
        monkeypatch.setattr(
            tui_app_module.sys, "frozen", False, raising=False,
        )

        resolved = Path(tui_app_module._resolve_css_path())
        expected = (
            Path(tui_app_module.__file__).resolve().parent / "tui.tcss"
        )
        assert resolved == expected
