"""Tests for save/load roundtrip."""

import tempfile
from pathlib import Path

from escape_the_valley.models import GMProfile, JournalEntry
from escape_the_valley.save import load_game, save_game
from escape_the_valley.worldgen import create_new_run


class TestSaveLoad:
    def test_roundtrip(self):
        """Save and load should produce identical state."""
        state = create_new_run(seed=42, gm_profile=GMProfile.FIRESIDE)

        # Add a journal entry
        state.journal.append(JournalEntry(
            day=1,
            location="Millford",
            event_id="test_event",
            scene_title="Test Scene",
            narration="Something happened.",
            choice_made="A: Did something",
            outcome="It worked.",
            deltas={"food": -5},
            tags=["test"],
        ))

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            loaded = load_game(base)

            assert loaded is not None
            assert loaded.run_id == state.run_id
            assert loaded.seed == state.seed
            assert loaded.day == state.day
            assert loaded.gm_profile == state.gm_profile
            assert loaded.weirdness_level == state.weirdness_level
            assert loaded.uncanny_tokens == state.uncanny_tokens
            assert len(loaded.party.members) == len(state.party.members)
            assert loaded.supplies.food == state.supplies.food
            assert loaded.wagon.condition == state.wagon.condition
            assert len(loaded.journal) == 1
            assert loaded.journal[0].event_id == "test_event"
            assert len(loaded.map_nodes) == len(state.map_nodes)

    def test_roundtrip_preserves_members(self):
        state = create_new_run(seed=42)
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            loaded = load_game(base)

            for orig, load in zip(state.party.members, loaded.party.members, strict=False):
                assert orig.name == load.name
                assert orig.health == load.health
                assert orig.condition == load.condition
                assert orig.traits == load.traits

    def test_no_save_returns_none(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            loaded = load_game(Path(tmpdir))
            assert loaded is None
