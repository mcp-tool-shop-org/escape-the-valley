"""Tests for memory system — store, pressures, themes, retrieval, brief."""

from escape_the_valley.memory import (
    MEMORY_BUDGET,
    GMBrief,
    add_card,
    build_gm_brief,
    compute_pressures,
    compute_themes,
    drop_lowest,
    format_brief_for_prompt,
    retrieve_memories,
)
from escape_the_valley.models import (
    Condition,
    JournalEntry,
    MemoryCard,
)
from escape_the_valley.worldgen import create_new_run


def _make_card(
    id: str = "test_card_1",
    kind: str = "event_callback",
    salience: float = 0.5,
    tags: list[str] | None = None,
    day_created: int = 1,
    day_last_seen: int = 1,
    cooldown_until: int = 0,
    source: str = "engine",
) -> MemoryCard:
    return MemoryCard(
        id=id,
        kind=kind,
        title=f"Title {id}",
        text=f"Text for {id}",
        tags=tags or [],
        day_created=day_created,
        day_last_seen=day_last_seen,
        cooldown_until=cooldown_until,
        salience=salience,
        source=source,
    )


# ── Store operations ─────────────────────────────────────────────────


class TestAddCard:
    def test_adds_card_to_state(self):
        state = create_new_run(seed=1)
        card = _make_card()
        add_card(state, card)
        assert len(state.memory_cards) == 1
        assert state.memory_cards[0].id == "test_card_1"

    def test_deduplicates_by_id(self):
        state = create_new_run(seed=1)
        card1 = _make_card(id="dup")
        card2 = _make_card(id="dup")
        add_card(state, card1)
        add_card(state, card2)
        assert len(state.memory_cards) == 1

    def test_evicts_when_over_budget(self):
        state = create_new_run(seed=1)
        # Fill to budget
        for i in range(MEMORY_BUDGET):
            add_card(state, _make_card(id=f"card_{i}", salience=0.5))
        assert len(state.memory_cards) == MEMORY_BUDGET

        # Add one more with high salience — lowest should be evicted
        add_card(state, _make_card(id="overflow", salience=0.9))
        assert len(state.memory_cards) == MEMORY_BUDGET
        assert any(c.id == "overflow" for c in state.memory_cards)


class TestDropLowest:
    def test_drops_lowest_salience(self):
        state = create_new_run(seed=1)
        state.memory_cards = [
            _make_card(id="low", salience=0.1),
            _make_card(id="mid", salience=0.5),
            _make_card(id="high", salience=0.9),
        ]
        drop_lowest(state, 1)
        assert len(state.memory_cards) == 2
        ids = {c.id for c in state.memory_cards}
        assert "low" not in ids
        assert "high" in ids

    def test_prefers_expired_cooldown(self):
        state = create_new_run(seed=1)
        state.day = 10
        state.memory_cards = [
            _make_card(id="expired", salience=0.8, cooldown_until=5),
            _make_card(id="active", salience=0.3, cooldown_until=15),
        ]
        drop_lowest(state, 1)
        assert len(state.memory_cards) == 1
        assert state.memory_cards[0].id == "active"

    def test_noop_for_zero_count(self):
        state = create_new_run(seed=1)
        state.memory_cards = [_make_card()]
        drop_lowest(state, 0)
        assert len(state.memory_cards) == 1


# ── Pressures ────────────────────────────────────────────────────────


class TestComputePressures:
    def test_food_critical(self):
        state = create_new_run(seed=1)
        alive = state.party.alive_count
        state.supplies.food = alive * 2  # exactly critical threshold
        pressures = compute_pressures(state)
        assert "FOOD_CRITICAL" in pressures

    def test_water_gone(self):
        state = create_new_run(seed=1)
        state.supplies.water = 0
        pressures = compute_pressures(state)
        assert "WATER_GONE" in pressures

    def test_wagon_fragile(self):
        state = create_new_run(seed=1)
        state.wagon.condition = 10
        pressures = compute_pressures(state)
        assert "WAGON_FRAGILE" in pressures

    def test_party_sick(self):
        state = create_new_run(seed=1)
        # Make 2 members sick
        sick_count = 0
        for m in state.party.members:
            if m.is_alive() and sick_count < 2:
                m.condition = Condition.SICK
                sick_count += 1
        pressures = compute_pressures(state)
        assert "PARTY_SICK" in pressures

    def test_sorted_by_severity(self):
        state = create_new_run(seed=1)
        state.supplies.food = 0  # FOOD_GONE (severity 10)
        state.wagon.condition = 30  # WAGON_WORN (severity 4)
        pressures = compute_pressures(state)
        assert pressures.index("FOOD_GONE") < pressures.index("WAGON_WORN")

    def test_late_journey(self):
        state = create_new_run(seed=1)
        state.distance_traveled = 180
        state.total_distance = 200
        pressures = compute_pressures(state)
        assert "LATE_JOURNEY" in pressures

    def test_no_pressures_when_healthy(self):
        state = create_new_run(seed=1)
        # Default state should be mostly healthy
        pressures = compute_pressures(state)
        # Shouldn't have critical pressures
        assert "FOOD_GONE" not in pressures
        assert "WATER_GONE" not in pressures
        assert "PARTY_DEAD" not in pressures

    def test_all_dead(self):
        state = create_new_run(seed=1)
        for m in state.party.members:
            m.health = 0
        pressures = compute_pressures(state)
        assert pressures == ["PARTY_DEAD"]


# ── Themes ───────────────────────────────────────────────────────────


class TestComputeThemes:
    def test_extracts_themes_from_journal(self):
        state = create_new_run(seed=1)
        state.journal.append(JournalEntry(
            day=1, location="test", event_id="e1",
            scene_title="Test", narration="", choice_made="",
            outcome="", tags=["river", "crossing"],
        ))
        themes = compute_themes(state)
        assert "river" in themes

    def test_maps_folklore_to_uncanny(self):
        state = create_new_run(seed=1)
        state.recent_event_tags = ["folklore", "ghost"]
        themes = compute_themes(state)
        assert "uncanny" in themes

    def test_returns_max_three(self):
        state = create_new_run(seed=1)
        state.recent_event_tags = [
            "river", "folklore", "food", "bandit",
            "storm", "wagon", "death",
        ]
        themes = compute_themes(state)
        assert len(themes) <= 3

    def test_empty_when_no_tags(self):
        state = create_new_run(seed=1)
        themes = compute_themes(state)
        assert themes == []


# ── Retrieval ────────────────────────────────────────────────────────


class TestRetrieveMemories:
    def test_returns_matching_cards(self):
        state = create_new_run(seed=1)
        state.recent_event_tags = ["river"]
        card = _make_card(id="river_card", tags=["river"], salience=0.7)
        state.memory_cards = [card]

        results = retrieve_memories(state)
        assert len(results) == 1
        assert results[0].id == "river_card"

    def test_respects_cooldown(self):
        state = create_new_run(seed=1)
        state.day = 5
        state.recent_event_tags = ["river"]
        card = _make_card(
            id="cooled", tags=["river"], cooldown_until=10,
        )
        state.memory_cards = [card]

        results = retrieve_memories(state)
        assert len(results) == 0

    def test_sets_cooldown_on_retrieval(self):
        state = create_new_run(seed=1)
        state.day = 5
        state.recent_event_tags = ["river"]
        card = _make_card(
            id="fresh", tags=["river"], salience=0.7,
        )
        state.memory_cards = [card]

        results = retrieve_memories(state)
        assert results[0].cooldown_until == 8  # day 5 + 3

    def test_updates_day_last_seen(self):
        state = create_new_run(seed=1)
        state.day = 7
        state.recent_event_tags = ["river"]
        card = _make_card(
            id="old", tags=["river"], day_last_seen=2,
        )
        state.memory_cards = [card]

        results = retrieve_memories(state)
        assert results[0].day_last_seen == 7

    def test_max_results_cap(self):
        state = create_new_run(seed=1)
        state.recent_event_tags = ["river"]
        for i in range(10):
            state.memory_cards.append(
                _make_card(
                    id=f"c{i}", tags=["river"], salience=0.7,
                ),
            )

        results = retrieve_memories(state, max_results=3)
        assert len(results) == 3

    def test_higher_salience_ranks_higher(self):
        state = create_new_run(seed=1)
        state.recent_event_tags = ["river"]
        low = _make_card(id="low", tags=["river"], salience=0.3)
        high = _make_card(id="high", tags=["river"], salience=0.9)
        state.memory_cards = [low, high]

        results = retrieve_memories(state, max_results=2)
        assert results[0].id == "high"

    def test_empty_cards_returns_empty(self):
        state = create_new_run(seed=1)
        results = retrieve_memories(state)
        assert results == []


# ── GMBrief builder ──────────────────────────────────────────────────


class TestBuildGMBrief:
    def test_returns_brief(self):
        state = create_new_run(seed=1)
        brief = build_gm_brief(state)
        assert isinstance(brief, GMBrief)
        assert brief.situation
        assert brief.tone_profile == state.gm_profile.value

    def test_includes_pressures_when_starving(self):
        state = create_new_run(seed=1)
        state.supplies.food = 0
        brief = build_gm_brief(state)
        assert "FOOD_GONE" in brief.pressures

    def test_weirdness_none_when_no_tokens(self):
        state = create_new_run(seed=1)
        state.uncanny_tokens = 0
        brief = build_gm_brief(state)
        assert brief.weirdness_allowance == "none"

    def test_weirdness_strong(self):
        state = create_new_run(seed=1)
        state.uncanny_tokens = 2
        state.weirdness_level = 3
        brief = build_gm_brief(state)
        assert brief.weirdness_allowance == "strong"


# ── Format for prompt ────────────────────────────────────────────────


class TestFormatBrief:
    def test_includes_situation(self):
        brief = GMBrief(
            situation="Day 5, morning. Plains terrain.",
            pressures=["FOOD_LOW"],
            themes=["hunger"],
            tone_profile="fireside",
        )
        text = format_brief_for_prompt(brief)
        assert "GM BRIEF:" in text
        assert "Day 5, morning" in text
        assert "FOOD_LOW" in text
        assert "hunger" in text

    def test_includes_memories(self):
        card = _make_card(id="npc_ferry", kind="npc")
        brief = GMBrief(
            situation="Day 3.",
            pressures=[],
            themes=[],
            callbacks=[card],
            tone_profile="fireside",
        )
        text = format_brief_for_prompt(brief)
        assert "[npc]" in text
        assert "Title npc_ferry" in text

    def test_omits_weirdness_when_none(self):
        brief = GMBrief(
            situation="Day 1.",
            pressures=[],
            themes=[],
            tone_profile="fireside",
            weirdness_allowance="none",
        )
        text = format_brief_for_prompt(brief)
        assert "Weirdness" not in text
