"""Deterministic survival physics — consumption, health, breakdown."""

from __future__ import annotations

from .models import (
    Biome,
    Condition,
    Pace,
    RunState,
    SeededRNG,
    Trait,
)

# Pace modifiers
PACE_DISTANCE = {Pace.SLOW: 3, Pace.STEADY: 5, Pace.HARD: 8}
PACE_CONSUMPTION = {Pace.SLOW: 0.8, Pace.STEADY: 1.0, Pace.HARD: 1.3}
PACE_BREAKDOWN_BONUS = {Pace.SLOW: 0, Pace.STEADY: 1, Pace.HARD: 3}


def compute_daily_consumption(state: RunState) -> dict[str, int]:
    """Compute daily food/water consumption based on party, pace, weather."""
    alive = state.party.alive_count
    if alive == 0:
        return {"food": 0, "water": 0}

    pace_mod = PACE_CONSUMPTION[state.wagon.pace]

    # Base: 2 food, 2 water per person per day
    food = int(2 * alive * pace_mod)
    water = int(2 * alive * pace_mod)

    # Weather modifiers
    node = _current_node(state)
    if node:
        if node.biome == Biome.DESERT:
            water = int(water * 1.5)
        elif node.biome == Biome.ALPINE:
            food = int(food * 1.2)

    return {"food": -food, "water": -water}


def compute_travel_distance(state: RunState) -> int:
    """How far the party travels in one day based on pace and conditions."""
    base = PACE_DISTANCE[state.wagon.pace]

    # Wagon in bad shape slows you down
    if state.wagon.condition < 30:
        base = max(1, base - 2)
    elif state.wagon.condition < 60:
        base = max(1, base - 1)

    # Sick animals slow down
    if state.wagon.animals_health < 40:
        base = max(1, base - 2)

    return base


def check_breakdown(state: RunState, rng: SeededRNG) -> dict[str, int] | None:
    """Check if the wagon breaks down. Returns parts delta or None."""
    node = _current_node(state)
    hazard = node.hazard if node else 5

    # Base chance: hazard/100 + pace bonus
    chance = hazard / 100 + PACE_BREAKDOWN_BONUS[state.wagon.pace] / 100

    # Worse wagon = higher chance
    if state.wagon.condition < 50:
        chance += 0.05
    if state.wagon.condition < 25:
        chance += 0.10

    if rng.random() < chance:
        # Mechanic trait reduces damage
        damage = rng.randint(10, 25)
        if state.party.has_trait(Trait.MECHANIC):
            damage = max(5, damage - 8)
        return {"wagon_damage": damage}

    return None


def apply_breakdown(state: RunState, damage: int) -> dict[str, int]:
    """Apply wagon breakdown. Auto-spends parts if available."""
    state.wagon.condition = max(0, state.wagon.condition - damage)
    if state.supplies.parts > 0:
        state.supplies.parts -= 1
        repair = min(damage, 15)
        state.wagon.condition = min(100, state.wagon.condition + repair)
        return {"parts": -1}
    return {}


def check_health_effects(state: RunState, rng: SeededRNG) -> list[dict]:
    """Check for illness/injury effects on party members."""
    effects = []

    for member in state.party.members:
        if not member.is_alive():
            continue

        # Starvation
        if state.supplies.food <= 0:
            member.health = max(0, member.health - rng.randint(5, 15))
            effects.append({"member": member.name, "type": "starvation", "health_delta": -10})

        # Dehydration (worse than hunger)
        if state.supplies.water <= 0:
            member.health = max(0, member.health - rng.randint(8, 20))
            effects.append({"member": member.name, "type": "dehydration", "health_delta": -15})

        # Random sickness chance
        sick_chance = 0.03
        if state.party.morale < 30:
            sick_chance += 0.03
        if member.condition == Condition.EXHAUSTED:
            sick_chance += 0.05

        node = _current_node(state)
        if node and node.biome == Biome.SWAMP:
            sick_chance += 0.04

        if member.condition == Condition.HEALTHY and rng.random() < sick_chance:
            member.condition = Condition.SICK
            effects.append({"member": member.name, "type": "fell_sick"})

        # Sick members lose health unless treated
        if member.condition == Condition.SICK:
            member.health = max(0, member.health - rng.randint(3, 8))

            # Healer trait helps
            if state.party.has_trait(Trait.HEALER) and state.supplies.meds > 0:
                member.condition = Condition.HEALTHY
                member.health = min(100, member.health + 10)
                state.supplies.meds -= 1
                effects.append({"member": member.name, "type": "healed"})

        # Death check
        if member.health <= 0:
            effects.append({"member": member.name, "type": "died"})

    return effects


def attempt_hunt(state: RunState, rng: SeededRNG) -> dict[str, int]:
    """Attempt to hunt. Costs ammo, may yield food, may cause injury."""
    if state.supplies.ammo <= 0:
        return {}

    deltas: dict[str, int] = {"ammo": -1}

    # Base success chance
    success_chance = 0.5
    node = _current_node(state)
    if node:
        if node.biome == Biome.FOREST:
            success_chance += 0.15
        elif node.biome == Biome.PLAINS:
            success_chance += 0.10
        elif node.biome == Biome.DESERT:
            success_chance -= 0.20

    if state.party.has_trait(Trait.TRACKER):
        success_chance += 0.15
    if state.party.has_trait(Trait.SHARP_EYE):
        success_chance += 0.10

    if rng.random() < success_chance:
        food_gained = rng.randint(5, 15)
        deltas["food"] = food_gained
    else:
        # Chance of injury on failed hunt
        if rng.random() < 0.15:
            alive = [m for m in state.party.members if m.is_alive()]
            if alive:
                unlucky = rng.choice(alive)
                unlucky.health = max(0, unlucky.health - rng.randint(5, 15))
                unlucky.condition = Condition.INJURED

    return deltas


def attempt_repair(state: RunState) -> dict[str, int]:
    """Attempt wagon repair using parts."""
    if state.supplies.parts <= 0:
        return {}

    repair_amount = 20
    if state.party.has_trait(Trait.MECHANIC):
        repair_amount = 30

    state.wagon.condition = min(100, state.wagon.condition + repair_amount)
    return {"parts": -1}


def rest_day(state: RunState, rng: SeededRNG) -> list[dict]:
    """Party rests for a day. Health recovers, morale improves slightly."""
    effects = []
    for member in state.party.members:
        if not member.is_alive():
            continue

        heal = rng.randint(3, 10)
        member.health = min(100, member.health + heal)

        # Sick members might recover
        if member.condition in (Condition.SICK, Condition.INJURED) and rng.random() < 0.3:
            member.condition = Condition.HEALTHY
            effects.append({"member": member.name, "type": "recovered"})

    # Morale boost
    state.party.morale = min(100, state.party.morale + rng.randint(3, 8))
    return effects


def update_morale(state: RunState, event_mood: int = 0) -> None:
    """Update morale based on conditions. event_mood is an external modifier."""
    delta = event_mood

    # Low supplies hurt morale
    if state.supplies.food < state.party.alive_count * 3:
        delta -= 3
    if state.supplies.water < state.party.alive_count * 3:
        delta -= 4

    # Deaths are devastating
    dead_count = sum(1 for m in state.party.members if not m.is_alive())
    delta -= dead_count * 8

    # Good wagon condition is reassuring
    if state.wagon.condition > 70:
        delta += 1

    # Hard pace is tiring
    if state.wagon.pace == Pace.HARD:
        delta -= 2

    state.party.morale = max(0, min(100, state.party.morale + delta))

    # Exhaustion from very low morale
    if state.party.morale < 15:
        for m in state.party.members:
            if m.is_alive() and m.condition == Condition.HEALTHY:
                m.condition = Condition.EXHAUSTED


def check_game_over(state: RunState) -> str | None:
    """Check if the game is over. Returns cause or None."""
    if state.party.alive_count == 0:
        return "All party members have perished."

    if state.wagon.condition <= 0 and state.supplies.parts <= 0:
        return "Wagon destroyed beyond repair with no parts remaining."

    # Check if reached final node
    if state.map_nodes:
        final_node = state.map_nodes[-1].node_id
        if state.location_id == final_node and state.distance_remaining <= 0:
            return "VICTORY"

    return None


def _current_node(state: RunState) -> object | None:
    """Get the current map node."""
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
