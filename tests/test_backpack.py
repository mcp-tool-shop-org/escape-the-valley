"""Tests for BackpackManager — mock XRPL, no network."""


from escape_the_valley.backpack import (
    BackpackManager,
    _build_memo,
    _hex_encode,
    _shorten_address,
)
from escape_the_valley.models import RunState, SuppliesState


def _make_state(**overrides) -> RunState:
    defaults = dict(
        run_id="test1234",
        seed=1,
        day=5,
        supplies=SuppliesState(items={
            "food": 50, "water": 50, "meds": 5,
            "ammo": 20, "parts": 3,
        }),
    )
    defaults.update(overrides)
    return RunState(**defaults)


class TestHelpers:
    def test_hex_encode(self):
        result = _hex_encode("hello")
        assert result == "68656C6C6F"

    def test_shorten_address_short(self):
        assert _shorten_address("rABC") == "rABC"

    def test_shorten_address_long(self):
        addr = "rN7qKvMzTdmhcjbw1234567890xKp"
        result = _shorten_address(addr)
        assert result.startswith("rN7q")
        assert result.endswith("xKp")  # last 4
        assert "..." in result

    def test_build_memo_format(self):
        deltas = {"food": -3, "water": 5}
        memos = _build_memo("run1", 5, deltas)
        assert len(memos) == 1
        # Memo data is hex-encoded
        assert memos[0].memo_data is not None


class TestBackpackManagerAvailability:
    def test_available_reflects_import(self):
        mgr = BackpackManager()
        # _HAS_XRPL may or may not be True depending on env
        assert isinstance(mgr.available, bool)

    def test_status_line_off(self):
        state = _make_state()
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "OFF" in line

    def test_status_line_on(self):
        state = _make_state()
        state.backpack.enabled = True
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "ON" in line

    def test_status_line_unsettled(self):
        from escape_the_valley.backpack_models import SettlementRecord

        state = _make_state()
        state.backpack.enabled = True
        state.backpack.pending_settlements = [
            SettlementRecord(day=3, location="Test", status="pending"),
        ]
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "Unsettled: 1 checkpoint" in line
        assert "checkpoints" not in line  # singular

    def test_status_line_unsettled_plural(self):
        from escape_the_valley.backpack_models import SettlementRecord

        state = _make_state()
        state.backpack.enabled = True
        state.backpack.pending_settlements = [
            SettlementRecord(day=3, location="A", status="pending"),
            SettlementRecord(day=5, location="B", status="pending"),
        ]
        mgr = BackpackManager()
        line = mgr.status_line(state)
        assert "Unsettled: 2 checkpoints" in line

    def test_wallet_info_no_wallet(self):
        state = _make_state()
        mgr = BackpackManager()
        info = mgr.wallet_info(state)
        assert info["status"] == "No wallet"

    def test_wallet_info_with_wallet(self):
        state = _make_state()
        state.backpack.wallet_address = "rTestAddress12345678"
        state.backpack.issuer_address = "rIssuerAddress12345"
        state.backpack.trust_lines_ready = True
        mgr = BackpackManager()
        info = mgr.wallet_info(state)
        assert "address" in info
        assert info["trust_lines"] is True


class TestDisable:
    def test_disable_marks_off(self):
        state = _make_state()
        state.backpack.enabled = True
        mgr = BackpackManager()
        mgr.disable(state)
        assert state.backpack.enabled is False


class TestAcceptParcel:
    def test_accept_applies_supplies(self):
        from escape_the_valley.backpack_models import ParcelRecord

        state = _make_state()
        parcel = ParcelRecord(
            parcel_id="test:FOD:5",
            sender="rSender",
            contents={"food": 5},
            day_received=3,
        )
        mgr = BackpackManager()
        result = mgr.accept_parcel(parcel, state)
        assert result is True
        assert parcel.accepted is True
        assert state.supplies.food == 55  # 50 + 5

    def test_accept_respects_cap(self):
        from escape_the_valley.backpack_models import ParcelRecord

        state = _make_state()
        parcel = ParcelRecord(
            parcel_id="test:FOD:100",
            sender="rSender",
            contents={"food": 100},
            day_received=3,
        )
        mgr = BackpackManager()
        mgr.accept_parcel(parcel, state, cap=20)
        assert state.supplies.food == 70  # 50 + 20 (capped)


class TestSettleNoXrpl:
    def test_settle_not_enabled(self):
        state = _make_state()
        mgr = BackpackManager()
        result = mgr.settle(state, "TestTown")
        assert result.success is False
        assert "not enabled" in result.message.lower() or "not available" in result.message.lower()

    def test_enable_without_xrpl(self):
        """Enable should fail gracefully if xrpl-py not installed."""
        mgr = BackpackManager()
        if not mgr.available:
            state = _make_state()
            result = mgr.enable(state)
            assert result.success is False
            assert "xrpl" in result.message.lower()


class TestSettleDeltaComputation:
    def test_no_changes_no_settlement(self):
        state = _make_state()
        state.backpack.enabled = True
        state.backpack.wallet_address = "rTest"
        state.backpack.wallet_secret = "sTest"
        state.backpack.issuer_address = "rIssuer"
        state.backpack.issuer_secret = "sIssuer"
        # Set last settled = current
        state.backpack.last_settled_supplies = {
            "food": 50, "water": 50, "meds": 5,
            "ammo": 20, "parts": 3,
        }

        mgr = BackpackManager()
        if not mgr.available:
            return  # Can't test XRPL ops without xrpl-py
        result = mgr.settle(state, "TestTown")
        # No deltas → success with "No changes"
        assert result.success is True
        assert "no changes" in result.message.lower()
