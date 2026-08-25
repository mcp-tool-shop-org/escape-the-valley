"""Ledger Backpack TUI overlays — menu, nudge, enable flow, parcels."""

from __future__ import annotations

import re

from textual.markup import escape
from textual.widgets import Static

from .backpack_models import XRPL_EXTRA_PIP, XRPL_TOKEN_MAP


def _escape_dynamic(text: str) -> str:
    """Escape a fragment spliced into a markup template.

    ``textual.markup.escape`` only wraps complete tag-shaped runs. A leftover
    ``[`` (e.g. truncating ``rSender[/pwn]`` to ``rSender[...``) still opens a
    tag into the chrome and raises MarkupError. Neutralize those after escape.
    """
    return re.sub(r"(?<!\\)\[", r"\\[", escape(text))


def _short_r_address(addr: str) -> str:
    """Canonical short form (rN7q...4xKp). Same rule as backpack._shorten_address."""
    if len(addr) <= 10:
        return addr
    return f"{addr[:4]}...{addr[-4:]}"


def _token_display_label(code: str) -> str:
    """4-char overlay label for an on-chain ticker (FOOD not FOD).

    Wallet balances arrive as XRPL 3-char codes. Sibling overlays use the
    4-char display names; LEARN_TEXT pairs them as ``FOOD (FOD)``.
    """
    for key, (ticker, display) in XRPL_TOKEN_MAP.items():
        if code == ticker:
            return f"{display} ({ticker})"
        if code == display or code.lower() == key:
            return display
    return code


# ── Ledger Menu Overlay ──────────────────────────────────────────

# 80x24 inner box is 34x9 (width 50%, height 60%, padding 1 2). E/L/Esc
# (OFF) and W/P/S/D/Esc (ON) sit in the first painted lines so wrapping
# body copy cannot push them below the fold. Do not rely on overflow-y.
LEDGER_OFF_TEXT = """\
[b]Ledger Backpack: OFF[/b]
  [b]E[/b]) Enable Backpack
  [b]L[/b]) Learn what this does
  [b]Esc[/b]) Close
Track FOOD, WATR, MEDS, AMMO, PART on XRPL Testnet.
Optional. The trail is the same either way.
"""

LEDGER_ON_TEXT = """\
[b]Ledger Backpack: ON[/b]  (Testnet)
  [b]W[/b]) Wallet  [b]R[/b]) Proof this save
  [b]P[/b]) Send parcel to traveler
  [b]S[/b]) Settle now
  [b]D[/b]) Disable Backpack
  [b]Esc[/b]) Close
Supplies settle at town. Parcels may arrive.
"""


class LedgerMenuOverlay(Static):
    """Toggle-able ledger menu panel."""

    def update_from_state(self, enabled: bool) -> None:
        self.update(LEDGER_ON_TEXT if enabled else LEDGER_OFF_TEXT)


# ── Nudge Overlay ────────────────────────────────────────────────

# A/R-style packed actions: max-height 40% at 80x24 is inner 34x4.
# E/N/L must sit in those painted lines. This overlay offers no Esc row
# (Escape still closes via on_key).
NUDGE_TEXT = """\
[b]Ledger Backpack available[/b]
  [b]E[/b]) Enable now   [b]N[/b]) Not now
  [b]L[/b]) Learn more
Optional. Same trail either way.
"""


class NudgeOverlay(Static):
    """First-time backpack nudge at town arrival."""

    def on_mount(self) -> None:
        self.update(NUDGE_TEXT)


# ── Enable Flow Overlay ─────────────────────────────────────────

# No spare blank rows: at 80x24 the overlay is 34x7 inner (50% x max-height
# 50%, padding 1 2). Esc must sit in those painted lines, not below the fold.
ENABLE_PROGRESS_TEXT = """\
[b]Enabling Ledger Backpack...[/b]
Creating wallets on XRPL Testnet...
This may take a moment.
Press [b]Esc[/b] to close.
"""

ENABLE_SUCCESS_TEXT = """\
[b]Ledger Backpack: Enabled[/b]
Wallet: {address}
Your pack is now receipted. Supplies settle at town checkpoints.
Press [b]Esc[/b] to continue.
"""

# Esc above wrapping {message}: production faucet / extra-missing copy
# used to paint only heading + wrapped body, with Esc in visual.plain.
# Trailer restated the faucet sentence — dropped so the message itself fits.
ENABLE_FAILURE_TEXT = """\
[b]Couldn't enable right now[/b]
Press [b]Esc[/b] to continue.
{message}
"""


class EnableFlowOverlay(Static):
    """Multi-step enable flow display."""

    def show_progress(self) -> None:
        self.update(ENABLE_PROGRESS_TEXT)

    def show_success(self, address: str) -> None:
        # Full classic r-address, not the 4...4 short form. Escape the
        # fragment so chrome [b]Esc[/b] stays a real bold span.
        self.update(ENABLE_SUCCESS_TEXT.format(
            address=_escape_dynamic(address),
        ))

    def show_failure(self, message: str) -> None:
        # message may carry caller-supplied text (e.g. an exception string
        # surfaced from a failed enable attempt). Static.update() parses
        # markup eagerly (unlike notify()/Toast, which defer to paint time),
        # so leftover '[' or an orphan "[/tag]" would raise MarkupError and
        # crash the whole app. Escape only the dynamic fragment so the
        # literal [b]/[/b] chrome in ENABLE_FAILURE_TEXT still renders bold.
        self.update(ENABLE_FAILURE_TEXT.format(message=_escape_dynamic(message)))


# ── Parcel Notification ──────────────────────────────────────────

# A/R share one row so max-height 40% at 80x24 (inner 34x4) still paints
# the action keys. Do not rely on overflow-y: auto — there is no scroll hint.
PARCEL_TEXT = """\
[b]Parcel arrived![/b]
From: {sender}
Contents: {contents}
  [b]A[/b]) Accept   [b]R[/b]) Refuse
"""


class ParcelNotification(Static):
    """Town parcel notification."""

    def show_parcel(self, sender: str, contents: str) -> None:
        # Same rN7q...4xKp stem as Wallet/Enable — prefix-8 collided.
        self.update(PARCEL_TEXT.format(
            sender=_escape_dynamic(_short_r_address(sender)),
            contents=_escape_dynamic(contents),
        ))


# ── Wallet Info Overlay ──────────────────────────────────────────

# 80x24 inner box is 34x9 (width 50%, height 60%, padding 1 2). FOOD rows
# and Esc go above the address block so they paint even when issuer/pending
# wrap. Two-column balances are packed in update_from_info.
WALLET_TEXT = """\
[b]Wallet Info[/b]
{balances_text}
Press [b]Esc[/b] to close.
Address: {address_short}
{address}
Issuer:  {issuer}
Trust lines: {trust_lines}
Settlements: {settlements}  Pending: {pending}
"""


class WalletInfoOverlay(Static):
    """Wallet details display."""

    def update_from_info(self, info: dict) -> None:
        balances = info.get("balances", {})
        if balances:
            items = [
                f"{_token_display_label(code)}: {amount}"
                for code, amount in balances.items()
            ]
            # Two columns: five tokens fit in three 34-col rows (FOOD+WATR,
            # MEDS+AMMO, PART). A stacked list clips MEDS/Esc at 80x24.
            packed = [
                "  ".join(items[i:i + 2])
                for i in range(0, len(items), 2)
            ]
            balances_text = "\n".join(packed)
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

        full_addr = info.get("address") or info.get("address_short", "?")
        short_addr = info.get("address_short") or _short_r_address(str(full_addr))
        self.update(WALLET_TEXT.format(
            address_short=_escape_dynamic(str(short_addr)),
            address=_escape_dynamic(str(full_addr)),
            issuer=_escape_dynamic(info.get("issuer", "?")),
            trust_lines="Yes" if info.get("trust_lines") else "No",
            settlements=_escape_dynamic(str(info.get("settlements", 0))),
            pending=_escape_dynamic(str(info.get("pending", 0))),
            balances_text=_escape_dynamic(balances_text),
        ))


# ── Ledger Proof Overlay ─────────────────────────────────────────

# 80x24 inner box is 34x9 (same frame as Wallet Info). Verdict, packed
# resource ticks, and Esc sit above wrapping notes so they paint at 80x24.
# ui (tui_app.py / tui.tcss) must compose #ledger_proof and bind R.
PROOF_TEXT = """\
[b]Ledger Proof: {verdict}[/b]
{resources_text}
Press [b]Esc[/b] to close.
Run: {run_id}
Settled: {settled}  Pending: {pending}
Memo: {memo}
{notes_text}
"""


class ProofOverlay(Static):
    """PASS/FAIL/INCONCLUSIVE report for the loaded save (F-a6efdd6c)."""

    def update_from_proof(self, info: dict) -> None:
        verdict = str(info.get("verdict") or "INCONCLUSIVE")
        resources = info.get("resources") or []
        if resources:
            ticks = [
                f"{r.get('resource', '?')} "
                f"{'✓' if r.get('ok') else '✗'}"
                for r in resources
            ]
            packed = [
                "  ".join(ticks[i:i + 2])
                for i in range(0, len(ticks), 2)
            ]
            resources_text = "\n".join(packed)
        else:
            resources_text = str(
                info.get("summary")
                or "No live report (backpack off or extra missing)."
            )
        notes = info.get("notes") or []
        notes_text = notes[0] if notes else ""
        self.update(PROOF_TEXT.format(
            verdict=_escape_dynamic(verdict),
            resources_text=_escape_dynamic(resources_text),
            run_id=_escape_dynamic(str(info.get("run_id") or "—")),
            settled=_escape_dynamic(str(info.get("settlements", 0))),
            pending=_escape_dynamic(str(info.get("pending", 0))),
            memo=_escape_dynamic(str(info.get("memo") or "not run")),
            notes_text=_escape_dynamic(str(notes_text)),
        ))


# ── Learn More Overlay ───────────────────────────────────────────

# 80x24 inner box is 42x11 (width 60%, height 70%, padding 1 2). Esc sits
# after the FOOD mapping, not after the quote, so it paints at 80x24 and
# 120x30. Do not rely on overflow-y: auto.
LEARN_TEXT = """\
[b]What is the Ledger Backpack?[/b]
Tracks 5 core supplies as tokens on XRPL Testnet:
  FOOD (FOD) • WATR (WTR) • MEDS (MED)
  AMMO (AMO) • PART (PRT)
Press [b]Esc[/b] to close.
Settled at each town as public receipts.
Travelers can send capped parcels to your wallet.
Testnet — no real money. Just receipts.
"Receipts don't make the trail kinder.
 They just make it honest."
"""


class LearnMoreOverlay(Static):
    """Explainer for the Ledger Backpack."""

    def on_mount(self) -> None:
        self.update(LEARN_TEXT)


# ── Send Parcel Overlay ────────────────────────────────────────

# 80x24 inner box is 34x8 (width 50%, max-height 60%, padding 1 2).
# cancel / Esc sit above wrapping supplies or error copy.
SEND_PARCEL_TEXT = """\
[b]Send Parcel[/b]
Type [b]cancel[/b] to go back.
Format: [b]<address> <supply> <amount>[/b]
{supplies_text}
"""

SEND_PARCEL_SUCCESS_TEXT = """\
[b]Parcel sent![/b]
Press [b]Esc[/b] to continue.
{message}
"""

SEND_PARCEL_FAILURE_TEXT = """\
[b]Send failed[/b]
Press [b]Esc[/b] to try again.
{message}
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
