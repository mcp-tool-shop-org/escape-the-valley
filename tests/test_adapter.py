"""Tests for adapter cliff-edge warnings."""

from escape_the_valley.adapter import _build_warnings
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
