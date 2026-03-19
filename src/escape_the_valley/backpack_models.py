"""Ledger Backpack data models — pure dataclasses, no xrpl imports.

Safe to import from models.py, save.py, adapter.py without pulling
in the optional xrpl-py dependency.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class SettlementRecord:
    """One checkpoint settlement on XRPL Testnet."""

    day: int = 0
    location: str = ""
    deltas: dict[str, int] = field(default_factory=dict)
    txids: list[str] = field(default_factory=list)
    status: str = "pending"  # "settled" | "pending" | "failed"
    memo: str = ""
    timestamp: str = ""


@dataclass
class ParcelRecord:
    """An incoming token parcel from another traveler."""

    parcel_id: str = ""
    sender: str = ""
    contents: dict[str, int] = field(default_factory=dict)
    txid: str = ""
    accepted: bool = False
    day_received: int = 0


@dataclass
class SentParcelRecord:
    """An outgoing parcel sent to another traveler via XRP + memo."""

    recipient: str = ""
    supply: str = ""  # game key (e.g. "food")
    amount: int = 0
    txid: str = ""
    day_sent: int = 0
    memo: str = ""


@dataclass
class PermitRecord:
    """A one-time clutch tool earned from a hard choice."""

    permit_id: str = ""
    txid: str = ""
    used: bool = False
    day_earned: int = 0
    day_used: int = 0


@dataclass
class BackpackState:
    """Full state for the optional XRPL Ledger Backpack."""

    enabled: bool = False
    wallet_address: str = ""
    wallet_secret: str = ""  # testnet only — acceptable for play money
    issuer_address: str = ""
    issuer_secret: str = ""  # testnet only — per-run "Trail Authority"
    trust_lines_ready: bool = False

    # Snapshot of the 5 XRPL-tracked resources at last settlement
    last_settled_supplies: dict[str, int] = field(default_factory=dict)
    last_settlement_day: int = 0

    # Settlement history
    settlements: list[SettlementRecord] = field(default_factory=list)
    pending_settlements: list[SettlementRecord] = field(default_factory=list)

    # Parcels from other travelers
    parcels: list[ParcelRecord] = field(default_factory=list)

    # Outgoing parcels sent to other travelers
    sent_parcels: list[SentParcelRecord] = field(default_factory=list)

    # Permits (clutch tools)
    permits: list[PermitRecord] = field(default_factory=list)

    # UX state
    nudge_shown: bool = False
    nudge_dismissed: bool = False  # "Not now" was clicked — never nag again


# ── Currency mapping ──────────────────────────────────────────────
# Game key → (XRPL 3-char standard code, 4-char display label)
# Only these 5 resources are tracked on-chain.

XRPL_TOKEN_MAP: dict[str, tuple[str, str]] = {
    "food": ("FOD", "FOOD"),
    "water": ("WTR", "WATR"),
    "meds": ("MED", "MEDS"),
    "ammo": ("AMO", "AMMO"),
    "parts": ("PRT", "PART"),
}

XRPL_RESOURCES: set[str] = set(XRPL_TOKEN_MAP.keys())

TESTNET_URL = "https://s.altnet.rippletest.net:51234/"
