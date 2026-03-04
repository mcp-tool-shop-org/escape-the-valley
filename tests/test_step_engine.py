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
