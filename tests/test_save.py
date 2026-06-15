"""Tests for save/load roundtrip."""

import json
import tempfile
from pathlib import Path

from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.models import GMProfile, JournalEntry
from escape_the_valley.save import (
    SAVE_DIR,
    SAVE_FILE,
    _state_to_dict,
    load_game,
    save_game,
)
from escape_the_valley.step_engine import StepEngine
from escape_the_valley.worldgen import create_new_run


def _drive(engine: StepEngine, intent: PlayerIntent) -> None:
    """Apply one intent, auto-resolving any event/route prompt with 'A'."""
    engine.step(intent)
    if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
        engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))


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

    def test_roundtrip_doctrine_taboo(self):
        state = create_new_run(seed=42)
        # Ensure we have values to roundtrip
        assert state.doctrine != ""
        assert state.taboo != ""

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            loaded = load_game(base)

            assert loaded.doctrine == state.doctrine
            assert loaded.taboo == state.taboo
            assert loaded.rationing_steps == state.rationing_steps

    def test_roundtrip_rationing_steps(self):
        state = create_new_run(seed=42)
        state.rationing_steps = 2

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            loaded = load_game(base)

            assert loaded.rationing_steps == 2

    def test_corrupted_json_returns_none(self):
        """Corrupted save file should return None, not crash."""
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_dir = base / SAVE_DIR
            save_dir.mkdir()
            save_path = save_dir / SAVE_FILE
            save_path.write_text("{invalid json!!", encoding="utf-8")

            loaded = load_game(base)
            assert loaded is None

    def test_truncated_json_returns_none(self):
        """Truncated save file should return None, not crash."""
        state = create_new_run(seed=42)
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)

            # Truncate the save file mid-JSON
            save_path = base / SAVE_DIR / SAVE_FILE
            content = save_path.read_text(encoding="utf-8")
            save_path.write_text(content[: len(content) // 2], encoding="utf-8")

            loaded = load_game(base)
            assert loaded is None

    def test_empty_file_returns_none(self):
        """Empty save file should return None, not crash."""
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_dir = base / SAVE_DIR
            save_dir.mkdir()
            (save_dir / SAVE_FILE).write_text("", encoding="utf-8")

            loaded = load_game(base)
            assert loaded is None

    def test_wrong_structure_returns_none(self):
        """Valid JSON but wrong structure should return None, not crash."""
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_dir = base / SAVE_DIR
            save_dir.mkdir()
            (save_dir / SAVE_FILE).write_text(
                json.dumps({"not": "a game state"}), encoding="utf-8",
            )

            loaded = load_game(base)
            assert loaded is None

    def test_roundtrip_sent_parcels(self):
        """Sent parcels should survive save/load roundtrip."""
        from escape_the_valley.backpack_models import SentParcelRecord

        state = create_new_run(seed=42)
        state.backpack.sent_parcels.append(SentParcelRecord(
            recipient="rRecipient12345",
            supply="food",
            amount=10,
            txid="ABCDEF123456",
            day_sent=3,
            memo="PARCEL|RUN:test|DAY:3|food:10",
        ))

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            loaded = load_game(base)

            assert loaded is not None
            assert len(loaded.backpack.sent_parcels) == 1
            sp = loaded.backpack.sent_parcels[0]
            assert sp.recipient == "rRecipient12345"
            assert sp.supply == "food"
            assert sp.amount == 10
            assert sp.txid == "ABCDEF123456"
            assert sp.day_sent == 3

    def test_backward_compat_no_sent_parcels(self):
        """Old saves without sent_parcels should load with empty list."""
        state = create_new_run(seed=42)
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)

            # Strip sent_parcels from the saved JSON
            save_path = base / SAVE_DIR / SAVE_FILE
            data = json.loads(save_path.read_text(encoding="utf-8"))
            data.get("backpack", {}).pop("sent_parcels", None)
            save_path.write_text(
                json.dumps(data, indent=2), encoding="utf-8",
            )

            loaded = load_game(base)
            assert loaded is not None
            assert loaded.backpack.sent_parcels == []

    def test_backward_compat_missing_doctrine(self):
        """Old saves without doctrine/taboo should load with defaults."""
        state = create_new_run(seed=42)
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)

            # Manually strip the new fields from the JSON
            import json
            save_path = base / ".trail" / "run.json"
            data = json.loads(save_path.read_text(encoding="utf-8"))
            data.pop("doctrine", None)
            data.pop("taboo", None)
            data.pop("rationing_steps", None)
            save_path.write_text(
                json.dumps(data, indent=2), encoding="utf-8",
            )

            loaded = load_game(base)
            assert loaded.doctrine == ""
            assert loaded.taboo == ""
            assert loaded.rationing_steps == 0


class TestSaveLoadDeterminism:
    """ENG-A-01 / ENG-A-08 — a saved-and-continued run must be byte-identical
    to a never-saved run driven by the same intents (GM off)."""

    INTENTS = [
        PlayerIntent(IntentAction.TRAVEL),
        PlayerIntent(IntentAction.REST),
        PlayerIntent(IntentAction.TRAVEL),
        PlayerIntent(IntentAction.HUNT),
        PlayerIntent(IntentAction.TRAVEL),
        PlayerIntent(IntentAction.REST),
    ]

    def _sync_rng_state(self, engine: StepEngine) -> None:
        """Mirror what _save() does so the serialized RNG position is current."""
        engine.state.rng_counter = engine.rng.counter
        engine.state.rng_state = engine.rng.getstate()

    def test_save_load_continue_matches_never_saved(self):
        """Play N, save, load, play M more — must equal a never-saved engine
        driven by the identical intent stream."""
        seed = 31337
        split = 3  # save after this many intents

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)

            # Engine A — saves after `split` intents, reloads, continues.
            engine_a = StepEngine(create_new_run(seed=seed), GMConfig(enabled=False))
            for intent in self.INTENTS[:split]:
                _drive(engine_a, intent)
            self._sync_rng_state(engine_a)
            save_game(engine_a.state, base)

            loaded = load_game(base)
            assert loaded is not None
            # The full PRNG state must survive the round trip.
            assert loaded.rng_state is not None
            engine_a2 = StepEngine(loaded, GMConfig(enabled=False))
            for intent in self.INTENTS[split:]:
                _drive(engine_a2, intent)
            self._sync_rng_state(engine_a2)

            # Engine B — never saved, same seed, same intents end to end.
            engine_b = StepEngine(create_new_run(seed=seed), GMConfig(enabled=False))
            for intent in self.INTENTS:
                _drive(engine_b, intent)
            self._sync_rng_state(engine_b)

            # Full serialized state must be byte-identical.
            assert _state_to_dict(engine_a2.state) == _state_to_dict(engine_b.state)

    def test_legacy_save_without_rng_state_still_loads(self):
        """Saves predating ENG-A-01 (no rng_state) must still load and run via
        the counter-replay fallback — no crash."""
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            engine = StepEngine(create_new_run(seed=7), GMConfig(enabled=False))
            for intent in self.INTENTS[:2]:
                _drive(engine, intent)
            self._sync_rng_state(engine)
            save_game(engine.state, base)

            # Strip rng_state to simulate a pre-ENG-A-01 save.
            save_path = base / SAVE_DIR / SAVE_FILE
            data = json.loads(save_path.read_text(encoding="utf-8"))
            data.pop("rng_state", None)
            save_path.write_text(json.dumps(data, indent=2), encoding="utf-8")

            loaded = load_game(base)
            assert loaded is not None
            assert loaded.rng_state is None
            # Engine reconstructs via counter-replay and keeps running.
            engine2 = StepEngine(loaded, GMConfig(enabled=False))
            _drive(engine2, PlayerIntent(IntentAction.TRAVEL))
