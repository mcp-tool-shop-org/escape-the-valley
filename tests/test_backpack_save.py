"""Tests for backpack save/load roundtrip and backward compat."""

import json
import tempfile
from pathlib import Path

from escape_the_valley.backpack_models import (
    ParcelRecord,
    SettlementRecord,
    minted_snapshot_of,
    stamp_minted_snapshot,
)
from escape_the_valley.save import load_game, save_game
from escape_the_valley.worldgen import create_new_run


class TestBackpackSaveRoundtrip:
    def test_roundtrip_default_backpack(self):
        """New run with default backpack should roundtrip cleanly."""
        state = create_new_run(seed=42)
        assert state.backpack.enabled is False

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            loaded = load_game(base)

            assert loaded is not None
            assert loaded.backpack.enabled is False
            assert loaded.backpack.wallet_address == ""
            assert loaded.backpack.nudge_shown is False

    def test_roundtrip_enabled_backpack(self):
        """Enabled backpack with settlements should roundtrip."""
        state = create_new_run(seed=42)
        state.backpack.enabled = True
        state.backpack.wallet_address = "rTestWallet123"
        state.backpack.wallet_secret = "sTestSecret"
        state.backpack.issuer_address = "rIssuer456"
        state.backpack.issuer_secret = "sIssuerSecret"
        state.backpack.trust_lines_ready = True
        state.backpack.last_settled_supplies = {"food": 45, "water": 40}
        state.backpack.last_settlement_day = 3
        state.backpack.nudge_shown = True
        state.backpack.nudge_dismissed = False

        state.backpack.settlements.append(SettlementRecord(
            day=3,
            location="Millford",
            deltas={"food": -5, "water": -10},
            txids=["ABC123DEF456"],
            status="settled",
            memo="TRAIL|RUN:test|DAY:3",
            timestamp="2026-03-04T00:00:00+00:00",
        ))

        state.backpack.parcels.append(ParcelRecord(
            parcel_id="rSender:FOD:5",
            sender="rSender",
            contents={"food": 5},
            accepted=True,
            day_received=4,
        ))

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            loaded = load_game(base)

            assert loaded.backpack.enabled is True
            assert loaded.backpack.wallet_address == "rTestWallet123"
            assert loaded.backpack.trust_lines_ready is True
            assert loaded.backpack.last_settlement_day == 3
            assert len(loaded.backpack.settlements) == 1
            assert loaded.backpack.settlements[0].location == "Millford"
            assert loaded.backpack.settlements[0].txids == ["ABC123DEF456"]
            assert len(loaded.backpack.parcels) == 1
            assert loaded.backpack.parcels[0].accepted is True

    def test_secrets_never_written_to_run_json(self):
        """A-06 / ledger-001: enabled backpack saves must NOT leak seeds.

        Wallet/issuer seeds live only in the local .trail/secrets.json sidecar
        (the engine executor moved them there); run.json must be secret-free.
        """
        state = create_new_run(seed=42)
        state.backpack.enabled = True
        state.backpack.wallet_address = "rPlayerAddr"
        state.backpack.wallet_secret = "sEDPlayerSeedTopSecret"
        state.backpack.issuer_address = "rIssuerAddr"
        state.backpack.issuer_secret = "sEDIssuerSeedTopSecret"
        state.backpack.trust_lines_ready = True

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)

            run_text = (base / ".trail" / "run.json").read_text(encoding="utf-8")
            # The seeds must not appear anywhere in run.json.
            assert "sEDPlayerSeedTopSecret" not in run_text
            assert "sEDIssuerSeedTopSecret" not in run_text
            assert "wallet_secret" not in run_text
            assert "issuer_secret" not in run_text

            # But the run loads back with seeds restored from the sidecar,
            # so the backpack stays usable.
            loaded = load_game(base)
            assert loaded is not None
            assert loaded.backpack.wallet_secret == "sEDPlayerSeedTopSecret"
            assert loaded.backpack.issuer_secret == "sEDIssuerSeedTopSecret"
            assert loaded.backpack.wallet_address == "rPlayerAddr"

    def test_backward_compat_no_backpack(self):
        """Old saves without backpack key should load with defaults."""
        state = create_new_run(seed=42)
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)

            # Strip backpack from saved JSON
            save_path = base / ".trail" / "run.json"
            data = json.loads(save_path.read_text(encoding="utf-8"))
            data.pop("backpack", None)
            save_path.write_text(
                json.dumps(data, indent=2), encoding="utf-8",
            )

            loaded = load_game(base)
            assert loaded.backpack.enabled is False
            assert loaded.backpack.wallet_address == ""
            assert loaded.backpack.settlements == []

    def test_minted_initial_survives_save_via_permit_shim(self):
        """F-a6efdd6c: save.py does not yet persist minted_initial, but the
        reserved permit shim round-trips through run.json so conservation
        can be replayed against a reloaded player save.
        """
        snap = {"food": 50, "water": 50, "meds": 10, "ammo": 20, "parts": 10}
        state = create_new_run(seed=42)
        state.backpack.enabled = True
        state.backpack.wallet_address = "rTestWallet123"
        state.backpack.last_settled_supplies = {
            "food": 40, "water": 45, "meds": 10, "ammo": 18, "parts": 9,
        }
        stamp_minted_snapshot(state.backpack, snap)
        # Dedicated field is in-memory; save.py will drop it on load.
        assert state.backpack.minted_initial == snap

        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir)
            save_game(state, base)
            loaded = load_game(base)

            # Direct field is gone until engine persists it...
            # ...but the shim restores a complete snapshot.
            restored = minted_snapshot_of(loaded.backpack)
            assert restored == snap
            # And conservation is not tautological: last_settled != minted.
            assert loaded.backpack.last_settled_supplies["food"] == 40
            assert restored["food"] == 50
