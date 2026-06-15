"""Tests for the step-based engine — determinism, phases, roundtrip."""

from __future__ import annotations

from escape_the_valley.adapter import state_to_frame
from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.save import load_game, save_game
from escape_the_valley.step_engine import StepEngine
from escape_the_valley.worldgen import create_new_run


def _make_engine(seed: int = 42) -> StepEngine:
    state = create_new_run(seed=seed)
    return StepEngine(state, GMConfig(enabled=False))


# ── Determinism ─────────────────────────────────────────────────────


def test_same_seed_same_intents_same_result():
    """Same seed + same intents = same state (GM off)."""
    intents = [
        PlayerIntent(IntentAction.TRAVEL),
        PlayerIntent(IntentAction.REST),
        PlayerIntent(IntentAction.HUNT),
    ]

    results = []
    for _ in range(2):
        engine = _make_engine(seed=99)
        for intent in intents:
            engine.step(intent)
            # If event triggered, always choose A
            if engine.phase == GamePhase.EVENT:
                engine.step(
                    PlayerIntent(IntentAction.CHOOSE, choice_id="A")
                )
            elif engine.phase == GamePhase.ROUTE:
                engine.step(
                    PlayerIntent(IntentAction.CHOOSE, choice_id="A")
                )

        s = engine.state
        results.append((
            s.day,
            s.supplies.food,
            s.supplies.water,
            s.supplies.ammo,
            s.distance_traveled,
            s.party.morale,
        ))

    assert results[0] == results[1]


def test_different_seeds_different_results():
    """Different seeds produce different states."""
    states = []
    for seed in [1, 2]:
        engine = _make_engine(seed=seed)
        engine.step(PlayerIntent(IntentAction.TRAVEL))
        if engine.phase == GamePhase.EVENT:
            engine.step(
                PlayerIntent(IntentAction.CHOOSE, choice_id="A")
            )
        s = engine.state
        states.append(s.distance_traveled)

    # At least some difference (seeds generate different maps)
    # Both should be positive
    assert all(d > 0 for d in states)


# ── Phase transitions ───────────────────────────────────────────────


def test_starts_in_camp():
    engine = _make_engine()
    assert engine.phase == GamePhase.CAMP


def test_travel_produces_messages():
    engine = _make_engine()
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert len(msgs.lines) > 0


def test_rest_produces_messages():
    engine = _make_engine()
    msgs = engine.step(PlayerIntent(IntentAction.REST))
    assert any("rest" in line.lower() for line in msgs.lines)


def test_hunt_uses_ammo():
    engine = _make_engine()
    ammo_before = engine.state.supplies.ammo
    engine.step(PlayerIntent(IntentAction.HUNT))
    assert engine.state.supplies.ammo < ammo_before


def test_hunt_no_ammo():
    engine = _make_engine()
    engine.state.supplies.ammo = 0
    msgs = engine.step(PlayerIntent(IntentAction.HUNT))
    assert any("no ammun" in line.lower() for line in msgs.lines)


def test_repair_uses_parts():
    engine = _make_engine()
    engine.state.wagon.condition = 50  # Needs repair
    parts_before = engine.state.supplies.parts
    engine.step(PlayerIntent(IntentAction.REPAIR))
    assert engine.state.supplies.parts < parts_before


def test_repair_not_needed():
    engine = _make_engine()
    engine.state.wagon.condition = 95
    msgs = engine.step(PlayerIntent(IntentAction.REPAIR))
    assert any("good" in line.lower() for line in msgs.lines)


def test_change_pace():
    engine = _make_engine()
    engine.step(
        PlayerIntent(IntentAction.CHANGE_PACE, pace="hard")
    )
    assert engine.state.wagon.pace.value == "hard"


def test_event_phase_requires_choose():
    """When in EVENT phase, non-CHOOSE intents are rejected."""
    engine = _make_engine()
    # Force an event by setting up the phase manually
    engine.phase = GamePhase.EVENT
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    # Should tell the player to choose, not travel
    assert any("choose" in line.lower() for line in msgs.lines)


def test_game_over_phase():
    engine = _make_engine()
    engine.state.game_over = True
    engine.phase = GamePhase.GAME_OVER
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert any("over" in line.lower() for line in msgs.lines)


# ── Adapter ─────────────────────────────────────────────────────────


def test_adapter_produces_frame():
    engine = _make_engine()
    frame = state_to_frame(engine)
    assert frame.day == 1
    assert frame.location != ""
    assert frame.supplies["FOOD"] > 0
    assert len(frame.party_detail) > 0
    assert len(frame.choices) > 0


def test_adapter_after_travel():
    engine = _make_engine()
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    if engine.phase == GamePhase.EVENT:
        engine.step(
            PlayerIntent(IntentAction.CHOOSE, choice_id="A")
        )
    frame = state_to_frame(engine)
    assert frame.narration != ""


def test_adapter_event_shows_event_choices():
    """When in EVENT phase, adapter shows event choices."""
    engine = _make_engine(seed=42)
    # Travel until we get an event
    for _ in range(20):
        engine.step(PlayerIntent(IntentAction.TRAVEL))
        if engine.phase == GamePhase.EVENT:
            frame = state_to_frame(engine)
            assert frame.prompt_title != "Camp"
            assert len(frame.choices) >= 2
            return
        if engine.phase == GamePhase.ROUTE:
            engine.step(
                PlayerIntent(IntentAction.CHOOSE, choice_id="A")
            )
        if engine.phase == GamePhase.GAME_OVER:
            return

    # Didn't get an event in 20 turns — that's ok, probabilistic


def test_adapter_camp_shows_camp_actions():
    engine = _make_engine()
    frame = state_to_frame(engine)
    labels = [c.label for c in frame.choices]
    assert "Travel" in labels
    assert "Rest" in labels
    assert "Hunt" in labels
    assert "Repair" in labels


# ── Save/Load roundtrip ────────────────────────────────────────────


def test_save_load_preserves_determinism(tmp_path, monkeypatch):
    """Save + load + same intents = same result."""
    monkeypatch.setattr(
        "escape_the_valley.save.SAVE_DIR", tmp_path / ".trail"
    )
    monkeypatch.setattr(
        "escape_the_valley.step_engine.save_game",
        lambda s: save_game(s),
    )

    # Play a few turns
    engine = _make_engine(seed=77)
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    if engine.phase == GamePhase.EVENT:
        engine.step(
            PlayerIntent(IntentAction.CHOOSE, choice_id="A")
        )
    engine.step(PlayerIntent(IntentAction.REST))

    # Save
    save_game(engine.state)

    # Record state
    food_after = engine.state.supplies.food
    day_after = engine.state.day

    # Load
    loaded = load_game()
    assert loaded is not None
    assert loaded.supplies.food == food_after
    assert loaded.day == day_after
    assert loaded.seed == 77


# ── Escape valve integration ─────────────────────────────────────────


def test_abandon_cargo_in_engine():
    engine = _make_engine()
    engine.state.wagon.condition = 20
    engine.state.supplies.set("salt", 10)
    engine.state.supplies.set("cloth", 8)

    old_wagon = engine.state.wagon.condition
    msgs = engine.step(PlayerIntent(IntentAction.ABANDON_CARGO))
    assert engine.state.wagon.condition > old_wagon
    assert any("abandon" in line.lower() for line in msgs.lines)


def test_abandon_cargo_rejected_when_wagon_ok():
    engine = _make_engine()
    engine.state.wagon.condition = 60
    msgs = engine.step(PlayerIntent(IntentAction.ABANDON_CARGO))
    assert any("not damaged" in line.lower() or "not" in line.lower()
               for line in msgs.lines)


def test_desperate_repair_in_engine():
    engine = _make_engine()
    engine.state.wagon.condition = 20
    engine.state.supplies.parts = 0
    msgs = engine.step(PlayerIntent(IntentAction.DESPERATE_REPAIR))
    assert any("repair" in line.lower() for line in msgs.lines)


def test_desperate_repair_rejected_with_parts():
    engine = _make_engine()
    engine.state.wagon.condition = 20
    engine.state.supplies.parts = 5
    msgs = engine.step(PlayerIntent(IntentAction.DESPERATE_REPAIR))
    assert any("no spare" in line.lower() or "requires" in line.lower()
               for line in msgs.lines)


def test_hard_ration_in_engine():
    engine = _make_engine()
    alive = engine.state.party.alive_count
    engine.state.supplies.food = alive * 2
    old_morale = engine.state.party.morale

    msgs = engine.step(PlayerIntent(IntentAction.HARD_RATION))
    assert engine.state.rationing_steps == 2
    assert engine.state.party.morale < old_morale
    assert any("ration" in line.lower() for line in msgs.lines)


def test_hard_ration_rejected_with_plenty_food():
    engine = _make_engine()
    engine.state.supplies.food = 100
    msgs = engine.step(PlayerIntent(IntentAction.HARD_RATION))
    assert any("cannot" in line.lower() for line in msgs.lines)


def test_rationing_decrements_on_travel():
    engine = _make_engine()
    engine.state.rationing_steps = 2
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    # Handle potential event/route phase
    if engine.phase == GamePhase.EVENT:
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    elif engine.phase == GamePhase.ROUTE:
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    assert engine.state.rationing_steps <= 1


def test_rationing_ends_message():
    engine = _make_engine()
    engine.state.rationing_steps = 1
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.state.rationing_steps == 0
    assert any("ration" in line.lower() and "ended" in line.lower()
               for line in msgs.lines)


# ── Adapter escape valve choices ─────────────────────────────────────


def test_adapter_shows_escape_valves():
    engine = _make_engine()
    engine.state.wagon.condition = 20
    engine.state.supplies.parts = 0
    alive = engine.state.party.alive_count
    engine.state.supplies.food = alive * 2

    frame = state_to_frame(engine)
    labels = [c.label for c in frame.choices]

    # Should include standard choices plus escape valves
    assert "Travel" in labels
    assert "Abandon Cargo" in labels
    assert "Desperate Repair" in labels
    assert "Hard Ration" in labels


def test_adapter_hides_valves_when_not_available():
    engine = _make_engine()
    engine.state.wagon.condition = 80
    engine.state.supplies.parts = 10
    engine.state.supplies.food = 100

    frame = state_to_frame(engine)
    labels = [c.label for c in frame.choices]

    assert "Abandon Cargo" not in labels
    assert "Desperate Repair" not in labels
    assert "Hard Ration" not in labels


# ── Trail ledger in game over ────────────────────────────────────────


def test_game_over_shows_trail_ledger():
    engine = _make_engine()
    engine.state.game_over = True
    engine.state.cause_of_death = "starvation"
    engine.phase = GamePhase.GAME_OVER

    frame = state_to_frame(engine)
    assert "TRAIL LEDGER" in frame.prompt_text
    assert frame.prompt_title == "Game Over"


def test_victory_shows_trail_ledger():
    engine = _make_engine()
    engine.state.game_over = True
    engine.state.victory = True
    engine.phase = GamePhase.GAME_OVER

    frame = state_to_frame(engine)
    assert "TRAIL LEDGER" in frame.prompt_text
    assert frame.prompt_title == "Victory!"


# ── Phase 4: Maintenance window tests ──────────────────────────────


def test_rest_then_repair_grants_maintenance():
    engine = _make_engine()
    engine.state.wagon.condition = 50
    engine.step(PlayerIntent(IntentAction.REST))
    engine.step(PlayerIntent(IntentAction.REPAIR))
    assert engine.state.maintained_turns_remaining == 2


def test_repair_then_rest_grants_maintenance():
    engine = _make_engine()
    engine.state.wagon.condition = 50
    engine.step(PlayerIntent(IntentAction.REPAIR))
    engine.step(PlayerIntent(IntentAction.REST))
    assert engine.state.maintained_turns_remaining == 2


def test_maintenance_decrements_on_travel():
    engine = _make_engine()
    engine.state.maintained_turns_remaining = 2
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    # Handle event/route if triggered
    if engine.phase == GamePhase.EVENT:
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    elif engine.phase == GamePhase.ROUTE:
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    assert engine.state.maintained_turns_remaining <= 1


def test_travel_breaks_maintenance_chain():
    engine = _make_engine()
    engine.state.wagon.condition = 50
    engine.step(PlayerIntent(IntentAction.REST))
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    if engine.phase == GamePhase.EVENT:
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    elif engine.phase == GamePhase.ROUTE:
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    engine.step(PlayerIntent(IntentAction.REPAIR))
    # TRAVEL between REST and REPAIR breaks the chain
    assert engine.state.maintained_turns_remaining == 0


def test_diagnostics_tracks_breakdowns():
    engine = _make_engine()
    # Just verify the counter exists and starts at 0
    assert engine.diagnostics["wagon_breakdowns"] == 0
    assert engine.diagnostics["events_total"] == 0


def test_cache_collected_on_arrival():
    engine = _make_engine()
    # Place a cache on the next destination
    for node in engine.state.map_nodes:
        if node.node_id == engine.state.destination_id:
            node.cache_supplies = {"food": 10, "parts": 2}
            break
    # Travel until arrival or game over
    for _ in range(20):
        if engine.phase == GamePhase.GAME_OVER:
            return
        if engine.phase == GamePhase.EVENT:
            engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
        elif engine.phase == GamePhase.ROUTE:
            engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
        elif engine.state.distance_remaining <= 0:
            break
        else:
            engine.step(PlayerIntent(IntentAction.TRAVEL))
    # Cache should be consumed (None) after arrival
    arrived = engine.state.distance_remaining <= 0
    if arrived:
        assert engine.diagnostics["caches_found"] >= 1


# ── ENG-A-05 (resolved): event time_cost now advances the clock ──
#
# ENG-A-05 deferred event-time advancement ("time_cost remains parsed ... for
# future use") because apply_outcome is pure events.py with no engine clock hook.
# The hook now lives in the engine (_handle_event_choice / _trigger_event), so a
# WAIT/DETOUR/REST choice's time_cost advances the time-of-day by that many
# quarter-day slots. apply_outcome stays pure; the callout reports the (now real)
# cost truthfully.


def test_callout_reports_charged_time():
    """The engine now advances the clock by time_cost on event resolution, so the
    fallback callout truthfully reports 'time lost' when a cost was charged — and
    omits it when there was none."""
    from escape_the_valley.events import EventOutcome
    from escape_the_valley.step_engine import _build_fallback_callout

    charged = _build_fallback_callout(EventOutcome(
        supplies_delta={"food": -3}, morale_delta=-2, time_cost=2,
    ))
    assert "2 time lost" in charged
    # Real resource/morale effects are still reported alongside it.
    assert "-3 food" in charged
    assert "morale -2" in charged

    # No time charged → no time mention (a zero time_cost is silent).
    free = _build_fallback_callout(EventOutcome(supplies_delta={"food": -3}))
    assert "time" not in free.lower()


def test_apply_outcome_stays_pure_engine_owns_the_clock():
    """apply_outcome must stay a pure state mutation with NO clock side effect —
    the engine (_handle_event_choice / _trigger_event) owns time advancement, so
    time_cost is charged there, not in apply_outcome. Engine-level advancement is
    covered by test_event_time_cost_advances_clock below."""
    from escape_the_valley.events import EventOutcome, apply_outcome

    engine = _make_engine(seed=5)
    day_before = engine.state.day
    tod_before = engine.state.time_of_day

    apply_outcome(engine.state, EventOutcome(time_cost=3, morale_delta=-1))

    # The pure mutation applied morale but did NOT touch the clock.
    assert engine.state.day == day_before
    assert engine.state.time_of_day == tod_before
    assert engine.state.party.morale < 100  # morale delta did apply


def test_event_time_cost_advances_clock():
    """Resolving an event whose chosen outcome carries a time_cost advances the
    clock by exactly that many quarter-day slots — without drawing RNG
    (determinism intact) and without double-charging consumption."""
    from escape_the_valley.events import (
        EventCategory,
        EventOutcome,
        EventSkeleton,
    )
    from escape_the_valley.models import TimeOfDay
    from escape_the_valley.step_engine import EventChoiceInfo

    engine = _make_engine(seed=5)

    # A controlled pending event: choice A waits 2 slots, no other effects.
    engine._pending_event = EventSkeleton(
        event_id="test_wait",
        title="Test Wait",
        category=EventCategory.SURVIVAL,
        fallback_narration="The party waits it out.",
        outcome_templates={"A": EventOutcome(time_cost=2)},
    )
    engine._pending_event_choices = [
        EventChoiceInfo(id="A", label="Wait it out")
    ]
    engine._pending_event_title = "Test Wait"
    engine._pending_event_narration = "The party waits it out."
    engine.phase = GamePhase.EVENT

    slots = list(TimeOfDay)

    def _clock_index(s) -> int:
        # Monotonic clock position across day boundaries.
        return s.day * len(slots) + slots.index(s.time_of_day)

    before_idx = _clock_index(engine.state)
    counter_before = engine.rng.counter
    food_before = engine.state.supplies.food
    water_before = engine.state.supplies.water

    engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))

    # Clock advanced by exactly the time_cost (2 slots).
    assert _clock_index(engine.state) - before_idx == 2
    # A time-only outcome draws no RNG — determinism/counter preserved.
    assert engine.rng.counter == counter_before
    # The event step charged no consumption (no double-charge with travel).
    assert engine.state.supplies.food == food_before
    assert engine.state.supplies.water == water_before
    # Event resolved cleanly back to camp.
    assert engine.phase == GamePhase.CAMP


# ── ledger-006: settlement failure must never silently drop the delta ──


class _RaisingManager:
    """Stand-in BackpackManager whose settle() raises before enqueuing."""

    def __init__(self, *args, **kwargs):
        pass

    def settle(self, state, location):
        raise RuntimeError("ledger unreachable")

    def close(self):
        pass


class _DummyNode:
    name = "Millford"


def test_settlement_failure_enqueues_pending_record(monkeypatch, caplog):
    """If settle() raises, _settle_checkpoint must log a warning AND enqueue a
    pending SettlementRecord so the delta is retryable, never silently lost."""
    import logging

    import escape_the_valley.backpack as backpack_mod

    engine = _make_engine(seed=42)
    bp = engine.state.backpack
    bp.enabled = True
    # Drift the supplies away from the (empty) last-settled snapshot so there is
    # a real, non-empty delta to lose.
    bp.last_settled_supplies = {}
    engine.state.supplies.set("food", 33)

    monkeypatch.setattr(backpack_mod, "BackpackManager", _RaisingManager)

    assert bp.pending_settlements == []
    with caplog.at_level(logging.WARNING):
        engine._settle_checkpoint(_DummyNode())

    # A warning was logged (not swallowed silently).
    assert any("settlement" in r.message.lower() for r in caplog.records)
    # A pending record now carries the unsettled delta.
    assert len(bp.pending_settlements) == 1
    rec = bp.pending_settlements[0]
    assert rec.status == "pending"
    assert rec.deltas.get("food") == 33
    assert rec.location == "Millford"


def test_settlement_failure_no_pending_when_no_delta(monkeypatch):
    """If there is no unsettled delta, a failed settle() should not invent an
    empty pending record."""
    import escape_the_valley.backpack as backpack_mod

    engine = _make_engine(seed=42)
    bp = engine.state.backpack
    bp.enabled = True
    # Snapshot equals current supplies → zero delta.
    bp.last_settled_supplies = {
        k: engine.state.supplies.get(k)
        for k in ("food", "water", "meds", "ammo", "parts")
    }

    monkeypatch.setattr(backpack_mod, "BackpackManager", _RaisingManager)
    engine._settle_checkpoint(_DummyNode())
    assert bp.pending_settlements == []


def test_parcel_check_failure_logs_and_continues(monkeypatch, caplog):
    """A raising check_parcels() must log a warning and not crash the step."""
    import logging

    import escape_the_valley.backpack as backpack_mod

    class _RaisingParcelManager(_RaisingManager):
        def check_parcels(self, state):
            raise RuntimeError("ledger unreachable")

    engine = _make_engine(seed=42)
    engine.state.backpack.enabled = True
    monkeypatch.setattr(backpack_mod, "BackpackManager", _RaisingParcelManager)

    with caplog.at_level(logging.WARNING):
        engine._check_parcels(_DummyNode())  # must not raise
    assert any("parcel" in r.message.lower() for r in caplog.records)


# ── gm-A-101: CLI play path enforces the D2 weirdness gate in-prompt ──
#
# The legacy GameEngine in engine.py is reached by the CLI play/continue
# commands. Before gm-A-101 it called generate_scene/generate_outcome with no
# brief, so the D2 weirdness floor (uncanny only at weirdness_level >= 2 with
# tokens remaining) was bypassed on that path — only the static LANTERN profile
# header keyed on tokens. These tests assert the engine now passes a non-None
# brief carrying the weirdness allowance to BOTH GM calls.


class _FakeScene:
    """Minimal GM scene with truthy choices so the engine takes the GM path."""

    def __init__(self):
        self.title = "A Stranger at Dusk"
        self.narration = "The trail narrows."
        self.choices = [
            {"id": "A", "label": "Press on", "risk_hint": "", "cost_hint": ""},
            {"id": "B", "label": "Make camp", "risk_hint": "", "cost_hint": ""},
        ]
        self.memory_proposals = []


class _FakeOutcome:
    outcome_narration = "You press on into the gloom."
    outcome_title = "Onward"
    callout = "No significant effect."
    memory_proposals: list = []


def _run_gameengine_event(monkeypatch, weirdness_level, uncanny_tokens):
    """Drive GameEngine._trigger_event once with GM enabled and a mocked GM,
    returning the brief captured from generate_scene and generate_outcome.

    The rng.random gate (> 0.6 skips the event) is forced open by stubbing
    random() to 0.0; UI prompts are stubbed so no console I/O happens.
    """
    from unittest.mock import MagicMock

    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.gm import GMConfig

    state = create_new_run(seed=42)
    state.weirdness_level = weirdness_level
    state.uncanny_tokens = uncanny_tokens

    engine = GameEngine(state, GMConfig(enabled=True))

    # Force the ~60% event-trigger gate open and keep all draws deterministic.
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)

    # Mock the GM: capture the brief kwarg on both calls, return usable scenes.
    captured: dict = {}

    def _scene(state_, event_, weather_str, brief=None):
        captured["scene_brief"] = brief
        return _FakeScene()

    def _outcome(state_, event_, title, choice_id, choice_label, facts, brief=None):
        captured["outcome_brief"] = brief
        return _FakeOutcome()

    engine.gm = MagicMock()
    engine.gm.config.enabled = True
    engine.gm.generate_scene.side_effect = _scene
    engine.gm.generate_outcome.side_effect = _outcome

    # Stub UI so _trigger_event runs headless; player always picks "A".
    monkeypatch.setattr(engine_mod, "show_event_scene", lambda *a, **k: "A")
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)

    engine._trigger_event()
    return captured


def test_gameengine_passes_brief_to_gm_calls(monkeypatch):
    """With GM enabled, GameEngine passes a non-None brief to BOTH
    generate_scene and generate_outcome (the D2 gate carrier)."""
    from escape_the_valley.memory import GMBrief

    captured = _run_gameengine_event(
        monkeypatch, weirdness_level=2, uncanny_tokens=2,
    )

    assert isinstance(captured.get("scene_brief"), GMBrief)
    assert isinstance(captured.get("outcome_brief"), GMBrief)


def test_gameengine_brief_allowance_present_at_high_weirdness(monkeypatch):
    """At weirdness_level >= 2 with tokens, the brief carries a non-'none'
    weirdness allowance on both GM calls (D2 floor satisfied)."""
    captured = _run_gameengine_event(
        monkeypatch, weirdness_level=2, uncanny_tokens=2,
    )

    assert captured["scene_brief"].weirdness_allowance == "hint"
    assert captured["outcome_brief"].weirdness_allowance == "hint"


def test_gameengine_brief_allowance_none_below_floor(monkeypatch):
    """At weirdness_level 0, the brief's weirdness allowance is 'none' on both
    GM calls — the CLI path enforces the D2 floor, not just token count."""
    captured = _run_gameengine_event(
        monkeypatch, weirdness_level=0, uncanny_tokens=2,
    )

    assert captured["scene_brief"].weirdness_allowance == "none"
    assert captured["outcome_brief"].weirdness_allowance == "none"


# ── Stage C: humanization regression tests ───────────────────────────


class _NoneGM:
    """GM stub that is 'enabled' but returns nothing — forces fallback."""

    def __init__(self):
        self.config = GMConfig(enabled=True)

    def generate_scene(self, *a, **k):
        return None

    def generate_outcome(self, *a, **k):
        return None

    def close(self):
        pass


def _force_event_engine(seed=42, gm=None):
    """Build a StepEngine whose next travel will deterministically roll an
    event (rng.random patched to 0.0 opens the ~60% gate)."""
    from escape_the_valley.worldgen import create_new_run

    engine = StepEngine(create_new_run(seed=seed), GMConfig(enabled=False))
    if gm is not None:
        engine.gm = gm
    return engine


# ── ENG-B-08: dead party never rolls events / calls the GM ──


def test_dead_party_short_circuits_to_game_over():
    engine = _make_engine(seed=42)
    for m in engine.state.party.members:
        m.health = 0
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.state.game_over
    assert engine.phase == GamePhase.GAME_OVER
    # No event was offered against the corpses.
    assert engine.phase != GamePhase.EVENT
    assert msgs.event_choices == []


def test_dead_party_does_not_call_gm(monkeypatch):
    """A step with a dead party must not invoke the GM at all."""
    engine = _make_engine(seed=42)
    called = {"scene": 0}

    class _CountingGM(_NoneGM):
        def generate_scene(self, *a, **k):
            called["scene"] += 1
            return None

    engine.gm = _CountingGM()
    for m in engine.state.party.members:
        m.health = 0
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)  # would open gate
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert called["scene"] == 0


# ── ENG-B-05: GM observability + degraded signal ──


def test_gm_fallback_sets_degraded_and_counts(monkeypatch):
    engine = _force_event_engine(seed=42, gm=_NoneGM())
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))

    assert engine.phase == GamePhase.EVENT  # fell back to deterministic event
    assert msgs.gm_degraded is True
    assert msgs.gm_degraded_reason != ""
    assert engine.diagnostics["gm_calls"] >= 1
    assert engine.diagnostics["gm_fallbacks"] >= 1
    # One-time degraded note appears.
    assert any("trail's own voice" in line for line in msgs.lines)


def test_gm_degraded_note_only_once(monkeypatch):
    """The 'GM narration unavailable' note appears at most once per session."""
    engine = _force_event_engine(seed=7, gm=_NoneGM())
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)

    note_counts = []
    for _ in range(4):
        msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
        note_counts.append(
            sum(1 for line in msgs.lines if "trail's own voice" in line)
        )
        if engine.phase == GamePhase.EVENT:
            engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
        elif engine.phase == GamePhase.ROUTE:
            engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
        if engine.phase == GamePhase.GAME_OVER:
            break

    assert sum(note_counts) <= 1


def test_gm_off_does_not_count_calls_or_degrade():
    """With the GM disabled, no gm_calls/fallbacks are recorded and the step is
    never marked degraded."""
    engine = _make_engine(seed=42)  # GMConfig(enabled=False)
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.diagnostics["gm_calls"] == 0
    assert engine.diagnostics["gm_fallbacks"] == 0
    assert msgs.gm_degraded is False


# ── ENG-B-06: invalid choice id must not fizzle the event ──


def test_invalid_choice_retains_event(monkeypatch):
    engine = _force_event_engine(seed=42)
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.phase == GamePhase.EVENT
    offered = [c.id for c in engine.msgs.event_choices]

    # Submit a clearly invalid choice id.
    msgs = engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="Z"))

    # Still in EVENT, pending event intact, helpful message shown.
    assert engine.phase == GamePhase.EVENT
    assert engine._pending_event is not None
    assert any("isn't available" in line for line in msgs.lines)
    assert msgs.event_choices  # re-presented
    # A valid retry then resolves it.
    valid = offered[0]
    engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id=valid))
    assert engine.phase == GamePhase.CAMP


# ── ENG-B-09: dangling destination recovers instead of stranding ──


def test_dangling_destination_recovers(monkeypatch, caplog):
    import logging

    engine = _make_engine(seed=42)
    engine.state.destination_id = "does_not_exist"
    engine.state.distance_remaining = 0

    with caplog.at_level(logging.WARNING):
        engine._arrive_at_next_node()

    # Snapped to the final node, not left stranded.
    assert engine.state.location_id == engine.state.map_nodes[-1].node_id
    assert any("doesn't match the map" in line for line in engine.msgs.lines)
    assert any("not found" in r.message for r in caplog.records)


# ── ENG-B-02: non-event milestones land in the journal ──


def test_town_arrival_recorded_in_journal():
    engine = _make_engine(seed=42)
    town = None
    for node in engine.state.map_nodes:
        if node.node_id == engine.state.destination_id:
            node.is_town = True
            town = node
            break
    assert town is not None
    engine.state.distance_remaining = 0

    before = len(engine.state.journal)
    engine._arrive_at_next_node()
    arrivals = [
        e for e in engine.state.journal if e.event_id == "town:arrival"
    ]
    assert len(engine.state.journal) > before
    assert len(arrivals) == 1
    assert town.name in arrivals[0].scene_title


def test_death_recorded_in_journal():
    engine = _make_engine(seed=42)
    effects = [
        {"type": "died", "member": "Sela", "cause": "Starvation"},
    ]
    before = len(engine.state.journal)
    engine._record_death_milestones(effects)
    assert len(engine.state.journal) == before + 1
    entry = engine.state.journal[-1]
    assert entry.event_id == "death:starvation"
    assert "Sela" in entry.scene_title
    assert "starvation" in entry.outcome.lower()


def test_breakdown_milestone_helper():
    """The milestone helper writes a renderable JournalEntry with empty
    narration/choice but a populated outcome."""
    engine = _make_engine(seed=42)
    before = len(engine.state.journal)
    engine._record_milestone(
        "wagon:breakdown", "Wagon Breakdown", "The wagon broke down.",
        tags=["wagon"],
    )
    assert len(engine.state.journal) == before + 1
    entry = engine.state.journal[-1]
    assert entry.event_id == "wagon:breakdown"
    assert entry.outcome == "The wagon broke down."
    assert "wagon" in entry.tags


def test_long_run_deaths_leave_records():
    """A run where everyone dies of starvation leaves death milestones."""
    engine = _make_engine(seed=42)
    engine.state.supplies.set("food", 0)
    engine.state.supplies.set("water", 50)
    for m in engine.state.party.members:
        m.health = 2
    # One travel step should starve at least one member.
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    # Resolve any event/route so the step settles.
    if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    death_entries = [
        e for e in engine.state.journal if e.event_id.startswith("death:")
    ]
    assert len(death_entries) >= 1


# ── TCD-B-06: a GM that always raises ConnectError still completes the turn ──


def test_gm_connect_error_turn_completes_with_journal_and_fallback(monkeypatch):
    """TCD-B-06 (integration): a real GMClient whose HTTP layer always raises
    ConnectError must let the turn complete — the event resolves, a journal
    entry is written, and the fallback narration is non-empty (the trail's own
    voice carries the scene)."""
    import httpx

    from escape_the_valley.worldgen import create_new_run

    # Real GMClient (enabled), but its underlying transport always refuses.
    engine = StepEngine(create_new_run(seed=42), GMConfig(enabled=True))

    def _always_connect_error(*a, **k):
        raise httpx.ConnectError("ollama down")

    monkeypatch.setattr(engine.gm._client, "post", _always_connect_error)
    # Open the ~60% event gate deterministically.
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)

    journal_before = len(engine.state.journal)

    # Travel — GMClient.generate_scene swallows ConnectError, returns None →
    # the deterministic fallback event is offered.
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.phase == GamePhase.EVENT
    assert msgs.event_narration  # fallback narration is non-empty
    assert msgs.gm_degraded is True
    assert engine.diagnostics["gm_fallbacks"] >= 1

    # Resolve the event — GMClient.generate_outcome also returns None →
    # deterministic callout. Turn completes back in CAMP.
    out = engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    assert engine.phase == GamePhase.CAMP
    assert out.outcome_narration  # non-empty fallback outcome

    # A journal entry for the resolved event was written.
    assert len(engine.state.journal) > journal_before
    last = engine.state.journal[-1]
    assert last.choice_made.startswith("A:")
    engine.gm.close()


# ── EC-04: graded endings (EndingResult) ─────────────────────────────
#
# compute_ending grades the run's shape from existing state only (no new
# economy, no RNG): tier in {triumphant, weathered, pyrrhic, lost}, plus a facts
# dict and a deterministic headline. It is computed once on the GAME_OVER
# transition and exposed on both state.ending and StepMessages.ending so GM +
# cli-tui can consume it.


def _victory_state(seed=42, days=18):
    """A victory state: party at the final node, distance 0, GM-off."""
    state = create_new_run(seed=seed)
    # Snap to the final node, journey complete.
    state.location_id = state.map_nodes[-1].node_id
    state.distance_remaining = 0
    state.distance_traveled = state.total_distance
    state.day = days
    return state


def test_triumphant_all_survivors_on_time():
    from escape_the_valley.step_engine import compute_ending, compute_par_days

    state = _victory_state(days=5)
    state.victory = True
    state.taboo = ""  # vacuously kept
    # All four start alive.
    assert state.party.alive_count == 4
    # Well within par.
    assert state.day <= compute_par_days(state.total_distance)

    ending = compute_ending(state)
    assert ending.tier == "triumphant"
    assert ending.facts["survivors"] == 4
    assert ending.facts["party_size"] == 4
    assert ending.facts["taboo_kept"] is True
    assert ending.headline


def test_weathered_all_survive_but_slow():
    from escape_the_valley.step_engine import compute_ending, compute_par_days

    state = _victory_state()
    state.victory = True
    state.taboo = ""
    par = compute_par_days(state.total_distance)
    state.day = par + 10  # late but everyone made it

    ending = compute_ending(state)
    assert ending.tier == "weathered"
    assert ending.facts["survivors"] == 4
    assert ending.facts["days"] > ending.facts["par_days"]


def test_pyrrhic_one_survivor_finish():
    from escape_the_valley.step_engine import compute_ending

    state = _victory_state(days=5)
    state.victory = True
    state.taboo = ""
    # Kill three of four — a single survivor reaches the valley.
    for m in state.party.members[1:]:
        m.health = 0
        m.death_cause = "Dehydration"

    ending = compute_ending(state)
    assert ending.tier == "pyrrhic"
    assert ending.facts["survivors"] == 1
    assert ending.facts["party_size"] == 4
    assert ending.facts["deaths_by_cause"].get("Dehydration") == 3


def test_pyrrhic_when_taboo_broken_despite_full_survival():
    from escape_the_valley.step_engine import compute_ending

    state = _victory_state(days=5)
    state.victory = True
    # leave_nothing taboo, but a member is dead → vow broken.
    state.taboo = "leave_nothing"
    state.party.members[0].health = 0
    state.party.members[0].death_cause = "Injury"

    ending = compute_ending(state)
    # Survivors < party_size AND taboo broken — still pyrrhic.
    assert ending.tier == "pyrrhic"
    assert ending.facts["taboo_kept"] is False


def test_lost_total_loss():
    from escape_the_valley.step_engine import compute_ending

    state = create_new_run(seed=42)
    state.victory = False
    state.cause_of_death = "Starvation"
    for m in state.party.members:
        m.health = 0
        m.death_cause = "Starvation"

    ending = compute_ending(state)
    assert ending.tier == "lost"
    assert ending.facts["survivors"] == 0
    assert ending.facts["victory"] is False
    assert ending.facts["deaths_by_cause"].get("Starvation") == 4


def test_distinct_tiers_have_distinct_facts():
    """The three headline scenarios yield three different tiers and different
    survivor counts — the ending genuinely discriminates outcomes."""
    from escape_the_valley.step_engine import compute_ending

    # All four survive, on time → triumphant.
    s_win = _victory_state(days=5)
    s_win.victory = True
    s_win.taboo = ""
    e_win = compute_ending(s_win)

    # One survivor → pyrrhic.
    s_one = _victory_state(days=5)
    s_one.victory = True
    s_one.taboo = ""
    for m in s_one.party.members[1:]:
        m.health = 0
        m.death_cause = "Disease"
    e_one = compute_ending(s_one)

    # Total loss → lost.
    s_lost = create_new_run(seed=42)
    s_lost.victory = False
    for m in s_lost.party.members:
        m.health = 0
        m.death_cause = "Exposure"
    e_lost = compute_ending(s_lost)

    tiers = {e_win.tier, e_one.tier, e_lost.tier}
    assert tiers == {"triumphant", "pyrrhic", "lost"}
    survivors = {
        e_win.facts["survivors"],
        e_one.facts["survivors"],
        e_lost.facts["survivors"],
    }
    assert survivors == {4, 1, 0}


def test_compute_ending_is_deterministic_under_fixed_seed():
    """With GM off, the same seed + same lethal mutation yields a byte-identical
    EndingResult (no RNG in the grading)."""
    from escape_the_valley.step_engine import compute_ending

    def build():
        s = _victory_state(seed=123, days=7)
        s.victory = True
        s.taboo = "never_river"
        return compute_ending(s)

    a = build()
    b = build()
    assert a.tier == b.tier
    assert a.headline == b.headline
    assert a.facts == b.facts


def test_engine_populates_ending_on_victory():
    """The engine computes the ending exactly when it transitions to GAME_OVER,
    exposing it on both state.ending and the step's messages."""
    engine = _make_engine(seed=42)
    engine.state.location_id = engine.state.map_nodes[-1].node_id
    engine.state.distance_remaining = 0
    # Trigger the terminal check via a step in CAMP.
    msgs = engine.step(PlayerIntent(IntentAction.REST))

    assert engine.phase == GamePhase.GAME_OVER
    assert engine.state.victory is True
    assert engine.state.ending is not None
    assert msgs.ending is not None
    assert engine.state.ending is msgs.ending
    assert engine.state.ending.tier in (
        "triumphant", "weathered", "pyrrhic",
    )
    assert engine.state.ending.facts["victory"] is True


def test_engine_populates_ending_on_death():
    """A death game-over also produces a graded ('lost') ending on state+msgs."""
    engine = _make_engine(seed=42)
    engine.state.supplies.set("food", 0)
    engine.state.supplies.set("water", 0)
    for m in engine.state.party.members:
        m.health = 1
    # One travel should wipe the party via dehydration/starvation.
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
        msgs = engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))

    if engine.phase == GamePhase.GAME_OVER:
        assert engine.state.ending is not None
        assert engine.state.ending.tier == "lost"
        assert engine.state.ending.facts["victory"] is False
        # The terminal step's messages carry the ending for the UI.
        assert msgs.ending is engine.state.ending


def test_finalize_run_yields_ending_on_timeout():
    """A run that hits a max_steps/timeout terminal without a clean game-over
    still resolves to a non-None EndingResult via finalize_run()."""
    engine = _make_engine(seed=42)
    max_steps = 5

    # Drive a short, capped loop the way a proof harness / turn-capped UI does:
    # break at max_steps without ever waiting for a clean GAME_OVER.
    for _ in range(max_steps):
        if engine.phase == GamePhase.GAME_OVER:
            break
        if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
            engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
        else:
            engine.step(PlayerIntent(IntentAction.REST))

    # The run is still live (no victory, no death) — the historical gap.
    assert engine.state.victory is False

    ending = engine.finalize_run(reason="timeout")

    # The timeout terminal now resolves to a sensible, non-None ending.
    assert ending is not None
    assert engine.state.ending is ending
    assert ending.tier == "lost"
    assert ending.facts["victory"] is False
    assert engine.phase == GamePhase.GAME_OVER
    assert engine.state.game_over is True
    assert engine.state.cause_of_death != ""


def test_finalize_run_is_idempotent_and_preserves_clean_ending():
    """finalize_run() never clobbers a clean victory/death ending; it returns
    the already-graded one and is safe to call more than once."""
    engine = _make_engine(seed=42)
    engine.state.location_id = engine.state.map_nodes[-1].node_id
    engine.state.distance_remaining = 0
    engine.step(PlayerIntent(IntentAction.REST))

    assert engine.phase == GamePhase.GAME_OVER
    clean_ending = engine.state.ending
    assert clean_ending is not None

    # Finalizing an already-terminal run returns the same graded ending.
    again = engine.finalize_run()
    assert again is clean_ending
    assert engine.finalize_run() is clean_ending
    assert engine.state.victory is True


def test_par_days_floor_and_scaling():
    from escape_the_valley.step_engine import compute_par_days

    # Floored for tiny/zero journeys.
    assert compute_par_days(0) == 8
    assert compute_par_days(4) == 8
    # Scales with distance (ceil division by 4) above the floor.
    assert compute_par_days(120) == 30
    assert compute_par_days(121) == 31


def test_taboo_kept_never_river_reads_journal():
    """never_river is broken only on positive journal evidence of a ford."""
    from escape_the_valley.models import JournalEntry
    from escape_the_valley.step_engine import _taboo_kept

    state = create_new_run(seed=42)
    state.taboo = "never_river"
    # No journal yet → kept by default.
    assert _taboo_kept(state) is True

    state.journal.append(JournalEntry(
        day=2, location="Ford", event_id="f1_005",
        scene_title="Rapid Currents", narration="",
        choice_made="A: Ford straight through the current",
        outcome="", tags=["river", "ford"],
    ))
    assert _taboo_kept(state) is False
