"""Tests for backpack save/load roundtrip and backward compat."""

import json
import tempfile
from pathlib import Path

from escape_the_valley.backpack_models import (
    ParcelRecord,
    SettlementRecord,
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
