"""Tests for memory emitters — engine cards + GM validation."""

from escape_the_valley.events import EventCategory, EventSkeleton
from escape_the_valley.memory_emitters import (
    check_resource_crises,
    emit_arrival_card,
    emit_event_card,
    emit_health_cards,
    emit_resource_crisis_card,
    emit_wagon_card,
    validate_gm_cards,
)
from escape_the_valley.models import Biome, MapNode
from escape_the_valley.worldgen import create_new_run

# ── Health card emitters ─────────────────────────────────────────────


class TestEmitHealthCards:
    def test_death_creates_wound_card(self):
        state = create_new_run(seed=1)
        effects = [{"member": "Martha", "type": "died"}]
        emit_health_cards(state, effects)

        assert len(state.memory_cards) == 1
        card = state.memory_cards[0]
        assert card.kind == "wound"
        assert "Martha" in card.entities
        assert card.salience == 0.7
        assert card.source == "engine"

    def test_sickness_creates_wound_card(self):
        state = create_new_run(seed=1)
        effects = [{"member": "Jacob", "type": "fell_sick"}]
        emit_health_cards(state, effects)

        assert len(state.memory_cards) == 1
        assert state.memory_cards[0].kind == "wound"
        assert "sickness" in state.memory_cards[0].tags

    def test_recovery_creates_callback_card(self):
        state = create_new_run(seed=1)
        effects = [{"member": "Sarah", "type": "healed"}]
        emit_health_cards(state, effects)

        assert len(state.memory_cards) == 1
        assert state.memory_cards[0].kind == "event_callback"

    def test_multiple_effects(self):
        state = create_new_run(seed=1)
        effects = [
            {"member": "Martha", "type": "died"},
            {"member": "Jacob", "type": "fell_sick"},
        ]
        emit_health_cards(state, effects)
        assert len(state.memory_cards) == 2


# ── Resource crisis emitter ──────────────────────────────────────────


class TestEmitResourceCrisis:
    def test_first_crisis_emits_card(self):
        state = create_new_run(seed=1)
        emit_resource_crisis_card(state, "food")

        assert len(state.memory_cards) == 1
        assert state.memory_cards[0].kind == "crisis"
        assert "food" in state.resource_crises_seen

    def test_duplicate_crisis_ignored(self):
        state = create_new_run(seed=1)
        emit_resource_crisis_card(state, "food")
        emit_resource_crisis_card(state, "food")

        assert len(state.memory_cards) == 1
        assert state.resource_crises_seen.count("food") == 1


class TestCheckResourceCrises:
    def test_detects_zero_food(self):
        state = create_new_run(seed=1)
        state.supplies.food = 0
        check_resource_crises(state)

        assert "food" in state.resource_crises_seen
        assert any(c.kind == "crisis" for c in state.memory_cards)

    def test_no_crisis_when_stocked(self):
        state = create_new_run(seed=1)
        # Default state has supplies
        check_resource_crises(state)
        assert len(state.memory_cards) == 0


# ── Wagon card emitter ───────────────────────────────────────────────


class TestEmitWagonCard:
    def test_no_parts_emits_card(self):
        state = create_new_run(seed=1)
        emit_wagon_card(state, damage=10, had_parts=False)

        assert len(state.memory_cards) == 1
        assert "crisis" in state.memory_cards[0].tags

    def test_high_damage_emits_card(self):
        state = create_new_run(seed=1)
        emit_wagon_card(state, damage=20, had_parts=True)

        assert len(state.memory_cards) == 1
        assert state.memory_cards[0].title == "Heavy Wagon Damage"

    def test_minor_damage_with_parts_skipped(self):
        state = create_new_run(seed=1)
        emit_wagon_card(state, damage=10, had_parts=True)
        assert len(state.memory_cards) == 0


# ── Arrival card emitter ─────────────────────────────────────────────


class TestEmitArrivalCard:
    def test_town_emits_landmark(self):
        state = create_new_run(seed=1)
        node = MapNode(
            node_id="town_1", name="Dusty Springs",
            biome=Biome.DESERT, hazard=3,
            water_available=True, temperature=30, is_town=True,
        )
        emit_arrival_card(state, node)

        assert len(state.memory_cards) == 1
        assert state.memory_cards[0].kind == "landmark"
        assert "Dusty Springs" in state.memory_cards[0].entities

    def test_non_town_skipped(self):
        state = create_new_run(seed=1)
        node = MapNode(
            node_id="pass_1", name="Mountain Pass",
            biome=Biome.ALPINE, hazard=7,
            water_available=False, temperature=5, is_town=False,
        )
        emit_arrival_card(state, node)
        assert len(state.memory_cards) == 0


# ── Event card emitter ───────────────────────────────────────────────


class TestEmitEventCard:
    def test_folklore_event_emits_omen(self):
        state = create_new_run(seed=1)
        event = EventSkeleton(
            event_id="ghost_lantern",
            title="The Ghost Lantern",
            category=EventCategory.FOLKLORE,
            tags=["folklore", "ghost"],
            fallback_narration="A pale light drifts through the trees.",
        )
        emit_event_card(state, event)

        assert len(state.memory_cards) == 1
        assert state.memory_cards[0].kind == "omen"

    def test_non_folklore_skipped(self):
        state = create_new_run(seed=1)
        event = EventSkeleton(
            event_id="broken_wheel",
            title="Broken Wheel",
            category=EventCategory.SURVIVAL,
            tags=["wagon", "repair"],
        )
        emit_event_card(state, event)
        assert len(state.memory_cards) == 0


# ── GM card validation ───────────────────────────────────────────────


class TestValidateGMCards:
    def test_valid_npc_accepted(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "npc",
            "title": "The Ferryman",
            "text": "A gaunt figure at the crossing.",
            "tags": ["river", "npc"],
            "entities": ["Ferryman"],
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 1
        assert cards[0].kind == "npc"
        assert cards[0].salience == 0.5  # forced
        assert cards[0].source == "gm"

    def test_invalid_kind_rejected(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "wound",  # GM can't create wounds
            "title": "Test",
            "text": "Test text",
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 0

    def test_max_two_per_proposal(self):
        state = create_new_run(seed=1)
        proposed = [
            {"kind": "npc", "title": f"NPC {i}", "text": f"Text {i}"}
            for i in range(5)
        ]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 2

    def test_supply_numbers_rejected(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "rumor",
            "title": "Supply Count",
            "text": "They had 15 food left in the wagon.",
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 0

    def test_empty_title_rejected(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "npc",
            "title": "",
            "text": "Some text here.",
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 0

    def test_title_truncated(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "place",
            "title": "A" * 60,
            "text": "Some text.",
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 1
        assert len(cards[0].title) == 40

    def test_salience_forced(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "omen",
            "title": "Dark Sign",
            "text": "A crow circles thrice.",
            "salience": 0.9,  # GM tries to set high salience
        }]
        cards = validate_gm_cards(state, proposed)
        assert cards[0].salience == 0.5
