"""Skilled Heuristic Bot — priority-based survival strategy.

Layered decision-making:
A. Don't die this turn (escape valves at critical thresholds)
B. Prevent spirals (maintenance windows, health recovery)
C. Play smart (hunt timing, endgame push)
D. Smart event/route choices (style-aware, terrain-scored)

Key insight: non-travel turns are expensive. Every turn NOT traveling
means more consumption without progress. Minimize idle turns.
"""

from __future__ import annotations

from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.models import Condition, Pace
from escape_the_valley.physics import (
    can_abandon_cargo,
    can_desperate_repair,
    can_hard_ration,
    journey_pressure,
)

# ── Intent shortcuts ────────────────────────────────────────────────

TRAVEL = PlayerIntent(IntentAction.TRAVEL)
REST = PlayerIntent(IntentAction.REST)
HUNT = PlayerIntent(IntentAction.HUNT)
REPAIR = PlayerIntent(IntentAction.REPAIR)
ABANDON_CARGO = PlayerIntent(IntentAction.ABANDON_CARGO)
DESPERATE_REPAIR = PlayerIntent(IntentAction.DESPERATE_REPAIR)
HARD_RATION = PlayerIntent(IntentAction.HARD_RATION)
CHOOSE_A = PlayerIntent(IntentAction.CHOOSE, choice_id="A")

# Step counter for pace management
_step_counter = 0


def choose_intent(state, engine) -> PlayerIntent:
    """Main entry point — dispatch by game phase."""
    if engine.phase == GamePhase.EVENT:
        return _pick_event_choice(state, engine)
    if engine.phase == GamePhase.ROUTE:
        return _pick_route(state, engine)
    # CAMP phase
    pace_intent = _should_change_pace(state)
    if pace_intent:
        return pace_intent
    return _pick_camp_action(state)


# ── Camp actions: priority ladder ───────────────────────────────────

def _pick_camp_action(state) -> PlayerIntent:
    """Priority-based camp action selection.

    Rule: travel is king. Every non-travel turn costs consumption
    without progress. Only deviate when the alternative prevents death.
    """
    alive = state.party.alive_count
    if alive == 0:
        return TRAVEL

    # ── A. Don't die this turn ──

    # Critical food shortage — ration before starvation
    if can_hard_ration(state) and state.supplies.food < alive * 2:
        return HARD_RATION

    # Wagon about to die, no parts — desperate measures
    if can_desperate_repair(state) and state.wagon.condition < 15:
        return DESPERATE_REPAIR

    # Wagon critical, dump cargo to save it
    if can_abandon_cargo(state) and state.wagon.condition < 20:
        return ABANDON_CARGO

    # ── B. Prevent spirals (maintenance + health) ──

    # Complete maintenance window if we started one
    if state.wagon.condition < 70 and state.supplies.parts > 0:
        if state.last_action == "REST":
            return REPAIR  # complete the window

    # Start maintenance proactively — don't wait until critical
    # Skip if we already have active maintenance resistance
    if (
        state.wagon.condition < 65
        and state.supplies.parts > 0
        and state.maintained_turns_remaining <= 0
        and state.last_action != "REPAIR"
    ):
        return REST  # start maintenance window

    # Quick repair without full window when wagon is getting rough
    if state.wagon.condition < 40 and state.supplies.parts > 0:
        return REPAIR

    # Rest when several party members are sick/injured
    sick_or_hurt = sum(
        1 for m in state.party.members
        if m.is_alive() and m.condition in (Condition.SICK, Condition.INJURED)
    )
    if sick_or_hurt >= 2:
        return REST

    # Party health critically low
    if state.party.avg_health < 35:
        return REST

    # ── C. Play smart ──

    # Hunt only when food is genuinely low — each hunt is a turn not traveling
    if state.supplies.food < alive * 3 and state.supplies.ammo > 0:
        return HUNT

    # ── Default: travel ──
    return TRAVEL


# ── Pace management ─────────────────────────────────────────────────

def _should_change_pace(state) -> PlayerIntent | None:
    """Adjust pace based on conditions. Conservative — no HARD pace."""
    global _step_counter  # noqa: PLW0603
    _step_counter += 1

    # Only check every 8 steps to avoid flip-flopping
    if _step_counter % 8 != 0:
        return None

    pressure = journey_pressure(state)
    current = state.wagon.pace

    # Late game or fragile wagon → slow down to reduce breakdowns
    if state.wagon.condition < 35 or (pressure > 0.7 and state.wagon.condition < 50):
        if current != Pace.SLOW:
            return PlayerIntent(IntentAction.CHANGE_PACE, pace="slow")
        return None

    # Default: steady pace (best distance/risk ratio)
    if current != Pace.STEADY:
        return PlayerIntent(IntentAction.CHANGE_PACE, pace="steady")
    return None


# ── Event choice strategy ───────────────────────────────────────────

def _pick_event_choice(state, engine) -> PlayerIntent:
    """Choose event option based on resource pressure and choice style."""
    event = engine._pending_event
    choices = engine._pending_event_choices

    if not choices:
        return CHOOSE_A

    alive = state.party.alive_count or 1

    # Determine pressure level
    under_pressure = (
        state.supplies.food < alive * 4
        or state.supplies.water < alive * 4
        or state.wagon.condition < 30
    )
    strong = (
        state.supplies.food > alive * 8
        and state.supplies.water > alive * 8
        and state.wagon.condition > 60
    )

    # Try to read style from the event skeleton's fallback choices
    if event and event.fallback_choices:
        preferred = "CAUTIOUS" if under_pressure else ("BOLD" if strong else "NEUTRAL")
        fallback = "CAUTIOUS"

        best_idx = 0
        best_score = -1

        for i, fc in enumerate(event.fallback_choices):
            if i >= len(choices):
                break
            score = 0
            if fc.style == preferred:
                score = 3
            elif fc.style == "NEUTRAL":
                score = 2
            elif fc.style == fallback:
                score = 1
            if score > best_score:
                best_score = score
                best_idx = i

        choice_ids = ["A", "B", "C", "D"]
        if best_idx < len(choice_ids):
            return PlayerIntent(IntentAction.CHOOSE, choice_id=choice_ids[best_idx])

    return CHOOSE_A


# ── Route choice strategy ───────────────────────────────────────────

def _pick_route(state, engine) -> PlayerIntent:
    """Score route options and pick the best one."""
    routes = engine._pending_routes
    if not routes:
        return CHOOSE_A

    best_idx = 0
    best_score = float("-inf")

    for i, route in enumerate(routes):
        score = _score_route(state, route)
        if score > best_score:
            best_score = score
            best_idx = i

    choice_ids = ["A", "B", "C", "D"]
    if best_idx < len(choice_ids):
        return PlayerIntent(IntentAction.CHOOSE, choice_id=choice_ids[best_idx])
    return CHOOSE_A


def _score_route(state, route_option) -> float:
    """Score a route option based on current state."""
    node = _find_node_by_id(state, route_option.node_id)
    if not node:
        return 0.0

    score = 0.0
    alive = state.party.alive_count or 1

    # Prefer water sources when water is trending low
    if node.water_available and state.supplies.water < alive * 6:
        score += 3.0

    # Avoid high hazard when wagon is fragile
    if node.hazard > 5 and state.wagon.condition < 40:
        score -= 2.0

    # Towns are valuable (trade chance, water refill)
    if node.is_town:
        score += 2.0

    # Shorter distance when water is low (less consumption en route)
    if state.supplies.water < alive * 4:
        score -= route_option.distance * 0.1

    # Cache bonus
    if node.cache_supplies:
        score += 1.5

    # Prefer lower hazard in general
    score -= node.hazard * 0.15

    return score


# ── Helpers ──────────────────────────────────────────────────────────

def _current_node(state):
    """Get the current map node."""
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None


def _find_node_by_id(state, node_id: str):
    """Find a map node by ID."""
    for node in state.map_nodes:
        if node.node_id == node_id:
            return node
    return None


def reset_step_counter() -> None:
    """Reset the step counter between runs."""
    global _step_counter  # noqa: PLW0603
    _step_counter = 0
