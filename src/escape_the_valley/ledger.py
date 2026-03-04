"""Trail Ledger — story-first end-of-run summary.

Pure function: takes RunState, returns formatted lines.
Not analytics — campfire retelling.
"""

from __future__ import annotations

from .models import RunState
from .physics import journey_pressure


def build_trail_ledger(state: RunState) -> list[str]:
    """Build the full trail ledger for end-of-run display."""
    lines: list[str] = []
    lines.extend(_header(state))
    lines.append("")
    lines.extend(_journey(state))
    lines.append("")
    lines.extend(_roll_call(state))
    lines.append("")
    lines.extend(_costliest_day(state))
    lines.extend(_closest_call(state))
    lines.extend(_what_saved_or_killed(state))
    lines.extend(_promise(state))
    lines.extend(_doctrine_echo(state))
    return lines


def build_xrpl_postcard(state: RunState) -> list[str]:
    """Build an XRPL-receipted end-of-run postcard.

    Same trail summary as the standard ledger, plus settlement
    receipts and wallet info for sharing.
    """
    lines = build_trail_ledger(state)

    bp = state.backpack
    if bp.settlements:
        lines.append("RECEIPTS ON LEDGER")
        lines.append("-" * 30)
        for s in bp.settlements:
            delta_str = ", ".join(
                f"{k}{'+' if v > 0 else ''}{v}"
                for k, v in sorted(s.deltas.items())
            )
            txid_short = s.txids[0][:12] + "..." if s.txids else "pending"
            lines.append(
                f"  Day {s.day} @ {s.location}: "
                f"{delta_str}  [{txid_short}]"
            )
        lines.append("")

    if bp.wallet_address:
        lines.append(
            f"Wallet: {bp.wallet_address[:4]}..."
            f"{bp.wallet_address[-4:]}"
        )
        lines.append("Network: XRPL Testnet")
        lines.append("")

    lines.append(
        "Receipts don't make the trail kinder. "
        "They just make it honest."
    )
    return lines


def _header(state: RunState) -> list[str]:
    lines = ["=" * 40, "TRAIL LEDGER", "=" * 40]
    if state.victory:
        lines.append("THE VALLEY IS BEHIND YOU.")
    else:
        cause = state.cause_of_death or "The trail"
        lines.append(f"Cause: {cause}.")
    return lines


def _journey(state: RunState) -> list[str]:
    progress = journey_pressure(state)
    pct = int(progress * 100)
    lines = [
        f"{state.day} days on the trail.",
        f"Covered {state.distance_traveled} of "
        f"{state.total_distance} miles ({pct}%).",
    ]
    return lines


def _roll_call(state: RunState) -> list[str]:
    lines = ["The party:"]
    for m in state.party.members:
        if m.is_alive():
            lines.append(
                f"  {m.name} -- {m.health}% health, {m.condition.value}"
            )
        else:
            death_day = _find_death_day(state, m.name)
            cause = m.death_cause or "unknown"
            if death_day:
                lines.append(
                    f"  {m.name} -- died of {cause.lower()}, day {death_day}"
                )
            else:
                lines.append(
                    f"  {m.name} -- died of {cause.lower()}"
                )
    return lines


def _costliest_day(state: RunState) -> list[str]:
    if not state.journal:
        return []

    worst_day = 0
    worst_cost = 0
    worst_title = ""

    for entry in state.journal:
        cost = sum(abs(v) for v in entry.deltas.values() if v < 0)
        if cost > worst_cost:
            worst_cost = cost
            worst_day = entry.day
            worst_title = entry.scene_title

    if worst_day == 0:
        return []

    return [
        f"Costliest day: Day {worst_day} — {worst_title}",
        "",
    ]


def _closest_call(state: RunState) -> list[str]:
    # Find highest-salience crisis or wound card
    crisis_cards = [
        c for c in state.memory_cards
        if c.kind in ("crisis", "wound") and c.salience >= 0.5
    ]
    if not crisis_cards:
        return []

    closest = max(crisis_cards, key=lambda c: c.salience)
    return [
        f"Closest call: {closest.title}",
        f"  {closest.text}",
        "",
    ]


def _what_saved_or_killed(state: RunState) -> list[str]:
    if not state.resource_crises_seen:
        if state.victory:
            # Check which supplies are still high
            items = state.supplies.to_dict()
            best = max(items, key=items.get) if items else None
            if best and items[best] > 0:
                return [f"Supplies that carried you: {best}", ""]
        return []

    # Resources that ran out
    crises = state.resource_crises_seen
    if len(crises) == 1:
        return [f"The {crises[0]} ran out first. That changed everything.", ""]
    return [
        "What ran dry: " + ", ".join(crises) + ".",
        "",
    ]


def _promise(state: RunState) -> list[str]:
    # Find a promise or omen card
    promise_cards = [
        c for c in state.memory_cards
        if c.kind in ("promise", "omen")
    ]
    if not promise_cards:
        return []

    # Pick most recent
    card = max(promise_cards, key=lambda c: c.day_created)
    if state.victory:
        return [f"A sign remembered: {card.title}", ""]
    return [f"An omen from the trail: {card.title}", ""]


def _doctrine_echo(state: RunState) -> list[str]:
    lines: list[str] = []

    doctrine_flavor = {
        "travel_light": (
            "You traveled light. Less to carry, more to lose."
        ),
        "careful_hands": (
            "Every nail counted. The wagon remembers careful hands."
        ),
        "no_debts": (
            "No debts owed, none collected. The party held together."
        ),
    }
    if state.doctrine and state.doctrine in doctrine_flavor:
        lines.append(doctrine_flavor[state.doctrine])

    taboo_flavor = {
        "never_night": "The caravan never traveled at night.",
        "never_river": "They never drank river water.",
        "leave_nothing": "They left nothing behind.",
    }
    if state.taboo and state.taboo in taboo_flavor:
        lines.append(taboo_flavor[state.taboo])

    if lines:
        lines.append("")
    return lines


def _find_death_day(state: RunState, name: str) -> int | None:
    """Find the day a member died from memory cards."""
    for card in state.memory_cards:
        if card.kind == "wound" and name in card.entities:
            if "death" in card.id.lower() or "died" in card.text.lower():
                return card.day_created
    return None
