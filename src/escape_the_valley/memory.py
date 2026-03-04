"""Memory system — store, pressures, themes, retrieval, and GM brief builder.

Pure functions operating on RunState. No side effects, no I/O.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .models import Condition, MemoryCard, RunState
from .physics import journey_pressure

# ── Constants ────────────────────────────────────────────────────────

MEMORY_BUDGET = 50  # Max cards before eviction

# Tag → theme mapping (raw event tags → higher-level narrative labels)
TAG_TO_THEME: dict[str, str] = {
    "river": "river",
    "crossing": "river",
    "ford": "river",
    "folklore": "uncanny",
    "weird": "uncanny",
    "omen": "uncanny",
    "ghost": "uncanny",
    "food": "hunger",
    "hunger": "hunger",
    "starvation": "hunger",
    "hunt": "hunger",
    "trade": "commerce",
    "merchant": "commerce",
    "town": "commerce",
    "bandit": "danger",
    "attack": "danger",
    "ambush": "danger",
    "sickness": "plague",
    "disease": "plague",
    "fever": "plague",
    "storm": "weather",
    "rain": "weather",
    "snow": "weather",
    "cold": "weather",
    "wagon": "breakdown",
    "breakdown": "breakdown",
    "repair": "breakdown",
    "death": "loss",
    "grave": "loss",
    "morale": "spirit",
    "campfire": "spirit",
}


# ── Store operations ─────────────────────────────────────────────────

def add_card(state: RunState, card: MemoryCard) -> None:
    """Append a card, enforcing budget by evicting lowest-salience."""
    # Deduplicate: skip if card with same id exists
    for existing in state.memory_cards:
        if existing.id == card.id:
            return

    state.memory_cards.append(card)

    if len(state.memory_cards) > MEMORY_BUDGET:
        drop_lowest(state, len(state.memory_cards) - MEMORY_BUDGET)


def drop_lowest(state: RunState, count: int) -> None:
    """Evict the N lowest-salience cards (prefer expired cooldowns first)."""
    if count <= 0 or not state.memory_cards:
        return

    # Sort candidates: expired cooldown first, then lowest salience
    scored = sorted(
        state.memory_cards,
        key=lambda c: (
            0 if c.cooldown_until <= state.day else 1,  # expired first
            c.salience,  # lowest salience first
        ),
    )

    to_remove = set()
    for card in scored:
        if len(to_remove) >= count:
            break
        to_remove.add(card.id)

    state.memory_cards = [
        c for c in state.memory_cards if c.id not in to_remove
    ]


# ── Deterministic inference ──────────────────────────────────────────

def compute_pressures(state: RunState) -> list[str]:
    """Scan state for pressure tags, sorted by severity (worst first)."""
    pressures: list[tuple[int, str]] = []
    alive = state.party.alive_count

    if alive == 0:
        return ["PARTY_DEAD"]

    # Food pressure
    if state.supplies.food <= 0:
        pressures.append((10, "FOOD_GONE"))
    elif state.supplies.food <= alive * 2:
        pressures.append((8, "FOOD_CRITICAL"))
    elif state.supplies.food <= alive * 4:
        pressures.append((4, "FOOD_LOW"))

    # Water pressure
    if state.supplies.water <= 0:
        pressures.append((10, "WATER_GONE"))
    elif state.supplies.water <= alive * 2:
        pressures.append((9, "WATER_CRITICAL"))
    elif state.supplies.water <= alive * 4:
        pressures.append((5, "WATER_LOW"))

    # Wagon
    if state.wagon.condition <= 0:
        pressures.append((10, "WAGON_DESTROYED"))
    elif state.wagon.condition <= 15:
        pressures.append((7, "WAGON_FRAGILE"))
    elif state.wagon.condition < 40:
        pressures.append((4, "WAGON_WORN"))

    # Parts
    if state.supplies.parts <= 0 and state.wagon.condition < 50:
        pressures.append((6, "NO_PARTS"))

    # Party health
    sick_count = sum(
        1 for m in state.party.members
        if m.is_alive() and m.condition in (
            Condition.SICK, Condition.INJURED,
        )
    )
    if sick_count >= 2:
        pressures.append((6, "PARTY_SICK"))
    elif sick_count == 1:
        pressures.append((3, "PARTY_HURT"))

    low_health = sum(
        1 for m in state.party.members
        if m.is_alive() and m.health < 30
    )
    if low_health >= 2:
        pressures.append((7, "PARTY_DYING"))

    # Morale
    if state.party.morale < 15:
        pressures.append((6, "MORALE_BROKEN"))
    elif state.party.morale < 30:
        pressures.append((3, "MORALE_LOW"))

    # Journey
    pressure = journey_pressure(state)
    if pressure > 0.8:
        pressures.append((2, "LATE_JOURNEY"))
    elif pressure > 0.5:
        pressures.append((1, "MID_JOURNEY"))

    # Sort by severity descending, return labels
    pressures.sort(key=lambda x: -x[0])
    return [label for _, label in pressures]


def compute_themes(state: RunState) -> list[str]:
    """Derive 1-3 theme tags from recent journal entries + event tags."""
    raw_tags: list[str] = []

    # Last 5 journal entries
    for entry in state.journal[-5:]:
        raw_tags.extend(entry.tags)

    # Recent event tags from variety guard
    raw_tags.extend(state.recent_event_tags)

    # Map to themes
    theme_counts: dict[str, int] = {}
    for tag in raw_tags:
        theme = TAG_TO_THEME.get(tag.lower())
        if theme:
            theme_counts[theme] = theme_counts.get(theme, 0) + 1

    # Sort by frequency, return top 3
    sorted_themes = sorted(
        theme_counts.items(), key=lambda x: -x[1],
    )
    return [theme for theme, _ in sorted_themes[:3]]


# ── Poor Man's RAG retrieval ─────────────────────────────────────────

def retrieve_memories(
    state: RunState,
    max_results: int = 6,
) -> list[MemoryCard]:
    """Retrieve top-scoring memories using tag overlap + recency + salience."""
    if not state.memory_cards:
        return []

    # Current context tags for scoring
    context_tags: set[str] = set()
    for entry in state.journal[-3:]:
        context_tags.update(t.lower() for t in entry.tags)
    context_tags.update(t.lower() for t in state.recent_event_tags)

    # Add pressure-derived tags
    pressures = compute_pressures(state)
    for p in pressures:
        context_tags.add(p.lower())

    scored: list[tuple[float, MemoryCard]] = []

    for card in state.memory_cards:
        # Skip cards on cooldown
        if card.cooldown_until > state.day:
            continue

        score = 0.0

        # Tag overlap: 0.2 per matching tag
        card_tags = {t.lower() for t in card.tags}
        overlap = len(card_tags & context_tags)
        score += overlap * 0.2

        # Recency bonus
        days_ago = state.day - card.day_last_seen
        if days_ago <= 3:
            score += 0.3
        elif days_ago <= 5:
            score += 0.15

        # Salience weight
        score *= card.salience

        # Small bonus for engine cards (more reliable)
        if card.source == "engine":
            score += 0.05

        scored.append((score, card))

    # Sort by score descending
    scored.sort(key=lambda x: -x[0])

    # Take top results, update last_seen + cooldown
    results: list[MemoryCard] = []
    for _, card in scored[:max_results]:
        card.day_last_seen = state.day
        card.cooldown_until = state.day + 3
        results.append(card)

    return results


# ── GM Brief ─────────────────────────────────────────────────────────

@dataclass
class GMBrief:
    """Everything the GM needs to narrate intelligently."""

    situation: str
    pressures: list[str]
    themes: list[str]
    callbacks: list[MemoryCard] = field(default_factory=list)
    tone_profile: str = "fireside"
    weirdness_allowance: str = "none"  # none | hint | strong


def build_gm_brief(state: RunState) -> GMBrief:
    """Pure function: compute the full GM brief from state."""
    pressures = compute_pressures(state)
    themes = compute_themes(state)
    callbacks = retrieve_memories(state, max_results=6)

    situation = _build_situation(state, pressures)
    weirdness = _compute_weirdness(state)

    return GMBrief(
        situation=situation,
        pressures=pressures[:3],
        themes=themes,
        callbacks=callbacks,
        tone_profile=state.gm_profile.value,
        weirdness_allowance=weirdness,
    )


def _build_situation(state: RunState, pressures: list[str]) -> str:
    """Build a 2-4 sentence situation summary from state."""
    parts: list[str] = []

    # Time and place
    node = None
    for n in state.map_nodes:
        if n.node_id == state.location_id:
            node = n
            break

    biome = node.biome.value if node else "unknown terrain"
    parts.append(
        f"Day {state.day}, {state.time_of_day.value}. "
        f"{biome.capitalize()} terrain."
    )

    # Journey progress
    progress = journey_pressure(state)
    if progress < 0.25:
        parts.append("Early in the journey.")
    elif progress < 0.5:
        parts.append("Nearing the halfway mark.")
    elif progress < 0.75:
        parts.append("Well past halfway.")
    else:
        parts.append("The end of the trail is near.")

    # Party status
    alive = state.party.alive_count
    total = len(state.party.members)
    if alive < total:
        parts.append(f"{alive} of {total} party members alive.")

    # Top pressure as narrative color
    if pressures:
        top = pressures[0]
        pressure_flavor = {
            "FOOD_GONE": "They have no food.",
            "FOOD_CRITICAL": "Rations are nearly gone.",
            "WATER_GONE": "No water remains.",
            "WATER_CRITICAL": "Water is dangerously low.",
            "WAGON_DESTROYED": "The wagon is destroyed.",
            "WAGON_FRAGILE": "The wagon may not survive another hit.",
            "PARTY_SICK": "Multiple members are sick or injured.",
            "PARTY_DYING": "Several members are in critical condition.",
            "MORALE_BROKEN": "The party's spirit is shattered.",
        }
        if top in pressure_flavor:
            parts.append(pressure_flavor[top])

    # Doctrine/taboo flavor
    doctrine_flavor = {
        "travel_light": "The caravan travels light — less to eat, less to carry.",
        "careful_hands": "Every repair is deliberate. The caravan moves carefully.",
        "no_debts": "They carry no debts. Morale holds, but the wagon is lean.",
    }
    taboo_flavor = {
        "never_night": "They will not travel at night.",
        "never_river": "They refuse to drink river water.",
        "leave_nothing": "They leave nothing behind.",
    }
    if state.doctrine and state.doctrine in doctrine_flavor:
        parts.append(doctrine_flavor[state.doctrine])
    if state.taboo and state.taboo in taboo_flavor:
        parts.append(taboo_flavor[state.taboo])

    return " ".join(parts)


def _compute_weirdness(state: RunState) -> str:
    """Determine how much weird/folklore the GM may inject."""
    if state.uncanny_tokens <= 0:
        return "none"
    if state.weirdness_level >= 3:
        return "strong"
    if state.weirdness_level >= 1:
        return "hint"
    return "none"


def format_brief_for_prompt(brief: GMBrief) -> str:
    """Format GMBrief as text for injection into GM prompt."""
    lines: list[str] = []

    lines.append("GM BRIEF:")
    lines.append(f"Situation: {brief.situation}")

    if brief.pressures:
        lines.append(f"Pressures: {', '.join(brief.pressures)}")

    if brief.themes:
        lines.append(f"Themes: {', '.join(brief.themes)}")

    if brief.weirdness_allowance != "none":
        lines.append(
            f"Weirdness allowance: {brief.weirdness_allowance}"
        )

    if brief.callbacks:
        lines.append("")
        lines.append(
            "Relevant memories you may reference (optional):"
        )
        for card in brief.callbacks:
            lines.append(f"  - [{card.kind}] {card.title}: {card.text}")

    return "\n".join(lines)
