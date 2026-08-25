"""Tests for memory emitters — engine cards + GM validation."""

from escape_the_valley.events import EventCategory, EventSkeleton
from escape_the_valley.memory import add_card
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

    # ── F-778637b3: the GM does not own engine keys ──────────────────
    #
    # `add_card` (memory.py) dedupes purely by id equality and silently
    # no-ops on any collision. Engine-authored ids are deterministic and
    # predictable from source (e.g. f"eng_crisis_{resource}_d{state.day}").
    # A model-supplied "id" is not part of any schema the GM is given
    # (SCENE_SCHEMA / OUTCOME_SCHEMA), so it must never be honored — doing
    # so would let untrusted model output plant a card whose id collides
    # with a future deterministic engine id, silently suppressing the real
    # engine-authored record.

    def test_model_supplied_id_never_honored(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "npc",
            "title": "The Ferryman",
            "text": "A gaunt figure at the crossing.",
            "id": "totally_not_engine_controlled",
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 1
        assert cards[0].id != "totally_not_engine_controlled"
        assert cards[0].id == f"gm_npc_{state.day}_0"

    def test_crafted_id_does_not_suppress_future_engine_card(self):
        """The exact attack the finding describes: a GM proposal supplies
        an id matching a *future* deterministic engine card id. Before the
        fix this would plant a `gm`-sourced card under that id; when the
        engine later tried to emit the real card, `add_card`'s
        id-equality dedup would silently drop it. After the fix the GM
        can never claim an engine id in the first place, so the later
        engine card is always added.
        """
        state = create_new_run(seed=1)
        future_engine_id = f"eng_crisis_food_d{state.day}"

        proposed = [{
            "kind": "rumor",
            "title": "A Rumor",
            "text": "Something is said at the fire.",
            "id": future_engine_id,  # crafted to collide with the engine's future id
        }]
        gm_cards = validate_gm_cards(state, proposed)
        for card in gm_cards:
            add_card(state, card)

        # The GM card must NOT have claimed the engine's id.
        assert not any(c.id == future_engine_id for c in state.memory_cards)

        # The engine can still emit its real card under that id, untouched.
        emit_resource_crisis_card(state, "food")
        assert any(
            c.id == future_engine_id and c.source == "engine"
            for c in state.memory_cards
        )

    def test_none_proposed_returns_empty_list(self):
        # F-9b0797f9 defense in depth — a None must never reach the
        # proposed[:2] slice as a bare TypeError.
        state = create_new_run(seed=1)
        assert validate_gm_cards(state, None) == []

    def test_non_dict_proposal_elements_skipped(self):
        state = create_new_run(seed=1)
        proposed = [None, "not a proposal", 7]
        assert validate_gm_cards(state, proposed) == []

    def test_null_tags_and_entities_do_not_crash(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "place",
            "title": "The Hollow",
            "text": "A quiet clearing off the trail.",
            "tags": None,
            "entities": None,
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 1
        assert cards[0].tags == []
        assert cards[0].entities == []

    def test_non_list_tags_and_entities_do_not_crash(self):
        # A model could plausibly emit a bare string instead of a list;
        # `or []` alone would not catch this (a non-empty string is
        # truthy), so this exercises the isinstance guard specifically.
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "place",
            "title": "The Hollow",
            "text": "A quiet clearing off the trail.",
            "tags": "not-a-list",
            "entities": 42,
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 1
        assert cards[0].tags == []
        assert cards[0].entities == []
