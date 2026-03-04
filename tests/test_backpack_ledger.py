"""Tests for XRPL postcard in ledger and adapter backpack_status."""

from escape_the_valley.backpack_models import BackpackState, SettlementRecord
from escape_the_valley.ledger import build_xrpl_postcard
from escape_the_valley.models import (
    PartyMember,
    PartyState,
    RunState,
    SuppliesState,
    WagonState,
)


def _make_state(**overrides) -> RunState:
    defaults = dict(
        run_id="test",
        seed=1,
        day=15,
        distance_traveled=120,
        total_distance=300,
        game_over=True,
        victory=True,
        cause_of_death="",
        party=PartyState(
            members=[PartyMember(name="Elias", health=60)],
            morale=50,
        ),
        wagon=WagonState(condition=30),
        supplies=SuppliesState(items={"food": 10, "water": 15}),
    )
    defaults.update(overrides)
    return RunState(**defaults)


class TestXrplPostcard:
    def test_includes_trail_ledger(self):
        """XRPL postcard should include standard trail ledger content."""
        state = _make_state()
        postcard = build_xrpl_postcard(state)
        text = "\n".join(postcard)
        assert "TRAIL LEDGER" in text
        assert "Elias" in text

    def test_includes_receipts(self):
        """XRPL postcard should list settlement receipts."""
        state = _make_state()
        state.backpack = BackpackState(
            enabled=True,
            wallet_address="rTestWallet12345678",
            settlements=[
                SettlementRecord(
                    day=3,
                    location="Millford",
                    deltas={"food": -5, "water": -3},
                    txids=["ABCDEF123456789012"],
                    status="settled",
                    memo="TRAIL|RUN:test|DAY:3",
                ),
            ],
        )
        postcard = build_xrpl_postcard(state)
        text = "\n".join(postcard)
        assert "RECEIPTS ON LEDGER" in text
        assert "Millford" in text
        assert "ABCDEF123456..." in text

    def test_includes_wallet_address(self):
        state = _make_state()
        state.backpack = BackpackState(
            enabled=True,
            wallet_address="rTestWalletABCD1234",
        )
        postcard = build_xrpl_postcard(state)
        text = "\n".join(postcard)
        assert "rTes" in text  # first 4
        assert "1234" in text  # last 4
        assert "XRPL Testnet" in text

    def test_includes_tagline(self):
        state = _make_state()
        postcard = build_xrpl_postcard(state)
        text = "\n".join(postcard)
        assert "Receipts don't make the trail kinder" in text

    def test_no_settlements_still_works(self):
        state = _make_state()
        postcard = build_xrpl_postcard(state)
        text = "\n".join(postcard)
        assert "TRAIL LEDGER" in text
        assert "RECEIPTS ON LEDGER" not in text


class TestAdapterBackpackStatus:
    def test_backpack_status_populated(self):
        """FrameState should include backpack_status."""
        from escape_the_valley.worldgen import create_new_run

        state = create_new_run(seed=42)

        from escape_the_valley.gm import GMConfig
        from escape_the_valley.step_engine import StepEngine

        gm_config = GMConfig(enabled=False)
        engine = StepEngine(state, gm_config)

        from escape_the_valley.adapter import state_to_frame

        frame = state_to_frame(engine)
        assert frame.backpack_status != ""
        assert "OFF" in frame.backpack_status
