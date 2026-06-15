"""Tests for save/load roundtrip."""

import json
import tempfile
from datetime import datetime
from pathlib import Path

import pytest

from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.models import GMProfile, JournalEntry
from escape_the_valley.save import (
    SAVE_DIR,
    SAVE_FILE,
    SAVE_VERSION,
    _state_to_dict,
    load_game,
    load_game_result,
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


class TestSecretsSidecar:
    """ledger-001 / ENG-A-04 — wallet/issuer seeds never touch run.json; they
    live in a local .trail/secrets.json sidecar and restore in-memory on load."""

    def _enable_backpack(self, state):
        bp = state.backpack
        bp.enabled = True
        bp.wallet_address = "rWalletAddr0000000000000000000"
        bp.wallet_secret = "sWalletSecretSEED000000000000"
        bp.issuer_address = "rIssuerAddr0000000000000000000"
        bp.issuer_secret = "sIssuerSecretSEED000000000000"
        bp.trust_lines_ready = True

    def test_secrets_absent_from_run_json(self):
        """run.json must contain neither secret, anywhere in the file."""
        state = create_new_run(seed=42)
        self._enable_backpack(state)

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)

            raw = (base / SAVE_DIR / SAVE_FILE).read_text(encoding="utf-8")
            assert state.backpack.wallet_secret not in raw
            assert state.backpack.issuer_secret not in raw
            assert "wallet_secret" not in raw
            assert "issuer_secret" not in raw

            # Public addresses still round-trip in run.json.
            data = json.loads(raw)
            assert data["backpack"]["wallet_address"] == state.backpack.wallet_address
            assert data["backpack"]["issuer_address"] == state.backpack.issuer_address

    def test_secrets_written_to_sidecar(self):
        """secrets.json holds the seeds keyed by run_id."""
        state = create_new_run(seed=42)
        self._enable_backpack(state)

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)

            sidecar = base / SAVE_DIR / "secrets.json"
            assert sidecar.exists()
            store = json.loads(sidecar.read_text(encoding="utf-8"))
            assert store[state.run_id]["wallet_secret"] == state.backpack.wallet_secret
            assert store[state.run_id]["issuer_secret"] == state.backpack.issuer_secret

    def test_load_restores_secrets_from_sidecar(self):
        """A load round-trip repopulates the in-memory secrets."""
        state = create_new_run(seed=42)
        self._enable_backpack(state)

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)

            loaded = load_game(base)
            assert loaded is not None
            assert loaded.backpack.wallet_secret == state.backpack.wallet_secret
            assert loaded.backpack.issuer_secret == state.backpack.issuer_secret
            # And the public fields too.
            assert loaded.backpack.wallet_address == state.backpack.wallet_address

    def test_load_without_sidecar_leaves_secrets_empty(self):
        """If the sidecar is missing, load must not crash — secrets stay empty."""
        state = create_new_run(seed=42)
        self._enable_backpack(state)

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            # Remove the sidecar entirely.
            (base / SAVE_DIR / "secrets.json").unlink()

            loaded = load_game(base)
            assert loaded is not None
            assert loaded.backpack.wallet_secret == ""
            assert loaded.backpack.issuer_secret == ""
            assert loaded.backpack.enabled is True  # rest of backpack intact

    def test_sidecar_preserves_other_runs(self):
        """Saving one run must not clobber another run's secrets in the file."""
        state_a = create_new_run(seed=1)
        self._enable_backpack(state_a)
        state_b = create_new_run(seed=2)
        self._enable_backpack(state_b)
        state_b.backpack.wallet_secret = "sDifferentWalletSEED0000000000"

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state_a, base)
            save_game(state_b, base)

            store = json.loads(
                (base / SAVE_DIR / "secrets.json").read_text(encoding="utf-8")
            )
            assert state_a.run_id in store
            assert state_b.run_id in store
            assert store[state_a.run_id]["wallet_secret"] == state_a.backpack.wallet_secret
            assert store[state_b.run_id]["wallet_secret"] == state_b.backpack.wallet_secret


class TestSaveVersion:
    """ENG-B-04: save_version is written, read, and branched on."""

    def test_save_writes_version(self, tmp_path):
        state = create_new_run(seed=42)
        save_game(state, base_path=tmp_path)
        data = json.loads(
            (tmp_path / SAVE_DIR / SAVE_FILE).read_text(encoding="utf-8")
        )
        assert data["save_version"] == SAVE_VERSION

    def test_legacy_save_without_version_loads(self, tmp_path):
        """A pre-versioning save (no save_version key) loads as version 0."""
        state = create_new_run(seed=42)
        save_game(state, base_path=tmp_path)
        save_path = tmp_path / SAVE_DIR / SAVE_FILE
        data = json.loads(save_path.read_text(encoding="utf-8"))
        data.pop("save_version", None)
        save_path.write_text(json.dumps(data, indent=2), encoding="utf-8")

        result = load_game_result(base_path=tmp_path)
        assert result.ok
        assert result.state is not None

    def test_newer_version_reported_incompatible(self, tmp_path):
        """A save from a future build is 'incompatible', not 'corrupt'."""
        state = create_new_run(seed=42)
        save_game(state, base_path=tmp_path)
        save_path = tmp_path / SAVE_DIR / SAVE_FILE
        data = json.loads(save_path.read_text(encoding="utf-8"))
        data["save_version"] = SAVE_VERSION + 5
        save_path.write_text(json.dumps(data, indent=2), encoding="utf-8")

        result = load_game_result(base_path=tmp_path)
        assert not result.ok
        assert result.reason == "incompatible"
        assert result.state is None
        # Incompatible saves are left in place (a newer build may read them).
        assert save_path.exists()

    def test_result_reason_no_save(self, tmp_path):
        result = load_game_result(base_path=tmp_path)
        assert not result.ok
        assert result.reason == "no_save"

    def test_result_reason_corrupt(self, tmp_path):
        save_dir = tmp_path / SAVE_DIR
        save_dir.mkdir()
        (save_dir / SAVE_FILE).write_text("{bad json", encoding="utf-8")
        result = load_game_result(base_path=tmp_path)
        assert not result.ok
        assert result.reason == "corrupt"

    def test_result_ok_on_good_save(self, tmp_path):
        state = create_new_run(seed=42)
        save_game(state, base_path=tmp_path)
        result = load_game_result(base_path=tmp_path)
        assert result.ok
        assert result.reason == ""
        assert result.state is not None
        assert result.state.seed == 42


class TestCorruptSaveBackup:
    """TCD-B-03: a corrupt run.json is preserved as run.json.corrupt-<ts>
    BEFORE load_game returns, so the next autosave can never overwrite it."""

    def _corrupt_files(self, save_dir):
        return list(save_dir.glob(f"{SAVE_FILE}.corrupt-*"))

    def test_corrupt_json_is_backed_up(self, tmp_path):
        save_dir = tmp_path / SAVE_DIR
        save_dir.mkdir()
        save_path = save_dir / SAVE_FILE
        save_path.write_text("{this is not valid json", encoding="utf-8")

        assert load_game(base_path=tmp_path) is None

        # Original is gone (renamed), a corrupt-<ts> backup exists with content.
        assert not save_path.exists()
        backups = self._corrupt_files(save_dir)
        assert len(backups) == 1
        assert backups[0].read_text(encoding="utf-8") == "{this is not valid json"

    def test_reconstruct_error_is_backed_up(self, tmp_path):
        """Valid JSON but wrong structure → corrupt → backed up."""
        save_dir = tmp_path / SAVE_DIR
        save_dir.mkdir()
        save_path = save_dir / SAVE_FILE
        save_path.write_text(json.dumps({"not": "a game state"}), encoding="utf-8")

        assert load_game(base_path=tmp_path) is None
        assert not save_path.exists()
        assert len(self._corrupt_files(save_dir)) == 1

    def test_next_save_cannot_overwrite_backup(self, tmp_path):
        """After a corrupt load + backup, a fresh save writes a NEW run.json and
        leaves the corrupt evidence untouched."""
        save_dir = tmp_path / SAVE_DIR
        save_dir.mkdir()
        (save_dir / SAVE_FILE).write_text("{corrupt!!!", encoding="utf-8")

        assert load_game(base_path=tmp_path) is None
        backups_before = self._corrupt_files(save_dir)
        assert len(backups_before) == 1

        # A fresh, valid save now lands.
        save_game(create_new_run(seed=7), base_path=tmp_path)
        assert (save_dir / SAVE_FILE).exists()
        # The corrupt backup is still there, byte-for-byte.
        backups_after = self._corrupt_files(save_dir)
        assert len(backups_after) == 1
        assert backups_after[0].read_text(encoding="utf-8") == "{corrupt!!!"

    def test_good_save_is_not_backed_up(self, tmp_path):
        save_game(create_new_run(seed=42), base_path=tmp_path)
        assert load_game(base_path=tmp_path) is not None
        assert self._corrupt_files(tmp_path / SAVE_DIR) == []


class TestAtomicSave:
    """Stage-C: save_game() writes run.json atomically (temp file in the same
    dir, then os.replace) so a crash mid-save can never leave a half-written
    file, and no leftover *.tmp survives a normal save."""

    def _temp_files(self, save_dir):
        # Anything the atomic writer might leave behind: our prefix + .tmp.
        return [
            p
            for p in save_dir.iterdir()
            if p.is_file() and p.name.endswith(".tmp")
        ]

    def test_save_leaves_no_temp_file(self, tmp_path):
        save_game(create_new_run(seed=42), base_path=tmp_path)
        save_dir = tmp_path / SAVE_DIR
        # run.json exists and is valid JSON; no leftover temp file.
        save_path = save_dir / SAVE_FILE
        assert save_path.exists()
        json.loads(save_path.read_text(encoding="utf-8"))  # raises if invalid
        assert self._temp_files(save_dir) == []

    def test_repeated_saves_leave_no_temp_files(self, tmp_path):
        """Overwriting an existing run.json must not accumulate temp files."""
        state = create_new_run(seed=42)
        for _ in range(3):
            save_game(state, base_path=tmp_path)
        save_dir = tmp_path / SAVE_DIR
        assert (save_dir / SAVE_FILE).exists()
        assert self._temp_files(save_dir) == []

    def test_save_uses_temp_then_replace(self, tmp_path, monkeypatch):
        """The write must go through os.replace (atomic), not a direct write to
        run.json — so run.json is only ever the old or new complete file."""
        import escape_the_valley.save as save_mod

        replaced = []
        real_replace = save_mod.os.replace

        def spy_replace(src, dst):
            replaced.append((str(src), str(dst)))
            return real_replace(src, dst)

        monkeypatch.setattr(save_mod.os, "replace", spy_replace)

        save_game(create_new_run(seed=42), base_path=tmp_path)

        save_path = tmp_path / SAVE_DIR / SAVE_FILE
        # Exactly one replace landed run.json, and the source was a temp file.
        landed = [r for r in replaced if r[1] == str(save_path)]
        assert len(landed) == 1
        assert landed[0][0].endswith(".tmp")

    def test_failed_write_leaves_run_json_intact_and_no_temp(self, tmp_path):
        """If the atomic replace fails after a good save already exists, the
        prior run.json survives whole and no temp file is left behind."""
        import escape_the_valley.save as save_mod

        # First, a good save so a complete run.json is on disk.
        save_game(create_new_run(seed=42), base_path=tmp_path)
        save_dir = tmp_path / SAVE_DIR
        save_path = save_dir / SAVE_FILE
        good = save_path.read_text(encoding="utf-8")

        # Now make os.replace blow up to simulate a crash at the replace step.
        def boom(src, dst):
            raise OSError("simulated crash during replace")

        original_replace = save_mod.os.replace
        save_mod.os.replace = boom
        try:
            with pytest.raises(OSError):
                save_game(create_new_run(seed=99), base_path=tmp_path)
        finally:
            save_mod.os.replace = original_replace

        # The original run.json is untouched (only-ever-fully-replaced).
        assert save_path.read_text(encoding="utf-8") == good
        # And the failed attempt cleaned up its temp file.
        leftover = [
            p for p in save_dir.iterdir() if p.is_file() and p.name.endswith(".tmp")
        ]
        assert leftover == []


class TestCorruptBackupUniqueness:
    """Stage-C: two corrupt loads that resolve to the same microsecond
    timestamp must produce two distinct backups — the second must not silently
    overwrite the first (Path.rename clobbers)."""

    def _corrupt_files(self, save_dir):
        return list(save_dir.glob(f"{SAVE_FILE}.corrupt-*"))

    def test_two_corrupt_backups_in_same_microsecond_both_survive(
        self, tmp_path, monkeypatch
    ):
        import escape_the_valley.save as save_mod

        # Freeze the timestamp so both backups target the same base name,
        # forcing the uniquifier path (the real-world collision case).
        class _FrozenDatetime:
            @staticmethod
            def now(tz=None):
                return datetime(2026, 6, 15, 12, 0, 0, 0, tzinfo=tz)

        monkeypatch.setattr(save_mod, "datetime", _FrozenDatetime)

        save_dir = tmp_path / SAVE_DIR
        save_dir.mkdir()
        save_path = save_dir / SAVE_FILE

        # First corrupt load + backup.
        save_path.write_text("{first corrupt", encoding="utf-8")
        assert load_game(base_path=tmp_path) is None

        # Second corrupt load + backup — same frozen microsecond.
        save_path.write_text("{second corrupt", encoding="utf-8")
        assert load_game(base_path=tmp_path) is None

        backups = self._corrupt_files(save_dir)
        assert len(backups) == 2
        contents = sorted(b.read_text(encoding="utf-8") for b in backups)
        assert contents == ["{first corrupt", "{second corrupt"]
