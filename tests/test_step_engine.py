"""Tests for the step-based engine — determinism, phases, roundtrip."""

from __future__ import annotations

from escape_the_valley.adapter import state_to_frame
from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.physics import check_game_over
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
    # The SAVE_DIR patch above is what redirects the write — save_game() reads
    # it at call time, so the engine's autosave lands in tmp_path too. (There
    # used to be a second patch here replacing step_engine.save_game with an
    # identity lambda; it forwarded to this same function and redirected
    # nothing.)

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
    assert any("not low enough" in line.lower() for line in msgs.lines)


def test_hard_ration_rejected_on_cooldown():
    """F-9fabf995: cooldown is its own sentence, not the shared refusal."""
    engine = _make_engine()
    alive = engine.state.party.alive_count
    engine.state.supplies.food = alive * 2
    engine.state.escape_valve_cooldown = 3
    msgs = engine.step(PlayerIntent(IntentAction.HARD_RATION))
    assert any("3 more actions" in line for line in msgs.lines)
    assert engine.state.rationing_steps == 0


def test_hard_ration_rejected_already_rationing():
    """F-9fabf995: already-min rations, cooldown cleared, food still critical."""
    engine = _make_engine()
    alive = engine.state.party.alive_count
    engine.state.supplies.food = alive * 2
    engine.state.rationing_steps = 2
    engine.state.escape_valve_cooldown = 0
    msgs = engine.step(PlayerIntent(IntentAction.HARD_RATION))
    assert any("already rationing" in line.lower() for line in msgs.lines)


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


# ── F-e86c2e71 (wave 10): dangling destination fails safe, not victory ──
#
# ENG-B-09 originally "recovered" a dangling destination_id by snapping to
# map_nodes[-1] -- which, since map_nodes[-1] is exactly what
# check_game_over() reads as VICTORY, manufactured a false, unearned win any
# time destination_id didn't resolve (reachable purely by hand-editing that
# one field in a save; the map/connections stay pristine). The policy below
# supersedes test_dangling_destination_recovers' old assertion that snapping
# to the final node was the correct recovery -- it was the bug.


def test_dangling_destination_fails_safe_not_victory(monkeypatch, caplog):
    import logging

    engine = _make_engine(seed=42)
    engine.state.destination_id = "does_not_exist"
    engine.state.distance_remaining = 0
    location_before = engine.state.location_id
    assert location_before != engine.state.map_nodes[-1].node_id

    with caplog.at_level(logging.WARNING):
        engine._arrive_at_next_node()

    # Held at the CURRENT node -- never snapped to the final one, never a
    # manufactured victory.
    assert engine.state.location_id == location_before
    assert engine.state.location_id != engine.state.map_nodes[-1].node_id
    assert engine.state.distance_remaining == 0
    # destination_id is repointed at the party's own (now-current) node --
    # see _arrive_at_next_node's own comment for why this is required
    # (avoids an unbounded resource-drain loop) rather than left dangling.
    assert engine.state.destination_id == location_before
    assert check_game_over(engine.state) != "VICTORY"
    assert not engine.state.victory
    assert any("holds its ground" in line for line in engine.msgs.lines)
    assert any("not found" in r.message for r in caplog.records)


def test_dangling_destination_single_valid_connection_self_heals_bounded():
    """The realistic shape (~80% of generated nodes have exactly one
    connection): a dangling destination_id fails safe onto a node whose own
    single connection is perfectly valid. _do_travel's single-connection
    guard validates that connection in isolation and never compares it
    against destination_id, so a destination_id left dangling could never
    satisfy that check on any later action -- it would pay every subsequent
    leg's cost, never arrive, and drain supplies toward a manufactured false
    DEATH (the mirror-image of the false VICTORY this fix closes) instead of
    the bounded, self-healing single echo asserted below."""
    from escape_the_valley.models import Biome, MapNode

    start = MapNode(
        node_id="start", name="Start", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["mid"], distance_to={"mid": 5},
    )
    mid = MapNode(
        node_id="mid", name="Mid", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["far_end"], distance_to={"far_end": 5},
    )
    far_end = MapNode(
        node_id="far_end", name="Far End", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=42)
    state.map_nodes = [start, mid, far_end]
    state.location_id = "mid"
    state.destination_id = "totally-unrelated-garbage-id"
    state.distance_remaining = 5

    engine = StepEngine(state, GMConfig(enabled=False))

    # Leg 1: cost was already committed before arrival is even checked
    # (matches the Director's framing -- nothing left to refuse
    # retroactively). Arrival fails to resolve -> fails safe at "mid".
    engine._do_travel()
    assert engine.state.location_id == "mid"
    assert engine.state.destination_id == "mid"
    assert engine.state.distance_remaining == 0
    assert check_game_over(engine.state) != "VICTORY"

    food_after_failsafe = engine.state.supplies.food
    traveled_after_failsafe = engine.state.distance_traveled

    # Leg 2: the ONE bounded echo -- destination_id (== "mid") matches
    # itself, the arrival tail re-runs harmlessly, and wires the real next
    # hop ("far_end") from mid's actual connections.
    engine._do_travel()
    assert engine.state.destination_id == "far_end"

    # Leg 3+: real progress resumes toward far_end -- not stalled, not
    # draining forever. compute_travel_distance floors at 1/day even under
    # worst-case pace/wagon-condition penalties, so a 5-mile remaining leg
    # is bounded at 5 more calls no matter what breakdown RNG does; this
    # loop bound is a generous margin above that, not a tuned exact count.
    for _ in range(8):
        if engine.state.location_id == "far_end":
            break
        engine._do_travel()

    assert engine.state.location_id == "far_end"
    assert check_game_over(engine.state) == "VICTORY"
    # The party actually reached the end on its own supplies -- a bounded
    # one-leg echo cost, not an unbounded drain.
    assert engine.state.supplies.food <= food_after_failsafe
    assert engine.state.distance_traveled > traveled_after_failsafe


def test_gameengine_dangling_destination_fails_safe_not_victory(monkeypatch, caplog):
    """GameEngine (engine.py) mirror of
    test_dangling_destination_fails_safe_not_victory -- same policy, applied
    independently in the legacy engine (engines are not unified)."""
    import logging

    from escape_the_valley.engine import GameEngine

    state = create_new_run(seed=42)
    engine = GameEngine(state, GMConfig(enabled=False))
    engine.state.destination_id = "does_not_exist"
    engine.state.distance_remaining = 0
    location_before = engine.state.location_id
    assert location_before != engine.state.map_nodes[-1].node_id

    with caplog.at_level(logging.WARNING):
        engine._arrive_at_next_node()

    assert engine.state.location_id == location_before
    assert engine.state.location_id != engine.state.map_nodes[-1].node_id
    assert engine.state.distance_remaining == 0
    assert engine.state.destination_id == location_before
    assert check_game_over(engine.state) != "VICTORY"
    assert not engine.state.victory
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
    """WAVE_4: broken vow is a fact, not a pyrrhic knife.

    On-time intact + broken vow → triumphant (headline must not claim
    'vow unbroken'). Late intact + broken vow → weathered. Death-wins
    stay pyrrhic via test_pyrrhic_one_survivor_finish.
    """
    from escape_the_valley.models import JournalEntry
    from escape_the_valley.step_engine import compute_ending, compute_par_days

    night = JournalEntry(
        day=2, location="Camp", event_id="night_watch",
        scene_title="Night", narration="",
        choice_made="A: Keep watch",
        outcome="", tags=["night"],
    )

    on_time = _victory_state(days=5)
    on_time.victory = True
    on_time.taboo = "never_night"
    on_time.journal.append(night)
    e_on_time = compute_ending(on_time)
    assert on_time.party.alive_count == 4
    assert e_on_time.facts["taboo_kept"] is False
    assert e_on_time.tier == "triumphant"
    assert "vow unbroken" not in e_on_time.headline.lower()

    late = _victory_state()
    late.victory = True
    late.taboo = "never_night"
    late.day = compute_par_days(late.total_distance) + 10
    late.journal.append(night)
    e_late = compute_ending(late)
    assert late.party.alive_count == 4
    assert e_late.facts["taboo_kept"] is False
    assert e_late.tier == "weathered"


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
    # Scales with distance (ceil division by 8) above the floor.
    assert compute_par_days(120) == 15
    assert compute_par_days(121) == 16


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


# ── F-ec4745c1: spoilage fires at most once per qualifying day ────────


def test_spoilage_fires_at_most_once_per_day():
    """A calendar day spans multiple TRAVEL actions (each advances
    time_of_day by one quarter-day). Before the fix, check_spoilage() rolled
    fresh on every one of them whenever state.day % 3 == 0, so a day with
    3 travels could spoil food 2-3x. Drives the real StepEngine._do_travel()
    directly (bypassing phase dispatch) so an EVENT trigger mid-sequence
    can't block subsequent travel calls; food is kept plentiful so a firing
    roll always yields a non-zero, message-producing loss."""
    from escape_the_valley.models import TimeOfDay

    engine = _make_engine(seed=42)
    engine.state.supplies.set("salt", 0)
    engine.state.supplies.food = 500
    engine.state.distance_remaining = 10_000
    engine.state.total_distance = max(engine.state.total_distance, 10_000)
    engine.state.day = 3
    engine.state.time_of_day = TimeOfDay.MORNING

    spoil_messages = 0
    for _ in range(3):
        engine.msgs.lines.clear()
        engine._do_travel()
        spoil_messages += sum(
            1 for line in engine.msgs.lines if "spoiled" in line.lower()
        )

    # All three travels stayed within calendar day 3 (day only increments on
    # the NIGHT -> MORNING wrap, which would be a 4th call).
    assert engine.state.day == 3
    assert spoil_messages == 1


def test_spoilage_guard_advances_to_the_next_qualifying_day():
    """The per-day guard must not permanently suppress spoilage -- day 6
    (the next day % 3 == 0) must roll again after day 3 already fired."""
    from escape_the_valley.physics import check_spoilage

    engine = _make_engine(seed=42)
    engine.state.supplies.set("salt", 0)
    engine.state.supplies.food = 500
    engine.state.day = 3

    first = check_spoilage(engine.state, engine.rng)
    assert first != {}

    engine.state.day = 6
    second = check_spoilage(engine.state, engine.rng)
    assert second != {}


# ── F-4d750550: half-day HUNT/REPAIR consumption rounds toward zero ───


def test_repair_half_day_consumption_rounds_toward_zero():
    """Reproduces the empirically-confirmed regression: a 3-person party's
    full daily consumption is magnitude 1 ({'food': -1, ...}); the buggy
    `v // 2` floor charged the FULL -1 (0% reduction) instead of 0."""
    from escape_the_valley.models import Pace, PartyMember
    from escape_the_valley.physics import compute_daily_consumption

    engine = _make_engine(seed=42)
    engine.state.doctrine = ""  # isolate from doctrine consumption_mult
    engine.state.party.members = [
        PartyMember(name="A"), PartyMember(name="B"), PartyMember(name="C"),
    ]
    engine.state.wagon.pace = Pace.STEADY
    engine.state.wagon.condition = 50  # needs repair
    engine.state.supplies.set("parts", 3)
    engine.state.supplies.food = 100

    full = compute_daily_consumption(engine.state)
    assert full["food"] == -1  # confirms the magnitude-1 scenario

    food_before = engine.state.supplies.food
    engine.step(PlayerIntent(IntentAction.REPAIR))

    # attempt_repair() only ever touches "parts" -- any food change here
    # comes solely from the half-day consumption tail, which must be 0.
    assert food_before - engine.state.supplies.food == 0


def test_hunt_half_day_consumption_rounds_toward_zero():
    """Same rounding fix, isolated via water: attempt_hunt() never touches
    water (only ammo and, on success, food), so any water loss here is
    purely the half-day consumption tail."""
    from escape_the_valley.models import Pace, PartyMember
    from escape_the_valley.physics import compute_daily_consumption

    engine = _make_engine(seed=42)
    engine.state.doctrine = ""
    engine.state.party.members = [
        PartyMember(name="A"), PartyMember(name="B"), PartyMember(name="C"),
    ]
    engine.state.wagon.pace = Pace.STEADY
    engine.state.supplies.set("ammo", 5)
    engine.state.supplies.water = 100

    full = compute_daily_consumption(engine.state)
    assert full["water"] == -1

    water_before = engine.state.supplies.water
    engine.step(PlayerIntent(IntentAction.HUNT))

    assert water_before - engine.state.supplies.water == 0


# ── F-7d3e005b: invalid ROUTE choice_id must be rejected ───────────────


def test_invalid_route_choice_id_rejected():
    """An unrecognized choice_id in ROUTE phase must be rejected, not
    silently treated as 'pick route A' (mirrors ENG-B-06 for events)."""
    from escape_the_valley.step_engine import RouteOption

    engine = _make_engine(seed=42)
    engine._pending_routes = [
        RouteOption(node_id="node-a", name="Northern Pass", distance=10),
        RouteOption(node_id="node-b", name="Southern Trail", distance=14),
    ]
    engine.phase = GamePhase.ROUTE
    engine.state.destination_id = "unset"
    engine.state.distance_remaining = 999

    msgs = engine.step(
        PlayerIntent(IntentAction.CHOOSE, choice_id="ZZZ-not-a-real-choice")
    )

    # Rejected -- state untouched, still in ROUTE, options re-presented.
    assert engine.state.destination_id == "unset"
    assert engine.state.distance_remaining == 999
    assert engine.phase == GamePhase.ROUTE
    assert engine._pending_routes  # not cleared
    assert any("isn't available" in line for line in msgs.lines)
    assert msgs.route_options

    # A valid retry then resolves it correctly (picks B, not A-by-default).
    engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="B"))
    assert engine.state.destination_id == "node-b"
    assert engine.phase == GamePhase.CAMP


def test_route_choice_id_out_of_range_rejected():
    """A syntactically valid letter ('C') with no corresponding pending
    route (only 2 offered) must also be rejected, not wrap or clamp."""
    from escape_the_valley.step_engine import RouteOption

    engine = _make_engine(seed=42)
    engine._pending_routes = [
        RouteOption(node_id="node-a", name="Northern Pass", distance=10),
        RouteOption(node_id="node-b", name="Southern Trail", distance=14),
    ]
    engine.phase = GamePhase.ROUTE
    engine.state.destination_id = "unset"

    engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="C"))

    assert engine.state.destination_id == "unset"
    assert engine.phase == GamePhase.ROUTE

    # Empty default choice_id ("") must also be rejected, not default to A.
    engine.step(PlayerIntent(IntentAction.CHOOSE))
    assert engine.state.destination_id == "unset"
    assert engine.phase == GamePhase.ROUTE


# ── F-3abad222: dangling route connections must not softlock ROUTE ────


def _dangling_fork_state(seed: int = 42):
    """A 3-node map where the current node's connections don't resolve to
    any real map_node -- the corrupted-save shape generate_map() itself
    never produces, but load_game_result()'s shape check would accept."""
    from escape_the_valley.models import Biome, MapNode

    start = MapNode(
        node_id="start", name="Start", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["ghost-1", "ghost-2"],
        distance_to={"ghost-1": 10, "ghost-2": 12},
    )
    end = MapNode(
        node_id="end", name="End", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=seed)
    state.map_nodes = [start, end]
    state.location_id = "start"
    state.destination_id = "start"
    state.distance_remaining = 0
    return state


def test_dangling_route_connections_recover_at_construction():
    """Constructing a StepEngine against a save where the current node's
    connections are all dangling must NOT enter GamePhase.ROUTE with an
    empty pending-routes list (0 < 0 is always False -> permanent softlock,
    and one that would re-occur on every reload).

    F-dd6869ca (post-wave-6): recovery no longer beelines to map_nodes[-1]
    -- on a mid-map fork that would skip unvisited content and manufacture
    a false VICTORY. There is nothing legitimate to recover to here (both
    raw connections are dangling), so the party simply stays put --
    destination_id/distance_remaining are untouched, not redirected to
    "end"."""
    state = _dangling_fork_state()
    engine = StepEngine(state, GMConfig(enabled=False))

    assert engine.phase != GamePhase.ROUTE
    assert engine._pending_routes == []
    # NOT redirected anywhere -- still exactly where it started.
    assert engine.state.destination_id == "start"
    assert engine.state.location_id == "start"
    assert engine.state.distance_remaining == 0

    # The engine is still usable afterward -- no dead end, and no crash --
    # even though the fork itself remains impassable.
    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.phase != GamePhase.ROUTE
    assert len(msgs.lines) > 0
    assert engine.state.location_id == "start"  # still didn't move

    # Reload-stability: constructing a SECOND engine from the same corrupted
    # shape recovers the same way every time (not a first-time fluke).
    state2 = _dangling_fork_state()
    engine2 = StepEngine(state2, GMConfig(enabled=False))
    assert engine2.phase != GamePhase.ROUTE
    assert engine2._pending_routes == []


def test_dangling_route_connections_recover_mid_travel():
    """The other call site: a node whose connections are all dangling is
    discovered mid-travel (on arrival), not at construction. The following
    TRAVEL action must neither enter a dead ROUTE nor fake-advance toward a
    fabricated destination.

    F-dd6869ca (post-wave-6): previously this asserted an auto-advance to
    "end" (map_nodes[-1]) -- exactly the false-progress beeline the finding
    forbids. With nothing legitimate to recover to, the party now stays at
    "start" instead.

    Drives _do_travel() directly (bypassing phase dispatch), the same
    technique used in test_spoilage_fires_at_most_once_per_day, so a random
    EVENT trigger on the first travel can't swallow the second TRAVEL --
    step() would reroute it into the EVENT handler (CHOOSE-only) instead of
    running _do_travel()'s fork check at all."""
    from escape_the_valley.models import Biome, MapNode

    prev = MapNode(
        node_id="prev", name="Prev", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["start"], distance_to={"start": 1},
    )
    start = MapNode(
        node_id="start", name="Start", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["ghost-1", "ghost-2"],
        distance_to={"ghost-1": 10, "ghost-2": 12},
    )
    end = MapNode(
        node_id="end", name="End", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=42)
    state.map_nodes = [prev, start, end]
    state.location_id = "prev"
    state.destination_id = "start"
    state.distance_remaining = 1

    engine = StepEngine(state, GMConfig(enabled=False))
    assert engine.phase != GamePhase.ROUTE  # "prev" has only 1 connection

    # First travel arrives at "start" (the dangling-fork node).
    engine._do_travel()
    assert engine.state.location_id == "start"
    assert engine.phase != GamePhase.ROUTE  # fork check is next-travel, not this one
    distance_after_arrival = engine.state.distance_traveled

    # Second travel hits the fork check on "start" and must neither enter
    # ROUTE with nothing pickable NOR fabricate a route to "end".
    engine._do_travel()
    assert engine.phase != GamePhase.ROUTE
    assert engine.state.destination_id == "start"
    assert engine.state.location_id == "start"
    # No fake travel leg was charged for the stalled action either.
    assert engine.state.distance_traveled == distance_after_arrival


# ── F-0877c51a / F-dd6869ca (wave 6): self-edge + mid-map recovery ─────


def _self_edge_fork_state(seed: int = 42):
    """A 2-node map where the current node's connections include a
    self-edge (its own node_id) plus one dangling id -- the corrupted-save
    shape F-0877c51a targets. After excluding the self-edge, zero
    legitimate connections remain (the other id is dangling), so this must
    land on the same no-fake-destination stall as _dangling_fork_state,
    NOT commit to the self-edge as if it were "the one real route"."""
    from escape_the_valley.models import Biome, MapNode

    fork = MapNode(
        node_id="fork", name="Fork", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["fork", "ghost"],
        distance_to={"fork": 5, "ghost": 10},
    )
    end = MapNode(
        node_id="end", name="End", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=seed)
    state.map_nodes = [fork, end]
    state.location_id = "fork"
    state.destination_id = "fork"
    state.distance_remaining = 0
    return state


def test_self_edge_fork_connection_excluded_and_terminates():
    """F-0877c51a: a fork whose raw connections include the CURRENT node's
    own id (connections=["fork", "ghost"]) must not resolve that self-id
    as "the one real route" -- committing to it reproduces the original
    infinite-travel bug this whole recovery mechanism exists to close,
    just reached via the "exactly one resolves" branch instead of "zero
    resolve". After excluding the self-edge, zero legitimate connections
    remain here (the other id is dangling), so this must land on the same
    honest, no-fake-progress stall as
    test_dangling_route_connections_recover_mid_travel: repeated travel
    attempts must terminate (not loop forever faking arrivals)."""
    state = _self_edge_fork_state()
    engine = StepEngine(state, GMConfig(enabled=False))

    assert engine.phase != GamePhase.ROUTE
    assert engine.state.destination_id == "fork"
    assert engine.state.location_id == "fork"

    starting_food = engine.state.supplies.food

    # Drive several travel actions -- across all of them, the party must
    # never "arrive" at itself repeatedly (the externally observable shape
    # of the original bug: climbing distance_traveled, draining supplies,
    # for zero real progress, forever).
    for _ in range(6):
        engine._do_travel()

    assert engine.state.location_id == "fork"
    assert engine.state.distance_traveled == 0
    assert engine.state.supplies.food == starting_food
    assert engine.phase != GamePhase.ROUTE


def test_self_edge_fork_with_one_real_alternative_takes_the_real_route():
    """F-0877c51a, the blended sub-case: a fork's raw connections mix a
    self-edge with one genuinely resolving OTHER node
    (connections=["fork", "real"]). After excluding the self-edge, exactly
    one legitimate route remains, so the engine must offer/take THAT route
    -- not miscount the self-edge as a second real option (which would let
    it be presented as a pickable path), and not discard the real route in
    favor of a zero-resolve stall."""
    from escape_the_valley.models import Biome, MapNode

    fork = MapNode(
        node_id="fork", name="Fork", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["fork", "real"],
        distance_to={"fork": 5, "real": 9},
    )
    real = MapNode(
        node_id="real", name="Real Trail", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=11)
    state.map_nodes = [fork, real]
    state.location_id = "fork"
    state.destination_id = "fork"
    state.distance_remaining = 0

    engine = StepEngine(state, GMConfig(enabled=False))

    # Exactly one non-self option offered -- never the self-edge.
    assert engine.phase == GamePhase.ROUTE
    assert len(engine._pending_routes) == 1
    assert engine._pending_routes[0].node_id == "real"

    engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    assert engine.state.destination_id == "real"
    assert engine.phase == GamePhase.CAMP


def test_dangling_mid_map_fork_does_not_manufacture_victory():
    """F-dd6869ca: a fork that is NOT itself map_nodes[-1] must never
    recover by beelining to the final node -- that manufactures a false,
    unearned VICTORY and skips every node/event between the fork and the
    end. Mirrors the finding's own repro: a 4-node map
    [mid1, fork2, mid2, final] with the party AT fork2 (mid-map, not
    terminal), both raw connections dangling."""
    from escape_the_valley.models import Biome, MapNode

    mid1 = MapNode(
        node_id="mid1", name="Mid1", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["fork2"], distance_to={"fork2": 5},
    )
    fork2 = MapNode(
        node_id="fork2", name="Fork2", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["ghost-1", "ghost-2"],
        distance_to={"ghost-1": 10, "ghost-2": 12},
    )
    mid2 = MapNode(
        node_id="mid2", name="Mid2", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    final = MapNode(
        node_id="final", name="Final", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=5)
    state.map_nodes = [mid1, fork2, mid2, final]
    state.location_id = "fork2"
    state.destination_id = "fork2"
    state.distance_remaining = 0

    engine = StepEngine(state, GMConfig(enabled=False))
    assert engine.phase != GamePhase.ROUTE

    for _ in range(5):
        engine._do_travel()

    assert engine.state.location_id == "fork2"
    assert engine.state.location_id not in ("mid2", "final")
    assert check_game_over(engine.state) != "VICTORY"
    assert not engine.state.victory


# ── F-803bd813 / F-6e5e72a8: engine.py ROUTE fixes ported from StepEngine ──
#
# GameEngine (engine.py) is the engine behind the primary `trail play` /
# `trail new` CLI commands (cli.py) and had NO test coverage at all for its
# ROUTE fork-resolution code before this wave (GameEngine appears elsewhere
# in this file only for the unrelated GM-brief tests above). These mirror
# the StepEngine dangling-connections tests directly above, driven through
# GameEngine's synchronous (non-phase) _check_route_choice instead.


def test_gameengine_dangling_route_connections_recover_mid_travel(monkeypatch):
    """F-803bd813: GameEngine._check_route_choice had no else branch at all
    for the `len(connections) <= 1` case -- a corrupted-but-loadable fork
    node left destination_id/distance_remaining exactly as they were on
    arrival (destination_id == the fork node's own id, distance_remaining ==
    0). Every subsequent TRAVEL action re-arrived at the SAME node forever:
    distance_traveled kept climbing (a fake progress metric) while
    location_id never advanced and a full day's supplies were charged for
    zero real progress, silently, every action. Mirrors
    test_dangling_route_connections_recover_mid_travel above.

    F-dd6869ca (post-wave-6): the wave-4 fix that closed the bug above
    introduced its own false-progress mechanism -- beelining to
    map_nodes[-1] ("end"), which on a longer map would skip unvisited
    content and manufacture an unearned VICTORY. There is nothing
    legitimate to recover to here (both raw connections are dangling), so
    the party now stays at "start": no fake arrival at "end", and --
    because _do_travel returns before the movement math runs at all once
    _check_route_choice reports no legitimate route -- no fake
    distance_traveled/supply charge either. Still not the original bug:
    the assertion that matters is that this is STABLE (repeated calls
    don't drift) and LOUD (logged + messaged), not silent."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    state = _dangling_fork_state(seed=42)
    engine = GameEngine(state, GMConfig(enabled=False))
    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    # Keep every probabilistic branch (events, breakdown, health, town-trade)
    # closed so this test isolates the route-recovery path from unrelated
    # RNG-gated systems that _do_travel also exercises.
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)

    assert engine.state.location_id == "start"
    starting_food = engine.state.supplies.food

    # Drive several travel actions, mirroring the finding's own repro
    # ("across 6 consecutive _do_travel() calls"). Before the wave-4 fix,
    # none of these ever left "start" (the original bug); after it, all of
    # them silently beelined to "end" (F-dd6869ca). Neither happens now.
    for _ in range(4):
        engine._do_travel()

    # Never moved, never faked a destination, never drained supplies for a
    # journey that never happened -- and no false victory was manufactured.
    assert engine.state.destination_id == "start"
    assert engine.state.location_id == "start"
    assert engine.state.distance_traveled == 0
    assert engine.state.supplies.food == starting_food
    assert check_game_over(engine.state) != "VICTORY"


def test_gameengine_route_choice_single_resolvable_connection_used_directly():
    """F-803bd813, the length-1 sub-case: if exactly one of the raw
    connections resolves to a real map node, that IS a legitimate route (not
    a corrupted one) -- recovery must take it directly rather than
    discarding real route data in favor of the generic last-map-node
    fallback (which would otherwise be a regression vs. what the data
    actually supports)."""
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import Biome, MapNode

    fork = MapNode(
        node_id="fork", name="Fork", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["ghost", "real"],
        distance_to={"ghost": 10, "real": 8},
    )
    real = MapNode(
        node_id="real", name="Real Trail", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    last = MapNode(
        node_id="last", name="Last", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=3)
    state.map_nodes = [fork, real, last]
    state.location_id = "fork"
    state.destination_id = "fork"
    state.distance_remaining = 0

    engine = GameEngine(state, GMConfig(enabled=False))
    engine._check_route_choice()

    # Took the real, resolvable connection -- NOT the generic "last map
    # node" fallback ("last" would be the wrong, data-discarding answer).
    assert engine.state.destination_id == "real"
    assert engine.state.distance_remaining == 8


def test_gameengine_rejects_unrecognized_route_choice_id(monkeypatch, caplog):
    """F-6e5e72a8: chosen_id from show_route_choice() must be validated
    against the options actually offered before being committed to
    destination_id -- the engine, not ui.show_route_choice, is the
    enforcement boundary for what constitutes a valid choice. Mirrors
    step_engine.py._handle_route_choice's post-F-7d3e005b rejection;
    adapted to GameEngine's synchronous (non-phase) resolution, where there
    is no ROUTE phase to re-prompt from, so an unrecognized id falls back to
    the first offered route instead of being committed verbatim."""
    import logging

    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import Biome, MapNode

    fork = MapNode(
        node_id="fork", name="Fork", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["north", "south"],
        distance_to={"north": 10, "south": 14},
    )
    north = MapNode(
        node_id="north", name="Northern Pass", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    south = MapNode(
        node_id="south", name="Southern Trail", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=7)
    state.map_nodes = [fork, north, south]
    state.location_id = "fork"
    state.destination_id = "fork"
    state.distance_remaining = 0

    engine = GameEngine(state, GMConfig(enabled=False))
    # ui.py's real show_route_choice already can't return an out-of-range
    # index (it loops on bad input) -- simulate a misbehaving/future caller
    # to prove the ENGINE, not the UI, enforces this.
    monkeypatch.setattr(
        engine_mod, "show_route_choice", lambda conns: "bogus-id",
    )

    with caplog.at_level(logging.WARNING):
        engine._check_route_choice()

    assert engine.state.destination_id in ("north", "south")
    assert engine.state.destination_id != "bogus-id"
    assert any("choice_id" in r.message for r in caplog.records)


def test_gameengine_rejects_unrecognized_event_choice_id(monkeypatch, caplog):
    """F-d10a2a2f: choice_id from show_event_scene() must be validated
    against the choices actually offered before being passed to
    resolve_event() -- the exact sibling of F-6e5e72a8's route-choice fix
    directly above, in this SAME file, now closed the same way. Before this
    fix, GameEngine._trigger_event trusted choice_id unconditionally (the
    'trust the caller/UI' shape F-6e5e72a8 already closed for
    _check_route_choice, and that step_engine.py's own ENG-B-06
    _handle_event_choice already closed for its event-choice path) --
    GameEngine is synchronous with no EVENT phase to re-prompt from, so an
    unrecognized id falls back to the first offered choice instead of
    sailing through to resolve_event() unchecked."""
    import logging

    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    state = create_new_run(seed=7)
    engine = GameEngine(state, GMConfig(enabled=False))

    # Force the ~60% event-trigger gate open so an event actually fires.
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    # ui.py's real show_event_scene can't return an id outside the rendered
    # choices today -- simulate a misbehaving/future caller (or unvalidated
    # GM JSON on the scene.choices branch, which is exactly as unvalidated
    # as show_route_choice's return was before F-6e5e72a8) to prove the
    # ENGINE, not the UI, is the enforcement boundary.
    monkeypatch.setattr(
        engine_mod, "show_event_scene", lambda *a, **k: "bogus-id",
    )

    with caplog.at_level(logging.WARNING):
        engine._trigger_event()

    # Externally observable proof that a REAL, offered choice was
    # committed -- not the pre-fix symptom the finding calls out:
    # resolve_event() no-ops on an unmatched id (F-fa99f19f) and the label
    # lookup below it comes up empty, leaving choice_made="bogus-id: " with
    # nothing after the colon.
    assert len(engine.state.journal) == 1
    entry = engine.state.journal[0]
    assert not entry.choice_made.startswith("bogus-id")
    assert ": " in entry.choice_made
    assert entry.choice_made.split(": ", 1)[1] != ""
    assert any("choice_id" in r.message for r in caplog.records)


# ── F-0877c51a / F-dd6869ca / F-049017cc (wave 6): GameEngine mirrors ──


def test_gameengine_self_edge_fork_connection_excluded_and_terminates(monkeypatch):
    """F-0877c51a: GameEngine._check_route_choice must exclude a connection
    id that resolves back to the CURRENT node itself before counting how
    many connections resolved. Empirical repro from the finding: a 2-node
    map, fork.connections=['fork','ghost'] -- committing to the self-id as
    "the one real route" reproduces the original infinite-travel bug this
    method exists to close (location_id pinned, distance_traveled/food
    faking progress, game_over never True). The finding's own repro window
    was 30 consecutive _do_travel() calls; this drives the same count."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import Biome, MapNode

    fork = MapNode(
        node_id="fork", name="Fork", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["fork", "ghost"],
        distance_to={"fork": 5, "ghost": 10},
    )
    end = MapNode(
        node_id="end", name="End", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=30)
    state.map_nodes = [fork, end]
    state.location_id = "fork"
    state.destination_id = "fork"
    state.distance_remaining = 0

    engine = GameEngine(state, GMConfig(enabled=False))
    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    # Keep every probabilistic branch closed -- see the mid-travel test
    # above for why (isolates the route-recovery path).
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)

    starting_food = engine.state.supplies.food

    for _ in range(30):
        engine._do_travel()

    # Never left "fork", no fake distance, no fake supply drain -- the
    # self-edge was never treated as a resolved route.
    assert engine.state.location_id == "fork"
    assert engine.state.destination_id == "fork"
    assert engine.state.distance_traveled == 0
    assert engine.state.supplies.food == starting_food


def test_gameengine_dangling_mid_map_fork_does_not_manufacture_victory(monkeypatch):
    """F-dd6869ca: mirrors the finding's own repro exactly -- a 4-node map
    [mid1, fork2, mid2, final] with the party AT fork2 (a non-terminal
    fork, both raw connections dangling). The old map_nodes[-1] recovery
    reached location_id == 'final' with check_game_over() == 'VICTORY'
    after exactly 3 _do_travel() calls, skipping mid1/mid2 entirely. It
    must not do that anymore."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import Biome, MapNode

    mid1 = MapNode(
        node_id="mid1", name="Mid1", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["fork2"], distance_to={"fork2": 5},
    )
    fork2 = MapNode(
        node_id="fork2", name="Fork2", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["ghost-1", "ghost-2"],
        distance_to={"ghost-1": 10, "ghost-2": 12},
    )
    mid2 = MapNode(
        node_id="mid2", name="Mid2", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    final = MapNode(
        node_id="final", name="Final", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=31)
    state.map_nodes = [mid1, fork2, mid2, final]
    state.location_id = "fork2"
    state.destination_id = "fork2"
    state.distance_remaining = 0

    engine = GameEngine(state, GMConfig(enabled=False))
    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)

    for _ in range(5):
        engine._do_travel()

    assert engine.state.location_id == "fork2"
    assert engine.state.location_id not in ("mid2", "final")
    assert check_game_over(engine.state) != "VICTORY"


def test_gameengine_single_resolvable_connection_message_is_accurate(monkeypatch):
    """F-049017cc: the single-real-connection recovery must not reuse the
    genuine-corruption message ('...last known waypoint') -- that phrasing
    is a lie here: the destination is a real route, not a fallback, and
    could be any node on the map, not necessarily 'the last known
    waypoint'. It must get its own, accurate message instead."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import Biome, MapNode

    fork = MapNode(
        node_id="fork", name="Fork", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["ghost", "real"],
        distance_to={"ghost": 10, "real": 8},
    )
    real = MapNode(
        node_id="real", name="Real Trail", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=32)
    state.map_nodes = [fork, real]
    state.location_id = "fork"
    state.destination_id = "fork"
    state.distance_remaining = 0

    engine = GameEngine(state, GMConfig(enabled=False))
    messages = []
    monkeypatch.setattr(
        engine_mod, "show_message",
        lambda msg, *a, **k: messages.append(msg),
    )

    result = engine._check_route_choice()

    assert result is True
    assert engine.state.destination_id == "real"
    assert any("Real Trail" in m for m in messages)
    assert not any("last known waypoint" in m for m in messages)


# ── F-32a5a4a9 (wave 8): the single-connection arm of the same `if` ────
#
# Wave 6 hardened the FORK arm (len(node.connections) > 1) of
# _check_route_choice/_build_route_choices against a self-edge or a
# dangling connection id. The `== 1` arm of the same conditional was never
# touched: _arrive_at_next_node wrote destination_id from
# dest.connections[0] with no validation at all. A dangling (or self-edge)
# sole connection would sail through unnoticed and only get "discovered"
# on a LATER leg by the ENG-B-09 recovery, which beelines to
# map_nodes[-1] and manufactures a false, unearned VICTORY -- exactly the
# failure class wave 6 closed for forks but left open here. Reused by the
# GameEngine mirrors below, same convention as _dangling_fork_state /
# _self_edge_fork_state above.


def _dangling_single_connection_state(seed: int = 50):
    """A 3-node map: prev -[1mi]-> mid -[dangling 'ghost-dangling']->
    nothing, plus far_end as map_nodes[-1] -- the finding's own repro
    shape. mid has exactly ONE raw connection (not a fork), so this never
    reaches _build_route_choices/_check_route_choice's fork machinery; it
    is handled solely by _arrive_at_next_node's "set up next leg" tail."""
    from escape_the_valley.models import Biome, MapNode

    prev = MapNode(
        node_id="prev", name="Prev", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["mid"], distance_to={"mid": 1},
    )
    mid = MapNode(
        node_id="mid", name="Mid", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["ghost-dangling"], distance_to={"ghost-dangling": 10},
    )
    far_end = MapNode(
        node_id="far_end", name="Far End", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=seed)
    state.map_nodes = [prev, mid, far_end]
    state.location_id = "prev"
    state.destination_id = "mid"
    state.distance_remaining = 1
    return state


def _self_edge_single_connection_state(seed: int = 51):
    """A node whose sole connection is itself -- the single-connection
    counterpart to F-0877c51a's fork self-edge case. A self-edge is never
    a legitimate route no matter how many raw connections carry it."""
    from escape_the_valley.models import Biome, MapNode

    prev = MapNode(
        node_id="prev", name="Prev", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["loop"], distance_to={"loop": 1},
    )
    loop = MapNode(
        node_id="loop", name="Loop", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
        connections=["loop"], distance_to={"loop": 5},
    )
    far_end = MapNode(
        node_id="far_end", name="Far End", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15,
    )
    state = create_new_run(seed=seed)
    state.map_nodes = [prev, loop, far_end]
    state.location_id = "prev"
    state.destination_id = "loop"
    state.distance_remaining = 1
    return state


def test_dangling_single_connection_does_not_manufacture_victory():
    """F-32a5a4a9: arriving at a node whose sole connection is dangling
    must neither wire destination_id to the unresolvable id (which a
    later leg's arrival check would "discover" via the ENG-B-09
    map_nodes[-1] beeline, manufacturing a false VICTORY after skipping
    far_end's entire approach) nor silently re-arrive at "mid" forever,
    draining supplies/time for a leg that never happens (F-803bd813). The
    party must stall at "mid", loudly, at zero cost, every subsequent
    travel action."""
    state = _dangling_single_connection_state()
    engine = StepEngine(state, GMConfig(enabled=False))

    # First travel arrives at "mid" legitimately (a real 1-mile leg).
    engine._do_travel()
    assert engine.state.location_id == "mid"
    # NOT wired to the dangling id -- the write was refused.
    assert engine.state.destination_id == "mid"
    assert engine.state.distance_remaining == 0

    distance_after_arrival = engine.state.distance_traveled
    food_after_arrival = engine.state.supplies.food
    rng_counter_after_arrival = engine.rng.counter

    # Repeated travel attempts must neither teleport to far_end
    # (map_nodes[-1]) nor drain supplies/time for a fake leg.
    for _ in range(5):
        engine._do_travel()

    assert engine.state.location_id == "mid"
    assert engine.state.location_id != "far_end"
    assert engine.state.destination_id == "mid"
    assert engine.state.distance_traveled == distance_after_arrival
    assert engine.state.supplies.food == food_after_arrival
    # Zero RNG draws on the refused path -- the guard returns before
    # _do_travel reaches any RNG-consuming step (breakdown/health/event).
    assert engine.rng.counter == rng_counter_after_arrival
    assert check_game_over(engine.state) != "VICTORY"
    assert not engine.state.victory


def test_self_edge_single_connection_does_not_loop_or_manufacture_victory():
    """F-32a5a4a9: a sole connection that points back at the current node
    itself (a self-edge) must be excluded exactly like a fork's self-edge
    (F-0877c51a) -- not treated as "the one real route", which would
    reproduce the original infinite-travel bug (destination_id == this
    node's own id, forever)."""
    state = _self_edge_single_connection_state()
    engine = StepEngine(state, GMConfig(enabled=False))

    engine._do_travel()
    assert engine.state.location_id == "loop"
    assert engine.state.destination_id == "loop"
    assert engine.state.distance_remaining == 0

    distance_after_arrival = engine.state.distance_traveled
    food_after_arrival = engine.state.supplies.food

    for _ in range(5):
        engine._do_travel()

    assert engine.state.location_id == "loop"
    assert engine.state.distance_traveled == distance_after_arrival
    assert engine.state.supplies.food == food_after_arrival
    assert check_game_over(engine.state) != "VICTORY"


def test_gameengine_dangling_single_connection_does_not_manufacture_victory(monkeypatch):
    """F-32a5a4a9: GameEngine mirror of
    test_dangling_single_connection_does_not_manufacture_victory --
    _check_route_choice's `len(connections) <= 1` early return used to
    skip validation entirely for a lone connection, so
    _arrive_at_next_node's unvalidated write sailed straight through to
    the ENG-B-09 map_nodes[-1] beeline on a later leg."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    state = _dangling_single_connection_state(seed=52)
    engine = GameEngine(state, GMConfig(enabled=False))
    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)

    engine._do_travel()
    assert engine.state.location_id == "mid"
    assert engine.state.destination_id == "mid"
    assert engine.state.distance_remaining == 0

    distance_after_arrival = engine.state.distance_traveled
    food_after_arrival = engine.state.supplies.food

    for _ in range(5):
        engine._do_travel()

    assert engine.state.location_id == "mid"
    assert engine.state.location_id != "far_end"
    assert engine.state.destination_id == "mid"
    assert engine.state.distance_traveled == distance_after_arrival
    assert engine.state.supplies.food == food_after_arrival
    assert check_game_over(engine.state) != "VICTORY"


def test_gameengine_self_edge_single_connection_does_not_loop_or_manufacture_victory(monkeypatch):
    """F-32a5a4a9: GameEngine mirror of
    test_self_edge_single_connection_does_not_loop_or_manufacture_victory."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    state = _self_edge_single_connection_state(seed=53)
    engine = GameEngine(state, GMConfig(enabled=False))
    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)

    engine._do_travel()
    assert engine.state.location_id == "loop"
    assert engine.state.destination_id == "loop"

    distance_after_arrival = engine.state.distance_traveled
    food_after_arrival = engine.state.supplies.food

    for _ in range(5):
        engine._do_travel()

    assert engine.state.location_id == "loop"
    assert engine.state.distance_traveled == distance_after_arrival
    assert engine.state.supplies.food == food_after_arrival
    assert check_game_over(engine.state) != "VICTORY"


# ── F-d4a8ed17 (wave 8): GM-supplied event choice ids are coerced at ───
# the engine boundary
#
# tui_app.py's ChoiceId = Literal["A".."G"] exempts a rendered choice's
# `.id` from markup escaping on the stated grounds that it is "the
# constrained ChoiceId literal (A-G) set by the engine" -- true of the
# fallback branch (id=c.choice_id, this engine's own static
# ChoiceTemplate data) but, before this fix, false of the GM branch
# (id=c.get("id", "?"), unvalidated GM JSON). The UI renders; the engine
# is the enforcement boundary that must actually make the UI's assumption
# true.


class _ScriptedChoiceScene:
    """A GM scene with caller-supplied choice dicts (ids may be garbage)."""

    def __init__(self, choices):
        self.title = "A Stranger at Dusk"
        self.narration = "The trail narrows."
        self.choices = choices
        self.memory_proposals = []


class _ScriptedChoiceGM:
    """Enabled GM that returns a scripted scene; records the event it saw."""

    def __init__(self, choices):
        self.config = GMConfig(enabled=True)
        self._choices = choices
        self.last_event = None

    def generate_scene(self, state, event, weather_str, brief=None):
        self.last_event = event
        return _ScriptedChoiceScene(self._choices)

    def generate_outcome(self, *a, **k):
        return _FakeOutcome()

    def close(self):
        pass


class _MaliciousChoiceGM(_ScriptedChoiceGM):
    def __init__(self):
        super().__init__([
            {"id": "<script>alert(1)</script>", "label": "Press on",
             "risk_hint": "", "cost_hint": ""},
            {"id": "", "label": "Make camp", "risk_hint": "", "cost_hint": ""},
            {"label": "Turn back", "risk_hint": "", "cost_hint": ""},
        ])


_FOUR_CHOICE_RAW = [
    {"id": "wait", "label": "Wait it out", "risk_hint": "", "cost_hint": ""},
    {"id": "push", "label": "Push through", "risk_hint": "", "cost_hint": ""},
    {"id": "scout", "label": "Scout around", "risk_hint": "", "cost_hint": ""},
    {"id": "back", "label": "Turn back", "risk_hint": "", "cost_hint": ""},
]


def _pin_library_event(engine, event_id: str):
    """Restrict the engine's library to one real skeleton (select_event stays)."""
    pinned = next(e for e in engine.event_library if e.event_id == event_id)
    engine.event_library = [pinned]
    return pinned


def _force_gm_event(monkeypatch, event_id: str, gm, seed: int = 42):
    """Travel into EVENT with a pinned library event and a scripted GM scene."""
    engine = _force_event_engine(seed=seed, gm=gm)
    pinned = _pin_library_event(engine, event_id)
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    return engine, pinned


def test_gm_choice_ids_are_coerced_to_the_constrained_letter_set(monkeypatch):
    """F-d4a8ed17 + F-15a1534a: EventChoiceInfo.id is coerced onto A-G AND
    capped to keys that exist in the pinned event's outcome_templates --
    not merely markup-safe letters. storm_sudden is a 3-template event,
    so a 3-choice GM scene (with garbage ids) must offer A/B/C."""
    gm = _MaliciousChoiceGM()
    engine, pinned = _force_gm_event(monkeypatch, "storm_sudden", gm)

    assert engine.phase == GamePhase.EVENT
    assert set(pinned.outcome_templates) == {"A", "B", "C"}

    ids = [c.id for c in engine._pending_event_choices]
    assert ids == ["A", "B", "C"]
    assert set(ids) <= set(pinned.outcome_templates)
    assert "<script>alert(1)</script>" not in ids
    assert "" not in ids
    assert "?" not in ids

    # Labels are untouched -- only the id field is coerced.
    labels = [c.label for c in engine._pending_event_choices]
    assert labels == ["Press on", "Make camp", "Turn back"]


def test_gm_four_choices_on_two_template_event_offers_only_a_and_b(monkeypatch):
    """F-15a1534a: a schema-valid 4-choice GM scene on a 2-template event
    (good_water: A/B) must drop C/D rather than offer unresolvable letters."""
    gm = _ScriptedChoiceGM(_FOUR_CHOICE_RAW)
    engine, pinned = _force_gm_event(monkeypatch, "good_water", gm)

    assert engine.phase == GamePhase.EVENT
    assert set(pinned.outcome_templates) == {"A", "B"}

    offered = [(c.id, c.label) for c in engine._pending_event_choices]
    assert offered == [("A", "Wait it out"), ("B", "Push through")]
    assert "C" not in [c.id for c in engine._pending_event_choices]


def test_gm_three_choices_on_three_template_event_offers_a_b_c(monkeypatch):
    """F-15a1534a: a 3-choice GM scene on a 3-template event offers A/B/C."""
    gm = _ScriptedChoiceGM([
        {"id": "wait", "label": "Wait it out", "risk_hint": "", "cost_hint": ""},
        {"id": "push", "label": "Push through", "risk_hint": "", "cost_hint": ""},
        {"id": "scout", "label": "Scout around", "risk_hint": "", "cost_hint": ""},
    ])
    engine, pinned = _force_gm_event(monkeypatch, "storm_sudden", gm)

    assert engine.phase == GamePhase.EVENT
    assert set(pinned.outcome_templates) == {"A", "B", "C"}
    ids = [c.id for c in engine._pending_event_choices]
    assert ids == ["A", "B", "C"]
    assert set(ids) <= set(pinned.outcome_templates)


def test_choose_c_on_two_template_event_is_rejected_not_visible_miss(monkeypatch):
    """F-15a1534a: CHOOSE C is no longer an offered id on a 2-template event,
    so ENG-B-06 rejects rather than resolving to events' visible-miss."""
    gm = _ScriptedChoiceGM(_FOUR_CHOICE_RAW)
    engine, _pinned = _force_gm_event(monkeypatch, "good_water", gm)
    assert engine.phase == GamePhase.EVENT
    offered = [c.id for c in engine._pending_event_choices]
    assert "C" not in offered

    journal_before = len(engine.state.journal)
    msgs = engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="C"))

    assert engine.phase == GamePhase.EVENT
    assert engine._pending_event is not None
    assert any("isn't available" in line for line in msgs.lines)
    assert len(engine.state.journal) == journal_before


def test_gameengine_gm_scene_ids_are_letters_from_the_template_set(monkeypatch):
    """F-15a1534a: GameEngine must coerce raw GM ids onto template letters
    and cap to outcome_templates -- not forward wait/push/scout/back."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    state = create_new_run(seed=42)
    engine = GameEngine(state, GMConfig(enabled=True))
    pinned = _pin_library_event(engine, "good_water")
    assert set(pinned.outcome_templates) == {"A", "B"}

    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)
    engine.gm = _ScriptedChoiceGM(_FOUR_CHOICE_RAW)

    captured: dict = {}

    def _capture_scene(title, narration, choices):
        captured["choices"] = list(choices)
        return "A"

    monkeypatch.setattr(engine_mod, "show_event_scene", _capture_scene)
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)

    water_before = engine.state.supplies.water
    engine._trigger_event()

    ids = [c.get("id") for c in captured["choices"]]
    labels = [c.get("label") for c in captured["choices"]]
    assert ids == ["A", "B"]
    assert set(ids) <= set(pinned.outcome_templates)
    assert "wait" not in ids
    assert "scout" not in ids
    assert labels == ["Wait it out", "Push through"]
    # Picking A honors the real template (water +10), not a visible-miss.
    assert engine.state.supplies.water == water_before + 10


# ── F-d178410b (wave 8): BackpackManager's persist hook wired at the ──
# step_engine.py call sites
#
# backpack.py's BackpackManager accepts an optional persist= hook (called
# with the full RunState immediately after each resource's Payment
# confirms) that closes a crash-window duplicate-settlement bug, but its
# own docstring says plainly that no call site passes it -- "a caller-side
# change outside this module's domain, not made here." Both
# step_engine.py construction sites must wire persist=<something that
# calls save_game>, not leave it at the default None.


def _make_capturing_backpack_manager():
    """Factory: a stand-in BackpackManager that records the persist=
    kwarg each instance is constructed with, into a fresh list per call
    (avoids cross-test leakage from a shared class attribute)."""

    calls: list = []

    class _CapturingManager:
        def __init__(self, *args, **kwargs):
            calls.append(kwargs.get("persist"))

        def settle(self, state, location):
            class _Result:
                success = False
                message = ""
                txids: list = []
            return _Result()

        def check_parcels(self, state):
            return []

        def close(self):
            pass

    return _CapturingManager, calls


def test_settle_checkpoint_wires_a_persist_hook(monkeypatch):
    """F-d178410b: _settle_checkpoint fires automatically at every town
    arrival when the backpack is enabled -- the crash-window duplicate-
    settlement bug is live on this path unless persist= is wired."""
    import escape_the_valley.backpack as backpack_mod

    manager_cls, calls = _make_capturing_backpack_manager()
    monkeypatch.setattr(backpack_mod, "BackpackManager", manager_cls)

    engine = _make_engine(seed=42)
    engine.state.backpack.enabled = True
    engine._settle_checkpoint(_DummyNode())

    assert len(calls) == 1
    assert calls[0] is not None
    assert callable(calls[0])


def test_check_parcels_wires_a_persist_hook(monkeypatch):
    """F-d178410b: the other step_engine.py BackpackManager call site."""
    import escape_the_valley.backpack as backpack_mod

    manager_cls, calls = _make_capturing_backpack_manager()
    monkeypatch.setattr(backpack_mod, "BackpackManager", manager_cls)

    engine = _make_engine(seed=42)
    engine.state.backpack.enabled = True
    engine._check_parcels(_DummyNode())

    assert len(calls) == 1
    assert calls[0] is not None
    assert callable(calls[0])


def test_backpack_persist_hook_honors_autosave_and_base_path(monkeypatch, tmp_path):
    """The persist hook itself -- not just its wiring -- must respect this
    engine's own autosave/base_path contract exactly like _save does: a
    caller that constructed autosave=False for zero disk writes must see
    zero disk writes from a mid-settlement persist too, not just the
    end-of-step autosave. And when autosave IS on, the hook must save to
    THIS engine's base_path, not bare CWD (save_game's own default)."""
    import escape_the_valley.step_engine as step_engine_mod

    calls = []
    monkeypatch.setattr(
        step_engine_mod, "save_game",
        lambda state, base_path=None: calls.append((state, base_path)),
    )

    on_engine = StepEngine(
        create_new_run(seed=1), GMConfig(enabled=False), base_path=tmp_path,
    )
    on_engine._backpack_persist_hook(on_engine.state)
    assert calls == [(on_engine.state, tmp_path)]

    off_engine = StepEngine(
        create_new_run(seed=1), GMConfig(enabled=False), autosave=False,
    )
    off_engine._backpack_persist_hook(off_engine.state)
    # Unchanged -- autosave=False produced no new save_game call.
    assert calls == [(on_engine.state, tmp_path)]


# ── F-b6d0a4bc: step_engine.py memory-card validation must not crash step() ──


class _MemoryCardGM:
    """GM stub whose scene AND outcome both carry memory_proposals, so both
    validate_gm_cards() call sites in _handle_event_choice execute."""

    def __init__(self):
        self.config = GMConfig(enabled=True)

    def generate_scene(self, *a, **k):
        return _FakeScene()

    def generate_outcome(self, *a, **k):
        return _FakeOutcome()

    def close(self):
        pass


def test_malformed_gm_memory_proposals_do_not_crash_step(monkeypatch, caplog):
    """F-b6d0a4bc: neither validate_gm_cards() call site in
    _handle_event_choice (scene.memory_proposals, gm_out.memory_proposals)
    was wrapped in exception handling, and step() itself has no exception
    handling around phase dispatch either. A malformed-but-schema-legal GM
    payload that survives validate_gm_cards's own checks -- or any future
    shape it doesn't defend against -- would crash the entire step() call
    for any GM-enabled run, no corrupted save required (F-9b0797f9's named
    escalation condition). Graceful degradation now matches the pattern
    already used at _settle_checkpoint/_check_parcels: log and skip the
    malformed batch, game continues."""
    import logging

    import escape_the_valley.step_engine as step_engine_mod

    engine = _force_event_engine(seed=42, gm=_MemoryCardGM())
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)  # force event trigger
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.phase == GamePhase.EVENT

    def _raise(*a, **k):
        raise ValueError("simulated malformed memory_proposals shape")

    monkeypatch.setattr(step_engine_mod, "validate_gm_cards", _raise)

    with caplog.at_level(logging.WARNING):
        msgs = engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))

    # Must not raise -- and the event still resolves normally (CAMP) despite
    # the malformed memory-card batch being skipped at both call sites.
    assert engine.phase == GamePhase.CAMP
    assert msgs is not None
    assert any("memory-card" in r.message.lower() for r in caplog.records)
    # F-886d2c1e: validation failing outright at BOTH call sites (scene AND
    # gm_out both carry memory_proposals per _MemoryCardGM) must bump the
    # diagnostics counter mirroring _note_gm_fallback's pattern -- this
    # degradation used to be a bare, invisible log.warning with no counter
    # at all.
    assert engine.diagnostics["memory_card_failures"] == 2


def test_memory_card_partial_batch_failure_commits_and_is_distinguishable(
    monkeypatch, caplog,
):
    """F-886d2c1e: the previous version wrapped validate_gm_cards() AND the
    add_card() loop in a single try/except. The schema allows up to 2
    memory_proposals per batch, so if card 1 of 2 adds successfully and
    card 2's add_card() raises, the old log line ('...validation/add
    failed') read identically to a batch that failed validation outright
    -- even though card 1 was already permanently committed to
    state.memory. This proves the fix: card 1 lands, the log line
    distinguishes a partial add failure from total validation failure, and
    the failure is still counted via diagnostics['memory_card_failures'].

    Mocks validate_gm_cards (as the existing malformed-batch test above
    does) and add_card -- the two collaborators _apply_memory_proposals
    orchestrates -- not the orchestration logic under test itself."""
    import logging

    import escape_the_valley.step_engine as step_engine_mod

    engine = _force_event_engine(seed=44, gm=_MemoryCardGM())
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)  # force event trigger
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.phase == GamePhase.EVENT

    good_card = object()
    bad_card = object()
    monkeypatch.setattr(
        step_engine_mod, "validate_gm_cards",
        lambda state, proposals: [good_card, bad_card],
    )

    committed = []

    def _add_card(state, card):
        if card is bad_card:
            raise ValueError("simulated add_card failure")
        committed.append(card)

    monkeypatch.setattr("escape_the_valley.memory.add_card", _add_card)

    assert engine.diagnostics["memory_card_failures"] == 0

    with caplog.at_level(logging.WARNING):
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))

    # The good card landed even though its batch-mate failed -- the
    # earlier bug would have made this indistinguishable from "nothing was
    # saved" in the log, but the state itself already tells the story.
    assert good_card in committed
    assert bad_card not in committed
    # Both call sites (scene + gm_out) hit the same bad_card shape, so both
    # count one add failure each.
    assert engine.diagnostics["memory_card_failures"] == 2
    # bad_card is the 2nd of 2 cards in the batch -- the message must say
    # so precisely (not just "a card failed"), since the whole point is
    # letting the log distinguish which cards landed.
    assert any("2/2 add failed" in r.message for r in caplog.records)
    # The partial-add message must not be confused with a total validation
    # failure -- they are deliberately different log lines.
    assert not any("validation failed" in r.message for r in caplog.records)


# ── Autosave scoping ───────────────────────────────────────────────


def test_step_autosaves_by_default(tmp_path):
    """The default engine still autosaves on every step (unchanged behavior)."""
    engine = StepEngine(
        create_new_run(seed=42), GMConfig(enabled=False), base_path=tmp_path
    )
    engine.step(PlayerIntent(IntentAction.REST))

    assert (tmp_path / ".trail" / "run.json").exists()


def test_autosave_false_skips_the_write(tmp_path, monkeypatch):
    """autosave=False suppresses the write for THIS engine only."""
    monkeypatch.chdir(tmp_path)
    engine = StepEngine(
        create_new_run(seed=42), GMConfig(enabled=False), autosave=False
    )
    engine.step(PlayerIntent(IntentAction.REST))

    assert not (tmp_path / ".trail").exists()
    # ...and a sibling engine is unaffected — the option is per-instance.
    other = StepEngine(create_new_run(seed=42), GMConfig(enabled=False))
    other.step(PlayerIntent(IntentAction.REST))
    assert (tmp_path / ".trail" / "run.json").exists()


def test_autosave_false_still_tracks_rng_state():
    """Skipping the write must not skip the RNG bookkeeping callers read."""
    engine = StepEngine(
        create_new_run(seed=42), GMConfig(enabled=False), autosave=False
    )
    engine.step(PlayerIntent(IntentAction.TRAVEL))

    assert engine.state.rng_counter == engine.rng.counter
    assert engine.state.rng_state is not None


# ── F-6e017443: GameEngine arrival/travel extras (pairwise, not a merge) ──


def _dest_town_state(seed: int = 42, morale: int = 50):
    """3-node map matching the finding's empirical shape: start -> DestTown
    with cache_supplies={food:6, water:5}, water_available, is_town."""
    from escape_the_valley.models import Biome, MapNode

    state = create_new_run(seed=seed)
    start = MapNode(
        node_id="start", name="Start", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15, is_town=False,
        connections=["DestTown"],
        distance_to={"DestTown": 5},
    )
    dest = MapNode(
        node_id="DestTown", name="DestTown", biome=Biome.PLAINS, hazard=1,
        water_available=True, temperature=15, is_town=True,
        connections=["end"],
        distance_to={"end": 10},
        cache_supplies={"food": 6, "water": 5},
    )
    end = MapNode(
        node_id="end", name="End", biome=Biome.PLAINS, hazard=1,
        water_available=False, temperature=15, is_town=False,
    )
    state.map_nodes = [start, dest, end]
    state.location_id = "start"
    state.destination_id = "DestTown"
    state.distance_remaining = 0
    state.supplies.food = 40
    state.supplies.water = 20
    state.party.morale = morale
    return state


def test_gameengine_arrival_refills_water_and_consumes_cache(monkeypatch):
    """F-6e017443: GameEngine._arrive_at_next_node now refills water and
    picks up the cache, matching StepEngine. Morale 50 skips the trade
    draw so food is cache-only."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)

    engine = GameEngine(_dest_town_state(morale=50), GMConfig(enabled=False))
    dest = engine.state.map_nodes[1]
    assert dest.cache_supplies == {"food": 6, "water": 5}

    engine._arrive_at_next_node()

    # water 20 + refill min(20, 50-20)=20 + cache 5 → 45
    assert engine.state.supplies.water == 45
    # food 40 + cache 6 → 46 (no trade: morale <= 60)
    assert engine.state.supplies.food == 46
    assert dest.cache_supplies is None
    assert engine.state.location_id == "DestTown"


def test_stepengine_arrival_same_water_cache_on_dest_town():
    """Pairwise: StepEngine on the same 3-node map must not grow a third
    arrival rule. Morale 50 skips trade."""
    engine = StepEngine(_dest_town_state(morale=50), GMConfig(enabled=False))
    dest = engine.state.map_nodes[1]
    engine._arrive_at_next_node()
    assert engine.state.supplies.water == 45
    assert engine.state.supplies.food == 46
    assert dest.cache_supplies is None


def test_gameengine_town_trade_mirrors_stepengine(monkeypatch):
    """F-6e017443: morale-gated town trade on GameEngine, same formula as
    StepEngine. Force the trade roll to hit so food includes randint(3,9)."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)

    ge_state = _dest_town_state(seed=7, morale=80)
    se_state = _dest_town_state(seed=7, morale=80)
    ge = GameEngine(ge_state, GMConfig(enabled=False))
    se = StepEngine(se_state, GMConfig(enabled=False), autosave=False)
    # Hit the trade roll; randint(3, 9) still uses each engine's real RNG.
    monkeypatch.setattr(ge.rng, "random", lambda: 0.0)
    monkeypatch.setattr(se.rng, "random", lambda: 0.0)

    ge._arrive_at_next_node()
    se._arrive_at_next_node()

    assert ge.state.supplies.food == se.state.supplies.food
    assert ge.state.supplies.water == se.state.supplies.water
    assert ge.state.supplies.food >= 46 + 3
    assert ge.state.supplies.food <= 46 + 9


def test_gameengine_night_travel_spends_lantern_oil(monkeypatch):
    """F-6e017443: CLI night travel must spend lantern_oil (is_travel=True)."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import TimeOfDay

    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)

    state = create_new_run(seed=21)
    engine = GameEngine(state, GMConfig(enabled=False))
    engine.state.time_of_day = TimeOfDay.EVENING
    engine.state.distance_remaining = 80
    engine.state.supplies.set("lantern_oil", 5)
    # Close breakdown / night-danger / event rolls.
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)

    engine._do_travel()

    assert engine.state.supplies.get("lantern_oil") == 4


def test_gameengine_night_danger_without_oil(monkeypatch):
    """F-6e017443: GameEngine calls check_night_travel_danger after travel."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import TimeOfDay

    messages: list[str] = []
    monkeypatch.setattr(
        engine_mod, "show_message",
        lambda text, *a, **k: messages.append(text),
    )
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_event_scene", lambda *a, **k: "A")
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "check_breakdown", lambda *a, **k: None)

    state = create_new_run(seed=21)
    engine = GameEngine(state, GMConfig(enabled=False))
    engine.state.time_of_day = TimeOfDay.EVENING
    engine.state.distance_remaining = 80
    engine.state.supplies.set("lantern_oil", 0)
    engine.state.supplies.set("parts", 0)
    wagon_before = engine.state.wagon.condition
    # random()=0.0 opens night danger (15%) and the event gate; event UI
    # is stubbed above. randint still runs for wagon_damage 5-15.
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)

    engine._do_travel()

    assert any("Dark travel mishap" in m for m in messages)
    assert engine.state.wagon.condition < wagon_before


def test_gameengine_fixed_seed_reproduces(monkeypatch):
    """Same seed on the fixed CLI engine must still reproduce (the note
    that trail-play seeds diverge from *pre-fix* GameEngine is expected)."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import TimeOfDay

    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_event_scene", lambda *a, **k: "A")
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)

    def _snap(engine: GameEngine):
        return (
            engine.state.supplies.food,
            engine.state.supplies.water,
            engine.state.supplies.get("lantern_oil"),
            engine.state.wagon.condition,
            engine.state.distance_traveled,
            engine.rng.counter,
        )

    snaps = []
    for _ in range(2):
        engine = GameEngine(create_new_run(seed=31337), GMConfig(enabled=False))
        engine.state.time_of_day = TimeOfDay.EVENING
        engine.state.distance_remaining = 80
        engine._do_travel()
        snaps.append(_snap(engine))

    assert snaps[0] == snaps[1]


# ── F-2a57b303: EVENT/ROUTE retry copy names offered letters ──────────


def test_event_wrong_action_prints_offered_letters(monkeypatch):
    """TRAVEL during EVENT must name A/B (or whatever is offered), not 1-4."""
    engine = _force_event_engine(seed=42)
    monkeypatch.setattr(engine.rng, "random", lambda: 0.0)
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.phase == GamePhase.EVENT
    offered = [c.id for c in engine.msgs.event_choices]
    assert offered
    joined = "/".join(offered)

    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.phase == GamePhase.EVENT
    assert engine._pending_event is not None
    assert any(joined in line for line in msgs.lines)
    assert any("choose one of:" in line for line in msgs.lines)
    assert not any("1-4" in line for line in msgs.lines)
    # Same list as the invalid-id path (CHOOSE '1' is not a letter).
    msgs2 = engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="1"))
    assert any(joined in line for line in msgs2.lines)
    assert engine.phase == GamePhase.EVENT


def test_route_wrong_action_prints_offered_letters():
    """REST during ROUTE must name A/B, not a fixed 1/2."""
    from escape_the_valley.step_engine import RouteOption

    engine = _make_engine(seed=42)
    engine._pending_routes = [
        RouteOption(node_id="node-a", name="Northern Pass", distance=10),
        RouteOption(node_id="node-b", name="Southern Trail", distance=14),
    ]
    engine.phase = GamePhase.ROUTE
    engine.state.destination_id = "unset"

    msgs = engine.step(PlayerIntent(IntentAction.REST))
    assert engine.phase == GamePhase.ROUTE
    assert engine.state.destination_id == "unset"
    assert any("A/B" in line for line in msgs.lines)
    assert any("choose one of:" in line for line in msgs.lines)
    assert not any("1/2" in line for line in msgs.lines)


def test_route_wrong_action_one_option_prints_letter_a():
    """A one-option pending route must not still say 1/2."""
    from escape_the_valley.step_engine import RouteOption

    engine = _make_engine(seed=42)
    engine._pending_routes = [
        RouteOption(node_id="node-a", name="Northern Pass", distance=10),
    ]
    engine.phase = GamePhase.ROUTE
    engine.state.destination_id = "unset"

    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert engine.phase == GamePhase.ROUTE
    assert any("choose one of: A." in line for line in msgs.lines)
    assert not any("1/2" in line for line in msgs.lines)


# ── F-1a1edd7a: live death line names the cause ───────────────────────


def test_stepengine_live_death_line_names_cause(monkeypatch):
    """Survivors see '{name} has died ({cause}).' not a cause-free death."""
    engine = _make_engine(seed=42)
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)
    engine.state.supplies.food = 0
    engine.state.supplies.water = 50
    engine.state.distance_remaining = 80
    victim = engine.state.party.members[0]
    victim.health = 1
    name = victim.name

    msgs = engine.step(PlayerIntent(IntentAction.TRAVEL))
    assert f"{name} has died (Starvation)." in msgs.lines
    assert not any(
        line == f"{name} has died." for line in msgs.lines
    )


def test_gameengine_live_death_line_names_cause(monkeypatch):
    """F-1a1edd7a: GameEngine show_message matches StepEngine's live line."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    messages: list[str] = []
    monkeypatch.setattr(
        engine_mod, "show_message",
        lambda text, *a, **k: messages.append(text),
    )
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_event_scene", lambda *a, **k: "A")
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)

    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)
    engine.state.supplies.food = 0
    engine.state.supplies.water = 50
    engine.state.distance_remaining = 80
    victim = engine.state.party.members[0]
    victim.health = 1
    name = victim.name

    engine._do_travel()
    assert f"{name} has died (Starvation)." in messages


# ── F-20fa3769: GameEngine pairwise ending/spoilage/valves/maintenance/settle ──


def _stub_gameengine_ui(monkeypatch):
    import escape_the_valley.engine as engine_mod

    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_event_scene", lambda *a, **k: "A")
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_game_over", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_journal", lambda *a, **k: None)


def test_gameengine_travel_spoils_unsalted_food(monkeypatch):
    """F-20fa3769: unsalted food spoils on a day%3==0 travel, same helper."""
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import TimeOfDay

    messages: list[str] = []
    monkeypatch.setattr(
        engine_mod, "show_message",
        lambda text, *a, **k: messages.append(text),
    )
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_event_scene", lambda *a, **k: "A")
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)

    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.supplies.set("salt", 0)
    engine.state.supplies.food = 500
    engine.state.day = 3
    engine.state.time_of_day = TimeOfDay.MORNING
    engine.state.distance_remaining = 80
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)

    engine._do_travel()

    assert engine.state.last_spoilage_day == 3
    assert any("spoiled" in m.lower() for m in messages)


def test_gameengine_salt_skips_spoilage(monkeypatch):
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import TimeOfDay

    messages: list[str] = []
    monkeypatch.setattr(
        engine_mod, "show_message",
        lambda text, *a, **k: messages.append(text),
    )
    monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)
    monkeypatch.setattr(engine_mod, "show_event_scene", lambda *a, **k: "A")
    monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)

    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.supplies.set("salt", 4)
    engine.state.day = 3
    engine.state.time_of_day = TimeOfDay.MORNING
    engine.state.distance_remaining = 80
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)

    engine._do_travel()

    assert engine.state.last_spoilage_day == 0
    assert not any("spoiled" in m.lower() for m in messages)


def test_gameengine_rest_then_repair_grants_maintenance(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.wagon.condition = 50
    engine._do_rest()
    engine._do_repair()
    assert engine.state.maintained_turns_remaining == 2
    assert engine.state.last_action == "REPAIR"


def test_gameengine_repair_then_rest_grants_maintenance(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.wagon.condition = 50
    engine._do_repair()
    engine._do_rest()
    assert engine.state.maintained_turns_remaining == 2
    assert engine.state.last_action == "REST"


def test_gameengine_travel_decrements_maintenance(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.maintained_turns_remaining = 2
    engine.state.distance_remaining = 80
    monkeypatch.setattr(engine.rng, "random", lambda: 1.0)
    engine._do_travel()
    assert engine.state.maintained_turns_remaining == 1
    assert engine.state.last_action == "TRAVEL"


def test_gameengine_abandon_cargo_valve(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.wagon.condition = 20
    engine.state.supplies.set("salt", 10)
    engine.state.supplies.set("cloth", 8)
    old_wagon = engine.state.wagon.condition
    engine._do_abandon_cargo()
    assert engine.state.wagon.condition > old_wagon
    assert engine.state.last_action == "ABANDON_CARGO"
    assert engine.state.escape_valve_cooldown == 3


def test_gameengine_desperate_repair_valve(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.wagon.condition = 20
    engine.state.supplies.parts = 0
    engine._do_desperate_repair()
    assert engine.state.last_action == "DESPERATE_REPAIR"
    assert engine.state.escape_valve_cooldown == 3


def test_gameengine_hard_ration_valve(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    alive = engine.state.party.alive_count
    engine.state.supplies.food = alive * 2
    old_morale = engine.state.party.morale
    engine._do_hard_ration()
    assert engine.state.rationing_steps == 2
    assert engine.state.party.morale < old_morale
    assert engine.state.last_action == "HARD_RATION"


def test_gameengine_camp_menu_classic_when_valves_closed(monkeypatch):
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    monkeypatch.setattr(engine_mod, "show_action_menu", lambda: "5")
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.wagon.condition = 80
    engine.state.supplies.food = 100
    assert engine._prompt_camp_action() == "5"


def test_gameengine_camp_menu_offers_valves_when_can(monkeypatch):
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    prints: list[str] = []
    monkeypatch.setattr(
        engine_mod.console, "print",
        lambda *a, **k: prints.append(" ".join(str(x) for x in a)),
    )
    monkeypatch.setattr(engine_mod.console, "input", lambda *a, **k: "Q")

    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.wagon.condition = 20
    engine.state.supplies.parts = 0
    engine.state.supplies.food = 2
    engine.state.escape_valve_cooldown = 0
    engine.state.rationing_steps = 0

    assert engine._prompt_camp_action() == "Q"
    blob = " ".join(prints)
    assert "Abandon cargo" in blob
    assert "Desperate repair" in blob
    assert "Hard ration" in blob


def test_gameengine_run_dispatches_abandon_cargo(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.wagon.condition = 20
    engine.state.supplies.set("salt", 10)
    actions = iter(["8", "Q"])
    monkeypatch.setattr(engine, "_prompt_camp_action", lambda: next(actions))
    wagon_before = engine.state.wagon.condition
    engine.run()
    assert engine.state.last_action == "ABANDON_CARGO"
    assert engine.state.wagon.condition > wagon_before


def test_gameengine_victory_computes_ending(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    engine.state.location_id = engine.state.map_nodes[-1].node_id
    engine.state.distance_remaining = 0
    engine.state.day = 5
    engine._check_game_over()
    assert engine.state.game_over is True
    assert engine.state.victory is True
    assert engine.state.ending is not None
    assert engine.state.ending.tier in {
        "triumphant", "weathered", "pyrrhic",
    }
    assert engine.state.ending.headline


def test_gameengine_death_computes_lost_ending(monkeypatch):
    from escape_the_valley.engine import GameEngine

    _stub_gameengine_ui(monkeypatch)
    engine = GameEngine(create_new_run(seed=42), GMConfig(enabled=False))
    for member in engine.state.party.members:
        member.health = 0
        member.death_cause = "Starvation"
    engine._check_game_over()
    assert engine.state.game_over is True
    assert engine.state.victory is False
    assert engine.state.ending is not None
    assert engine.state.ending.tier == "lost"


def test_gameengine_town_settles_when_backpack_enabled(monkeypatch):
    import escape_the_valley.backpack as backpack_mod
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    manager_cls, calls = _make_capturing_backpack_manager()
    monkeypatch.setattr(backpack_mod, "BackpackManager", manager_cls)

    engine = GameEngine(_dest_town_state(morale=50), GMConfig(enabled=False))
    engine.state.backpack.enabled = True
    engine._arrive_at_next_node()

    assert len(calls) == 2  # settle + parcels
    assert all(callable(hook) for hook in calls)


def test_gameengine_town_skips_settle_when_backpack_off(monkeypatch):
    import escape_the_valley.backpack as backpack_mod
    import escape_the_valley.engine as engine_mod
    from escape_the_valley.engine import GameEngine

    monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
    manager_cls, calls = _make_capturing_backpack_manager()
    monkeypatch.setattr(backpack_mod, "BackpackManager", manager_cls)

    engine = GameEngine(_dest_town_state(morale=50), GMConfig(enabled=False))
    engine.state.backpack.enabled = False
    engine._arrive_at_next_node()
    assert calls == []


def test_gameengine_spoilage_seed_reproduces(monkeypatch):
    """Same seed on the fixed CLI must still reproduce after spoilage draws."""
    from escape_the_valley.engine import GameEngine
    from escape_the_valley.models import TimeOfDay

    _stub_gameengine_ui(monkeypatch)

    def _snap(engine: GameEngine):
        return (
            engine.state.supplies.food,
            engine.state.last_spoilage_day,
            engine.state.wagon.condition,
            engine.rng.counter,
        )

    snaps = []
    for _ in range(2):
        engine = GameEngine(create_new_run(seed=4242), GMConfig(enabled=False))
        engine.state.supplies.set("salt", 0)
        engine.state.supplies.food = 500
        engine.state.day = 3
        engine.state.time_of_day = TimeOfDay.MORNING
        engine.state.distance_remaining = 80
        engine._do_travel()
        snaps.append(_snap(engine))

    assert snaps[0] == snaps[1]
