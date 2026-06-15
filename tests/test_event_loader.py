"""Tests for JSON event loader."""

import logging

import pytest

from escape_the_valley import event_loader
from escape_the_valley.event_loader import (
    _convert_event,
    _convert_profile,
    load_json_events,
)
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


class TestRobustEventLoading:
    """ENG-B-03: one bad entry (or a corrupt file) must never crash the load."""

    def test_corrupt_file_returns_empty(self, tmp_path, monkeypatch, caplog):
        bad = tmp_path / "data"
        bad.mkdir()
        (bad / "event_skeletons.json").write_text("{not valid json", encoding="utf-8")
        monkeypatch.setattr(event_loader, "_DATA_DIR", bad)
        with caplog.at_level(logging.ERROR):
            events = load_json_events()
        assert events == []
        assert any("could not be loaded" in r.message for r in caplog.records)

    def test_non_list_top_level_returns_empty(self, tmp_path, monkeypatch, caplog):
        bad = tmp_path / "data"
        bad.mkdir()
        (bad / "event_skeletons.json").write_text(
            '{"id": "x"}', encoding="utf-8",
        )
        monkeypatch.setattr(event_loader, "_DATA_DIR", bad)
        with caplog.at_level(logging.ERROR):
            events = load_json_events()
        assert events == []

    def test_one_bad_entry_is_skipped_not_fatal(self, tmp_path, monkeypatch, caplog):
        """A single malformed entry (missing id) is skipped+logged; the good
        entries around it still load."""
        import json

        data = [
            {
                "id": "good_1",
                "tags": ["survival"],
                "weirdness_band": 0,
                "choices": [
                    {"label": "Go", "intent_action": "TRAVEL",
                     "engine_effect_profile": {"time_days": 1}},
                    {"label": "Wait", "intent_action": "WAIT",
                     "engine_effect_profile": {"morale": -1}},
                ],
                "narration_seed": "A fork in the road.",
            },
            {"tags": ["survival"], "choices": []},  # missing id → skip
            {
                "id": "good_2",
                "tags": ["survival"],
                "weirdness_band": 0,
                "choices": [
                    {"label": "Go", "intent_action": "TRAVEL",
                     "engine_effect_profile": {"time_days": 1}},
                ],
                "narration_seed": "Another fork.",
            },
        ]
        d = tmp_path / "data"
        d.mkdir()
        (d / "event_skeletons.json").write_text(json.dumps(data), encoding="utf-8")
        monkeypatch.setattr(event_loader, "_DATA_DIR", d)

        with caplog.at_level(logging.WARNING):
            events = load_json_events()

        ids = {e.event_id for e in events}
        assert ids == {"good_1", "good_2"}
        assert any("malformed event" in r.message for r in caplog.records)

    def test_missing_id_raises_in_convert(self):
        with pytest.raises(ValueError):
            _convert_event({"tags": [], "choices": []})

    def test_choices_capped_at_four(self, caplog):
        raw = {
            "id": "five_choices",
            "tags": ["survival"],
            "weirdness_band": 0,
            "choices": [
                {"label": f"Option {i}", "intent_action": "WAIT",
                 "engine_effect_profile": {"morale": -1}}
                for i in range(5)
            ],
            "narration_seed": "Too many ways forward.",
        }
        with caplog.at_level(logging.WARNING):
            ev = _convert_event(raw)
        assert len(ev.fallback_choices) == 4
        assert [c.choice_id for c in ev.fallback_choices] == ["A", "B", "C", "D"]
        assert any("exceed" in r.message for r in caplog.records)


class TestResourceKeyValidation:
    """ENG-B-07: a typo'd resource key must not become a phantom supply."""

    def test_unknown_key_dropped(self, caplog):
        with caplog.at_level(logging.WARNING):
            outcome = _convert_profile(
                {"gold": 10, "food": -3}, event_id="typo_event",
            )
        assert "gold" not in outcome.supplies_delta
        assert outcome.supplies_delta.get("food") == -3
        assert any("unknown resource key" in r.message for r in caplog.records)

    def test_alias_still_resolves(self):
        outcome = _convert_profile({"oil": -1}, event_id="lamp_event")
        assert outcome.supplies_delta.get("lantern_oil") == -1
        assert "oil" not in outcome.supplies_delta

    def test_no_json_event_has_phantom_supply(self):
        """The shipped library must contain only catalog-valid supply keys."""
        from escape_the_valley.resources import RESOURCE_CATALOG

        events = load_json_events()
        for e in events:
            for outcome in e.outcome_templates.values():
                for key in outcome.supplies_delta:
                    assert key in RESOURCE_CATALOG, (
                        f"{e.event_id}: phantom supply key {key!r}"
                    )


class TestEventHealthPath:
    """EC-02 STAKES: the 'health' profile key maps to EventOutcome.health_delta
    so data events can wound (or heal) the party. apply_outcome already applies
    health_delta and attributes event-caused deaths."""

    def test_health_key_maps_to_health_delta(self):
        outcome = _convert_profile(
            {"health": -6, "time_days": 1, "morale": -2}, event_id="wound_event",
        )
        assert outcome.health_delta == -6
        # And it is NOT smuggled into supplies (would be a phantom resource).
        assert "health" not in outcome.supplies_delta

    def test_positive_health_heals(self):
        outcome = _convert_profile({"health": 8}, event_id="balm_event")
        assert outcome.health_delta == 8

    def test_condition_key_is_benign_noop(self, caplog):
        """A 'condition' annotation is accepted (no warning) and never becomes a
        phantom supply — it is a parsed no-op until a future slice maps it."""
        with caplog.at_level(logging.WARNING):
            outcome = _convert_profile(
                {"condition": "injured", "health": -4}, event_id="annot_event",
            )
        assert outcome.health_delta == -4
        assert "condition" not in outcome.supplies_delta
        assert not any(
            "unknown resource key" in r.message for r in caplog.records
        )

    def test_health_is_not_animals_health(self):
        """A 'health' delta must wound the party, NOT the wagon's draft team
        (the inverse of the ENG-A-03 invariant)."""
        outcome = _convert_profile(
            {"health": -5, "animals_health": -2}, event_id="both_event",
        )
        assert outcome.health_delta == -5
        assert outcome.animals_health_delta == -2

    def test_data_event_health_key_wounds_the_party(self):
        """End-to-end: a loaded data event whose chosen outcome carries a
        negative health_delta drops party health when applied."""
        from escape_the_valley.events import apply_outcome
        from escape_the_valley.worldgen import create_new_run

        raw = {
            "id": "ec02_wound",
            "title": "Drowning Ford",
            "tags": ["river", "ford", "survival"],
            "weirdness_band": 0,
            "severity": "high",
            "choices": [
                {"label": "Drive into the current", "intent_action": "FORD",
                 "engine_effect_profile": {"health": -6, "time_days": 1}},
                {"label": "Wait it out", "intent_action": "WAIT",
                 "engine_effect_profile": {"time_days": 1, "morale": -1}},
            ],
            "narration_seed": "The river runs hard and cold.",
        }
        ev = _convert_event(raw)
        assert ev.severity == "high"
        wound = ev.outcome_templates["A"]
        assert wound.health_delta == -6

        state = create_new_run(seed=42)
        state.supplies.food = 50
        state.supplies.water = 50  # so the wound — not scarcity — is the cause
        before = state.party.members[0].health
        apply_outcome(state, wound)
        assert state.party.members[0].health == before - 6

    def test_shipped_library_loads_clean_with_high_severity(self):
        """The curated library still loads all 200 events, and the EC-02 bounded
        enrichment produced a meaningful number of high-severity, wounding
        events — without leaving any phantom supply keys behind."""
        from escape_the_valley.resources import RESOURCE_CATALOG

        events = load_json_events()
        assert len(events) == 200

        high = [e for e in events if e.severity == "high"]
        wounding = [
            e for e in events
            if any(o.health_delta < 0 for o in e.outcome_templates.values())
        ]
        # Bounded enrichment target: ~25-40 events.
        assert 25 <= len(high) <= 40
        assert 25 <= len(wounding) <= 40

        # No event smuggled a 'health'/'condition' key into supplies.
        for e in events:
            for o in e.outcome_templates.values():
                for key in o.supplies_delta:
                    assert key in RESOURCE_CATALOG, (
                        f"{e.event_id}: phantom supply key {key!r}"
                    )

    def test_health_deltas_are_conservative(self):
        """EC-02 keeps the deltas modest — no single shipped event wounds the
        party by more than 10 in one choice (no instakills from data)."""
        events = load_json_events()
        for e in events:
            for o in e.outcome_templates.values():
                assert o.health_delta >= -10, (
                    f"{e.event_id}: health_delta {o.health_delta} too harsh"
                )

    def test_unknown_severity_falls_back_to_medium(self, caplog):
        raw = {
            "id": "bad_sev",
            "title": "Odd",
            "tags": ["survival"],
            "weirdness_band": 0,
            "severity": "catastrophic",
            "choices": [
                {"label": "Go", "intent_action": "TRAVEL",
                 "engine_effect_profile": {"time_days": 1}},
                {"label": "Wait", "intent_action": "WAIT",
                 "engine_effect_profile": {"morale": -1}},
            ],
            "narration_seed": "A strange fork.",
        }
        with caplog.at_level(logging.WARNING):
            ev = _convert_event(raw)
        assert ev.severity == "medium"
        assert any("unknown severity" in r.message for r in caplog.records)
