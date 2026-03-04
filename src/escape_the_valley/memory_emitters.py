"""Memory emitters — engine-created cards + GM card validation.

Engine emitters are deterministic: same state → same cards.
GM cards are validated and salience-capped.
"""

from __future__ import annotations

from .events import EventSkeleton
from .memory import add_card
from .models import MemoryCard, RunState

# GM may only create these kinds
_GM_ALLOWED_KINDS = {"npc", "omen", "place", "rumor", "promise"}

_TITLE_MAX = 40
_TEXT_MAX = 300
_GM_MAX_PER_PROPOSAL = 2


# ── Engine emitters (deterministic, salience=0.7) ────────────────────


def emit_health_cards(
    state: RunState,
    effects: list[dict],
) -> None:
    """Emit memory cards from health check effects (sickness, death, etc.)."""
    for eff in effects:
        member = eff.get("member", "unknown")
        eff_type = eff.get("type", "")

        if eff_type == "died":
            card = MemoryCard(
                id=f"eng_death_{member.lower()}_d{state.day}",
                kind="wound",
                title=f"{member}'s Death",
                text=f"{member} perished on day {state.day}.",
                tags=["death", "loss"],
                day_created=state.day,
                day_last_seen=state.day,
                entities=[member],
                salience=0.7,
                source="engine",
            )
            add_card(state, card)

        elif eff_type == "fell_sick":
            card = MemoryCard(
                id=f"eng_sick_{member.lower()}_d{state.day}",
                kind="wound",
                title=f"{member}'s Illness",
                text=f"{member} fell sick on day {state.day}.",
                tags=["sickness", "disease"],
                day_created=state.day,
                day_last_seen=state.day,
                entities=[member],
                salience=0.7,
                source="engine",
            )
            add_card(state, card)

        elif eff_type == "healed":
            card = MemoryCard(
                id=f"eng_healed_{member.lower()}_d{state.day}",
                kind="event_callback",
                title=f"{member} Recovered",
                text=f"{member} recovered from their ailment on day {state.day}.",
                tags=["recovery"],
                day_created=state.day,
                day_last_seen=state.day,
                entities=[member],
                salience=0.7,
                source="engine",
            )
            add_card(state, card)


def emit_resource_crisis_card(
    state: RunState,
    resource: str,
) -> None:
    """Emit a crisis card the first time a resource hits 0."""
    if resource in state.resource_crises_seen:
        return

    state.resource_crises_seen.append(resource)

    label_map = {
        "food": "Starvation Begins",
        "water": "The Water Ran Out",
        "ammo": "Out of Ammunition",
        "meds": "No Medicine Left",
        "parts": "Last Part Used",
    }
    title = label_map.get(resource, f"No {resource.title()} Left")

    card = MemoryCard(
        id=f"eng_crisis_{resource}_d{state.day}",
        kind="crisis",
        title=title,
        text=f"The party ran out of {resource} on day {state.day}.",
        tags=[resource, "crisis"],
        day_created=state.day,
        day_last_seen=state.day,
        salience=0.7,
        source="engine",
    )
    add_card(state, card)


def emit_wagon_card(
    state: RunState,
    damage: int,
    had_parts: bool,
) -> None:
    """Emit a card for significant wagon events."""
    # Only emit for notable events: high damage or last-part repair
    if damage < 15 and had_parts:
        return

    if not had_parts:
        card = MemoryCard(
            id=f"eng_wagon_noparts_d{state.day}",
            kind="event_callback",
            title="Breakdown Without Parts",
            text=(
                f"The wagon took {damage} damage on day {state.day} "
                f"with no parts to repair it."
            ),
            tags=["wagon", "breakdown", "crisis"],
            day_created=state.day,
            day_last_seen=state.day,
            salience=0.7,
            source="engine",
        )
    else:
        card = MemoryCard(
            id=f"eng_wagon_hit_d{state.day}",
            kind="event_callback",
            title="Heavy Wagon Damage",
            text=f"The wagon took {damage} damage on day {state.day}.",
            tags=["wagon", "breakdown"],
            day_created=state.day,
            day_last_seen=state.day,
            salience=0.7,
            source="engine",
        )
    add_card(state, card)


def emit_arrival_card(state: RunState, node) -> None:
    """Emit a card when arriving at a notable location."""
    if not node.is_town:
        return

    card = MemoryCard(
        id=f"eng_arrival_{node.node_id}_d{state.day}",
        kind="landmark",
        title=f"Arrived at {node.name}"[:_TITLE_MAX],
        text=(
            f"The party reached {node.name}, "
            f"a settlement in {node.biome.value} terrain, "
            f"on day {state.day}."
        ),
        tags=["town", "landmark", node.biome.value],
        day_created=state.day,
        day_last_seen=state.day,
        entities=[node.name],
        salience=0.7,
        source="engine",
    )
    add_card(state, card)


def emit_event_card(
    state: RunState,
    event: EventSkeleton,
) -> None:
    """Emit a card for folklore/uncanny events."""
    if "folklore" not in event.tags and event.category != "folklore":
        return

    card = MemoryCard(
        id=f"eng_event_{event.event_id}_d{state.day}",
        kind="omen",
        title=event.title[:_TITLE_MAX],
        text=(
            event.fallback_narration[:_TEXT_MAX]
            if event.fallback_narration
            else f"A strange event on day {state.day}."
        ),
        tags=list(event.tags),
        day_created=state.day,
        day_last_seen=state.day,
        salience=0.7,
        source="engine",
    )
    add_card(state, card)


# ── Escape valve emitter ─────────────────────────────────────────────


def emit_escape_valve_card(
    state: RunState,
    valve_type: str,
    detail: str,
) -> None:
    """Emit a crisis card when an escape valve is used."""
    title_map = {
        "abandon_cargo": "Cargo Abandoned",
        "desperate_repair": "Desperate Repair Attempt",
        "hard_ration": "Hard Rationing Imposed",
    }
    card = MemoryCard(
        id=f"eng_valve_{valve_type}_d{state.day}",
        kind="crisis",
        title=title_map.get(valve_type, valve_type.replace("_", " ").title()),
        text=f"Day {state.day}: {detail}"[:_TEXT_MAX],
        tags=["escape_valve", valve_type],
        day_created=state.day,
        day_last_seen=state.day,
        salience=0.7,
        source="engine",
    )
    add_card(state, card)


# ── Resource crisis detector ─────────────────────────────────────────


def check_resource_crises(state: RunState) -> None:
    """Check all resources, emit crisis cards for any newly at zero."""
    critical = ["food", "water", "ammo", "meds", "parts"]
    for key in critical:
        if state.supplies.get(key) <= 0:
            emit_resource_crisis_card(state, key)


# ── GM card validation ───────────────────────────────────────────────


def validate_gm_cards(
    state: RunState,
    proposed: list[dict],
) -> list[MemoryCard]:
    """Validate GM-proposed memory cards. Returns accepted cards.

    Rules:
    - Max 2 per proposal
    - Allowed kinds: npc, omen, place, rumor, promise
    - Title ≤ 40 chars, text ≤ 300 chars
    - Must not reference supply quantities
    - Salience forced to 0.5
    """
    accepted: list[MemoryCard] = []

    for proposal in proposed[:_GM_MAX_PER_PROPOSAL]:
        kind = proposal.get("kind", "")
        if kind not in _GM_ALLOWED_KINDS:
            continue

        title = str(proposal.get("title", ""))[:_TITLE_MAX]
        text = str(proposal.get("text", ""))[:_TEXT_MAX]

        if not title or not text:
            continue

        # Reject texts that reference supply numbers
        if _mentions_supply_numbers(text):
            continue

        card_id = proposal.get(
            "id",
            f"gm_{kind}_{state.day}_{len(accepted)}",
        )

        card = MemoryCard(
            id=card_id,
            kind=kind,
            title=title,
            text=text,
            tags=proposal.get("tags", [])[:5],
            day_created=state.day,
            day_last_seen=state.day,
            entities=proposal.get("entities", [])[:5],
            salience=0.5,  # Forced for GM cards
            cooldown_until=0,
            source="gm",
        )
        accepted.append(card)

    return accepted


def _mentions_supply_numbers(text: str) -> bool:
    """Check if text references specific supply quantities."""
    import re

    # Match patterns like "15 food", "food: 30", "20 water"
    supply_words = [
        "food", "water", "ammo", "meds", "parts",
        "firewood", "salt", "rope", "tools", "lantern_oil",
        "cloth", "boots",
    ]
    for word in supply_words:
        if re.search(rf"\b\d+\s+{word}\b", text, re.IGNORECASE):
            return True
        if re.search(rf"\b{word}\s*:\s*\d+", text, re.IGNORECASE):
            return True
    return False
