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
