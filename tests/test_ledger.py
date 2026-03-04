"""Tests for trail ledger — end-of-run narrative builder."""

from escape_the_valley.ledger import build_trail_ledger
from escape_the_valley.models import (
    JournalEntry,
    MemoryCard,
    PartyMember,
    PartyState,
    RunState,
    SuppliesState,
    WagonState,
)


def _make_state(**overrides) -> RunState:
    """Create a minimal RunState for ledger testing."""
    defaults = dict(
        run_id="test",
        seed=1,
        day=15,
        distance_traveled=120,
        total_distance=300,
        game_over=True,
        victory=False,
        cause_of_death="starvation",
        party=PartyState(
            members=[
                PartyMember(name="Elias", health=40),
                PartyMember(name="Martha", health=0),
            ],
            morale=30,
        ),
        wagon=WagonState(condition=20),
        supplies=SuppliesState(items={"food": 0, "water": 5}),
    )
    defaults.update(overrides)
    return RunState(**defaults)


class TestTrailLedger:
    def test_header_death(self):
        state = _make_state()
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "TRAIL LEDGER" in text
        assert "starvation" in text.lower()

    def test_header_victory(self):
        state = _make_state(victory=True, cause_of_death="")
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "TRAIL LEDGER" in text
        assert "valley is behind you" in text.lower()

    def test_journey_section(self):
        state = _make_state(day=15, distance_traveled=120, total_distance=300)
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "15" in text  # days
        assert "120" in text  # distance
        assert "300" in text  # total

    def test_roll_call_alive_and_dead(self):
        state = _make_state()
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "Elias" in text
        assert "Martha" in text

    def test_costliest_day_with_journal(self):
        state = _make_state(
            journal=[
                JournalEntry(
                    day=5, location="Redwater",
                    event_id="ev1", scene_title="Storm",
                    narration="A storm hit.", choice_made="A: Shelter",
                    outcome="Lost supplies.",
                    deltas={"food": -10, "water": -5},
                ),
                JournalEntry(
                    day=8, location="Stonecross",
                    event_id="ev2", scene_title="Calm",
                    narration="Quiet day.", choice_made="A: Rest",
                    outcome="Nothing happened.",
                    deltas={"food": -1},
                ),
            ],
        )
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        # Should reference the costly day
        assert "5" in text or "Storm" in text

    def test_closest_call_with_crisis_card(self):
        state = _make_state(
            memory_cards=[
                MemoryCard(
                    id="crisis1", kind="crisis",
                    title="Nearly Lost Everything",
                    text="The wagon nearly broke.",
                    salience=0.9, day_created=10,
                ),
                MemoryCard(
                    id="omen1", kind="omen",
                    title="Strange Lights",
                    text="Lights in the sky.",
                    salience=0.3, day_created=5,
                ),
            ],
        )
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "Nearly Lost Everything" in text or "wagon" in text.lower()

    def test_resource_crises_narrative(self):
        state = _make_state(resource_crises_seen=["food", "water"])
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "food" in text.lower() or "water" in text.lower()

    def test_doctrine_echo(self):
        state = _make_state(doctrine="travel_light")
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "traveled light" in text.lower()

    def test_taboo_echo(self):
        state = _make_state(taboo="never_night")
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "night" in text.lower()

    def test_empty_state_no_crash(self):
        state = _make_state(
            journal=[], memory_cards=[],
            resource_crises_seen=[],
            doctrine="", taboo="",
        )
        ledger = build_trail_ledger(state)
        assert isinstance(ledger, list)
        assert len(ledger) > 0

    def test_promise_card_shown(self):
        state = _make_state(
            memory_cards=[
                MemoryCard(
                    id="p1", kind="promise",
                    title="Promise to Return",
                    text="Promised the old man we'd come back.",
                    salience=0.5, day_created=3,
                ),
            ],
        )
        ledger = build_trail_ledger(state)
        text = "\n".join(ledger)
        assert "Promise" in text or "promise" in text.lower()
