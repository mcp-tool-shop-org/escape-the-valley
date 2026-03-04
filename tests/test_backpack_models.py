"""Tests for backpack_models — pure data models + token map."""

from escape_the_valley.backpack_models import (
    TESTNET_URL,
    XRPL_RESOURCES,
    XRPL_TOKEN_MAP,
    BackpackState,
    ParcelRecord,
    PermitRecord,
    SettlementRecord,
)


class TestBackpackState:
    def test_defaults(self):
        bp = BackpackState()
        assert bp.enabled is False
        assert bp.wallet_address == ""
        assert bp.wallet_secret == ""
        assert bp.issuer_address == ""
        assert bp.issuer_secret == ""
        assert bp.trust_lines_ready is False
        assert bp.settlements == []
        assert bp.pending_settlements == []
        assert bp.parcels == []
        assert bp.permits == []
        assert bp.nudge_shown is False
        assert bp.nudge_dismissed is False

    def test_settlement_record_defaults(self):
        rec = SettlementRecord()
        assert rec.day == 0
        assert rec.status == "pending"
        assert rec.deltas == {}
        assert rec.txids == []

    def test_parcel_record(self):
        parcel = ParcelRecord(
            parcel_id="abc:FOD:5",
            sender="rSender123",
            contents={"food": 5},
            day_received=3,
        )
        assert parcel.accepted is False
        assert parcel.contents["food"] == 5

    def test_permit_record(self):
        permit = PermitRecord(permit_id="p1", day_earned=5)
        assert permit.used is False
        assert permit.day_used == 0


class TestTokenMap:
    def test_five_tokens_mapped(self):
        assert len(XRPL_TOKEN_MAP) == 5

    def test_all_codes_three_chars(self):
        for _key, (code, _display) in XRPL_TOKEN_MAP.items():
            assert len(code) == 3

    def test_keys_match_resources(self):
        assert set(XRPL_TOKEN_MAP.keys()) == XRPL_RESOURCES

    def test_expected_mappings(self):
        assert XRPL_TOKEN_MAP["food"] == ("FOD", "FOOD")
        assert XRPL_TOKEN_MAP["water"] == ("WTR", "WATR")
        assert XRPL_TOKEN_MAP["meds"] == ("MED", "MEDS")
        assert XRPL_TOKEN_MAP["ammo"] == ("AMO", "AMMO")
        assert XRPL_TOKEN_MAP["parts"] == ("PRT", "PART")

    def test_testnet_url(self):
        assert "rippletest" in TESTNET_URL
