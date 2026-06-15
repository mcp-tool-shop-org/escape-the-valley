"""Tests for adapter cliff-edge warnings."""

from escape_the_valley.adapter import _build_warnings
from escape_the_valley.backpack_models import ParcelRecord
from escape_the_valley.gm import GMConfig
from escape_the_valley.save import load_game
from escape_the_valley.step_engine import StepEngine
from escape_the_valley.tui_app import LedgerTrailApp
from escape_the_valley.worldgen import create_new_run


class TestCliffEdgeWarnings:
    def test_food_one_day(self):
        state = create_new_run(seed=42)
        alive = state.party.alive_count
        state.supplies.food = alive * 2  # exactly one day
        warnings = _build_warnings(state)
        assert any("Food for one day" in w for w in warnings)

    def test_water_one_day(self):
        state = create_new_run(seed=42)
        alive = state.party.alive_count
        state.supplies.water = alive * 2
        warnings = _build_warnings(state)
        assert any("Water for one day" in w for w in warnings)

    def test_no_cliff_edge_when_plenty(self):
        state = create_new_run(seed=42)
        warnings = _build_warnings(state)
        assert not any("Food for one day" in w for w in warnings)
        assert not any("Water for one day" in w for w in warnings)

    def test_wagon_no_parts(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 10
        state.supplies.parts = 0
        warnings = _build_warnings(state)
        assert any("One more break" in w for w in warnings)

    def test_wagon_has_parts(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 10
        state.supplies.parts = 2
        warnings = _build_warnings(state)
        assert any("last legs" in w for w in warnings)

    def test_parts_zero_wagon_weak(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 40
        state.supplies.parts = 0
        warnings = _build_warnings(state)
        assert any("No spare parts" in w for w in warnings)

    def test_cliff_edge_suppresses_standard(self):
        """Cliff-edge for food should suppress standard 'Low FOOD' warning."""
        state = create_new_run(seed=42)
        alive = state.party.alive_count
        state.supplies.food = alive * 2
        warnings = _build_warnings(state)
        assert any("Food for one day" in w for w in warnings)
        assert not any("Low FOOD" in w for w in warnings)


class TestCalloutLevel:
    def test_verbose_shows_standard_warnings(self):
        """Verbose mode shows both cliff-edge and standard warnings."""
        state = create_new_run(seed=42)
        state.callout_level = "verbose"
        alive = state.party.alive_count
        # Set food above cliff-edge (alive*2) but at/below warning_low (10)
        state.supplies.food = alive * 2 + 1
        state.wagon.condition = 25  # below 30 threshold
        warnings = _build_warnings(state)
        assert any("Low FOOD" in w for w in warnings)
        assert any("Wagon critical" in w for w in warnings)

    def test_minimal_hides_standard_warnings(self):
        """Minimal mode suppresses standard warnings."""
        state = create_new_run(seed=42)
        state.callout_level = "minimal"
        alive = state.party.alive_count
        state.supplies.food = alive * 2 + 1  # standard low, not cliff-edge
        state.wagon.condition = 25  # below 30 threshold
        warnings = _build_warnings(state)
        assert not any("Low FOOD" in w for w in warnings)
        assert not any("Wagon critical" in w for w in warnings)

    def test_minimal_keeps_cliff_edge(self):
        """Minimal mode still shows cliff-edge warnings."""
        state = create_new_run(seed=42)
        state.callout_level = "minimal"
        alive = state.party.alive_count
        state.supplies.food = alive * 2
        state.wagon.condition = 10
        state.supplies.parts = 0
        warnings = _build_warnings(state)
        assert any("Food for one day" in w for w in warnings)
        assert any("One more break" in w for w in warnings)

    def test_minimal_hides_sick_members(self):
        """Minimal mode suppresses individual sick member warnings."""
        from escape_the_valley.models import Condition

        state = create_new_run(seed=42)
        state.callout_level = "minimal"
        state.party.members[0].condition = Condition.SICK
        warnings = _build_warnings(state)
        assert not any("is sick" in w for w in warnings)


# ── cli-tui-001: TUI persists ledger/parcel state ───────────────────


class _FakeBackpackManager:
    """Stand-in for BackpackManager that mutates state without any network.

    The real manager creates a testnet wallet (enable) or settles on-chain;
    here we only flip local flags so the TUI action paths run save-only.
    """

    def enable(self, state):
        from escape_the_valley.backpack import EnableResult

        state.backpack.enabled = True
        state.backpack.wallet_address = "rFakeWalletAddr123"
        return EnableResult(
            success=True,
            message="enabled",
            wallet_address="rFakeWalletAddr123",
        )

    def disable(self, state):
        state.backpack.enabled = False

    def settle(self, state, location):
        from escape_the_valley.backpack import SettlementResult

        return SettlementResult(success=True, message="settled")

    def accept_parcel(self, parcel, state):
        parcel.accepted = True

    def refuse_parcel(self, parcel):
        parcel.parcel_id = f"refused:{parcel.parcel_id}"

    def close(self):
        pass


def _make_app(seed: int = 7) -> LedgerTrailApp:
    """Build an app with a real engine but neutered Textual-runtime hooks."""
    state = create_new_run(seed=seed)
    engine = StepEngine(state, GMConfig(enabled=False))
    app = LedgerTrailApp(engine=engine)

    # Neutralize the Textual runtime surface the actions touch so they can run
    # outside a live App without a DOM. The mutation + save path stays real.
    app._render_all = lambda: None
    app._sync_frame = lambda: None
    app._close_all_overlays = lambda: None
    app.notify = lambda *a, **k: None

    class _Overlay:
        def show_progress(self):
            pass

        def show_success(self, *a):
            pass

        def show_failure(self, *a):
            pass

    app.query_one = lambda *a, **k: _Overlay()
    return app


class TestTuiPersistence:
    """cli-tui-001: every backpack/parcel mutation must persist to disk.

    Ledger actions don't route through StepEngine autosave, so without an
    explicit _save() a created wallet (or settle/parcel change) is lost on quit.
    """

    def test_save_helper_writes_run_json(self, tmp_path, monkeypatch):
        """_save() actually persists the engine state to .trail/run.json."""
        monkeypatch.chdir(tmp_path)
        app = _make_app()
        app._engine.state.backpack.enabled = True
        app._engine.state.backpack.wallet_address = "rPersistMe"

        app._save()

        loaded = load_game(tmp_path)
        assert loaded is not None
        assert loaded.backpack.enabled is True
        assert loaded.backpack.wallet_address == "rPersistMe"

    def test_ledger_enable_persists(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            "escape_the_valley.backpack.BackpackManager", _FakeBackpackManager,
        )
        app = _make_app()
        app.action_ledger_enable()

        loaded = load_game(tmp_path)
        assert loaded is not None
        assert loaded.backpack.enabled is True
        assert loaded.backpack.wallet_address == "rFakeWalletAddr123"

    def test_ledger_disable_persists(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            "escape_the_valley.backpack.BackpackManager", _FakeBackpackManager,
        )
        app = _make_app()
        app._engine.state.backpack.enabled = True
        app.action_ledger_disable()

        loaded = load_game(tmp_path)
        assert loaded is not None
        assert loaded.backpack.enabled is False

    def test_ledger_settle_persists(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            "escape_the_valley.backpack.BackpackManager", _FakeBackpackManager,
        )
        app = _make_app()
        # settle is save-after-mutation; assert the action calls _save once.
        calls = []
        app._save = lambda: calls.append(1)
        app.action_ledger_settle()
        assert calls == [1]

    def test_accept_parcel_persists(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            "escape_the_valley.backpack.BackpackManager", _FakeBackpackManager,
        )
        app = _make_app()
        parcel = ParcelRecord(
            parcel_id="rSender:FOD:5",
            sender="rSender",
            contents={"food": 5},
            accepted=False,
            day_received=1,
        )
        app._engine.state.backpack.parcels.append(parcel)
        app._current_parcel = parcel
        app.action_accept_parcel()

        loaded = load_game(tmp_path)
        assert loaded is not None
        assert loaded.backpack.parcels[0].accepted is True

    def test_refuse_parcel_persists(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            "escape_the_valley.backpack.BackpackManager", _FakeBackpackManager,
        )
        app = _make_app()
        parcel = ParcelRecord(
            parcel_id="rSender:FOD:5",
            sender="rSender",
            contents={"food": 5},
            accepted=False,
            day_received=1,
        )
        app._engine.state.backpack.parcels.append(parcel)
        app._current_parcel = parcel
        app.action_refuse_parcel()

        loaded = load_game(tmp_path)
        assert loaded is not None
        assert loaded.backpack.parcels[0].parcel_id.startswith("refused:")

    def test_nudge_dismiss_persists(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        app = _make_app()
        app.action_nudge_dismiss()

        loaded = load_game(tmp_path)
        assert loaded is not None
        assert loaded.backpack.nudge_dismissed is True


# ── cli-tui-008: ui.show_outcome degrades on non-numeric deltas ─────


class TestShowOutcomeRobustness:
    """A corrupted/loaded save with a non-int delta must not crash the render."""

    def test_non_numeric_delta_does_not_raise(self):
        from escape_the_valley import ui

        # Mixed deltas: a valid int, plus several non-numeric values that the
        # old code would have hit `val > 0` on and raised TypeError.
        deltas = {"food": -3, "water": "lots", "meds": None, "ammo": ["x"]}
        # Should render cleanly (the numeric one shown, the rest skipped).
        ui.show_outcome("Outcome", "narration", "callout", deltas)

    def test_numeric_deltas_still_render(self, capsys):
        from escape_the_valley import ui

        ui.show_outcome("Outcome", "narration", "", {"food": 5, "water": -2})
        out = capsys.readouterr().out
        assert "food" in out
        assert "water" in out


# ── cli-tui-B-01: escape valves are reachable ───────────────────────


class TestEscapeValveReachability:
    """The conditional valves E/F/G must be both shown AND pickable.

    Before the fix the adapter rendered them but no key dispatched them, so a
    player in a death spiral could see 'Abandon Cargo' and never choose it.
    """

    def _spiral_state(self, seed=11):
        """A state where all three valve gates are open."""
        from escape_the_valley.intent import GamePhase
        from escape_the_valley.step_engine import StepEngine

        state = create_new_run(seed=seed)
        state.wagon.condition = 10          # < 30
        state.supplies.parts = 0            # no parts
        alive = state.party.alive_count
        state.supplies.food = max(0, alive * 3 - 1)  # food critical
        state.rationing_steps = 0
        state.escape_valve_cooldown = 0
        engine = StepEngine(state, GMConfig(enabled=False))
        engine.phase = GamePhase.CAMP
        return state, engine

    def test_all_three_valves_offered(self):
        from escape_the_valley.adapter import camp_choices

        state, _ = self._spiral_state()
        ids = [cid for cid, *_ in camp_choices(state)]
        assert ids == ["A", "B", "C", "D", "E", "F", "G"]

    def test_valve_letters_map_to_intents(self):
        from escape_the_valley.adapter import camp_choice_intent
        from escape_the_valley.intent import IntentAction

        state, _ = self._spiral_state()
        assert camp_choice_intent(state, "E") == IntentAction.ABANDON_CARGO
        assert camp_choice_intent(state, "F") == IntentAction.DESPERATE_REPAIR
        assert camp_choice_intent(state, "G") == IntentAction.HARD_RATION

    def test_unavailable_valve_maps_to_none(self):
        """When a gate is closed, its letter resolves to nothing (ignored)."""
        from escape_the_valley.adapter import camp_choice_intent

        state = create_new_run(seed=11)  # healthy: no valves open
        assert camp_choice_intent(state, "E") is None
        assert camp_choice_intent(state, "F") is None
        assert camp_choice_intent(state, "G") is None

    def test_letters_shift_when_only_some_valves_open(self):
        """Only hard-ration open -> it takes the first valve slot, 'E'."""
        from escape_the_valley.adapter import camp_choice_intent
        from escape_the_valley.intent import IntentAction

        state = create_new_run(seed=11)
        # Healthy wagon (no abandon/desperate), but food critical -> ration.
        alive = state.party.alive_count
        state.supplies.food = max(0, alive * 3 - 1)
        state.rationing_steps = 0
        state.escape_valve_cooldown = 0
        assert camp_choice_intent(state, "E") == IntentAction.HARD_RATION
        assert camp_choice_intent(state, "F") is None

    def test_action_choose_dispatches_valve(self, tmp_path, monkeypatch):
        """action_choose('E') actually steps the engine via the valve intent."""
        monkeypatch.chdir(tmp_path)
        state, engine = self._spiral_state()
        app = LedgerTrailApp(engine=engine)
        app._render_all = lambda: None
        app.notify = lambda *a, **k: None

        captured = {}
        real_step = engine.step

        def _spy(intent):
            captured["action"] = intent.action
            return real_step(intent)

        engine.step = _spy
        app.action_choose("E")

        from escape_the_valley.intent import IntentAction

        assert captured.get("action") == IntentAction.ABANDON_CARGO


# ── gm-B-02 / ENG-B-05 consumer: degraded GM signal in the frame ────


class TestDegradedGmSignal:
    """The engine's degraded-narration flag must reach the FrameState."""

    def test_frame_carries_degraded_flag(self):
        from escape_the_valley.adapter import state_to_frame
        from escape_the_valley.gm import GMConfig
        from escape_the_valley.step_engine import StepEngine

        state = create_new_run(seed=7)
        engine = StepEngine(state, GMConfig(enabled=False))
        engine.msgs.gm_degraded = True
        engine.msgs.gm_degraded_reason = "ollama unreachable"
        frame = state_to_frame(engine)
        assert frame.gm_degraded is True
        assert frame.gm_degraded_reason == "ollama unreachable"

    def test_frame_clean_when_not_degraded(self):
        from escape_the_valley.adapter import state_to_frame
        from escape_the_valley.gm import GMConfig
        from escape_the_valley.step_engine import StepEngine

        state = create_new_run(seed=7)
        engine = StepEngine(state, GMConfig(enabled=False))
        frame = state_to_frame(engine)
        assert frame.gm_degraded is False

    def test_eventbar_renders_fallback_notice(self):
        from escape_the_valley.tui_app import EventBar, FrameState

        bar = EventBar()
        rendered = {}
        bar.update = lambda txt: rendered.setdefault("text", txt)
        bar.update_from(FrameState(gm_degraded=True))
        assert "engine fallback" in rendered["text"]

    def test_eventbar_no_notice_when_healthy(self):
        from escape_the_valley.tui_app import EventBar, FrameState

        bar = EventBar()
        rendered = {}
        bar.update = lambda txt: rendered.setdefault("text", txt)
        bar.update_from(FrameState(gm_degraded=False))
        assert "engine fallback" not in rendered["text"]

    def test_eventbar_busy_state(self):
        """A worker in flight shows the thinking state, not the choices."""
        from escape_the_valley.tui_app import Choice, EventBar, FrameState

        bar = EventBar()
        rendered = {}
        bar.update = lambda txt: rendered.setdefault("text", txt)
        frame = FrameState(choices=[Choice("A", "Travel")])
        bar.update_from(frame, busy=True)
        assert "thinking" in rendered["text"].lower()
        assert "Travel" not in rendered["text"]


# ── cli-tui-B-02: blocking work runs on a worker; sync fallback off-loop ─


class TestStepWorkerFallback:
    """Without a live Textual loop, step()/ledger calls run synchronously.

    The worker path needs a real event loop; unit tests calling actions
    directly must keep working (and stay deterministic) via the synchronous
    fallback. The live worker path is exercised by the Pilot smoke test.
    """

    def test_no_runtime_means_synchronous_step(self):
        state = create_new_run(seed=7)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        app._render_all = lambda: None
        app.notify = lambda *a, **k: None

        assert app._has_worker_runtime() is False

        stepped = {"n": 0}
        real_step = engine.step

        def _spy(intent):
            stepped["n"] += 1
            return real_step(intent)

        engine.step = _spy
        app.action_intent("TRAVEL")
        # Synchronous: the step ran inline and we are not stuck "in flight".
        assert stepped["n"] == 1
        assert app._in_flight is False

    def test_in_flight_guard_blocks_reentry(self):
        """A queued action while a worker runs is ignored, not double-stepped."""
        state = create_new_run(seed=7)
        engine = StepEngine(state, GMConfig(enabled=False))
        app = LedgerTrailApp(engine=engine)
        app._render_all = lambda: None
        app.notify = lambda *a, **k: None

        stepped = {"n": 0}
        real_step = engine.step

        def _spy(intent):
            stepped["n"] += 1
            return real_step(intent)

        engine.step = _spy
        app._in_flight = True  # simulate a worker already running
        app.action_intent("TRAVEL")
        app.action_choose("A")
        assert stepped["n"] == 0  # both ignored


# ── cli-tui-B-03: the send-parcel overlay is a real, working input ──


class _FakeInput:
    def __init__(self, value=""):
        self.id = "parcel_input"
        self.value = value


class _FakeSubmitted:
    """Stand-in for textual.widgets.Input.Submitted."""

    def __init__(self, value, input_id="parcel_input"):
        self.value = value
        self.input = _FakeInput(value)
        self.input.id = input_id


class _RecordingOverlay:
    def __init__(self):
        self.success = None
        self.failure = None

    def show_form(self, *a):
        pass

    def show_success(self, msg):
        self.success = msg

    def show_failure(self, msg):
        self.failure = msg

    def update(self, *a):
        pass


def _parcel_app(tmp_path, monkeypatch, *, send_result=None):
    """An app wired for send-parcel testing without a live DOM."""
    monkeypatch.chdir(tmp_path)
    state = create_new_run(seed=5)
    state.backpack.enabled = True
    state.backpack.wallet_address = "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh"
    engine = StepEngine(state, GMConfig(enabled=False))
    app = LedgerTrailApp(engine=engine)
    app._render_all = lambda: None
    app._sync_frame = lambda: None
    app.notify = lambda *a, **k: None
    app.show_send_parcel = True

    overlay = _RecordingOverlay()
    app.query_one = lambda *a, **k: overlay

    from escape_the_valley.backpack import BackpackManager, SendResult

    sent = {"called": False, "args": None}

    def _send(self, st, recipient, supply, amount):
        sent["called"] = True
        sent["args"] = (recipient, supply, amount)
        return send_result or SendResult(
            success=True, message="sent", txid="ABC",
        )

    monkeypatch.setattr(BackpackManager, "send_parcel", _send)
    monkeypatch.setattr(BackpackManager, "close", lambda self: None)
    return app, overlay, sent


class TestSendParcelOverlayInput:
    def test_valid_command_sends(self, tmp_path, monkeypatch):
        app, overlay, sent = _parcel_app(tmp_path, monkeypatch)
        app.on_input_submitted(
            _FakeSubmitted("rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe food 10"),
        )
        assert sent["called"] is True
        assert sent["args"] == ("rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe", "food", 10)
        assert overlay.success == "sent"

    def test_bad_address_blocks_send(self, tmp_path, monkeypatch):
        app, overlay, sent = _parcel_app(tmp_path, monkeypatch)
        app.on_input_submitted(_FakeSubmitted("not-an-address food 10"))
        assert sent["called"] is False
        assert "not a valid XRPL" in overlay.failure

    def test_wrong_arity_shows_format_hint(self, tmp_path, monkeypatch):
        app, overlay, sent = _parcel_app(tmp_path, monkeypatch)
        app.on_input_submitted(_FakeSubmitted("rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe"))
        assert sent["called"] is False
        assert "Expected" in overlay.failure

    def test_non_numeric_amount_rejected(self, tmp_path, monkeypatch):
        app, overlay, sent = _parcel_app(tmp_path, monkeypatch)
        app.on_input_submitted(
            _FakeSubmitted("rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe food lots"),
        )
        assert sent["called"] is False
        assert "whole number" in overlay.failure

    def test_cancel_closes_without_send(self, tmp_path, monkeypatch):
        app, overlay, sent = _parcel_app(tmp_path, monkeypatch)
        app.on_input_submitted(_FakeSubmitted("cancel"))
        assert sent["called"] is False
        assert app.show_send_parcel is False

    def test_failed_send_surfaces_message(self, tmp_path, monkeypatch):
        from escape_the_valley.backpack import SendResult

        app, overlay, sent = _parcel_app(
            tmp_path, monkeypatch,
            send_result=SendResult(success=False, message="network down"),
        )
        app.on_input_submitted(
            _FakeSubmitted("rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe food 10"),
        )
        assert sent["called"] is True
        assert overlay.failure == "network down"


# ── cli-tui-B-10: ledger-off status names the key to turn it on ─────


class TestLedgerOffHint:
    def test_disabled_status_names_the_key(self):
        from escape_the_valley.adapter import state_to_frame
        from escape_the_valley.gm import GMConfig
        from escape_the_valley.step_engine import StepEngine

        state = create_new_run(seed=7)
        assert state.backpack.enabled is False
        engine = StepEngine(state, GMConfig(enabled=False))
        frame = state_to_frame(engine)
        assert frame.backpack_status == "Ledger: OFF (press L)"

    def test_enabled_status_unchanged(self):
        from escape_the_valley.adapter import state_to_frame
        from escape_the_valley.gm import GMConfig
        from escape_the_valley.step_engine import StepEngine

        state = create_new_run(seed=7)
        state.backpack.enabled = True
        state.backpack.wallet_address = "rABC"
        engine = StepEngine(state, GMConfig(enabled=False))
        frame = state_to_frame(engine)
        assert "press L" not in frame.backpack_status
        assert "Ledger: ON" in frame.backpack_status


# ── cli-tui-B-06: non-color danger cues + NO_COLOR ──────────────────


class TestAccessibilityCues:
    """Urgency must survive a monochrome / colorblind read."""

    def test_supply_cue_thresholds(self):
        from escape_the_valley.ui import _supply_cue

        assert _supply_cue(0) == " (CRITICAL)"
        assert _supply_cue(3) == " (LOW)"
        assert _supply_cue(5) == " (LOW)"
        assert _supply_cue(6) == ""
        assert _supply_cue(50) == ""

    def test_no_color_env_detected(self, monkeypatch):
        from escape_the_valley import ui

        monkeypatch.setenv("NO_COLOR", "1")
        assert ui._no_color() is True
        monkeypatch.delenv("NO_COLOR", raising=False)
        assert ui._no_color() is False

    def test_status_shows_text_cues_under_no_color(self, monkeypatch, capsys):
        """With color stripped, low resources/health/morale still read as such."""
        from escape_the_valley import ui

        state = create_new_run(seed=7)
        state.supplies.food = 0          # critical
        state.supplies.water = 3         # low
        state.party.members[0].health = 12   # critical health
        state.party.morale = 10              # critical morale

        # Force a monochrome console so the assertion tests the plain text,
        # not ANSI color codes.
        monkeypatch.setattr(ui, "console", ui.Console(no_color=True, force_terminal=False))
        ui.show_status(state)
        out = capsys.readouterr().out
        assert "(CRITICAL)" in out  # food and/or morale
        assert "(LOW)" in out       # water
        assert "(!)" in out         # critical health marker
