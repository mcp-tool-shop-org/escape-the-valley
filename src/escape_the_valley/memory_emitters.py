"""Memory emitters — engine-created cards + GM card validation.

Engine emitters are deterministic: same state → same cards.
GM cards are validated and salience-capped.
"""

from __future__ import annotations

import hashlib

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
            cause = str(eff.get("cause") or "unknown")
            card = MemoryCard(
                id=f"eng_death_{member.lower()}_d{state.day}",
                kind="wound",
                title=f"{member}'s Death",
                text=f"{member} perished on day {state.day} ({cause}).",
                tags=["death", "loss", cause.lower().replace(" ", "_")],
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
    - Card id is always engine-computed (F-778637b3): the GM does not own
      engine keys, so a model-supplied "id" — a field neither SCENE_SCHEMA
      nor OUTCOME_SCHEMA documents or requests — is never honored. Engine
      emitters mint deterministic ids (e.g. ``eng_crisis_<resource>_d<day>``)
      and ``add_card`` dedupes purely by id equality; honoring a model id
      would let untrusted output silently suppress a legitimate
      engine-authored card that happens to collide with it later.
    - The id is a content digest, unique per CARD rather than per CALL
      (F-6f03c718) — see the inline comment above the digest computation
      below for why a shared counter or a new RunState field would not do.

    Defense in depth (F-9b0797f9): ``gm.py``'s ``SceneResponse.from_dict`` /
    ``OutcomeResponse.from_dict`` already normalize a top-level JSON `null`
    for ``memory_proposals`` to ``[]``, but this function must not assume
    every caller does the same, nor that every list element is a
    well-formed dict, nor that a proposal's ``tags``/``entities`` are lists
    rather than an explicit `null`, nor that list elements are strings —
    so shape is re-validated here rather than trusted from the input.
    Non-empty ``str`` elements are kept; ``None``, ints, dicts, and empty
    strings are dropped before they hit a MemoryCard (F-bcf0063c).
    """
    accepted: list[MemoryCard] = []

    if not isinstance(proposed, list):
        return accepted

    for proposal in proposed[:_GM_MAX_PER_PROPOSAL]:
        if not isinstance(proposal, dict):
            continue

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

        # Never honor a model-supplied "id" — see docstring (F-778637b3).
        #
        # F-6f03c718 — the prior fix (f"gm_{kind}_{state.day}_{len(accepted)}")
        # was only unique WITHIN one validate_gm_cards() call: `accepted` is a
        # fresh local list every call, so `len(accepted)` restarts at 0 on the
        # next one. step_engine.py calls this function at least twice per turn
        # (scene, then outcome), and a calendar day can span multiple turns,
        # so two ordinary — not adversarial — calls proposing the same `kind`
        # on the same day minted identical ids, and add_card's id-equality
        # dedup silently discarded the second, entirely legitimate card.
        #
        # The id must be unique per CARD, not per CALL, and it has to get
        # there without a shared counter: a monotonic counter would need a
        # new field on RunState, which lives in models.py — out of this
        # domain's ownership — and without relying on call order/timing in
        # step_engine.py, which is also out of this domain's ownership and
        # under concurrent revision this same wave. A stable digest of the
        # card's own content (kind, day, title, text) needs neither: it is
        # unique per card because it is *computed from* the card, stays
        # engine-computed and deterministic (no randomness, no wall-clock,
        # so seeded/--gm off runs stay reproducible), and two proposals can
        # only collide here if they share kind, day, title, AND text
        # byte-for-byte — i.e. they are the same card. This does not reopen
        # F-778637b3: the model still never supplies (or picks) the id
        # itself, and it has no practical way to aim for a specific target
        # id — that would mean inverting SHA-256, and even then a "gm_"
        # prefix can never enter the disjoint "eng_" namespace regardless of
        # the digest. add_card (memory.py) now logs a warning on any
        # same-id drop, so even a byte-identical duplicate stays visible
        # instead of silently vanishing.
        digest_src = "\x1f".join((kind, str(state.day), title, text))
        digest = hashlib.sha256(digest_src.encode("utf-8")).hexdigest()[:12]
        card_id = f"gm_{kind}_{state.day}_{digest}"

        tags = proposal.get("tags") or []
        entities = proposal.get("entities") or []

        card = MemoryCard(
            id=card_id,
            kind=kind,
            title=title,
            text=text,
            tags=_nonempty_str_items(tags),
            day_created=state.day,
            day_last_seen=state.day,
            entities=_nonempty_str_items(entities),
            salience=0.5,  # Forced for GM cards
            cooldown_until=0,
            source="gm",
        )
        accepted.append(card)

    return accepted


def _nonempty_str_items(value: object, *, limit: int = 5) -> list[str]:
    """Keep only non-empty str list elements, capped at *limit* (F-bcf0063c).

    A list of tags is not a list of str: a small local model can emit
    optional array emptiness as a null element (``["river", null]``).
    Field-level ``None`` / non-list are already rejected by the caller;
    this is the list-element sibling. Drop ``None``, ints, dicts, and
    empty strings so they never reach a MemoryCard.
    """
    if not isinstance(value, list):
        return []
    return [item for item in value if isinstance(item, str) and item][:limit]


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
