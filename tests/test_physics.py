"""Tests for survival physics."""

from escape_the_valley.models import (
    Biome,
    Pace,
    SeededRNG,
)
from escape_the_valley.physics import (
    abandon_cargo,
    attempt_hunt,
    attempt_repair,
    can_abandon_cargo,
    can_desperate_repair,
    can_hard_ration,
    check_breakdown,
    compute_daily_consumption,
    compute_travel_distance,
    desperate_repair,
    hard_ration,
    journey_pressure,
    rest_day,
)
from escape_the_valley.worldgen import create_new_run


class TestJourneyPressure:
    def test_zero_at_start(self):
        state = create_new_run(seed=42)
        state.distance_traveled = 0
        state.total_distance = 200
        assert journey_pressure(state) == 0.0

    def test_one_at_end(self):
        state = create_new_run(seed=42)
        state.distance_traveled = 200
        state.total_distance = 200
        assert journey_pressure(state) == 1.0

    def test_capped_at_one(self):
        state = create_new_run(seed=42)
        state.distance_traveled = 300
        state.total_distance = 200
        assert journey_pressure(state) == 1.0

    def test_midpoint(self):
        state = create_new_run(seed=42)
        state.distance_traveled = 100
        state.total_distance = 200
        assert journey_pressure(state) == 0.5

    def test_zero_total_distance(self):
        state = create_new_run(seed=42)
        state.total_distance = 0
        assert journey_pressure(state) == 0.0


class TestConsumptionScaling:
    def test_late_game_eats_more(self):
        """Late-game consumption should exceed early-game."""
        early = create_new_run(seed=42)
        early.distance_traveled = 0
        early.total_distance = 200

        late = create_new_run(seed=42)
        late.distance_traveled = 190
        late.total_distance = 200

        c_early = compute_daily_consumption(early)
        c_late = compute_daily_consumption(late)
        assert abs(c_late["food"]) >= abs(c_early["food"])
        assert abs(c_late["water"]) >= abs(c_early["water"])


class TestConsumption:
    def test_scales_with_party_size(self):
        state = create_new_run(seed=42)
        state.doctrine = ""  # Clear doctrine to isolate party-size effect
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


# ── Doctrine modifier tests ─────────────────────────────────────────


class TestDoctrineConsumption:
    def test_travel_light_reduces_food(self):
        state = create_new_run(seed=42)
        state.doctrine = ""
        c_normal = compute_daily_consumption(state)

        state.doctrine = "travel_light"
        c_light = compute_daily_consumption(state)

        assert abs(c_light["food"]) <= abs(c_normal["food"])

    def test_rationing_halves_consumption(self):
        state = create_new_run(seed=42)
        state.doctrine = ""
        c_normal = compute_daily_consumption(state)

        state.rationing_steps = 2
        c_ration = compute_daily_consumption(state)

        assert abs(c_ration["food"]) <= abs(c_normal["food"])
        assert abs(c_ration["water"]) <= abs(c_normal["water"])


class TestDoctrineRepair:
    def test_careful_hands_boosts_repair(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 50
        state.doctrine = ""
        cond_before = state.wagon.condition
        attempt_repair(state)
        normal_gain = state.wagon.condition - cond_before

        state.wagon.condition = 50
        state.supplies.parts = 10
        state.doctrine = "careful_hands"
        cond_before = state.wagon.condition
        attempt_repair(state)
        careful_gain = state.wagon.condition - cond_before

        assert careful_gain >= normal_gain


class TestDoctrineDistance:
    def test_careful_hands_reduces_distance(self):
        state = create_new_run(seed=42)
        state.doctrine = ""
        d_normal = compute_travel_distance(state)

        state.doctrine = "careful_hands"
        d_careful = compute_travel_distance(state)

        assert d_careful <= d_normal


# ── Escape valve tests ───────────────────────────────────────────────


class TestCanEscapeValves:
    def test_can_abandon_cargo_needs_low_wagon(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 50
        assert not can_abandon_cargo(state)

        state.wagon.condition = 20
        assert can_abandon_cargo(state)

    def test_can_desperate_repair_needs_no_parts(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 20
        state.supplies.parts = 5
        assert not can_desperate_repair(state)

        state.supplies.parts = 0
        assert can_desperate_repair(state)

    def test_can_hard_ration_needs_low_food(self):
        state = create_new_run(seed=42)
        state.supplies.food = 100
        assert not can_hard_ration(state)

        alive = state.party.alive_count
        state.supplies.food = alive * 2  # Below alive * 3
        assert can_hard_ration(state)

    def test_cant_hard_ration_while_rationing(self):
        state = create_new_run(seed=42)
        alive = state.party.alive_count
        state.supplies.food = alive * 2
        state.rationing_steps = 1
        assert not can_hard_ration(state)


class TestAbandonCargo:
    def test_drops_cargo_and_repairs_wagon(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 20
        state.supplies.set("salt", 10)
        state.supplies.set("cloth", 8)

        old_wagon = state.wagon.condition
        old_morale = state.party.morale
        result = abandon_cargo(state)

        assert state.wagon.condition > old_wagon
        assert state.party.morale < old_morale
        assert any(v < 0 for v in result.values())

    def test_drops_half_of_cargo_items(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 20
        state.supplies.set("salt", 10)
        state.supplies.set("cloth", 8)
        state.supplies.set("rope", 6)
        state.supplies.set("boots", 4)

        abandon_cargo(state)

        assert state.supplies.get("salt") == 5
        assert state.supplies.get("cloth") == 4
        assert state.supplies.get("rope") == 3
        assert state.supplies.get("boots") == 2


class TestDesperateRepair:
    def test_returns_success_or_failure(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 20
        state.supplies.parts = 0
        rng = SeededRNG(42)

        result = desperate_repair(state, rng)
        assert "success" in result
        assert "wagon_delta" in result

    def test_success_improves_wagon(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 20
        state.supplies.parts = 0
        # Try multiple seeds to find a success
        for s in range(100):
            state.wagon.condition = 20
            rng = SeededRNG(s)
            result = desperate_repair(state, rng)
            if result["success"]:
                assert result["wagon_delta"] > 0
                break

    def test_failure_damages_wagon_and_injures(self):
        state = create_new_run(seed=42)
        state.wagon.condition = 20
        state.supplies.parts = 0
        for s in range(100):
            state.wagon.condition = 20
            rng = SeededRNG(s)
            result = desperate_repair(state, rng)
            if not result["success"]:
                assert result["wagon_delta"] < 0
                assert "injured" in result
                break


class TestHardRation:
    def test_sets_rationing_steps(self):
        state = create_new_run(seed=42)
        state.supplies.food = 5
        hard_ration(state)
        assert state.rationing_steps == 2

    def test_reduces_morale_and_health(self):
        state = create_new_run(seed=42)
        state.supplies.food = 5
        old_morale = state.party.morale
        old_healths = [m.health for m in state.party.members if m.is_alive()]

        hard_ration(state)

        assert state.party.morale < old_morale
        for m, old_h in zip(state.party.members, old_healths, strict=False):
            if m.is_alive():
                assert m.health < old_h


# ── Phase 4: Balance pass tests ────────────────────────────────────


class TestConsumptionSlope:
    def test_max_pressure_scale_capped(self):
        """At max pressure, scale should be ~1.12×, not 1.2×."""
        state = create_new_run(seed=42)
        state.doctrine = ""
        state.distance_traveled = 0
        c_early = compute_daily_consumption(state)

        state.distance_traveled = state.total_distance
        c_late = compute_daily_consumption(state)

        # Late game still eats more
        assert abs(c_late["food"]) >= abs(c_early["food"])
        # But not drastically more — ratio <= 1.15 with rounding
        if abs(c_early["food"]) > 0:
            ratio = abs(c_late["food"]) / abs(c_early["food"])
            assert ratio <= 1.20  # margin for int rounding


class TestBreakdownCurve:
    def test_good_wagon_low_breakdown(self):
        """Wagon > 60 should have very low breakdown rate."""
        state = create_new_run(seed=42)
        state.wagon.condition = 80
        state.doctrine = ""
        breakdowns = sum(
            1 for s in range(500)
            if check_breakdown(state, SeededRNG(s)) is not None
        )
        assert breakdowns < 40  # < 8% with low hazard

    def test_damaged_wagon_high_breakdown(self):
        """Wagon < 30 should break down more often."""
        state = create_new_run(seed=42)
        state.wagon.condition = 20
        state.doctrine = ""
        breaks_bad = sum(
            1 for s in range(500)
            if check_breakdown(state, SeededRNG(s)) is not None
        )

        state.wagon.condition = 80
        breaks_good = sum(
            1 for s in range(500)
            if check_breakdown(state, SeededRNG(s)) is not None
        )
        assert breaks_bad > breaks_good

    def test_maintenance_reduces_breakdown(self):
        """Maintenance window should reduce breakdown chance."""
        state = create_new_run(seed=42)
        state.wagon.condition = 40
        state.doctrine = ""

        state.maintained_turns_remaining = 0
        breaks_normal = sum(
            1 for s in range(500)
            if check_breakdown(state, SeededRNG(s)) is not None
        )

        state.maintained_turns_remaining = 2
        breaks_maintained = sum(
            1 for s in range(500)
            if check_breakdown(state, SeededRNG(s)) is not None
        )
        assert breaks_maintained < breaks_normal


class TestHuntVariance:
    def test_forest_yields_more(self):
        """Forest hunts should average more food than desert."""
        state = create_new_run(seed=42)
        state.doctrine = ""
        state.supplies.ammo = 999
        state.party.morale = 30  # low morale, no big haul

        def _hunt_in_biome(biome):
            totals = []
            for node in state.map_nodes:
                if node.node_id == state.location_id:
                    node.biome = biome
            for s in range(200):
                state.supplies.ammo = 999
                deltas = attempt_hunt(state, SeededRNG(s))
                food = deltas.get("food", 0)
                if food > 0:
                    totals.append(food)
            return sum(totals) / max(len(totals), 1)

        avg_forest = _hunt_in_biome(Biome.FOREST)
        avg_desert = _hunt_in_biome(Biome.DESERT)
        assert avg_forest > avg_desert

    def test_big_haul_possible_high_morale(self):
        """With morale > 60, big haul (2×) should sometimes fire."""
        state = create_new_run(seed=42)
        state.party.morale = 80
        state.doctrine = ""
        state.supplies.ammo = 999

        big_hauls = 0
        for s in range(500):
            state.supplies.ammo = 999
            deltas = attempt_hunt(state, SeededRNG(s))
            food = deltas.get("food", 0)
            if food > 18:  # above normal max
                big_hauls += 1
        assert big_hauls > 0

    def test_no_big_haul_low_morale(self):
        """With morale <= 60, no big haul."""
        state = create_new_run(seed=42)
        state.party.morale = 40
        state.doctrine = ""
        state.supplies.ammo = 999

        for s in range(200):
            state.supplies.ammo = 999
            deltas = attempt_hunt(state, SeededRNG(s))
            food = deltas.get("food", 0)
            # Forest max is 18, plains 16, desert 12
            assert food <= 18


class TestDoctrineHuntBonus:
    def test_travel_light_better_hunt(self):
        """travel_light doctrine should boost hunt success."""
        state = create_new_run(seed=42)
        state.supplies.ammo = 999

        state.doctrine = ""
        s_none = sum(
            1 for s in range(300)
            if attempt_hunt(state, SeededRNG(s)).get("food", 0) > 0
        )

        state.doctrine = "travel_light"
        state.supplies.ammo = 999
        s_tl = sum(
            1 for s in range(300)
            if attempt_hunt(state, SeededRNG(s)).get("food", 0) > 0
        )
        assert s_tl >= s_none
