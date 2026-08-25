"""Ledger Backpack TUI overlays — menu, nudge, enable flow, parcels."""

from __future__ import annotations

import re

from textual.markup import escape
from textual.widgets import Static

from .backpack_models import XRPL_EXTRA_PIP


def _escape_dynamic(text: str) -> str:
    """Escape a fragment spliced into a markup template.

    ``textual.markup.escape`` only wraps complete tag-shaped runs. A leftover
    ``[`` (e.g. truncating ``rSender[/pwn]`` to ``rSender[...``) still opens a
    tag into the chrome and raises MarkupError. Neutralize those after escape.
    """
    return re.sub(r"(?<!\\)\[", r"\\[", escape(text))

# ── Ledger Menu Overlay ──────────────────────────────────────────

LEDGER_OFF_TEXT = """\
[b]Ledger Backpack: OFF[/b]

Track your 5 core supplies (FOOD, WATR, MEDS, AMMO, PART)
as receipted tokens on XRPL Testnet.

Optional. The trail is the same either way.

  [b]E[/b]) Enable Backpack
  [b]L[/b]) Learn what this does
  [b]Esc[/b]) Close
"""

LEDGER_ON_TEXT = """\
[b]Ledger Backpack: ON[/b]  (Testnet)

Supplies are receipted at town checkpoints.
Parcels may arrive from other travelers.

  [b]W[/b]) Wallet info
  [b]P[/b]) Send parcel to traveler
  [b]S[/b]) Settle now
  [b]D[/b]) Disable Backpack
  [b]Esc[/b]) Close
"""


class LedgerMenuOverlay(Static):
    """Toggle-able ledger menu panel."""

    def update_from_state(self, enabled: bool) -> None:
        self.update(LEDGER_ON_TEXT if enabled else LEDGER_OFF_TEXT)


# ── Nudge Overlay ────────────────────────────────────────────────

NUDGE_TEXT = """\
[b]Ledger Backpack available[/b]

Track your supplies on XRPL Testnet.
Optional — the trail works the same either way.

  [b]E[/b]) Enable now
  [b]N[/b]) Not now (won't ask again)
  [b]L[/b]) Learn more
"""


class NudgeOverlay(Static):
    """First-time backpack nudge at town arrival."""

    def on_mount(self) -> None:
        self.update(NUDGE_TEXT)


# ── Enable Flow Overlay ─────────────────────────────────────────

ENABLE_PROGRESS_TEXT = """\
[b]Enabling Ledger Backpack...[/b]

Creating wallets on XRPL Testnet...
This may take a moment.
"""

ENABLE_SUCCESS_TEXT = """\
[b]Ledger Backpack: Enabled[/b]

Your pack is now receipted.
Supplies will settle at town checkpoints.

Wallet: {address}

Press [b]Esc[/b] to continue.
"""

ENABLE_FAILURE_TEXT = """\
[b]Couldn't enable right now[/b]

{message}

The trail continues. You can try again
at the next town from the Ledger menu (L).

Press [b]Esc[/b] to continue.
"""


class EnableFlowOverlay(Static):
    """Multi-step enable flow display."""

    def show_progress(self) -> None:
        self.update(ENABLE_PROGRESS_TEXT)

    def show_success(self, address: str) -> None:
        short = f"{address[:4]}...{address[-4:]}" if len(address) > 10 else address
        self.update(ENABLE_SUCCESS_TEXT.format(address=_escape_dynamic(short)))

    def show_failure(self, message: str) -> None:
        # message may carry caller-supplied text (e.g. an exception string
        # surfaced from a failed enable attempt). Static.update() parses
        # markup eagerly (unlike notify()/Toast, which defer to paint time),
        # so leftover '[' or an orphan "[/tag]" would raise MarkupError and
        # crash the whole app. Escape only the dynamic fragment so the
        # literal [b]/[/b] chrome in ENABLE_FAILURE_TEXT still renders bold.
        self.update(ENABLE_FAILURE_TEXT.format(message=_escape_dynamic(message)))


# ── Parcel Notification ──────────────────────────────────────────

PARCEL_TEXT = """\
[b]Parcel arrived![/b]

From: {sender}
Contents: {contents}

  [b]A[/b]) Accept
  [b]R[/b]) Refuse
"""


class ParcelNotification(Static):
    """Town parcel notification."""

    def show_parcel(self, sender: str, contents: str) -> None:
        short_sender = f"{sender[:8]}..." if len(sender) > 12 else sender
        self.update(PARCEL_TEXT.format(
            sender=_escape_dynamic(short_sender),
            contents=_escape_dynamic(contents),
        ))


# ── Wallet Info Overlay ──────────────────────────────────────────

WALLET_TEXT = """\
[b]Wallet Info[/b]

Address: {address}
Issuer:  {issuer}
Trust lines: {trust_lines}
Settlements: {settlements}
Pending: {pending}

{balances_text}
Press [b]Esc[/b] to close.
"""


class WalletInfoOverlay(Static):
    """Wallet details display."""

    def update_from_info(self, info: dict) -> None:
        balances = info.get("balances", {})
        if balances:
            bal_lines = "\n".join(
                f"  {code}: {amount}" for code, amount in balances.items()
            )
            balances_text = f"Balances:\n{bal_lines}"
        elif info.get("extra_missing"):
            # F-64e78470: extra gone after an enabled save is not the
            # empty-wallet case and not a network miss. Name the pip extra.
            balances_text = (
                f"Balances: unavailable (xrpl extra missing). "
                f"Install with: {XRPL_EXTRA_PIP}"
            )
        elif info.get("balances_error"):
            # ledger-B08: an empty balances dict alone is ambiguous — it could be
            # a genuinely empty wallet OR an unreachable ledger. The error flag
            # lets the overlay say which, instead of implying the pack is empty.
            balances_text = "Balances: unavailable (couldn't reach the ledger)"
        else:
            balances_text = "Balances: unavailable"

        self.update(WALLET_TEXT.format(
            address=_escape_dynamic(info.get("address_short", "?")),
            issuer=_escape_dynamic(info.get("issuer", "?")),
            trust_lines="Yes" if info.get("trust_lines") else "No",
            settlements=_escape_dynamic(str(info.get("settlements", 0))),
            pending=_escape_dynamic(str(info.get("pending", 0))),
            balances_text=_escape_dynamic(balances_text),
        ))


# ── Learn More Overlay ───────────────────────────────────────────

LEARN_TEXT = """\
[b]What is the Ledger Backpack?[/b]

The Ledger Backpack tracks your 5 core supplies
as tokens on the XRPL Testnet:

  FOOD (FOD) • WATR (WTR) • MEDS (MED)
  AMMO (AMO) • PART (PRT)

At each town, your supply changes are "settled" —
recorded as transactions on a public ledger.

Other travelers can send you parcels (bonus supplies)
using your wallet address. Parcels are capped
so they don't break the game balance.

This is testnet — no real money. Just receipts.

"Receipts don't make the trail kinder.
 They just make it honest."

Press [b]Esc[/b] to close.
"""


class LearnMoreOverlay(Static):
    """Explainer for the Ledger Backpack."""

    def on_mount(self) -> None:
        self.update(LEARN_TEXT)


# ── Send Parcel Overlay ────────────────────────────────────────

SEND_PARCEL_TEXT = """\
[b]Send Parcel[/b]

Send supplies to another traveler's wallet.
They'll find your parcel at their next town.

Supply types: food, water, meds, ammo, parts

Current supplies:
{supplies_text}

Enter command in the format:
  [b]<address> <supply> <amount>[/b]

Type [b]cancel[/b] to go back.
"""

SEND_PARCEL_SUCCESS_TEXT = """\
[b]Parcel sent![/b]

{message}

Press [b]Esc[/b] to continue.
"""

SEND_PARCEL_FAILURE_TEXT = """\
[b]Send failed[/b]

{message}

Press [b]Esc[/b] to try again.
"""


class SendParcelOverlay(Static):
    """Parcel send flow display."""

    def show_form(self, supplies_text: str) -> None:
        self.update(SEND_PARCEL_TEXT.format(
            supplies_text=_escape_dynamic(supplies_text),
        ))

    def show_success(self, message: str) -> None:
        self.update(SEND_PARCEL_SUCCESS_TEXT.format(
            message=_escape_dynamic(message),
        ))

    def show_failure(self, message: str) -> None:
        # message routinely carries raw XRPL/HTTP error text, or an invalid
        # address/amount echoed back from the caller's parser (see
        # tui_app.py on_input_submitted) -- untrusted, unlike the [b]/[/b]
        # chrome in SEND_PARCEL_FAILURE_TEXT. Static.update() parses markup
        # eagerly and synchronously (unlike notify()/Toast, which defer to
        # paint time), so leftover '[' or an orphan "[/tag]" here would
        # raise MarkupError and crash the whole app. Escape only the
        # dynamic fragment so the literal chrome still renders bold.
        self.update(SEND_PARCEL_FAILURE_TEXT.format(
            message=_escape_dynamic(message),
        ))
