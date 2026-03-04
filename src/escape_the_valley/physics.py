"""Deterministic survival physics — consumption, health, breakdown."""

from __future__ import annotations

from .models import (
    DOCTRINE_MODIFIERS,
    Biome,
    Condition,
    Pace,
    RunState,
    SeededRNG,
    TimeOfDay,
    Trait,
)

# Pace modifiers
PACE_DISTANCE = {Pace.SLOW: 3, Pace.STEADY: 5, Pace.HARD: 8}
PACE_CONSUMPTION = {Pace.SLOW: 0.8, Pace.STEADY: 1.0, Pace.HARD: 1.3}
PACE_BREAKDOWN_BONUS = {Pace.SLOW: 0, Pace.STEADY: 1, Pace.HARD: 3}


def journey_pressure(state: RunState) -> float:
    """How far through the journey: 0.0 (start) to 1.0 (destination).

    Used by scarcity curve — consumption and event severity scale with this.
    """
    if state.total_distance <= 0:
        return 0.0
    return min(1.0, state.distance_traveled / state.total_distance)


def compute_daily_consumption(
    state: RunState, *, is_travel: bool = False,
) -> dict[str, int]:
    """Compute daily food/water/firewood/lantern_oil consumption."""
    alive = state.party.alive_count
    if alive == 0:
        return {"food": 0, "water": 0}

    pace_mod = PACE_CONSUMPTION[state.wagon.pace]

    # Scarcity curve: late-game consumption scales up to 1.12×
    pressure = journey_pressure(state)
    consumption_scale = 1.0 + 0.12 * pressure

    # Doctrine modifier
    doc_mods = DOCTRINE_MODIFIERS.get(state.doctrine, {})
    doc_mult = doc_mods.get("consumption_mult", 1.0)

    # Base: 2 food, 2 water per person per day.
    # Each step is one quarter-day, so divide by 4.
    # Use max(1, ...) to ensure at least 1 consumed per step.
    raw_food = 2 * alive * pace_mod * consumption_scale * doc_mult / 4
    raw_water = 2 * alive * pace_mod * consumption_scale * doc_mult / 4
    food = max(1, int(raw_food))
    water = max(1, int(raw_water))

    # Hard ration halves food/water
    if state.rationing_steps > 0:
        food = max(1, food // 2)
        water = max(1, water // 2)

    # Weather modifiers
    node = _current_node(state)
    if node:
        if node.biome == Biome.DESERT:
            water = int(water * 1.5)
        elif node.biome == Biome.ALPINE:
            food = int(food * 1.2)

    deltas: dict[str, int] = {"food": -food, "water": -water}

    # Firewood: 1 per night camp (evening/night time of day)
    if state.time_of_day in (TimeOfDay.EVENING, TimeOfDay.NIGHT):
        fw = 1
        # Cold biomes use more
        if node and node.biome == Biome.ALPINE:
            fw = 2
        deltas["firewood"] = -fw

    # Lantern oil: 1 per night travel
    if is_travel and state.time_of_day in (
        TimeOfDay.EVENING, TimeOfDay.NIGHT,
    ):
        deltas["lantern_oil"] = -1

    return deltas


def compute_travel_distance(state: RunState) -> int:
    """How far the party travels in one day based on pace and conditions."""
    base = PACE_DISTANCE[state.wagon.pace]

    # Doctrine modifier
    doc_mods = DOCTRINE_MODIFIERS.get(state.doctrine, {})
    penalty = int(doc_mods.get("distance_penalty", 0))
    base = max(1, base - penalty)

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

    # Doctrine modifier
    doc_mods = DOCTRINE_MODIFIERS.get(state.doctrine, {})
    chance += doc_mods.get("breakdown_bonus", 0)

    # Condition-dependent breakdown curve
    cond = state.wagon.condition
    if cond > 60:
        chance *= 0.3       # well-maintained = very safe
    elif cond < 30:
        chance *= 1.8       # badly damaged = scary

    # Maintenance window: breakdown resistance
    if state.maintained_turns_remaining > 0:
        chance *= 0.3

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
            cause = _proximate_death_cause(member, state)
            member.death_cause = cause
            effects.append({
                "member": member.name,
                "type": "died",
                "cause": cause,
            })

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

    # Doctrine hunt bonus
    doc_mods = DOCTRINE_MODIFIERS.get(state.doctrine, {})
    success_chance += doc_mods.get("hunt_bonus", 0)

    if rng.random() < success_chance:
        # Biome-specific yield ranges
        if node and node.biome == Biome.FOREST:
            food_gained = rng.randint(8, 18)
        elif node and node.biome == Biome.PLAINS:
            food_gained = rng.randint(7, 16)
        else:
            food_gained = rng.randint(5, 12)

        # Big haul: 10% chance of 2× yield when morale is high
        if state.party.morale > 60 and rng.random() < 0.10:
            food_gained *= 2

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
    """Attempt wagon repair using parts. Tools improve success."""
    if state.supplies.parts <= 0:
        return {}

    repair_amount = 20
    if state.party.has_trait(Trait.MECHANIC):
        repair_amount = 30
    # Tools bonus: +10 repair if available
    if state.supplies.get("tools") > 0:
        repair_amount += 10

    # Doctrine modifier
    doc_mods = DOCTRINE_MODIFIERS.get(state.doctrine, {})
    repair_mult = doc_mods.get("repair_mult", 1.0)
    repair_amount = int(repair_amount * repair_mult)

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

    # Doctrine morale floor
    doc_mods = DOCTRINE_MODIFIERS.get(state.doctrine, {})
    morale_floor = int(doc_mods.get("morale_floor", 0))

    state.party.morale = max(morale_floor, min(100, state.party.morale + delta))

    # Exhaustion from very low morale
    if state.party.morale < 15:
        for m in state.party.members:
            if m.is_alive() and m.condition == Condition.HEALTHY:
                m.condition = Condition.EXHAUSTED


def check_spoilage(state: RunState, rng: SeededRNG) -> dict[str, int]:
    """If salt == 0 and day % 3 == 0, some food spoils."""
    if state.supplies.get("salt") > 0 or state.day % 3 != 0:
        return {}

    loss = rng.randint(2, 4)
    actual = min(loss, state.supplies.food)
    if actual > 0:
        return {"food": -actual}
    return {}


def check_night_travel_danger(
    state: RunState, rng: SeededRNG,
) -> dict[str, int] | None:
    """Night travel without lantern oil increases breakdown/injury chance."""
    if state.time_of_day not in (TimeOfDay.EVENING, TimeOfDay.NIGHT):
        return None
    if state.supplies.get("lantern_oil") > 0:
        return None

    # 15% chance of an extra breakdown or injury
    if rng.random() < 0.15:
        damage = rng.randint(5, 15)
        return {"wagon_damage": damage}
    return None


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


# ── Escape valve availability checks ─────────────────────────────────


def can_abandon_cargo(state: RunState) -> bool:
    """Wagon must be in bad shape to justify dumping cargo."""
    return state.wagon.condition < 30 and state.escape_valve_cooldown <= 0


def can_desperate_repair(state: RunState) -> bool:
    """Wagon bad AND no parts — the only option left."""
    return (
        state.wagon.condition < 30
        and state.supplies.parts <= 0
        and state.escape_valve_cooldown <= 0
    )


def can_hard_ration(state: RunState) -> bool:
    """Food critically low AND not already rationing."""
    alive = state.party.alive_count
    return (
        alive > 0
        and state.supplies.food < alive * 3
        and state.rationing_steps <= 0
        and state.escape_valve_cooldown <= 0
    )


# ── Escape valve actions ─────────────────────────────────────────────


def abandon_cargo(state: RunState) -> dict[str, int]:
    """Drop non-essential supplies to lighten the wagon."""
    deltas: dict[str, int] = {}
    droppable = ["salt", "cloth", "rope", "boots"]

    for key in droppable:
        current = state.supplies.get(key)
        if current > 0:
            drop = max(1, current // 2)
            deltas[key] = -drop
            state.supplies.set(key, current - drop)

    # Wagon repair from lightened load
    state.wagon.condition = min(100, state.wagon.condition + 25)

    # Morale hit
    state.party.morale = max(0, state.party.morale - 8)

    return deltas


def desperate_repair(
    state: RunState, rng: SeededRNG,
) -> dict[str, object]:
    """Risky repair without parts. Returns result dict."""
    success_chance = 0.50
    if state.party.has_trait(Trait.MECHANIC):
        success_chance += 0.20
    if state.supplies.get("tools") > 0:
        success_chance += 0.10

    # Consume cloth if available (makeshift patch)
    used_cloth = False
    if state.supplies.get("cloth") > 0:
        state.supplies.set("cloth", state.supplies.get("cloth") - 1)
        used_cloth = True

    if rng.random() < success_chance:
        state.wagon.condition = min(100, state.wagon.condition + 15)
        return {
            "success": True,
            "wagon_delta": 15,
            "cloth_used": used_cloth,
        }

    # Failure: wagon gets worse, someone gets hurt
    state.wagon.condition = max(0, state.wagon.condition - 10)
    alive = [m for m in state.party.members if m.is_alive()]
    injured_name = ""
    if alive:
        unlucky = rng.choice(alive)
        unlucky.health = max(0, unlucky.health - rng.randint(5, 15))
        unlucky.condition = Condition.INJURED
        injured_name = unlucky.name

    return {
        "success": False,
        "wagon_delta": -10,
        "cloth_used": used_cloth,
        "injured": injured_name,
    }


def hard_ration(state: RunState) -> None:
    """Declare hard rations — 2 travel steps at half consumption."""
    state.rationing_steps = 2

    # Immediate costs
    state.party.morale = max(0, state.party.morale - 10)
    for member in state.party.members:
        if member.is_alive():
            member.health = max(0, member.health - 5)


def _proximate_death_cause(member, state: RunState) -> str:
    """Determine what killed a party member based on current state."""
    # Priority order: dehydration > starvation > disease > injury > exhaustion
    if state.supplies.water <= 0:
        return "Dehydration"
    if state.supplies.food <= 0:
        return "Starvation"
    if member.condition == Condition.SICK:
        node = _current_node(state)
        if node and node.biome == Biome.SWAMP:
            return "Disease"
        return "Disease"
    if member.condition == Condition.INJURED:
        return "Injury"
    if member.condition == Condition.EXHAUSTED:
        return "Exhaustion"
    # Fallback — died from accumulated damage
    return "Exposure"


def determine_cause_of_death(state: RunState) -> str:
    """Derive the canonical game-over cause from state.

    Returns a short clinical string like "Starvation" or "Wagon failure".
    """
    # Wagon failure — entire caravan stranded
    if state.wagon.condition <= 0 and state.supplies.parts <= 0:
        return "Wagon failure"

    # All dead — aggregate per-member causes
    dead = [m for m in state.party.members if not m.is_alive()]
    if dead:
        causes = [m.death_cause for m in dead if m.death_cause]
        if causes:
            # Most common cause wins
            from collections import Counter
            most_common = Counter(causes).most_common(1)[0][0]
            return most_common

    # Generic fallback
    return "The trail"


def _current_node(state: RunState) -> object | None:
    """Get the current map node."""
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
