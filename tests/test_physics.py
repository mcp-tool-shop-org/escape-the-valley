"""Tests for survival physics."""

from escape_the_valley.models import (
    Pace,
    SeededRNG,
)
from escape_the_valley.physics import (
    attempt_hunt,
    attempt_repair,
    compute_daily_consumption,
    compute_travel_distance,
    rest_day,
)
from escape_the_valley.worldgen import create_new_run


class TestConsumption:
    def test_scales_with_party_size(self):
        state = create_new_run(seed=42)
        c1 = compute_daily_consumption(state)

        # Kill 2 members
        state.party.members[2].health = 0
        state.party.members[3].health = 0
        c2 = compute_daily_consumption(state)

        # Fewer people = less consumption
        assert abs(c2["food"]) < abs(c1["food"])

    def test_pace_affects_consumption(self):
        state = create_new_run(seed=42)
        state.wagon.pace = Pace.SLOW
        c_slow = compute_daily_consumption(state)

        state.wagon.pace = Pace.HARD
        c_hard = compute_daily_consumption(state)

        assert abs(c_hard["food"]) > abs(c_slow["food"])

    def test_no_consumption_if_all_dead(self):
        state = create_new_run(seed=42)
        for m in state.party.members:
            m.health = 0
        c = compute_daily_consumption(state)
        assert c["food"] == 0
        assert c["water"] == 0


class TestTravelDistance:
    def test_pace_affects_distance(self):
        state = create_new_run(seed=42)

        state.wagon.pace = Pace.SLOW
        d_slow = compute_travel_distance(state)

        state.wagon.pace = Pace.HARD
        d_hard = compute_travel_distance(state)

        assert d_hard > d_slow

    def test_bad_wagon_slows(self):
        state = create_new_run(seed=42)
        state.wagon.pace = Pace.STEADY

        state.wagon.condition = 100
        d_good = compute_travel_distance(state)

        state.wagon.condition = 20
        d_bad = compute_travel_distance(state)

        assert d_bad < d_good


class TestHunt:
    def test_costs_ammo(self):
        state = create_new_run(seed=42)
        rng = SeededRNG(42)
        deltas = attempt_hunt(state, rng)
        assert deltas.get("ammo", 0) == -1

    def test_no_ammo_no_hunt(self):
        state = create_new_run(seed=42)
        state.supplies.ammo = 0
        rng = SeededRNG(42)
        deltas = attempt_hunt(state, rng)
        assert deltas == {}


class TestRepair:
    def test_costs_parts(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 50
        deltas = attempt_repair(state)
        assert deltas.get("parts", 0) == -1

    def test_no_parts_no_repair(self):
        state = create_new_run(seed=42)
        state.supplies.parts = 0
        deltas = attempt_repair(state)
        assert deltas == {}

    def test_improves_condition(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 50
        attempt_repair(state)
        assert state.wagon.condition > 50


class TestRest:
    def test_heals_party(self):
        state = create_new_run(seed=42)
        for m in state.party.members:
            m.health = 50
        rng = SeededRNG(42)
        rest_day(state, rng)
        assert all(m.health > 50 for m in state.party.members)
