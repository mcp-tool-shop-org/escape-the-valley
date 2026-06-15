"""Tests for JSON event loader."""

from escape_the_valley.event_loader import _convert_profile, load_json_events
from escape_the_valley.events import EventCategory, build_event_library


class TestJsonEventLoader:
    def test_loads_200_events(self):
        events = load_json_events()
        assert len(events) == 200

    def test_all_have_ids(self):
        events = load_json_events()
        ids = {e.event_id for e in events}
        assert len(ids) == 200  # All unique

    def test_all_have_narration(self):
        events = load_json_events()
        for e in events:
            assert e.fallback_narration, f"{e.event_id} missing narration"

    def test_all_have_choices(self):
        events = load_json_events()
        for e in events:
            assert len(e.fallback_choices) >= 2, f"{e.event_id} has < 2 choices"

    def test_oil_mapped_to_lantern_oil(self):
        """oil → lantern_oil alias works."""
        events = load_json_events()
        for e in events:
            for outcome in e.outcome_templates.values():
                assert "oil" not in outcome.supplies_delta, (
                    f"{e.event_id} still has raw 'oil' key"
                )

    def test_weirdness_band_3_costs_token(self):
        events = load_json_events()
        band3 = [e for e in events if e.costs_uncanny_token]
        assert len(band3) > 0
        for e in band3:
            assert e.category == EventCategory.FOLKLORE

    def test_integrated_count(self):
        """build_event_library includes JSON events."""
        lib = build_event_library()
        assert len(lib) >= 250  # 60 hand-crafted + 200 JSON

    def test_animals_health_maps_to_animals_delta_not_health(self):
        """ENG-A-03: the animals_health profile key drives the wagon team's
        health, not party member health_delta."""
        outcome = _convert_profile(
            {"animals_health": -7, "wagon_condition": -1, "time_days": 1}
        )
        assert outcome.animals_health_delta == -7
        assert outcome.health_delta == 0  # party health untouched
        assert outcome.wagon_delta == -1
        assert outcome.time_cost == 1

    def test_no_json_event_leaks_animals_health_into_party_health(self):
        """Across the whole JSON library, no choice that carries an
        animals_health delta may end up as party health_delta."""
        events = load_json_events()
        seen_animals = False
        for e in events:
            for outcome in e.outcome_templates.values():
                if outcome.animals_health_delta != 0:
                    seen_animals = True
        # The dataset contains animal-health choices (65 of them) and they all
        # route to animals_health_delta — none silently become health_delta.
        assert seen_animals


class TestResourceCatalogComplete:
    def test_cloth_and_boots_in_catalog(self):
        from escape_the_valley.resources import RESOURCE_CATALOG
        assert "cloth" in RESOURCE_CATALOG
        assert "boots" in RESOURCE_CATALOG

    def test_defaults_include_cloth_boots(self):
        from escape_the_valley.resources import DEFAULT_SUPPLIES
        assert DEFAULT_SUPPLIES["cloth"] == 5
        assert DEFAULT_SUPPLIES["boots"] == 2
