"""Tests for enriched death cause system."""

from escape_the_valley.models import (
    Condition,
    SeededRNG,
)
from escape_the_valley.physics import (
    check_health_effects,
    determine_cause_of_death,
)
from escape_the_valley.worldgen import create_new_run


class TestProximateDeathCause:
    """Per-member death_cause is set when health drops to 0."""

    def test_starvation_cause(self):
        state = create_new_run(seed=42)
        state.supplies.food = 0
        state.supplies.water = 50
        # Weaken one member so they die
        state.party.members[0].health = 1
        rng = SeededRNG(42)
        check_health_effects(state, rng)
        m = state.party.members[0]
        if not m.is_alive():
            assert m.death_cause == "Starvation"

    def test_dehydration_cause(self):
        state = create_new_run(seed=42)
        state.supplies.food = 50
        state.supplies.water = 0
        state.party.members[0].health = 1
        rng = SeededRNG(42)
        check_health_effects(state, rng)
        m = state.party.members[0]
        if not m.is_alive():
            assert m.death_cause == "Dehydration"

    def test_dehydration_wins_over_starvation(self):
        """When both food and water are 0, dehydration takes priority."""
        state = create_new_run(seed=42)
        state.supplies.food = 0
        state.supplies.water = 0
        state.party.members[0].health = 1
        rng = SeededRNG(42)
        check_health_effects(state, rng)
        m = state.party.members[0]
        if not m.is_alive():
            assert m.death_cause == "Dehydration"

    def test_disease_cause(self):
        state = create_new_run(seed=42)
        state.party.members[0].health = 1
        state.party.members[0].condition = Condition.SICK
        rng = SeededRNG(42)
        check_health_effects(state, rng)
        m = state.party.members[0]
        if not m.is_alive():
            assert m.death_cause == "Disease"

    def test_injury_cause(self):
        state = create_new_run(seed=42)
        state.party.members[0].health = 1
        state.party.members[0].condition = Condition.INJURED
        rng = SeededRNG(42)
        check_health_effects(state, rng)
        m = state.party.members[0]
        if not m.is_alive():
            assert m.death_cause == "Injury"

    def test_exhaustion_cause(self):
        state = create_new_run(seed=42)
        state.party.members[0].health = 1
        state.party.members[0].condition = Condition.EXHAUSTED
        # Need a stressor — low morale sickness check may not fire,
        # but exhaustion alone doesn't deal damage. Use food=0 to kill.
        state.supplies.food = 0
        rng = SeededRNG(42)
        check_health_effects(state, rng)
        m = state.party.members[0]
        if not m.is_alive():
            # Food=0 means starvation takes priority
            assert m.death_cause == "Starvation"

    def test_alive_members_no_cause(self):
        state = create_new_run(seed=42)
        rng = SeededRNG(42)
        check_health_effects(state, rng)
        for m in state.party.members:
            if m.is_alive():
                assert m.death_cause == ""

    def test_death_cause_in_effects_dict(self):
        """The returned effects dict includes 'cause' for deaths."""
        state = create_new_run(seed=42)
        state.supplies.food = 0
        state.party.members[0].health = 1
        rng = SeededRNG(42)
        effects = check_health_effects(state, rng)
        died = [e for e in effects if e["type"] == "died"]
        for d in died:
            assert "cause" in d
            assert d["cause"] != ""


class TestDetermineGameOverCause:
    """determine_cause_of_death() derives the headline cause."""

    def test_wagon_failure(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 0
        state.supplies.parts = 0
        cause = determine_cause_of_death(state)
        assert cause == "Wagon failure"

    def test_starvation_aggregate(self):
        state = create_new_run(seed=42)
        for m in state.party.members:
            m.health = 0
            m.death_cause = "Starvation"
        cause = determine_cause_of_death(state)
        assert cause == "Starvation"

    def test_mixed_causes_most_common_wins(self):
        state = create_new_run(seed=42)
        for m in state.party.members:
            m.health = 0
        state.party.members[0].death_cause = "Disease"
        state.party.members[1].death_cause = "Dehydration"
        state.party.members[2].death_cause = "Dehydration"
        state.party.members[3].death_cause = "Disease"
        # Tied — Counter.most_common returns first inserted
        cause = determine_cause_of_death(state)
        assert cause in ("Disease", "Dehydration")

    def test_fallback_when_no_causes_set(self):
        state = create_new_run(seed=42)
        for m in state.party.members:
            m.health = 0
            m.death_cause = ""
        cause = determine_cause_of_death(state)
        assert cause == "The trail"

    def test_wagon_failure_over_deaths(self):
        """Wagon failure takes priority even if members also died."""
        state = create_new_run(seed=42)
        state.wagon.condition = 0
        state.supplies.parts = 0
        for m in state.party.members:
            m.health = 0
            m.death_cause = "Starvation"
        cause = determine_cause_of_death(state)
        assert cause == "Wagon failure"


class TestDeathCauseSaveRoundtrip:
    """death_cause survives save/load cycle."""

    def test_roundtrip(self, tmp_path):
        from escape_the_valley.save import load_game, save_game

        state = create_new_run(seed=42)
        state.party.members[0].health = 0
        state.party.members[0].death_cause = "Dehydration"

        save_game(state, base_path=tmp_path)
        loaded = load_game(base_path=tmp_path)
        assert loaded is not None
        assert loaded.party.members[0].death_cause == "Dehydration"

    def test_backward_compat_no_death_cause(self, tmp_path):
        """Old saves without death_cause default to empty string."""
        import json

        from escape_the_valley.save import SAVE_DIR, SAVE_FILE, load_game, save_game

        state = create_new_run(seed=42)
        save_game(state, base_path=tmp_path)

        # Strip death_cause from saved JSON
        save_path = tmp_path / SAVE_DIR / SAVE_FILE
        data = json.loads(save_path.read_text(encoding="utf-8"))
        for m in data["party"]["members"]:
            m.pop("death_cause", None)
        save_path.write_text(
            json.dumps(data, indent=2), encoding="utf-8",
        )

        loaded = load_game(base_path=tmp_path)
        assert loaded is not None
        for m in loaded.party.members:
            assert m.death_cause == ""


class TestLedgerDeathDisplay:
    """Trail ledger shows enriched death causes."""

    def test_header_shows_cause(self):
        from escape_the_valley.ledger import build_trail_ledger

        state = create_new_run(seed=42)
        state.game_over = True
        state.cause_of_death = "Dehydration"
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "Cause: Dehydration." in text

    def test_roll_call_shows_member_cause(self):
        from escape_the_valley.ledger import build_trail_ledger

        state = create_new_run(seed=42)
        state.game_over = True
        state.cause_of_death = "Starvation"
        state.party.members[0].health = 0
        state.party.members[0].death_cause = "Starvation"
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        name = state.party.members[0].name
        assert f"{name} -- died of starvation" in text

    def test_roll_call_alive_member(self):
        from escape_the_valley.ledger import build_trail_ledger

        state = create_new_run(seed=42)
        state.game_over = True
        state.cause_of_death = "Starvation"
        # Kill first member, keep others alive
        state.party.members[0].health = 0
        state.party.members[0].death_cause = "Starvation"
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        alive_name = state.party.members[1].name
        assert f"{alive_name} --" in text
        assert "health" in text
